export type ToolId = "claude-code" | "codex";

export interface NormalizedEvent {
  meta: Record<string, unknown>;
  session?: string;
  source: ToolId;
  state: "ERROR" | "IDLE" | "SUCCESS" | "WAITING" | "WORKING";
  timestamp: number;
}

type HookPayload = Record<string, unknown>;

const WAITING_NOTIFICATIONS = new Set([
  "agent_needs_input",
  "elicitation_dialog",
  "elicitation_url_dialog",
  "permission_prompt",
]);
const WORKING_NOTIFICATIONS = new Set([
  "elicitation_complete",
  "elicitation_response",
]);
const WORKING_EVENTS = new Set([
  "ElicitationResult",
  "PermissionDenied",
  "PostToolBatch",
  "PostToolUseFailure",
]);

function asObject(input: unknown): HookPayload | undefined {
  return input !== null && typeof input === "object" && !Array.isArray(input)
    ? (input as HookPayload)
    : undefined;
}

function stringField(payload: HookPayload, ...names: string[]) {
  for (const name of names) {
    const value = payload[name];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

export function translate(tool: ToolId, input: unknown): NormalizedEvent[] {
  const payload = asObject(input);
  if (!payload) {
    return [];
  }

  const hookEvent = stringField(
    payload,
    "hook_event_name",
    "hookEventName",
    "event_name",
    "eventName",
    "type"
  );
  if (!hookEvent) {
    return [];
  }

  // User-level hooks also run inside subagents. AI-Light V1 represents only
  // the main conversation, so nested agent events must not seize the lamp.
  if (stringField(payload, "agent_id", "agentId")) {
    return [];
  }

  const notificationType = stringField(
    payload,
    "notification_type",
    "notificationType"
  );
  const toolName = stringField(payload, "tool_name", "toolName");
  const state = mapState(tool, hookEvent, notificationType, toolName, payload);
  if (!state) {
    return [];
  }

  const session = stringField(payload, "session_id", "sessionId", "thread_id");
  return [
    {
      meta: {
        hookEvent,
        ...(notificationType ? { notificationType } : {}),
        ...(toolName ? { toolName } : {}),
      },
      ...(session ? { session } : {}),
      source: tool,
      state,
      timestamp: Date.now(),
    },
  ];
}

function mapState(
  tool: ToolId,
  hookEvent: string,
  notificationType: string | undefined,
  toolName: string | undefined,
  payload: HookPayload
): NormalizedEvent["state"] | undefined {
  if (tool === "claude-code") {
    return mapClaudeState(hookEvent, notificationType, toolName, payload);
  }
  if (hookEvent === "UserPromptSubmit") {
    return "WORKING";
  }
  if (hookEvent === "PermissionRequest") {
    return "WAITING";
  }
  if (hookEvent === "Stop") {
    return "SUCCESS";
  }
  if (hookEvent === "SessionEnd") {
    return "IDLE";
  }
  return undefined;
}

function mapClaudeState(
  hookEvent: string,
  notificationType: string | undefined,
  toolName: string | undefined,
  payload: HookPayload
): NormalizedEvent["state"] | undefined {
  if (hookEvent === "SessionStart" || hookEvent === "SessionEnd") {
    return "IDLE";
  }
  if (hookEvent === "StopFailure") {
    return "ERROR";
  }
  if (hookEvent === "PermissionRequest" || hookEvent === "Elicitation") {
    return "WAITING";
  }
  if (
    hookEvent === "PreToolUse" &&
    (toolName === "AskUserQuestion" || toolName === "ExitPlanMode")
  ) {
    return "WAITING";
  }
  if (hookEvent === "Stop") {
    return Array.isArray(payload.background_tasks) &&
      payload.background_tasks.length > 0
      ? "WORKING"
      : "SUCCESS";
  }
  if (hookEvent === "UserPromptSubmit" || WORKING_EVENTS.has(hookEvent)) {
    return "WORKING";
  }
  if (hookEvent !== "Notification" || !notificationType) {
    return undefined;
  }
  if (WAITING_NOTIFICATIONS.has(notificationType)) {
    return "WAITING";
  }
  return WORKING_NOTIFICATIONS.has(notificationType) ? "WORKING" : undefined;
}

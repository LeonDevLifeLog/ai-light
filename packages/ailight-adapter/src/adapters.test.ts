import assert from "node:assert/strict";
import test from "node:test";
import { translate } from "./adapters.js";

test("maps Claude Code lifecycle events", () => {
  assert.equal(
    translate("claude-code", {
      hook_event_name: "UserPromptSubmit",
      session_id: "claude-1",
    })[0]?.state,
    "WORKING"
  );
  assert.equal(
    translate("claude-code", {
      hook_event_name: "Notification",
      notification_type: "permission_prompt",
    })[0]?.state,
    "WAITING"
  );
  assert.equal(
    translate("claude-code", { hook_event_name: "PostToolBatch" })[0]?.state,
    "WORKING"
  );
  assert.equal(
    translate("claude-code", { hook_event_name: "StopFailure" })[0]?.state,
    "ERROR"
  );
});

test("maps Claude attention and recovery boundaries", () => {
  assert.equal(
    translate("claude-code", {
      hook_event_name: "PreToolUse",
      tool_name: "AskUserQuestion",
    })[0]?.state,
    "WAITING"
  );
  assert.equal(
    translate("claude-code", { hook_event_name: "Elicitation" })[0]?.state,
    "WAITING"
  );
  assert.equal(
    translate("claude-code", { hook_event_name: "ElicitationResult" })[0]
      ?.state,
    "WORKING"
  );
  assert.equal(
    translate("claude-code", {
      background_tasks: [{ status: "running" }],
      hook_event_name: "Stop",
    })[0]?.state,
    "WORKING"
  );
});

test("ignores nested agent events and non-boundary hooks", () => {
  assert.deepEqual(
    translate("claude-code", {
      agent_id: "agent-1",
      hook_event_name: "PermissionRequest",
    }),
    []
  );
  assert.deepEqual(
    translate("claude-code", {
      hook_event_name: "PreToolUse",
      tool_name: "Read",
    }),
    []
  );
  assert.deepEqual(
    translate("claude-code", {
      hook_event_name: "Notification",
      notification_type: "idle_prompt",
    }),
    []
  );
});

test("maps Codex lifecycle events and ignores unknown events", () => {
  assert.equal(
    translate("codex", { hook_event_name: "PermissionRequest" })[0]?.state,
    "WAITING"
  );
  assert.equal(
    translate("codex", { hook_event_name: "Stop" })[0]?.state,
    "SUCCESS"
  );
  assert.deepEqual(translate("codex", { hook_event_name: "PostToolUse" }), []);
});

test("maps Qoder lifecycle, attention, recovery, and failure events", () => {
  const cases = [
    ["SessionStart", "IDLE"],
    ["UserPromptSubmit", "WORKING"],
    ["PermissionRequest", "WAITING"],
    ["PermissionDenied", "WORKING"],
    ["Elicitation", "WAITING"],
    ["ElicitationResult", "WORKING"],
    ["PostToolUseFailure", "WORKING"],
    ["Stop", "SUCCESS"],
    ["StopFailure", "ERROR"],
    ["SessionEnd", "IDLE"],
  ] as const;
  for (const [hookEvent, state] of cases) {
    assert.equal(
      translate("qoder", {
        hook_event_name: hookEvent,
        session_id: "qoder-1",
      })[0]?.state,
      state
    );
  }
  assert.deepEqual(
    translate("qoder", {
      agent_id: "subagent-1",
      hook_event_name: "PermissionRequest",
    }),
    []
  );
  assert.deepEqual(translate("qoder", { hook_event_name: "FileChanged" }), []);
});

test("maps WorkBuddy supported lifecycle events", () => {
  assert.equal(
    translate("workbuddy", { hook_event_name: "SessionStart" })[0]?.state,
    "IDLE"
  );
  assert.equal(
    translate("workbuddy", { hook_event_name: "UserPromptSubmit" })[0]?.state,
    "WORKING"
  );
  assert.equal(
    translate("workbuddy", {
      hook_event_name: "PreToolUse",
      tool_name: "AskUserQuestion",
    })[0]?.state,
    "WAITING"
  );
  assert.equal(
    translate("workbuddy", { hook_event_name: "Stop" })[0]?.state,
    "SUCCESS"
  );
  assert.equal(
    translate("workbuddy", { hook_event_name: "SessionEnd" })[0]?.state,
    "IDLE"
  );
  assert.deepEqual(
    translate("workbuddy", {
      hook_event_name: "PreToolUse",
      tool_name: "Read",
    }),
    []
  );
  assert.deepEqual(
    translate("workbuddy", { hook_event_name: "PostToolUse" }),
    []
  );
});

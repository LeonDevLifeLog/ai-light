import { createHash } from "node:crypto";
import { access, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import type { NormalizedEvent } from "./adapters.js";

interface RuntimeDescriptor {
  authToken?: string;
  protocol: { max: number; min: number };
  transport: { host: string; port: number; type: "http" };
}

export function aiLightHome(env = process.env) {
  const override = env.AILIGHT_HOME?.trim();
  return override ? resolve(override) : join(homedir(), ".ailight");
}

export async function shouldSuppressCodexHook(input: unknown) {
  if (input === null || typeof input !== "object") {
    return false;
  }
  const { hook_event_name, session_id, source, transcript_path } =
    input as Record<string, unknown>;
  if (
    transcript_path !== null ||
    typeof session_id !== "string" ||
    !session_id
  ) {
    return false;
  }

  // thread/sample inherits its parent's hooks and may use a cached config.
  // Only directly started or resumed sessions can opt in without a transcript.
  const directory = join(aiLightHome(), "codex-ephemeral-sessions");
  const marker = join(
    directory,
    createHash("sha256").update(session_id).digest("hex")
  );
  if (hook_event_name === "SessionStart") {
    if (source === "startup" || source === "resume") {
      await mkdir(directory, { mode: 0o700, recursive: true });
      await writeFile(marker, "", { mode: 0o600 });
    }
    return true;
  }
  try {
    await access(marker);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return true;
    }
    throw error;
  }
  if (hook_event_name === "SessionEnd") {
    await rm(marker, { force: true });
  }
  return false;
}

export async function readRuntime(): Promise<RuntimeDescriptor> {
  const content = await readFile(join(aiLightHome(), "runtime.json"), "utf8");
  const parsed = JSON.parse(content) as Partial<RuntimeDescriptor>;
  const transport = parsed.transport;
  if (
    parsed.protocol?.min !== 1 ||
    (parsed.protocol.max ?? 0) < 1 ||
    transport?.type !== "http" ||
    transport.host !== "127.0.0.1" ||
    !Number.isInteger(transport.port) ||
    transport.port < 1 ||
    transport.port > 65_535
  ) {
    throw new Error("RUNTIME_INVALID");
  }
  return parsed as RuntimeDescriptor;
}

export async function deliver(event: NormalizedEvent) {
  const runtime = await readRuntime();
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 300);
  try {
    const response = await fetch(
      `http://${runtime.transport.host}:${runtime.transport.port}/hook`,
      {
        body: JSON.stringify({
          event: "state_change",
          meta: {
            ...event.meta,
            adapter: {
              name: "@ai-light/adapter",
              protocolVersion: 1,
            },
          },
          session: event.session,
          source: event.source,
          state: event.state,
          ts: event.timestamp,
        }),
        headers: {
          ...(runtime.authToken
            ? { Authorization: `Bearer ${runtime.authToken}` }
            : {}),
          "Content-Type": "application/json",
        },
        method: "POST",
        signal: controller.signal,
      }
    );
    if (!response.ok) {
      throw new Error(`SERVICE_HTTP_${response.status}`);
    }
  } finally {
    clearTimeout(timeout);
  }
}

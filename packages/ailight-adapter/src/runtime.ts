import { readFile } from "node:fs/promises";
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

#!/usr/bin/env node

import { fileURLToPath } from "node:url";
import { type ToolId, translate } from "./adapters.js";
import { detect, install, uninstall } from "./integrations.js";
import { aiLightHome, deliver, readRuntime } from "./runtime.js";

function isTool(value: string | undefined): value is ToolId {
  return value === "claude-code" || value === "codex";
}

async function stdinJson() {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of process.stdin) {
    const buffer = Buffer.from(chunk);
    size += buffer.length;
    if (size > 1024 * 1024) {
      throw new Error("INPUT_TOO_LARGE");
    }
    chunks.push(buffer);
  }
  return JSON.parse(Buffer.concat(chunks).toString("utf8")) as unknown;
}

function output(data: unknown) {
  process.stdout.write(`${JSON.stringify({ data, ok: true })}\n`);
}

async function main() {
  const [command, toolArg, ...options] = process.argv.slice(2);
  const dryRun = options.includes("--dry-run");
  if (command === "version") {
    output({ name: "@ai-light/adapter", version: "0.1.1" });
    return;
  }
  if (command === "doctor") {
    const runtime = await readRuntime();
    output({ home: aiLightHome(), node: process.execPath, runtime });
    return;
  }
  if (!isTool(toolArg)) {
    throw new Error("TOOL_NOT_SUPPORTED");
  }
  if (command === "hook" || command === "translate") {
    const events = translate(toolArg, await stdinJson());
    if (command === "translate") {
      output(events);
      return;
    }
    for (const event of events) {
      await deliver(event);
    }
    return;
  }
  if (command === "detect") {
    output(await detect(toolArg));
    return;
  }
  const cliScript = fileURLToPath(import.meta.url);
  if (command === "install" || command === "repair") {
    output(await install(toolArg, cliScript, dryRun));
    return;
  }
  if (command === "uninstall") {
    output(await uninstall(toolArg, dryRun));
    return;
  }
  throw new Error("COMMAND_NOT_SUPPORTED");
}

main().catch((error: unknown) => {
  if (process.argv[2] === "hook") {
    process.exitCode = 0;
    return;
  }
  const message = error instanceof Error ? error.message : "INTERNAL_ERROR";
  process.stdout.write(
    `${JSON.stringify({ error: { code: message, message }, ok: false })}\n`
  );
  process.exitCode = 1;
});

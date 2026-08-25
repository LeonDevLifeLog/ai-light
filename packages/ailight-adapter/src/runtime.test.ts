import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { deliver } from "./runtime.js";

test("delivers a normalized event using the runtime descriptor", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-runtime-"));
  const previousHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_HOME = root;
  let received = "";
  const server = createServer((request, response) => {
    request.on("data", (chunk) => {
      received += chunk.toString();
    });
    request.on("end", () => {
      response.writeHead(200, { "Content-Type": "application/json" });
      response.end('{"ok":true,"applied":true}');
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address();
    assert.notEqual(address, null);
    assert.equal(typeof address, "object");
    const port = typeof address === "object" && address ? address.port : 0;
    await mkdir(root, { recursive: true });
    await writeFile(
      join(root, "runtime.json"),
      JSON.stringify({
        authToken: "secret",
        protocol: { max: 1, min: 1 },
        transport: { host: "127.0.0.1", port, type: "http" },
      })
    );
    await deliver({
      meta: { hookEvent: "Stop" },
      session: "session-1",
      source: "codex",
      state: "SUCCESS",
      timestamp: 123,
    });
    const payload = JSON.parse(received) as Record<string, unknown>;
    assert.equal(payload.source, "codex");
    assert.equal(payload.state, "SUCCESS");
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    );
    process.env.AILIGHT_HOME = previousHome;
    await rm(root, { force: true, recursive: true });
  }
});

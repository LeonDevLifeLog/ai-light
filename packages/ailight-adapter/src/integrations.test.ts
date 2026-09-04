import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { configPath, detect, install, uninstall } from "./integrations.js";

const POSIX_SCRIPT_PATTERN = /'\/adapter path\/cli\.js'/;
const MANAGED_MARKER_PATTERN = /--managed-by.*ai-light/;
const WINDOWS_SCRIPT_PATTERN = /"\/adapter path\/cli\.js"/;
const WORKBUDDY_PATTERN = /workbuddy/;

test("installs idempotently and preserves user hooks", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const path = configPath("claude-code");
    await mkdir(join(root, ".claude"), { recursive: true });
    await writeFile(
      path,
      JSON.stringify({
        hooks: { Stop: [{ hooks: [{ command: "user", type: "command" }] }] },
      })
    );
    await install("claude-code", "/adapter/cli.js", false);
    await install("claude-code", "/adapter/cli.js", false);
    assert.equal((await detect("claude-code")).connected, true);
    let config = JSON.parse(await readFile(path, "utf8")) as {
      hooks: Record<
        string,
        Array<{ hooks: Array<{ command: string }>; matcher?: string }>
      >;
    };
    assert.equal(config.hooks.Stop?.flatMap((group) => group.hooks).length, 2);
    assert.equal(config.hooks.PostToolBatch?.length, 1);
    assert.equal(config.hooks.PostToolUse, undefined);
    assert.equal(
      config.hooks.PreToolUse?.[0]?.matcher,
      "AskUserQuestion|ExitPlanMode"
    );
    await uninstall("claude-code", false);
    config = JSON.parse(await readFile(path, "utf8")) as typeof config;
    assert.equal(config.hooks.Stop?.[0]?.hooks[0]?.command, "user");
  } finally {
    if (previousHome === undefined) {
      process.env.AILIGHT_TEST_USER_HOME = undefined;
    } else {
      process.env.AILIGHT_TEST_USER_HOME = previousHome;
    }
    if (previousAiLightHome === undefined) {
      process.env.AILIGHT_HOME = undefined;
    } else {
      process.env.AILIGHT_HOME = previousAiLightHome;
    }
    await rm(root, { force: true, recursive: true });
  }
});

test("writes Codex command hooks using the official string command shape", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-codex-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    await install("codex", "/adapter path/cli.js", false);
    const path = configPath("codex");
    const config = JSON.parse(await readFile(path, "utf8")) as {
      hooks: Record<
        string,
        Array<{
          hooks: Array<{
            args?: string[];
            command: string;
            commandWindows?: string;
          }>;
        }>
      >;
    };
    const handler = config.hooks.Stop?.[0]?.hooks[0];
    assert.equal(handler?.args, undefined);
    assert.match(handler?.command ?? "", POSIX_SCRIPT_PATTERN);
    assert.match(handler?.command ?? "", MANAGED_MARKER_PATTERN);
    assert.match(handler?.commandWindows ?? "", WINDOWS_SCRIPT_PATTERN);
    assert.equal((await detect("codex")).connected, true);
    await uninstall("codex", false);
    assert.equal((await detect("codex")).managedCount, 0);
  } finally {
    if (previousHome === undefined) {
      process.env.AILIGHT_TEST_USER_HOME = undefined;
    } else {
      process.env.AILIGHT_TEST_USER_HOME = previousHome;
    }
    if (previousAiLightHome === undefined) {
      process.env.AILIGHT_HOME = undefined;
    } else {
      process.env.AILIGHT_HOME = previousAiLightHome;
    }
    await rm(root, { force: true, recursive: true });
  }
});

test("manages WorkBuddy hooks in the independent .workbuddy config", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-workbuddy-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const path = configPath("workbuddy");
    assert.equal(path, join(root, ".workbuddy", "settings.json"));
    await mkdir(join(root, ".workbuddy"), { recursive: true });
    await writeFile(
      path,
      JSON.stringify({
        hooks: { Stop: [{ hooks: [{ command: "user", type: "command" }] }] },
      })
    );

    await install("workbuddy", "/adapter path/cli.js", false);
    await install("workbuddy", "/adapter path/cli.js", false);

    const config = JSON.parse(await readFile(path, "utf8")) as {
      hooks: Record<
        string,
        Array<{
          hooks: Array<{ args?: string[]; command: string }>;
          matcher?: string;
        }>
      >;
    };
    assert.equal((await detect("workbuddy")).connected, true);
    assert.equal(config.hooks.Stop?.flatMap((group) => group.hooks).length, 2);
    assert.equal(config.hooks.SessionStart?.[0]?.matcher, "startup");
    assert.equal(config.hooks.SessionEnd?.[0]?.matcher, "other");
    assert.equal(
      config.hooks.PreToolUse?.[0]?.matcher,
      "AskUserQuestion|ExitPlanMode"
    );
    assert.equal(config.hooks.Stop?.[1]?.hooks[0]?.args, undefined);
    assert.match(
      config.hooks.Stop?.[1]?.hooks[0]?.command ?? "",
      WORKBUDDY_PATTERN
    );

    await uninstall("workbuddy", false);
    const clean = JSON.parse(await readFile(path, "utf8")) as typeof config;
    assert.equal(clean.hooks.Stop?.[0]?.hooks[0]?.command, "user");
    assert.equal(clean.hooks.SessionStart, undefined);
  } finally {
    if (previousHome === undefined) {
      process.env.AILIGHT_TEST_USER_HOME = undefined;
    } else {
      process.env.AILIGHT_TEST_USER_HOME = previousHome;
    }
    if (previousAiLightHome === undefined) {
      process.env.AILIGHT_HOME = undefined;
    } else {
      process.env.AILIGHT_HOME = previousAiLightHome;
    }
    await rm(root, { force: true, recursive: true });
  }
});

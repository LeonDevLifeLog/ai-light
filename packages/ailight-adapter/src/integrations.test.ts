import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  configPath,
  configPaths,
  detect,
  install,
  uninstall,
} from "./integrations.js";

const POSIX_SCRIPT_PATTERN = /'\/adapter path\/cli\.js'/;
const MANAGED_MARKER_PATTERN = /--managed-by.*ai-light/;
const WINDOWS_COMMAND_PATTERN =
  /^powershell\.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand [A-Za-z0-9+/=]+$/;
const QODER_PATTERN = /qoder/;
const TRAE_PATTERN = /trae/;
const CONFIG_PARSE_FAILED_PATTERN = /CONFIG_PARSE_FAILED/;
const WORKBUDDY_PATTERN = /workbuddy/;

interface TestHookConfig {
  hooks: Record<
    string,
    Array<{
      hooks: Array<{ args?: string[]; command: string }>;
    }>
  >;
}

function runCommand(
  executable: string,
  args: string[],
  input: string
): Promise<{ stderr: string; stdout: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, args, {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    const stdout: Buffer[] = [];
    const stderr: Buffer[] = [];
    child.stdout.on("data", (chunk: Buffer) => stdout.push(chunk));
    child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk));
    child.on("error", reject);
    child.on("close", (code) => {
      const result = {
        stderr: Buffer.concat(stderr).toString("utf8"),
        stdout: Buffer.concat(stdout).toString("utf8"),
      };
      if (code === 0) {
        resolve(result);
      } else {
        reject(
          new Error(
            `${executable} exited with ${code}: ${result.stderr || result.stdout}`
          )
        );
      }
    });
    child.stdin.end(input);
  });
}

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
            timeout?: number;
          }>;
        }>
      >;
    };
    const handler = config.hooks.Stop?.[0]?.hooks[0];
    assert.equal(handler?.args, undefined);
    assert.match(handler?.command ?? "", POSIX_SCRIPT_PATTERN);
    assert.match(handler?.command ?? "", MANAGED_MARKER_PATTERN);
    assert.match(handler?.commandWindows ?? "", WINDOWS_COMMAND_PATTERN);
    assert.equal(handler?.timeout, 20);
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

test("runs the Codex Windows command through cmd and PowerShell with hostile paths", {
  skip: process.platform !== "win32",
}, async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight adapter & shell-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const scriptDir = join(root, "adapter's files & runtime");
    const cliScript = join(scriptDir, "cli hook.js");
    await mkdir(scriptDir, { recursive: true });
    await writeFile(
      cliScript,
      [
        "const chunks = [];",
        "for await (const chunk of process.stdin) chunks.push(chunk);",
        "process.stdout.write(JSON.stringify({",
        "  args: process.argv.slice(2),",
        "  input: Buffer.concat(chunks).toString('utf8'),",
        "}));",
      ].join("\n")
    );
    await install("codex", cliScript, false);
    const config = JSON.parse(await readFile(configPath("codex"), "utf8")) as {
      hooks: Record<
        string,
        Array<{ hooks: Array<{ commandWindows?: string }> }>
      >;
    };
    const command =
      config.hooks.UserPromptSubmit?.[0]?.hooks[0]?.commandWindows;
    assert.ok(command);
    const input = JSON.stringify({
      hook_event_name: "UserPromptSubmit",
      path: "C:\\work dir\\quoted's & file.txt",
    });
    const shells: [string, string[]][] = [
      ["cmd.exe", ["/d", "/s", "/c", command]],
      [
        "powershell.exe",
        ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command],
      ],
    ];
    if (
      spawnSync(
        "pwsh.exe",
        ["-NoProfile", "-Command", "$PSVersionTable.PSVersion"],
        {
          windowsHide: true,
        }
      ).status === 0
    ) {
      shells.push([
        "pwsh.exe",
        ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", command],
      ]);
    }
    for (const [shell, args] of shells) {
      const result = await runCommand(shell, args, input);
      const output = JSON.parse(result.stdout) as {
        args: string[];
        input: string;
      };
      assert.deepEqual(output.args, [
        "hook",
        "codex",
        "--managed-by",
        "ai-light",
        "--schema",
        "1",
      ]);
      assert.equal(output.input, input);
    }
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

test("manages both existing Qoder config directories", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-qoder-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const paths = [
      join(root, ".qoder", "settings.json"),
      join(root, ".qoder-cn", "settings.json"),
    ];
    for (const path of paths) {
      await mkdir(join(path, ".."), { recursive: true });
      await writeFile(
        path,
        JSON.stringify({
          hooks: {
            Stop: [{ hooks: [{ command: "user", type: "command" }] }],
          },
        })
      );
    }

    await install("qoder", "/adapter path/cli.js", false);
    await install("qoder", "/adapter path/cli.js", false);

    const status = await detect("qoder");
    assert.equal(status.connected, true);
    assert.deepEqual(status.paths, paths);
    for (const path of paths) {
      const config = JSON.parse(await readFile(path, "utf8")) as TestHookConfig;
      assert.equal(
        config.hooks.Stop?.flatMap((group) => group.hooks).length,
        2
      );
      assert.equal(config.hooks.StopFailure?.length, 1);
      assert.equal(config.hooks.Elicitation?.length, 1);
      assert.equal(config.hooks.FileChanged, undefined);
      assert.equal(config.hooks.Stop?.[1]?.hooks[0]?.args, undefined);
      assert.match(
        config.hooks.Stop?.[1]?.hooks[0]?.command ?? "",
        QODER_PATTERN
      );
    }

    await uninstall("qoder", false);
    for (const path of paths) {
      const clean = JSON.parse(await readFile(path, "utf8")) as TestHookConfig;
      assert.equal(clean.hooks.Stop?.[0]?.hooks[0]?.command, "user");
      assert.equal(clean.hooks.StopFailure, undefined);
    }
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

test("selects the existing Qoder variant or defaults to .qoder", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-qoder-paths-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  try {
    assert.deepEqual(await configPaths("qoder"), [
      join(root, ".qoder", "settings.json"),
    ]);
    await mkdir(join(root, ".qoder-cn"), { recursive: true });
    assert.deepEqual(await configPaths("qoder"), [
      join(root, ".qoder-cn", "settings.json"),
    ]);
  } finally {
    if (previousHome === undefined) {
      process.env.AILIGHT_TEST_USER_HOME = undefined;
    } else {
      process.env.AILIGHT_TEST_USER_HOME = previousHome;
    }
    await rm(root, { force: true, recursive: true });
  }
});

test("manages TraeCode hooks in the independent versioned config", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-trae-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const path = configPath("trae");
    assert.equal(path, join(root, ".trae-cn", "hooks.json"));
    await mkdir(join(root, ".trae-cn"), { recursive: true });
    await writeFile(
      path,
      JSON.stringify({
        hooks: { Stop: [{ hooks: [{ command: "user", type: "command" }] }] },
        version: 1,
      })
    );
    await install("trae", "/adapter path/cli.js", false);
    await install("trae", "/adapter path/cli.js", false);

    const config = JSON.parse(await readFile(path, "utf8")) as {
      hooks: Record<
        string,
        Array<{
          hooks: Array<{ command: string; commandWindows?: string }>;
          matcher?: string;
        }>
      >;
      version: number;
    };
    assert.equal(config.version, 1);
    assert.equal((await detect("trae")).connected, true);
    assert.equal(config.hooks.Stop?.flatMap((group) => group.hooks).length, 2);
    assert.equal(config.hooks.PreToolUse?.[0]?.matcher, "AskUserQuestion");
    assert.equal(
      config.hooks.Notification?.[0]?.matcher,
      "idle_prompt|permission_prompt|document_review|ask_user_question|browser_interaction"
    );
    const handler = config.hooks.UserPromptSubmit?.[0]?.hooks[0];
    assert.match(handler?.command ?? "", TRAE_PATTERN);
    assert.equal(handler?.commandWindows, undefined);

    await uninstall("trae", false);
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

test("preserves an unsupported TraeCode schema without writing", async () => {
  const root = await mkdtemp(join(tmpdir(), "ailight-adapter-trae-schema-"));
  const previousHome = process.env.AILIGHT_TEST_USER_HOME;
  const previousAiLightHome = process.env.AILIGHT_HOME;
  process.env.AILIGHT_TEST_USER_HOME = root;
  process.env.AILIGHT_HOME = join(root, ".ailight");
  try {
    const path = configPath("trae");
    await mkdir(join(root, ".trae-cn"), { recursive: true });
    const original = `${JSON.stringify({ hooks: {}, version: 2 })}\n`;
    await writeFile(path, original);
    await assert.rejects(
      install("trae", "/adapter/cli.js", false),
      CONFIG_PARSE_FAILED_PATTERN
    );
    assert.equal(await readFile(path, "utf8"), original);
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

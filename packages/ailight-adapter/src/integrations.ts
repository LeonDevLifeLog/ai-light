import {
  copyFile,
  mkdir,
  readFile,
  rename,
  stat,
  writeFile,
} from "node:fs/promises";
import { homedir } from "node:os";
import { basename, dirname, join } from "node:path";
import type { ToolId } from "./adapters.js";
import { aiLightHome } from "./runtime.js";

interface HookHandler {
  args?: string[];
  command: string;
  commandWindows?: string;
  timeout?: number;
  type: "command";
}

interface HookGroup {
  hooks: HookHandler[];
  matcher?: string;
}

interface ToolConfig {
  hooks?: Record<string, HookGroup[]>;
  [key: string]: unknown;
}

const MANAGED_ARGS = ["--managed-by", "ai-light", "--schema", "1"];

const EVENTS: Record<ToolId, Array<{ event: string; matcher?: string }>> = {
  "claude-code": [
    { event: "SessionStart" },
    { event: "UserPromptSubmit" },
    { event: "PreToolUse", matcher: "AskUserQuestion|ExitPlanMode" },
    { event: "PermissionRequest" },
    { event: "PermissionDenied" },
    { event: "Elicitation" },
    { event: "ElicitationResult" },
    {
      event: "Notification",
      matcher:
        "permission_prompt|elicitation_dialog|elicitation_url_dialog|elicitation_complete|elicitation_response|agent_needs_input",
    },
    { event: "PostToolBatch" },
    { event: "PostToolUseFailure" },
    { event: "Stop" },
    { event: "StopFailure" },
    { event: "SessionEnd" },
  ],
  codex: [
    { event: "UserPromptSubmit" },
    { event: "PermissionRequest" },
    { event: "Stop" },
    { event: "SessionEnd" },
  ],
  qoder: [
    { event: "SessionStart" },
    { event: "UserPromptSubmit" },
    { event: "PermissionRequest" },
    { event: "PermissionDenied" },
    { event: "Elicitation" },
    { event: "ElicitationResult" },
    { event: "PostToolUseFailure" },
    { event: "Stop" },
    { event: "StopFailure" },
    { event: "SessionEnd" },
  ],
  workbuddy: [
    { event: "SessionStart", matcher: "startup" },
    { event: "UserPromptSubmit" },
    { event: "PreToolUse", matcher: "AskUserQuestion|ExitPlanMode" },
    { event: "Stop" },
    { event: "SessionEnd", matcher: "other" },
  ],
};

export function configPath(tool: ToolId, env = process.env) {
  const userHome = env.AILIGHT_TEST_USER_HOME || homedir();
  if (tool === "claude-code") {
    return join(userHome, ".claude", "settings.json");
  }
  if (tool === "workbuddy") {
    return join(userHome, ".workbuddy", "settings.json");
  }
  if (tool === "qoder") {
    return join(userHome, ".qoder", "settings.json");
  }
  return join(userHome, ".codex", "hooks.json");
}

async function directoryExists(path: string) {
  try {
    return (await stat(path)).isDirectory();
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

export async function configPaths(tool: ToolId, env = process.env) {
  if (tool !== "qoder") {
    return [configPath(tool, env)];
  }
  const userHome = env.AILIGHT_TEST_USER_HOME || homedir();
  const defaultDirectory = join(userHome, ".qoder");
  const directories = [defaultDirectory, join(userHome, ".qoder-cn")];
  const existing = (
    await Promise.all(
      directories.map(async (directory) => ({
        directory,
        exists: await directoryExists(directory),
      }))
    )
  ).filter((item) => item.exists);
  const targets =
    existing.length > 0 ? existing : [{ directory: defaultDirectory }];
  return targets.map((item) => join(item.directory, "settings.json"));
}

function isManaged(handler: HookHandler) {
  if (handler.type !== "command") {
    return false;
  }
  if (Array.isArray(handler.args)) {
    return MANAGED_ARGS.every((part) => handler.args?.includes(part));
  }
  return MANAGED_ARGS.every((part) => handler.command.includes(part));
}

function quotePosix(value: string) {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}

function quotePowerShell(value: string) {
  return `'${value.replaceAll("'", "''")}'`;
}

function windowsPowerShellCommand(executable: string, args: string[]) {
  const script = `& ${[executable, ...args].map(quotePowerShell).join(" ")}`;
  const encoded = Buffer.from(script, "utf16le").toString("base64");
  return [
    "powershell.exe",
    "-NoLogo",
    "-NoProfile",
    "-NonInteractive",
    "-ExecutionPolicy",
    "Bypass",
    "-EncodedCommand",
    encoded,
  ].join(" ");
}

async function loadConfig(path: string): Promise<ToolConfig> {
  try {
    const value = JSON.parse(await readFile(path, "utf8")) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new Error("CONFIG_PARSE_FAILED");
    }
    return value as ToolConfig;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return {};
    }
    throw new Error("CONFIG_PARSE_FAILED", { cause: error });
  }
}

function withoutManaged(config: ToolConfig) {
  const hooks: Record<string, HookGroup[]> = {};
  for (const [event, groups] of Object.entries(config.hooks ?? {})) {
    const kept = groups
      .map((group) => ({
        ...group,
        hooks: group.hooks.filter((handler) => !isManaged(handler)),
      }))
      .filter((group) => group.hooks.length > 0);
    if (kept.length > 0) {
      hooks[event] = kept;
    }
  }
  return { ...config, hooks };
}

function withManaged(config: ToolConfig, tool: ToolId, cliScript: string) {
  const clean = withoutManaged(config);
  const hooks = { ...(clean.hooks ?? {}) };
  for (const item of EVENTS[tool]) {
    const invocation = [cliScript, "hook", tool, ...MANAGED_ARGS];
    const handler: HookHandler =
      tool === "claude-code"
        ? {
            args: invocation,
            command: process.execPath,
            timeout: 2,
            type: "command",
          }
        : {
            command: [process.execPath, ...invocation]
              .map(quotePosix)
              .join(" "),
            // Codex may launch commandWindows through cmd.exe or PowerShell.
            // An explicitly encoded PowerShell command avoids ambiguous nested
            // quoting when Node or the adapter path contains spaces/metacharacters.
            commandWindows: windowsPowerShellCommand(
              process.execPath,
              invocation
            ),
            timeout: 20,
            type: "command",
          };
    hooks[item.event] = [
      ...(hooks[item.event] ?? []),
      {
        hooks: [handler],
        ...(item.matcher ? { matcher: item.matcher } : {}),
      },
    ];
  }
  return { ...clean, hooks };
}

async function persist(path: string, config: ToolConfig, backup: boolean) {
  await mkdir(dirname(path), { recursive: true });
  if (backup) {
    const backupDir = join(aiLightHome(), "backups", basename(dirname(path)));
    await mkdir(backupDir, { recursive: true });
    try {
      await copyFile(path, join(backupDir, `${Date.now()}-${basename(path)}`));
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
        throw new Error("BACKUP_FAILED", { cause: error });
      }
    }
  }
  const temporary = `${path}.${process.pid}.tmp`;
  await writeFile(temporary, `${JSON.stringify(config, null, 2)}\n`, {
    mode: 0o600,
  });
  await rename(temporary, path);
}

export async function detect(tool: ToolId) {
  const paths = await configPaths(tool);
  const configs = await Promise.all(paths.map(loadConfig));
  const managedCount = configs.reduce(
    (total, config) =>
      total +
      Object.values(config.hooks ?? {})
        .flat()
        .flatMap((group) => group.hooks)
        .filter(isManaged).length,
    0
  );
  return {
    connected: managedCount === EVENTS[tool].length * paths.length,
    managedCount,
    path: paths[0],
    paths,
  };
}

export async function install(
  tool: ToolId,
  cliScript: string,
  dryRun: boolean
) {
  const paths = await configPaths(tool);
  const current = await Promise.all(paths.map(loadConfig));
  const next = current.map((config) => withManaged(config, tool, cliScript));
  if (!dryRun) {
    for (const [index, path] of paths.entries()) {
      await persist(path, next[index] as ToolConfig, true);
    }
  }
  return {
    changed: JSON.stringify(current) !== JSON.stringify(next),
    path: paths[0],
    paths,
  };
}

export async function uninstall(tool: ToolId, dryRun: boolean) {
  const paths = await configPaths(tool);
  const current = await Promise.all(paths.map(loadConfig));
  const next = current.map(withoutManaged);
  if (!dryRun) {
    for (const [index, path] of paths.entries()) {
      await persist(path, next[index] as ToolConfig, true);
    }
  }
  return {
    changed: JSON.stringify(current) !== JSON.stringify(next),
    path: paths[0],
    paths,
  };
}

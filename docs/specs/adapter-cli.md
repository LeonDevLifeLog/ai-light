# AI-Light Adapter CLI 设计规范

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.2 |
| 文档状态 | Claude Code / Codex / WorkBuddy 已实现；Qoder 自 Adapter 0.1.5、TraeCode 自 Adapter 0.1.6 起支持，待实机验收 |
| 适用范围 | Claude Code、Codex、Qoder、TraeCode、WorkBuddy |
| CLI 技术栈 | Node.js 20+ / TypeScript / ESM |
| npm 包 | `@ai-light/adapter` |
| CLI 命令 | `ailight-adapter` |
| 上游契约 | `hook-api.md` V1.0、ADR-0001、`product-boundary.md` |
| 共享数据目录 | `~/.ailight/`（可由 `AILIGHT_HOME` 覆盖） |

> 本文定义 AI-Light 官方 Adapter CLI 的产品边界、进程模型、命令契约、共享目录、事件转换、客户端 Hook 安装、升级与验收要求。本文不定义灯效、主题或 BLE 协议；Adapter 只把外部工具事件归一化为 AI-Light Hook API 事件。

---

## 1. 背景与目标

AI-Light 已提供本地 Hook Server、标准五态、状态仲裁、主题编译与 BLE 下发能力，但 Claude Code、Codex、Qoder、TraeCode、WorkBuddy 的原始 Hook JSON 与 AI-Light `POST /hook` 请求模型不同，不能仅靠把 Hook URL 指向 AI-Light 完成可靠接入。

Adapter CLI 是工具协议与 AI-Light 稳定协议之间的防腐层：

```text
频繁变化的 AI 工具 Hook schema
              │ stdin JSON
              ▼
      AI-Light Adapter CLI
              │ NormalizedEvent
              ▼
      AI-Light Hook API V1
              │
              ▼
     仲裁 → 主题 → SCENE → BLE
```

### 1.1 目标

1. 完成 Claude Code、Codex、Qoder 与 WorkBuddy 的真实接入闭环。
2. 对最终用户隐藏 JSON、HTTP 地址、端口和 Hook 细节。
3. Adapter 可独立构建、测试、发布和升级，不要求同步发布桌面端。
4. 桌面端、CLI 和未来 Skill 复用同一套安装、诊断和修复能力。
5. 外部工具 schema 变化不进入 `ailight-core`。
6. Adapter 失败不得阻塞或改变 AI 工具的正常行为。

### 1.2 非目标

V1 不包含：

- Cursor 或其他工具的正式适配。
- Claude Desktop 纯聊天模式。
- Codex 云端任务的远程状态同步。
- Adapter 常驻 daemon。
- 动态插件 ABI 或在 AI-Light 进程内加载第三方代码。
- Adapter 直接控制 BLE、SCENE 或主题。
- Mac App Store / macOS App Sandbox 分发。
- Hook 进程在执行期间自更新。

---

## 2. 核心设计原则

### 2.1 机制与策略分离

- AI 工具原始事件到标准状态的映射属于 Adapter。
- 状态仲裁、终态驻留和主题映射属于 AI-Light Core。
- Adapter MUST NOT 生成 BLE 帧或读取主题文件。
- Adapter MUST 通过公开 Hook API 上报标准事件。

### 2.2 单一实现，多入口复用

Hook 配置的检测、安装、备份、修复和卸载 MUST 在 CLI 中实现。桌面端和未来 Skill 只调用 CLI，不得各自维护另一套配置合并逻辑。

```text
AI-Light UI ─────┐
                 ├─> ailight-adapter install/doctor/uninstall
未来 Skill ─────┘
```

### 2.3 失败开放

灯效是辅助能力。AI-Light 未启动、runtime 文件缺失、请求超时、未知事件或 Adapter 内部错误均不得中断 Claude Code/Codex/Qoder/WorkBuddy。Hook 处理命令在这些情况下 SHOULD 记录脱敏诊断并以退出码 `0` 结束。

### 2.4 用户无端口心智

端口是内部传输细节：

- 设置页 V1 MUST NOT 提供端口编辑。
- 普通接入界面 MUST NOT 展示端口或 Hook URL。
- Hook 配置 MUST NOT 固化 AI-Light 端口。
- CLI MUST 从 `~/.ailight/runtime.json` 发现实际传输地址。

---

## 3. 总体架构

```text
┌──────────────────────────────────────────────────────────────┐
│ Claude Code / Codex / Qoder / TraeCode / WorkBuddy          │
│ lifecycle hook → 启动 ailight-adapter → stdin 原始 JSON      │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────┐
│ @ai-light/adapter                                            │
│                                                              │
│ CLI Router                                                   │
│ ├─ hook claude-code                                          │
│ ├─ hook codex                                                │
│ ├─ hook workbuddy                                            │
│ ├─ install / uninstall / repair                              │
│ └─ doctor / version / translate                              │
│                                                              │
│ Adapter Registry                                             │
│ ├─ ClaudeCodeAdapter                                         │
│ ├─ CodexAdapter                                              │
│ ├─ QoderAdapter                                              │
│ └─ WorkBuddyAdapter                                          │
│                                                              │
│ Runtime Client                                               │
│ ├─ 读取 ~/.ailight/runtime.json                              │
│ ├─ 校验 owner / 权限 / PID / loopback                        │
│ └─ POST /hook                                                │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────┐
│ AI-Light Desktop / ailight-core                              │
│ Hook Server → Arbiter → Theme → Transport → BLE              │
└──────────────────────────────────────────────────────────────┘
```

### 3.1 进程模型

`ailight-adapter hook <tool>` 是短生命周期、无状态进程：

1. 从 stdin 读取一个 JSON 对象。
2. 根据 `<tool>` 选择 Adapter。
3. 解析为零到多个标准事件。
4. 读取 runtime descriptor。
5. 向 AI-Light 上报。
6. 快速退出。

V1 MUST NOT 启动后台 daemon、监听网络端口或轮询 AI 工具。

### 3.2 推荐仓库结构

```text
packages/ailight-adapter/
├── package.json
├── tsconfig.json
├── src/
│   ├── cli.ts
│   ├── adapters/
│   │   ├── adapter.ts
│   │   ├── claude-code.ts
│   │   └── codex.ts
│   ├── commands/
│   │   ├── hook.ts
│   │   ├── install.ts
│   │   ├── uninstall.ts
│   │   ├── repair.ts
│   │   ├── doctor.ts
│   │   ├── translate.ts
│   │   └── version.ts
│   ├── config/
│   │   ├── claude-code.ts
│   │   ├── codex.ts
│   │   └── managed-entry.ts
│   ├── runtime/
│   │   ├── home.ts
│   │   ├── descriptor.ts
│   │   └── client.ts
│   ├── protocol/
│   │   └── normalized-event.ts
│   └── diagnostics/
│       ├── logger.ts
│       └── redact.ts
├── test/
│   ├── fixtures/
│   │   ├── claude-code/
│   │   └── codex/
│   └── integration/
└── dist/
```

---

## 4. npm 包与运行时

### 4.1 包定义

```json
{
  "name": "@ai-light/adapter",
  "version": "0.1.1",
  "type": "module",
  "bin": {
    "ailight-adapter": "./dist/cli.js"
  },
  "engines": {
    "node": ">=20"
  },
  "files": ["dist"]
}
```

实现要求：

- MUST 使用 TypeScript 和 ESM。
- MUST 支持 Node.js 20 或更高版本。
- SHOULD 优先使用 Node 原生 `fetch`、`fs`、`path`、`os`、`crypto` 和 `util.parseArgs`。
- 运行时依赖 SHOULD 保持为零；若引入依赖，必须说明必要性并纳入供应链审计。
- 开发与构建使用 pnpm；公开分发使用 npm registry。

### 4.2 安装

底层安装命令：

```bash
npm install --global @ai-light/adapter
```

该命令是分发实现，不是普通用户的主交互。AI-Light 接入页 SHOULD 提供“一键连接”，负责检测 Node、npm、Adapter 和目标客户端；自动安装不可用时才展示可复制命令。

### 4.3 CLI 程序位置

CLI 程序本体由 npm global 目录管理，不复制到 `~/.ailight/bin`。共享目录只保存配置、运行时信息、备份和诊断状态，避免产生两个可执行文件版本来源。

注入 Hook 时 MUST 解析并写入实际可执行入口，不得假设 GUI/IDE 启动环境具有与用户终端相同的 `PATH`。

---

## 5. 共享目录规范

### 5.1 路径解析

Desktop 与 CLI 的共享事实源为：

```text
~/.ailight/
```

允许通过环境变量覆盖：

```text
AILIGHT_HOME=/absolute/path
```

解析优先级：

```text
AILIGHT_HOME > 当前真实用户家目录/.ailight
```

Node 侧使用 `os.homedir()`；Rust 侧 MUST 使用系统用户目录解析能力。不得把进程当前工作目录作为回退。

### 5.2 目录结构

```text
~/.ailight/
├── config.json
├── runtime.json
├── integrations.json
├── themes/
├── adapter/
│   ├── config.json
│   └── state.json
├── backups/
│   ├── claude-code/
│   └── codex/
└── logs/
    ├── desktop/
    └── adapter/
```

| 路径 | 主要写入方 | 用途 |
|---|---|---|
| `config.json` | Desktop | AI-Light 持久化设置 |
| `runtime.json` | Desktop | Adapter 运行时服务发现 |
| `integrations.json` | CLI | 已管理集成清单与配置指纹 |
| `themes/` | Desktop | 用户主题 |
| `adapter/config.json` | CLI | CLI 自身非敏感设置 |
| `adapter/state.json` | CLI | 版本、最近诊断与升级状态 |
| `backups/` | CLI | 修改外部配置前的可恢复备份 |
| `logs/` | Desktop/CLI | 本地脱敏日志 |

### 5.3 权限与写入

- `~/.ailight` 在 macOS/Linux MUST 使用 `0700`。
- `runtime.json`、`config.json`、`integrations.json` 和 `adapter/*` MUST 使用 `0600`。
- Windows MUST 尽可能限制为当前用户访问。
- 配置和 runtime 文件 MUST 使用同目录临时文件加原子 rename。
- CLI MUST 拒绝使用属于其他用户或权限明显过宽的敏感文件，并在 `doctor` 中报告修复建议。

### 5.4 macOS 分发边界

macOS Desktop V1 采用非 App Sandbox 的官网/GitHub 安装包分发，可使用 Developer ID 签名、公证和 Hardened Runtime，但不启用 `com.apple.security.app-sandbox`。

启用 App Sandbox 后，桌面端不能假设可以直接访问真实用户家目录，npm CLI 也不能作为具有相同 App Group entitlement 的受信 Helper。因此 Mac App Store 分发不属于 V1；未来若必须支持，应另行设计签名 Helper/XPC 通道，不能沿用“npm CLI 直接共享沙箱容器”的假设。

### 5.5 现有配置迁移

从 Tauri app config dir 迁移到 `~/.ailight` 时：

1. 仅在新目录尚无有效配置时迁移。
2. 先复制、校验，再切换事实源。
3. 首个迁移版本保留旧目录用于回滚，不立即删除。
4. 新旧目录同时存在时不得静默互相覆盖。
5. 写入迁移版本，保证过程幂等。

---

## 6. Runtime 服务发现与传输

### 6.1 决策

V1 继续复用现有回环 HTTP Hook Server。端口配置功能暂不开放，但保留内部端口退避：

- 首选 `127.0.0.1:25679`。
- 占用时退避至 `25680..25689`。
- Desktop 把实际地址写入 `~/.ailight/runtime.json`。
- CLI 每次 Hook 调用读取该文件，不长期缓存端口。

HTTP 是内部传输实现；用户和目标客户端 Hook 配置均不感知端口。

### 6.2 RuntimeDescriptor V1

```json
{
  "schemaVersion": 1,
  "transport": {
    "type": "http",
    "host": "127.0.0.1",
    "port": 25681
  },
  "pid": 12345,
  "authToken": "random-runtime-token",
  "desktopVersion": "0.1.0",
  "protocol": {
    "min": 1,
    "max": 1
  },
  "startedAt": 1787380000000
}
```

约束：

- `schemaVersion` MUST 为 CLI 支持的版本。
- `transport.type` V1 MUST 为 `http`。
- `host` MUST 是 `127.0.0.1`；CLI MUST 拒绝非回环地址。
- `port` MUST 在 `1..65535`。
- `pid` SHOULD 指向仍存活的 AI-Light 进程。
- `authToken` MUST 不为空，并通过 `Authorization: Bearer` 发送。
- runtime 文件 MUST 在 Desktop 每次启动时重建，退出时尽力删除。

`transport` 使用可辨识联合结构，为未来 `unix_socket` / `named_pipe` 留出无破坏扩展空间，但两者不进入 V1 实现。

### 6.3 请求模型

CLI 复用 `hook-api.md` 的 `POST /hook`：

```json
{
  "source": "claude-code",
  "event": "state_change",
  "state": "WORKING",
  "session": "abc123",
  "ts": 1787380000000,
  "meta": {
    "adapter": {
      "name": "@ai-light/adapter",
      "version": "0.1.0",
      "protocolVersion": 1
    },
    "hookEvent": "UserPromptSubmit"
  }
}
```

Adapter MUST NOT 在 `meta` 中发送 prompt 正文、transcript 内容、认证信息或完整文件内容。

### 6.4 超时与重试

- 单次连接与请求超时 SHOULD 不超过 300ms。
- 仅网络错误或 `5xx` MAY 重试一次。
- 总 Hook 处理目标 MUST 不超过 800ms。
- `4xx` MUST NOT 重试。
- 请求成功且 `applied=false` 仍视为成功。
- runtime 缺失或 AI-Light 未运行时必须快速结束。

---

## 7. 标准事件模型

CLI 内部使用：

```ts
export interface NormalizedEvent {
  source: "claude-code" | "codex" | "qoder" | "trae" | "workbuddy" | string;
  state: "IDLE" | "WORKING" | "WAITING" | "SUCCESS" | "ERROR" | string;
  session?: string;
  timestamp: number;
  reason?: string;
  meta?: Record<string, unknown>;
}
```

Adapter 接口：

```ts
export interface ToolAdapter {
  readonly id: string;
  translate(input: unknown): NormalizedEvent[];
}
```

约束：

- `translate` MUST 是无 IO 的确定性函数。
- 单个原始事件 MAY 产生零到多个标准事件。
- 未识别事件 MUST 返回空数组，不得抛出导致 Hook 失败的异常。
- schema 校验错误 SHOULD 形成脱敏诊断，但 Hook 命令最终退出 `0`。
- `session` 优先使用工具提供的稳定会话 ID。

---

## 8. CLI 命令契约

### 8.1 Hook 入口

```bash
ailight-adapter hook claude-code
ailight-adapter hook codex
ailight-adapter hook qoder
```

- stdin：一个工具原始 Hook JSON。
- stdout：默认必须为空。
- stderr：仅在显式 debug 模式下输出脱敏诊断。
- 退出码：上报成功、忽略事件和可恢复失败均为 `0`。

### 8.2 安装与生命周期

```bash
ailight-adapter detect claude-code --json
ailight-adapter detect codex --json
ailight-adapter detect qoder --json
ailight-adapter install claude-code --json
ailight-adapter install codex --json
ailight-adapter install qoder --json
ailight-adapter repair claude-code --json
ailight-adapter repair codex --json
ailight-adapter uninstall claude-code --json
ailight-adapter uninstall codex --json
ailight-adapter uninstall qoder --json
```

变更型命令 MUST 支持预览：

```bash
ailight-adapter install claude-code --dry-run --json
```

`--dry-run` 返回目标文件、将新增/修改的托管条目和风险，不写文件。

### 8.3 诊断

```bash
ailight-adapter doctor --json
ailight-adapter doctor claude-code --json
ailight-adapter doctor codex --json
ailight-adapter doctor qoder --json
```

至少检查：

- Node 版本与路径。
- Adapter 版本与实际入口。
- `AILIGHT_HOME` 与共享目录权限。
- runtime 文件格式、PID 和服务连通性。
- Desktop/Adapter 协议版本交集。
- 工具是否安装。
- Hook 配置是否存在、重复、漂移或损坏。
- 托管配置使用的 CLI 路径是否仍有效。

### 8.4 开发辅助

```bash
ailight-adapter version --json
ailight-adapter translate claude-code < fixture.json
ailight-adapter translate codex < fixture.json
ailight-adapter translate qoder < fixture.json
ailight-adapter emit WORKING --source manual
```

`translate` MUST 不访问 runtime 或网络，便于 fixture 和第三方适配器开发。

### 8.5 JSON 输出封装

除 `hook` 外，支持 `--json` 的命令使用统一结构：

```json
{
  "ok": true,
  "command": "doctor",
  "data": {},
  "warnings": []
}
```

失败：

```json
{
  "ok": false,
  "command": "install",
  "error": {
    "code": "CONFIG_PARSE_FAILED",
    "message": "无法解析现有 Claude Code 配置"
  },
  "warnings": []
}
```

命令错误码 MUST 稳定，供 Tauri 和 Skill 使用；`message` 可本地化，`code` 不得随文案变化。

---

## 9. Hook 配置管理

### 9.1 所有权

CLI 是 Hook 配置管理的唯一实现。AI-Light UI 调用 CLI 的 JSON 命令，不直接解析或写入 Claude/Codex 配置。

### 9.2 通用写入规则

安装器 MUST：

1. 发现目标工具与配置位置。
2. 读取并完整解析现有配置。
3. 生成 `--dry-run` 计划。
4. 写入前备份到 `~/.ailight/backups/<tool>/`。
5. 合并而非覆盖已有 Hook。
6. 给托管条目添加稳定 AI-Light 标识。
7. 重复安装保持幂等，不重复追加。
8. 使用临时文件、校验和原子 rename。
9. 写后重新解析并验证。
10. 只删除或修复 AI-Light 自己管理的条目。

如果原配置无法解析，MUST 停止写入，不得尝试“修复”为一份只有 AI-Light Hook 的新文件。

### 9.3 托管条目标识

优先使用工具 schema 允许的描述或标识字段；若 schema 不允许额外字段，则通过严格、版本化的命令形态识别：

```text
ailight-adapter hook <tool> --managed-by ai-light --schema 1
```

识别 MUST 同时核对命令、参数和目标工具，不能仅凭字符串包含 `ailight` 删除用户配置。

### 9.4 配置指纹

`integrations.json` 保存托管片段的规范化摘要：

```json
{
  "schemaVersion": 1,
  "integrations": {
    "claude-code": {
      "status": "connected",
      "configPath": "/Users/alice/.claude/settings.json",
      "managedDigest": "sha256:...",
      "adapterVersion": "0.1.1"
    }
  }
}
```

摘要只覆盖 AI-Light 托管片段，不能把完整用户配置复制到共享状态文件。

---

## 10. Claude Code Adapter

### 10.1 接入方式

Claude Code Hook 支持 command 与 HTTP 等 handler。V1 统一使用 command handler 调用 Adapter CLI，使 Claude 与 Codex 共用同一个转换、服务发现和诊断路径。

参考：<https://code.claude.com/docs/en/hooks>

概念配置：

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/absolute/path/to/ailight-adapter",
            "args": ["hook", "claude-code", "--managed-by", "ai-light", "--schema", "1"],
            "async": true,
            "timeout": 2
          }
        ]
      }
    ]
  }
}
```

实际生成必须以目标 Claude Code 版本支持的配置 schema 为准，并通过 fixture/实机验证后固化。

### 10.2 事件映射

| Claude Code 事件 | matcher/条件 | AI-Light 状态 | 语义 |
|---|---|---|---|
| `SessionStart` | 主会话 | `IDLE` | 会话启动但尚无任务 |
| `UserPromptSubmit` | — | `WORKING` | 新一轮开始处理 |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `WAITING` | 等待用户回答或确认计划 |
| `PermissionRequest` | — | `WAITING` | 等待权限决定 |
| `PermissionDenied` | — | `WORKING` | auto mode 拒绝结果返回 Claude，继续决策 |
| `Elicitation` | — | `WAITING` | MCP 请求用户输入 |
| `ElicitationResult` | — | `WORKING` | 用户已完成 MCP 输入 |
| `Notification` | `permission_prompt` | `WAITING` | 权限通知兜底 |
| `Notification` | `elicitation_dialog\|elicitation_url_dialog` | `WAITING` | MCP 输入等待兜底 |
| `Notification` | `elicitation_complete\|elicitation_response` | `WORKING` | MCP 输入完成恢复兜底 |
| `Notification` | `agent_needs_input` | `WAITING` | Agent 明确需要输入 |
| `PostToolBatch` | — | `WORKING` | 整批并行工具完成，即将继续模型调用 |
| `PostToolUseFailure` | — | `WORKING` | 单工具失败已反馈 Claude，回合继续 |
| `Stop` | `background_tasks` 非空 | `WORKING` | 后台任务仍在运行 |
| `Stop` | 无后台任务 | `SUCCESS` | 本轮响应正常结束 |
| `StopFailure` | — | `ERROR` | 本轮因 API/系统错误终止 |
| `SessionEnd` | — | `IDLE` | 会话结束，释放该来源 |

Claude Code、Qoder 等客户端中，payload 含 `agent_id` 时视为子智能体内部事件，V1 不改变主会话灯效。TraeCode 的默认 Agent（`solo_agent`）和自定义 Agent（`custom`）作为顶层任务都会携带 `agent_id`，且当前 Hook 载荷没有可靠的嵌套标记，因此 TraeCode 暂不使用 `agent_id` 过滤事件。完整审计和全部 Hook 分类见 [Claude Code Hooks → AI-Light 状态映射研究](../research/claude-code-hooks-state-mapping.md)。

### 10.3 明确不订阅的事件

V1 不订阅：

- 普通 `PreToolUse`：发生在权限询问之前，只有 `AskUserQuestion` / `ExitPlanMode` 具有确定等待语义。
- `PostToolUse`：并行工具会并发多次触发，由每批一次的 `PostToolBatch` 取代。
- `TaskCreated` / `TaskCompleted`：与 turn 生命周期叠加易产生重复终态。
- `SubagentStart` / `SubagentStop`：V1 只表达主会话状态。
- `idle_prompt`：在 `Stop → SUCCESS` 后约 60 秒触发，不应破坏完成态。

Claude Code 当前没有“用户已批准普通权限”的独立生命周期事件。`PostToolBatch` 是确定性的恢复点，但只在获批工具批次执行完成后触发；长时间命令执行期间可能继续显示 `WAITING`，Adapter 不使用超时猜测授权结果。

### 10.4 Stop 的语义边界

`Stop → SUCCESS` 只表示“Claude 完成了当前一轮响应”，不证明用户的完整工程目标已经实现。若 `background_tasks` 非空则保持 `WORKING`。UI 文案 SHOULD 使用“本轮完成”或“响应完成”，不得宣称“任务已全部完成”。

### 10.5 会话恢复

恢复会话仍使用 Claude 提供的 `session_id`。同一会话内状态按事件顺序更新；Adapter 不自行维护长生命周期状态机。`SessionEnd` 缺失时由后续事件、Desktop 重启或业务层现有机制收敛，不在 Hook 进程中启动定时任务。

---

## 11. Codex Adapter

### 11.1 接入方式

Codex V1 优先使用 lifecycle command hooks。当前官方配置支持 `hooks.json` 同构事件或 `config.toml` 内联 hooks，且 `command` 是包含可执行文件与参数的完整命令字符串（Windows 可用 `commandWindows` 覆盖）；command handler 通过 stdin 接收原始 JSON。`notify` 仍可调用命令并传入 JSON，但只作为兼容降级通道。

参考：<https://learn.chatgpt.com/docs/config-file/config-reference>

安装器应优先选择官方当前推荐且不会覆盖用户配置的 Hook 位置；具体文件优先级必须用目标版本实测固定，不能仅依据旧调研中的版本路径。

### 11.2 事件映射

| Codex 事件 | AI-Light 状态 | 语义 |
|---|---|---|
| `UserPromptSubmit` | `WORKING` | 新一轮开始处理 |
| `PermissionRequest` | `WAITING` | 等待用户授权 |
| `Stop` | `SUCCESS` | 当前回合正常结束 |
| `SessionEnd` | `IDLE` | 会话结束 |

如果目标 Codex 版本提供稳定失败终态，Adapter MAY 映射为 `ERROR`，但必须先取得官方 schema 或实机 fixture，不得从非零进程退出码臆测整个任务失败。

### 11.3 notify 降级

仅在 lifecycle hooks 不可用或目标客户端缺少所需终态时 MAY 使用 `notify`：

- 同一终态不得同时启用 Hook 与 `notify`，防止重复提示音和驻留重置。
- 安装器必须检测既有 `notify`，不得覆盖用户命令。
- 若无法安全合并，报告 `NEEDS_MANUAL_ACTION`，而不是静默替换。
- Desktop 与 CLI 不得对“Codex Desktop 永远不支持”作固定断言，能力以当前官方配置和本机检测为准。

### 11.4 WAITING 局限

Codex 若仅提供权限请求而没有稳定的“等待一般输入”事件，则 `WAITING` 只覆盖权限等待。Adapter 不应通过读取对话文本或猜测回复内容推断等待状态。

### 11.5 WorkBuddy Adapter

WorkBuddy 从 CodeBuddy fork 并保留 Claude Code 兼容 Hook 结构，但配置命名空间固定为 `~/.workbuddy/settings.json`；Adapter 不读取或修改 `.codebuddy`。

WorkBuddy 支持从 Adapter `0.1.3` 开始；提供 WorkBuddy 卡片的 Desktop MUST 将兼容下限设为 `0.1.3`，避免旧 Adapter 在连接阶段返回 `TOOL_NOT_SUPPORTED`。

| WorkBuddy 事件 | matcher/条件 | AI-Light 状态 |
|---|---|---|
| `SessionStart` | `startup` | `IDLE` |
| `UserPromptSubmit` | — | `WORKING` |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `WAITING` |
| `Stop` | — | `SUCCESS` |
| `SessionEnd` | `other` | `IDLE` |

WorkBuddy 文档未定义可靠失败事件，Adapter MUST NOT 读取工具输出或退出码推断 `ERROR`。安装、检测、备份、幂等合并和卸载规则与 §9 一致。

### 11.6 Qoder Adapter

Qoder 桌面端与 Qoder CLI 使用兼容的 Hooks 结构；国际版使用 `~/.qoder/settings.json`，国内版使用 `~/.qoder-cn/settings.json`。Adapter `0.1.5` 起支持 `qoder` source；Desktop MUST 将兼容下限设为 `0.1.5`，避免旧 Adapter 被误判为可用。

| Qoder 事件 | AI-Light 状态 | 语义 |
|---|---|---|
| `SessionStart` / `SessionEnd` | `IDLE` | 任务生命周期边界 |
| `UserPromptSubmit` | `WORKING` | 用户提交新指令 |
| `PermissionRequest` / `Elicitation` | `WAITING` | 等待权限或信息输入 |
| `PermissionDenied` / `ElicitationResult` | `WORKING` | 结果返回后继续处理 |
| `PostToolUseFailure` | `WORKING` | 单个工具失败已返回 Agent，不等于任务失败 |
| `Stop` | `SUCCESS` | 主 Agent 本轮响应结束 |
| `StopFailure` | `ERROR` | 主 Agent 停止失败 |

携带 `agent_id` 的事件属于子智能体，不改变主任务灯效。Qoder 同时支持 HTTP handler，但 V1 统一安装 command handler，以复用 Adapter 的 `runtime.json` 服务发现、短期 Token、脱敏日志、跨平台命令和失败开放语义。路径选择采用存在性驱动：只存在一个发行版目录时管理该目录，两者并存时同时管理，两者均不存在时默认创建 `~/.qoder/settings.json`；全部目标完整才判定已连接。安装、检测、备份、幂等合并和卸载规则与 §9 一致；配置可见不代表 Runtime 已加载，真实闭环仍需启动低风险任务验证。

---

## 12. 桌面端集成体验

### 12.1 主流程

接入页对普通用户只呈现：

```text
Claude Code  [连接]
Codex        [连接]
Qoder        [连接]
WorkBuddy    [连接]
```

点击连接：

1. Desktop 调用 `detect <tool> --json`。
2. 检测 Node/npm/Adapter/工具/配置。
3. Adapter 未安装时由 Desktop 发起 npm 安装；权限不允许时展示降级命令。
4. Desktop 调用 `install <tool> --dry-run --json`。
5. 用户确认计划后调用正式安装。
6. UI 显示“已连接，等待首次真实事件”。
7. 收到对应 `source` 的 Hook 后显示“已验证”。

### 12.2 状态模型

```text
not_installed
ready_to_connect
connected_unverified
connected
needs_repair
unsupported
```

“测试灯效”和“验证工具连接”必须分开：

- 测试灯效：验证 Desktop → Core → BLE。
- 验证工具连接：必须收到 Claude/Codex 的真实 Hook，不能用手动 `triggerState` 伪装成功。

### 12.3 端口 UI

V1 设置页禁用或移除端口编辑。高级接口文档 MAY 继续说明 Hook API 默认地址，但普通接入卡不得显示端口、URL 或 Token。

---

## 13. 错误模型

### 13.1 稳定错误码

建议至少包含：

| code | 含义 |
|---|---|
| `NODE_UNSUPPORTED` | Node 版本不满足要求 |
| `TOOL_NOT_FOUND` | 目标工具未发现 |
| `ADAPTER_NOT_FOUND` | CLI 未安装或入口失效 |
| `CONFIG_NOT_FOUND` | 目标配置不存在且无法安全创建 |
| `CONFIG_PARSE_FAILED` | 现有配置无法解析 |
| `CONFIG_WRITE_FAILED` | 写入或原子替换失败 |
| `BACKUP_FAILED` | 无法创建备份，写入被阻止 |
| `MANAGED_ENTRY_DRIFTED` | 托管条目被修改 |
| `RUNTIME_NOT_FOUND` | AI-Light 未运行或 runtime 缺失 |
| `RUNTIME_INVALID` | runtime schema、权限或 owner 非法 |
| `PROTOCOL_INCOMPATIBLE` | Desktop 与 Adapter 无协议交集 |
| `SERVICE_UNREACHABLE` | 本地 Hook Server 不可达 |
| `NEEDS_MANUAL_ACTION` | 无法安全自动合并 |

### 13.2 Hook 与管理命令退出码

- `hook`：可恢复错误退出 `0`，避免阻塞 AI 工具。
- `install/repair/uninstall/doctor`：失败使用非零退出码，并通过 `--json` 输出稳定错误码。
- 用户取消或 dry-run 不属于失败。

---

## 14. 安全与隐私

### 14.1 最小数据原则

Adapter 默认只处理和上报：

- `hook_event_name`
- `session_id`
- 必要 matcher 字段，如 notification/error 类型
- 接收时间
- Adapter 名称、版本和协议版本

默认禁止：

- 上报 prompt 正文。
- 读取或上传 transcript 文件。
- 记录认证 Token。
- 记录完整 cwd；如诊断确有需要，仅保留 basename 或哈希。
- 访问非回环网络地址。

### 14.2 输入防护

- stdin 最大长度 SHOULD 为 1 MiB，超限忽略并记录错误码。
- JSON 根节点 MUST 为对象。
- 字符串字段必须设长度上限。
- `source` 由 Adapter 固定，不采信原始输入覆盖。
- `state` 由映射表生成，不直接接受工具输入中的任意值。
- 日志写入前必须脱敏。

### 14.3 npm 供应链

- npm 包 SHOULD 使用组织 scope。
- 发布使用最小权限 token、2FA/可信发布机制和 provenance（registry 支持时）。
- lockfile 必须提交。
- 发布包只包含 `dist` 与必要元数据。
- 依赖新增必须经过审计。
- 自动升级前必须解析目标版本并检查协议兼容，不能盲目执行未知 `latest`。

---

## 15. 日志与可观测性

### 15.1 Hook 热路径

默认不写 stdout。日志写入 `~/.ailight/logs/adapter/`，使用轮转和大小上限；写日志失败不得影响 Hook。

日志允许包含：

- 时间戳。
- Adapter 版本。
- 工具 ID、Hook 事件名、映射结果。
- runtime schema 版本、目标端口（debug 级别）。
- HTTP 状态码和稳定错误码。

不得包含 prompt、Token、完整 transcript、完整用户配置。

### 15.2 最近事件诊断

`adapter/state.json` MAY 保存每个工具最近一次脱敏结果：

```json
{
  "schemaVersion": 1,
  "tools": {
    "claude-code": {
      "lastEvent": "UserPromptSubmit",
      "mappedState": "WORKING",
      "receivedAt": 1787380000000,
      "delivery": "accepted"
    }
  }
}
```

该文件用于接入页诊断，不是业务状态事实源；当前业务状态仍只存在于 Rust Core。

---

## 16. 版本与升级

### 16.1 版本独立

Desktop、Adapter 和 Hook Protocol 独立版本：

```text
AI-Light Desktop  0.1.0
Adapter npm       0.4.2
Hook Protocol     1
```

同一协议主版本内，minor/patch 更新不得破坏既有字段和语义。

### 16.2 升级所有权

npm 不会自动升级全局包。V1 推荐：

1. Desktop 在接入页或启动后的非 Hook 热路径检查 registry。
2. 显示当前版本、目标版本和兼容状态。
3. 默认提供一键升级。
4. 可选开启空闲时自动升级。
5. 使用明确目标版本执行 `npm install --global @ai-light/adapter@x.y.z`。
6. 升级后运行 `doctor --json`。
7. 失败时保留当前可用版本并给出恢复建议。

Hook 进程 MUST NOT 查询 registry 或自更新。

### 16.3 兼容协商

CLI 读取 runtime 中 Desktop 支持的协议区间，与自身区间求交集：

- 有交集：选择双方支持的最高版本。
- 无交集：Hook 静默失败开放，管理命令报告 `PROTOCOL_INCOMPATIBLE`。

---

## 17. Skills 扩展

未来 Skill 通过稳定 CLI 命令自动接入：

```text
检测环境
  → ailight-adapter doctor --json
预览接入
  → ailight-adapter install <tool> --dry-run --json
用户确认
  → ailight-adapter install <tool> --json
验证
  → ailight-adapter doctor <tool> --json
```

边界：

- Skill 是自动化入口，不是运行时依赖。
- 没有 Skill 时，Desktop 和 CLI 必须能够完成完整接入。
- Skill 不得直接编辑 Claude/Codex 配置，应调用 CLI。
- Skill 必须展示 dry-run 计划并遵守用户确认边界。
- Skill 不读取或输出 runtime Token。

---

## 18. 测试方案

### 18.1 单元测试

- Claude/Codex 每个事件的 golden fixture → 标准事件逐字段断言。
- 未知事件 → 空数组。
- 缺字段、超长输入、非对象 JSON。
- runtime schema 与 loopback 校验。
- 配置合并、幂等安装、漂移检测和只删除托管条目。
- 日志脱敏。
- 协议版本协商。

### 18.2 文件系统集成测试

所有写入测试 MUST 使用临时 `AILIGHT_HOME` 和临时工具配置目录：

- 新安装。
- 已有其他 Hooks 时合并。
- 重复安装。
- 配置语法错误时拒绝写入。
- 备份失败时拒绝写入。
- 修复被修改的托管条目。
- 卸载后保留其他 Hooks。
- 原子写入失败模拟。

### 18.3 Hook Server 集成测试

- 启动测试 Hook Server并生成 runtime。
- CLI 从 stdin 接收官方格式 fixture。
- 断言服务收到正确标准事件。
- `applied=false` 仍成功。
- 连接失败、超时和 `5xx` 重试上限。
- 非回环地址被拒绝。

### 18.4 跨平台测试

至少覆盖：

- macOS：系统 Node、nvm/fnm/volta 任一主流版本管理器场景。
- Windows：npm global `.cmd` 入口、含空格路径、PowerShell/Git Bash 启动环境。
- Linux：系统 Node 与用户级 npm prefix。

### 18.5 实机黄金闭环

必须使用真实 Claude Code/Codex/WorkBuddy、真实 AI-Light Desktop 和 AgentCore-Light：

```text
工具 Hook → Adapter CLI → Hook Server → Arbiter
→ Theme → Transport → BLE → 实体灯/声音
```

---

## 19. 分阶段实施

### Phase A：CLI 骨架与协议

- 创建 `packages/ailight-adapter`。
- 实现命令路由、标准事件、runtime 读取和 Hook API client。
- 建立 fixture、JSON 输出和错误码。

验收：`translate` 与测试 Hook Server全绿。

### Phase B：Claude Code 黄金闭环

- 实现 Claude Adapter。
- 实现检测、dry-run、安装、修复和卸载。
- 完成真实事件与实体灯验证。

验收：WORKING、WAITING、SUCCESS、ERROR、IDLE 均有真实路径。

### Phase C：Codex 黄金闭环

- 实现 Codex Adapter 与 Hook 配置管理。
- 验证 CLI 与 Desktop 实际支持范围。
- 仅在必要时实现 `notify` 降级。

验收：WORKING、权限 WAITING、SUCCESS、IDLE 有真实路径；能力缺口被准确展示。

### Phase D：Desktop 产品化

- 迁移共享目录。
- 写入 runtime descriptor。
- 禁用端口配置 UI。
- 接入页调用 CLI 完成一键连接、诊断、修复和升级。
- 区分灯效测试与真实工具验证。

### Phase E：独立发布与 Skill

- 建立 npm 可信发布与版本检查。
- 提供一键升级及可选自动升级。
- 编写调用 CLI 的 AI-Light 接入 Skill。

---

## 20. 黄金闭环验收标准

Claude Code：

1. 用户从 AI-Light 点击“连接 Claude Code”。
2. 已有 Claude Hooks 不被覆盖。
3. 提交 prompt 后实体灯进入 `WORKING`。
4. 权限请求或需要输入时进入 `WAITING`。
5. 批准后后续工作事件恢复 `WORKING`。
6. 正常停止进入 `SUCCESS`。
7. `StopFailure` 进入 `ERROR`。
8. `SessionEnd` 回到 `IDLE`。
9. 接入页能显示最近真实事件。
10. 卸载后仅移除 AI-Light 托管条目。

Codex：

1. 用户从 AI-Light 点击“连接 Codex”。
2. 已有 Codex hooks/notify 不被覆盖。
3. 提交任务进入 `WORKING`。
4. 权限请求进入 `WAITING`。
5. 回合正常结束进入 `SUCCESS`。
6. 会话结束进入 `IDLE`。
7. 不可表达的状态在 UI 中如实标注，不使用内容猜测。
8. CLI 和 Desktop 实际支持差异由本机检测得出。

通用：

- AI-Light 未运行时不影响工具。
- 端口冲突对用户透明。
- 用户无需手改 JSON 或理解端口。
- runtime Token不泄漏到日志或 Hook 配置。
- Adapter 更新不要求重新发布 Desktop，协议兼容时不要求重装 Hook。
- macOS、Windows、Linux 安装和卸载均可恢复。

---

## 21. 已知限制与待实测事项

1. Claude Code/Codex/WorkBuddy 配置 schema 会演进，安装模板必须由官方文档和目标版本 fixture 驱动。
2. npm global 安装受 Node 版本管理器、PATH 和企业权限影响，Desktop 必须提供诊断与降级指引。
3. Codex 的普通“等待用户输入”可能缺少独立事件，V1 不做文本启发式。
4. Claude `Stop` 表示回合结束，不等于用户完整任务最终完成。
5. 当前共享目录方案不兼容 macOS App Sandbox；未来如进入 Mac App Store，必须引入签名 Helper/XPC 或重新设计共享通道。
6. Unix Domain Socket / Windows Named Pipe 仅作为未来传输选项；V1 使用 runtime-discovered loopback HTTP。
7. 真实 Hook 配置路径、事件字段与 Desktop/CLI 差异必须在实施前用当前版本实测，不得只依赖历史调研。

---

## 22. 参考资料

- [AI-Light Hook API](./hook-api.md)
- [AI-Light 产品边界](../requirements/product-boundary.md)
- [ADR-0001：接入层设计决策](../decisions/ADR-0001-接入层设计决策.md)
- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [Codex Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)
- [Qoder Hooks](https://docs.qoder.com/qoder/hooks)
- [Qoder CLI Hooks Reference](https://docs.qoder.com/cli/hooks-reference)
- [Apple：Protecting user data with App Sandbox](https://developer.apple.com/documentation/security/protecting-user-data-with-app-sandbox)
- [Apple：Application Support Directory](https://developer.apple.com/documentation/foundation/url/applicationsupportdirectory)

---

## 23. 决策摘要

| # | 决策 | 结论 |
|---|---|---|
| D-CLI-01 | Adapter 形态 | Node.js 短生命周期 CLI，不做 daemon |
| D-CLI-02 | 分发 | npm 包 `@ai-light/adapter`，命令 `ailight-adapter` |
| D-CLI-03 | 首期工具 | Claude Code、Codex；后续由 KAD-14/KAD-15 增加 WorkBuddy、Qoder |
| D-CLI-04 | 核心边界 | 原始 Hook → 标准事件；不碰 BLE/主题 |
| D-CLI-05 | 配置所有权 | CLI 统一管理，Desktop/Skill 调用 CLI |
| D-CLI-06 | 共享目录 | `~/.ailight`，支持 `AILIGHT_HOME` |
| D-CLI-07 | 传输 | runtime-discovered loopback HTTP |
| D-CLI-08 | 端口 | UI 禁止配置；内部 25679 优先并自动退避 |
| D-CLI-09 | 安全 | 当前用户私有目录、短期 Token、只连回环 |
| D-CLI-10 | macOS | 非 App Sandbox 分发；App Store 不在 V1 |
| D-CLI-11 | 升级 | Desktop 编排一键/可选自动升级；Hook 不自更新 |
| D-CLI-12 | Skills | 后续自动化入口，不是运行时依赖 |
| D-CLI-13 | WorkBuddy | 复用兼容 Hook 协议，独立管理 `~/.workbuddy/settings.json` |
| D-CLI-14 | Qoder | 复用 Adapter CLI，存在性驱动管理 `~/.qoder/settings.json` 与 `~/.qoder-cn/settings.json`，支持可靠失败终态 |

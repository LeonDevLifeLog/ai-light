# AI-Light IPC 契约（Commands + Events + Config）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.0 |
| 文档状态 | 生效（开工依据） |
| 适用范围 | Rust Core ↔ 前端（React）的接口面；以及 config.json 存储格式 |
| 架构依据 | KAD-03（Rust 唯一事实源 + events 推送）、ADR-0001/0002、hook-api V1.0、theme-format V1.0 |
| 生效日期 | 2026-08-19 |

> **本契约是 UI 与 Rust 的翻译层**：UI 设计时只依赖本文，Rust 实现时也只依赖本文。任何修改必须同步更新本文。

---

## 1. 调用约定

- **Commands**：前端 `invoke` → Rust `#[tauri::command]`，用于**配置类/操作类**调用；返回 `Result<T, AppError>`
- **Events**：Rust `emit` → 前端订阅，用于**状态类**推送；只推变化，不推全量（全量快照用 `get_app_state`）
- **命名**：payload 字段 camelCase；事件名 kebab-case
- **错误模型**：`AppError = { code: string, message: string }`，code 取值见 §4
- **幂等**：与 hook-api/V0.4 一致——重复下发相同内容不重复执行副作用

## 2. Commands 清单

### 2.1 状态查询

#### `get_app_state() → AppState`
启动时拉取一次全量快照。

```jsonc
{
  "service": { "version": "0.1.0", "port": 25679, "tokenEnabled": false },
  "device": {
    "connected": false,
    "address": null, "name": null,
    "fwVersion": null, "hardwareVariant": null,
    "batteryPercent": null, "powerSource": null, "chargeState": null
  },
  "business": {
    "state": "IDLE", "source": null, "session": null,
    "sinceTs": 0, "theme": "default"
  },
  "themes": ["default", "minimal", "neon", "nature", "aurora", "focus"],
  "activeTheme": "default"
}
```

> `device` 字段按能力位可缺省（无电池版无 battery 字段）；未连接时相关字段为 null。

### 2.2 主题域

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `get_themes()` | — | `[{ "name": "default", "builtin": true }, …]`（不含内容） | — | P1 |
| `get_theme(name)` | name: string | 主题完整 JSON（theme-format V1.0） | `NOT_FOUND` | P1 |
| `set_active_theme(name)` | name: string | `()` | `NOT_FOUND` / `THEME_INVALID` | P1 |
| `import_theme(content)` | content: 主题 JSON 字符串 | 主题名 string | `THEME_INVALID` / `CONFLICT`（与内置同名） | P1 |
| `export_theme(name)` | name: string | `{ "status": "exported", "fileName": "<name>.ailight-theme.json" }`；取消系统保存窗口返回 `{ "status": "cancelled" }` | `BAD_REQUEST` / `NOT_FOUND` / `THEME_INVALID` / `THEME_BUILTIN` / `INTERNAL` | P1 |
| `delete_theme(name)` | name: string | `{ "ok": true }` | `BAD_REQUEST` / `NOT_FOUND` / `THEME_BUILTIN`（内置不可删） | P1 |

**`set_active_theme` 副作用**：若当前业务状态非 IDLE，用新主题重新编译当前状态并下发（`APPLY_IF_CHANGED` 幂等对齐）。

**`delete_theme` 副作用**：仅允许删除用户主题；删除当前主题时先切换到内置 `default`，复用 `set_active_theme` 的持久化、事件和当前 SCENE 重放语义，再删除用户主题文件。文件删除失败时尝试恢复原主题。

**`export_theme` 副作用**：仅允许导出 `builtin == false` 的用户主题（包括导入后保存的主题）。Rust 重新校验持久化内容后打开系统保存窗口，以 `<name>.ailight-theme.json` 为默认文件名并原样写出；取消保存不是错误。导出不切换主题、不修改配置、不 emit event、不下发 SCENE。

### 2.3 设备域

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `scan_devices()` | — | `[{ "name": "ACLight-1A2B", "address": "AA:BB:…", "rssi": -55, "recognized": true }]` | — | P1 |
| `connect_device(address)` | address: string | `()`（连接结果同时走 events） | `NOT_FOUND` | P1 |
| `disconnect_device()` | — | `{ "ok": true }` | `DEVICE_DISCONNECT_FAILED` | P1 |
| `forget_device()` | — | `{ "ok": true }` | `DEVICE_DISCONNECT_FAILED` / `INTERNAL` | P1 |

- **识别规则**：`recognized` = 广播名 `ACLight-` 前缀 **或** 服务发现含 GB_TRANS 协议 UUID（对齐 pyPcTest）
- **connect 成功后**：自动写入 `config.remembered_device` + 执行 V0.4 握手（§5）→ 重发当前业务 SCENE（APPLY_IF_CHANGED）对齐
- **disconnect**：主动断开并取消当前连接代次的自动重连，保留 `rememberedDevice`，下次启动仍自动连接
- **forget**：先主动断开并取消自动重连，成功后清除并持久化 `rememberedDevice`
- 扫描时长：客户端实现，建议 5s（P1 可配置化）

### 2.4 控制域（手动触发 / 试听）

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `trigger_state(state, meta?)` | state: string, meta?: object | boolean（是否生效） | `BAD_REQUEST` | P1 |
| `preview_scene(state, theme?, content?)` | state: string, theme?: string, content?: string | `()` | `NOT_FOUND` / `THEME_INVALID` / `DEVICE_NOT_CONNECTED` | P1 |
| `reset_outputs()` | — | `()` | — | P1 |

- **`trigger_state`**：`source` 固定 `"manual"`，走仲裁器（与 hook-api 同语义），`applied` 幂等对账
- **`preview_scene`**：用主题的 state 映射编译 SCENE，以 **RESTART_SCENE** 语义下发试听；`content` 存在时优先校验并编译未保存的主题草稿，但不替换当前主题；**不改变业务状态**；设备未连接返回 `DEVICE_NOT_CONNECTED`
- **`reset_outputs`**：对应协议 `RESET_OUTPUTS`（0x05）原子全停；**同时业务状态复位为 IDLE**（保持灯效与业务状态一致）

### 2.5 配置域

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `get_config()` | — | Config（§3） | — | P1 |
| `update_config(patch)` | patch: Partial\<Config> | 更新后完整 Config | `BAD_REQUEST` / `AUTOSTART_FAILED` | P1 |
| `get_integration_status(tool)` | `claude-code \| codex` | Adapter 托管状态 | `BAD_REQUEST` / `ADAPTER_*` / `NODE_*` / `NPM_NOT_FOUND` / `TOOLCHAIN_*` | P1 |
| `install_integration(tool)` | `claude-code \| codex` | 写入结果 | `BAD_REQUEST` / `ADAPTER_*` / `NODE_*` / `NPM_NOT_FOUND` / `TOOLCHAIN_*` / `EXECUTABLE_TIMEOUT` | P1 |
| `uninstall_integration(tool)` | `claude-code \| codex` | 写入结果 | `BAD_REQUEST` / `ADAPTER_*` / `TOOLCHAIN_*` | P1 |

**`update_config` 允许字段**：`token` / `autostart` / `badgeOrientation` / `themeMode`。`portPreference` 为遗留兼容字段，不再接受用户 patch；Hook Server 固定优先 25679 并自动退避，实际地址通过 `~/.ailight/runtime.json` 提供给 Adapter（KAD-11）。`autostart` 采用"先 OS 后 config"：OS 登录项操作成功才写缓存，失败返回 `AUTOSTART_FAILED` 且 config 不变（KAD-09）。`rememberedDevice` 由连接流程管理，不接受用户 patch。仲裁固定为最近活动优先，不属于配置（ADR-0005 / KAD-13）。

**接入域与工具链（ADR-0006）**：`get_integration_status` / `install_integration` / `uninstall_integration` 全部经由 ToolchainService 解析的同一份工具链执行（稳定入口 `node + adapter cli.js`，不依赖 PATH 与 `.cmd` shim）。`get_integration_status` 为只读查询可用缓存；Adapter 缺失时返回结构化未连接状态 `{ connected: false, reason: "adapter_missing", toolchainState, toolchainSummary }` 而非错误。`install_integration` 强制复验；Adapter 缺失时用已选 Node + npm CLI 安装明确兼容版本（不装 `latest`），安装后重新解析。`uninstall_integration` 在 Adapter 不可用时返回 `ADAPTER_NOT_FOUND`（needs_repair 语义），不得误删其他 Hook。

### 2.6 工具链域（ADR-0006）

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `get_toolchain_status(force?)` | force?: boolean（默认 false，可用缓存） | ToolchainStatus（下方 schema） | — | P1 |
| `set_toolchain_overrides(patch)` | patch: `{ node?, npm?, adapter? }`（绝对路径） | ToolchainStatus | `TOOLCHAIN_OVERRIDE_INVALID`（字段级验证错误，details 携带 kind/path/reason） | P1 |
| `reset_toolchain_overrides()` | — | ToolchainStatus（mode 恢复 auto） | — | P1 |
| `select_executable(kind)` | kind: `node \| npm \| adapter` | ToolchainStatus（取消选择返回当前状态，不改变配置） | `BAD_REQUEST`（kind 非法）/ `TOOLCHAIN_OVERRIDE_INVALID`（所选路径验证失败） | P1 |

**ToolchainStatus schema**（`state` 全集：`checking / ready / node_missing / node_incompatible / npm_missing / adapter_missing / adapter_incompatible / invalid_override / ambiguous / permission_denied`）：

```jsonc
{
  "state": "ready",
  "mode": "auto",                    // "auto" | "manual"
  "summary": "Node.js 22.14.0 · npm 10.9.2 · Adapter 0.4.2",
  "node":   { "state": "ready", "path": "…node.exe", "version": "22.14.0", "source": "windowsRegistry", "overridden": false },
  "npm":    { "state": "ready", "path": "…npm-cli.js", "version": "10.9.2", "source": "siblingOfNode", "overridden": false },
  "adapter": { "state": "ready", "path": "…cli.js", "launcherPath": "…ailight-adapter.cmd", "version": "0.4.2", "source": "npmGlobalPrefix", "overridden": false },
  "issues": [ { "code": "NODE_NOT_FOUND", "message": "…", "tool": "node", "recovery": "安装 Node.js 20+，或点击「选择 Node」手动指定" } ],
  "checkedAt": "2026-08-30T10:00:00Z"
}
```

- 持久化：`~/.ailight/toolchain.json`（schema v1，独立于 config.json；overrides 是用户意图，selected 是可再生的缓存）
- `select_executable` 由后端打开原生文件选择器并立即验证；前端不能传任意未确认路径冒充选择结果
- 缓存失效采用内容信号（文件不存在 / mtime·size 变化 / override 变更 / 安装完成），写操作一律强制复验（ADR-0006）
- `AILIGHT_ADAPTER_BIN` 环境变量保留为开发/测试 override，直接执行并跳过解析器（ADR-0006 §13.2）

## 3. config.json Schema

文件位置：`~/.ailight/config.json`（`AILIGHT_HOME` 可覆盖），旧 app config dir 首次启动幂等迁移（KAD-11）。

```jsonc
{
  "version": 1,                    // schema 版本，当前 = 1
  "portPreference": 25679,         // 遗留兼容字段；运行时不接受用户修改
  "rememberedDevice": {            // 记住的设备；null = 无
    "address": "AA:BB:CC:DD:EE:FF",
    "name": "ACLight-1A2B"
  },
  "token": "",                     // 空字符串 = 不校验（hook-api §7）；非空 = 启用 Bearer 校验
  "autostart": false,              // 开机自启（OS 登录项为唯一事实源，config 为启动校准缓存；KAD-09）
  "badgeOrientation": "horizontal", // "horizontal"（默认）| "vertical"
  "themeMode": "dark"              // "dark"（默认）| "light" | "system"（外观模式；system = 跟随系统）
}
```

- 未知字段：加载时忽略并记日志（向前兼容）
- 非法值：回退默认值 + 记日志，不拒绝启动（`themeMode` 非法回退 `"dark"`）
- `token` 明文存储为已知风险（KAD-04 后果，V2 迁系统钥匙串）

## 4. 错误码（AppError.code）

| code | 含义 | 场景 |
|---|---|---|
| `BAD_REQUEST` | 参数非法 | trigger_state 状态名非法、patch 字段非法、export_theme 名称非法 |
| `NOT_FOUND` | 对象不存在 | get_theme/set_active_theme/export_theme/delete_theme、connect_device 地址未扫到 |
| `CONFLICT` | 冲突 | import_theme 与内置主题同名 |
| `THEME_INVALID` | 主题校验失败 | import/set_active_theme/preview_scene/export_theme（含校验失败原因于 message） |
| `THEME_BUILTIN` | 内置主题不可操作 | delete_theme/export_theme(内置) |
| `DEVICE_NOT_CONNECTED` | 设备未连接 | preview_scene 前置检查 |
| `DEVICE_DISCONNECT_FAILED` | 主动断开失败 | disconnect_device / forget_device；忘记操作不会清除记忆 |
| `AUTOSTART_FAILED` | 开机自启 OS 登录项操作失败 | update_config(autostart) 时 enable/disable 抛错（权限、路径失效、平台异常等） |
| `ADAPTER_NOT_FOUND` | Adapter CLI 不可执行 | 查询或管理 Claude Code/Codex 接入 |
| `ADAPTER_COMMAND_FAILED` | Adapter 管理命令失败 | 检测、安装或卸载 Hook |
| `ADAPTER_INSTALL_FAILED` | npm 全局安装失败 | 首次连接工具 |
| `ADAPTER_INVALID_OUTPUT` | Adapter 返回无法解析的数据 | Adapter 管理命令执行后解析失败 |
| `NPM_NOT_FOUND` | 已发现 Node，但无关联 npm | 首次连接且 Adapter 尚未安装 |
| `NODE_NOT_FOUND` | 未发现 Node.js | 工具链解析失败（恢复：安装 Node 20+ 或手动选择） |
| `NODE_INCOMPATIBLE` | Node 版本低于 20 | 工具链解析发现仅存在低版本（恢复：切换/选择兼容版本） |
| `TOOLCHAIN_OVERRIDE_INVALID` | 手动路径不存在或验证失败 | set_toolchain_overrides / select_executable / 工具链解析（恢复：重新选择或恢复自动检测） |
| `TOOLCHAIN_AMBIGUOUS` | 多组候选无法安全决策 | 工具链解析（恢复：用户选择一组 Node） |
| `TOOLCHAIN_PERMISSION_DENIED` | 文件或子进程权限不足 | 工具链解析/验证（恢复：调整权限/安装范围） |
| `EXECUTABLE_TIMEOUT` | 候选验证/命令执行超时 | 工具链验证、Adapter 命令（恢复：选择其他路径/查看诊断） |
| `INTERNAL` | 内部异常 | 兜底（含 BLE 下发失败） |

**错误 details 字段（ADR-0006）**：工具链域错误的 `AppError` 附加可选 `details`（kind/path/source/reason 或完整 ToolchainStatus），面向诊断展示；不得把完整环境变量或 token 返回前端。

## 5. Events 清单（Rust → 前端）

| 事件名 | 触发时机 | payload | 实现状态（2026-08-21） |
|---|---|---|---|
| `business-state-changed` | 仲裁结果变化（含 hold 回落） | `{ state, source, session, sinceTs, theme }`（`reset_outputs` 复位时仅携带 `state`，其余字段保持前端现值） | ✅ 已 emit |
| `device-connection-changed` | 连接/断开（含断连宽限开始） | `{ connected, address, name, reason?, reconnecting? }`（`reason`：`link_lost` / `reconnect_failed` / `manual_disconnect` / `forgotten`） | ✅ 连接、断连、主动断开、忘记、重连放弃均已 emit |
| `device-power-changed` | POWER_CHANGED / 握手后首次查询 | `{ batteryPercent, powerSource, chargeState, powerFlags }` | ✅ 握手 + 主动事件均已 emit |
| `device-fault` | FAULT_EVENT | `{ source, code, context }` | ✅ 已 emit |
| `theme-changed` | 主题切换生效 | `{ name }` | ✅ 已 emit |
| `config-changed` | 配置更新成功（设置页 / 托盘徽章朝向） | 更新后完整 Config | ✅ 已 emit |
| `open-config` | 托盘「打开配置」点击 | —（UI 导航事件） | ✅ 托盘已 emit，前端跳转 /devices |
| `hook-log`（P2） | 每次 hook 事件受理 | `{ source, state, session, applied, ts }`（排障日志面板用） | ❌ P2 未实现 |

**订阅约定**：前端启动时订阅全部事件；`get_app_state` 快照 + 事件增量构成完整视图。Rust 侧不关心前端是否在监听（事件可丢弃，前端可随时用快照自愈）。

## 6. 与既有规范的映射关系

| 本契约 | 上游依据 |
|---|---|
| trigger_state / reset_outputs 语义 | hook-api V1.0（manual source）、协议 V0.4 §12.4 |
| 主题相关 commands | theme-format V1.0（校验/编译）、ADR-0002 |
| 状态仲裁 | ADR-0005、KAD-13（固定最近活动优先，不进入 Config） |
| 设备识别/握手 | 协议 V0.4 §5、pyPcTest 识别逻辑 |
| 事件命名风格 | KAD-03（events 只读推送） |

## 7. 第一期实现范围（P1 汇总）

**P1 commands**：get_app_state / get_themes / get_theme / set_active_theme / import_theme / export_theme / delete_theme / scan_devices / connect_device / disconnect_device / forget_device / trigger_state / preview_scene / reset_outputs / get_config / update_config / get_integration_status / install_integration / uninstall_integration / get_toolchain_status / set_toolchain_overrides / reset_toolchain_overrides / select_executable
**P1 events**：business-state-changed / device-connection-changed / device-power-changed / device-fault / theme-changed
**P2（后续）**：hook-log

## 8. 实现状态对账快照（2026-08-21）

以代码为事实源（对应 ui-design.md §11 路线图对账）：

- **P1 commands（16 个）**：✅ 全部已注册（`src-tauri/src/commands.rs`）并由前端 `api` 层对接。
- **P1 events（5 个）**：✅ 全部已 emit。`device-connection-changed` 覆盖连接与断连双向；`device-power-changed` 由握手 GET_POWER_STATUS 与 POWER_CHANGED 主动事件触发；`device-fault` 由 FAULT_EVENT 触发。
- **工具链域 commands（4 个，ADR-0006，2026-08-30 增补）**：✅ 已注册并对接（`src-tauri/src/toolchain/` + 前端 `src/features/toolchain/`）；接入域三命令统一经 ToolchainService/ProcessRunner 执行。
- **UI 导航事件**：`open-config`（托盘「打开配置」）✅ 已 emit，前端订阅跳转 /devices。
- **P2 event（1 个）**：❌ `hook-log` 未实现。
- **主题导出**：✅ `export_theme` 已注册并由前端对接；仅用户主题显示入口，Rust 重新校验内容并以系统保存窗口写出，内置主题返回 `THEME_BUILTIN`，取消保存静默结束。
- **主题删除**：✅ `delete_theme` 已注册并由前端对接；仅用户主题显示入口，内置主题由 Rust 返回 `THEME_BUILTIN`；删除当前用户主题先回退 `default`。
- **试听错误映射**：✅ `preview_scene` 在设备未连接时前置返回 `DEVICE_NOT_CONNECTED`。
- **开机自启（G-06）**：✅ 已实装（2026-08-21）。`update_config` 先 OS 后 config（新增 `AUTOSTART_FAILED`）；setup 启动校准 `is_enabled()` 写回 config；平台 = macOS LaunchAgent / Windows Run key / Linux XDG autostart（tauri-plugin-autostart 2.5.1）；三平台实机待验证（U-08）。
- **配置项**：Hook Server 固定优先 25679、占用时自动退避；`portPreference` 仅为旧配置兼容字段，设置页不再开放修改。其余配置项均已生效。

*本文随实现推进修订；修改须同步更新 UI 设计与 Rust 实现双方。*

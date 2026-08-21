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
  "service": { "version": "0.1.0", "port": 47800, "tokenEnabled": false },
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
| `export_theme(name)` | name: string | `{ "content": "<JSON 字符串>" }` | `NOT_FOUND` | P2 |
| `delete_theme(name)` | name: string | `{ "ok": true }` | `NOT_FOUND` / `THEME_BUILTIN`（内置不可删） | P2 |

**`set_active_theme` 副作用**：若当前业务状态非 IDLE，用新主题重新编译当前状态并下发（`APPLY_IF_CHANGED` 幂等对齐）。

### 2.3 设备域

| Command | 请求 | 响应 | 错误码 | 优先级 |
|---|---|---|---|---|
| `scan_devices()` | — | `[{ "name": "ACLight-1A2B", "address": "AA:BB:…", "rssi": -55, "recognized": true }]` | — | P1 |
| `connect_device(address)` | address: string | `()`（连接结果同时走 events） | `NOT_FOUND` | P1 |
| `disconnect_device()` | — | `{ "ok": true }` | — | P2 |
| `forget_device()` | — | `{ "ok": true }` | — | P2 |

- **识别规则**：`recognized` = 广播名 `ACLight-` 前缀 **或** 服务发现含 GB_TRANS 协议 UUID（对齐 pyPcTest）
- **connect 成功后**：自动写入 `config.remembered_device` + 执行 V0.4 握手（§5）→ 重发当前业务 SCENE（APPLY_IF_CHANGED）对齐
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

**`update_config` 允许字段**：`arbitrationMode` / `token` / `autostart` / `badgeOrientation`。`autostart` 采用"先 OS 后 config"：OS 登录项操作成功才写缓存，失败返回 `AUTOSTART_FAILED` 且 config 不变（KAD-09）。`portPreference` P1 只读，变更与 HTTP 服务热重启留待 P2；`rememberedDevice` 由连接流程管理，不接受用户 patch。

## 3. config.json Schema

文件位置：app config dir（KAD-04），文件名 `config.json`。

```jsonc
{
  "version": 1,                    // schema 版本，当前 = 1
  "arbitrationMode": "priority",   // "priority"（默认）| "last_active"（ADR-0001 Q8）
  "portPreference": 47800,         // hook 服务首选端口；0 = 自动（47800 起退避至 47810）
  "rememberedDevice": {            // 记住的设备；null = 无
    "address": "AA:BB:CC:DD:EE:FF",
    "name": "ACLight-1A2B"
  },
  "token": "",                     // 空字符串 = 不校验（hook-api §7）；非空 = 启用 Bearer 校验
  "autostart": false,              // 开机自启（OS 登录项为唯一事实源，config 为启动校准缓存；KAD-09）
  "badgeOrientation": "horizontal" // "horizontal"（默认）| "vertical"
}
```

- 未知字段：加载时忽略并记日志（向前兼容）
- 非法值：回退默认值 + 记日志，不拒绝启动
- `token` 明文存储为已知风险（KAD-04 后果，V2 迁系统钥匙串）

## 4. 错误码（AppError.code）

| code | 含义 | 场景 |
|---|---|---|
| `BAD_REQUEST` | 参数非法 | trigger_state 状态名非法、patch 字段非法 |
| `NOT_FOUND` | 对象不存在 | get_theme/set_active_theme/export/delete、connect_device 地址未扫到 |
| `CONFLICT` | 冲突 | import_theme 与内置主题同名 |
| `THEME_INVALID` | 主题校验失败 | import/set_active_theme/preview_scene（含校验失败原因于 message） |
| `THEME_BUILTIN` | 内置主题不可操作 | delete_theme(内置) |
| `DEVICE_NOT_CONNECTED` | 设备未连接 | preview_scene |
| `AUTOSTART_FAILED` | 开机自启 OS 登录项操作失败 | update_config(autostart) 时 enable/disable 抛错（权限、路径失效、平台异常等） |
| `INTERNAL` | 内部异常 | 兜底（含 BLE 下发失败） |

## 5. Events 清单（Rust → 前端）

| 事件名 | 触发时机 | payload | 实现状态（2026-08-21） |
|---|---|---|---|
| `business-state-changed` | 仲裁结果变化（含 hold 回落） | `{ state, source, session, sinceTs, theme }` | ✅ 已 emit |
| `device-connection-changed` | 连接/断开（含断连宽限开始） | `{ connected, address, name, reason?, reconnecting? }`（`reason`：`link_lost` / `reconnect_failed`；`reconnecting`：断连后是否处于自动重连） | ✅ 连接 / 断连 / 重连放弃均已 emit |
| `device-power-changed` | POWER_CHANGED / 握手后首次查询 | `{ batteryPercent, powerSource, chargeState, powerFlags }` | ✅ 握手 + 主动事件均已 emit |
| `device-fault` | FAULT_EVENT | `{ source, code, context }` | ✅ 已 emit |
| `theme-changed` | 主题切换生效 | `{ name }` | ✅ 已 emit |
| `config-changed` | 配置更新成功（设置页 / 托盘徽章朝向） | 更新后完整 Config | ✅ 已 emit |
| `hook-log`（P2） | 每次 hook 事件受理 | `{ source, state, session, applied, ts }`（排障日志面板用） | ❌ P2 未实现 |

**订阅约定**：前端启动时订阅全部事件；`get_app_state` 快照 + 事件增量构成完整视图。Rust 侧不关心前端是否在监听（事件可丢弃，前端可随时用快照自愈）。

## 6. 与既有规范的映射关系

| 本契约 | 上游依据 |
|---|---|
| trigger_state / reset_outputs 语义 | hook-api V1.0（manual source）、协议 V0.4 §12.4 |
| 主题相关 commands | theme-format V1.0（校验/编译）、ADR-0002 |
| arbitrationMode | ADR-0001 Q8 |
| 设备识别/握手 | 协议 V0.4 §5、pyPcTest 识别逻辑 |
| 事件命名风格 | KAD-03（events 只读推送） |

## 7. 第一期实现范围（P1 汇总）

**P1 commands**：get_app_state / get_themes / get_theme / set_active_theme / import_theme / scan_devices / connect_device / trigger_state / preview_scene / reset_outputs / get_config / update_config
**P1 events**：business-state-changed / device-connection-changed / device-power-changed / device-fault / theme-changed
**P2（后续）**：export_theme / delete_theme / disconnect_device / forget_device / hook-log

## 8. 实现状态对账快照（2026-08-21）

以代码为事实源（对应 ui-design.md §11 路线图对账）：

- **P1 commands（12 个）**：✅ 全部已注册（`src-tauri/src/commands.rs`）并由前端 `api` 层对接。
- **P1 events（5 个）**：✅ 全部已 emit。`device-connection-changed` 覆盖连接与断连双向；`device-power-changed` 由握手 GET_POWER_STATUS 与 POWER_CHANGED 主动事件触发；`device-fault` 由 FAULT_EVENT 触发。
- **P2 commands / event（5 个）**：❌ 全部未实现。
- **错误码映射偏差**：`preview_scene` 在设备未连接时实际返回 `INTERNAL`（commands 层统一 `internal()` 映射），契约要求 `DEVICE_NOT_CONNECTED`——待修正。
- **开机自启（G-06）**：✅ 已实装（2026-08-21）。`update_config` 先 OS 后 config（新增 `AUTOSTART_FAILED`）；setup 启动校准 `is_enabled()` 写回 config；平台 = macOS LaunchAgent / Windows Run key / Linux XDG autostart（tauri-plugin-autostart 2.5.1）；三平台实机待验证（U-08）。
- **配置项**：`arbitrationMode` / `token`（服务端 Bearer 校验已实现）/ `badgeOrientation` / `rememberedDevice` 已生效；`autostart` 已接 `tauri-plugin-autostart`（OS 登录项为事实源，config 为校准缓存）；`portPreference` 已持久化但 `hook_server::serve` 未读取。

*本文随实现推进修订；修改须同步更新 UI 设计与 Rust 实现双方。*

# AI-Light 交互组件契约（UI Interaction Spec）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.13 |
| 文档状态 | 生效；已按代码实现状态对账（V1.13，2026-08-21） |
| 范围 | L5 展示层**组件级**行为契约（中粒度） |
| 上游 | [ui-design.md](./ui-design.md) / [ui-interactions.md](./ui-interactions.md) / [ipc-contract.md](./ipc-contract.md) / [theme-format.md](./theme-format.md) / 蓝牙硬件 V0.4 |
| 下游 | `ui-ux-pro-max` 技能 / 前端组件开发 |
| 配套原型 | [docs/design/ui-preview.html](../design/ui-preview.html) |
| 颗粒度 | L4 组件层 + L5 关键控件 + 联动矩阵 + 失败路径 |
| 不包含 | 代码实现 / CSS / 视觉细节（见 ui-design.md）/ 协议字段细节 |

> 本文是 L5 展示层的**组件视角契约**——和 [ui-interactions.md](./ui-interactions.md)（流程视角）互补。
> 任何组件调整先更新本文，再用 `ui-ux-pro-max` 技能或人工实现。

---

## 目录

- [§1 文档约定](#1-文档约定)
- [§2 组件金字塔（6 层）](#2-组件金字塔6-层)
- [§3 全局联动矩阵](#3-全局联动矩阵)
- [§4 失败路径矩阵](#4-失败路径矩阵)
- [§5 核心状态机](#5-核心状态机)
- [§6 L2 页面层组件](#6-l2-页面层组件)
- [§7 L3 区域层组件](#7-l3-区域层组件)
- [§8 L4 通用组件库](#8-l4-通用组件库)
- [§9 组件生命周期与资源清理](#9-组件生命周期与资源清理)
- [§10 变更日志](#10-变更日志)

---

## 1. 文档约定

### 1.1 命名约定

| 类别 | 命名法 | 示例 |
|---|---|---|
| 触发器（来自外部 / 用户） | `on{Event}` / `on{Action}` | `onBusinessStateChanged` / `onClickConnect` / `onSliderChange` |
| 副作用（前端发起） | `invoke{Command}` / `emit{Event}` | `invokeConnectDevice` / `emitToastError` |
| 视觉态枚举 | 小写连字符 | `default / hover / focus / active / disabled / loading / error / empty` |
| 状态机迁移 | `[From] --(条件)--> [To]` | `[Disconnected] --(scan+connect)--> [Connecting]` |
| 数据流方向 | `Rust → UI` / `UI → Rust` / `User → UI` | — |
| 字段引用 | 反引号包裹 | `businessState`、`theme.name`、`device.batteryPercent` |

### 1.2 视觉态全集（8 态）

所有可交互组件必须支持以下 8 态的语义定义，具体视觉由 [ui-design.md §10](./ui-design.md) 设计代币落地：

| 态 | 触发条件 | 语义 |
|---|---|---|
| `default` | 初始 / 数据就绪 | 常规可交互外观 |
| `hover` | 鼠标悬停 / 触控长按 | 提示"可交互"，轻微视觉提升 |
| `focus` | Tab 聚焦 / 编程聚焦 | 提示"键盘当前位置"，焦点环可见 |
| `active` | 鼠标按下 / 触发中 | 提示"正在响应"，按下反馈 |
| `disabled` | 不可用 | 不响应输入；视觉灰显 |
| `loading` | 异步进行中 | 占位骨架 / spinner；屏蔽重复触发 |
| `error` | 校验失败 / 操作失败 | 错误色边框 + 错误提示；抖动反馈 |
| `empty` | 无数据 | 主操作按钮 + 引导文案 |

### 1.3 联动表模板

本文档所有联动表采用统一 4 列模板：

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|

- **Source Event**：触发源（Rust event / 用户操作 / 内部事件）
- **目标组件**：受影响的组件 ID
- **同步字段**：哪些 props/state 被刷新
- **同步方式**：`full`（全量替换） / `patch`（局部合并） / `append`（追加）/ `remove`（移除）/ `toggle`（切换布尔）

### 1.4 组件详表统一子节模板

每个 L2~L4 组件使用以下 6 子节结构：

```
A.x.1 用途
A.x.2 对外契约（Props / Emits / Invokes）
A.x.3 视觉态全集
A.x.4 联动矩阵
A.x.5 边界条件（空 / 错 / 载 / 中断）
A.x.6 无障碍（键盘 / ARIA / reduced-motion / 色盲）
```

---

## 2. 组件金字塔（6 层）

```
L1 业务域        5 个一级导航项（状态 / 设备 / 接入 / 主题 / 试听）+ 设置入口
  ↓
L2 页面层        6 个 page-section（dashboard / devices / integrations / themes / preview / settings）
  ↓
L3 区域层        每页面内的功能区块（e.g. Dashboard = StatusHero + DeviceCard + ThemeCard）
  ↓
L4 组件层        可复用 UI 组件（LightDot / TrafficBadge / DeviceCard / StateTab / MotionPresetCard / Switch ...）
  ↓
L5 控件层        子控件（color picker / slider / select / chip / tag / button ...）
  ↓
L6 状态层        每个 L4/L5 组件的 8 个视觉态
```

### 2.1 L1 业务域清单

| 路径 | 业务目标 | 一句话描述 |
|---|---|---|
| `/` | 状态总览 | "我现在啥状态？" |
| `/devices` | 设备管理 | "我连了啥灯牌？" |
| `/integrations` | 接入外部工具 | "AI 工具咋把状态告诉我？" |
| `/themes` | 主题中心 | "灯效咋变？" |
| `/preview` | 试听 | "手动试试灯效对不对" |
| `/settings` | 设置 | "调一调偏好" |

### 2.2 L2 → L3 映射

| L2 页面 | L3 区域组件 | 数量 |
|---|---|---|
| Dashboard | `StatusHero` / `DeviceCard` / `ThemeCard` | 3 |
| Devices | `ScanProgress` / `ScanResultList` / `DeviceDetailCard` / `FaultAlert` | 4 |
| Integrations | `IntegrationCard` × 4 / `HelpFooter` | 5 |
| Themes | `ThemeGrid` / `ThemeDetailPanel` / `ImportThemeDialog` / `ThemeEditorDialog` | 4 |
| Preview | `StandardStateButtonGroup` / `CustomStateInput` / `CustomStateQuickList` / `ResetOutputsButton` | 4 |
| Settings | `SettingGroup` × N（外观 / 设备 / 主题 / 系统） | 4 |

---

## 3. 全局联动矩阵

> 核心交付物之一：列出每个 Rust event 推送到前端后，哪些组件 / 哪些字段被同步。

### 3.1 业务状态层（`business-state-changed`）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `business-state-changed` | `TrafficBadge` | `currentState` / `currentOrient` | patch |
| `business-state-changed` | `StatusHero.stateName` | `state.text` / `state.color` | patch |
| `business-state-changed` | `StatusHero.stateSubtitle` | `state.subtitle` | patch |
| `business-state-changed` | `Dashboard.DeviceCard.realtimeTag` | `source` / `sinceTs` | patch |
| `business-state-changed` | `Preview.previewTag` | `currentState` | patch |
| `business-state-changed` | `ThemeEditor.editorPreviewState` | `editingState`（仅当用户当前未编辑） | patch |
| `business-state-changed` | 托盘菜单文字 | `currentState` | patch |

**Payload**（[ipc-contract.md §5](./ipc-contract.md)）：
```typescript
{ state: BusinessState, source: string|null, session: string|null, sinceTs: number, theme: string }
```

### 3.2 设备连接层（`device-connection-changed`）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `device-connection-changed` | `Dashboard.DeviceCard` | `connected` / `address` / `name` | full |
| `device-connection-changed` | `Sidebar.statusDot` | `connected` | patch |
| `device-connection-changed` | `Devices.ScanResultList` | 对应卡片的 `state` 字段 | patch |
| `device-connection-changed` | `ThemeEditor.previewSceneButton` | `disabled = !connected` | patch |
| `device-connection-changed` | `Preview.resetOutputsButton` | `disabled = !connected` | patch |

**Payload**：
```typescript
{ connected: boolean, address: string|null, name: string|null, reconnecting?: boolean, reason?: string }
```

> ✅ 实现状态：连接成功（`reconnecting: false`）与断连（`reason: "link_lost"`、`reconnecting: true`）均已 emit；重连放弃时 emit `reason: "reconnect_failed"`、`reconnecting: false`。`reason` 值域当前为 `link_lost` / `reconnect_failed`。

### 3.3 设备电源层（`device-power-changed`）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `device-power-changed` | `DeviceCard.batteryBlock` | `batteryPercent` / `powerSource` / `chargeState` | patch |
| `device-power-changed` | `DeviceCard.lowBatteryBadge` | `batteryPercent < 20` → 显示 | toggle |
| `device-power-changed` | `DeviceCard.chargingIcon` | `chargeState == CHARGING` | toggle |
| `device-power-changed` | 全局 Toast | `batteryPercent < 10` → "电量过低警告" | emit |

**Payload**：
```typescript
{ batteryPercent: number|null, powerSource: string|null, chargeState: string|null, powerFlags: number }
```

> ✅ 实现状态：握手 GET_POWER_STATUS 与运行期 POWER_CHANGED 均已 emit（无电池能力设备不发）。

### 3.4 设备故障层（`device-fault`）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `device-fault` | `Dashboard.FaultAlert` | `source` / `code` / `context` | append |
| `device-fault` | `Devices.FaultAlert` | `source` / `code` / `context` | append |
| `device-fault` | `DeviceCard.faultIndicator` | 故障源图标 + tooltip | patch |

**Payload**：
```typescript
{ source: 'LED'|'BUZZER'|'POWER'|'PROTOCOL', code: number, context: number }
```

> ✅ 实现状态：FAULT_EVENT 已接线并 emit `device-fault`。

### 3.5 主题层（`theme-changed`）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `theme-changed` | `Dashboard.ThemeCard` | `activeTheme.name` / 缩略灯条 | patch |
| `theme-changed` | `Themes.ThemeGrid` | 当前激活主题卡的外发光 + `当前使用` tag | patch |
| `theme-changed` | `ThemeEditor.meta` | `editingTheme.name` | patch |
| `theme-changed` | 托盘菜单文字 | `当前主题：<name>` | patch |
| `theme-changed` | `Preview.currentThemeLabel` | `<themeName>` | patch |

**Payload**：
```typescript
{ name: string }
```

### 3.6 配置层（`update_config` 成功后，前端响应）

| 配置变更 | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `badgeOrientation: 'horizontal'\|'vertical'` | `TrafficBadge.layout` | — | patch |
| `badgeOrientation` 变更 | `Sidebar.trayMenu` 单选 | — | patch |
| `arbitrationMode` 变更 | `Settings.arbitrationCards` 选中卡片 | — | patch（ModeOption 卡片，见 §7.6.1） |
| `autostart` 变更 | `Settings.autostartSwitch` | — | patch（先 OS 后 config，失败 `AUTOSTART_FAILED` 回滚） |
| `config-changed`（Rust 事件） | 全组件 | `config.badgeOrientation` 等完整 Config | full sync |

> `portPreference` 热重启为 P2；`autostart` 真实切换已实装（2026-08-21，KAD-09），不再属于 P2。
>
> ✅ 实现状态：`update_config`（设置页与托盘徽章朝向共用）成功后 emit `config-changed`，前端订阅整包同步。

### 3.7 蓝牙主动事件（来自协议 V0.4 §11）

| Source Event | 目标组件 | 同步字段 | 同步方式 |
|---|---|---|---|
| `DEVICE_READY (0xE0)` | `DeviceCard` | 显示 `fw_version` / `hardware_variant` | patch |
| `POWER_CHANGED (0xE2)` | `DeviceCard.batteryBlock` | 同 3.3 | patch |
| `BUTTON_EVENT (0xE3)` | `DeviceCard` 按键记录 | `event` / `duration_ms`（V2 显示） | append |
| `FAULT_EVENT (0xEF)` | 同 3.4 | — | — |

> ✅ 实现状态：四个协议主动事件均已接线（DEVICE_READY 用于握手；POWER_CHANGED / FAULT_EVENT 已 emit；BUTTON_EVENT 当前仅记录日志，V2 展示）。

### 3.8 初始化流程（一次性）

```
窗口挂载 → invokeGetAppState → 全量快照填充所有组件
         → subscribe all events → 进入增量更新
```

**快照字段**（[ipc-contract.md §2.1](./ipc-contract.md)）：
- `service` → Sidebar 端口 / 版本号
- `device` → DeviceCard 全字段
- `business` → StatusHero / 主题卡
- `themes` → ThemeGrid 全量列表
- `activeTheme` → ThemeGrid 高亮 + 编辑器初始值

---

## 4. 失败路径矩阵

> 核心交付物之二：覆盖 ipc-contract §4 + 蓝牙 V0.4 §3.6 全部错误码到 UI 反馈。

### 4.1 ipc-contract 错误码 → UI 反馈

| 错误码 | 触发场景 | UI 反馈 | 用户操作 |
|---|---|---|---|
| `NOT_FOUND` | 主题名不存在 / 设备地址找不到 | Toast `<对象> 不存在` | 重试 / 返回 |
| `THEME_INVALID` | 导入 / 编辑主题校验失败 | Dialog "主题校验失败" + `<details>`（缺失字段 / 非法值 / SCENE 引用缺失） | 修改 / 取消 |
| `CONFLICT` | 导入主题与内置同名 | Dialog "导入失败：与内置主题 `<name>` 同名" | 重命名 / 取消 |
| `THEME_BUILTIN` | 尝试删除内置主题 | Dialog "内置主题不可删除" | 关闭 |
| `BAD_REQUEST` | 参数非法（如 trigger_state 状态名含非法字符） | Toast "请求参数非法：`<reason>`" | 修正输入 |
| `DEVICE_NOT_CONNECTED` | preview_scene / trigger_state 时未连接 | Toast "请先连接设备" | 跳转 `/devices` |
| `INTERNAL` | Rust 侧异常（含 BLE 下发失败） | Toast "服务异常，请查看日志" | 打开日志目录 |

### 4.2 蓝牙协议 result code → UI 反馈（V0.4 §3.6）

| Result code | 触发场景 | UI 反馈 |
|---|---|---|
| `0x00 OK` | 正常 | （silent） |
| `0x01 INVALID_LENGTH` | 帧数据区长度错 | 日志（不弹 UI） |
| `0x02 INVALID_PARAMETER` | SCENE 参数非法 | Toast "灯效参数非法：`<field>`" |
| `0x03 UNSUPPORTED_COMMAND` | 命令字未定义 | 日志（不弹 UI） |
| `0x04 BUSY` | 设备暂时忙 | Toast "设备忙，请稍后" |
| `0x05 INVALID_STATE` | 当前状态不允许 | Toast "设备当前不允许此操作" |
| `0x06 VERSION_MISMATCH` | 协议版本字节 ≠ 0x04 | Toast "设备协议版本不兼容，请升级固件" + 断开 |
| `0x07 NOT_READY` | 外设 / CCC 未就绪 | Toast "设备未就绪" |
| `0x09 LOW_BATTERY` | 低电量保护拒绝 | Toast "电量过低，灯效已停止" + 设备卡 lowBatteryBadge 高亮 |
| `0x0A INTERNAL_ERROR` | 设备内部异常 | Toast "设备内部异常" + device-fault 同步推送 |
| `0x0B NOT_SUPPORTED` | 无电池版调 POWER_OFF 等 | Toast "设备不支持此操作" |

### 4.3 蓝牙连接握手各阶段失败（V0.4 §5）

| 阶段 | 后端动作 | 失败 UI 反馈 |
|---|---|---|
| 1. BLE 连接 | `btleplug.connect` | Toast "连接失败：`<reason>`"（超时 / 权限被拒 / 距离过远） |
| 2. DIS 读取 | `read DIS 0x2A26` | 日志（不弹 UI；继续握手） |
| 3. CCC 使能 | `subscribe TX CCC` | Toast "无法使能通知通道" + 断开 |
| 4. DEVICE_READY | 等设备主动事件 | 超时 3s → Toast "设备无应答" + 断开 |
| 5. GET_DEVICE_INFO | `0x02` | Toast "读取设备信息失败" + 断开 |
| 6. GET_CAPABILITIES | `0x04` | Toast "读取设备能力失败" + 断开 |
| 7. BAS 订阅（条件） | `subscribe BAS 0x2A19/0x2BED` | 日志（不弹 UI；无电池版本无 BAS） |
| 8. GET_POWER_STATUS | `0x50` | Toast "读取电源状态失败" + 断开 |

**任一阶段失败 → 设备卡回滚到 `Disconnected` + 触发 `device-connection-changed{connected: false, reason}`**

> ✅ 实现状态：阶段 1~8 已实现。握手失败走连接命令错误路径（前端 Toast），不额外 emit `device-connection-changed{false}`；运行中断连才触发 false 事件与退避重连。

### 4.4 蓝牙断连与宽限期（V0.4 §13）

| 时间窗口 | 设备侧行为 | UI 反馈 |
|---|---|---|
| 断开瞬间 | 当前 SCENE 继续运行 | Toast "设备已断开" + 设备卡 `Reconnecting` 态 |
| 0~60s | 等重连 | Toast "重连中..."（可关闭） |
| 60s 内重连成功 | 无感恢复 | Toast "设备已重新连接" + 设备卡恢复 |
| 60s 超时 | 设备自动 RESET_OUTPUTS | Toast "设备已离线，请手动重连" + 设备卡 `Disconnected` + [去连接] 按钮高亮 |
| 重连失败 N 次 | 退避后停止 | Toast "重连失败，请检查设备" + 设备卡保持 `Disconnected` |

> ✅ 实现状态：断连监听与客户端退避重连（5 次，约 75s 窗口，期间已手动连接则放弃）已实现；前端 `Reconnecting` 视觉态（Devices 页重连中卡 + Dashboard 摘要）与断连/重连 Toast 已实装（2026-08-21）。

### 4.5 主题相关失败路径

| 场景 | UI 反馈 |
|---|---|
| 导入文件 > 1MB | Dialog "文件过大（>1MB）" |
| 导入文件非 `.ailight-theme.json` 扩展名 | 文件选择器过滤（前置拦截） |
| JSON 解析失败 | Dialog "JSON 解析失败：第 X 行 Y 列 `<reason>`" |
| 顶层多键 / 缺键（theme / scenes / states） | Dialog "校验失败：缺少/多余键 `<key>`" |
| SCENE 引用不存在 | Dialog "校验失败：states[`<state>`].scene 引用 `<scene>` 不存在" |
| 字段非法（如 brightness=101、duration_ms=0） | Dialog "校验失败：`<field>` 值非法：`<value>`" |
| 编辑保存失败（写文件失败） | Toast "保存失败：`<reason>`" + 编辑器保留输入 |
| 主题切换校验失败 | Toast "主题 'X' 校验失败" + 不切换 + 保留当前主题 |

### 4.6 启动期 / 进程级失败

| 场景 | UI 反馈 |
|---|---|
| `hook_server` 未启动（端口占用） | 启动时 Toast "hook 服务启动失败：端口被占用"；UI 不感知后续 hook 事件 |
| 启动期 no reactor panic（ADR-0003） | 进程退出 + 系统对话框 "服务启动异常，请查看日志" |
| `config.json` 缺失 / 非法 | 启动时使用默认值 + 日志；UI 不弹错 |

---

## 5. 核心状态机

### 5.1 窗口可见性（[ui-design.md §7.1](./ui-design.md)）

```
[Hidden]   --(托盘"显示窗口")-->     [Visible]
[Visible]  --(用户点 X)-->            [Hidden]    // 关窗 = 隐藏
[任意]     --(托盘"退出")-->         [Terminating]
[Visible]  --(单实例新启动)-->        [Visible]   // 聚焦
```

> ✅ 实现状态：托盘「显示窗口」/「退出」、关窗 = 隐藏、单实例聚焦均已在 Rust 侧落地（`src-tauri/src/tray.rs` + `on_window_event`）。

### 5.2 设备生命周期

```
                   ┌──────────────┐
                   │Disconnected  │ ← 初始态 / 60s 超时 / 主动断开
                   └──────�───────┘
                          │ invoke connect_device(addr)
                          ▼
                   ┌──────────────┐
                   │ Connecting   │ 进入 V0.4 §5 握手 7 阶段
                   └──────┬───────┘
              握手失败 ↓  ↓ 成功
              ┌────────┘  └────────┐
              ▼                    ▼
       [Disconnected]       ┌──────────────┐
                            │ Connected    │ ← 设备就绪
                            └──────┬───────┘
                          链路异常 ↓
                                   ▼
                            ┌──────────────┐
                            │Reconnecting  │ 退避重连 N 次
                            └──────�───────┘
                          成功 ↓   ↓ 失败 N 次
                              ▼   ▼
                       [Connected] [Disconnected]
```

**UI 反馈映射**：

| 状态 | DeviceCard 视觉 | Tag | 可点击操作 |
|---|---|---|---|
| `Disconnected` | 占位卡 + 虚线边框 | "未连接"（灰） | [去连接]（跳转 /devices） |
| `Connecting` | spinner + "连接中..." | "连接中"（warn） | 禁用 |
| `Connected` | 完整字段 | "已连接"（accent） | [断开] P2 |
| `Reconnecting` | spinner + "重连中...（3/5）" | "重连中"（warn） | 禁用 |

> ✅ 实现状态：`Connected ↔ Disconnected` 双向已实现（断连由事件驱动）；`Reconnecting` 视觉态已实装（由 `device.reconnecting` 驱动）；`Connected` 下的 [断开] 为 P2 未实装。

### 5.3 业务状态（来自仲裁，[ADR-0001](../decisions/ADR-0001)）

```
[IDLE]
    --(任意 hook 事件 WORKING/WAITING/SUCCESS/ERROR/自定义)--> [新状态]
[任意非 IDLE]
    --(hold_ms 到期 且 当前为终态)--> [IDLE]
[任意]
    --(reset_outputs)-->                                       [IDLE]
[任意非 ERROR]
    --(ERROR 事件)-->                                          [ERROR]   // 优先级抢占
```

**UI 联动**：
- 状态切换 → 立即触发 `business-state-changed` → TrafficBadge + StatusHero 联动（§3.1）
- SUCCESS/ERROR 的 `hold_ms` 倒计时由后端管理，前端**不展示倒计时进度**（V2 评估）

### 5.4 主题编辑器模式

```
[Closed] --(点 [以此主题创建])--> [Open, quick, WORKING]
[Open, quick] --(切 workbench)--> [Open, workbench, WORKING]   // 精确字段展开
[Open, workbench] --(切 quick)--> [Open, quick, WORKING]       // 精确字段隐藏
[Open, 任何模式] --(切 state-tab)--> [Open, 相同模式, 新 state] // 数据保留
[Open, 任何模式] --(点 取消/Esc)--> [Closed]                  // 丢弃修改
[Open, 任何模式] --(点 保存)--> [Closed, 写文件]               // 校验 + 持久化
[Open, 任何模式] --(点 另存为)--> [Closed + 命名 Dialog]       // 同上
```

**关键不变量**：
- 模式切换**不丢**用户已填数据（数据在 `STATE_DATA` 对象中保留）
- `editingState` 切换**不丢**当前模式的字段
- 标准 5 态不可删除；允许创建和删除自定义状态
- 用户界面不以“相位”为主要标签，使用“灯光顺序 / 出场时间”
- 取消/关闭 Dialog 时存在未保存修改 → 必须确认后才丢弃

---

## 6. L2 页面层组件

> 本章开始按统一子节模板（§1.4）逐个描述 L2 页面层组件。

### 6.1 `Sidebar`

**6.1.1 用途**
主窗口左侧固定 220px 宽侧边栏，提供 5 项一级导航 + 设置入口；底部展示连接状态 + 版本号 + 端口。

**6.1.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `currentRoute` | string，当前激活路由 |
| Props | `serviceVersion` | string |
| Props | `servicePort` | number |
| Props | `deviceConnected` | boolean |
| Emit | `onNavigate(route)` | 用户点击导航项 |
| 订阅 | `device-connection-changed` | 更新底部状态点 |
| 订阅 | `business-state-changed` | （V2：托盘菜单显示当前状态；P1 仅 sidebar 底部状态点） |

**6.1.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `default` | 未激活导航项 | fg-2 / fg-3 图标 | hover → fg + bg 微亮 |
| `active` | `currentRoute == item.route` | accent / accent-soft / accent 图标 | 同 hover |
| `disabled` | (none) | — | — |

**6.1.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `device-connection-changed` | `Sidebar.statusDot.color` | patch |
| Dark OLED（P1 固定） | (none) | — |
| 用户点击 nav-item | `currentRoute` | full |

**6.1.5 边界条件**
- 启动时 `serviceVersion = "0.0.0"` / `servicePort = 0`：显示 `--` 占位
- 端口 0：表示 hook 服务启动失败；底部红点提示

**6.1.6 无障碍**
- 导航项 = `<button>` + `aria-current="page"`
- Tab 顺序 = 视觉顺序
- Esc 在导航项聚焦时无操作

---

### 6.2 `StatusHero`

**6.2.1 用途**
Dashboard 顶部大卡：3 灯红绿灯徽章 + 状态名（大号 32px）+ 副标题（一行中文）。

**6.2.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `currentState` | `BusinessState`（IDLE/WORKING/WAITING/SUCCESS/ERROR/自定义） |
| Props | `currentOrient` | `'horizontal' \| 'vertical'` |
| Props | `sinceTs` | number（毫秒时间戳） |
| 订阅 | `business-state-changed` | 全部字段 |
| 订阅 | `update_config.badgeOrientation` | `currentOrient` |

**6.2.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `default` | 任意状态 | 3 灯按业务状态亮 / 灭 / 呼吸 / 闪烁 | 无 |
| `disconnected` | 设备未连接 | 3 灯全暗 + opacity 0.4 + 文字 "设备离线" | 无 |
| `reduced-motion` | 系统偏好 | 仅颜色变化，无呼吸/闪烁动画 | 无 |

**6.2.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `business-state-changed` | `currentState` | patch |
| `device-connection-changed` | `disconnected` 态切换 | toggle |
| `update_config.badgeOrientation` | `currentOrient` | patch |

**6.2.5 边界条件**
- `currentState` 未在主题中映射 → 走 fallback IDLE（V0.4 §3）+ Toast（仅 /preview 触发时弹）
- 自定义状态名长度 > 16 字符 → 截断 + tooltip
- 朝向切换 250ms transition（CSS）

**6.2.6 无障碍**
- 状态名 = `<h1>` + `aria-live="polite"`（业务状态变化时朗读）
- 红绿灯组 = `role="status"` + `aria-label="当前状态：<state>"`
- `prefers-reduced-motion: reduce` 时关闭呼吸/闪烁

---

### 6.3 `DeviceCard`

**6.3.1 用途**
Dashboard 与 Devices 页共用：展示当前 / 扫描到的设备。三列元数据：电量 / 信号 / 同步时间。

**6.3.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `device` | `AppState.device`（见 [ipc-contract.md §2.1](./ipc-contract.md)） |
| Props | `mode` | `'dashboard' \| 'scan-result' \| 'detail'` |
| Props | `connected` | boolean |
| Emit | `onClickConnect(address)` | 仅 `mode = 'scan-result'` |
| 订阅 | `device-connection-changed` | `connected` / `address` / `name` |
| 订阅 | `device-power-changed` | `batteryPercent` / `powerSource` / `chargeState` |
| 订阅 | `device-fault` | `source` / `code` / `context` |

**6.3.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `disconnected` | `!connected` | 占位卡 + 虚线边框 + "未连接" tag | hover → [去连接] |
| `connecting` | `state == 'Connecting'` | spinner + "连接中..." | 禁用 |
| `connected` | `connected == true` | 完整字段 + "已连接" tag（accent） | hover → [断开] P2 |
| `reconnecting` | `state == 'Reconnecting'` | spinner + "重连中...(N/M)" | 禁用 |
| `charging` | `chargeState == 'CHARGING'` | 电池格 + ⚡ 充电图标 | 同 `connected` |
| `full` | `chargeState == 'FULL'` | 电池格 100% + ✓ | 同 `connected` |
| `lowBattery` | `batteryPercent < 20` | 电池格 warning 色 + Toast 警告 | 同 `connected` |
| `fault` | 收到 `device-fault` | 红色故障指示条 + tooltip 显示 source/code | 同 `connected` |

**6.3.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `device-connection-changed` | `connected` / `address` / `name` | full |
| `device-power-changed` | `batteryPercent` / `powerSource` / `chargeState` | patch |
| `device-fault` | 故障指示 + Alert 追加 | append |
| `update_config` | (none) | — |

**6.3.5 边界条件**
- 无电池版：`batteryPercent = null` → 电量格显示 "无电池"
- `batteryPercent == 0xFF`（未标定） → 显示 `--`
- `rssi` 不可用 → 信号条显示 0 格 + "信号未知"
- `sinceTs > 30s` 未更新 → 同步时间显示 "30+ 秒前"（warning 色）

**6.3.6 无障碍**
- 整个卡 = `<article>` + `aria-label`
- 电池格 = `<progress>` + `aria-valuenow`
- 信号条 = 纯视觉（无文本等价）；色盲时仅靠格数

---

### 6.4 `ThemeCard`

**6.4.1 用途**
Dashboard 主题卡 + Themes 页主题网格卡：3 灯条色块缩略 + 主题名 + 描述 + [使用此主题] / [正在使用] 按钮。

**6.4.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `theme` | `{ name, builtin, description, previewColors: [color × 3] }` |
| Props | `isActive` | boolean |
| Props | `mode` | `'dashboard' \| 'grid'` |
| Emit | `onClickApply(themeName)` | 仅 `mode = 'grid'` |
| Emit | `onClickChangeTheme` | 仅 `mode = 'dashboard'` → 跳转 `/themes` |
| 订阅 | `theme-changed` | `isActive` |
| 订阅 | `get_themes` | 网格全量 |

**6.4.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `default` | 非当前主题 | bg-elev + border-soft | hover → border + shadow |
| `active` | `isActive == true` | accent 边框 + box-shadow-glow + "当前使用" tag | 同 hover |
| `applying` | 点击 [使用此主题] 后 | button = loading 态（spinner） | 禁用 |
| `error` | `set_active_theme` 失败 | button 抖动 + 错误 Toast | hover → border-destructive |

**6.4.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `theme-changed` | `isActive` | patch |
| `get_themes` | 网格全量 | full |
| 用户点击 [使用此主题] | `isActive` 本地乐观更新 → 等待 event 确认 | patch |

**6.4.5 边界条件**
- 用户主题导入成功 → 自动刷新网格
- 用户主题删除 → 自动移除卡
- `previewColors` 缺失 → 显示默认 3 灰块

**6.4.6 无障碍**
- 卡片 = `<button>`（整卡可点）+ `aria-pressed="isActive"`
- [使用此主题] 按钮 = `<button type="button">`

---

### 6.5 `IntegrationCard`

**6.5.1 用途**
Integrations 页的客户端配置卡（Claude Code / Codex / Qoder / Cursor）。

**6.5.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `client` | `{ id, name, status, configPath, configSnippet, helpText }` |
| Props | `status` | `'configured' \| 'unconfigured' \| 'reserved'` |
| Emit | `onClickTestConnection(clientId)` | 触发测试连接 |
| Emit | `onClickCopy(content)` | 复制配置代码 |
| Invoke | `trigger_state('WORKING', { source: clientId })`（[测试连接]） | — |

**6.5.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `configured` | `status == 'configured'` | accent tag "已配置" + 测试连接 enabled | hover |
| `unconfigured` | `status == 'unconfigured'` | warn tag "未配置" + 测试连接 enabled | hover |
| `reserved` | `status == 'reserved'` | 灰 tag "预留" + opacity 0.6 | 禁用 |
| `testing` | [测试连接] 点击后 | button = loading 态 | 禁用 |
| `error` | 测试失败 | button 抖动 + Toast | hover |

**6.5.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `business-state-changed` | (none) | — |
| `update_config` | (none) | — |

**6.5.5 边界条件**
- Codex Desktop 用户：tag 改为 "不兼容" + 灰显（KAD-04 提示）
- 文件路径不存在：`status` 检测时显示 "配置文件未找到"
- [测试连接] 失败 → Toast "测试失败：5 秒内未看到灯效变化"

**6.5.6 无障碍**
- 折叠区 = `<details>` / `<summary>`
- 复制按钮 = `<button>` + `aria-live` 复制成功提示

---

### 6.6 `SettingRow`

**6.6.1 用途**
Settings 页分组内的单行设置项：左图标 + 名称 + 可选说明，右控件（Switch / SegControl / ModeOption 卡片 / StatusTag / 主题预览入口等）。

**6.6.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `icon` | Lucide 图标 |
| Props | `title` | string（行名称） |
| Props | `description?` | string（用户友好说明，可省略） |
| Props | `stacked?` | boolean（控件改为整行铺开，仲裁模式卡片用） |
| Props | `children` | React 子节点（控件） |
| Emit | 由 control 触发 | — |
| Invoke | `update_config(patch)` | 由控件 change 触发 |

**6.6.3 视觉态全集**

| 态 | 触发条件 | 视觉 | 可交互 |
|---|---|---|---|
| `default` | 初始 | bg-elev + 描述 fg-2 | 控件交互 |
| `disabled` | `disabled == true` 或控制权不在用户 | opacity 0.5 | 禁用 |
| `saving` | `update_config` 调用中 | 控件 loading | 禁用 |
| `error` | `update_config` 失败 | Toast + 控件回滚到原值 | 同 default |

**6.6.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `update_config` 成功 | 应用新值到 UI | patch |
| `update_config` 失败 | 回滚 + Toast | patch |

**6.6.5 边界条件**
- 端口切换：需重启服务 → Toast "服务重启中..." → 成功后 Toast "端口已切换"
- 自启动：✅ 已实装（tauri-plugin-autostart 2.5.1，KAD-09）；失败路径 `AUTOSTART_FAILED` → Toast + 控件回滚到原值；OS 登录项为唯一事实源，config 为启动校准缓存
- 仲裁模式：ModeOption 卡片独占选择（`aria-pressed`）；「优先级抢占」带「默认」标签；切换经 `update_config(arbitrationMode)` 即时生效

**6.6.6 无障碍**
- 行名 = 可见 `<strong>`；控件自带 `aria-label`（如"仲裁模式""开机自启"）
- 说明为纯展示文本，不承担控件命名职责

---

## 7. L3 区域层组件

### 7.1 Dashboard 区域

**用途**：3 个 L2 组件的栅格布局规则

**组件顺序**（视觉顺序）：
```
┌─────────────────────────┐
│       StatusHero        │
├─────────────┬───────────�
│ DeviceCard  │ ThemeCard │
└─────────────┴───────────┘
```

**联动**：3 个组件均订阅 `business-state-changed` / `device-*` / `theme-changed`，但**无交叉联动**（彼此独立）。

**空态**：
- DeviceCard `disconnected` → "去连接" 按钮 hover → focus
- ThemeCard `default` → 正常显示

**错误态**：见 §4。

---

### 7.2 Devices 区域

**组件顺序**：
```
┌─────────────────────────┐
│  标题 + [重新查找] 按钮  │
├─────────────────────────┤
│     ScanProgress        │ ← 扫描中显示
├─────────────────────────┤
│   ScanResultList        │ ← 卡片列表
├─────────────────────────┤
│    DeviceDetailCard     │ ← 已连接设备
├─────────────────────────┤
│     FaultAlert × N      │ ← 故障列表
└─────────────────────────┘
```

**联动**：
- ScanProgress ↔ ScanResultList（同一扫描的进度 + 结果）
- DeviceDetailCard 独立
- FaultAlert 独立

**边界条件**：
- 扫描超时（5s 无结果）→ ScanProgress 隐藏 + ScanResultList 显示空态
- 蓝牙权限被拒 → 顶部红色 Alert + [重试] 按钮
- 扫描中点击 [重新查找] → 取消当前扫描 + 重新发起

---

### 7.3 Themes 区域

**组件顺序**：
```
┌─────────────────────────┐
│ 标题 + [编辑当前主题]    │
│ 标题 + [导入新主题]      │
├─────────────────────────┤
│       ThemeGrid         │ 3 列网格
├─────────────────────────┤
│    ThemeDetailPanel     │ （V2）选中主题时展开
└─────────────────────────┘
```

**联动**：
- ThemeGrid 点击 → 设置当前选中主题（本地 state）→ DetailPanel 同步
- [编辑当前主题] → 打开 ThemeEditorDialog（独占模式）
- [导入新主题] → 打开 ImportThemeDialog（独占模式）

**Dialog 层级**：ThemeEditor > ImportTheme > 主窗口；同一时刻仅一个 Dialog。

---

### 7.4 Integrations 区域

**组件顺序**：
```
┌─────────────────────────┐
│ 标题 + 描述              │
├─────────────────────────┤
│ IntegrationCard × 4    │
├─────────────────────────┤
│     HelpFooter          │ "这些配置在做什么？" 解释卡
└─────────────────────────┘
```

**联动**：4 个 IntegrationCard 完全独立；HelpFooter 纯静态。

---

### 7.5 Preview 区域

**组件顺序**：
```
┌─────────────────────────┐
│ 标题 + 当前主题显示      │
├─────────────────────────┤
│ StandardStateButtonGroup│ 5 标准按钮（IDLE/WORKING/WAITING/SUCCESS/ERROR）
├─────────────────────────┤
│   CustomStateInput      │ 输入框 + [触发]
├─────────────────────────┤
│ CustomStateQuickList    │ 最近 5 个自定义状态
├─────────────────────────┤
│  ResetOutputsButton     │ "全部重置"
└─────────────────────────┘
```

**联动**：
- StandardStateButtonGroup 点击 → `trigger_state` → `business-state-changed` → TrafficBadge 联动（Dashboard 也会变）
- CustomStateInput Enter 键 → 同点击 [触发]
- CustomStateQuickList 点击 → 同上 + 同时把名字加入最近列表（FIFO）
- ResetOutputsButton → `reset_outputs` → 全停 + 业务复位 IDLE

**快捷键**（详见 [ui-interactions.md 附录 A.8](./ui-interactions.md)）：
- `1`~`5`：标准状态
- `0`：全部重置
- `Esc`：清空输入框 focus

**边界条件**：
- 设备未连接 → 所有按钮 disabled + Tooltip "请先连接设备"
- 主题映射缺失自定义状态 → Toast "该状态未在主题中映射"

---

### 7.6 Settings 区域

**组件顺序**：
```
┌─────────────────────────┐
│  标题                    │
├─────────────────────────┤
│ SettingGroup: 服务       │ port（只读）+ arbitrationMode + 接入保护状态
├─────────────────────────┤
│ SettingGroup: 显示       │ badgeOrientation + 当前主题
├─────────────────────────┤
│ SettingGroup: 系统       │ autostart（✅ 可切换）
└─────────────────────────┘
```

**联动**：每 SettingGroup 内的 SettingRow 互不联动；Group 间独立。

**7.6.1 服务组**：服务端口（只读数值）；仲裁模式 = 两张 ModeOption 卡片（整行铺开，单选圆点 + 效果说明 + 「默认」标签）；接入保护 = 状态标签（"仅限本机" / "Token 已启用"）。

**7.6.2 显示组**：灯组朝向（SegControl 横排/纵向）；当前主题 = 主题预览入口（Link → /themes）：3 个灯色圆点（取当前主题 WORKING/SUCCESS/ERROR 场景实际 `leds.high`/`low` 色）+ 主题名 + 可选「提示音」标记 + ChevronRight。预览随 `config.activeTheme` 变化刷新；主题读取失败回退为纯名称。

**7.6.3 系统组**：开机自启 Switch（`aria-checked` + 开启态视觉 = 品牌绿底 + 滑块右移 16px）。

---

## 8. L4 通用组件库

> 本章列出所有 L4 可复用组件。每个组件使用统一子节模板（§1.4）。视觉具体由 ui-design.md 代币落地，本文只描述行为。

### 8.1 `LightDot`（红绿灯灯位）

**8.1.1 用途**
单颗红绿灯灯位（圆形 40px 横排 / 28px 竖排）。在 TrafficBadge、state-tab dots、preview 按钮缩略图等位置复用。

**8.1.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `color` | `'red' \| 'yellow' \| 'green' \| 'off'` |
| Props | `animation` | `'none' \| 'breath' \| 'blink'` |
| Props | `size` | `'normal' \| 'compact'`（normal=40px, compact=28px） |

**8.1.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | `color != 'off'` | radial-gradient + inset shadow |
| `off` | `color == 'off'` | var(--muted) + opacity 0.42 |
| `breath` | `animation == 'breath'` | 2s ease-in-out infinite（opacity 1 � 0.62） |
| `blink` | `animation == 'blink'` | 1Hz 闪烁（50% on / 50% off） |
| `reduced-motion` | 系统偏好 | 移除 breath / blink 动画 |

**8.1.4 联动矩阵**：纯展示组件，由父组件传入 props。

**8.1.5 边界条件**
- `color` 非法值 → fallback `'off'`
- `animation == 'breath'` 仅当 `color == 'green'` 有效

**8.1.6 无障碍**
- `role="status"` + `aria-label="<color> 灯"`
- `prefers-reduced-motion: reduce` → 移除所有动画类

---

### 8.2 `TrafficBadge`（3 灯组合徽章 + 朝向）

**8.2.1 用途**
Dashboard StatusHero 内的红绿灯徽章；支持横排 / 竖排。

**8.2.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `state` | `BusinessState` |
| Props | `orient` | `'horizontal' \| 'vertical'` |
| 订阅 | `business-state-changed` | `state` |
| 订阅 | `update_config.badgeOrientation` | `orient` |

**8.2.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `horizontal` | `orient == 'horizontal'` | 3 灯横排，灯心距 32px |
| `vertical` | `orient == 'vertical'` | 3 灯竖排，灯心距 20px |
| `disconnected` | 设备未连接 | 3 灯全 off + opacity 0.4 |

**8.2.4 联动矩阵**：见 §3.1

**8.2.5 边界条件**
- 朝向切换：CSS transition 250ms ease-out

**8.2.6 无障碍**
- 父 `<div role="status" aria-live="polite">`
- 内部 3 灯 = 3 个 LightDot（各自 aria-label）

---

### 8.3 `StateTab`（主题创作器状态切换）

**8.3.1 用途**
主题创作器内标准 5 态 + 用户自定义状态 tab。

**8.3.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `state` | `BusinessState` |
| Props | `isActive` | boolean |
| Props | `previewColors` | `[color × 3]`（3 灯色块缩略） |
| Emit | `onClick(state)` | 切换 editingState |

**8.3.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 非激活 | bg-elev + border-soft |
| `active` | `isActive == true` | accent-soft + accent border |
| `hover` | 鼠标悬停 | border 提升 |

**8.3.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 用户点击 | `editingState` | patch |
| `theme-changed` | 预览色块 | patch |

**8.3.5 边界条件**
- 标准 5 态严格按固定顺序且不可删除
- 自定义状态显示在标准状态之后，可添加、删除

**8.3.6 无障碍**
- 5 个 tab = `<button>` + `aria-pressed`
- `role="tablist"` 父容器（若实现完整 ARIA tabs 模式）

---

### 8.4 `MotionPresetCard`（6 个运动预设）

**8.4.1 用途**
快速创作第一步：常亮 / 呼吸 / 闪烁 / 流动 / 渐亮 / 渐弱。协议曲线名不对用户展示。

**8.4.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `motion` | `steady / breathe / blink / flow / fade-in / fade-out` |
| Props | `isActive` | boolean |
| Props | `previewColor` | string（hex） |
| Emit | `onClick(motion)` | 生成对应三灯轨道参数 |

**8.4.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 非激活 | bg + border-soft + 波形 SVG |
| `active` | `isActive == true` | accent-soft + accent border + box-shadow 1px accent |
| `hover` | 鼠标悬停 | border 提升 |

**8.4.4 联动矩阵**：纯展示 + 触发。

**8.4.5 边界条件**
- 6 种运动效果严格按固定顺序
- SINE 协议枚举预留但 UI 不暴露（V0.4 §7.2）

**8.4.6 无障碍**
- `<button>` + `aria-pressed`

---

### 8.5 `ColorPickerRow`（灯轨色行）

**8.5.1 用途**
灯轨颜色编辑：low color picker + high color picker + brightness slider。

**8.5.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `trackLabel` | `'顶' \| '中' \| '底'` |
| Props | `lowColor` | string |
| Props | `highColor` | string |
| Props | `brightness` | number (0~100) |
| Props | `quickMode` | boolean（隐藏精确协议参数） |
| Emit | `onChangeLow(color)` / `onChangeHigh(color)` / `onChangeBrightness(b)` | — |

**8.5.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | 60px 标签 + 3 列 picker / slider |
| `quickMode` | `quickMode == true` | 只显示用户可理解的颜色与强度控制 |

**8.5.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `editingState` 切换 | 重置为新状态的当前值 | full |

**8.5.5 边界条件**
- CONSTANT 波形时：`lowColor` picker 禁用 + 提示 "CONSTANT 模式下忽略"
- `brightness == 0` → 视觉上等于 off；tooltip "亮度为 0 = 全黑"
- 颜色非法（如空字符串）→ fallback `#000000`

**8.5.6 无障碍**
- 颜色 picker = `<input type="color">` + `<label>`
- 滑块 = `<input type="range">` + `aria-label`

---

### 8.6 `BrightnessSlider`（亮度滑块 0-100）

**8.6.1 用途**
灯轨亮度调节。

**8.6.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `value` | number (0~100) |
| Emit | `onChange(value)` | — |

**8.6.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | thumb = accent + glow |
| `hover` | 鼠标悬停 thumb | thumb 略放大 |
| `disabled` | CONSTANT 波形且锁定 | opacity 0.5 |

**8.6.4 联动矩阵**：纯交互。

**8.6.5 边界条件**
- `value == 0` → 视觉显示为 0%；不允许负数

**8.6.6 无障碍**
- `<input type="range" min="0" max="100">` + `aria-valuenow`

---

### 8.7 `EntryTimingSlider`（出场时间 0-360°）

**8.7.1 用途**
轨道工作台的精确出场时间滑块。角度只是精确值，主标签不得显示“相位”。

**8.7.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `value` | number (0~360) |
| Emit | `onChange(value)` | — |

**8.7.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | thumb = accent |
| `snap` | 值靠近预设（0/120/240） | thumb 吸附 + 视觉提示 |

**8.7.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| LightOrderPreset 切换 | `value` 设置为预设组合 | patch |

**8.7.5 边界条件**
- `value > 360` → clamp 到 360
- `value < 0` → clamp 到 0
- 用户拖动后 → 灯光顺序进入自定义

**8.7.6 无障碍**
- `<input type="range">` + `aria-label="该灯在一轮动画中的出场位置"`

---

### 8.8 `BuzzerSwitch`（蜂鸣开关）

**8.8.1 用途**
蜂鸣器总开关。

**8.8.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `on` | boolean |
| Emit | `onChange(on)` | — |

**8.8.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | `on == false` | bg-secondary |
| `on` | `on == true` | accent + thumb 右侧 |
| `hover` | 鼠标悬停 | thumb 略放大 |

**8.8.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 用户切换 | `on` | patch |

**8.8.5 边界条件**
- 设备无蜂鸣能力（`PASSIVE_BUZZER` 位未置位）→ 显示禁用 + tooltip "设备无蜂鸣器"

**8.8.6 无障碍**
- `<button role="switch" aria-checked>` + 键盘 Space 切换

---

### 8.9 `BuzzerSegmentChip`（蜂鸣片段 chip）

**8.9.1 用途**
蜂鸣轨道内的单个音调 / 静音片段。

**8.9.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `index` | number（1~16） |
| Props | `freq` | number (Hz) |
| Props | `duration` | number (ms) |
| Props | `volume` | number (0~100) |
| Emit | `onEdit(index)` | 点击 chip 弹出编辑 popover |
| Emit | `onDelete(index)` | — |

**8.9.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | bg + border-soft + 3 字段并列 |
| `hover` | 鼠标悬停 | border 提升 + 删除图标浮现 |
| `edit` | 正在编辑 | accent border |

**8.9.4 联动矩阵**：纯交互。

**8.9.5 边界条件**
- 最多 16 段（V0.4 协议硬上限）
- `freq == 0` → 显示为 "静音" + 灰色
- 字段超出设备能力范围 → 红色边框 + tooltip

**8.9.6 无障碍**
- 整个 chip = `<button>` + `aria-label="第 N 段：<freq>Hz <duration>ms 音量 <vol>%"`

---

### 8.10 `LightOrderPreset`（灯光顺序预设）

**8.10.1 用途**
快速创作和轨道工作台共用的四个直觉预设：一起 / 从上往下 / 从下往上 / 交错。

**8.10.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `active` | `'together' \| 'top-down' \| 'bottom-up' \| 'staggered' \| 'custom'` |
| Emit | `onClick(value)` | — |

**8.10.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 非激活 | bg + fg-2 |
| `active` | 当前预设 | accent-soft + accent border + accent text |
| `hover` | 鼠标悬停 | border 提升 |

**8.10.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| EntryTimingSlider 自定义值 | `active = 'custom'` | patch |
| 用户点击预设 | 三灯 `phase_deg` 写入对应组合 | patch |

**8.10.5 边界条件**
- 4 个预设严格按固定顺序

**8.10.6 无障碍**
- `<button>` + `aria-pressed`

---

### 8.11 `SegControl`（segment control）

**8.11.1 用途**
分段单选控件（themeMode 暗/亮/自动 等）。

**8.11.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `options` | `[{ value, label }]` |
| Props | `value` | 当前选中 |
| Emit | `onChange(value)` | — |

**8.11.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | 圆角容器 + N 个按钮 |
| `selected` | `btn.value == value` | bg-secondary + 文本高亮 |
| `hover` | 鼠标悬停 | 按钮 bg 微亮 |

**8.11.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `update_config.badgeOrientation` | 当前 value | patch |

**8.11.5 边界条件**
- N >= 2；典型 3 个
- 选项 label 过长 → 截断 + tooltip

**8.11.6 无障碍**
- 容器 = `role="radiogroup"`
- 选项 = `<button role="radio" aria-checked>`

---

### 8.12 `Switch`（通用开关）

**8.12.1 用途**
通用开关（autostart / buzzer 等）。

**8.12.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `on` | boolean |
| Props | `disabled` | boolean |
| Emit | `onChange(on)` | — |

**8.12.3 视觉态全集**：同 BuzzerSwitch

> ✅ 已落地：开启态 = `background: var(--accent)` + 滑块 `translateX(16px)`（`.switch[aria-checked="true"]`）；保存中 `disabled` 半透明。

**8.12.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `update_config` 成功 | 当前 on 状态 | patch |
| `update_config` 失败 | 回滚 + Toast | patch |

**8.12.5 边界条件**
- `disabled == true` → 鼠标悬停仍显示焦点环但不响应

**8.12.6 无障碍**
- `<button role="switch" aria-checked aria-disabled>` + Space 切换

---

### 8.13 `Select`（通用选择器）

**8.13.1 用途**
下拉选择器（主题编辑器 end_level / 场景选择等）。仲裁模式已改用 ModeOption 卡片（见 §7.6.1），不再使用 Select。

**8.13.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `options` | `[{ value, label }]` |
| Props | `value` | 当前选中 |
| Props | `placeholder` | string |
| Emit | `onChange(value)` | — |

**8.13.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | bg + border-soft |
| `open` | 下拉展开 | 列表 + 当前选中 highlight |
| `disabled` | 不可用 | opacity 0.5 |

**8.13.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `update_config` 成功 | 当前 value | patch |
| `update_config` 失败 | 回滚 + Toast | patch |

**8.13.5 边界条件**
- 选项列表为空 → 显示 "无可选项"
- 键盘 ↑/↓ 切换选项；Enter 确认；Esc 关闭

**8.13.6 无障碍**
- `<button aria-haspopup="listbox">`
- 列表 = `role="listbox"` + 选项 = `role="option"`

---

### 8.14 `Input`（通用输入）

**8.14.1 用途**
文本输入（主题名 / 自定义状态名 等）。

**8.14.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `value` | string |
| Props | `placeholder` | string |
| Props | `maxLength` | number |
| Props | `pattern` | RegExp（如 `[a-zA-Z0-9_-]+`） |
| Props | `error` | string \| null |
| Emit | `onChange(value)` / `onSubmit(value)` | — |

**8.14.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | bg-2 + border-soft |
| `focus` | 聚焦 | accent border + ring |
| `error` | `error != null` | destructive border + 下方错误文字 |
| `disabled` | 不可用 | opacity 0.5 |

**8.14.4 联动矩阵**：纯交互。

**8.14.5 边界条件**
- `pattern` 校验失败 → 提交时阻止 + 显示错误
- 超 `maxLength` → 截断（不弹错）

**8.14.6 无障碍**
- `<input>` + `<label for>` 关联
- `aria-invalid="true"` 当 error

---

### 8.15 `Toast`（Sonner 通用反馈）

**8.15.1 用途**
通用 Toast（错误 / 成功 / 信息）。

**8.15.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `type` | `'error' \| 'success' \| 'info' \| 'warning'` |
| Props | `message` | string |
| Props | `duration` | number（ms，默认 4000） |
| Props | `action` | `{ label, onClick }` \| null |
| API | `toast.error(msg, opts?)` / `toast.success(...)` / 等 | 全局 API |

**8.15.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `error` | `type == 'error'` | 左侧红条 + 错误图标 |
| `success` | `type == 'success'` | 左侧绿条 + ✓ |
| `info` | `type == 'info'` | 左侧灰条 + ℹ |
| `warning` | `type == 'warning'` | 左侧黄条 + ⚠ |

**8.15.4 联动矩阵**：无（被外部调用）。

**8.15.5 边界条件**
- 多 Toast 堆叠：最多 3 个同时显示，超出排队
- 用户点击 × 立即关闭
- `duration == 0` = 不自动关闭（需用户手动）

**8.15.6 无障碍**
- `role="status"` 或 `role="alert"`（error/warning 用 alert）
- `aria-live="polite" / "assertive"`

---

### 8.16 `Dialog`（模态对话框）

**8.16.1 用途**
模态对话框（ThemeEditor / ImportTheme / 断开确认 等）。

**8.16.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `open` | boolean |
| Props | `title` | string |
| Props | `description` | string |
| Props | `children` | React 子节点 |
| Props | `footer` | React 子节点（按钮组） |
| Props | `onClose` | () => void |
| Props | `maskClosable` | boolean（默认 true） |

**8.16.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `closed` | `open == false` | 隐藏 |
| `opening` | 打开过渡中 | fade-in 150ms |
| `open` | 打开 | 居中 + backdrop blur |
| `closing` | 关闭过渡中 | fade-out 150ms |

**8.16.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 用户点 Esc / 关闭按钮 | `open = false` | patch |
| 用户点 backdrop | `open = false`（若 maskClosable） | patch |

**8.16.5 边界条件**
- 同时仅一个 Dialog 打开（新的覆盖旧的）
- Esc 关闭最上层
- 打开时禁止主窗口滚动

**8.16.6 无障碍**
- `role="dialog" aria-modal="true" aria-labelledby aria-describedby`
- 焦点陷阱（Tab 在 Dialog 内循环）
- 打开时焦点移到第一个可交互元素；关闭时焦点回到触发元素

---

### 8.17 `Progress`（进度条）

**8.17.1 用途**
扫描倒计时进度条。

**8.17.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `value` | number (0~100) |
| Props | `duration` | number（ms，扫描总时长） |
| Props | `indeterminate` | boolean |

**8.17.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | 4px 高条 + accent gradient |
| `indeterminate` | 不知道进度 | pulse 动画 |

**8.17.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 扫描计时器 | `value` | patch（每 100ms） |
| 扫描完成 | 隐藏 | toggle |

**8.17.5 边界条件**
- 进度条满 = 扫描完成（5s）

**8.17.6 无障碍**
- `<progress>` 语义标签

---

### 8.18 `Alert`（设备故障 / 扫描失败）

**8.18.1 用途**
错误提示卡（红色 Alert）。

**8.18.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `title` | string |
| Props | `description` | string |
| Props | `variant` | `'error' \| 'warning' \| 'info'` |
| Props | `dismissible` | boolean |
| Emit | `onDismiss()` | — |

**8.18.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `error` | `variant == 'error'` | 红色边框 + 错误图标 |
| `warning` | `variant == 'warning'` | 黄色边框 + ⚠ |
| `info` | `variant == 'info'` | 蓝色边框 + ℹ |

**8.18.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| `device-fault` | 追加 Alert | append |
| 用户点 × | 移除 | remove |

**8.18.5 边界条件**
- 多个 Alert 堆叠：最多 3 个同时显示
- 设备故障 Alert **不自动消失**（V2 加 dismiss）

**8.18.6 无障碍**
- `role="alert"` + `aria-live="assertive"`

---

### 8.19 `Tag`（状态标签）

**8.19.1 用途**
状态标签（已连接 / 未连接 / 当前使用 / 预留）。

**8.19.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `variant` | `'default' \| 'accent' \| 'warn' \| 'destructive'` |
| Props | `withDot` | boolean |
| Props | `dotAnimation` | `'pulse' \| 'static' \| 'none'` |

**8.19.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `default` | 初始 | bg + fg-2 + 边框 |
| `accent` | `variant == 'accent'` | accent-soft + accent 文本 + dot |
| `warn` | `variant == 'warn'` | warn 配色 |
| `destructive` | `variant == 'destructive'` | 红色配色 |
| `pulse` | `dotAnimation == 'pulse'` | dot 闪烁（仅 accent + warn） |

**8.19.4 联动矩阵**：纯展示。

**8.19.5 边界条件**
- 长文本截断 + tooltip

**8.19.6 无障碍**
- `<span>` 纯展示

---

### 8.20 `CodeBlock`（JSON 配置折叠展示 + 复制）

**8.20.1 用途**
Integrations 页 JSON 配置展示：折叠 + 复制。

**8.20.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `code` | string（JSON 文本） |
| Props | `language` | `'json' \| 'toml'`（默认 json） |
| Props | `collapsible` | boolean |
| Props | `defaultCollapsed` | boolean |

**8.20.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `collapsed` | 默认折叠 | 仅 summary "查看配置" |
| `expanded` | 用户点击 | pre + 复制按钮 |
| `copied` | 复制成功 | 按钮文字 "已复制" 2 秒后恢复 |

**8.20.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 用户点复制 | 写入剪贴板 | （外部） |

**8.20.5 边界条件**
- `code` 长度 > 5KB → 横向滚动 + 折叠默认展开（避免折叠无用）

**8.20.6 无障碍**
- `<details>` / `<summary>` 原生语义

---

### 8.21 `SignalBars`（4 格信号条）

**8.21.1 用途**
设备信号强度展示。

**8.21.2 对外契约**

| 类别 | 项 | 说明 |
|---|---|---|
| Props | `rssi` | number \| null |
| Props | `levels` | number（格数，默认 4） |

**8.21.3 视觉态全集**

| 态 | 触发条件 | 视觉 |
|---|---|---|
| `strong` | `rssi > -60` | 4 格全亮 accent |
| `medium` | `-60 >= rssi > -75` | 3 格亮 + 1 格灰 |
| `weak` | `-75 >= rssi > -85` | 2 格亮 + 2 格灰 |
| `poor` | `rssi <= -85` | 1 格亮 + 3 格灰 |
| `unknown` | `rssi == null` | 4 格全灰 |

**8.21.4 联动矩阵**

| Source Event | 字段 | 同步方式 |
|---|---|---|
| 设备能力变化 | `rssi` | patch |

**8.21.5 边界条件**
- RSSI 值映射：>-60 强、-60~-75 中、-75~-85 弱、<=-85 极弱

**8.21.6 无障碍**
- 视觉信号 + 文字 "信号 <strong/medium/weak/poor>"
- 色盲不影响（格数即信号强度）

---

## 9. 组件生命周期与资源清理

> V1.1 增量章节。定义每个 L4 组件的 mount / update / unmount 行为 + 资源清理契约。
> 配套代码：`useEffect` cleanup 函数、`AbortController`、`unlisten` 句柄等。

### 9.1 组件生命周期三阶段

每个有副作用的组件（L2 页面层 + L3 区域层 + L4 通用组件库中带订阅/计时器/监听的）必须实现以下三阶段契约：

```
[Mount]
  ↓ 初始化 props 派生 state
  ↓ 注册事件订阅 / 启动计时器 / 建立 IPC channel
  ↓ 触发首次渲染
[Update]
  ↓ props 变化 → 重新派生 state（useMemo / useCallback 避免不必要渲染）
  ↓ state 变化 → 触发重渲染
  ↓ 订阅 event 触发 → 同步更新 state
[Unmount]
  ↓ 取消所有事件订阅（unlisten）
  ↓ 清理所有计时器（clearTimeout / clearInterval）
  ↓ 取消所有 in-flight 请求（AbortController）
  ↓ 释放所有引用（refs = null）
```

### 9.2 资源清理检查清单

每个组件 unmount 时必须清理的资源：

| 资源类型 | 来源 | 清理方式 |
|---|---|---|
| Tauri event 订阅 | `listen(eventName, handler)` | 返回的 `UnlistenFn` 在 cleanup 中调用 |
| Tauri command 调用 | `invoke(cmd, args)` | AbortController 或 timeout |
| `setTimeout` / `setInterval` | 扫描倒计时、Toast 关闭、Toast 自动消失 | `clearTimeout` / `clearInterval` |
| `requestAnimationFrame` | 呼吸/闪烁动画 | `cancelAnimationFrame` |
| 焦点监听 | `document.addEventListener('focus', ...)` | `removeEventListener` |
| 全局键盘监听 | Dialog 快捷键、Preview 数字键 | `removeEventListener('keydown', ...)` |
| `ResizeObserver` / `IntersectionObserver` | 响应式布局 | `disconnect()` |
| `WebSocket` / `EventSource` | V2 实时推送 | `close()` |
| 第三方库实例 | Sonner / Tailwind / Lucide | 包内自带 cleanup（无需手动） |
| 内部 ref | DOM ref、对象 ref | `ref.current = null` |

### 9.3 事件订阅的精确生命周期

```typescript
// 推荐模式：useEffect + listen + cleanup
useEffect(() => {
  const unlisten = listen<TEventPayload>('event-name', (event) => {
    // 处理事件
  });
  return () => {
    unlisten.then((fn) => fn());  // 取消订阅
  };
}, []);  // 空依赖：仅 mount/unmount 时执行

// 多事件订阅模式
useEffect(() => {
  const unlistens: UnlistenFn[] = [];
  listen('event-a', handlerA).then((fn) => unlistens.push(fn));
  listen('event-b', handlerB).then((fn) => unlistens.push(fn));
  return () => {
    unlistens.forEach((fn) => fn());
  };
}, []);
```

### 9.4 计时器与异步请求的统一模式

```typescript
useEffect(() => {
  const controller = new AbortController();
  const timer = setTimeout(async () => {
    try {
      await invoke('some_command', { args }, { signal: controller.signal });
    } catch (err) {
      if (err.name !== 'AbortError') {
        toast.error('操作失败');
      }
    }
  }, 1000);
  return () => {
    clearTimeout(timer);
    controller.abort();
  };
}, [deps]);
```

### 9.5 焦点与键盘监听的清理

Dialog / Preview 页等需要全局键盘监听的组件：

```typescript
useEffect(() => {
  if (!open) return;
  const handler = (e: KeyboardEvent) => {
    if (e.key === 'Escape') onClose();
    if (e.key === '1') handleState('IDLE');
    // ...
  };
  document.addEventListener('keydown', handler);
  return () => document.removeEventListener('keydown', handler);
}, [open, onClose]);
```

### 9.6 Toast 的自动关闭

Toast 组件（Sonner）自带 lifecycle 管理：

- 显示 → duration ms 后自动关闭（默认 4000）
- 错误 Toast 默认 duration = 6000（多给用户读时间）
- 用户点击 × 立即关闭
- `duration = 0` 表示不自动关闭（必须用户手动）
- 多个 Toast 堆叠：最多 3 个同时显示；超出排队等待
- 路由切换时：当前页面的 Toast 不主动清理（Sonner 默认跨页面持续），V2 评估是否路由切换时清空

### 9.7 Dialog 的焦点陷阱与还原

打开 Dialog 时：
1. 记录当前 focus 元素：`document.activeElement`
2. 移动焦点到 Dialog 内第一个可交互元素
3. Tab 键在 Dialog 内循环（不跳出）

关闭 Dialog 时：
1. 焦点还原到打开前的元素
2. 移除全局键盘监听（Esc 关闭）

### 9.8 组件实例复用与重置

某些 L4 组件在 props 变化时不重新 mount 而是复用，需显式重置 state：

| 组件 | props 变化 | 重置行为 |
|---|---|---|
| `ThemeEditor` | `editingState` 变化 | 局部 state 重置；保留 `STATE_DATA` |
| `Dialog` | `open` true→false→true | 子组件 state 重置（key 变化） |
| `ScanResultList` | `scanning` false→true | 清空上次结果 |
| `LightDot` | `color` 变化 | CSS 过渡自动；无需手动 |

### 9.9 内存泄漏自检清单

每次 PR 涉及组件改动时，由 reviewer 检查：

- [ ] 所有 `listen` 都有对应 `unlisten`
- [ ] 所有 `setTimeout` / `setInterval` 在 cleanup 中清理
- [ ] 所有 `addEventListener` 在 cleanup 中移除
- [ ] 所有 async 操作有 AbortController 或 timeout 保护
- [ ] Dialog 关闭后焦点正确还原
- [ ] 没有循环引用（如 useRef 引用自己）
- [ ] 大对象（如主题 JSON 完整内容）在 unmount 时解除引用

---

## 10. 变更日志

| 版本 | 日期 | 变更 |
|---|---|---|
| V1.13 | 2026-08-21 | 设置页 UI 对账（以代码为事实源，用户触发审计）：§3.6 `arbitrationMode` 联动目标更新为选项卡片；§6.6 `SettingRow` 契约改为 `icon/title/description?/stacked?/children`；§7.6 Settings 区域补充仲裁模式选项卡片（ModeOption）与当前主题预览入口；§8.12 Switch 补开启态视觉；§8.13 Select 注明仲裁模式已改用卡片；§7.5 快捷键引用从失效的 §13.8 修正为附录 A.8。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5（含新增 `open-config`）；§4.1 AppError.code 均在 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003/0004、KAD-04/06/08/09 引用有效。 |
| V1.12 | 2026-08-21 | G-06 开机自启实装对账（KAD-09 / ADR-0004）：§3.6 配置层联动表新增 `autostart` 行并移除 P2 标注；§6.6.5 边界条件改为真实切换 + `AUTOSTART_FAILED` 失败路径；§7 Settings 组 autostart 由禁用态改为可切换。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4（含新增 `AUTOSTART_FAILED`）；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003/0004、KAD-04/06/08/09 引用有效。同步修正版本头漂移（V1.10 → V1.12）。 |
| V1.11 | 2026-08-21 | 断连 UX 闭环：§3.2 payload 增加 `reconnecting` / `reason`（值域 `link_lost` / `reconnect_failed`，由 Rust 侧 emit）；§4.4、§5.2 更新为前端 `Reconnecting` 视觉态与 Toast 已实装。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003、KAD-04/06/08 引用有效。 |
| V1.10 | 2026-08-21 | G-04 托盘实装对账：§3.6 新增 `config-changed` 事件联动（✅，设置页与托盘共用 update_config 路径）；§5.1 窗口可见性状态机标注已实装（托盘「显示窗口」/「退出」/关窗隐藏/单实例聚焦）。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003、KAD-04/06/08 引用有效。 |
| V1.9 | 2026-08-21 | 实现状态对账（G-01~G-03 闭环）：§3.2~§3.4 事件实现状态更新为 ✅；§3.7 协议主动事件全部接线（BUTTON_EVENT 仅日志）；§4.3 握手阶段 1~8 已实现；§4.4 断连监听与退避重连已实现（前端 Reconnecting 视觉态待办）；§5.2 `Connected ↔ Disconnected` 双向已实现。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003、KAD-04/06/08 引用有效。 |
| V1.8 | 2026-08-21 | 实现状态对账（以代码为事实源，用户触发审计）：§3.2~§3.4、§3.7 标注事件未接线（`device-connection-changed` 仅连接方向；`device-power-changed` / `device-fault` / 协议主动事件未 emit）；§4.3 握手失败路径标注部分实现；§4.4 断连宽限、§5.2 `Reconnecting` 标注未实现。5 项语义硬检查通过：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0003、KAD-04/06/08 引用有效。 |
| V1.0 | 2026-08-20 | 首版：6 层金字塔 + 全局联动矩阵 + 失败路径矩阵 + 4 状态机 + L2/L3/L4 共 33 个组件详表 |
| V1.1 | 2026-08-20 | 增量：§9 组件生命周期与资源清理（mount/update/unmount 三阶段 + 资源清理检查清单 + 6 个常见模式 + 内存泄漏自检清单） |
| V1.2 | 2026-08-20 | 增量：AGENTS.md 新增"触发式双文档审计"条款——ipc-contract / theme-format / 蓝牙 V0.4 / ADR 变更前必须强制对齐 ui-interactions.md 与 ui-interaction-spec.md |
| V1.3 | 2026-08-20 | 重构：废除"季度 + 触发式"双条款，改为单一"内容驱动审计"——5 个内容信号触发（会话入口 / 变更前 / 变更后自动 / 用户触发 / 漂移信号），无时间边界 |
| V1.4 | 2026-08-20 | 对齐报告：前端实现后完成 5 项语义硬检查。§3 Source Events 全部存在于 ipc-contract §5；§4.1 AppError.code 全部存在于 ipc-contract §4；§4.2 result code 与蓝牙 V0.4 §3.6 一致；§6~§8 主题字段与 theme-format 一致；ADR / KAD 引用均可解析。修复 `badgeOrientation` IPC、`INTERNAL` 错误码与导航计数漂移。 |
| V1.5 | 2026-08-21 | 主题创作器重构：快速创作隐藏协议术语，以运动、速度、灯序和声音预设生成 SCENE；轨道工作台将相位改述为出场时间；状态 tab 支持自定义状态。对齐报告：§3 Source Events、§4.1 AppError、§4.2 result code、§6~§8 theme-format 字段和 ADR/KAD 引用五项均通过；`brightness` / `volume` 统一为 0~100。 |
| V1.6 | 2026-08-21 | 对齐报告：Theme JSON Schema 与主题指南落地后完成 5 项语义硬检查。§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 与 ipc-contract §4 一致；§4.2 result code 与蓝牙 V0.4 §3.6 一致（保留值 0x08 无 UI 行为）；§6~§8 使用的主题字段均存在于 theme-format 与 Theme Schema；ADR-0001/0003、KAD-04 引用有效。同步修正文档版本头漂移。 |
| V1.7 | 2026-08-21 | 对齐报告：主题定义迁移为 Rust DTO + JsonSchema 单一来源后完成 5 项语义硬检查。§3 Source Events、§4.1 AppError.code、§4.2 蓝牙 result code 均与上游契约一致；§6~§8 使用的字段完整存在于 DTO 生成的 Theme Schema；ADR/KAD 引用有效。强类型 `Curve` / `EndLevel` 的序列化值与现有 UI 一致，`LedTrackDef oneOf` 未改变交互契约。 |

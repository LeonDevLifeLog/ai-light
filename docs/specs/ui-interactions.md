# AI-Light 交互说明（UI Interactions）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.38 |
| 文档状态 | 生效；已按代码实现状态对账（V1.38，2026-09-05） |
| 范围 | L5 展示层所有用户可感知的交互 |
| 上游 | [docs/specs/ui-design.md](./ui-design.md)、[docs/specs/ipc-contract.md](./ipc-contract.md)、[docs/specs/theme-format.md](./theme-format.md) |
| 配套原型 | [docs/design/ui-preview.html](../design/ui-preview.html) |

> 本文聚焦"用户在每个动作里看到/做到什么"，不重复设计代币、视觉规范（[ui-design.md](./ui-design.md)）或协议字段（[theme-format.md](./theme-format.md)）。

---

## 1. 信息架构与导航

### 1.1 一级导航（左侧栏）

| 路径 | 页面 | 触发场景 |
|---|---|---|
| `/` | 状态总览 | 默认打开，看当前业务状态 / 设备 / 主题 |
| `/devices` | 设备管理 | 连接 / 断开灯牌 |
| `/integrations` | 接入外部工具 | 配置 Claude Code / Codex / WorkBuddy 等的 hook |
| `/themes` | 主题中心 | 浏览 / 切换 / 编辑主题 |
| `/preview` | 试听 | 模拟业务状态；连接设备后试听实际灯光与声音 |
| `/settings` | 设置 | 显示（外观模式 / 灯组朝向 / 当前主题）+ 系统（开机自启 / 外部运行环境 / API 接口文档） |

切换：单击 sidebar 任意项 → 对应 page-section 激活（其余隐藏）。

侧栏底部默认只显示设备连接状态；版本号与服务端口收进「高级信息」折叠项，避免把诊断信息持续暴露在主界面。

### 1.2 顶部区域

**当前版本无顶部条**。所有交互均通过 sidebar + 页面内操作完成。设计上避免"演示/调试入口"出现在生产路径中。

---

## 2. 通用交互模式

### 2.1 实时事件流

后端通过 Tauri events 推送变化，前端订阅：

| Event | Payload | 受影响的 UI | 实现状态 |
|---|---|---|---|
| `business-state-changed` | `{ state, source, session, sinceTs, theme }` | Dashboard 红绿灯徽章 + 状态名 + 副标题 | ✅ Rust 已 emit |
| `device-connection-changed` | `{ connected, address, name, reason?, reconnecting? }` | Dashboard 设备卡 + Sidebar 底部「已连接」状态 + Devices 页重连中卡 | ✅ 连接 / 断连 / 主动断开 / 忘记 / 重连放弃均已 emit（断连时清空电源字段） |
| `device-power-changed` | `{ capabilityBits, batteryMv, batteryPercent, powerSource, chargeState, powerFlags }` | Dashboard 与 Devices 设备卡电量格 | ✅ 握手 GET_POWER_STATUS + POWER_CHANGED 主动事件均已 emit |
| `device-fault` | `{ source, code, context }` | Devices 页告警卡 | ✅ FAULT_EVENT 已接线并 emit |
| `theme-changed` | `{ name }` | Dashboard 主题卡 + Sidebar 底部「当前主题」 | ✅ Rust 已 emit |
| `config-changed` | 更新后完整 Config | 全局配置同步（含托盘徽章朝向单选勾选） | ✅ 设置 / 托盘修改后 emit |
| `open-config` | — | AppShell 跳转 /devices | ✅ 托盘「打开配置」emit（UI 导航事件） |

**初始化流程**：打开主窗口 → 自动调 `get_app_state()` 拉全量快照 → 订阅 events 接收增量。

### 2.2 配置写入

任何"修改后立即生效"的设置（外观模式、灯组朝向、自启动、主题名、主题编辑）走：

```
前端 invoke `update_*` 或对应 command
  ↓ 后端写入 config.json + 更新内存
  ↓ 触发对应 event
  ↓ 前端事件订阅处刷新
```

失败回滚：UI 显示原值 + Toast 错误说明（不静默丢失用户操作）。

---

## 3. 状态总览（`/`）

### 3.1 红绿灯徽章

- **业务状态 → 灯位** 映射（V0.4 §7 + ADR-0002）：
  | 状态 | 灯位 + 动画 |
  |---|---|
  | `IDLE` | 全灭 |
  | `WORKING` | 绿灯 呼吸（2s 周期，ease-in-out）|
  | `WAITING` | 黄灯 常亮 |
  | `SUCCESS` | 绿灯 常亮 |
  | `ERROR` | 红灯 闪烁（1Hz，0/49%-50/100%））|
  | 自定义状态 | 主题映射；未映射 → 全灭（fallback IDLE）|

- **朝向**（来自用户设置 `badgeOrientation`，默认 `horizontal`）：
  - `horizontal`：3 灯位横排，灯心距 `var(--space-7)`（32px），灯径 40px
  - `vertical`：3 灯位竖排，灯心距 `var(--space-5)`（20px），灯径 28px（紧凑）
- **无障碍**：`prefers-reduced-motion: reduce` 时关闭呼吸/闪烁动画（仅颜色与文字标识）。
- **色盲友好**：状态名 + 源标签永远可见（颜色不是唯一信号）。

### 3.2 状态名 + 副标题

- 状态名（大号 Inter 600 / 32px / `--text-4xl`）随业务状态变化，颜色按上表
- 副标题（一行 `var(--text-base`），`）` 一句中文友好描述：
  | 状态 | 副标题 |
  |---|---|
  | IDLE | 一切就绪，等待任务 |
  | WORKING | AI 正在思考和执行 |
  | WAITING | 需要你确认或输入 |
  | SUCCESS | 这次任务顺利收尾 |
  | ERROR | 任务没跑通，看一眼 |

### 3.3 设备卡

- 显示：图标 + 名称（用户起的别名，如「客厅的灯牌」）+ 副标题（位置描述）
- Dashboard 摘要在默认窗口宽度下使用多行信息层级：设备名最多两行，连接状态与电量位于独立元信息行；超长名称省略并通过悬停提示展示全文，电量与导航箭头不得被名称挤出卡片。
- 三列元数据：电量（百分比 + 图标）/ 信号（4 格彩条）/ 同步时间（相对时间，如「5 秒前」）
- 状态 tag：`已连接`（绿）/ `未连接`（灰）/ 故障时显示设备故障 Alert

### 3.4 主题卡

- 显示：3 灯条色块缩略 + 主题名（中文）+ 一句描述
- 状态 tag：`正在使用`（绿）
- 操作：[换主题] → 跳转 `/themes`

---

## 4. 设备管理（`/devices`）

### 4.1 扫描流程

```
首次进入 /devices
  ↓
后端自动 scan_devices()（5s 倒计时）
  ↓
按钮进入禁用 loading 态 + 显示 Progress 条 + 「正在查找附近的灯牌…」
  ↓
结果列表分 3 类：
  ├ 已记住的 AgentCore 灯牌：[重新连接] 按钮（仅真实连接时显示禁用的 [已连接]）
  ├ 新发现的 AgentCore 灯牌（ACLight-* 开头）：[连接] 按钮
  ├ 同名其它 AgentCore：与上一类同
  └ 非灯牌蓝牙设备：灰显 + 「不是灯牌，无法连接」
```

扫描项不得仅凭地址与设备快照相同判定为已连接；必须同时满足 `device.connected == true` 且地址相同。已记住但离线的设备标记「已记住」，保留可点击的 [重新连接]，避免历史地址造成不可恢复的禁用态。

- 扫描结果按「已记住设备 → 已识别状态灯 → 其它蓝牙设备」排序；同类设备按 RSSI 从强到弱，未知信号置后，地址作为稳定兜底顺序。
- 扫描反馈至少保持 400ms，防止本地 mock / 缓存结果过快返回而让用户无法感知点击已生效。
- 页头与空态的重试动作统一命名为「重新查找设备」。

### 4.2 连接单台

点击 [连接] → 后端 `connect_device_internal`：
1. `ble::scan` 4s 重扫确保设备还在
2. `ble::connect_to_address` 建链
3. 发现 GB_TRANS 特征 + 订阅 TX Notify（✅ 已实现）
4. V0.4 握手——等 DEVICE_READY → GET_DEVICE_INFO → GET_CAPABILITIES → GET_POWER_STATUS（按能力位）（✅ 已实现；固件 / 硬件变体 / 电量写入设备快照）
5. 热切换设备（`DeviceIo::set`）✅
6. 写 `config.remembered_device` ✅
7. 触发 `device-connection-changed` ✅
8. 引擎 resync（重发当前业务 SCENE）✅

UI 反馈：设备卡状态 tag 立即更新；失败显示 Toast（原因 + 重试）。

### 4.3 断开

- Devices 页顶部「我的设备」以 `config.rememberedDevice` 为存在条件：只要未执行忘记，已记住设备在已连接、连接中、自动重连、重连失败或普通离线时均保留卡片。离线卡展示 [重新连接] / [忘记设备]，电量、固件、硬件等实时字段显示 `—`。
- [断开连接]：取消当前连接代次的自动重连 → BLE 主动断开 → 清理设备快照；保留记忆设备，下次启动仍会自动连接。
- [忘记设备]：应用内确认 Dialog → 先主动断开并取消重连 → 清除并持久化记忆设备；断开失败时不清除记忆。
- 自动重连中显示 [停止重连] 与 [忘记设备]；旧退避任务通过连接代次校验退出，不得在用户操作后重新连回。
- 成功分别 Toast「设备已断开」/「已忘记设备」；失败保留可恢复状态并显示原因。

### 4.4 故障告警

> ✅ 实现状态：FAULT_EVENT (0xEF) 已接线，Rust 收到后 emit `device-fault`，Devices 页告警卡即可出现。

收到 `device-fault` event → Devices 页顶部插入红色 Alert 卡：
- 标题：「设备故障事件」
- 内容：设备名 + 故障源（LED / 蜂鸣器 / 电源 / 协议内部）+ code
- 不阻塞其它 UI；可手动关闭（V2 加 Alert dismiss）

---

## 5. 接入外部工具（`/integrations`）

### 5.1 3 个客户端卡

| 客户端 | 状态 tag（默认） | 配置文件 | 接入方式 |
|---|---|---|---|
| Claude Code | 已连接 / 未连接 | `~/.claude/settings.json` | Node Adapter command hook |
| Codex | 已连接 / 未连接 | `~/.codex/hooks.json` | Node Adapter command hook；notify 仅作后续兼容降级 |
| WorkBuddy | 已连接 / 未连接 | `~/.workbuddy/settings.json` | Node Adapter command hook；协议兼容 CodeBuddy，配置命名空间独立 |

### 5.2 操作

客户端卡只包含一个上下文主动作：未连接时 [连接]，已连接时 [断开]。连接前先检查运行环境（ToolchainService 强制复验）：就绪时直接写入托管 Hook；Adapter 缺失或版本不兼容时按钮分别进入「确认并安装」/「确认并升级」态，并内联展示操作说明（目标包 `@ai-light/adapter`、使用已检测的 Node.js 与 npm、不提权不改 PATH），用户确认后才执行兼容版本安装；Node/npm 未就绪时停止连接，由运行环境恢复卡提供手动选择路径或重新检测。断开只删除 AI-Light 托管 Hook。按钮执行时进入 loading（文案「正在检查运行环境」）并禁止重复触发，结果通过 Toast 反馈。

### 5.2.1 运行环境卡（ADR-0006）

接入页顶部常驻一张运行环境卡：

- 状态摘要下常驻「接入是如何工作的？」说明区：中性底色、独立于异常告警；解释 `@ai-light/adapter` 配置 Hook 并向本机 AI-Light 上报任务状态，展示「AI 客户端 → 接入组件 → 本机 AI-Light → 状态灯」流程。包名链接通过系统浏览器打开 npm 页面，失败内联显示可复制 URL；补充 Node.js 20+ / npm 依赖及首次连接的兼容版本安装确认。流程可换行，包名链接保留键盘焦点与外链标识，深浅色复用现有语义色。
- 自动检测成功时状态区显示一行摘要（如 `Node.js 22.14.0 · npm 10.9.2 · Adapter 0.1.2`）+ [查看详情] [重新检测]
- [查看详情] 展开各工具的路径（等宽字体、可换行、带复制按钮）、版本、来源（环境 PATH / Node 同安装族 / npm 全局目录 / 版本管理器等）与检测模式
- 请求失败保留上次检测结果，内联显示「检测失败」与错误原因，始终提供 [重新检测]。后端 `INTERNAL` issue 即使附带 `checking` 也按失败展示；无结果且已结束检测不得显示检查中。
- Node 缺失、版本过低、npm 不可用分别显示「需要安装 Node.js」「需要升级 Node.js」「npm 不可用」。原地展开安装/修复指引，提供官方下载入口、版本管理器说明及修复后重新检测步骤；选择文件为次要操作，说明文件类型。
- Adapter 缺失/不兼容分别显示「需要安装接入组件」「接入组件需要升级」，明确引导下方工具卡 [连接] 承接安装/升级确认。手动指定 Adapter 文件仅放在高级详情。
- `toolchain.json` 损坏、非法或 schema 不兼容时优先显示「环境配置需要重建」，保留原文件且禁止接入写操作；明确点击 [重建配置并检测] 才重建配置。
- [重新检测] 保留手动路径；[改用自动检测] 清除全部手动路径并重新发现，操作旁明确说明影响。路径选择/恢复失败内联保留，取消选择不提示成功；恢复结果由卡片持续呈现。
- [查看详情] 包含版本、路径、来源、全部 issue 与高级文件选择；提供 [复制诊断信息]，复制失败支持手动复制。多候选与权限状态暂保留契约展示，当前后端未可靠独立归类；未知失败保留实际原因。
- Node/npm 以真实执行能力为准：Node 能运行 npm 的版本与全局 prefix 查询即视为可用；安装目录不同仅作诊断，不阻塞接入
- 状态同时用图标、文字和颜色表达；异步结果通过 `aria-live` 区域宣告

### 5.3 Codex 特殊说明

能力以当前本机 Codex 配置与 Adapter 检测结果为准，不再固定显示“Desktop 暂不支持”。V1 优先 lifecycle command hooks，不与 `notify` 重复发送终态。

WorkBuddy 只使用其明确支持的 `SessionStart`、`UserPromptSubmit`、`PreToolUse`、`Stop`、`SessionEnd` 事件，分别同步空闲、工作、等待提问、完成和会话结束；协议未提供可靠失败事件，因此不推断 `ERROR`。

### 5.4 配置生效流程

```
用户 [连接] → Desktop 调用 Adapter CLI → 备份并幂等写入 Hook
  ↓
工具启动 → 第一次 hook 触发 → Adapter 读取 stdin 与 ~/.ailight/runtime.json → POST /hook
  ↓
AI-Light 收到 → 仲裁 → 主题映射 → SCENE 下发 → 灯亮
  ↓
UI 事件流：business-state-changed → Dashboard 红绿灯变化
```

---

## 6. 主题中心（`/themes`）

### 6.1 浏览

- 6 张内置主题卡（默认 / 极简 / 专注 / 自然 / 极光 / 霓虹）
- 主题卡含：主题名 + 中文描述 + 缩略图（3 灯条色块）+ [使用此主题] / [正在使用] 按钮；用户主题额外显示 [导出]、[删除]
- 当前激活主题有外发光边框 + `当前使用` tag

### 6.2 切换主题

点击 [使用此主题] → 后端 `set_active_theme(name)`：
1. 校验主题存在且合法（theme-format V1.0 §4 整体校验）
2. 写 `config.active_theme` + 内存更新
4. 触发 `theme-changed` event
5. 前端：
   - 当前业务 `IDLE` → 仅 UI 更新
   - 当前业务非 IDLE → 重编译 SCENE 并下发（APPLY_IF_CHANGED），Dashboard 红绿灯立即反映新主题

错误回滚：UI 不切换 + 错误 Toast。

### 6.3 编辑主题

点击 [编辑当前主题] → 打开主题编辑器 Dialog。

### 6.4 导入主题

点击 [导入新主题] → 打开 Dialog：
1. 文件选择器选 `.ailight-theme.json` 或粘贴 JSON 文本
2. `import_theme(content)`：
   - 解析失败 → 错误 Toast（指出校验失败原因）
   - 与内置同名 → 错误 Toast（CONFLICT）
   - 成功 → 写 `themes/<name>.ailight-theme.json`，网格自动刷新

### 6.5 导出用户主题

- 所有 `builtin == false` 的用户主题均显示 [导出]，包括通过文件导入后保存的主题；内置主题不显示，Rust 侧仍以 `THEME_BUILTIN` 阻止绕过 UI 的请求。
- 点击 [导出] → Rust 重新读取并校验持久化主题 → 打开系统保存窗口；默认文件名为 `<name>.ailight-theme.json`，文件内容保持原样，可由现有导入流程重新导入。
- 导出期间当前卡片的 [导出] 显示 loading，并禁用该卡片的应用、导出和删除；其他主题卡仍可操作。
- 保存成功 → Toast“主题已导出：`<name>.ailight-theme.json`”；用户取消系统保存窗口 → 静默结束；读取、校验或写入失败 → Toast“主题导出失败：`<reason>`”。
- 导出不切换当前主题、不修改配置、不触发事件、不改变设备灯效。

### 6.6 删除用户主题

- 内置主题不显示删除入口，Rust 侧仍以 `THEME_BUILTIN` 阻止绕过 UI 的删除请求。
- 用户主题点击 [删除] → Dialog 显示主题名并说明不可恢复；确认按钮使用危险操作样式，删除期间禁用重复操作。
- 普通用户主题删除成功 → 移除本机主题文件、刷新网格、关闭对应详情并显示成功 Toast。
- 当前用户主题删除前，Dialog 明示将切换默认主题；确认后先应用内置 `default`（含配置持久化、`theme-changed` 和当前 SCENE 重放），再删除文件。
- 删除失败 → Dialog 内显示具体原因并保留主题；若文件删除失败，后端尝试恢复原主题。

---

## 7. 主题创作器 Dialog

### 7.1 进入与默认模式

- 入口：`/themes` → [以当前主题创建] → Dialog 打开
- **面向最终用户**：默认不暴露协议术语（SCENE / curve / 相位 / 占空比 / JSON），只展示"动效、速度、颜色、顺序、声音"五个用户语言分组
- 标题栏：仅放主题名称输入；专家入口不与主题身份混排
- 标题栏字段命名为「主题标识」，说明其用于保存和导入，避免误解为可输入任意自然语言的展示名
- 内置主题始终另存为用户主题，且不得与内置主题同名
- 右侧始终有**软件动画预览**：按草稿真实曲线 / 周期 / 相位 / 亮度模拟三灯 + 蜂鸣，不依赖设备

### 7.2 一屏一状态

先选状态，再为该状态设计一套灯效：

```
┌─ 状态 chips（中文名 + 英文编码 + 状态色点）──────────┐
│ [空闲 IDLE] [工作中 WORKING] [等你回复 WAITING]       │
│ [完成 SUCCESS] [出错 ERROR]                          │
└────────────────────────────────────────────────────┘
新增状态 / 删除当前自定义状态
─────────────────────────────────────────────────────────
借用主题效果（折叠卡片：来源主题 + 来源状态 → 覆盖当前状态）
灯光怎么动？    常亮 / 呼吸 / 闪烁 / 流动 / 渐亮 / 渐弱   ← 6 张带波形图示卡片
变化速度        舒缓(≈2.8s) / 适中(≈1.4s) / 活跃(≈0.6s) ← 仅非"常亮"时出现
三颗灯的颜色与亮度  顶灯 / 中灯 / 底灯 各一行：颜色 + 亮度滑块；熄灭灯显示"熄灭"虚线
三灯怎么依次出现？ 一起 / 上→下 / 下→上 / 交错           ← 仅非"常亮"时出现
提示声音        无声 / 轻提示 / 确认音 / 警报音            ← 4 张卡片
逐灯精确调整（折叠入口，紧邻其展开内容）
```

- 标准 5 态固定保留，允许添加、删除自定义状态
- `<brightness> = 0` 表示该灯全黑；`leds[i] = null` 表示该灯熄灭（预览显示灰暗状态）
- 每颗灯的颜色行提供 [熄灭此灯] / [点亮此灯]；颜色选择器只编辑协议支持的 `#RRGGBB`，“透明/无颜色”统一映射为 `leds[i] = null`
- 动效判定基于**当前场景的主导曲线**（第一条非熄灭灯轨的 `curve`），而非当前选中灯
- 修改自动保存为本机草稿；重新打开同一来源主题时恢复
- 设备已连接时显示「在设备上试听」，将未保存草稿交给 `preview_scene(content)` 并以 `RESTART_SCENE` 重播
- 「借用主题效果」以完整折叠卡片呈现，可把任意内置/用户主题的标准状态效果复制到当前状态；面板明确显示来源与“将覆盖：当前主题 · 当前状态”，成功后 Toast 回显复制路径

### 7.3 进阶参数（可选，默认收起）

点击基础编辑末尾的 [逐灯精确调整] 折叠入口展开，仅高级用户使用；入口与展开内容相邻，使用 `aria-expanded` 表达状态，并继续使用用户语言：

| 步骤 | 内容 | 控件 |
|---|---|---|
| 逐灯运动方式 | 顶 / 中 / 底 单独设置 常亮 / 闪烁 / 呼吸 / 渐亮 / 渐弱 | select → `curve` |
| 低点颜色 | 波形低点颜色 | `low` color |
| 动画节奏 | 一轮用时、出场时间 | `period_ms` / `phase_deg` |
| 播放结束 | 重复次数 + 熄灭 / 停在暗色 / 停在亮色 | `repeat` / `end_level` |
| 状态切换 | 过渡时长、终态驻留 | `transition_ms` / `hold_ms` |
| 提示音细节 | 频率 / 时长 / 音量 / 重复次数 | buzzer `segments` |

界面主要标签不得使用"相位、曲线、占空比"；角度值只作为"出场时间"的精确值展示；"占空比"仅对闪烁（SQUARE）显示。

### 7.4 模式切换副作用

```
[展开逐灯精确调整] → 在入口下方显示精确字段；数据保留
[收起逐灯精确调整] → 隐藏精确字段；数据保留
```

切换不丢用户已填的数据（数据在草稿对象中保留）。

### 7.5 保存

- [保存主题] → 校验（theme-format V1 §1~§5）+ 写入 `themes/<name>.ailight-theme.json`
- [取消] → 关闭 Dialog，不保存任何修改（有改动时二次确认）

保存成功后：
1. 当前 SCENE 重新编译（如果业务非 IDLE）
2. 触发 `theme-changed` event
3. Dashboard 主题卡更新

---

## 8. 试听（`/preview`）

### 8.1 标准状态按钮

5 个按钮：空闲 / 工作中 / 等待中 / 已完成 / 出错了。默认只展示用户语言，不展示 `IDLE / WORKING / WAITING / SUCCESS / ERROR` 编码。

点击 → `trigger_state(state, meta)`：
1. 后端 `engine::process_event(source='manual', state, None, None)`
2. 走仲裁器（manual 与其他来源一样按最近活动接管）
3. 主题映射 → SCENE 编译 → SET_SCENE 下发
4. 触发 `business-state-changed`
5. Dashboard 红绿灯立即变化

页面将此区域定义为「模拟业务状态」：设备未连接时仍可触发，并用 Toast 区分「软件状态已切换」与「灯牌将展示效果」。

**注意**：`trigger_state` 与 hook 事件走同一路径，不绕过仲裁（保证一致性）。

### 8.2 自定义状态

输入框：自定义状态标识（如 `REVIEW`，旁注「等待审核」帮助理解；格式受主题契约限制）
[触发] → 同上，但状态名不在 5 态中 → 走主题映射：
- 主题有映射 → 按映射灯效亮起
- 主题无映射 → fallback IDLE（全灭）；输入框下方常驻说明「当前主题没有对应效果时，将使用‘空闲’效果」

### 8.3 最近用过

最近 5 个自定义状态以快捷按钮呈现，点击即触发。无需手动输入。

### 8.4 关闭灯效

[恢复为空闲] → `reset_outputs()`：
- 设备端：灯全灭、蜂鸣停止、清空当前 SCENE
- 业务状态：复位为 IDLE
- 触发 `business-state-changed { state: IDLE }`

---

## 9. 设置（`/settings`）

页面分两组卡片（显示 / 系统），每行 = 图标 + 名称 + 一行用户友好说明 + 控件；页脚提示"所有设置即时生效并自动保存"。**不含任何开发者向文案**（P1/P2、明文存储等均不出现）。

### 9.1 显示

- 外观模式（`themeMode`）：三张选项卡片（亮色 / 暗色 / 跟随系统），各带图标 + 一句说明；选中态为绿描边 + 淡绿底 + 圆点填充。切换即时生效：
  - 亮色：`<html data-theme="light">`，slate 浅底 + 深绿链接色
  - 暗色：`<html data-theme="dark">`（默认，既有体验不变）
  - 跟随系统：`data-theme` 随 `prefers-color-scheme` 实时切换（含 OS 运行中变更）
  - 持久化经 `update_config(themeMode)` → config.json；重启首帧由 localStorage 引导缓存恢复（config 为事实源）
- 灯组朝向（`badgeOrientation`）：[横排] [纵向] 分段控件，切换即时生效（红绿灯布局直接变）。
- 当前主题：主题入口（Link → /themes）——三个灯色预览点（取当前主题 WORKING/SUCCESS/ERROR 场景实际灯色）+ 主题名 + 「提示音」标记（任一代表场景含蜂鸣时显示）+ 箭头。预览随 `config.activeTheme` 变化刷新，读取失败时回退为纯主题名。

### 9.2 系统

- 开机自启：✅ 已实装（2026-08-21，KAD-09 / ADR-0004）。Switch 真实切换：`update_config` 先 OS 后 config（OS 登录项为唯一事实源，config 为启动校准缓存）；失败返回 `AUTOSTART_FAILED` → Toast + 回滚到原值；重启时 `is_enabled()` 校准写回。
- 外部运行环境：摘要行后接默认收起的详情与操作，复用 Node.js / npm / Adapter 详情与路径恢复能力。Adapter 已就绪时为高级用户提供 [检查更新]；只有用户触发后才访问 npm registry，并展示当前版本、目标兼容版本与是否可升级。升级按钮必须明确写出精确目标版本（如「升级至 0.1.10」），不得安装 `latest`；完成后重新检测工具链并运行 Adapter doctor，成功 Toast 展示新版本，失败保留当前信息与可重试操作。不提供后台自动检查或自动升级开关。
- API 接口文档：作为系统组直接子项展示，不折叠；[打开文档] 使用系统默认浏览器打开当前实际监听地址 `http://127.0.0.1:{service.port}/docs/`。打开中显示「正在打开…」并屏蔽重复触发；应用状态尚未就绪时禁用。成功不额外提示，打开失败显示原因 Toast。端口由 AI-Light 自动管理，不提供用户编辑。

> 未展示项：日志查看（P2）。

---

## 10. 端到端流程示例

### 10.1 首次启动

```
双击图标
  ↓
Tauri Builder.setup()：
  - 加载 config.json（缺失则用默认）
  - 初始化日志（info 级）
  - 加载内置默认主题
  - 启动 Engine（tokio spawn，单 writer 队列）
  - 启动 L1 hook_server（axum，25679）
  - 启动事件轮询（200ms tick 仲裁 + emit）
  ↓
Tauri Builder.on_window_event()：
  - 窗口关闭 → api.prevent_close() + hide()
  ↓
托盘常驻（图标 + 菜单，macOS Dock 不显示）
  ↓
启动即显示主窗口（RunEvent::Ready → show + focus，/ Dashboard）
  ↓
前端 invoke get_app_state() 拉快照
  ↓
订阅 events，进入 Dashboard 视图
  ↓
（关窗后）用户点击托盘"显示窗口" → 主窗口重新出现
```

### 10.2 hook 触发闭环

```
外部工具（Claude Code 等）
  ↓ POST /hook { source, event, state }
L1 hook_server
  ↓ 校验（hook-api V1.0）
  ↓ arbiter.process( source, state ) → 当前业务状态
  ↓ engine.compile_current() → 当前 SCENE
  ↓ 与当前有效 SCENE 比较 → 相同则不发；不同则 SET_SCENE
  ↓ emit business-state-changed
  ↓
前端 Dashboard：
  - 红绿灯徽章切换
  - 状态名 + 副标题更新
  ↓
设备端：
  - 4 条轨道（3 灯 + 1 蜂鸣）从同一 scene_epoch 启动
  - 动画在本地 20 ms 步长推进
```

### 10.3 主题编辑全流程

```
用户 /themes → [编辑当前主题]
  ↓
Dialog 打开，默认 [简单] + [空闲 [tab]] 选中
  ↓
用户改 [顶灯] 主色 → 视觉立即反映
  ↓
用户切 [进阶] 模式 → 6 步骤编辑展开
  ↓
用户改 1·波形（TRIANGLE → SQUARE）
  ↓ 用户把灯光顺序改为「从上往下」
  ↓ 用户改 4·蜂鸣（增加一段静音间隔）
  ↓
[保存修改]
  ↓
后端校验（theme-format V1.0）
  ├─ 非法 → 错误 Toast，保持原主题
  └─ 合法 → 写文件 + 内存更新 + emit theme-changed
       ↓
前端：
  - Dashboard 主题卡更新
  - 如果当前业务非 IDLE → 引擎重编译 SCENE + 下发
       ↓
设备端：
  - 新 SCENE 原子替换（apply_mode=APPLY_IF_CHANGED，内容不同则重启）
  - 4 条轨道从新 scene_epoch 启动
```

---

## 11. 错误处理矩阵

| 场景 | UI 反馈 |
|---|---|
| 主题导入失败（JSON 非法）| Dialog 错误提示（含校验失败原因）|
| 主题与内置同名 | Dialog 错误提示（CONFLICT）|
| 主题保存失败（写文件失败）| Toast + 保持当前编辑状态 |
| 主题导出失败 | Toast“主题导出失败：`<reason>`”；取消系统保存窗口不提示 |
| 主题删除失败 | Dialog 保留并显示原因；主题卡不移除 |
| 设备扫描失败（蓝牙权限 / 系统错误）| 红色告警条 + 重试按钮 |
| 设备连接失败 | Toast（含原因）+ 保留在 /devices |
| 主动断开失败 | Toast（含原因）+ 保留连接与记忆设备 |
| 端口热重启失败 | Toast（端口与失败原因）+ 输入回滚，旧 Hook Server 继续运行 |
| API 文档打开失败 | Toast「无法打开 API 文档」+ 系统或浏览器返回的具体原因 |
| 未连接设备试听 | Toast「请先连接设备后再试听灯效」（`DEVICE_NOT_CONNECTED`） |
| 设备断连 | Toast「设备已断开」+ 设备卡显示「未连接」|
| 设备重连成功 | Toast「设备已重新连接」+ 设备卡恢复 |
| 设备故障（FAULT_EVENT）| 红色 Alert 卡 + Dashboard 设备卡故障指示 |
| hook_server 未启动 | L1 服务侧问题；UI 不感知（红绿灯不变）|
| 设置保存失败 | UI 显示原值 + Toast 错误说明 |
| 启动期 no reactor panic | 启动期崩溃（ADR-0003 / KAD-08）；不可恢复 → 进程退出 |

---

## 12. 托盘（图标 + 菜单）

**实现状态（2026-08-21）：✅ 已实装**——图标 + 菜单（显示窗口 / 当前状态 / 当前主题 / 设备 / 徽章朝向单选 / 打开配置 / 退出）由 `src-tauri/src/tray.rs` 构建，动态文字经业务事件更新。优先级已按 ui-design §11.1 口径确认为 P1（本文原 V2 标注作废）。

> 托盘图标当前复用应用图标占位（mac 模板图单色），正式素材待替换；三平台行为验证（U-05）待实机。

设计要点（来自 ui-design.md §5.1.1）：

```
┌─ 显示窗口 ───────────┐
│ 当前状态：WORKING    │ ← 动态
│ 当前主题：neon       │ ← 动态
├─ 徽章朝向 ───────────┤
│ ● 横向               │ ← 单选
│ ○ 纵向               │
├─ 打开配置 ───────────┤
│ 退出 ───────────────┘
```

落地情况：
- 单实例保证（`tauri-plugin-single-instance`）✅
- 关窗 = 隐藏，菜单"退出"才是真退出 ✅（托盘退出入口已实现）
- mac 菜单栏 / win 通知区 / linux DE 三平台适配：⏳ 待实机验证（U-05）

---

## 13. 键盘与可达性

### 13.1 键盘导航

- Tab 顺序 = 视觉顺序
- Esc 关闭最上层 Dialog
- Enter 提交当前焦点表单

### 13.2 焦点环

所有可交互元素：`focus` 状态显示 2-4px 焦点环（`outline`），颜色 `var(--ring)`。

### 13.3 prefers-reduced-motion

启用后关闭红绿灯呼吸/闪烁动画（仅颜色与文字标识），符合 §3.1 无障碍要求。

### 13.4 对比度

- 正文（暗色模式）：`var(--fg)` vs `var(--bg)` ≥ 4.5:1（WCAG AA）
- 未激活灯位 vs 背景 ≥ 3:1（次要信息）
- 浅色模式由浅色 token 保证（`#0F172A` on `#F8FAFC`）

---

## 14. 待办与 V2

| 编号 | 项 | 优先级 |
 |---|---|---|
| G-01 | BLE 断连监听 + `device-connection-changed{false}` + 退避重连（5 次，约 75s 窗口） | ✅ 已实现（2026-08-21；实机冒烟 U-01 待完成） |
| G-02 | 握手信息读取（DEVICE_READY / GET_DEVICE_INFO / GET_CAPABILITIES / GET_POWER_STATUS）→ `device-power-changed` | ✅ 已实现（2026-08-21；实机冒烟 U-01 待完成） |
| G-03 | FAULT_EVENT 接线 → `device-fault` | ✅ 已实现（2026-08-21；实机冒烟 U-01 待完成） |
| G-04 | 托盘实装（图标 + 菜单；口径已定 P1） | ✅ 已实现（2026-08-21；图标占位待替换，U-05 待实机） |
| G-05 | Hook Server 地址管理 | ✅ 已实现（固定优先 25679、自动退避、runtime 文件供 Adapter 发现；不开放用户修改） |
| G-06 | `autostart` 接入 tauri-plugin-autostart | ✅ 已实现（2026-08-21；三平台实机 U-08 待完成） |
| U-01 | btleplug 三平台冒烟（mac/win/linux） | P1 阻塞 release |
| U-02 | axum 编译/启动验证 | P1 |
| U-05 | 托盘图标三平台差异 | P1（托盘实装后） |
| U-08 | 开机自启三平台实机（mac LaunchAgent / win Run key / linux XDG） | P1（自启实装后） |
| V2-2 | 接入密码 UI 重新评估 | V2 |
| V2-3 | 主题编辑器加入波形实时动画预览 | V2 |
| V2-4 | 设备详情页（电量历史 / 固件升级）| V2 |
| V2-5 | 日志查看面板（应用内）| V2 |
| V2-6 | 主题导入支持 URL / 分享码 | V2 |

---

## 附录 A：中粒度补充（与 [ui-interaction-spec.md](./ui-interaction-spec.md) 对齐）

> 本附录是 ui-interactions.md 的中粒度补充章节，对原 §2~§13 中颗粒不足处做扩展。
> 完整组件级行为契约见姊妹文档 [ui-interaction-spec.md](./ui-interaction-spec.md)（L4 组件层 + L5 关键控件 + 联动矩阵 + 失败路径）。

### A.1 §2.3 扩展：组件视觉态全集（8 态）

| 态 | 触发条件 | 视觉（由 ui-design.md 代币落地） |
|---|---|---|
| `default` | 初始 / 数据就绪 | bg-elev + border-soft |
| `hover` | 鼠标悬停 / 触控长按 | border + shadow-md 提升 |
| `focus` | Tab 聚焦 / 编程聚焦 | 2px ring（颜色 = `--ring`） |
| `active` | 鼠标按下 / 触发中 | bg-secondary + `scale(0.98)` 150ms |
| `disabled` | 不可用 | opacity 0.5 + cursor not-allowed |
| `loading` | 异步进行中 | 内嵌 pulse 动画 + 占位骨架 |
| `error` | 校验失败 / 操作失败 | border-destructive + 抖动 200ms 一次 |
| `empty` | 无数据 | 纯文字提示 + 主操作按钮 |

每个可交互组件必须支持上述 8 态。具体每个组件的态全集见 [ui-interaction-spec.md §6~§8](./ui-interaction-spec.md)。

---

### A.2 §3.1 扩展：红绿灯徽章微交互

**朝向切换**：CSS transition 250ms ease-out（横→纵、纵→横过渡平滑）。

**状态切换动画**：
- 颜色变化：fade 200ms
- 呼吸（WORKING）：2s 周期 ease-in-out infinite
- 闪烁（ERROR）：1Hz 0/49%-50/100%）
- 切换生效：`prefers-reduced-motion: reduce` 时关闭呼吸/闪烁动画（仅颜色与文字标识）

**离线态**：设备断开时，3 灯全暗 + opacity 0.4 + 文字提示 "设备离线"。

**联动**：`business-state-changed` → 立即更新；`device-connection-changed` → 切换离线态。

---

### A.3 §3.3 扩展：设备卡 7 种态

| 态 | 触发条件 | 视觉 | 可点击 |
|---|---|---|---|
| `disconnected` | `!connected` | 占位卡 + 虚线边框 + "未连接" tag | [去连接] → `/devices` |
| `connecting` | 蓝牙握手进行中 | spinner + "连接中..." | 禁用 |
| `connected` | 握手完成 | 完整字段 + "已连接" tag（accent） | [断开连接] / [忘记设备] |
| `reconnecting` | 链路异常退避重连 | spinner + "重连中...(N/M)" | 禁用 |
| `lowBattery` | `batteryPercent < 20` | 电池格 warning 色 + Toast 警告 | 同 connected |
| `charging` / `full` | `chargeState` 变化 | 电池格 + ⚡ / ✓ 图标 | 同 connected |
| `fault` | 收到 `device-fault` | 红色故障指示条 + tooltip 显示 source/code | 同 connected |

**边界条件**：
- `powerFlags.Bit0 = 0`，或无运行时快照且 `capabilityBits.Bit4 = 0` → "无电池"
- `powerFlags.Bit0 = 1 && batteryPercent = null` → "电量未标定"；`batteryMv` 可用时追加显示原始 mV
- 有电池能力但尚无运行时电源快照 → "电池状态未知"
- `rssi` 不可用 → 信号条 0 格 + "信号未知"
- `sinceTs > 30s` 未更新 → "30+ 秒前"（warning 色）

---

### A.4 §4.x 新增：蓝牙交互各阶段 UI 反馈（V0.4 §5）

> ✅ 实现状态对账（2026-08-21）：阶段 1~8（BLE 连接 / 特征发现 / TX 订阅 / DEVICE_READY / GET_DEVICE_INFO / GET_CAPABILITIES / GET_POWER_STATUS）已全部实现；BAS 订阅按能力位尚未单独订阅（电量经 GET_POWER_STATUS / POWER_CHANGED 获取）。

| 阶段 | 后端动作 | UI 反馈 | 失败 Toast |
|---|---|---|---|
| 1. BLE 连接 | `btleplug.connect` | 设备卡 `Connecting` + spinner | "连接失败：`<reason>`" |
| 2. DIS 读取 | `read DIS 0x2A26` | （silent） | （日志） |
| 3. CCC 使能 | `subscribe TX CCC` | （silent） | "无法使能通知通道" + 断开 |
| 4. DEVICE_READY | 等设备主动事件（≤3s） | （silent） | "设备无应答" + 断开 |
| 5. GET_DEVICE_INFO | `0x02` | 设备卡显示 fw/hardwareVariant | "读取设备信息失败" + 断开 |
| 6. GET_CAPABILITIES | `0x04` | 设备卡"就绪"，触发引擎 resync | "读取设备能力失败" + 断开 |
| 7. BAS 订阅（可选） | `subscribe BAS 0x2A19/0x2BED` | 设备卡出现电量字段 | （日志；无电池版正常） |
| 8. GET_POWER_STATUS | `0x50` | 设备卡电源/电量字段填入 | "读取电源状态失败" + 断开 |

**任一阶段失败** → 设备卡回滚到 `Disconnected` + 触发 `device-connection-changed{connected: false, reason}`。

**断连宽限期**（V0.4 §13）：✅ 客户端链路已实现（断连监听 → `device-connection-changed{false, reason:"link_lost", reconnecting:true}` → 5 次退避重连，约 75s 窗口，期间已手动连接则放弃；放弃时 emit `reason:"reconnect_failed"`）；前端 `Reconnecting` 视觉态与断连/重连 Toast 已实装（2026-08-21）。

| 时间窗口 | 设备侧行为 | UI 反馈 |
|---|---|---|
| 断开瞬间 | 当前 SCENE 继续运行 | Toast "设备已断开" + 设备卡 `Reconnecting` 态 |
| 0~60s | 等重连 | Toast "重连中..."（可关闭） |
| 60s 内重连成功 | 无感恢复 | Toast "设备已重新连接" + 设备卡恢复 |
| 60s 超时 | 设备自动 RESET_OUTPUTS | Toast "设备已离线" + 设备卡 `Disconnected` + [去连接] 高亮 |
| 重连失败 N 次 | 退避后停止 | Toast "重连失败，请检查设备" |

---

### A.5 §6.7 新增：主题导入的 UI 细化

**文件选择器**：
- 扩展名过滤：仅 `.ailight-theme.json`
- 文件大小限制：1 MB（> 则拒绝）
- 解析过程：显示 Progress（< 100ms 通常瞬切）

**失败类型 → UI 反馈**：

| 失败类型 | UI 反馈 |
|---|---|
| 文件 > 1MB | Dialog "文件过大（>1MB）" |
| JSON 解析失败 | Dialog "JSON 解析失败：第 X 行 Y 列 `<reason>`" |
| 顶层多键 / 缺键 | Dialog "校验失败：缺少/多余键 `<key>`" |
| SCENE 引用不存在 | Dialog "校验失败：states[`<state>`].scene 引用 `<scene>` 不存在" |
| 字段非法 | Dialog "校验失败：`<field>` 值非法：`<value>`"（如 brightness=101、duration_ms=0） |
| 与内置同名 | Dialog "导入失败：与内置主题 `<name>` 同名" |

**成功**：网格新增卡 + Toast "导入成功：`<name>`"。

---

### A.6 §8.2 新增：自定义状态行为细化

**输入校验**：
- trim 后必须匹配 `[a-zA-Z0-9_-]+`
- 长度 1~64（展示超过 16 字符时按组件契约截断并提供完整 tooltip）
- 校验失败 → Input 显示 destructive 边框 + 错误文字

**最近 5 个（FIFO）**：
- 来源：CustomStateInput 提交成功的状态名
- 持久化：localStorage 或 config（待定）
- 去重：相同名不重复入栈
- 排序：最近使用在前
- 容量超出：移除最旧

**主题映射失败**：
- 全灭（fallback IDLE）
- Toast "该状态未在主题中映射"

**与 5 标准状态同名**：仍按 5 标准状态处理（不视为自定义）。

---

### A.7 §11 扩展：错误处理矩阵细化（错误码精确映射）

**ipc-contract §4**：

| 错误码 | UI 反馈 |
|---|---|
| `NOT_FOUND` | Toast "`<对象>` 不存在" |
| `THEME_INVALID` | Dialog "主题校验失败：`<details>`" |
| `CONFLICT` | Dialog "与内置主题 `<X>` 同名" |
| `THEME_BUILTIN` | Toast / Dialog“内置主题不可导出或删除” |
| `BAD_REQUEST` | Toast "请求参数非法：`<reason>`" |
| `DEVICE_NOT_CONNECTED` | Toast "请先连接设备" + 跳转 `/devices` |
| `DEVICE_DISCONNECT_FAILED` | Toast "无法断开设备：`<reason>`"，保留记忆设备 |
| `AUTOSTART_FAILED` | Toast "开机自启设置失败：`<reason>`"，Switch 回滚 |

**蓝牙 V0.4 §3.6**：

| Result code | UI 反馈 |
|---|---|
| `0x06 VERSION_MISMATCH` | Toast "设备协议版本不兼容，请升级固件" + 断开 |
| `0x09 LOW_BATTERY` | Toast "电量过低，灯效已停止" + 设备卡 lowBatteryBadge 高亮 |
| `0x0B NOT_SUPPORTED` | Toast "设备不支持此操作" |
| `0x07 NOT_READY` | Toast "设备未就绪" |
| `0x02 INVALID_PARAMETER` | Toast "灯效参数非法：`<field>`" |
| `0x04 BUSY` | Toast "设备忙，请稍后" |
| `0x05 INVALID_STATE` | Toast "设备当前不允许此操作" |
| `0x0A INTERNAL_ERROR` | Toast "设备内部异常" + 触发 `device-fault` |

---

### A.8 §13 扩展：键盘焦点流

| 页面 | Tab 顺序 | 快捷键 |
|---|---|---|
| Dashboard | （无可交互元素，焦点默认 body） | — |
| Devices | 重新查找 → 扫描结果 [连接] × N | Cmd/Ctrl+R = 重新查找 |
| Integrations | 运行环境 [重新检测] → [查看详情] → 恢复动作（按状态出现）→ Claude Code/Codex/WorkBuddy [连接或断开] | — |
| Themes | 编辑当前主题 → 导入新主题 → 主题卡 [使用此主题] → 用户主题 [删除] | Cmd/Ctrl+I = 导入；Esc = 关闭删除确认 |
| Preview | 5 标准按钮 → 自定义输入 → [触发] → 最近 N → [全部重置] | `1`~`5` = 标准状态；`0` = 全部重置 |
| Settings | 按视觉顺序 Tab | Esc 关闭 Dialog |
| 全局 | — | Esc 关闭最上层 Dialog；Enter 提交当前焦点表单 |

**焦点环**：所有可交互元素 `focus` 状态显示 2-4px 焦点环（`outline`），颜色 `var(--ring)`。

**焦点陷阱**：Dialog 打开时焦点移到第一个可交互元素；Tab 在 Dialog 内循环；关闭时焦点回到触发元素。

---

*附录结束。完整组件契约见 [ui-interaction-spec.md](./ui-interaction-spec.md)。*

---

## 14. 变更日志

| 版本 | 日期 | 变更 |
|---|---|---|
| V1.38 | 2026-09-05 | 运行环境卡新增常驻接入原理说明：npm 包链接、Hook 状态流、Node/npm 依赖和首次安装确认；中性底色与错误告警区分，流程自适应换行，外链失败内联反馈。对齐报告：§3 IPC events、§4.1 错误码、§4.2 蓝牙 result 名称核对通过；§6~§8 仅增加说明区，未新增主题字段；ADR/KAD 引用有效。 |
| V1.37 | 2026-09-05 | 运行环境恢复闭环：请求/解析异常展示失败并可重试，保留旧结果；Node/npm 内联安装指引；Adapter 安装与升级分开标识；手动文件选择收纳到详情；恢复失败持久展示、取消不报成功，配置损坏优先引导重建。对齐报告：§3 IPC Source Events 均存在（prefers-color-scheme 为浏览器媒体查询，非 IPC event）；§4.1 错误码全部存在于 ipc-contract §4；§4.2 result 名称均存在于蓝牙 V0.4 §3.6；§6~§8 仅修改工具链展示，主题字段未变；ADR/KAD 引用有效。 |
| V1.36 | 2026-09-04 | §1.1/§9 移除设置页“连接安全”与空置后的“服务”分组，将 API 接口文档作为“系统”直接子项展示，并收敛为显示/系统两组。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 未变且均存在于 ipc-contract §5；§4.1 AppError.code 未变且均存在于 ipc-contract §4；§4.2 蓝牙 result code 未变且与 V0.4 §3.6 一致；§6~§8 未新增主题字段且与 theme-format 一致；ADR-0001~0006、KAD-01~14 引用有效。 |
| V1.35 | 2026-09-04 | §1/§5/A.8 新增 WorkBuddy 一键接入卡与 `~/.workbuddy/settings.json` 独立配置路径；仅映射官方文档明确支持的五个生命周期事件，不推断失败态。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 未变且与 ipc-contract §5 一致；§4.1 未新增 AppError.code；§4.2 蓝牙 result code 未变且与 V0.4 §3.6 一致；§6~§8 未新增主题字段；ADR-0001~0006、KAD-01~14 引用有效。 |
| V1.34 | 2026-09-04 | §3.3 优化 Dashboard 设备摘要的窄卡布局：设备名称最多两行，连接状态与电量拆为独立元信息行，超长名称提供全文提示，固定保留电量与导航箭头空间。对齐报告（变更后自动，5 项语义硬检查通过）：Source Events 与 ipc-contract §5 一致；AppError.code 未变；蓝牙 result code 未变且与 V0.4 §3.6 一致；未新增主题字段；ADR-0001~0006、KAD 引用有效。 |
| V1.33 | 2026-09-04 | §5.2.1 将 Node/npm 兼容性收敛为真实执行能力：安装目录不同仅诊断，不阻塞接入。对齐报告（变更后自动）：§3 Source Events 未变且与 ipc-contract §5 一致；§4.1 未新增错误码，`NPM_NOT_FOUND` 仍存在于 ipc-contract §4；§4.2 蓝牙 result code 未变；§6~§8 未新增主题字段；ADR-0006 追加决策可解析，KAD 引用未变。 |
| V1.32 | 2026-09-01 | 修复电池存在性与百分比未知被 `null` 混淆：Dashboard 与 Devices 共用四态派生模型；快照和事件补齐 `capabilityBits` / `batteryMv`，有电池但百分比未标定时显示明确文字及可用电压。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（`device-power-changed` payload 已同步）；§4.1 AppError.code 未变；§4.2 蓝牙 result code 未变且与 V0.4 §3.6 一致；§6~§8 主题字段未变且与 theme-format 一致；ADR-0001~0006、KAD 引用有效。 |
| V1.31 | 2026-08-31 | 附近设备按用户意图与匹配度排序：已记住设备优先，其次已识别状态灯，再按 RSSI 从强到弱；设备页说明与空态移除 `AgentCore-Light` 产品字样，统一改为「状态灯」。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 未变；§4.2 蓝牙 result code 与 V0.4 §3.6 一致；§6~§8 主题字段未变且与 theme-format 一致；ADR-0001~0006、KAD 引用有效。 |
| V1.30 | 2026-08-31 | 设备页恢复路径闭环：顶部改为由 `rememberedDevice` 驱动的常驻「我的设备」卡，覆盖已连接/连接中/自动重连/离线并始终提供重连或忘记入口；附近列表仅以 `connected && address 相同` 判定已连接，历史设备显示「已记住」+「重新连接」；设备全量快照补齐 `reconnecting`，避免启动期错过 event 后状态失真。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件，既有 payload 与快照字段已同步）；§4.1 AppError.code 未新增且均在 ipc-contract §4；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（未改协议）；§6~§8 主题字段未变且与 theme-format 一致；ADR-0001~0006、KAD 引用有效。 |
| V1.29 | 2026-08-30 | Settings 外部运行环境增加高级用户主动检查/精确版本升级：仅点击后访问 npm registry，只选择桌面端兼容版本，升级后重解析并执行 doctor，不引入自动更新。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 新增 `ADAPTER_UPDATE_FAILED` 已同步 ipc-contract §4；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（未触碰）；§6~§8 主题字段与 theme-format 一致（未触碰）；ADR-0001~0006、KAD 引用有效。 |
| V1.28 | 2026-08-30 | 工具链核心不变量收敛：§5.2 将 `adapter_incompatible` 接入「确认并升级」恢复链；§5.2.1 增加 `store_invalid` 只读保护与显式恢复；附录 Tab 顺序同步当前 Integrations 结构。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 新增 `TOOLCHAIN_STORE_INVALID` 已同步 ipc-contract §4；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（未触碰）；§6~§8 主题字段与 theme-format 一致（未触碰）；ADR-0001~0006、KAD 引用有效。 |
| V1.27 | 2026-08-30 | 工具链发现落地（ADR-0006）：§5.2 连接流程加入运行环境检查与 Adapter 缺失确认态；新增 §5.2.1 运行环境卡（摘要/详情/恢复卡/手动选择路径）。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（新增 `NODE_*` / `TOOLCHAIN_*` / `EXECUTABLE_TIMEOUT` 已同步收录）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（未触碰蓝牙章节）；§6~§8 主题字段与 theme-format 字段表一致（未触碰）；ADR-0001~0006、KAD 引用有效。 |
| V1.26 | 2026-08-30 | 实装用户主题导出：所有非内置主题显示导出入口，Rust 校验后通过系统保存窗口原样写出；内置主题强制保护，取消保存静默结束。对齐报告：§2 Source Events 与 ipc-contract §5 一致；错误码均存在于 ipc-contract §4；蓝牙 result code 与 V0.4 §3.6 一致；主题字段未变；ADR/KAD 引用有效。 |
| V1.25 | 2026-08-30 | 实装用户主题删除：仅用户主题展示删除入口，确认 Dialog 明示不可恢复；删除当前主题自动回退 default，Rust 强制保护内置主题。对齐报告：§3 Source Events 与 ipc-contract §5 一致；§4.1 AppError.code 未新增；蓝牙 result code 与 V0.4 §3.6 一致；§6~§8 主题字段未变；ADR/KAD 引用有效。 |
| V1.24 | 2026-08-30 | 设置页移除“多个工具同时运行时”，仲裁固定为最近活动优先（ADR-0005 / KAD-13）。对齐报告：§3 Source Events 与 ipc-contract §5 一致；§4.1 AppError.code 未变；蓝牙 result code 与 V0.4 §3.6 一致；§6~§8 主题字段未变；ADR/KAD 引用有效。 |
| V1.23 | 2026-08-22 | KAD-12 仲裁语义澄清：§9.1 明确同一工具始终跟随最新生命周期状态，优先级与最近活动规则仅在多个工具冲突时生效；设置页文案同步。对齐报告：§3 Source Events 与 ipc-contract §5 一致；§4.1 AppError.code 未变；蓝牙 result code 与 V0.4 §3.6 一致；§6~§8 主题字段未变；ADR/KAD 引用有效，新增 KAD-12 可解析。 |
| V1.22 | 2026-08-22 | Node Adapter CLI 接入：§5 改为 Claude Code/Codex 一键连接与断开，移除复制 curl、伪测试与端口展示；§9.1 移除用户端口编辑，实际地址由 `~/.ailight/runtime.json` 自动发现。对齐报告：§3 Source Events 未变且均存在于 ipc-contract §5；新增 Adapter AppError.code 已同步 ipc-contract §4；蓝牙 result code 未变且与 V0.4 §3.6 一致；§6~§8 主题字段未变；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09/10/11 引用有效。 |
| V1.21 | 2026-08-22 | 设置页高级服务信息新增「接口文档」快捷入口：基于 `service.port` 用系统浏览器打开 Hook Server `/docs/` Swagger UI，包含 loading/disabled/失败 Toast，避免端口退避或热切换后打开旧地址。对齐报告：§3 Source Events 未变且均存在于 ipc-contract §5；§4.1 未新增 AppError.code，既有清单一致；§4.2 蓝牙 result code 未变且与 V0.4 §3.6 一致；§6~§8 未新增主题字段，与 theme-format 一致；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09/10 引用有效。 |
| V1.20 | 2026-08-22 | 设备与服务闭环：§4.3 实装主动断开、忘记设备及连接代次取消重连；§9.1 实装默认 25679、精确端口热重启及失败回滚；§11 补充 `DEVICE_DISCONNECT_FAILED` / `PORT_UNAVAILABLE` / `DEVICE_NOT_CONNECTED` 反馈。对齐报告：§3 Source Events 均存在于 ipc-contract §5；§4.1 AppError.code 均在 ipc-contract §4；蓝牙 result code 未变且仍与 V0.4 §3.6 一致；§6~§8 主题字段未变且与 theme-format 一致；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09/10 引用有效。 |
| V1.19 | 2026-08-22 | 全页面 UX review 优化：§1.1 将版本/端口收进「高级信息」；§4.1 补扫描最短可感知反馈并统一重试文案；§5 更新客户端支持状态、复制反馈并隐藏暂不支持项的无效操作；§7 将名称约束明确为「主题标识」；§8 区分状态模拟与灯牌试听、统一五态中文名；§9 将仲裁/保护改写为用户语言并折叠服务端口。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（错误路径未变）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（协议行为未变）；§6~§8 使用的 `leds` / `high` / `brightness` 字段与 theme-format 一致；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09 引用有效。 |
| V1.18 | 2026-08-22 | 主题创作器关闭与熄灯交互修复：§7.2 每颗灯新增“熄灭此灯 / 点亮此灯”，透明语义映射为协议支持的 `leds[i] = null`；取消、右上角关闭与 Esc 统一走应用内放弃修改确认 Dialog，替换 WebView 中无稳定反馈的原生 `window.confirm`。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（错误路径未变）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（熄灯仍编译为合法 SCENE）；§6~§8 使用的 `leds` / `high` 字段与 theme-format 一致；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09 引用有效。 |
| V1.17 | 2026-08-22 | 主题创作器布局 review 优化：§7.1 主题名称区移除无语义关联的进阶按钮；§7.2 “借用主题效果”改为完整 accordion，按“来源选择 → 覆盖当前状态”两行呈现，右侧预览增加当前状态标题；§7.3/§7.4 “逐灯精确调整”入口移至基础编辑末尾并紧邻展开内容，使用 `aria-expanded` 且不占用主操作色。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（错误路径未变）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（未触碰协议行为）；§6~§8 主题字段与 theme-format 字段表一致（未增删字段）；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09 引用有效。 |
| V1.16 | 2026-08-22 | 主题创作器重构为面向最终用户的引导式编辑（以代码为事实源）：§7.1 默认不暴露协议术语、右侧新增软件动画预览（按真实曲线/周期/相位/亮度模拟三灯+蜂鸣）；§7.2 改为"一屏一状态"——状态 chips（中文名+编码+色点）、6 张带波形图示的动效卡、速度/顺序仅在非"常亮"时出现、顶/中/底逐灯颜色+亮度行、熄灭灯显示"熄灭"虚线；§7.3 进阶参数改名为"逐灯精确调整"，并补回逐灯运动方式（curve）；§7.4 模式切换副作用改为"展开/收起进阶"。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（沿用 `THEME_INVALID` / `CONFLICT` / `BAD_REQUEST` / `INTERNAL`）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（本次未触碰蓝牙章节）；§6~§8 主题字段与 theme-format 字段表一致（仅改名，未改字段）；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09 引用有效。 |
| V1.15 | 2026-08-21 | 外观模式实装（亮/暗/跟随系统，用户触发）：§1.1 `/settings` 导航摘要与 §2.2 配置写入加入外观模式；§9.2 新增外观模式卡片（三选项 + `data-theme` 切换 + `update_config(themeMode)` 持久化 + localStorage 首帧引导），`themeMode` 从"未展示项"移除。对齐报告（变更后自动，5 项语义硬检查通过）：§3 Source Events 均存在于 ipc-contract §5（无新增事件）；§4.1 AppError.code 均在 ipc-contract §4（沿用 `BAD_REQUEST`）；§4.2 蓝牙 result code 与 V0.4 §3.6 一致（本次未触碰蓝牙章节）；§6~§8 主题字段与 theme-format 字段表一致；ADR-0001/0002/0003/0004、KAD-03/04/06/08/09 引用有效。 |
| V1.14 | 2026-08-21 | 设置页 UI 对账（以代码为事实源，用户触发审计）：§1.1 `/settings` 导航摘要更新；§9 按当前页面结构重写（服务/显示/系统三组，仲裁模式选项卡片 + 主题预览入口 + 接入保护标签）；§2.2 移除不存在的 `themeMode`；§14 补 U-08。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5（含新增 `open-config`）；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003/0004、KAD-03/06/08/09 引用有效。 |
| V1.13 | 2026-08-21 | G-06 开机自启实装对账（KAD-09 / ADR-0004）：§9.4 系统设置由"P1 禁用态"改为真实切换（`update_config` 先 OS 后 config、`AUTOSTART_FAILED` 失败路径、启动校准）；§14 G-06 标记完成。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致（含新增 `AUTOSTART_FAILED`）；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003/0004、KAD-03/06/08/09 引用有效。同步修正版本头漂移（V1.10 → V1.13）。 |
| V1.12 | 2026-08-21 | 断连 UX 闭环：§2.1 `device-connection-changed` payload 扩展 `reason` / `reconnecting`（值域 `link_lost` / `reconnect_failed`）；A.4 断连宽限标注前端 `Reconnecting` 视觉态与 Toast 已实装。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003、KAD-03/06/08 引用有效。 |
| V1.11 | 2026-08-21 | 产品形态调整：§10.1 首次启动改为"托盘常驻 + 启动即显示主窗口"（RunEvent::Ready → show + focus；macOS Dock 不显示），关窗后由托盘唤回。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003、KAD-03/06/08 引用有效。 |
| V1.10 | 2026-08-21 | G-04 托盘实装对账：§2.1 新增 `config-changed` / `open-config` 事件（均 ✅）；§12 托盘更新为已实装（P1 口径确认，原 V2 标注作废），图标占位待替换、U-05 待实机；§14 G-04 标记完成。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003、KAD-03/06/08 引用有效。 |
| V1.9 | 2026-08-21 | 实现状态对账（G-01~G-03 闭环）：§2.1 事件表 `device-connection-changed` / `device-power-changed` / `device-fault` 全部改为 ✅；§4.2 握手流程标注已实现；§4.4 故障告警链路已接线；§14 G-01~G-03 标记完成；A.4 阶段 1~8 已实现，断连宽限客户端链路已实现（前端 Reconnecting 视觉态待办）。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003、KAD-03/06/08 引用有效。 |
| V1.8 | 2026-08-21 | 实现状态对账（以代码为事实源，用户触发审计）：§2.1 事件表新增实现状态列（`device-power-changed` / `device-fault` 未 emit）；§4.2 握手流程按实际修正（DEVICE_READY 等信息读取未接线）；§4.4 / A.4 标注未实现链路；§12 托盘修正为"本体与菜单均未实装"并记录 P1/V2 口径冲突；§14 待办表新增 G-01~G-06 实现缺口。5 项语义硬检查通过：Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题字段与 theme-format 字段表一致；ADR-0001/0002/0003、KAD-03/06/08 引用有效。 |
| V1.7 | 2026-08-21 | 对齐报告：主题定义迁移为 Rust DTO + JsonSchema 单一来源后完成 5 项语义硬检查。Source Events、AppError.code、蓝牙 result code 均与上游契约一致；主题编辑字段完整存在于 DTO 生成的 Theme Schema；ADR/KAD 引用有效。强类型 `Curve` / `EndLevel` 保持既有 JSON 字符串，`LedTrackDef` 条件约束由 `oneOf` 表达，因此 UI 契约无需变更。 |
| V1.6 | 2026-08-21 | 对齐报告：Theme JSON Schema 与主题指南落地后完成 5 项语义硬检查。Source Events 均存在于 ipc-contract §5；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致（保留值 0x08 无 UI 行为）；主题编辑字段均存在于 theme-format 与 Theme Schema；ADR-0001/0002/0003、KAD-03/06/08 引用有效。同步修正文档版本头漂移。 |
| V1.5 | 2026-08-21 | 主题个性化重构：简单/进阶改为快速创作/轨道工作台；快速创作采用运动、速度、灯序、声音等用户语言，支持自定义状态和草稿三灯预览；`brightness` / `volume` 统一为 0~100。对齐报告：Source Events、AppError、蓝牙 result code、theme-format 字段、ADR/KAD 引用五项检查通过；修复主题完整示例和内置同名覆盖语义漂移。 |
| V1.4 | 2026-08-20 | 对齐报告：完成前端实现后的 5 项语义硬检查。Source Events 与 ipc-contract §5 一致；AppError.code 与 ipc-contract §4 一致；蓝牙 result code 与 V0.4 §3.6 一致；主题编辑字段与 theme-format 字段表一致；ADR-0001 / KAD-03 / KAD-06 引用有效。同步确认 5 个主业务导航 + 设置入口，并补齐 `badgeOrientation` IPC 持久化。 |

---

*文档结束。修改交互流程请同步更新本文与 [ui-design.md](./ui-design.md)。*

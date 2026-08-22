# AI-Light 交互说明（UI Interactions）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.16 |
| 文档状态 | 生效；已按代码实现状态对账（V1.16，2026-08-22） |
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
| `/integrations` | 接入外部工具 | 配置 Claude Code / Codex 等的 hook |
| `/themes` | 主题中心 | 浏览 / 切换 / 编辑主题 |
| `/preview` | 试听 | 手动触发任意状态以验证灯效 |
| `/settings` | 设置 | 服务（端口 / 仲裁模式 / 接入保护）+ 显示（外观模式 / 灯组朝向 / 当前主题）+ 系统（开机自启） |

切换：单击 sidebar 任意项 → 对应 page-section 激活（其余隐藏）。

### 1.2 顶部区域

**当前版本无顶部条**。所有交互均通过 sidebar + 页面内操作完成。设计上避免"演示/调试入口"出现在生产路径中。

---

## 2. 通用交互模式

### 2.1 实时事件流

后端通过 Tauri events 推送变化，前端订阅：

| Event | Payload | 受影响的 UI | 实现状态 |
|---|---|---|---|
| `business-state-changed` | `{ state, source, session, sinceTs, theme }` | Dashboard 红绿灯徽章 + 状态名 + 副标题 | ✅ Rust 已 emit |
| `device-connection-changed` | `{ connected, address, name, reason?, reconnecting? }` | Dashboard 设备卡 + Sidebar 底部「已连接」状态 + Devices 页重连中卡 | ✅ 连接 / 断连 / 重连放弃均已 emit（断连时清空电源字段） |
| `device-power-changed` | `{ batteryPercent, powerSource, chargeState, powerFlags }` | Dashboard 设备卡电量格 | ✅ 握手 GET_POWER_STATUS + POWER_CHANGED 主动事件均已 emit |
| `device-fault` | `{ source, code, context }` | Devices 页告警卡 | ✅ FAULT_EVENT 已接线并 emit |
| `theme-changed` | `{ name }` | Dashboard 主题卡 + Sidebar 底部「当前主题」 | ✅ Rust 已 emit |
| `config-changed` | 更新后完整 Config | 全局配置同步（含托盘徽章朝向单选勾选） | ✅ 设置 / 托盘修改后 emit |
| `open-config` | — | AppShell 跳转 /devices | ✅ 托盘「打开配置」emit（UI 导航事件） |

**初始化流程**：打开主窗口 → 自动调 `get_app_state()` 拉全量快照 → 订阅 events 接收增量。

### 2.2 配置写入

任何"修改后立即生效"的设置（外观模式、灯组朝向、自启动、仲裁模式、主题名、主题编辑）走：

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
显示 Progress 条 + 「正在查找附近的灯牌…」
  ↓
结果列表分 3 类：
  ├ AgentCore 灯牌（ACLight-* 开头）：[连接] 按钮
  ├ 同名其它 AgentCore：与上一类同
  └ 非灯牌蓝牙设备：灰显 + 「不是灯牌，无法连接」
```

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

`P2`：当前版本未提供 UI 入口；通过电源切断 / 走远超时实现。

### 4.4 故障告警

> ✅ 实现状态：FAULT_EVENT (0xEF) 已接线，Rust 收到后 emit `device-fault`，Devices 页告警卡即可出现。

收到 `device-fault` event → Devices 页顶部插入红色 Alert 卡：
- 标题：「设备故障事件」
- 内容：设备名 + 故障源（LED / 蜂鸣器 / 电源 / 协议内部）+ code
- 不阻塞其它 UI；可手动关闭（V2 加 Alert dismiss）

---

## 5. 接入外部工具（`/integrations`）

### 5.1 4 个客户端卡

| 客户端 | 状态 tag（默认） | 配置文件 | 接入方式 |
|---|---|---|---|
| Claude Code | 已配置 / 未配置 | `~/.claude/settings.json` | 🟢 HTTP hook 直连 |
| Codex | 未配置 | `~/.codex/hooks.json` + `~/.codex/config.toml` | 🟢 command（curl） + notify |
| Qoder | 预留 | `~/.qoder/settings.json` | 🟢 command（curl，与 Claude Code 同构）|
| Cursor | 预留 | （桥接进程）| 🟡 桥接（第一期不接）|

### 5.2 操作

每张卡含 3 个动作：
- [测试连接] → 后端向本工具的配置路径 POST 一个测试 hook 事件，Dashboard 5 秒内应看到红绿灯变化
- [▸ 查看 JSON 配置] → 折叠显示完整配置代码（已折叠）
- [复制] → 一键复制配置代码到剪贴板

### 5.3 Codex 特殊说明

- ⚠️ Codex Desktop 会重写 `notify` 配置（包装成 Computer Use），可能丢失用户的 ai-light 项
- 建议：纯 CLI Codex 用户才用此卡；Desktop 用户标记为「不兼容」

### 5.4 配置生效流程

```
用户 [复制] 配置 → 粘贴到对应文件 → 重启工具
  ↓
工具启动 → 第一次 hook 触发 → POST http://127.0.0.1:47800/hook
  ↓
AI-Light 收到 → 仲裁 → 主题映射 → SCENE 下发 → 灯亮
  ↓
UI 事件流：business-state-changed → Dashboard 红绿灯变化
```

---

## 6. 主题中心（`/themes`）

### 6.1 浏览

- 6 张内置主题卡（默认 / 极简 / 专注 / 自然 / 极光 / 霓虹）
- 主题卡含：主题名 + 中文描述 + 缩略图（3 灯条色块）+ [使用此主题] / [正在使用] 按钮
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

---

## 7. 主题创作器 Dialog

### 7.1 进入与默认模式

- 入口：`/themes` → [以当前主题创建] → Dialog 打开
- **面向最终用户**：默认不暴露协议术语（SCENE / curve / 相位 / 占空比 / JSON），只展示"动效、速度、颜色、顺序、声音"五个用户语言分组
- 标题栏：主题名称输入 + [进阶参数] 切换器（默认收起）
- 内置主题始终另存为用户主题，且不得与内置主题同名
- 右侧始终有**软件动画预览**：按草稿真实曲线 / 周期 / 相位 / 亮度模拟三灯 + 蜂鸣，不依赖设备

### 7.2 一屏一状态

先选状态，再为该状态设计一套灯效：

```
┌─ 状态 chips（中文名 + 英文编码 + 状态色点）──────────┐
│ [空闲 IDLE] [工作中 WORKING] [等你回复 WAITING]       │
│ [完成 SUCCESS] [出错 ERROR]                          │
└────────────────────────────────────────────────────┘
新增状态 / 删除当前自定义状态    │   从其他主题借用效果（折叠面板）
─────────────────────────────────────────────────────────
灯光怎么动？    常亮 / 呼吸 / 闪烁 / 流动 / 渐亮 / 渐弱   ← 6 张带波形图示卡片
变化速度        舒缓(≈2.8s) / 适中(≈1.4s) / 活跃(≈0.6s) ← 仅非"常亮"时出现
三颗灯的颜色与亮度  顶灯 / 中灯 / 底灯 各一行：颜色 + 亮度滑块；熄灭灯显示"熄灭"虚线
三灯怎么依次出现？ 一起 / 上→下 / 下→上 / 交错           ← 仅非"常亮"时出现
提示声音        无声 / 轻提示 / 确认音 / 警报音            ← 4 张卡片
```

- 标准 5 态固定保留，允许添加、删除自定义状态
- `<brightness> = 0` 表示该灯全黑；`leds[i] = null` 表示该灯熄灭（预览显示灰暗状态）
- 动效判定基于**当前场景的主导曲线**（第一条非熄灭灯轨的 `curve`），而非当前选中灯
- 修改自动保存为本机草稿；重新打开同一来源主题时恢复
- 设备已连接时显示「在设备上试听」，将未保存草稿交给 `preview_scene(content)` 并以 `RESTART_SCENE` 重播
- 「从其他主题借用效果」可把任意内置/用户主题的标准状态效果复制到当前状态，形成混搭主题

### 7.3 进阶参数（可选，默认收起）

点击 [进阶参数] 展开，仅高级用户使用，继续使用用户语言：

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
[展开进阶] → 显示逐灯精确字段；数据保留
[收起进阶] → 隐藏精确字段；数据保留
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

5 个按钮：空闲 / 工作中 / 等你回复 / 完成 / 出错。

点击 → `trigger_state(state, meta)`：
1. 后端 `engine::process_event(source='manual', state, None, None)`
2. 走仲裁器（manual 也参与优先级抢占）
3. 主题映射 → SCENE 编译 → SET_SCENE 下发
4. 触发 `business-state-changed`
5. Dashboard 红绿灯立即变化

**注意**：`trigger_state` 与 hook 事件走同一路径，不绕过仲裁（保证一致性）。

### 8.2 自定义状态

输入框：自定义状态名（如「审查代码」）
[触发] → 同上，但状态名不在 5 态中 → 走主题映射：
- 主题有映射 → 按映射灯效亮起
- 主题无映射 → fallback IDLE（全灭）+ Toast「该状态未在主题中映射」

### 8.3 最近用过

最近 5 个自定义状态以快捷按钮呈现，点击即触发。无需手动输入。

### 8.4 关闭灯效

[关闭灯效] → `reset_outputs()`：
- 设备端：灯全灭、蜂鸣停止、清空当前 SCENE
- 业务状态：复位为 IDLE
- 触发 `business-state-changed { state: IDLE }`

---

## 9. 设置（`/settings`）

页面分三组卡片（服务 / 显示 / 系统），每行 = 图标 + 名称 + 一行用户友好说明 + 控件；页脚提示"所有设置即时生效并自动保存"。**不含任何开发者向文案**（P1/P2、明文存储、第一版不开放等均不出现）。

### 9.1 服务

- 服务端口：只读显示当前监听端口（"智能体工具连接本机服务的端口，一般无需修改"）。
- 仲裁模式：两张选项卡片（独占选择），选中态 = 绿色描边 + 淡绿底 + 圆点填充；「优先级抢占」带「默认」标签。选项与效果：
  - 优先级抢占：重要状态优先——错误 > 完成 > 进行中 > 等待 > 空闲
  - 最近活跃：最后上报状态的工具接管灯效
  - 切换经 `update_config(arbitrationMode)` 即时生效（引擎热切换）。
- 接入保护：状态标签（"仅限本机" / "Token 已启用"）；第一版 UI 不开放 Token 编辑入口（服务端 Bearer 校验见 hook-api §7）。

### 9.2 显示

- 外观模式（`themeMode`）：三张选项卡片（亮色 / 暗色 / 跟随系统），各带图标 + 一句说明；选中态同仲裁模式卡片（绿描边 + 淡绿底 + 圆点填充）。切换即时生效：
  - 亮色：`<html data-theme="light">`，slate 浅底 + 深绿链接色
  - 暗色：`<html data-theme="dark">`（默认，既有体验不变）
  - 跟随系统：`data-theme` 随 `prefers-color-scheme` 实时切换（含 OS 运行中变更）
  - 持久化经 `update_config(themeMode)` → config.json；重启首帧由 localStorage 引导缓存恢复（config 为事实源）
- 灯组朝向（`badgeOrientation`）：[横排] [纵向] 分段控件，切换即时生效（红绿灯布局直接变）。
- 当前主题：主题入口（Link → /themes）——三个灯色预览点（取当前主题 WORKING/SUCCESS/ERROR 场景实际灯色）+ 主题名 + 「提示音」标记（任一代表场景含蜂鸣时显示）+ 箭头。预览随 `config.activeTheme` 变化刷新，读取失败时回退为纯主题名。

### 9.3 系统

- 开机自启：✅ 已实装（2026-08-21，KAD-09 / ADR-0004）。Switch 真实切换：`update_config` 先 OS 后 config（OS 登录项为唯一事实源，config 为启动校准缓存）；失败返回 `AUTOSTART_FAILED` → Toast + 回滚到原值；重启时 `is_enabled()` 校准写回。

> 未展示项：日志查看（P2）、portPreference 修改（P2 热重启）——页面不渲染，仅本文档记录。

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
  - 启动 L1 hook_server（axum，47800）
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
| 设备扫描失败（蓝牙权限 / 系统错误）| 红色告警条 + 重试按钮 |
| 设备连接失败 | Toast（含原因）+ 保留在 /devices |
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
| G-05 | `portPreference` 读取与热重启 | P1 |
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
| `connected` | 握手完成 | 完整字段 + "已连接" tag（accent） | hover → [断开] P2 |
| `reconnecting` | 链路异常退避重连 | spinner + "重连中...(N/M)" | 禁用 |
| `lowBattery` | `batteryPercent < 20` | 电池格 warning 色 + Toast 警告 | 同 connected |
| `charging` / `full` | `chargeState` 变化 | 电池格 + ⚡ / ✓ 图标 | 同 connected |
| `fault` | 收到 `device-fault` | 红色故障指示条 + tooltip 显示 source/code | 同 connected |

**边界条件**：
- 无电池版：`batteryPercent = null` → "无电池"
- `batteryPercent == 0xFF`（未标定） → `--`
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
| `THEME_BUILTIN` | Dialog "内置主题不可删除" |
| `BAD_REQUEST` | Toast "请求参数非法：`<reason>`" |
| `DEVICE_NOT_CONNECTED` | Toast "请先连接设备" + 跳转 `/devices` |

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
| Integrations | 客户端卡 × 4：[测试连接] → [查看 JSON] → [复制] | — |
| Themes | 编辑当前主题 → 导入新主题 → 主题卡 [使用此主题] × N | Cmd/Ctrl+I = 导入 |
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

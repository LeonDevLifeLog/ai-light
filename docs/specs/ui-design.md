# AI-Light L5 展示层 UI 设计规范

| 项目 | 内容 |
|---|---|
| 文档版本 | V0.1（设计阶段首版） |
| 文档状态 | ⏸ 设计阶段，待用户审阅后定稿 |
| 范围 | L5 展示层（前端 UI + 托盘 + 窗口生命周期） |
| 上游 | [docs/specs/ipc-contract.md V1.0](./ipc-contract.md)、[theme-format.md V1.0](./theme-format.md)、[architecture.md](./architecture.md) KAD-06 |
| 设计方法 | ui-ux-pro-max skill（Dark OLED 基线 + Inter 字体 + 代码深/运行绿调色板） |
| 决策人 | 李昻 / 小艺 |
| 关联 | [docs/requirements/product-boundary.md L5](../requirements/product-boundary.md) |

> 本文是 L5 展示层的"功能全貌 + 设计契约"——前端开发只依赖本文 + ipc-contract。任何 UI 调整需先更新本文。
> 未定稿处标 ⏸；已敲定决策标 ✅。

---

## 1. 目的与读者

- **目的**：把六层边界（L1~L6）中的 L5 落地为前端代码时需要的"功能全貌 + 视觉契约"，逐项记录页面、流程、状态、组件、设计代币、验收剧本
- **读者**：前端开发者（React 侧）、产品设计、主题作者、维护者、未来接手开发的工程师
- **不包含**：接口细节（见 ipc-contract.md）、协议细节（见硬件 V0.4）、已定业务决策（见 ADR-0001/0002/0003）

---

## 2. 产品形态回顾（✅ 已确认）

- **托盘常驻为主**，主窗口可关闭（关窗 = 隐藏，非退出）— KAD-06
- 启动时主窗口同时打开（/ Dashboard）；关窗 = 隐藏，托盘常驻，可随时从托盘唤回窗口
- 单实例（`tauri-plugin-single-instance` 已接入）
- 开机自启（KAD-06 SHOULD，P1 暂缓，应做档跟进）

---

## 3. 设计基线 ✅

### 3.1 Style — Dark Mode (OLED) 优先

- **Dark 优先**：开发者工具，弱光环境常用；OLED 屏省电护眼
- 浅色模式作为可选项（P2 暂缓）
- 调色板语义：代码深 + 运行绿（与"指示灯亮起"的视觉隐喻一致）

### 3.2 调色板（采纳 ui-ux-pro-max 推荐）

| 角色 | Hex | CSS Variable | 用途 |
|---|---|---|---|
| Primary | `#1E293B` | `--color-primary` | 主要操作色 |
| On Primary | `#FFFFFF` | `--color-on-primary` | 主按钮文字 |
| Secondary | `#334155` | `--color-secondary` | 次级操作 |
| Accent / CTA | `#22C55E` | `--color-accent` | 主强调（运行中绿） |
| Background | `#0F172A` | `--color-background` | 页面背景（OLED 真黑） |
| Foreground | `#F8FAFC` | `--color-foreground` | 主要文字 |
| Muted | `#272F42` | `--color-muted` | 暗化表面 / 未激活灯位 |
| Border | `#475569` | `--color-border` | 分割线 / 边框 |
| Destructive | `#EF4444` | `--color-destructive` | ERROR 状态 / 危险操作 |
| Ring | `#1E293B` | `--color-ring` | 焦点环 |

**业务状态语义色（在 token 之上叠加）**：

| 状态 | 颜色 | 来源 |
|---|---|---|
| IDLE | 全部熄灭 | — |
| WORKING | `#22C55E`（accent） | 运行中绿 |
| WAITING | `#F59E0B`（amber-500） | 警告黄 |
| SUCCESS | `#22C55E`（accent） | 与 WORKING 同色，靠动画区分 |
| ERROR | `#EF4444`（destructive） | 错误红 |

### 3.3 字体 ✅

- **Inter** 全栈：Heading + Body 共用，权重 300 / 400 / 500 / 600 / 700
- 数据列（IP、地址、时间戳、端口号）使用 tabular-nums
- 字号 scale：12 / 14 / 16 / 18 / 24 / 32
- 行高：body 1.5、heading 1.2~1.3

### 3.4 效果

- 极简 glow（box-shadow / text-shadow `0 0 10px`）—— 用于状态变化时
- 主题切换：**瞬切**（用户决策 ✅）
- 其他过渡：150-300ms ease-out
- 严格遵循 `prefers-reduced-motion`（reduced 时关闭呼吸/闪烁）

---

## 4. 信息架构（IA）

### 4.1 顶级结构 ✅

主窗口采用 **侧边栏 + 主区** 布局：

```
┌──────────────────────────────────────────────────────────┐
│ [⌂] AI-Light                                  [— □ ×]   │
├────────────┬─────────────────────────────────────────────┤
│ Sidebar    │  Main Content                               │
│ ────────   │  ────────────                               │
│ ⚡ 状态    │                                              │
│ 📡 设备    │  <页面内容>                                  │
│ 🔗 接入    │                                              │
│ 🎨 主题    │                                              │
│ 🔔 试听    │                                              │
│ ⚙ 设置    │                                              │
│            │                                              │
│ ────────   │                                              │
│ v0.1.0     │                                              │
│ 端口:47800 │                                              │
└────────────┴─────────────────────────────────────────────┘
```

- 5 个主业务导航项（状态 / 设备 / 接入 / 主题 / 试听）+ 设置入口
- 侧边栏固定宽度 220px；底部展示版本号 + 当前 hook 服务端口

### 4.2 路由表 ✅

| 路径 | 页面 | 优先级 | 关联 commands | 关联 events |
|---|---|---|---|---|
| `/` | 状态总览（Dashboard） | P1 | `get_app_state` | `business-state-changed` / `device-connection-changed` / `device-power-changed` / `theme-changed` |
| `/devices` | 设备管理 | P1 | `scan_devices`, `connect_device` | `device-connection-changed` / `device-power-changed` / `device-fault` |
| `/integrations` | 接入外部工具 | P1 | `get_app_state`（读取当前服务端口） | — |
| `/themes` | 主题中心 | P1 | `get_themes`, `get_theme`, `set_active_theme`, `import_theme` | `theme-changed` |
| `/preview` | 试听面板 | P1 | `trigger_state`, `preview_scene`, `reset_outputs` | `business-state-changed` |
| `/settings` | 设置 | P1 | `get_config`, `update_config` | — |
| `*` | 404 | — | — | — |

---

## 5. 各页面功能详解

### 5.1 状态总览（`/`）— Dashboard ✅

**目的**：让用户瞄一眼就能看到当前 AI 工作状态 + 设备连接状态 + 主题。

**布局**：

```
┌─────────────────────────────────────────────────────────┐
│ AI-Light                              [≡ 历史] [—]     │
├─────────────────────────────────────────────────────────┤
│                                                         │
│              ● ● ●          ← 红绿灯式状态徽章（详见 5.1.1）│
│              R Y G                                       │
│                                                         │
│              WORKING                                     │
│              source: claude-code                        │
│              since 12:34:56                             │
│                                                         │
│  设备                                                   │
│  ┌────────────────────────────────────────────┐         │
│  │ 🟢 ACLight-1A2B           电量 75%       │         │
│  │ fw1.0.0 · 蓝牙 · 5秒前更新               │         │
│  └────────────────────────────────────────────┘         │
│                                                         │
│  主题                                                   │
│  ┌────────────────────────────────────────────┐         │
│  │ neon · WORKING 预览 · [换主题 →]          │         │
│  └────────────────────────────────────────────┘         │
└─────────────────────────────────────────────────────────┘
```

#### 5.1.1 红绿灯式状态徽章 ✅

**视觉**：3 个圆形灯位（红 R / 黄 Y / 绿 G）组成的红绿灯，模拟现实红绿灯的视觉语言，与产品名 "AI-Light" 的"灯"语义对齐。

```
横向（默认）：  ● ● ●          纵向：        ●
              R  Y  G                      ●
                                          ●
```

**业务状态 → 灯位颜色/动画 映射**：

| 业务状态 | 红灯 R | 黄灯 Y | 绿灯 G | 备注 |
|---|---|---|---|---|
| `IDLE` | 灭 | 灭 | 灭 | 全灭 |
| `WORKING` | 灭 | 灭 | **绿 呼吸（2s 周期，ease-in-out）** | 表达"进行中" |
| `WAITING` | 灭 | **黄 常亮** | 灭 | 等待用户输入 / 权限 |
| `SUCCESS` | 灭 | 灭 | **绿 常亮** | 持续到下一事件或 `hold_ms` 回落 |
| `ERROR` | **红 闪烁 1Hz** | 灭 | 灭 | 持续到下一事件或 `hold_ms` 回落 |
| 自定义状态 | 按主题 SCENE 映射 | | | 未映射 → 全灭（fallback IDLE） |

> **WORKING vs SUCCESS 区分**：两者都用绿灯，靠"呼吸 vs 常亮"区分。这是红绿灯语义的常见延伸（绿灯既可"通行"也可"进行中"）。

**视觉参数**：

| 项 | 值 |
|---|---|
| 灯直径 | 横向 32px / 纵向 24px |
| 激活颜色 | 红 `#EF4444` / 黄 `#F59E0B` / 绿 `#22C55E` |
| 未激活颜色 | `#272F42`（slate-700） + opacity 0.4 |
| 横向灯心距 | 56px（灯间 24px） |
| 纵向灯心距 | 44px（灯间 20px） |
| 状态名位置 | 灯组下方居中，Inter 600 / 24px / foreground |
| 源标签 | 状态名下方 14px / muted |

**朝向偏好（用户设置项）**：

- 新增 `config.json` 字段：`badgeOrientation: "horizontal" | "vertical"`（默认 `"horizontal"`）
- 入口：
  - **托盘菜单** → "徽章朝向" 子菜单（横/纵，单选，实时生效）
  - **Settings 页** → "显示" 区 → 单选控件
- 持久化：通过 `update_config({ badgeOrientation: "vertical" })`

**托盘图标**：P1 **不变色**（mac 菜单栏单色 / win 通知区彩色但动画不可见 / linux 依赖 DE）。仅在托盘菜单文字里显示"当前状态：WORKING"作为补充。V2 再考虑彩色支持成熟时升级。

#### 5.1.2 事件订阅

- `business-state-changed` → 红绿灯徽章 + 状态名 + 源标签 + since 时间戳同步更新
- `device-connection-changed` / `device-power-changed` → 设备卡更新
- `theme-changed` → 主题卡更新

### 5.2 设备管理（`/devices`）✅

**目的**：扫描附近设备、连接/断开、查看连接历史。

**布局**：

- 顶部操作区：[重新扫描] 按钮（点击触发扫描，扫描中显示 5s 倒计时 Progress）
- **扫描结果列表**：设备名 / 地址 / RSSI / recognized 标识（协议 §2.1 前缀匹配）/ [连接] 按钮
  - 空态："未发现 AgentCore-Light 设备，请确认灯牌已上电"
  - 错误态：扫描失败的错误提示 + [重试]
- **已连接区**：当前连接设备信息卡
  - 字段：设备名 / 地址 / 电量 / 固件 / 硬件版本 / 信号强度
  - 操作：[断开]（P2）/ [忘记设备]（P2）
- **设备故障区**：FAULT_EVENT 触发的红色 Alert

**关键流程**：

- **首次进入**：`/devices` → 自动触发首次扫描（不阻塞渲染）
- **点击 [连接]**：
  ```
  触发 → scan(adapter, 5s) → connect_device_internal(address, name)
       ↓ 成功 → emit device-connection-changed{true} → 引擎 resync
       ↓ 失败 → Toast 错误提示（含原因）+ 保留在 /devices
  ```

### 5.3 主题中心（`/themes`）✅

**目的**：浏览/切换内置与用户主题，导入新主题。

**布局**：

- 顶部：[导入主题] 按钮（打开 Dialog：文件选择器 或 粘贴 JSON 文本框）
- **主题网格**（响应式 2~4 列卡片）：
  - 每张卡片：主题名 / builtin or user 徽章 / 缩略灯效示意（红绿灯静态预览）/ [应用] 按钮
  - 内置主题 6 张卡片（default / minimal / neon / nature / aurora / focus）
  - 用户主题 N 张（按文件系统枚举）
- **右侧详情面板**（选中主题时展开）：
  - 完整 JSON 内容（折叠展示）
  - 状态→SCENE 映射表（表格）
  - 当前生效 SCENE 实时预览动图

**关键流程**：

- **选择主题 → [应用]**：
  ```
  set_active_theme(name)
    ├─ 校验失败 → 错误 Toast（不切换主题）
    └─ 校验通过 → 写 config + emit theme-changed
         ├─ 当前业务 IDLE → 仅 UI 更新
         └─ 当前业务非 IDLE → 重编译 SCENE 并下发（APPLY_IF_CHANGED）
  ```
- **导入主题**：
  ```
  选择 .ailight-theme.json 文件 → import_theme(content)
    ├─ 解析失败 → Dialog 错误提示
    ├─ 与内置同名 → 错误提示（CONFLICT）
    └─ 成功 → 网格自动刷新 + Toast 成功提示
  ```
- **编辑主题**：P1 已做 UI 编辑器（默认简单模式 + 进阶模式分6 步骤：波形/颜色/节奏/三灯关系/蜂鸣/重复与终态）

### 5.4 试听面板（`/preview`）✅

**目的**：手动触发任意业务状态（含自定义）以验证灯效 / 调试主题。

**布局**：

- 顶部：当前激活主题显示（只读）
- **标准状态按钮组**（5 个）：
  - `[ IDLE ] [ WORKING ] [ WAITING ] [ SUCCESS ] [ ERROR ]`
  - 点击 → `trigger_state(state, meta)` → 走仲裁 → 事件触发灯效
- **自定义状态输入**：
  - Input + [触发] 按钮
  - 支持最近 5 个自定义状态名的快捷按钮组（用户决策 ⏸ #3）
- **预览 SCENE** ✅（P1 已实装：`preview_scene` command + Preview 页"试听当前灯效"按钮）：
  - 按 SCENE 名直接预览，不改变业务状态
  - 设备未连接时禁用 + Tooltip 提示
- **[全部重置]** 按钮：`reset_outputs()` → 灯效全停 + 业务状态回 IDLE

**关键交互**：

- 点 [WORKING]：业务状态变为 WORKING（红绿灯徽章切到绿色呼吸），物理灯变化
- 点 [自定义] 输入 "REVIEW"：
  - 主题映射若有 → 灯效按映射变化
  - 主题映射若无 → 全部熄灭（fallback IDLE）+ Toast 提示"该状态未在主题中映射"
- [预览 SCENE]：用 RESTART_SCENE 语义下发，不改变业务状态（Dashboard 红绿灯不动）

### 5.5 设置（`/settings`）✅

**目的**：全局配置入口。

**布局**（分组卡片）：

- **服务**：
  - 端口（显示当前值；修改需重启 → P2 支持 portPreference 修改重启，P1 仅展示）
  - 接入密码（**第一版 UI 不开放**；服务端支持 token 校验，但用户面板无开关，V2 再开放）
  - 仲裁模式（Select：`priority` 默认 / `last_active`）
- **设备**：
  - 记住的设备（显示 address/name；[忘记设备] P2）
- **主题**：
  - 当前激活主题名（点击 → 跳转 /themes）
- **显示**：
  - **徽章朝向**（单选：横向 / 纵向）— 5.1.1 红绿灯徽章
- **其他**：
  - 开机自启（Switch；P1 暂缓——需要 `tauri-plugin-autostart` 启用）
  - 日志目录（只读 + [打开目录] 按钮）

**关键交互**：

- 任意 Switch / Select 变化 → `update_config({ field: value })` → 实时持久化
- 失败回滚：UI 显示原值 + Toast 错误

---

## 6. 关键交互流程 ✅

### 6.1 首次启动

```
启动应用
  ↓
加载 config.json（缺失则用默认）
  ↓
初始化日志 + L1 hook_server + 仲裁 + 引擎
  ↓
检查 remembered_device
  ├─ 有 → 自动连接（带 loading Toast）
  └─ 无 → 静默待命，托盘菜单高亮提示"尚未连接设备"
  ↓
托盘常驻，主窗口不自动弹出
  ↓
用户：托盘菜单 → "显示窗口" → 主窗口出现 / Dashboard
```

### 6.2 主题切换（运行时）

```
用户在 /themes 选择主题 → 点 [应用]
  ↓
set_active_theme(name)
  ├─ 校验失败 → 错误 Toast（不切换）
  └─ 校验通过 → 写 config + emit theme-changed
       ├─ 当前业务 IDLE → 仅 UI 更新（瞬切）
       └─ 当前业务非 IDLE → 重编译 SCENE 并下发（APPLY_IF_CHANGED）
            ↓
       前端：theme-changed event → 主题卡更新 + Dashboard 状态预览刷新
```

### 6.3 设备重连

```
事件：device-connection-changed{connected: false, reason: "link_lost"}
  ↓
UI: Toast 提示"设备已断开"（不阻塞）
  ↓
后端：Rust 侧退避重连（N 次，KAD-07）
  ├─ 成功 → emit device-connection-changed{true} → 引擎 resync → Dashboard 设备卡更新
  └─ 失败 → 提示 + 提供"手动重连"按钮（在 /devices 页）
```

> **注**：重连逻辑在 Rust 侧，UI 仅消费事件；不在前端做兜机。

---

## 7. 状态机

### 7.1 窗口可见性 ✅

```
[Hidden]   --(托盘"显示窗口")-->     [Visible]
[Visible]  --(用户点 X)-->            [Hidden]    // 关窗 = 隐藏（KAD-06）
[任意]     --(托盘"退出")-->         [Terminating]
[Visible]  --(单实例插件新启动信号)--> [Visible]   // 聚焦已有窗口
```

### 7.2 设备状态 ✅

```
[Disconnected]
    --(scan + connect_device)--> [Connecting]
[Connecting]
    --(V0.4 握手成功)-->         [Connected]
    --(超时/失败)-->              [Disconnected]
[Connected]
    --(链路异常)-->              [Reconnecting]
        --(成功)-->              [Connected]
        --(失败)-->              [Disconnected]
[任意]
    --(forget_device P2)-->      [Disconnected] + 清空 config.rememberedDevice
```

### 7.3 业务状态（来自仲裁）✅

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

### 7.4 主题朝向偏好 ✅

```
[Horizontal] (默认)
    --(用户切到 Vertical / Settings / 托盘菜单)--> [Vertical]
[Vertical]
    --(用户切到 Horizontal)--> [Horizontal]
    ↓
persisted to config.json via update_config
```

---

## 8. 组件选型清单（shadcn/ui 待补）

| 组件 | 用途 | 状态 |
|---|---|---|
| Button | 全局 | ✅ 已装 |
| Tooltip | 状态徽章 hover / 禁用按钮说明 | ✅ 已装 |
| Card | 主题 / 设备 / 设置分组卡 | P1 需新增 |
| Sidebar | 主导航（侧边栏） | P1 需新增 |
| Toast (Sonner) | events 提示 / 操作反馈 | P1 需新增 |
| Dialog | 导入主题确认 / 断开确认 / 主题详情 | P1 需新增 |
| Badge | builtin/user 徽章 / 状态标签 | P1 需新增 |
| Switch | 自启动 / 仲裁模式切换 | P1 需新增 |
| Select | 仲裁模式 / 主题选择 / 朝向选择 | P1 需新增 |
| Input | 自定义状态名 / 主题名 | P1 需新增 |
| ScrollArea | 扫描结果列表 / 主题网格 | P1 需新增 |
| Progress | 扫描倒计时 | P1 需新增 |
| Alert | 错误提示 / 设备故障 | P1 需新增 |
| Separator | 设置分组分隔 | P1 需新增 |

**图标库**：统一使用 **Lucide**（已与 shadcn/ui 生态一致）。

**新增组件数**：12 个（Card / Dialog / Badge / Switch / Select / Input / ScrollArea / Progress / Alert / Separator / Sonner / Sidebar 组合）。

---

## 9. 无障碍 & 跨平台适配

### 9.1 桌面窗口自适应

- 最小窗口尺寸：800 × 500
- 默认窗口尺寸：1000 × 600（已配置，见 tauri.conf.json）
- 窗口 resize 时布局自适应（CSS Grid / Flexbox）
- 最小宽度缩到 800 以下时，Sidebar 自动折叠为顶部汉堡菜单（V2）

### 9.2 键盘导航 ✅

- Tab 顺序 = 视觉顺序
- Esc 关闭最上层 Dialog
- Enter 提交表单 / 触发默认按钮
- 焦点环：可见（2-4px，遵循 `focus-states` 规则）

### 9.3 颜色对比 ✅

- 暗色模式 4.5:1（WCAG AA 正文）
- 亮色模式 4.5:1（WCAG AA 正文，V2 启用）
- 红绿灯激活灯位 vs `#0F172A`：≥ 4.5:1
- 未激活灯位 vs 背景：≥ 3:1（次要信息）

### 9.4 平台原生托盘

- **macOS**：菜单栏图标（模板图，单色）+ 菜单；应用激活策略为 Accessory——Dock 不显示图标，关窗 = 隐藏、退出只经托盘（KAD-06）
- **Windows**：通知区图标 + 菜单
- **Linux**：依赖 DE（GNOME 扩展 / KDE 原生），需各 DE 兼容测试（U-05）

### 9.5 托盘菜单结构 ✅

```
┌─ 显示窗口 ───────────┐
│ 当前状态：WORKING     │ ← 动态
│ 当前主题：neon        │ ← 动态
├─ 徽章朝向 ───────────┤
│ ● 横向               │ ← 单选
│ ○ 纵向               │
├─ 打开配置 ───────────┤
│ 退出 ───────────────┘
```

> ✅ 已实装（2026-08-21）：菜单由 Rust 侧直接构建（`src-tauri/src/tray.rs`），动态文字经业务事件更新；「打开配置」emit `open-config` 由前端跳转 /devices；图标复用应用图标占位（mac 模板图单色），正式托盘图标待替换。

---

## 10. 设计代币（Design Tokens）✅ 仅文档记录

> **决策**：本次仅文档记录，不落 Tailwind 配置（首版 V0.1 评审后再统一落代码）。

### 10.1 颜色 token

```text
color/primary       #1E293B
color/on-primary   #FFFFFF
color/secondary    #334155
color/accent       #22C55E   /* WORKING / SUCCESS / 运行中 */
color/background   #0F172A   /* OLED 真黑 */
color/foreground   #F8FAFC
color/muted        #272F42   /* 未激活灯位 / 次要表面 */
color/border       #475569
color/destructive  #EF4444   /* ERROR / 危险操作 */
color/ring         #1E293B   /* 焦点环 */

/* 业务状态色（语义层） */
status/idle        = color/muted
status/working     = color/accent      (#22C55E)
status/waiting     = #F59E0B           (amber-500)
status/success     = color/accent      (#22C55E)
status/error       = color/destructive (#EF4444)
```

### 10.2 字体 token

```text
font/family     = "Inter", system-ui, sans-serif
font/weight/light    = 300
font/weight/normal   = 400
font/weight/medium   = 500
font/weight/semibold = 600
font/weight/bold     = 700

font/size/xs    = 12px
font/size/sm    = 14px
font/size/base  = 16px
font/size/md    = 18px
font/size/lg    = 24px
font/size/xl    = 32px

line-height/body   = 1.5
line-height/heading = 1.25
```

### 10.3 间距 / 圆角 / 阴影 / 过渡

```text
spacing/rhythm = 4 / 8 dp 增量体系

radius/sm   = 4px
radius/md   = 8px
radius/lg   = 12px
radius/full = 9999px   /* 红绿灯灯位 */

shadow/sm = 0 1px 2px rgba(0,0,0,0.3)
shadow/md = 0 4px 8px rgba(0,0,0,0.4)
shadow/lg = 0 10px 24px rgba(0,0,0,0.5)

duration/fast = 150ms
duration/base = 200ms
duration/slow = 300ms
easing/standard = cubic-bezier(0.2, 0, 0, 1)   /* ease-out */
easing/enter    = ease-out
easing/exit     = ease-in

/* 红绿灯灯位 */
light/diameter/horizontal = 32px
light/diameter/vertical   = 24px
light/gap/horizontal      = 24px (灯间)
light/gap/vertical        = 20px (灯间)
light/breath-cycle        = 2000ms (WORKING)
light/blink-cycle         = 1000ms (ERROR, 1Hz)
```

### 10.4 Z-index 层级

```text
z/base       = 0
z/raised     = 10
z/sticky     = 20
z/overlay    = 40    /* Toast */
z/modal      = 100   /* Dialog */
z/popover    = 200
z/tooltip    = 300
```

---

## 11. 功能路线图与实现状态对账（2026-08-21 以代码为事实源修订）

### 11.1 必做（MVP 闭合）✅ 高优先级

- **托盘常驻服务**：✅ 已实装（2026-08-21）。图标 + 菜单（显示窗口 / 当前状态 / 当前主题 / 设备 / 徽章朝向 / 打开配置 / 退出）+ 动态联动全部落地；图标复用应用图标占位待替换；三平台行为验证（U-05）待实机
- **主窗口 5 页 UI**：✅ 已实装（状态总览 / 设备管理 / 主题中心 / 试听 / 设置）
- **12 个 P1 commands 前端对接**：✅ 已实装（`get_app_state` / `get_themes` / `get_theme` / `set_active_theme` / `import_theme` / `scan_devices` / `connect_device` / `trigger_state` / `preview_scene` / `reset_outputs` / `get_config` / `update_config`）
- **P1 events 订阅**：✅ 前端已订阅 5 个 P1 events，Rust 全部已 emit（含断连方向、`device-power-changed`、`device-fault`）。`hook-log` 为 P2
- **红绿灯徽章组件**（核心视觉）+ 朝向偏好设置：✅ 已实装

### 11.2 应做（产品完整性）

- **`portPreference` 实际读取与重启**：❌ 未实现。config 有字段但 `hook_server::serve` 固定 47800 起退避、不读取；`update_config` 不允许修改（P1 只读）
- **token Bearer 校验**：✅ 已实现。hook_server 在配置 token 后强制 Bearer 校验，含 `token_auth` 单测；UI 设置入口按设计不开放（V2）
- **`autostart` 真实启用**：❌ 未实现。`update_config` 仅持久化字段，未接 `tauri-plugin-autostart`（KAD-06 SHOULD）；Settings 页展示禁用态
- **`badgeOrientation` 设置项**：✅ 已实现（Settings 页 + Dashboard 实时生效 + config 持久化）
- **P2 commands**：❌ 全部未实现。`export_theme` / `delete_theme` / `disconnect_device` / `forget_device` / `hook-log` event（ipc-contract §7）

### 11.3 待定（V2 / 远期）

- **`direct_scene`** 高级直控通道：❌ V2 未实现（ADR-0001 Q9 预留枚举；hook 仅接受 `state_change`）
- **主题编辑器 UI** ✅（P1 已实装：快速创作 + 轨道工作台；见 §13 决策表 #5 对账）
- **接入密码 UI**：❌ 未实现（第一版不开放，V2 重新评估）
- **token 系统钥匙串**：❌ V2 未实现（U-07，迁 mac Keychain / win Credential Manager / linux secret-service）
- **Tauri updater 在线升级**：❌ V2 未实现（需签名，L6 V2）
- **多设备并发**：❌ V2 未实现（当前注册表只预留单灯）
- **浅色模式**：❌ P2 未实现（用户决策 ⏸ #4）
- **Settings 自启动 Switch**：❌ 未实现（当前展示禁用态，待 tauri-plugin-autostart 启用）

### 11.4 三平台实测（影响 release 阻塞）

- **U-01**：btleplug 三平台冒烟（mac / win / linux，Win 服务缓存规避 + Linux BlueZ 依赖）
- **U-02**：axum 编译体积/编译时间对比
- **U-05**：托盘图标三平台差异（mac 菜单栏 / win 通知区 / linux DE 兼容）
- **Codex Desktop notify 重写冲突**（GitHub #28404）实测
- **Codex WAITING 缺口**：Codex 无 idle 事件，WAITING 语义只能近似（PermissionRequest + 回合未结束判定）—— 适配器层启发式

### 11.5 文档 / 适配器

- **`docs/specs/adapters/` 目录**：❌ 目录不存在，配置模板未回填（**ADR-0001 Q6 延后项**）

---

## 12. 验收剧本

> **实现状态对账（2026-08-21）**：涉及托盘的步骤（12.1 步骤 1/2/5/8、12.3 步骤 5、12.4 步骤 1）**已实现**（启动即显示主窗口 + 托盘常驻 + 菜单动态文字）；托盘图标为应用图标占位，待正式素材替换。**12.5 断连与重连全链路已实现**（断连事件 + `Reconnecting` 视觉态 + 断连/重连 Toast + 5 次退避重连）；实机验证待 U-01。

### 12.1 首次启动

1. 启动 → 托盘出现 + **主窗口同时打开**（/ Dashboard）
2. 关闭窗口 → 窗口隐藏、托盘常驻；托盘菜单 → "显示窗口" → 主窗口重新出现
3. Dashboard → 红绿灯全灭（IDLE）
4. 设备卡显示"未连接"
5. 托盘菜单 → "打开配置" → 跳转 /devices
6. 自动扫描 → 选中设备 → [连接] → 设备卡更新
7. 关窗口 → 窗口隐藏（托盘仍存）
8. 托盘菜单 → "退出" → 进程终止

### 12.2 hook 触发闭环

1. 终端执行：
   ```bash
   curl -X POST http://127.0.0.1:47800/hook \
     -H 'Content-Type: application/json' \
     -d '{"source":"manual","event":"state_change","state":"WORKING"}'
   ```
2. Dashboard → 红绿灯绿色开始呼吸，状态名 "WORKING"，源标签 `source: manual`
3. 物理灯变化（如有设备连接）
4. 关闭主窗口 → 灯仍保持 WORKING（托盘常驻生效）
5. 重新打开窗口 → 红绿灯仍为绿色呼吸（持久化）
6. `state=ERROR` → 红绿灯红色闪烁

### 12.3 主题切换

1. /themes → 选择"neon" → [应用]
2. 业务状态徽章保留（红绿灯状态不动）—— 主题切换是"映射表"切换，不是"状态"切换
3. 主题卡更新为"neon"
4. 关闭再开窗口 → 主题持久化（来自 config.json 的 `activeTheme`）
5. 托盘菜单"当前主题"实时更新

### 12.4 红绿灯朝向偏好

1. 托盘菜单 → 徽章朝向 → 纵向 → Dashboard 红绿灯立即改为竖排
2. Settings → 显示 → 徽章朝向 → 横向 → Dashboard 红绿灯立即改为横排
3. 关闭再开应用 → 朝向保留（config.badgeOrientation 持久化）

### 12.5 设备断连与重连

1. 物理关闭 AgentCore-Light 灯牌
2. UI → Toast 提示"设备已断开"
3. Dashboard 设备卡显示"已断开"
4. 重新上电灯牌 → 后台退避重连（不可见）
5. 重连成功 → Toast "设备已重新连接" + Dashboard 设备卡恢复
6. 红绿灯徽章（如有业务状态）保留

---

## 13. 待用户决策（设计阶段遗留）

| # | 决策点 | 选项 | 当前 | 备注 |
|---|---|---|---|---|
| 1 | ~~状态徽章视觉~~ | ~~纯色块 / 呼吸 / 静态灯效~~ | **红绿灯式**（详见 5.1.1） | ✅ 已定 |
| 2 | ~~主题切换过渡~~ | ~~瞬切 / 渐变~~ | **瞬切** | ✅ 已定 |
| 3 | 试听面板自定义状态 | 是否加"最近 5 个自定义状态"快捷按钮组 | ✅ 已实装 | 对账：preview.tsx 已实现最近 5 个快捷按钮（本地会话内） |
| 4 | 浅色模式 | P1 不做 / P2 启用 | ⏸ 建议 P2 暂缓 | 设计代币已为亮色预留 |
| 5 | 主题编辑器 UI | P1 仅 JSON 导入 / P2 出 UI 编辑器 | ✅ 已实装（P1） | 对账：themes.tsx 已实现快速创作 + 轨道工作台（V1.5 重构），本行"P1 不做"作废 |

---

## 14. 变更日志

| 版本 | 日期 | 变更 |
|---|---|---|
| V0.1 | 2026-08-20 | 首版设计文档：信息架构 + 5 页面 + 红绿灯徽章方案 + 设计代币 + 路线图 + 验收剧本 |
| V0.2 | 2026-08-21 | 实现状态对账（以代码为事实源）：§11 路线图逐项标注 ✅/⚠️/❌；§12 验收剧本标注未实现步骤；§13 决策表 #3（最近 5 个自定义状态）与 #5（主题编辑器）改为已实装；同步修正预览 SCENE 状态 |
| V0.3 | 2026-08-21 | G-01~G-03 闭环对账：P1 events 5/5 全部 emit（断连双向 / `device-power-changed` / `device-fault`）；§12.5 断连重连后端链路已实现，前端 Toast/Reconnecting 视觉态与实机验证待办 |
| V0.4 | 2026-08-21 | G-04 托盘实装（P1 口径确认）：§9.5 菜单由 Rust 直建并动态联动（原"前端 invoke"设计废止）；§11.1 托盘标记已实装；§12 托盘相关验收步骤已实现（图标占位待替换）；`open-config` / `config-changed` 事件随托盘落地 |
| V0.5 | 2026-08-21 | macOS 平台适配：激活策略设为 Accessory（Dock 不显示图标），托盘常驻与窗口生命周期完全解耦（关窗 = 隐藏、退出只经托盘）；同步 §9.4 |
| V0.6 | 2026-08-21 | 产品形态调整：启动时主窗口同时打开（RunEvent::Ready → show + focus），不再是"仅托盘出现"；关窗后仍由托盘唤回；同步 §2 / §12.1 验收剧本 |
| V0.7 | 2026-08-21 | 断连 UX 闭环：§12.5 全链路已实现（断连事件 + 前端 Reconnecting 视觉态 + 断连/重连 Toast + 5 次退避重连）；`device-connection-changed` payload 增加 `reason` / `reconnecting` |

# AI-Light 客户端技术架构（决策日志）

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.0 |
| 文档性质 | **技术方案决策日志**（非蓝图式设计文档）——记录"为什么这么设计" |
| 方法论 | ADR（Nygard）+ Y-Statements 摘要 + MADR 备选记录 + RFC 2119 表述 |
| 生效日期 | 2026-08-19 |
| 上游 | `docs/requirements/product-boundary.md`（六层边界）、ADR-0001/0002/0003、hook-api V1.0、theme-format V1.0 |

> **写给谁**：未来接手开发的工程师、以及未来的自己。本文不描述系统现状（那是代码的事），只回答"每个技术决策为什么这么做、考虑过什么、后果是什么"。
> **未定稿处**：标 ⚠️ 的决策/假设需实现期或实测验证后回填结论。

---

## 1. 目的与读者

- **目的**：把六层边界（L1~L6）落地为 Tauri 代码结构时，需要做的**实现级架构决策**，逐条记录背景、决定、备选、后果
- **读者**：前端开发者（React 侧）、后端/协议开发者（Rust 侧）、适配器开发者、主题作者、维护者
- **不包含**：接口细节（见 hook-api.md / theme-format.md）、协议细节（见硬件 V0.4）、已定业务决策（见 ADR-0001/0002/0003）

## 2. 架构图景（六层 → 代码结构）

```text
┌────────────────────────────────────────────────────────┐
│ AI-Light（Tauri v2 应用）                              │
│                                                        │
│  Frontend — React 19 + TS（L5 展示层）                  │
│  ├─ 配置窗口：主题编辑器 / 设备管理 / 状态展示 / 手动触发 │
│  └─ 托盘菜单（常驻，窗口可关，Q2 落地）                  │
│        ▲           ▲                                   │
│        │ events    │ invoke                             │
│        │（状态推送）│（配置操作）                        │
│  Rust Core                                             │
│  ├─ L1 hook_server   HTTP 47800（hook-api V1.0）       │
│  ├─ L2 arbiter       优先级仲裁（ADR-0001 Q8）          │
│  ├─ L2 theme         主题加载/校验/编译（theme-format） │
│  ├─ L3 protocol      V0.4 编解码（借鉴 pyPcTest 分层）  │
│  ├─ L3 transport     单 writer 队列 + 重试幂等          │
│  ├─ L4 ble           btpleug 设备管理/重连              │
│  ├─ config/logging/托盘/单实例                          │
│  └─ 唯一事实源：Rust 侧"当前业务状态 + 设备状态"        │
└────────────────────────────────────────────────────────┘
```

**模块对应表**（Rust `src-tauri/src/` 下）：

| 模块 | 层 | 职责（一句话） |
|---|---|---|
| `main.rs` / `app` | — | 应用入口、生命周期、托盘、单实例 |
| `hook_server` | L1 | HTTP 服务：/hook /api/status /api/health，token 校验 |
| `arbiter` | L2 | 状态仲裁：优先级抢占 → 当前业务状态（唯一事实源） |
| `theme` | L2 | 主题加载/整体校验/SCENE 编译（JSON → V0.4 字节） |
| `protocol` | L3 | 帧编解码、命令构造、结果码映射（纯函数，无 IO） |
| `transport` | L3 | 单 writer 发送队列、超时重试、幂等对账 |
| `ble` | L4 | btleplug 封装：扫描/连接/握手/断连重连 |
| `config` | — | config.json + themes/ 管理 |
| `logging` | — | tracing 初始化、滚动文件 |

---

## 3. 关键架构决策（KAD）

### KAD-01 BLE 访问层：btleplug

> **【摘要】** 在需要一套 Rust 代码覆盖 mac/win/linux 的 BLE 访问的背景下，我们决定采用 **btleplug**，以达成跨平台一致性与最小维护成本，接受其 Windows 服务缓存问题需自行规避（协议层已要求禁用缓存重新发现，pyPcTest/Bleak 有同款经验）。

- **背景**：设备为 GATT Server（BLE Peripheral）；客户端需扫描、连接、服务发现、Notify 订阅、Write。跨三平台。
- **决策**：使用 `btleplug`（MUST）。BLE 操作在独立异步执行器上运行，不得阻塞 Tauri 主线程与 command 线程（SHOULD：使用单独 tokio runtime 承载 btleplug 事件流）。
- **备选方案**：
  1. 各平台原生绑定（CoreBluetooth / WinRT / BlueZ 直写）——工作量 ×3，双端维护，否决
  2. Python sidecar（复用 pyPcTest 的 Bleak 栈）——双运行时、IPC 复杂度、分发体积，否决
- **后果**：跨平台一致性✅；Windows 服务缓存与偶发连接问题需专门处理⚠️；btleplug 事件循环与 Tauri async 的线程模型整合需 spike 验证⚠️。
- **验证**：三平台冒烟（扫描→连接→握手→SET_SCENE 全链路）。状态：⚠️ 待开发验证。

### KAD-02 本地 HTTP server：axum

> **【摘要】** 在 L1 需要极简本地 HTTP 服务（3 端点 + 可选 token）的背景下，我们决定采用 **axum**，以达成后续可扩展性与开箱的中间件支持，接受其依赖体积（tokio 已被 Tauri 2 依赖，边际成本低）。

- **背景**：hook-api V1.0 要求 `POST /hook` + `GET /api/status` + `GET /api/health`，JSON、可选 Bearer token、端口冲突退避（47801~47810）。
- **决策**：使用 `axum`（MUST），监听 `127.0.0.1`（MUST NOT 绑定非回环地址）。token 校验用中间件（SHOULD）。端口占用时自动退避（MUST，hook-api §1）。
- **备选方案**：
  1. `tiny_http`——极轻但路由/JSON/中间件全手写，扩展 direct_scene（V2）时成本高，否决
  2. `actix-web`——运行时与 Tauri 的 tokio 生态割裂，否决
  3. 原生 `std::net::TcpListener` 手写 HTTP——不可维护，否决
- **后果**：依赖 +编译时间少量增加（tokio 已存在）✅；axum server 需与 Tauri async runtime 共存（独立 runtime 或 tauri 的 tokio 复用）⚠️。
- **验证**：构建体积/编译时间对比、端口退避行为。状态：⚠️ 待实现验证。

### KAD-03 状态流架构：Rust 侧唯一事实源 + events 推前端

> **【摘要】** 在灯效下发、设备重连都在 Rust 侧、且托盘常驻场景前端可能长时间不打开的背景下，我们决定**业务状态与设备状态的唯一事实源放在 Rust 侧**，前端通过 Tauri events 订阅展示，以达成"灯效不依赖前端存活"，接受前端与 Rust 的状态同步需要明确事件契约。

- **背景**：仲裁（L2）产出当前业务状态；BLE 断连重连、场景下发在 Rust 侧；窗口可关（Q2），前端生命周期与核心服务解耦。
- **决策**：当前业务状态（`{state, source, session, since}`）与设备状态（连接/电量/固件）仅存于 Rust（MUST）；状态变化经 `app://state-changed` 等事件推送给前端（MUST）；前端只读展示 + invoke 执行配置类操作（MUST）。hook 事件、设备事件（POWER_CHANGED/FAULT_EVENT）统一走 events（SHOULD）。
- **备选方案**：
  1. 前端 zustand 主导状态、Rust 透传——双事实源，重连/仲裁逻辑无处安放，否决
  2. 状态全放前端——设备层与协议层被迫依赖前端存活，违背托盘常驻定位，否决
- **后果**：灯效链路（hook→仲裁→下发）不依赖 UI ✅；前端刷新/关闭不影响核心 ✅；需要维护一份 events 契约文档（状态：随实现补充）。
- **验证**：前端关闭时 hook 触发灯效正常（核心验收项）。状态：✅ 设计确定，实现期验证。

### KAD-04 配置与主题存储

> **【摘要】** 在配置量级为"几个键 + 若干主题文件"的背景下，我们决定用 **JSON 文件**（config.json + themes/ 目录）落在 Tauri app config dir，以达成零依赖与主题可分享，拒绝引入数据库。

- **背景**：设置项（token、仲裁模式、记住的设备、端口偏好）+ 主题资产（默认主题 + 用户导入主题）。
- **决策**：
  - 配置文件：`config.json` 于 app config dir（MUST）；结构小而平，非敏感（token 可选，存明文前提示风险或未来引入系统钥匙串——SHOULD 记录为改进项）
  - 主题：`themes/*.ailight-theme.json`（MUST）；**默认主题作为资源编译进二进制**（include_str），用户主题放 themes/（MUST）
  - 主题导入/导出通过 UI 完成（SHOULD），不引导用户手改文件
- **备选方案**：SQLite（tauri-plugin-sql）——配置量级过度设计，否决；注册表/plist——平台分裂，否决。
- **后果**：零依赖✅；主题分享 = 一个 JSON 文件✅；token 明文存储是已知风险⚠️。
- **验证**：三平台路径行为（mac `~/Library/Application Support/…`、win `%APPDATA%`、linux `~/.config`）。状态：✅ 设计确定。

### KAD-05 日志：tracing + tracing-appender

> **【摘要】** 在需要结构化日志与滚动文件、且协议要求 DEBUG 日志可编译关闭的背景下，我们决定采用 **tracing** 生态，以达成可观测性与性能双要求。

- **背景**：排障（hook 事件流、BLE 时序、协议收发）、滚动文件防膨胀；协议 §14.2 要求 DEBUG 日志可编译关闭。
- **决策**：`tracing` + `tracing-subscriber` + `tracing-appender`（MUST）；日志目录用 app log dir；级别默认 info、调试构建 debug（MUST）；协议层 DEBUG 日志由 feature flag 控制编译开关（MUST，对齐协议 §14.2 性能验收）。
- **备选方案**：`log` + `env_logger`——无结构化与滚动，否决；`fern`——滚动手写，否决。
- **后果**：结构化、可滚动✅；hook 事件全量记录（含 source/state）便于追溯✅。
- **验证**：release 构建下无协议 DEBUG 输出（协议性能验收项）。状态：✅ 设计确定。

### KAD-06 托盘常驻与窗口生命周期

> **【摘要】** 在产品形态为托盘常驻、窗口可关（Q2）的背景下，我们决定**关闭窗口 = 隐藏而非退出**，并启用单实例，以达成"灯效服务与 UI 生命周期解耦"，接受托盘图标跨平台行为差异需适配。

- **背景**：常驻服务（hook 监听、BLE 保持）不应随窗口关闭而终止。
- **决策**：
  - 托盘图标 + 菜单（显示状态 / 打开配置 / 退出）（MUST）
  - 窗口 `on_close_requested` → 隐藏（prevent_close + hide），托盘"退出"才终止进程（MUST）
  - `tauri-plugin-single-instance`（MUST，防多开导致端口/设备冲突）
  - 开机自启（tauri-plugin-autostart）作为设置项（SHOULD，第一期可暂缓）
- **备选方案**：窗口常驻不隐藏——违背 Q2，否决；无托盘——无法后台，否决。
- **后果**：服务与 UI 解耦✅；mac 菜单栏 / win 通知区 / linux 各 DE 的托盘行为需逐平台适配⚠️。
- **验证**：三平台托盘菜单与关闭行为冒烟。状态：✅ 设计确定。

### KAD-07 单 writer 发送队列

> **【摘要】** 在协议要求"同一时刻只允许一个完整协议事务在途"（V0.4 §15.6）的背景下，我们决定在 transport 层实现 **mpsc 队列 + 事务状态机**，以达成协议合规与无交错，接受队列背压与超时逻辑需仔细设计。

- **背景**：SET_SCENE 可跨多个 BLE 数据块；业务命令不得与分片写入交错；500ms 超时、最多重发 2 次、保持原序列号（协议 §3.5）；重复请求设备幂等（§3.5/§8.4）。
- **决策**：
  - 所有出站命令（握手、业务、查询）走同一 mpsc channel（MUST），由单一 writer 任务消费
  - 事务状态机：`Idle → Sending(seq) → Awaiting(seq) → Retry/Timeout → Done`（MUST）；同一时刻仅一个在途事务
  - 业务去重在前：主题编译后与当前有效 SCENE 比较（APPLY_IF_CHANGED），相同则不上队列（MUST，协议 §8.4 矩阵）
  - 断线重连后：重发当前业务 SCENE（APPLY_IF_CHANGED）+ GET_OUTPUT_STATUS 对账（MUST，协议 §15.5）
- **备选方案**：每个命令直接 await 写入——分片交错风险，否决；每命令独立锁——优先级反转/死锁风险，否决。
- **后果**：协议合规✅；背压（设备无应答时队列堆积）需限长策略⚠️（超时丢弃旧事务，保留最新业务意图）。
- **验证**：粘包/分包、重试幂等、断连对账（协议 §18.2 验收项）。状态：✅ 设计确定。

### KAD-08 Async 执行模型边界：core 不绑 runtime，setup 契约显式化

> **【摘要】** 在 `ailight-core` 仅依赖 `tokio` 而不引入 Tauri 的前提下，由调用方（Tauri 侧 setup 回调）显式保证 `tokio::spawn` 调用点位于 runtime 上下文内；core 在 `Transport::new` / `Engine::new` 内做 `debug_assert` 兜底，把"必须有 runtime"从注释约定提升为调试期可验证事实。

- **背景**：启动期 macOS 上 `there is no reactor running` panic 触发 abort（详见 ADR-0003）；根因是 Tauri `setup` 回调运行在 AppKit 主线程（不在 Tokio runtime），而 `core` 在构造期裸用 `tokio::spawn` 启动 writer 任务。
- **决策**：
  - `ailight-core` **不引入 Tauri 依赖**，runtime 上下文契约由调用方负责（MUST）
  - `src-tauri/src/lib.rs` 在 `Engine::new` 之前用 `tauri::async_runtime::handle().inner().enter()` 的 guard 进入 runtime 上下文（MUST，ADR-0003 D-02）
  - `core` 的 `Transport::new` / `Engine::new` 加 `debug_assert!(tokio::runtime::Handle::try_current().is_ok(), ...)` 并在文档注释中显式声明契约（MUST，ADR-0003 D-03）
  - `setup` 回调里**禁止**裸调 `tokio::spawn`，新增常驻任务一律走 `tauri::async_runtime::spawn`（MUST，ADR-0003 D-04）
- **备选方案**：core 内置自起 runtime——行为不可预测，否决；改 `Transport::new` 签名为返回 future pair——更彻底但牵动测试，留作 V1.1+ 候选（ADR-0003 备选 B）。
- **后果**：启动 panic 解决；core 保持分层独立；契约调试期可见；新增 setup 任务的"裸 spawn"风险被长期禁止规则覆盖✅。
- **验证**：跨平台启动冒烟（mac / win / linux 三套 setup 路径）。状态：✅ 设计确定。

---

## 4. 不确定性清单（open questions）

| # | 不确定项 | 关联 | 处置 |
|---|---|---|---|
| U-01 | btleplug 三平台实际行为（Win 缓存规避、Linux BlueZ 依赖） | KAD-01 | 开发期 spike + 三平台冒烟 |
| U-02 | axum 对构建体积/编译时间影响 | KAD-02 | 构建验证（可与模板基线对比） |
| U-03 | Claude Code HTTP hook 真实请求格式（变量占位/时序） | Q6 实测 | hook-api 示例回填 `docs/specs/adapters/` |
| U-04 | Codex Desktop notify 重写冲突规避 | Q6 实测 | 实测后定适配模板 |
| U-05 | 托盘图标三平台差异（mac 菜单栏/win 通知区/linux DE） | KAD-06 | 开发期逐平台适配 |
| U-06 | Tauri async command 与 btleplug 事件循环线程模型整合 | KAD-01/03 | ✅ 已落地：见 KAD-08 + ADR-0003（setup 侧已通过 `enter()` guard 解决；BLE 线程侧始终在 async fn 内部，原本安全） |
| U-07 | token 明文存储风险 | KAD-04 | 改进项：系统钥匙串（mac Keychain/win Credential Manager/linux secret-service），V2 |

## 5. 影响（对他人的影响）

| 受众 | 影响 |
|---|---|
| **前端开发者** | 状态流只读（events）+ 配置操作（invoke）；主题编辑器数据模型 = theme-format V1.0；不持有业务状态 |
| **适配器开发者** | 对接 hook-api V1.0（POST /hook），source 注册；🟢 配置模板在 `docs/specs/adapters/`（待实测回填） |
| **主题作者** | 格式 = theme-format V1.0（`.ailight-theme.json`），整体校验 + 默认主题兜底 |
| **维护者** | 决策追溯链：本文档 ← ADR-0001/0002/0003 ← product-boundary.md；新决策追加 KAD-N 或新 ADR，不改写历史 |

---

## 6. 决策索引

| 决策 | 状态 |
|---|---|
| KAD-01 BLE = btleplug | ⚠️ 待开发验证 |
| KAD-02 HTTP = axum | ⚠️ 待实现验证 |
| KAD-03 状态流 = Rust 唯一事实源 + events | ✅ 设计确定 |
| KAD-04 配置/主题 = JSON 文件（config.json + themes/） | ✅ 设计确定 |
| KAD-05 日志 = tracing 生态 + 协议 DEBUG 编译开关 | ✅ 设计确定 |
| KAD-06 托盘常驻 + 关窗隐藏 + 单实例 | ✅ 设计确定 |
| KAD-07 单 writer 队列 + 事务状态机 | ✅ 设计确定 |
| KAD-08 core 不绑 runtime + setup 契约显式化（ADR-0003） | ✅ 设计确定 |

*本文档随开发推进回填 ⚠️ 项；已定决策如需变更，追加新 KAD 说明变更原因，不改写旧记录。*

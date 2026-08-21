# AI-Light 客户端产品边界梳理

| 项目 | 内容 |
|---|---|
| 文档版本 | V0.1（沟通记录版） |
| 文档状态 | 演进中——随接入层调研与设计持续更新 |
| 关联仓库 | [LeonDevLifeLog/ai-light](https://github.com/LeonDevLifeLog/ai-light)（Tauri v2 客户端） |
| 硬件协议 | [AgentCore-Light 蓝牙通信协议 V0.4](../specs/协议V0.4引用.md)（独立仓库 `hx_agentcore_light_ble8208b_prj`） |
| 编制日期 | 2026-08-19 |
| 编制人 | 小艺（记录），李昻（决策） |

> 本文是产品需求的"边界地图"：把整个产品拆成六层，逐层标注 ✅已明确 / ⚠️待明确，并记录沟通过程中的决策。**后续所有需求文档（PRD、接入协议、技术方案、ADR）都以本文为索引。**

---

## 1. 产品定位

跨平台（macOS / Windows / Linux）桌面客户端（Tauri v2），连接 AgentCore-Light 蓝牙指示灯牌硬件，将**智能体客户端（AI 编程工具/桌面 AI）的运行状态**实时映射为灯效 + 提示音。

- 产品对标灵感：Ghostyu PromLight（https://light.buildfpga.com/light-v2/）
- 核心设计哲学：**机制与策略分离**（与 V0.4 协议一致）
  - 设备只执行物理输出（SCENE：3 条灯轨道 + 蜂鸣轨道），不理解业务语义；
  - "AI 状态 → 灯效/声音"的映射全部在客户端完成；
  - 换主题、改颜色、调提示音 = 改客户端配置，永不升级固件。

## 2. 六层边界总览

| 层 | 名称 | 职责 | 状态 | 关键依据 |
|---|---|---|---|---|
| L1 | **接入层** | 接收各智能体客户端 hook 事件 | ✅ 已明确（hook-api V1.0） | 正式规范：`docs/specs/hook-api.md`；第一期支持 Claude Code / Qoder / Codex |
| L2 | **业务层** | 状态仲裁 → 业务状态 → 主题映射 → SCENE 编译 | ✅ 已明确（除 direct_scene 预留） | 5 态 + 优先级仲裁（ADR-0001）；主题格式 V1.0（ADR-0002） |
| L3 | **协议层** | V0.4 编解码、单 writer 队列、超时重试、幂等 | ✅ 已明确 | 协议 V0.4 §15 客户端实现指南 |
| L4 | **设备层** | BLE 扫描/连接/握手/能力发现/断连重连 | ✅ 已实现（握手信息读取 / 断连重连已落地，2026-08-21）；实机验证待 U-01 | 协议 V0.4 §5 握手流程；PCDaemon ble_worker 验证过退避重连 |
| L5 | **展示层** | 配置窗口（主题编辑、设备管理、试听） | ✅ 主窗口已实装（5 页 + 主题编辑器）；托盘未实装（2026-08-21 对账） | 托盘常驻、窗口可关；两个边界明确后才做 UI |
| L6 | **工程层** | 三平台打包、日志、分发升级 | ✅ 基本明确 | CI/CD 模板已就绪；手动分发，在线升级 V2 |

**当前唯一真正的开放接口：L1（接入层）。** L3/L4 已被 V0.4 锁死；L2 依赖 L1 的输入模型；L5 最后设计；L6 决策已定。

---

## 3. 各层细节

### L1 接入层（✅ 已明确）

**目标**：提供标准 API 服务，各智能体客户端通过自身 hook 机制调用，形成可插拔的事件接入。

**正式规范**：`docs/specs/hook-api.md`（V1.0）——本地 HTTP `127.0.0.1:47800`，`POST /hook` + `GET /api/status` + `GET /api/health`；标准 5 态事件模型；幂等对账（applied）；可选 token。

**第一期支持清单**（🟢 配置级接入）：
- Claude Code（本机 CLI 2.1.31）——hooks 原生支持 HTTP handler
- Qoder——hooks 事件与 Claude Code 同构
- Codex（CLI 0.147.0 + Desktop）——hooks + notify（注意 Desktop 重写 notify 冲突，待实测）

**暂缓/不覆盖**：Cursor（🟡 桥接方案存档）、WorkBuddy（🔴 无公开 hook）、Claude Desktop 纯聊天（🔴 不覆盖，内置 Claude Code 可接）。

**待办**：本机实测三客户端（Q6，延后至 hook-api 定稿后）→ 适配器配置模板入 `docs/specs/adapters/`。

### L2 业务层（✅ 已明确）

- **状态仲裁**：优先级抢占（`ERROR > SUCCESS > WORKING > WAITING > IDLE`，同级最近活跃），可配置切换"最近活跃"。——✅ 已确定（ADR-0001 Q8）
- **状态模型**：标准 5 态（IDLE/WORKING/WAITING/SUCCESS/ERROR）+ 开放状态名，主题映射表驱动。——✅ 已确定（ADR-0001 Q1）
- **主题系统**：命名 SCENE 库 + 状态引用（`.ailight-theme.json`），UI 可编辑、可分享导入；终态 hold_ms 驻留。——✅ 已确定（ADR-0002，规范 `docs/specs/theme-format.md`）
- **SCENE 编译**：业务状态 → SCENE 名 → JSON → V0.4 OutputScene，幂等去重（APPLY_IF_CHANGED）。——✅ 已明确（协议 §8.4）
- **会话**：第一期单灯单会话，session 字段透传记录。——✅ 已确定（ADR-0001 Q9）

### L3 协议层（✅ 已明确）

- V0.4 帧格式、命令表、能力发现、事件处理（DEVICE_READY / POWER_CHANGED / BUTTON_EVENT / FAULT_EVENT）
- 客户端实现指南（协议 §15）：连接握手 → 业务链路 → 单 writer 发送队列 → 断线重连对账
- 只支持 V0.4，旧固件提示升级、不做协议回退

### L4 设备层（✅ 已明确）

- 自动扫描连接、退避重连（PCDaemon 已验证）
- 设备注册表 + 当前激活设备抽象（为多灯预留，行为按单灯：自动连接上次设备）
- V0.4 握手：读 DIS → 使能 TX CCC → DEVICE_READY → GET_DEVICE_INFO → GET_CAPABILITIES → （BAS 订阅按能力位）→ GET_POWER_STATUS → 业务就绪

### L5 展示层（⏸ 最后设计）

- 产品形态：托盘常驻为主，窗口可关闭，窗口只在配置/试听时出现（✅ 已确认 Q2）
- 内容：设备管理、主题编辑、状态展示、手动灯效试听/测试
- UI 设计排期：待 L1/L2 边界明确后启动

### L6 工程层（✅ 基本明确）

- Tauri v2 + React 19 + TS + Tailwind v4 + shadcn/ui（模板已就绪，CI/CD 三平台打包）
- 分发：手动安装包（✅ 已确认 Q6）；在线升级（Tauri updater，需签名）V2 再议
- 日志：本地日志文件，调试可追溯

---

## 4. 已确认决策（决策日志）

| # | 决策 | 结论 | 日期 | 对应 ADR |
|---|---|---|---|---|
| D-01 | 产品形态 | 托盘常驻，窗口可关，窗口只在配置时出现 | 08-19 | — |
| D-02 | MVP 范围 | 暂不划分 MVP，先梳理完整需求与边界 | 08-19 | — |
| D-03 | 主题与配置 | 状态→SCENE 映射表，本地 JSON + UI 编辑 | 08-19 | — |
| D-04 | 分发升级 | 手动分发；在线升级 V2 | 08-19 | — |
| D-05 | 边界优先级 | 先锁 L1 接入标准 + L3/L4 表达标准，UI 最后 | 08-19 | — |
| D-06 | 事件接入形态 | 由客户端提供标准 API 服务，各客户端 hook 调用，可插拔（方向确认，细节待调研） | 08-19 | — |
| D-07 | 设计方法 | 方案基于事实调研（各客户端生命周期/hook 机制），不拍脑袋 | 08-19 | — |
| D-08 | 标准状态集 | 5 态：IDLE/WORKING/WAITING/SUCCESS/ERROR；状态名开放，5 态为内置主题默认键 | 08-19 | ADR-0001 Q1 |
| D-09 | 终态驻留语义 | SUCCESS/ERROR 默认驻留到下一事件，主题可配 `hold_ms` 自动回落（状态驻留器，非瞬时确认） | 08-19 | ADR-0001 Q2 |
| D-10 | WorkBuddy | 第一期不接入（无公开 hook，办公工作台语义异类），source 字段预留 | 08-19 | ADR-0001 Q3 |
| D-11 | Claude Desktop | 纯聊天模式不覆盖；内置 Claude Code 复用 hooks 正常接入（产品边界声明） | 08-19 | ADR-0001 Q4 |
| D-12 | Cursor 接入 | **第一期暂不接入**（08-19 晚调整）；cursor-bridge 桥接方案存档为未来选项，接口原则不变 | 08-19 | ADR-0001 Q5 |
| D-13 | 实测验证 | **延后**至 hook API 正式文档之后、开发启动前；实测内容与产出不变 | 08-19 | ADR-0001 Q6 |
| D-14 | hook API | 基线锁定（HTTP POST `127.0.0.1:47800/hook` + GET `/api/status`，仅回环，可选 token）；**出正式设计文档 `docs/specs/hook-api.md`** | 08-19 | ADR-0001 Q7 |
| D-15 | L2 仲裁规则 | 默认优先级抢占：ERROR > SUCCESS > WORKING > WAITING > IDLE，同级最近活跃；可配置切换"最近活跃" | 08-19 | ADR-0001 Q8 |
| D-16 | 会话支持 | 第一期不做（单灯单会话）；session 字段保留透传，未来启用不改协议 | 08-19 | ADR-0001 Q9 |
| D-17 | 主题结构 | 命名 SCENE 库 + 状态引用（`.ailight-theme.json`） | 08-19 | ADR-0002 T-01 |
| D-18 | 切换过渡 | transition_ms 状态级配置 | 08-19 | ADR-0002 T-02 |
| D-19 | 状态级覆盖 | 第一期不做，保持"状态 = 引用 SCENE"简单模型 | 08-19 | ADR-0002 T-03 |
| D-20 | 蜂鸣轨道 | 可选，null = 静音 | 08-19 | ADR-0002 T-04 |
| D-21 | 自定义状态 | 随用随写；未映射状态 → IDLE + 记日志 | 08-19 | ADR-0002 T-05 |
| D-22 | 校验容错 | 整体校验，任一非法 → 拒绝生效，默认主题兜底 | 08-19 | ADR-0002 T-06 |

## 5. 待确认问题（截至 08-19 晚）

✅ 已确定：L1 hook API 规范（`docs/specs/hook-api.md` V1.0）、主题格式规范（`docs/specs/theme-format.md` V1.0）、L2 仲裁与会话（ADR-0001 Q8/Q9）、主题格式 6 项（ADR-0002）。

⏳ 仍待定：

1. **高级直控通道**（direct_scene）：建议预留枚举、V2 实现

---

## 6. 参考材料

- 蓝牙通信协议 V0.4：`hx_agentcore_light_ble8208b_prj/docs/design/蓝牙硬件能力接口协议设计说明书_V0.4.md`
- PCDaemon Phase 1（2026-08-14 实验，未入库）：TCP 47800 JSON 行 hook 协议 + AI 状态机 + 7800 /api/status + 自动重连 BLE worker —— 已清理，设计意图保留
- pyPcTest（V0.2 手动测试工具）：四层架构参考（main/app/ble_transport/protocol）
- 灵感对标：Ghostyu PromLight

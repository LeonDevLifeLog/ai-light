# AGENTS.md

AI-Light：智能体状态灯跨平台客户端（macOS / Windows / Linux，Tauri v2）。把 AI 编程工具（Claude Code / Qoder / Codex 等）的运行状态通过 BLE 映射为桌面灯效 + 提示音。硬件为 AgentCore-Light（协议 V0.4，见硬件仓库 `hx_agentcore_light_ble8208b_prj`）。

## 项目结构

- `crates/ailight-core/` — Rust 核心逻辑（协议编解码 / 主题引擎 / 状态仲裁 / HTTP 接入 / 单 writer 队列 / BLE），**无 Tauri 依赖**，独立可测
- `src-tauri/` — Tauri 应用壳（12 个 commands、events 桥接、托盘、单实例）
- 仓库根 — 前端（React 19 + TypeScript + Tailwind v4 + shadcn/ui + Vite）
- `docs/` — 设计文档体系：`specs/`（hook-api / theme-format / ipc-contract / architecture）、`decisions/`（ADR）、`research/`、`requirements/`、`specs/themes/`（6 套内置主题）

## 常用命令

Rust 工具链为 rustup 安装（`~/.cargo/bin`，可能不在 PATH，先 `export PATH="$HOME/.cargo/bin:$PATH"`）。

```bash
# core 测试 / 覆盖率
cd crates/ailight-core && cargo test
cd crates/ailight-core && cargo llvm-cov --summary-only

# src-tauri 编译检查
cd src-tauri && cargo check

# 前端 / 全量
pnpm dev              # 前端 dev server
pnpm tauri dev        # 完整调试（tauri + 前端）
pnpm check            # ultracite（Biome）格式检查 —— CI 会跑，必须通过
pnpm typecheck        # tsc --noEmit
```

## 架构约定（改代码前必读）

- **机制与策略分离**：设备只执行 SCENE（灯效/声音参数），不理解业务语义（`PROCESSING`/`ERROR` 等）。业务状态 → 灯效的映射全在客户端（主题文件），换主题永不升级固件。
- **六层边界** L1 接入层 ~ L6 工程层，见 `docs/requirements/product-boundary.md`。
- **标准状态 5 态**：`IDLE / WORKING / WAITING / SUCCESS / ERROR` + 开放自定义状态；仲裁 = 优先级抢占（ERROR > SUCCESS > WORKING > WAITING > IDLE，同级最近活跃）。详见 ADR-0001。
- **主题格式**：命名 SCENE 库 + 状态引用（`.ailight-theme.json`），见 `docs/specs/theme-format.md`。
- **决策记录**：架构决策追加到 `docs/decisions/ADR-*` 或 `docs/specs/architecture.md` 的 KAD-*，**只追加不改写历史**。

## 关键坑（易踩，勿"修复"）

1. **SET_SCENE 应答实际 3 字节 `[applied, digest_hi, digest_low]`**（协议 §8.5 文档写 4 字节含 result）——`parse_set_scene_response` 兼容两种布局，这是实测事实，不要改回单布局。
2. **Biome/ultracite JSON 格式**：JSON 对象必须展开成多行（单行紧凑会 fail `pnpm check`）。提交时 lint-staged 自动跑 `ultracite fix`。
3. **主题 include_str! 路径相对源文件**：`crates/ailight-core/src/theme.rs` 里是 `../../../docs/specs/themes/`（3 级上溯），不是相对 crate 根。
4. **`tokio::spawn` 调用点必须在 Tokio runtime 上下文**（ADR-0003 / KAD-08）：Tauri 的 `.setup()` 回调运行在 macOS 主线程的 AppKit `did_finish_launching` 里，**不在**任何 Tokio runtime 上下文中——在 setup 内裸调 `tokio::spawn` 会直接 `there is no reactor running` panic，并因跨 `extern "C"` FFI 边界不可 unwind 触发 abort。`ailight-core` 不依赖 Tauri，runtime 上下文由 Tauri 侧用 `tauri::async_runtime::handle().inner().enter()` 的 guard 显式提供。`core` 的 `Transport::new` / `Engine::new` 已加 `debug_assert` 兜底。setup 内一律用 `tauri::async_runtime::spawn`，**禁止**裸 `tokio::spawn`。
4. **btleplug 0.11 API**：`subscribe()` 返回 `Result<()>`，通知流用 `notifications()`（item 含 `.uuid`/`.value`）；`properties()` 是 async 方法；async fn in trait 不兼容 dyn 对象，必须 `#[async_trait]`。
5. **hook 服务**：端口 47800，占用自动退避至 47810；只监听 127.0.0.1，不做局域网暴露。
6. **内置主题编译进二进制**（`BUILTIN_THEMES`）；用户主题在 app config dir 的 `themes/`，内置同名不可覆盖。

## 测试要求

- 修改 `ailight-core` 必须执行 `cargo test --manifest-path crates/ailight-core/Cargo.toml`，且全部测试通过；不要记录易漂移的测试总数。
- 协议层改动：golden tests 在 `protocol.rs`（对照协议文档 §17 全部帧示例，逐字节断言）。
- `ble.rs` 的连接/发现路径依赖真硬件，无法单测——保持编译通过，实机验证（文档 U-01）。
- 覆盖率基线：核心逻辑行覆盖 ~87%（排除硬件层），见 `cargo llvm-cov`。
- **双文档内容驱动审计**（V1.3 替换原"季度审计 + 触发式审计"双条款）：作为 AI agent，**没有时间概念**，审计边界基于**内容信号**而非日历时间。触发条件 = 下列 5 个内容信号任一出现即立即执行审计，不留漂移窗口：
  1. **会话入口触发**：每个新会话加载上下文时，自动 Read `ui-interactions.md` 与 `ui-interaction-spec.md` 章节标题与关键术语；发现章节缺失、引用断裂（指向不存在的文件 / 章节）、命名漂移（如视觉态全集 8 态名称不一致）立即在对话中告警。
  2. **变更前触发**：本会话即将 Edit / Write `ui-interactions.md` 或 `ui-interaction-spec.md` 的任何章节前，先 Read 两份文档对应章节 + 上游 4 个文档（ipc-contract / theme-format / 蓝牙 V0.4 / architecture.md）的相关章节，标注本次修改可能影响 spec.md §3 / §4 / §5 / §6~§8 的哪些条目。
  3. **变更后触发（自动）**：本次会话的 Edit / Write 落地后，在用户关闭会话或切到下一任务前自动跑一次"语义对齐检查"——5 项硬检查：
     - spec.md §3 联动矩阵的 Source Event 名称是否仍在 ipc-contract.md §5 events 清单
     - spec.md §4.1 失败路径的错误码是否仍在 ipc-contract.md §4 AppError.code 清单
     - spec.md §4.2 蓝牙 result code 是否仍在蓝牙 V0.4 §3.6 清单
     - spec.md §6 / §7 / §8 引用的 theme 字段名是否仍在 theme-format.md 字段表
     - spec.md 引用的 ADR / KAD 编号是否仍在 `docs/decisions/ADR-*` 或 `docs/specs/architecture.md`
     发现漂移 → 当前会话立即修复，不留到下次。
  4. **用户触发**：用户输入"对齐" / "审计" / "audit" / "检查漂移" / "一致性" 等关键词立即执行。
  5. **漂移信号触发**：发现以下任一情况立即审计：
     - spec.md 引用的术语在原文档中已不存在（grep 不到）
     - 上游文档新增字段但 spec.md 未引用
     - spec.md 内部两个章节描述同一行为不一致
     - ui-interactions.md 与 ui-interaction-spec.md 对同一概念描述矛盾
  审计产出："对齐报告"追加到两份文档变更日志；严重漂移阻塞 PR。

## CI / Release 变更守则

### 当前验证边界

- `quality` 始终执行：`pnpm check`、`pnpm typecheck`、`pnpm build`。
- 纯文档改动不启动 Tauri build。
- Pull Request 和普通前端改动只执行 Linux Tauri build。
- `crates/**`、`src-tauri/**`、依赖文件或 workflow 改动进入主分支时执行 Linux / macOS / Windows 三平台 build。
- `workflow_dispatch` 始终执行三平台 build。
- Linux Tauri job 必须执行 `ailight-core` 全量测试。
- Release 始终构建正式发布矩阵，不得因 CI 已执行 `--no-bundle` 而省略发布目标。
- 版本号需同步 `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` 三处。

### 修改 workflow 前检查

1. 先列出事件 × 文件类型 × 构建平台矩阵，确认没有意外扩大或缩小触发范围。
2. 修改或新增根目录构建配置时，同步检查 `paths-filter`：
   - `cross_platform` 必须是 `app` 的子集。
   - workflow 自身、Rust / Tauri、依赖锁文件必须触发跨平台验证。
   - 手动触发不得被文件过滤器跳过。
   - PR 事件通过 GitHub API 读取变更文件，必须保留 `pull-requests: read` 权限。
   - 动态矩阵交给 `fromJSON` 的条件表达式必须整体加括号，确保结果是 JSON 字符串而非布尔值。
3. 不直接使用 workflow 顶层 `paths-ignore` 跳过 required workflow，避免分支保护检查长期 pending。
4. 优先复用现有 job 输出，避免为轻量变更检测新增按分钟计费的独立 job。
5. Linux 系统依赖必须使用 `awalsh128/cache-apt-pkgs-action`，不得退回手写 `apt-get`（azure 镜像不可达会导致超时）。
6. Rust 缓存必须覆盖 `crates/ailight-core` 与 `src-tauri`。
7. Release 的版本校验必须先于发布矩阵，四个发布目标不得静默缩减。

### 修改后验收

- `actionlint .github/workflows/ci.yml .github/workflows/release.yml`
- `git diff --check`
- `pnpm check`
- `pnpm typecheck`
- `pnpm build`
- `cargo test --manifest-path crates/ailight-core/Cargo.toml`
- 同步更新 `docs/ci-cd/continuous-integration.md` 或 Release 操作手册。
- 最终人工复核纯文档、Pull Request、主分支推送、手动触发、tag Release 五条路径。

## 文档导航

| 主题 | 文档 |
|---|---|
| 六层边界 / 决策日志 | `docs/requirements/product-boundary.md` |
| 接入层 HTTP 协议 | `docs/specs/hook-api.md` |
| 主题文件格式 | `docs/specs/theme-format.md` |
| 前端 ↔ Rust 契约 | `docs/specs/ipc-contract.md` |
| L5 展示层组件契约（V1.0）| `docs/specs/ui-interaction-spec.md` |
| 技术架构（KAD） | `docs/specs/architecture.md` |
| 设计决策 | `docs/decisions/ADR-0001/0002/0003` |
| 内置主题 | `docs/specs/themes/README.md` |

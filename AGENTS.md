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

- 修改 `ailight-core` 必须 `cargo test` 全绿（当前 64 tests）。
- 协议层改动：golden tests 在 `protocol.rs`（对照协议文档 §17 全部帧示例，逐字节断言）。
- `ble.rs` 的连接/发现路径依赖真硬件，无法单测——保持编译通过，实机验证（文档 U-01）。
- 覆盖率基线：核心逻辑行覆盖 ~87%（排除硬件层），见 `cargo llvm-cov`。

## CI

- GitHub Actions（`.github/workflows/ci.yml`）：quality job（`pnpm check` + `typecheck` + `build`）+ 三平台 tauri build。
- Linux 构建依赖用 `awalsh128/cache-apt-pkgs-action`（apt 缓存）并已切换官方源——**不要回退**为手写 apt-get（azure 镜像不可达会导致超时）。
- 版本号需同步 `package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` 三处。

## 文档导航

| 主题 | 文档 |
|---|---|
| 六层边界 / 决策日志 | `docs/requirements/product-boundary.md` |
| 接入层 HTTP 协议 | `docs/specs/hook-api.md` |
| 主题文件格式 | `docs/specs/theme-format.md` |
| 前端 ↔ Rust 契约 | `docs/specs/ipc-contract.md` |
| 技术架构（KAD） | `docs/specs/architecture.md` |
| 设计决策 | `docs/decisions/ADR-0001/0002/0003` |
| 内置主题 | `docs/specs/themes/README.md` |

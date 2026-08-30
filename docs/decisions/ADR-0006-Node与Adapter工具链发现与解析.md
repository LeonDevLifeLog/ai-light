# ADR-0006：Node.js 与 Adapter 可执行程序发现与工具链解析

- 状态：已接受
- 日期：2026-08-30
- 依据：`docs/design/NodeJS与Adapter可执行程序发现设计方案.md`（方案 A：保留 Node.js/npm 外部工具链，`@ai-light/adapter` 独立发布与升级）
- 关联：KAD-11（共享目录 / runtime.json）、ADR-0003（async 执行模型边界）

## 背景

桌面端此前通过未解析的命令名（`ailight-adapter` / `npm`）启动外部工具，隐含假设 GUI 进程继承了用户终端 `PATH`。该假设在 Windows 上（官方安装器、nvm-windows、Volta、fnm、Scoop、Chocolatey、`.cmd` shim、开始菜单启动环境）尤其不可靠，且现有错误模型无法回答"搜索过哪里、找到哪些候选、为何拒绝、如何恢复"。

## 决策

1. **ToolchainService 位于 Tauri 壳层**（`src-tauri/src/toolchain/`），不进入 `ailight-core`：OS 可执行文件、用户目录、注册表与子进程属于平台集成，核心仲裁/主题/BLE 不依赖 Node/npm。模块划分为 model / store / discovery / windows / unix / validate / runner。
2. **持久化**：独立文件 `~/.ailight/toolchain.json`（schema v1，`AILIGHT_HOME` 可覆盖），原子写入并限当前用户读写；`overrides` 是用户意图（失效不静默删除，只标记 `invalid_override`），`selected` 是可再生的缓存；路径不展开 `~`/环境变量，非绝对路径加载时忽略并告警。
3. **发现与验证分离**：发现只生成候选（override > 上次已选 > 激活版本管理器 > PATH/OS 查询 > 注册表/官方安装 > 其他版本目录与常见目录），验证以受控执行收口：`<node> --version`（3 秒超时、64 KiB 输出上限、Node ≥ 20 门槛）、`<node> <npm-cli.js> --version` / `prefix --global`、`<node> <adapter cli.js> version --json`（校验 `ok=true`、包名、semver 与 Hook 协议兼容范围）。
4. **不执行 Shell（设计方案 §17.1）**：全部子进程使用 executable + args 数组；npm 优先解析到 `npm-cli.js`（含 Windows `.cmd` shim 的纯文本反解、Volta 固定路径）；仅无法解析时退回平台 launcher（Windows `.cmd` 经 std 受控转义执行），不手工拼接 `cmd /C` 字符串。
5. **同族一致性（§17.2）**：npm/Adapter 从选定 Node 安装树推导（sibling → Volta 固定路径 → prefix → PATH）；跨安装族组合标记 `mixedInstallation`，除非用户显式覆盖否则不自动选择。用户 override 支持逐项设置（高级路径），但 override 验证失败不静默回退。
6. **安装版本（§17.3）**：不无条件安装 `latest`。先用 `npm view @ai-light/adapter versions --json` 在兼容范围 `>=0.1.0 <0.2.0`（Desktop Hook Protocol V1）内取最大版本精确安装；registry 查询失败退回同一兼容范围表达式（node-semver 空格语法），错误分阶段报告并附脱敏 stderr 摘要。
7. **升级回滚（§17.4）**：npm 全局安装不保证事务回滚，本实现不宣称"自动保留旧版本"；升级前记录当前版本，失败时报告恢复命令。
8. **诊断与脱敏（§17.5）**：`AppError` 增加可选 `details`（kind/path/source/reason 或 ToolchainStatus）；message 与诊断报告中的用户家目录统一脱敏为 `<HOME>`；stderr 摘要限 500 字符；不记录完整环境变量、token 与第三方配置正文。
9. **缓存失效**：采用内容信号（文件不存在、size/mtime 变化、override 变更、安装完成），不依赖固定时间间隔；只读状态查询可用进程内缓存，写操作一律强制复验；并发探测在服务内合并。
10. **`AILIGHT_ADAPTER_BIN`**：保留为开发/测试 override，直接执行并跳过解析器（统一映射进解析器候选而非双事实源）；生产 UI 的用户选择走 `select_executable`（后端原生文件选择器 + 立即验证）。
11. **Hook 注入**：沿用 Adapter 现行行为（写入绝对 Node 路径 + 绝对 `cli.js` 路径）；Node 版本切换导致路径失效属已知代价，由 Adapter `doctor` + 修复流程缓解（设计方案 §9.4，M3 交付）。

## 后果

- 新增 4 个 IPC commands（`get_toolchain_status` / `set_toolchain_overrides` / `reset_toolchain_overrides` / `select_executable`）与 §4 错误码扩展（`NODE_NOT_FOUND` / `NODE_INCOMPATIBLE` / `TOOLCHAIN_OVERRIDE_INVALID` / `TOOLCHAIN_AMBIGUOUS` / `TOOLCHAIN_PERMISSION_DENIED` / `EXECUTABLE_TIMEOUT` 等）。
- 接入三命令（detect/install/uninstall）统一走 ProcessRunner，`ensure_adapter_installed` 的裸 `npm` 调用移除。
- Windows 注册表读取引入 `winreg 0.55`（仅 `cfg(windows)`；该版本已在依赖树内，不新增解析结果）。
- 前端 Integrations 页增加运行环境摘要/恢复卡与安装确认态；Settings 页增加"外部运行环境"折叠区。

## 验证

- `src-tauri` 单元测试：schema 兼容（未知字段/坏版本/相对路径 override）、`.cmd` shim 解析、候选去重与版本目录上限、超时/截断/非零退出、家目录脱敏、兼容范围择版、override 失败不回退、解析器真实执行冒烟。
- `pnpm check` / `pnpm typecheck` / `pnpm build` / `cargo test`（core）全部通过。
- Windows 实机冒烟（注册表发现、`.cmd`、空格/中文路径）与三平台版本管理器回归按设计方案 §14.2/§14.3 在 CI 实机覆盖。

## 追加决策：核心执行不变量收敛（2026-08-30）

实现复核后追加以下 MUST，不改写上方历史记录：

1. `detect/install/uninstall/doctor` 等 Adapter 管理命令只能消费一次 ToolchainService 解析得到的 `ResolvedToolchain`，并通过 ProcessRunner 执行；`AILIGHT_ADAPTER_BIN` 仅作为开发候选参与同一解析链，禁止直接执行旁路。
2. 损坏、非法或高于当前 schema 的 `toolchain.json` 进入 `store_invalid` 只读保护态。自动探测可以提供诊断，但不得覆盖原文件或执行接入写操作；只有用户明确“恢复自动检测”才允许重建。
3. 所有 Node/npm/Adapter 子进程统一使用清理后的 ProcessRunner 环境；移除 `NODE_OPTIONS` 与 `NPM_CONFIG_*`，使用参数数组、超时与输出上限。达到输出上限后继续排空管道，只停止累积，避免 EPIPE 或子进程异常退出。
4. ToolchainStatus 每个状态都必须存在 UI 恢复动作；`adapter_incompatible` 进入“确认并升级”链，`store_invalid` 进入“恢复自动检测”链，不允许仅显示不可操作错误。

## 追加决策：高级用户主动升级（2026-08-30）

1. Settings「外部运行环境」为已就绪 Adapter 提供用户主动触发的版本检查；页面加载、后台任务与定时器不得访问 npm registry。
2. 查询结果只暴露当前版本与兼容范围内的最高已发布版本。升级必须由用户点击带精确目标版本的按钮触发，后端再次确认该版本已发布且兼容，禁止使用 `latest`。
3. npm 安装完成不等于流程成功：ToolchainService 必须重新解析 Node/npm/Adapter，并通过同一 ResolvedToolchain 执行 `doctor --json`，再向 UI 返回新状态。
4. V1 不提供自动检查或自动升级开关。Adapter 缺失/不兼容仍由 Integrations 的必需安装恢复链处理，避免把高阶维护入口混入普通连接流程。

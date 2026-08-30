# Node.js 与 Adapter 可执行程序发现设计方案

> 状态：已评审——决策记录见 `docs/decisions/ADR-0006-Node与Adapter工具链发现与解析.md`；M1（ToolchainService / 发现验证 / IPC / 现有命令统一走 runner）与 M2（Integrations 恢复卡 / Settings 手动选择 / 诊断详情）已落地，M3（Hook 路径修复、升级路径、三平台版本管理器实机覆盖）待交付  
> 范围：方案 A——保留 Node.js/npm 外部工具链，`@ai-light/adapter` 独立发布与升级  
> 目标平台：Windows / macOS / Linux  
> 关联规范：`adapter-cli.md` §4、§12、§16、§18；`ipc-contract.md` §4；KAD-11

## 1. 背景与问题定义

AI-Light Desktop 当前通过未解析的命令名启动 `ailight-adapter` 和 `npm`：

```text
Command::new("ailight-adapter")
Command::new("npm")
```

这隐含假设桌面 GUI 进程继承了用户终端中的 `PATH`。该假设在 Windows 上尤其不可靠：Node.js 可能由官方安装器、nvm-windows、Volta、fnm、Scoop 或 Chocolatey 安装；用户环境变量可能在 AI-Light 启动后才更新；npm 和全局包入口通常是 `.cmd` shim；从开始菜单启动的 GUI 与 PowerShell、Git Bash 的运行环境也可能不同。

因此，“终端中 `node --version` 可用”不等于“AI-Light 进程能启动 node/npm/adapter”。当前错误模型只能报告 `NPM_NOT_FOUND` 或 `ADAPTER_NOT_FOUND`，不能回答：搜索过哪里、找到了哪些候选、为什么拒绝、用户如何恢复。

本设计解决的是完整工具链解析问题，而不只是增加一个 npm 路径文本框。

## 2. 目标与非目标

### 2.1 目标

1. 默认零配置发现 Node.js 20+、与该 Node 安装关联的 npm，以及全局安装的 `@ai-light/adapter`。
2. 自动发现失败或选错版本时，允许用户显式选择 Node/npm/Adapter 路径。
3. 每个候选都通过实际进程执行验证，不以“文件存在”作为可用结论。
4. 所有安装、升级、诊断和 Hook 注入使用同一份已解析工具链。
5. Hook 中写入不依赖 `PATH` 的稳定绝对命令，支持空格和非 ASCII 路径。
6. 给出可解释、可复制且默认脱敏的诊断结果。
7. 保持 Adapter 由 npm 全局目录管理，可独立发布和升级，不复制到 AI-Light 私有 bin 目录。

### 2.2 非目标

- 不捆绑 Node.js，不由 AI-Light 静默安装 Node.js。
- 不把 Adapter 内置进 Desktop。
- 不修改系统或用户 `PATH`。
- 不自动执行 shell profile、PowerShell profile 或任意用户脚本。
- 不支持 Node.js 20 以下版本；当前 Adapter 的 `engines.node` 为 `>=20`。
- 不在本设计中改变第三方工具 Hook 的业务状态映射。

## 3. 设计原则

1. **显式配置优先，自动发现兜底**：用户选择必须可预测，同时每次使用前仍验证安全性和可执行性。
2. **发现与验证分离**：发现阶段只生成候选；验证阶段以受控参数执行候选并结构化记录结果。
3. **Node 安装族一致性**：尽量从同一个 Node 安装推导 npm 与 Adapter，避免 Node A 配合 npm B。
4. **不执行 Shell**：Rust 直接创建子进程并传入参数，禁止拼接 `cmd /C`、PowerShell 或 `sh -c` 命令字符串。
5. **绝对路径落盘**：运行和 Hook 注入都使用规范化的绝对路径。
6. **失败可恢复**：每个失败状态必须包含原因、下一步动作和可查看的诊断详情。
7. **配置是意图，探测结果是缓存**：用户 override 是持久意图；版本、来源和验证时间只是可重新生成的结果。

## 4. 总体架构

```text
React Integrations / Settings
          │ Tauri IPC
          ▼
ToolchainService（src-tauri）
  ├─ CandidateDiscovery     生成 Node/npm/Adapter 候选
  ├─ CandidateValidator     受控执行、版本与关联性验证
  ├─ ToolchainResolver      排序并选择同族工具链
  ├─ ToolchainStore         toolchain.json 原子读写
  └─ ProcessRunner          所有安装/升级/Adapter 管理命令
          │
          ├─ node --version
          ├─ npm --version / npm prefix --global
          └─ <node.exe> <adapter-cli.js> ... --json
```

`ToolchainService` 位于 Tauri 壳层而不是 `ailight-core`：它处理 OS 可执行文件、用户目录、注册表和进程启动，属于平台集成；核心状态仲裁、主题和 BLE 不应依赖 Node/npm。

所有现有 `adapter_command` 与 `ensure_adapter_installed` 最终改为调用 `ProcessRunner`，禁止绕过解析器再次使用裸命令名。

## 5. 数据模型与持久化

### 5.1 文件位置

使用共享目录中的独立文件：

```text
~/.ailight/toolchain.json
```

不放入通用 `config.json`，原因是工具链包含平台路径、探测来源和诊断缓存，不属于主题、显示或设备偏好。文件按共享目录规范原子写入，并尽可能设置为仅当前用户可读写。

### 5.2 Schema

```json
{
  "version": 1,
  "mode": "auto",
  "overrides": {
    "node": null,
    "npm": null,
    "adapter": null
  },
  "selected": {
    "node": {
      "path": "C:\\Program Files\\nodejs\\node.exe",
      "version": "22.14.0",
      "source": "windowsRegistry"
    },
    "npm": {
      "path": "C:\\Program Files\\nodejs\\npm.cmd",
      "version": "10.9.2",
      "source": "siblingOfNode"
    },
    "adapter": {
      "launcherPath": "C:\\Users\\alice\\AppData\\Roaming\\npm\\ailight-adapter.cmd",
      "scriptPath": "C:\\Users\\alice\\AppData\\Roaming\\npm\\node_modules\\@ai-light\\adapter\\dist\\cli.js",
      "version": "0.4.2",
      "source": "npmGlobalPrefix"
    }
  },
  "lastResolvedAt": "2026-08-30T10:00:00Z"
}
```

规则：

- `mode`：`auto` 或 `manual`。manual 允许只覆盖部分字段，其余字段继续自动推导。
- `overrides` 是用户意图；路径失效时不得静默删除，只标记失败并建议恢复自动检测或重新选择。
- `selected` 是缓存，不是可信输入；每次关键写操作前重新验证。
- 前端不直接编辑 JSON，只通过 IPC 修改。
- 配置读取时忽略未知字段；schema 不支持时保留原文件并返回可恢复错误。
- 路径不做环境变量或 `~` 展开；持久化前必须转成绝对路径。

## 6. 自动发现机制

### 6.1 触发时机

- 首次打开“接入外部工具”页。
- 用户点击“重新检测”。
- 点击“连接”而当前工具链没有通过验证。
- 已保存路径不存在、不可启动或版本变化。
- Adapter 安装/升级完成后。
- 系统恢复或应用启动时不阻塞主界面；只在接入功能需要时惰性探测。

同一进程内合并并发探测请求，避免页面加载和按钮操作同时扫描。普通刷新可使用有效缓存；连接、安装、升级属于写操作，必须强制复验。

### 6.2 候选来源与顺序

候选先去重再验证。去重采用 Windows 大小写不敏感、其他平台大小写敏感的规范化绝对路径。

#### 所有平台

1. 用户 override。
2. 上次成功的 selected 路径。
3. AI-Light 当前进程 `PATH` 中的命令。
4. OS 原生路径查询结果：Windows `where.exe`；macOS/Linux 在已知环境中直接搜索 PATH，不执行用户 shell。
5. 从每个有效 Node 候选的同级目录查找 npm。
6. 用有效 npm 查询全局 prefix，并从 prefix 推导 Adapter。
7. 平台常见目录和已知版本管理器目录。

#### Windows

Node 候选至少覆盖：

- 注册表 `HKLM/HKCU` 中 Node.js 安装信息，兼容 32/64 位视图。
- `%ProgramFiles%\\nodejs\\node.exe`、`%ProgramFiles(x86)%\\nodejs\\node.exe`。
- `%LOCALAPPDATA%\\Programs\\nodejs\\node.exe`。
- nvm-windows：`%NVM_SYMLINK%`、`%NVM_HOME%` 及其版本目录；不假设 symlink 当前有效。
- Volta：`%VOLTA_HOME%\\bin\\node.exe`，无变量时补充 `%USERPROFILE%\\.volta\\bin`。
- fnm：环境变量暴露的 multshell 路径及 `%APPDATA%\\fnm` 下版本目录。
- Scoop：`%USERPROFILE%\\scoop\\apps\\nodejs*\\current\\node.exe`。
- Chocolatey：仅检查其 shim 和已知 Node 安装目标，不递归扫描整个磁盘。

npm 候选允许 `npm.cmd`、`npm.exe`；`npm.ps1` 不作为 Desktop 首选，因为直接执行策略和 PowerShell execution policy 不稳定。Adapter launcher 允许 `ailight-adapter.cmd`，但执行管理命令时优先使用下面的稳定脚本入口。

#### macOS / Linux

- 系统路径：`/usr/local/bin`、`/usr/bin`、Apple Silicon Homebrew `/opt/homebrew/bin`。
- nvm：`$NVM_DIR/versions/node/*/bin`，默认补充 `~/.nvm/versions/node/*/bin`。
- fnm：`$FNM_DIR` 与其 node-versions 目录。
- Volta：`$VOLTA_HOME/bin`，默认补充 `~/.volta/bin`。
- asdf/mise：只读取其稳定 shim/installs 目录，不启动 shell。
- Homebrew/Linuxbrew/Snap 的已知位置。

禁止递归扫描用户整个主目录。所有版本目录扫描设数量上限，并只读取符合版本目录格式的直接子目录。

### 6.3 Node 验证

对每个 Node 候选执行：

```text
<node-path> --version
```

约束：

- 超时 3 秒；stdout/stderr 各限制 64 KiB。
- 退出码必须为 0，输出必须匹配 `v<semver>`。
- major 必须 `>=20`。
- 记录 canonical path、版本、文件元数据和来源。
- 不从候选目录加载 DLL、脚本或配置来“验证”。

### 6.4 npm 验证与 Node 关联

对每个 npm 候选执行 `--version`，再执行等价于：

```text
npm prefix --global
```

Windows `.cmd` 不能作为 `Command::new` 在所有上下文中的可移植假设。实现应解析官方 npm shim 或定位 npm 自带的 `npm-cli.js`，最终优先用选定 Node 运行：

```text
<selected-node> <npm-cli.js> --version
<selected-node> <npm-cli.js> prefix --global
```

只有无法定位 `npm-cli.js` 时才使用平台 launcher，并通过专门的 Windows runner 处理 `.cmd`。runner 接收参数数组，不接受拼接后的 shell 字符串。

Node/npm 同族判定优先依据 npm CLI 所在 Node 安装树，其次通过 npm 输出与 prefix 关联。跨安装族组合降权，并在诊断中标记 `mixedInstallation=true`；除非用户显式覆盖，否则不自动选择混合工具链。

### 6.5 Adapter 发现与稳定执行入口

发现顺序：

1. 用户显式 Adapter 路径。
2. 上次验证成功的 `scriptPath`。
3. 选定 npm 的 global prefix：
   - Windows：`<prefix>\\node_modules\\@ai-light\\adapter\\dist\\cli.js`
   - macOS/Linux：`<prefix>/lib/node_modules/@ai-light/adapter/dist/cli.js`
4. global prefix 中的 `ailight-adapter(.cmd)` launcher，反查实际脚本。
5. 当前 PATH 中的 launcher，仅作为补充候选。

最终验证和执行统一使用：

```text
<selected-node-absolute-path> <adapter-script-absolute-path> version --json
```

这比把 `.cmd` shim 写入第三方 Hook 更稳定：避免目标工具的 PATH、PATHEXT、shell 类型和 PowerShell policy 差异。注入 Hook 时也应写入绝对 Node 路径 + 绝对 `cli.js` 路径，并由 Adapter 针对第三方配置格式完成正确转义。

验证要求：退出码 0、JSON `ok=true`、包名为 `@ai-light/adapter`、版本是合法 semver，并与 Desktop 支持的 Hook Protocol 有交集。

### 6.6 排序与选择

候选评分只用于自动模式，建议顺序：

1. 用户 override（验证通过即选中）。
2. 上次已选且仍有效的完整工具链，避免每次启动跳版本。
3. 当前系统激活的版本管理器入口。
4. 注册表/官方安装器当前安装。
5. 常见目录中的其他版本。

同档次优先：完整同族 Node + npm + Adapter、Node 版本满足要求、Adapter 协议兼容。不要单纯选择最大 Node 版本，否则可能绕过用户通过 nvm-windows 激活的版本。

解析结果状态：

```text
checking
ready
node_missing
node_incompatible
npm_missing
adapter_missing
adapter_incompatible
invalid_override
ambiguous
permission_denied
```

`adapter_missing` 仍是可自动恢复状态：只要 Node/npm ready，就可一键安装。

## 7. IPC 契约建议

新增四个 Tauri commands：

```text
get_toolchain_status(force?: boolean) -> ToolchainStatus
set_toolchain_overrides(patch)         -> ToolchainStatus
reset_toolchain_overrides()            -> ToolchainStatus
select_executable(kind)                -> ToolchainStatus
```

其中 `select_executable` 由后端打开原生文件选择器，前端不能传任意未确认路径冒充选择结果。也可保留 `set_toolchain_overrides` 支持“粘贴路径”，但必须返回字段级验证错误。

响应示例：

```json
{
  "state": "ready",
  "mode": "auto",
  "summary": "Node.js 22.14.0 · npm 10.9.2 · Adapter 0.4.2",
  "node": {
    "state": "ready",
    "path": "C:\\Program Files\\nodejs\\node.exe",
    "version": "22.14.0",
    "source": "windowsRegistry",
    "overridden": false
  },
  "npm": {
    "state": "ready",
    "path": "C:\\Program Files\\nodejs\\node_modules\\npm\\bin\\npm-cli.js",
    "version": "10.9.2",
    "source": "siblingOfNode",
    "overridden": false
  },
  "adapter": {
    "state": "ready",
    "path": "C:\\Users\\alice\\AppData\\Roaming\\npm\\node_modules\\@ai-light\\adapter\\dist\\cli.js",
    "version": "0.4.2",
    "source": "npmGlobalPrefix",
    "overridden": false
  },
  "issues": [],
  "checkedAt": "2026-08-30T10:00:00Z"
}
```

现有 commands 调整：

- `get_integration_status`：先解析工具链；Adapter 缺失时返回结构化未连接状态，而不是把页面刷新吞成空对象。
- `install_integration`：强制复验；Adapter 缺失时用已选 Node + npm CLI 安装；安装后重新解析并调用 Adapter。
- `uninstall_integration`：使用记录在 Hook 中或已解析的 Adapter；工具链损坏时返回 `needs_repair`，不得误删其他 Hook。

新增或细化错误码：

| code | 含义 | 恢复动作 |
|---|---|---|
| `NODE_NOT_FOUND` | 未发现 Node | 安装 Node 20+ 或手动选择 |
| `NODE_INCOMPATIBLE` | Node 版本低于 20 | 切换/选择兼容版本 |
| `NPM_NOT_FOUND` | 已发现 Node，但无关联 npm | 选择 npm 或修复 Node 安装 |
| `TOOLCHAIN_OVERRIDE_INVALID` | 手动路径不存在或验证失败 | 重新选择或恢复自动检测 |
| `TOOLCHAIN_AMBIGUOUS` | 多组候选无法安全决策 | 用户选择一组 Node |
| `TOOLCHAIN_PERMISSION_DENIED` | 文件或子进程权限不足 | 调整权限/安装范围 |
| `ADAPTER_NOT_FOUND` | npm 可用但 Adapter 未安装 | 一键安装 |
| `ADAPTER_INCOMPATIBLE` | Adapter 协议/版本不兼容 | 一键升级或选择其他版本 |
| `ADAPTER_INSTALL_FAILED` | npm 安装失败 | 展示 stderr 摘要与降级命令 |
| `EXECUTABLE_TIMEOUT` | 候选验证超时 | 选择其他路径/查看诊断 |

错误 `message` 面向用户，附加 `details` 保存 `kind/path/source/reason` 等结构化字段；不得把完整环境变量或 token 返回前端。

## 8. 用户体验设计

### 8.1 信息架构

普通用户的主路径仍留在“接入外部工具”页，不要求先去设置：

```text
运行环境  [可用]
Node.js 22.14.0 · npm 10.9.2 · Adapter 0.4.2
[查看详情] [重新检测]

Claude Code  [连接]
Codex        [连接]
```

自动检测成功时只显示一行摘要。存在问题时展开恢复卡：

```text
未找到可用的 Node.js
AI-Light 搜索了系统 PATH、Node.js 安装信息和常见版本管理器目录。
[选择 node.exe]  [重新检测]
需要 Node.js 20 或更高版本。
```

“设置 → 系统 → 外部运行环境”提供持久配置入口，但不是连接前置步骤。高级内容默认折叠，展示 Node/npm/Adapter 的路径、版本、来源以及“恢复自动检测”。

### 8.2 关键交互

1. 用户点击连接。
2. 页面内联显示“正在检查运行环境”，按钮 disabled，避免重复操作。
3. 工具链 ready：继续 Adapter `doctor → dry-run → install`。
4. Adapter missing：明确显示即将通过 npm 安装 `@ai-light/adapter`，用户确认后执行。
5. Node/npm missing：停止连接，原地给出选择路径或重新检测，不只发短暂 Toast。
6. 安装权限失败：保留错误摘要、复制降级命令与“重试”；不建议默认以管理员运行整个 AI-Light。
7. 手动选择后立即验证；失败信息显示在对应字段下，并将焦点移动到错误字段。

路径显示使用等宽字体，允许换行并提供复制按钮；不得只有 tooltip。状态同时使用图标、文字和颜色。所有操作可键盘完成，文件选择取消不改变现有配置，异步结果通过 `aria-live` 宣告。

### 8.3 诊断面板

“查看诊断详情”包括：

- AI-Light 版本、OS/架构。
- Node/npm/Adapter 的选中路径、版本、来源、override 状态。
- 每个发现器是否运行、候选数量和拒绝原因。
- npm global prefix 与工具链是否混用。
- 最近一次命令阶段和退出码；stderr 只保留长度受限、脱敏后的摘要。

提供“复制诊断报告”，默认隐藏用户名、Home 路径前缀、环境变量值、runtime token。报告不包含第三方配置正文。

## 9. 安装、升级与 Hook 稳定性

### 9.1 安装

安装命令不再调用裸 `npm`：

```text
<node.exe> <npm-cli.js> install --global @ai-light/adapter@<resolved-version>
```

默认指定明确版本，安装后重新发现 Adapter 并运行 `version --json` 与 `doctor --json`。registry 查询和安装要分阶段报告；网络失败、权限失败、包不存在不能都映射成同一错误。

### 9.2 权限策略

- 不自动提权，不静默使用管理员权限。
- npm global prefix 不可写时，优先建议用户采用用户级 Node 版本管理器或配置用户级 prefix。
- 可展示可复制命令，但命令必须来自结构化参数的安全渲染，且按当前 shell 提供明确版本；AI-Light 本身不执行复制出的 shell 文本。
- 不由 Desktop 自动修改 npm prefix，这会影响用户所有全局包。

### 9.3 升级

升级沿用已选 npm 安装族，使用明确目标版本。升级前记录当前版本；升级后验证失败时报告恢复命令，但 npm 全局安装本身不保证事务回滚，因此规范中的“保留当前可用版本”需要在实现前进一步验证 npm 行为，不能作为无条件承诺。

### 9.4 Hook 路径失效

nvm/fnm 切换或卸载 Node 后，已注入的绝对路径可能失效。这是绝对路径换取 GUI 稳定性的代价。处理策略：

- 每次打开接入页执行轻量 `doctor`。
- 发现 Hook 记录路径与当前 selected 不一致时状态为 `needs_repair`。
- 用户点击“修复”后由 Adapter 幂等替换仅属于 AI-Light 的托管条目。
- 不在 Node 版本切换时后台监听或静默改第三方配置。

## 10. 安全边界

1. 所有子进程调用使用 executable + args 数组；禁止把用户路径拼进 shell 字符串。
2. 自动发现只执行明确名称、明确位置的候选；不执行递归扫描发现的任意同名文件。
3. 手动选择的文件必须为普通文件或可解析的受支持 shim，路径 canonicalize 后再保存。
4. 验证进程使用超时、输出上限、清理后的环境；至少覆盖 `PATH`、`NODE_OPTIONS`、`NPM_CONFIG_*` 的威胁评估。尤其应移除不受信的 `NODE_OPTIONS`，避免验证/运行 Adapter 时注入模块。
5. 不记录完整环境、access token、registry 凭据或第三方配置正文。
6. npm 安装属于供应链和网络操作，必须显示目标包与版本；连接按钮不得在用户无感知时安装任意最新版本。
7. Adapter script 必须位于已验证 npm prefix 的预期包目录，package name 与版本必须由其 `package.json`/CLI 响应交叉验证。
8. 软链接和 junction 解析后验证最终目标；路径变化时使缓存失效。

## 11. 缓存与失效策略

不依赖固定时间间隔作为正确性边界，使用内容信号失效：

- executable/script 不存在。
- 文件 identity、mtime 或 size 改变。
- `--version` 输出变化。
- override 变更。
- npm 安装/升级完成。
- 子进程返回“找不到文件”“bad interpreter”或协议不兼容。
- 当前 Hook 中记录路径与 selected 不一致。

只读状态查询可使用进程内缓存；任何会写第三方配置、安装包或升级的动作都强制验证。

## 12. 实现分层与建议改动点

### 12.1 Rust/Tauri

建议新增：

```text
src-tauri/src/toolchain/
├── mod.rs          ToolchainService
├── model.rs        schema / status / issue
├── discovery.rs    跨平台公共发现
├── windows.rs      注册表、PATHEXT、.cmd、版本管理器
├── unix.rs         macOS/Linux 已知目录
├── validate.rs     超时、版本、JSON 验证
├── runner.rs       node/npm/adapter 受控执行
└── store.rs        toolchain.json 原子持久化
```

`commands.rs` 只负责参数校验和错误映射。阻塞文件/进程工作必须放入 `tauri::async_runtime::spawn_blocking`；不得在 `.setup()` 中裸 `tokio::spawn`。

Windows 注册表读取建议使用最小、成熟的 Rust crate 或 Windows API；引入依赖前记录理由。不要通过 `reg query` 的本地化文本解析核心安装路径。

### 12.2 前端

- `src/lib/ailight.ts` 增加 `ToolchainStatus`、`ToolStatus`、`ToolchainIssue` 类型和四个 API。
- Integrations 页增加运行环境摘要/恢复卡，刷新失败不得再吞掉错误伪装成“未连接”。
- Settings 页增加“外部运行环境”折叠区域。
- 复用现有 `Card`、`StatusTag`、`ActionButton` 和 Toast；持久错误使用页面内 Alert，而非仅 Toast。

### 12.3 Adapter

- Adapter 的 `install/repair` 输出中回报实际写入的 `nodePath` 与 `scriptPath`。
- `doctor --json` 检查 Hook 中绝对路径是否存在、是否与当前调用实例一致。
- Windows fixture 覆盖反斜杠、空格、非 ASCII 和 `.cmd` shim。
- Adapter 不自行成为另一套 Node/npm 自动发现器；Desktop 负责工具链解析，CLI 只诊断自身及托管 Hook。

## 13. 迁移与兼容

1. 首次升级没有 `toolchain.json`：运行自动发现并落盘 selected 缓存。
2. 继续支持 `AILIGHT_ADAPTER_BIN` 作为开发/测试 override，但生产 UI 中的用户选择优先级和语义必须明确；长期建议统一映射进 Toolchain override，避免双事实源。
3. 已存在 Hook：Adapter `doctor` 读取并验证；可用则不改，不可用或与 selected 不一致则显示 `needs_repair`。
4. 已安装 Adapter 但不在 PATH：从 npm global prefix 发现并直接使用，无需重装。
5. 自动发现失败不影响 BLE、主题、手动状态测试等非接入功能。

建议分三步交付：

- M1：ToolchainService、Windows 发现/验证、IPC、现有命令统一走 runner。
- M2：Integrations 恢复卡、Settings 手动选择、诊断报告。
- M3：Hook 路径修复、升级路径、三平台版本管理器覆盖。

每一步都必须保持旧功能可用；M1 完成前不应先加只有外观、不能真正改变执行路径的配置框。

## 14. 测试与验收矩阵

### 14.1 单元测试

- 候选去重、来源排序、同族选择、semver 与 Node 20 门槛。
- Windows 大小写、空格、非 ASCII、UNC/长路径（按支持边界明确结论）。
- `.cmd` shim 解析与参数不发生 shell 注入。
- toolchain schema 缺字段、未知字段、非法 override、原子写入失败。
- 超时、非零退出、超大输出、无效 UTF-8/JSON。
- 路径脱敏与诊断报告不泄露 token/环境值。

### 14.2 Windows 场景

| 场景 | 预期 |
|---|---|
| 官方 Node 安装器，GUI 启动时 PATH 缺失 | 注册表/常见目录发现，连接成功 |
| 安装 Node 后未重启 AI-Light | 重新检测成功，不要求重启应用 |
| nvm-windows 当前版本 | 选择当前 symlink 对应完整工具链 |
| nvm-windows 多版本 | 不盲选最高版本；显示来源，可手动切换 |
| Volta / fnm / Scoop / Chocolatey | 对应发现器找到并验证 |
| Adapter 已安装但不在 PATH | 通过 npm prefix 找到，不重复安装 |
| `C:\\Program Files` 路径 | Node + cli.js 参数传递正确 |
| 用户名/路径含中文 | 发现、保存、执行、Hook 均成功 |
| npm global prefix 不可写 | 不提权；显示权限恢复指引 |
| Node 18 + Node 22 并存 | 自动选择有效激活族或要求选择，不用 Node 18 |
| PATH 中恶意同名程序 | 可信来源排序、包身份验证和诊断可见 |

### 14.3 macOS/Linux 回归

- 系统 Node、Homebrew、nvm/fnm/Volta/asdf/mise 至少各覆盖约定范围。
- GUI PATH 缺失仍可发现。
- Hook 中含空格路径正确。
- symlink 目标切换触发失效和 `needs_repair`。

### 14.4 端到端验收

1. 全新环境：检测 Node/npm → 安装 Adapter → dry-run → 用户确认 → 注入 Hook。
2. Adapter 已安装但 PATH 不可见：直接连接，不重新安装。
3. 自动检测失败：手动选择 Node → 自动推导 npm/Adapter → 连接。
4. 切换 Node 版本导致路径失效：页面报告并一键修复托管 Hook。
5. 真实 Claude Code/Codex Hook → Adapter → Hook Server → 灯效闭环。
6. 非接入功能在工具链完全缺失时仍正常。

工程验收至少执行项目规定的 `pnpm check`、`pnpm typecheck`、`pnpm build`、Tauri 编译检查和相关 Rust/Adapter 测试；Windows 必须实机冒烟，不能只依赖 Unix CI。

## 15. 可观测性

内部阶段使用稳定标识，便于日志和诊断聚合：

```text
discover.node.registry
discover.node.path
validate.node.version
discover.npm.sibling
validate.npm.prefix
discover.adapter.global_prefix
validate.adapter.protocol
install.adapter
repair.integration
```

日志记录阶段、耗时、候选来源、结果分类，不记录命令环境、token 和完整用户路径。用户路径显示或上传前脱敏为 `<HOME>`；本机 debug 日志是否保留完整路径应由日志级别和隐私策略明确规定。

## 16. 风险与权衡

| 决策 | 收益 | 代价/缓解 |
|---|---|---|
| 保留外部 Node/npm | Adapter 可独立发布升级 | 安装族复杂；用统一解析器和手动 override 缓解 |
| Hook 写绝对 Node + cli.js | 不依赖 GUI/IDE PATH 和 `.cmd` | Node 切换会失效；doctor + repair 缓解 |
| 不执行 shell profile | 安全、确定、无副作用 | 无法覆盖任意自定义 shell 配置；提供手动选择 |
| 不自动提权/改 prefix | 不污染用户系统 | 某些系统级 npm 安装需用户处理；给出明确命令和替代路径 |
| 独立 toolchain.json | 边界清晰、可诊断 | 多一个 schema；由 ToolchainService 单点管理 |

## 17. 待评审决策

以下项目在实现前需形成 ADR/KAD 追加记录或明确产品结论：

1. Windows `.cmd` fallback 是否允许经 `cmd.exe /d /s /c` 执行；推荐优先解析到 `npm-cli.js`/Adapter `cli.js`，只在无法解析的受信 shim 上使用严格 runner。
2. 是否允许用户单独 override npm/Adapter；推荐 UI 默认只选择 Node，高级模式才开放逐项覆盖，减少混合安装族。
3. Adapter registry 目标版本由 Desktop 内置兼容表、远端元数据还是 npm dist-tag 决定；推荐使用 Desktop 兼容范围 + registry 明确版本，不能直接无条件安装 `latest`。
4. npm 升级失败的回滚保证如何实现；在有可验证机制前，不宣称自动保留旧版本。
5. 诊断日志中本地完整路径的保留策略与用户主动导出时的脱敏规则。

## 18. 完成定义

当且仅当以下条件全部成立，问题才视为解决：

- Windows GUI 进程 PATH 缺失时仍能发现已安装 Node/npm/Adapter。
- 自动发现失败时，用户能在应用内选择路径并看到字段级验证结果。
- 安装、升级、detect/install/uninstall/doctor 全部使用同一已解析工具链。
- 写入 Hook 的命令不依赖第三方工具进程的 PATH。
- 所有失败状态都有可执行的恢复路径和脱敏诊断。
- Windows 主要安装方式与三平台回归矩阵通过，真实 Hook 到灯效完成闭环。


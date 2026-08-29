# 持续集成

## 工作流入口

配置文件：`.github/workflows/ci.yml`

触发方式：

- 向 `main`、`master`、`develop` 推送代码
- 创建或更新 Pull Request
- 在 GitHub Actions 页面手动触发

同一分支产生新提交时，旧的未完成 CI 会被取消，以减少重复消耗。

## Job 说明

### Quality checks

运行环境为 `ubuntu-22.04`，超时时间 15 分钟，按顺序执行：

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm typecheck
pnpm build
```

任何步骤失败都会阻止该 Job 通过。

### Adapter checks

当 `packages/ailight-adapter/**`、workspace 配置、依赖锁文件或 workflow 发生变化时，在 `ubuntu-22.04` 上执行：

```bash
pnpm --dir packages/ailight-adapter check
pnpm --dir packages/ailight-adapter test
```

手动触发 CI 时也会执行该 Job。普通前端与纯文档变更不会单独启动 Adapter checks。

### Tauri build

工作流先按变更文件决定构建范围：纯文档改动只执行质量检查，不启动 Tauri 构建；Pull Request 和普通前端改动仅在 `ubuntu-22.04` 上执行原生 Tauri release 编译；Rust、Tauri、依赖锁文件或 workflow 改动在推送至 `main`、`master`、`develop` 时通过矩阵执行 Linux、macOS、Windows 三平台编译。手动触发始终执行三平台编译。单个平台超时时间为 45 分钟。

`ailight-core` 相关改动及手动全量运行时，Linux Tauri job 在编译应用前执行 core 全量测试；普通前端改动不重复运行 Rust 测试。Linux 系统依赖、pnpm 依赖和 Rust 构建产物均使用缓存；Release workflow 复用相同的 Linux 系统依赖缓存方案。

CI 使用 `--no-bundle` 校验前端与原生应用编译链路，不生成安装包；跨平台安装包由 Release 工作流统一生成。该策略让 PR 保留低成本的完整 Linux 编译门禁，并在代码进入主干后补充 macOS、Windows 兼容性反馈。

Linux 构建前会安装 WebKitGTK 等系统依赖。APT 设置 30 秒网络超时、3 次重试，安装步骤最多运行 8 分钟，避免镜像或网络异常导致 Job 长时间无结果。

三平台矩阵设置 `fail-fast: false`，一个平台失败不会取消其他平台，便于横向定位平台差异。

## 权限与依赖

- 工作流仅拥有 `contents: read` 权限
- Node.js 固定为 24
- pnpm 通过兼容 Node.js 24 的 `pnpm/action-setup@v6.0.9` 安装
- pnpm 版本由 `package.json` 的 `packageManager` 字段固定
- Rust 使用 stable 工具链
- 前端依赖必须与 `pnpm-lock.yaml` 一致
- Rust 依赖必须保留并提交 `src-tauri/Cargo.lock`

## 分支保护建议

在线上成功运行一次后，可在 GitHub 分支保护规则中将以下检查设为合并必需项：

- `Quality checks`
- `Tauri build (Linux)`

`Adapter checks` 只在相关路径变更时出现。如需将其设为 required check，应使用支持路径条件检查的规则，避免无关 PR 因该 Job 跳过而长期 pending。

macOS 和 Windows 检查不在 Pull Request 事件中运行，不应设置为 PR 合并必需项。

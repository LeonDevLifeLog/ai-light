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

### Tauri build

工作流通过矩阵在 `ubuntu-22.04`、`macos-latest`、`windows-latest` 上分别执行原生 Tauri release 编译，单个平台超时时间为 45 分钟。CI 使用 `--no-bundle` 校验前端与原生应用编译链路，不生成安装包；跨平台安装包由 Release 工作流统一生成。

Linux 构建前会安装 WebKitGTK 等系统依赖。APT 设置 30 秒网络超时、3 次重试，安装步骤最多运行 8 分钟，避免镜像或网络异常导致 Job 长时间无结果。

该矩阵用于验证三个桌面平台的真实编译链路，不能由单独的前端构建或 `cargo check` 替代。矩阵设置 `fail-fast: false`，一个平台失败不会取消其他平台，便于横向定位平台差异。

## 权限与依赖

- 工作流仅拥有 `contents: read` 权限
- Node.js 固定为 24
- pnpm 版本由 `package.json` 的 `packageManager` 字段固定
- Rust 使用 stable 工具链
- 前端依赖必须与 `pnpm-lock.yaml` 一致
- Rust 依赖必须保留并提交 `src-tauri/Cargo.lock`

## 分支保护建议

在线上成功运行一次后，可在 GitHub 分支保护规则中将以下检查设为合并必需项：

- `Quality checks`
- `Tauri build (Linux)`
- `Tauri build (macOS)`
- `Tauri build (Windows)`

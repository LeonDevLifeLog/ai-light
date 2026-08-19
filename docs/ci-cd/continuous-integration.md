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

### Tauri build (Linux)

运行环境为 `ubuntu-22.04`，超时时间 45 分钟。工作流安装 WebKitGTK 等系统依赖，通过 `tauri-apps/tauri-action@v1` 进行完整打包，并将安装包保存为 Workflow Artifact。

该 Job 用于验证真实桌面应用构建链路，不能由单独的前端构建或 `cargo check` 替代。

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


# CI/CD 故障排查

## Tauri 依赖版本不匹配

现象：`tauri-action` 报告 `Found version mismatched Tauri packages`。

处理：确保 `package.json` 中 `@tauri-apps/api` 与 `src-tauri/Cargo.toml` 中 `tauri` 使用相同的 major/minor 版本，并提交 `pnpm-lock.yaml` 和 `src-tauri/Cargo.lock`。不要关闭 Tauri CLI 的版本检查。

## 版本校验失败

现象：`Validate release version` 报告版本不一致或标签非法。

处理：确保 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 使用同一版本，并让标签等于该版本加 `v` 前缀。

## pnpm 安装失败

现象：`pnpm install --frozen-lockfile` 报告锁文件过期。

处理：使用项目声明的 pnpm 版本执行 `pnpm install`，检查依赖变更，然后提交更新后的 `pnpm-lock.yaml`。不要在 CI 中取消 `--frozen-lockfile` 来掩盖不一致。

## Linux Tauri 构建失败

优先检查：

- WebKitGTK 和 AppIndicator 系统包是否安装成功
- `src-tauri/Cargo.lock` 是否与 `Cargo.toml` 一致
- 前端 `pnpm build` 是否独立通过
- Runner 磁盘空间和 GitHub Actions 服务状态

## Release 上传失败

如果出现 `Resource not accessible by integration`：

- 确认 Release Job 保留 `contents: write`
- 确认仓库允许 GitHub Actions 创建 Release
- 确认使用工作流自动提供的 `secrets.GITHUB_TOKEN`

## macOS 应用提示损坏

当前构建使用 ad-hoc 签名，只解决无证书构建的基本签名问题，不能替代 Apple 公证。正式对外分发前，应配置 Developer ID Application 证书和 notarization。

## Windows 安装时出现信誉提示

当前 Windows 安装包未配置 Authenticode 证书。正式分发需要接入代码签名；仅重复构建不会消除 SmartScreen 信誉提示。

## 排查原则

1. 从首个失败步骤开始，不以后续连锁错误作为根因。
2. 使用与工作流一致的锁定依赖命令在本地复现。
3. 不通过放宽权限、关闭版本校验或取消锁文件约束规避问题。
4. 修复工作流后同步更新本目录中的进展和操作说明。

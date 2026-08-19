# CI/CD 建设进展

更新日期：2026-08-19

## 当前结论

项目已完成 CI/CD 基础设施配置：代码变更可执行质量检查和 Linux Tauri 打包验证；版本标签可触发 macOS、Linux、Windows 多平台构建，并生成 GitHub Draft Release。

## 已完成

| 分类 | 能力 | 实现位置 | 状态 |
| --- | --- | --- | --- |
| CI | 格式和静态规则检查 | `package.json`、`biome.jsonc` | 已完成 |
| CI | TypeScript 类型检查 | `.github/workflows/ci.yml` | 已完成 |
| CI | Vite 生产构建 | `.github/workflows/ci.yml` | 已完成 |
| CI | Linux、macOS、Windows Tauri release 编译 | `.github/workflows/ci.yml` | 已完成 |
| CI | pnpm 与 Rust 构建缓存 | `.github/workflows/ci.yml` | 已完成 |
| CI | 并发取消、最小权限、任务超时 | `.github/workflows/ci.yml` | 已完成 |
| CD | 标签和手动发布入口 | `.github/workflows/release.yml` | 已完成 |
| CD | 三处应用版本一致性校验 | `.github/workflows/release.yml` | 已完成 |
| CD | macOS ARM64/x64 构建 | `.github/workflows/release.yml` | 已完成 |
| CD | Linux x64 与 Windows x64 构建 | `.github/workflows/release.yml` | 已完成 |
| CD | 创建 Draft Release 并上传安装包 | `.github/workflows/release.yml` | 已完成 |
| CD | macOS ad-hoc 签名 | `.github/workflows/release.yml` | 已完成 |

## 验证记录

本地已通过以下检查：

- Ultracite/Biome 检查
- TypeScript 类型检查
- Vite 生产构建
- `cargo check --locked`
- GitHub Actions YAML 语法解析
- `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json` 版本一致性检查
- `git diff --check`

### GitHub Actions 运行记录

- 2026-08-19：PR #1 的首次 Linux Tauri 构建失败。根因是 Rust `tauri` 2.10 与 JavaScript `@tauri-apps/api` 2.11 的 minor 版本不一致。
- 2026-08-19：已将 Tauri API、CLI 和 Rust crate 对齐到 2.11 版本线，并将 `src-tauri/Cargo.lock` 纳入版本控制。本地 Tauri release 编译和锁定依赖检查已通过，等待 PR 重新运行确认。
- 2026-08-19：PR #1 后续运行在获取 Runner 前排队约 20 分钟，运行后停留于 Linux APT 依赖安装。CI 已扩展为三平台编译矩阵，并为 APT 增加超时和重试，等待线上确认。

## 待线上验证

- 确认 CI 质量检查及三个桌面平台编译均成功
- 首次推送版本标签后，确认四个平台构建成功
- 下载并安装各平台产物，执行启动和核心功能冒烟测试
- 确认多个矩阵任务能向同一个 Draft Release 正确汇总产物

## 未实施

- Apple Developer ID 正式签名和公证
- Windows Authenticode 代码签名
- Tauri Updater 应用内自动更新
- Linux ARM64、Windows ARM64 构建
- 自动化单元测试、端到端测试和安装包冒烟测试
- 依赖漏洞扫描、SBOM 和构建来源证明
- 自动发布正式 Release；当前必须人工验收 Draft 后发布

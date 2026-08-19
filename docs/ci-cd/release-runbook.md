# 发布操作手册

## 发布模型

配置文件：`.github/workflows/release.yml`

发布工作流构建以下目标：

| 平台 | 架构 | Runner |
| --- | --- | --- |
| macOS | ARM64 | `macos-latest` |
| macOS | x64 | `macos-latest` |
| Linux | x64 | `ubuntu-22.04` |
| Windows | x64 | `windows-latest` |

所有产物首先进入 GitHub Draft Release，不会自动公开。

## 发布前检查

1. 确认目标提交已通过 CI。
2. 同步修改以下三个版本号，且必须完全一致：
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
3. 执行本地检查：

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm typecheck
pnpm build
cargo check --locked --manifest-path src-tauri/Cargo.toml
```

4. 合并并推送版本变更。

## 标签发布

以 `0.1.0` 为例：

```bash
git tag v0.1.0
git push origin v0.1.0
```

标签必须符合 `v主版本.次版本.修订版本`，预发布版本可使用 `v0.2.0-beta.1`。工作流会拒绝非法标签或与应用版本不一致的标签。

## 手动重跑发布

在 GitHub Actions 中选择 `Release`，点击手动运行并输入已存在的版本标签，例如 `v0.1.0`。手动入口用于重试已有标签，不用于绕过版本校验。

## 发布验收

1. 确认 `Validate release version` 成功。
2. 确认四个平台矩阵任务均成功。
3. 在 Draft Release 中确认各平台安装包齐全。
4. 至少在目标平台执行安装、启动和核心功能冒烟测试。
5. 检查自动生成的 Release Notes。
6. 验收完成后，在 GitHub 页面人工发布 Draft Release。

## 失败处理与回滚

Draft Release 构建失败时不得发布。修复代码并更新版本后，应创建新标签；不要移动已经公开使用的版本标签。

如果标签尚未对外使用且明确需要删除，应同时删除远端标签和对应 Draft Release，再从正确提交重新创建。删除属于破坏性操作，执行前必须确认目标标签和 Release 均未公开使用。


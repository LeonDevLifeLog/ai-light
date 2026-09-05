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

macOS 打包任务还会自动检查产物 `Info.plist` 中的
`NSBluetoothAlwaysUsageDescription` 与
`NSBluetoothPeripheralUsageDescription`，防止 CoreBluetooth 在运行时被 TCC
直接终止。

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
3. 在 Draft Release 中确认各平台安装包齐全，并存在自动生成的 `update.json`；生成任务通过授权 Release 列表定位尚未公开、无法由 tag API 查询的 Draft，其中每个安装包都必须包含 GitHub 计算的 SHA-256、大小与发布后稳定的 tag 下载地址。
4. 至少在目标平台执行安装、启动和核心功能冒烟测试。
5. 检查自动生成的 Release Notes。
6. 验收完成后，在 GitHub 页面人工发布 Draft Release。

公开后，客户端通过 GitHub latest-release API 检测版本，并在官方 API、`ghfast.top`、`gh-proxy.com` 与 `ghproxy.net` 之间容错。下载前依次探测国内镜像，均不可用时回退到 Release 页面。公益镜像不作为版本事实源，也不自动执行下载后的安装包。

## 失败处理与回滚

Draft Release 构建失败时不得发布。修复代码并更新版本后，应创建新标签；不要移动已经公开使用的版本标签。

如果标签尚未对外使用且明确需要删除，应同时删除远端标签和对应 Draft Release，再从正确提交重新创建。删除属于破坏性操作，执行前必须确认目标标签和 Release 均未公开使用。

## npm Adapter 发布

### 发布边界

配置文件：`.github/workflows/npm-publish.yml`

`@ai-light/adapter` 与桌面应用使用独立版本线：

- 桌面应用标签：`v0.2.0`
- Adapter 标签：`adapter-v0.1.2`

Adapter 标签不会复用桌面应用版本校验，也不会创建四平台安装包。发布工作流使用 npm Trusted Publishing（OIDC），不保存长期 `NPM_TOKEN`；CI 只把候选包提交到 npm staging，维护者检查后使用 2FA 批准公开。

### 首次发布引导

npm staged publishing 要求包已存在。首次发布前，确认 npm 上已创建 `ai-light` organization，且当前账号有 scope 发布权限，然后执行：

```bash
cd packages/ailight-adapter
npm login --registry=https://registry.npmjs.org/
npm publish --access public --registry=https://registry.npmjs.org/
```

首次发布成功后，在 npm 包设置中添加 Trusted Publisher：

| 字段 | 值 |
| --- | --- |
| Provider | GitHub Actions |
| Organization/user | `LeonDevLifeLog` |
| Repository | `ai-light` |
| Workflow filename | `npm-publish.yml` |
| Allowed action | `npm stage publish` |

随后将 Publishing access 设置为要求 2FA 并禁止 token 发布。仓库为 private 时可以使用 Trusted Publishing，但 npm 不生成 provenance。

### 发布前检查

1. 确认目标提交的 `Quality checks` 与 `Adapter checks` 均通过。
2. 更新 `packages/ailight-adapter/package.json` 的版本号；CLI 会在运行时读取该文件，不维护第二份版本常量。
3. 执行：

```bash
pnpm --dir packages/ailight-adapter check
pnpm --dir packages/ailight-adapter test
cd packages/ailight-adapter
npm pack --dry-run --registry=https://registry.npmjs.org/
```

### 标签与审批

稳定版和预发布版本分别使用：

```bash
git tag adapter-v0.1.2
git push origin adapter-v0.1.2

git tag adapter-v0.2.0-beta.1
git push origin adapter-v0.2.0-beta.1
```

工作流要求标签与 Adapter 版本完全一致。稳定版本 stage 到 `latest`，含 SemVer prerelease 后缀的版本 stage 到 `next`。

Action 成功后，在 npmjs.com 的 Staged Packages 页面检查包名、版本、dist-tag、文件清单和 tarball，再使用 2FA 批准。审批后执行：

```bash
npm view @ai-light/adapter version --registry=https://registry.npmjs.org/
npm install --global @ai-light/adapter@0.1.2 \
  --registry=https://registry.npmjs.org/
ailight-adapter version --json
ailight-adapter doctor --json
```

### 手动重跑与失败处理

在 GitHub Actions 中选择 `Publish npm adapter`，输入已经存在的 `adapter-v*` 标签，可以重试失败的构建或 staging。手动入口仍会校验标签与包版本，不能从任意分支发布。

版本一旦进入 staging，就占用该 SemVer。构建或测试失败时修复代码并递增版本；staging 内容不正确时，先在 npmjs.com 拒绝候选包，再按确认后的版本策略重新发布。不要移动已经公开使用的 Git 标签。

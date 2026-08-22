# AI-Light

AI-Light 是一款面向 macOS、Windows 和 Linux 的智能体状态灯客户端。它接收 Claude Code、Codex、Qoder 等 AI 编程工具的任务事件，将 `WORKING`、`WAITING`、`SUCCESS`、`ERROR` 等状态实时转换为蓝牙状态灯的灯效与提示音。

应用本地运行，不需要云端中转。接入工具只需调用一个回环地址 HTTP API；灯效策略由可分享的 JSON 主题定义。

## 核心能力

- **统一状态模型**：标准五态 `IDLE / WORKING / WAITING / SUCCESS / ERROR`，同时支持自定义状态。
- **多来源仲裁**：多个智能体同时上报时，按优先级和最近活跃时间确定当前展示状态。
- **本地 Hook API**：仅监听 `127.0.0.1`，支持可选 Bearer Token、OpenAPI 3.1 规范和 Swagger UI 调试。
- **可编程主题**：使用 `.ailight-theme.json` 定义状态、灯光曲线、颜色、亮度、相位与提示音。
- **可靠设备写入**：所有场景通过单写入队列串行下发，避免并发写入造成状态错乱。
- **跨平台桌面体验**：基于 Tauri v2、React 19 和 Rust，支持单实例和本地配置。

## 工作方式

```text
AI 工具 / 适配器
       │ HTTP 状态事件
       ▼
Hook API → 状态仲裁 → 主题映射 → 场景编译 → BLE 状态灯
```

机制与策略相互分离：设备只执行灯光和声音场景，业务状态到场景的映射全部保留在客户端主题中。新增工具通常只需增加适配器，新增视觉风格只需增加主题。

## 快速体验 Hook API

应用启动后，Hook Server 默认监听 `http://127.0.0.1:25679`。如果首选端口被占用，会向后尝试最多 10 个端口；也可在设置页保存新端口并单独热重启 Hook Server。

```bash
curl http://127.0.0.1:25679/hook \
  --request POST \
  --header 'Content-Type: application/json' \
  --data '{
    "source": "manual",
    "event": "state_change",
    "state": "WORKING"
  }'
```

运行时接口：

- OpenAPI JSON：`http://127.0.0.1:25679/openapi.json`
- Swagger UI：`http://127.0.0.1:25679/docs`
- 当前状态：`http://127.0.0.1:25679/api/status`
- 健康检查：`http://127.0.0.1:25679/api/health`

完整说明见 [Hook API 使用指南](./docs/specs/hook-api.md)。

## 主题

主题文件使用 `.ailight-theme.json` 扩展名，由场景库和状态映射组成：

```json
{
  "theme": {
    "name": "minimal-blue",
    "version": 1
  },
  "scenes": {
    "blue": {
      "leds": [
        {
          "curve": "CONSTANT",
          "high": "#168CFF",
          "brightness": 60
        },
        null,
        null
      ]
    }
  },
  "states": {
    "WORKING": {
      "scene": "blue"
    }
  }
}
```

- [主题格式指南](./docs/specs/theme-format.md)
- [JSON Schema Draft 2020-12](./docs/specs/theme.schema.json)
- [六套内置主题](./docs/specs/themes/README.md)

## 技术架构

| 层级 | 位置 | 职责 |
|---|---|---|
| Rust Core | `crates/ailight-core/` | 协议、主题、仲裁、Hook API、写入队列和 BLE |
| Tauri Shell | `src-tauri/` | Commands、Events、单实例及桌面生命周期 |
| Web UI | `src/` | React 页面、设备管理、主题创作和试听 |
| Specifications | `docs/specs/` | HTTP、IPC、主题、架构和 UI 契约 |

Rust Core 不依赖 Tauri，可以独立编译和测试。

## 本地开发

### 环境要求

- Node.js 与 pnpm 10
- Rust stable（rustup）
- [Tauri v2 平台依赖](https://v2.tauri.app/start/prerequisites/)

安装依赖：

```bash
pnpm install
```

启动前端或完整桌面应用：

```bash
pnpm dev
pnpm tauri dev
```

质量检查：

```bash
pnpm check
pnpm typecheck
pnpm build

cd crates/ailight-core
cargo test

cd ../../src-tauri
cargo check
```

## 文档导航

| 主题 | 文档 |
|---|---|
| Hook API 指南 | [docs/specs/hook-api.md](./docs/specs/hook-api.md) |
| OpenAPI 3.1 | 运行应用后访问 `/openapi.json`（由 Rust DTO 与 Handler 注解生成） |
| 主题格式指南 | [docs/specs/theme-format.md](./docs/specs/theme-format.md) |
| Theme JSON Schema | [docs/specs/theme.schema.json](./docs/specs/theme.schema.json)（由 Rust DTO 生成） |
| IPC 契约 | [docs/specs/ipc-contract.md](./docs/specs/ipc-contract.md) |
| 技术架构 | [docs/specs/architecture.md](./docs/specs/architecture.md) |
| 产品边界 | [docs/requirements/product-boundary.md](./docs/requirements/product-boundary.md) |
| UI 交互说明 | [docs/specs/ui-interactions.md](./docs/specs/ui-interactions.md) |
| UI 组件契约 | [docs/specs/ui-interaction-spec.md](./docs/specs/ui-interaction-spec.md) |
| CI/CD | [docs/README.md](./docs/README.md) |

## 发布

版本号必须同步更新：

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

推送匹配的 `v*` 标签后，GitHub Actions 会构建 macOS、Windows 和 Linux 安装包，并创建待验收的草稿 Release。详见 [发布操作手册](./docs/ci-cd/release-runbook.md)。

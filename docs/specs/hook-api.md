# AI-Light Hook API 使用指南

| 项目 | 内容 |
|---|---|
| API 版本 | V1.0 |
| 机器契约 | 运行时 `GET /openapi.json`（由 Rust DTO 与 Handler 注解生成） |
| 默认地址 | `http://127.0.0.1:47800` |
| 服务范围 | 仅本机回环地址 |

Hook API 接收 AI 编程工具或适配器上报的任务状态。AI-Light 对多个来源的状态进行仲裁，再按当前主题转换为灯光和提示音。

## 1. 在线文档

应用运行后，Hook Server 同时提供规范文件和调试界面：

| 地址 | 用途 |
|---|---|
| `GET /openapi.json` | OpenAPI 3.1 JSON，可用于生成客户端、导入 Postman 或自动测试 |
| `GET /docs` | Swagger UI，可查看模型并直接调用接口 |

端口默认是 `47800`。端口被占用时，应用依次尝试 `47801` 至 `47810`；实际端口可在应用状态或 `/api/status` 的 `service.port` 中查看。Swagger UI 的 HTML、JavaScript 和 CSS 均嵌入应用并由 Hook Server 本地提供，不需要访问外部 CDN。

## 2. 快速开始

```bash
curl http://127.0.0.1:47800/hook \
  --request POST \
  --header 'Content-Type: application/json' \
  --data '{
    "source": "manual",
    "event": "state_change",
    "state": "WORKING",
    "session": "task-001",
    "meta": { "detail": "running tests" }
  }'
```

成功响应：

```json
{
  "ok": true,
  "applied": true,
  "detail": "state=WORKING applied=true"
}
```

查询状态与健康情况：

```bash
curl http://127.0.0.1:47800/api/status
curl http://127.0.0.1:47800/api/health
```

## 3. 接口一览

| 方法 | 路径 | 说明 |
|---|---|---|
| `POST` | `/hook` | 上报状态变化 |
| `GET` | `/api/status` | 查询服务、设备及当前业务状态快照 |
| `GET` | `/api/health` | 检查 Hook Server 是否运行 |
| `GET` | `/openapi.json` | 获取机器可读 API 契约 |
| `GET` | `/docs` | 打开交互式调试页面 |

完整的请求模型、响应模型、状态码和示例以 Rust DTO 与 Handler 注解为唯一事实源，`GET /openapi.json` 是它们生成的机器可读结果。

## 4. 状态事件

| 字段 | 必填 | 说明 |
|---|---|---|
| `source` | 是 | 来源标识，格式为 `[A-Za-z0-9_-]+`，最长 64 字符 |
| `event` | 是 | V1 固定为 `state_change` |
| `state` | 是 | `IDLE / WORKING / WAITING / SUCCESS / ERROR` 或自定义状态名 |
| `session` | 否 | 调用方会话标识，仅透传记录 |
| `ts` | 否 | Unix 毫秒时间戳；省略时使用服务端接收时间 |
| `meta` | 否 | JSON 对象形式的附加信息，不参与仲裁 |

连续收到相同 `source + state` 时，第一次通常返回 `applied=true`，后续返回 `applied=false`。后者表示事件已受理，但没有重播提示音或重置灯效相位，因此网络失败后的重试是安全的。

建议仅在网络错误或 `5xx` 时重试，最多两次，间隔 500 ms。`4xx` 表示请求本身需要修正。

## 5. 身份验证

Hook Server 始终只绑定 `127.0.0.1`，不开放局域网访问。接入密码默认关闭；开启后，`POST /hook` 必须携带：

```http
Authorization: Bearer <token>
```

调试时可在 Swagger UI 右上角选择 **Authorize** 输入 Token。

## 6. 来源命名建议

来源名不需要预注册。建议使用稳定、可辨识的小写名称，例如：

| `source` | 含义 |
|---|---|
| `claude-code` | Claude Code 适配器 |
| `qoder` | Qoder 适配器 |
| `codex` | Codex CLI 或桌面端适配器 |
| `manual` | 手动调试 |
| `_test` | 自动化测试 |

新增来源不需要修改 AI-Light，只需让适配器按契约发送事件。

## 7. 契约维护

- 修改公开路由时同步 Handler 的 `#[utoipa::path]` 注解；修改字段时同步 DTO 的文档注释与 `#[schema]` 约束。
- Hook Server 测试会验证生成的 OpenAPI 包含全部业务端点、DTO 字段说明、camelCase 命名、关键约束和安全定义。
- Markdown 负责概念、教程与接入建议，不复制维护完整 Schema。

相关设计：

- [产品边界](../requirements/product-boundary.md)
- [IPC 契约](./ipc-contract.md)
- [主题格式](./theme-format.md)
- [ADR-0001](../decisions/ADR-0001-接入层设计决策.md)

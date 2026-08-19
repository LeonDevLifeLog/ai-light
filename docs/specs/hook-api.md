# AI-Light 接入层 Hook API 规范

| 项目 | 内容 |
|---|---|
| 文档版本 | V1.0（正式版） |
| 文档状态 | 生效（ADR-0001 Q7 确定） |
| 基线 | 本地 HTTP 服务，端口 47800（延续 PCDaemon），仅回环 |
| 生效日期 | 2026-08-19 |

> 本文是 L1 接入层的正式接口规范：智能体客户端（或适配器）通过本 API 把任务生命周期事件上报给 AI-Light，由客户端仲裁（L2）→ 主题映射（L2）→ 协议下发（L3）点亮灯。

---

## 1. 服务约定

| 项目 | 约定 |
|---|---|
| Base URL | `http://127.0.0.1:47800` |
| 监听范围 | **仅本机回环**（127.0.0.1），不做局域网暴露 |
| 内容类型 | `application/json; charset=utf-8` |
| 端口冲突 | 端口被占用时客户端自动退避（尝试 47801~47810），实际端口见 `GET /api/status` 返回与托盘提示 |

## 2. 端点总览

| 方法 | 路径 | 用途 | 调用方 |
|---|---|---|---|
| POST | `/hook` | 上报状态事件 | 智能体客户端 hooks / 适配器 |
| GET | `/api/status` | 查询当前业务状态与设备状态 | 外部集成/监控/排障 |
| GET | `/api/health` | 健康检查（服务存活） | 适配器启动自检 |

## 3. POST /hook —— 状态事件

### 3.1 请求

```json
{
  "source": "claude-code",
  "event": "state_change",
  "state": "WORKING",
  "session": "task-001",
  "ts": 1724040000000,
  "meta": { "detail": "running tests", "progress": 60 }
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `source` | string | ✅ | 事件来源标识（客户端/适配器注册名，见 §6） |
| `event` | string | ✅ | 事件类型，当前唯一值 `state_change`；保留扩展（如 `direct_scene` V2） |
| `state` | string | ✅ | 标准状态（`IDLE/WORKING/WAITING/SUCCESS/ERROR`）或自定义状态名；`[a-zA-Z0-9_-]+` |
| `session` | string | 否 | 会话标识；第一期仅透传记录，不参与业务决策（ADR-0001 Q9） |
| `ts` | int | 否 | 事件时间（Unix 毫秒）；缺省 = 服务端接收时间 |
| `meta` | object | 否 | 附加信息（detail 描述、progress 进度等），客户端透传展示，不参与仲裁 |

### 3.2 应答

成功：

```json
{ "ok": true, "applied": true, "detail": "state=WORKING applied" }
```

| 字段 | 说明 |
|---|---|
| `ok` | 请求受理与否 |
| `applied` | 该事件是否实际改变了灯效（**幂等对账**）：`true` = 状态变更已生效；`false` = 与当前状态相同，未重复触发（不重播蜂鸣/不重置相位） |
| `detail` | 人类可读说明 |

**幂等语义**：相同 `source + state` 的连续事件 → 第二次起 `applied=false`（不重播），与 V0.4 `APPLY_IF_CHANGED` 一脉相承。需要强制重播（如试听）→ 由客户端 UI 走 `RESTART_SCENE` 通道，hook 不提供强制语义。

### 3.3 错误码

| HTTP | code | 含义 |
|---|---|---|
| 200 | `ok:true` | 受理 |
| 400 | `INVALID_REQUEST` | JSON 非法 / 缺必填字段 / state 或 source 格式非法 |
| 401 | `UNAUTHORIZED` | token 缺失或不匹配（开启校验时） |
| 404 | `NOT_FOUND` | 路径不存在 |
| 500 | `INTERNAL_ERROR` | 客户端内部错误（如 BLE 下发失败）——事件仍被记录，返回重试建议 |

错误应答体：`{ "ok": false, "code": "INVALID_REQUEST", "detail": "…" }`

### 3.4 重试约定

- 调用方建议：失败（网络错误/5xx）时重试最多 2 次，间隔 500 ms；幂等保证重试安全
- 客户端侧：收到重复事件（同 source+state）不重复执行副作用（对账见 3.2）

## 4. GET /api/status —— 状态查询

```json
{
  "service": { "version": "0.1.0", "port": 47800 },
  "device": {
    "connected": true,
    "name": "ACLight-1A2B",
    "battery_percent": 75,
    "power_source": "BATTERY",
    "fw_version": "1.0.0"
  },
  "business": {
    "state": "WORKING",
    "source": "claude-code",
    "session": "task-001",
    "since_ms": 1724040000000,
    "theme": "default"
  }
}
```

- `device` 各字段按能力位可缺省（无电池版无 battery 字段）；未连接时 `connected:false`
- `business.state` = 当前有效业务状态（仲裁结果）

## 5. GET /api/health —— 健康检查

```json
{ "ok": true, "version": "0.1.0" }
```

## 6. source 注册约定（适配器命名）

| source | 含义 | 接入形态 |
|---|---|---|
| `claude-code` | Claude Code（CLI/IDE/Desktop 内嵌） | 🟢 配置模板 |
| `qoder` | Qoder CLI | 🟢 配置模板 |
| `codex` | Codex CLI / Desktop | 🟢 配置模板 |
| `cursor` | Cursor（预留，第一期不接入） | 🟡 桥接进程（存档） |
| `manual` | 手动触发面板（UI 内置） | — |
| `_test` | 测试/调试 | — |

规则：`[a-z0-9_-]+`；新增客户端 = 新 source + 适配器，客户端本体零改动。

## 7. 安全

- **仅回环**：绑定 127.0.0.1；不绑定 0.0.0.0，防火墙不开放
- **可选 token**：客户端设置开启后，请求必须带 `Authorization: Bearer <token>`；默认关闭
- 不开 CORS（无浏览器跨源需求）；不做局域网/远程访问（V2 再议）

## 8. 示例

### 8.1 curl 手动触发

```bash
curl -X POST http://127.0.0.1:47800/hook \
  -H 'Content-Type: application/json' \
  -d '{"source":"manual","event":"state_change","state":"WORKING","meta":{"detail":"手动测试"}}'

# → {"ok":true,"applied":true,"detail":"state=WORKING applied"}
```

### 8.2 Claude Code HTTP hook 配置（~/.claude/settings.json，待实测确认）

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47800/hook",
      "body": "{\"source\":\"claude-code\",\"event\":\"state_change\",\"state\":\"WORKING\",\"session\":\"${session_id}\"}" }] }],
    "Stop":             [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47800/hook",
      "body": "{\"source\":\"claude-code\",\"event\":\"state_change\",\"state\":\"SUCCESS\",\"session\":\"${session_id}\"}" }] }],
    "StopFailure":      [{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:47800/hook",
      "body": "{\"source\":\"claude-code\",\"event\":\"state_change\",\"state\":\"ERROR\",\"session\":\"${session_id}\"}" }] }],
    "Notification":     [{ "matcher": "idle_prompt|permission_prompt", "hooks": [{ "type": "http", "url": "http://127.0.0.1:47800/hook",
      "body": "{\"source\":\"claude-code\",\"event\":\"state_change\",\"state\":\"WAITING\",\"session\":\"${session_id}\"}" }] }]
  }
}
```

> ⚠️ 配置模板待本机实测（Q6 延后项）：HTTP hook 的变量占位（`${session_id}`）、请求格式、事件时序需实测后定稿，正式模板入 `docs/specs/adapters/`。

### 8.3 Codex / Qoder 配置（示意，均待实测）

- Codex：`~/.codex/hooks.json`（command 类型 curl）+ `config.toml` `notify` 追加 `turn-ended`
- Qoder：`~/.qoder/settings.json`（command 类型 curl，事件与 Claude Code 同构）

## 9. 与 PCDaemon 的延续关系

- 端口 47800 延续；应答格式 `{ok, detail}` 延续并增加 `applied` 对账
- 传输从 TCP JSON 行协议升级为 **HTTP**（一行 curl 即可调用，任何 hook 机制天然支持）
- 状态语义从 PCDaemon 的 AI 状态机升级为**标准 5 态 + 开放状态名**（ADR-0001 Q1）

## 10. 参考

- 决策：`docs/decisions/ADR-0001-接入层设计决策.md`（Q7/Q8/Q9）
- 调研：`docs/research/接入层调研报告_V0.1.md`
- 上游：`docs/requirements/product-boundary.md`（L1 接入层）

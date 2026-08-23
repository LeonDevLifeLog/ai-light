# Claude Code Hooks → AI-Light 状态映射研究

| 项目 | 内容 |
|---|---|
| 状态 | 已完成官方文档审阅并落地 V1 映射 |
| 日期 | 2026-08-22 |
| 目标版本 | Claude Code 2.1.220（本机）；官方文档包含截至 2.1.236 的字段说明 |
| 事实源 | [Hooks Reference](https://code.claude.com/docs/en/hooks)、[Hooks Guide](https://code.claude.com/docs/en/hooks-guide) |

## 1. 结论

AI-Light 不应把每个 Hook 都变成灯牌状态。Claude Code 的 Hook 分为会话、回合、工具、展示、配置、工作树、压缩、MCP 交互、子智能体等多种用途；大量事件只是审计点，不代表用户可感知的业务状态边界。

V1 采用以下原则：

1. 只映射具有确定状态语义的边界事件。
2. 主会话是唯一展示对象；带 `agent_id` 的子智能体内部事件不参与灯牌状态。
3. 并行工具恢复使用 `PostToolBatch`，不使用每工具一次且并发触发的 `PostToolUse`。
4. 工具失败不等于回合失败；`PostToolUseFailure` 表示 Claude 获得错误并继续处理，映射为 `WORKING`。只有 `StopFailure` 映射 `ERROR`。
5. `Stop` 只表示本轮响应结束，不表示工程目标完成；若仍有 `background_tasks`，保持 `WORKING`。
6. 不通过计时器、终端文本或窗口检测猜测状态。

## 2. 生命周期模型

```text
SessionStart → IDLE
  └─ UserPromptSubmit → WORKING
       ├─ PermissionRequest → WAITING
       │    └─ 工具批次结束 → PostToolBatch → WORKING
       ├─ AskUserQuestion / ExitPlanMode → WAITING
       │    └─ 用户回答后的下一批处理 → PostToolBatch → WORKING
       ├─ Elicitation → WAITING
       │    └─ ElicitationResult → WORKING
       ├─ PermissionDenied / PostToolUseFailure → WORKING
       ├─ Stop(background_tasks 非空) → WORKING
       ├─ Stop(background_tasks 为空) → SUCCESS
       └─ StopFailure → ERROR
SessionEnd → IDLE
```

## 3. 正式订阅与映射

| Hook | 条件 | 状态 | 依据与语义 |
|---|---|---|---|
| `SessionStart` | 主会话 | `IDLE` | 会话存在，但尚无用户任务 |
| `UserPromptSubmit` | 主会话 | `WORKING` | 提示词已提交，模型即将处理 |
| `PreToolUse` | `AskUserQuestion\|ExitPlanMode` | `WAITING` | 工具本身会要求用户输入或确认 |
| `PermissionRequest` | 主会话 | `WAITING` | 权限对话框即将出现；即时事件 |
| `PermissionDenied` | 主会话 | `WORKING` | 自动模式拒绝后，模型收到结果并继续决策 |
| `Elicitation` | 主会话 | `WAITING` | MCP 服务请求用户填写表单或打开 URL |
| `ElicitationResult` | 主会话 | `WORKING` | 用户响应已产生，即将回传 MCP 服务 |
| `Notification` | `permission_prompt` | `WAITING` | 权限等待约 6 秒后的兜底通知 |
| `Notification` | `elicitation_dialog\|elicitation_url_dialog` | `WAITING` | MCP 输入等待约 6 秒后的兜底通知 |
| `Notification` | `agent_needs_input` | `WAITING` | 后台会话明确等待用户输入 |
| `Notification` | `elicitation_complete\|elicitation_response` | `WORKING` | MCP 交互完成/响应已发送的恢复兜底 |
| `PostToolBatch` | 主会话 | `WORKING` | 整批并行工具全部完成、下一次模型调用之前；无 matcher 且每批仅一次 |
| `PostToolUseFailure` | 主会话 | `WORKING` | 单个工具失败已反馈给 Claude，回合通常继续 |
| `Stop` | `background_tasks` 非空 | `WORKING` | 主响应暂止，但后台任务仍在执行 |
| `Stop` | 无后台任务 | `SUCCESS` | 本轮响应正常结束，不宣称完整工程目标完成 |
| `StopFailure` | 主会话 | `ERROR` | API、鉴权、限流等导致整个回合异常结束 |
| `SessionEnd` | 主会话 | `IDLE` | 会话退出、清除或切换，释放来源 |

所有条目首先应用 `agent_id` 过滤：字段存在时视为子智能体内部事件，V1 忽略。

## 4. 完整 Hook 点分类

| Hook | V1 处理 | 原因 |
|---|---|---|
| `SessionStart` | `IDLE` | 主会话边界 |
| `Setup` | 忽略 | 仅 `--init/--maintenance`，不是交互任务状态 |
| `InstructionsLoaded` | 忽略 | 异步审计事件 |
| `UserPromptSubmit` | `WORKING` | 回合开始边界 |
| `UserPromptExpansion` | 忽略 | 命令展开，随后仍有提示提交/工具事件 |
| `MessageDisplay` | 忽略 | 高频展示批次，不代表状态变化 |
| `PreToolUse` | 条件 `WAITING` | 仅用户交互型工具具有状态意义 |
| `PermissionRequest` | `WAITING` | 权限等待即时边界 |
| `PostToolUse` | 忽略 | 并行工具会并发多次触发；由 `PostToolBatch` 取代 |
| `PostToolUseFailure` | `WORKING` | 模型收到失败后继续处理 |
| `PostToolBatch` | `WORKING` | 批次恢复的唯一确定边界 |
| `PermissionDenied` | `WORKING` | 仅 auto mode；拒绝结果返回模型 |
| `Notification` | 条件映射 | 只使用明确的注意/恢复类型；忽略认证和完成歧义项 |
| `SubagentStart` / `SubagentStop` | 忽略 | 主会话仍处于工作中，避免子任务抢灯 |
| `TaskCreated` / `TaskCompleted` | 忽略 | 任务清单操作不等于主回合状态 |
| `Stop` | `SUCCESS` 或 `WORKING` | 根据后台任务区分真正结束和暂时停顿 |
| `StopFailure` | `ERROR` | 回合级失败 |
| `TeammateIdle` | 忽略 | 队友局部状态，不代表主会话等待用户 |
| `ConfigChange` | 忽略 | 配置审计事件 |
| `CwdChanged` / `DirectoryAdded` / `FileChanged` | 忽略 | 环境变化，不是业务状态边界 |
| `WorktreeCreate` / `WorktreeRemove` | 忽略 | 工作树生命周期，不是主回合状态 |
| `PreCompact` / `PostCompact` | 忽略 | 压缩可能发生在任务内或手动触发，单独映射会产生假状态 |
| `Elicitation` | `WAITING` | MCP 明确请求用户输入 |
| `ElicitationResult` | `WORKING` | 用户响应完成 |
| `SessionEnd` | `IDLE` | 会话释放边界 |

## 5. Notification 类型

| notification_type | V1 | 说明 |
|---|---|---|
| `permission_prompt` | `WAITING` | 延迟兜底，不能替代即时 `PermissionRequest` |
| `elicitation_dialog` | `WAITING` | MCP 表单等待兜底 |
| `elicitation_url_dialog` | `WAITING` | MCP 浏览器认证等待兜底 |
| `elicitation_complete` | `WORKING` | 交互完成恢复兜底 |
| `elicitation_response` | `WORKING` | 响应已发回服务端 |
| `agent_needs_input` | `WAITING` | 后台会话明确需要用户输入 |
| `idle_prompt` | 忽略 | 在 `Stop → SUCCESS` 约 60 秒后触发；改成等待会破坏完成态 |
| `auth_success` | 忽略 | 身份认证事件，不代表当前回合工作状态 |
| `agent_completed` | 忽略 | 同一类型同时覆盖成功和失败，无法可靠归一为单一状态 |

## 6. 已知不可观测边界

### 6.1 普通权限批准瞬间

`PermissionRequest` 在对话框出现前触发，但官方没有“用户已批准普通权限”的对应 Hook。`PostToolBatch` 只能在获批工具执行完毕后恢复 `WORKING`。因此长时间命令在执行期间可能继续显示 `WAITING`。这是官方事件模型的缺口，不通过超时猜测。

### 6.2 手动拒绝权限

`PermissionDenied` 只覆盖 auto mode，不覆盖用户手动拒绝、`PreToolUse` 阻止或 deny rule。是否随后触发 `PostToolBatch` 需实机验证；在没有确定事件前不映射。

### 6.3 用户中断

官方明确 `Stop` 不在用户中断时触发。若会话未结束，可能保留先前状态；`SessionEnd` 才能确定回到 `IDLE`。未来若产品必须精确表达中断，需要 Claude 提供新 Hook 或引入更高层会话通道。

## 7. 性能与可靠性

- Adapter command hook 必须快速、静默、失败开放；当前 HTTP 投递超时 300ms，Hook 进程失败不阻断 Claude。
- `PostToolBatch` 替代并发 `PostToolUse`，减少 Node 进程启动次数和灯效重复写入。
- `PermissionRequest` 与延迟 Notification 可能重复，但服务端相同 source/state 幂等去重。
- 使用 exec form 的绝对 Node 路径和 CLI 脚本路径，避免 shell 注入及 macOS GUI PATH 差异。
- 用户可在 Claude Code `/hooks` 中核对实际生效的 Hook；配置文件热更新通常自动生效，异常时重启会话。

## 8. 验收场景

1. 普通提示：`IDLE → WORKING → SUCCESS`。
2. 权限批准：`WORKING → WAITING → PostToolBatch 后 WORKING → SUCCESS`。
3. AskUserQuestion：`WORKING → WAITING → 后续工具批次 WORKING → SUCCESS`。
4. MCP Elicitation：`WORKING → WAITING → ElicitationResult WORKING`。
5. 单工具失败后自愈：保持/恢复 `WORKING`，最终由 `Stop` 或 `StopFailure` 定终态。
6. API 错误：`WORKING → ERROR`。
7. 后台任务仍运行的 Stop：保持 `WORKING`。
8. 子智能体内部权限/工具事件：不得改变主会话灯效。
9. 会话退出或 `/clear`：`IDLE`。


# AI-Light 日志与可观测性规范

| 属性 | 值 |
|---|---|
| 版本 | V1.1 |
| 范围 | Desktop / ailight-core |
| 决策来源 | architecture.md KAD-05 / KAD-18 |

## 1. 目标与边界

日志用于在本机还原关键生命周期和事件因果链，回答：什么时间收到什么信号、由谁触发、状态为何变化、映射了什么业务状态、是否成功下发设备。

V1 不提供日志上传、远程遥测、应用内日志查看或指标数据库。日志写入失败不得改变 Hook、仲裁和设备控制语义。

## 2. 输出与生命周期

- 同时写入 stderr 与应用日志目录中的按日滚动文件 `ailight.log.YYYY-MM-DD`。
- 默认发布构建为 `info`，调试构建为 `debug`。
- non-blocking writer guard 必须由应用状态持有到进程退出，保证缓冲日志正常刷盘。
- 每行由 `tracing-subscriber` 自动附加时间、级别、target 与结构化字段。
- 历史日志保留 14 天，并受 100 MiB 目录软上限约束；最新活动日志始终保留。
- 应用启动及随后每 6 小时清理一次；清理失败仅记 WARN，不阻塞业务。
- 只清理 `ailight.log.*` 普通文件，不跟随符号链接，不处理 Adapter 或其他文件。
- V1 不按大小截断当前活动文件，因此单日活动日志可能暂时超过软上限。

## 3. 公共字段

| 字段 | 含义 | 适用范围 |
|---|---|---|
| `event` | 稳定、机器可读的事件名称 | 所有关键日志 |
| `signal_id` | 单次业务信号的关联 ID | Hook、手动状态、仲裁、SCENE 队列 |
| `source` | 信号来源，如 `codex` | 状态链路 |
| `from_state` / `to_state` | 状态切换前后值 | 状态机 |
| `reason` | 切换、拒绝、重试或断开的稳定原因 | 分支事件 |
| `generation` | BLE 连接代次，用于识别过期任务 | 连接/重连 |
| `cmd` / `seq` | BLE 命令字与协议序列号 | 协议事务 |
| `attempt` / `retries` | 当前尝试与已重试次数 | 可重试操作 |
| `error` | 最终或阶段性错误摘要 | WARN / ERROR |

`signal_id` 格式是进程内唯一的 `sig-<接收时间十六进制>-<递增序号>`。它不是安全令牌，也不跨进程保证唯一。

## 4. 级别约定

| 级别 | 使用条件 | 示例 |
|---|---|---|
| `ERROR` | 操作最终失败、后台服务异常退出 | Hook 处理失败、SCENE 最终下发失败 |
| `WARN` | 可恢复异常、输入拒绝、重试与降级 | token 不匹配、端口退避、BLE 超时重试 |
| `INFO` | 生命周期、用户动作、真实状态变化、最终成功 | 应用启动、状态切换、设备连接完成 |
| `DEBUG` | 幂等事件、协议阶段、内部映射 | 状态未变化、BLE 命令发送与应答 |
| `TRACE` | 原始协议诊断 | 默认关闭，V1 不记录原始 Hook 正文 |

重复的同 source + state 信号只记录 `business_state_unchanged` DEBUG；只有真实切换记录 `business_state_transition` INFO。

## 5. 核心事件目录

### 5.1 应用与配置

`app_starting`、`config_load_degraded`、`config_loaded`、`theme_loaded`、`tray_initialized`、`app_started`、`app_ready`、`window_hidden`、`app_exiting`、`config_update_requested`、`config_updated`、`log_retention_cleanup_completed`、`log_retention_cleanup_failed`。

### 5.2 信号与业务状态

`hook_signal_received` → `hook_signal_rejected | hook_signal_failed | hook_signal_accepted`。

受理后：`business_state_transition | business_state_unchanged` → `scene_compiled` → `scene_dispatch_started` → `scene_dispatch_completed | scene_dispatch_failed`。

终态驻留到期以 `business_state_transition reason="hold_expired"` 记录。手动触发也生成 `signal_id`，沿用同一后半链路。

### 5.3 主题与输出

`theme_change_requested`、`theme_changed`、`theme_imported`、`theme_deleted`、`theme_state_unmapped`、`scene_preview_started`、`scene_preview_completed`、`outputs_reset_started`、`outputs_reset_completed`。

### 5.4 Hook Server

`hook_server_started`、`hook_server_port_fallback`、`hook_server_start_failed`、`hook_server_failed`。

### 5.5 BLE

`ble_scan_started`、`ble_scan_completed`、既有连接阶段日志、`device_connected`、断开/重连日志、`device_scene_resync_started`、`device_scene_resync_completed`、`ble_command_sent`、`ble_command_acknowledged`、`ble_command_retry`、`ble_command_failed`、`scene_rejected`。

## 6. 隐私与降噪

禁止记录：Bearer token、runtime token、Authorization header、完整环境变量、Hook `meta`、Hook 请求正文、完整主题 JSON、完整 SCENE、原始协议载荷。

- `session` 默认只记录是否存在，不记录原值。
- 配置更新只记录字段是否参与变更，token 只记录 `changes_token=true/false`。
- 用户路径不进入新增 INFO 日志；确需本机 DEBUG 诊断时也不得包含凭据。
- BLE 地址和设备名属于本机硬件诊断信息，可写本地日志，不得自动上传。
- 扫描候选、协议应答等高频信息使用 DEBUG；INFO 只保留汇总与状态变化。

## 7. 排障流程

1. 从错误附近找到 `signal_id`、`generation` 或 BLE `seq`。
2. 按 `signal_id` 检索完整业务链，确认信号是否受理、是否发生状态切换及 SCENE 是否下发。
3. 连接问题按 `generation` 检索，避免把已取消连接任务的日志误判为当前连接。
4. 协议问题按 `cmd + seq` 检索发送、重试、应答或最终超时。
5. 若只有 `scene_dispatch_completed` 但灯效不正确，再核对 `state`、当前主题以及设备是否在该时刻重连。

示例：

```bash
rg 'signal_id="sig-' ~/.ailight/logs
rg 'event="ble_command_failed"|event="scene_rejected"' ~/.ailight/logs
```

## 8. 验收标准

- 一个 Hook 信号可由同一 `signal_id` 追踪至状态仲裁和 SCENE 下发结果。
- 每次真实业务状态变化包含 `from_state`、`to_state`、`source` 与 `reason`。
- BLE 重试包含 `cmd`、`seq`、重试次数和最终结果。
- 发布日志不包含第 6 节禁止字段。
- 日志初始化 guard 在应用退出前保持存活。

## 9. 变更日志

| 版本 | 日期 | 内容 |
|---|---|---|
| V1.1 | 2026-09-12 | 增加 14 天保留期、100 MiB 目录软上限、最新文件保护、启动与周期清理以及失败开放语义。 |
| V1.0 | 2026-09-12 | 建立 Desktop/core 结构化日志字段、事件目录、信号关联链、级别、隐私及排障规范。 |

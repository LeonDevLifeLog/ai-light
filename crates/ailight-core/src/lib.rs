//! AI-Light 客户端核心逻辑（纯逻辑层，无 Tauri 依赖）
//!
//! 模块：
//! - `protocol`：V0.4 蓝牙通信协议编解码（帧层 + 命令层）
//! - `theme`：主题加载 / 校验 / SCENE 编译（theme-format V1.0）
//! - `arbiter`：状态仲裁（优先级抢占 / 最近活跃，ADR-0001 Q8）
//! - `config`：config.json 结构（ipc-contract §3）
//! - `hook_server`：L1 HTTP 接入服务（hook-api V1.0）
//! - `transport`：L3 单 writer 发送队列 + 事务状态机（协议 §3.5/§15.6）

pub mod arbiter;
pub mod ble;
pub mod config;
pub mod engine;
pub mod hook_server;
pub mod logging;
pub mod protocol;
pub mod theme;
pub mod transport;

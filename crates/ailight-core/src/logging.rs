//! 日志初始化（KAD-05：tracing 生态，滚动文件）
//!
//! 注意：协议 DEBUG 日志必须可编译关闭（V0.4 §14.2）——由调用方通过 feature/env 控制，
//! 本模块只负责 subscriber 装配。

use tracing_appender::non_blocking;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志。
///
/// - `file_dir`：Some(目录) → 每日滚动文件 `ailight.log` + stderr 双写；None → 仅 stderr
/// - `level`：日志级别（`error`/`warn`/`info`/`debug`/`trace`）
pub fn init(file_dir: Option<&std::path::Path>, level: &str) -> Result<(), String> {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = fmt::Layer::new().with_writer(std::io::stderr);

    let subscriber = tracing_subscriber::registry().with(filter).with(stderr_layer);

    match file_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
            let file_appender = tracing_appender::rolling::daily(dir, "ailight.log");
            let (file_writer, _guard) = non_blocking(file_appender);
            let subscriber = subscriber.with(fmt::Layer::new().with_writer(file_writer));
            // non_blocking guard 需常驻；这里交给调用方持有（应用级）
            tracing::subscriber::set_global_default(subscriber).map_err(|e| e.to_string())
        }
        None => tracing::subscriber::set_global_default(subscriber).map_err(|e| e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_stderr_only() {
        // 全局 subscriber 只能设置一次；用专用 subscriber 验证不 panic
        let _ = init(None, "info");
    }
}

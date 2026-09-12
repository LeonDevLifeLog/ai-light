//! 日志初始化（KAD-05：tracing 生态，滚动文件）
//!
//! 注意：协议 DEBUG 日志必须可编译关闭（V0.4 §14.2）——由调用方通过 feature/env 控制，
//! 本模块只负责 subscriber 装配。

use std::path::Path;
use std::time::{Duration, SystemTime};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

pub const LOG_RETENTION_DAYS: u64 = 14;
pub const LOG_DIRECTORY_MAX_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct CleanupReport {
    pub scanned_files: usize,
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub remaining_bytes: u64,
}

struct LogFile {
    path: std::path::PathBuf,
    modified: SystemTime,
    size: u64,
}

/// 清理 Desktop 日志。只处理 `ailight.log.*` 普通文件，且始终保护最新文件。
pub fn cleanup_retention(file_dir: &Path) -> Result<CleanupReport, String> {
    cleanup_retention_at(
        file_dir,
        SystemTime::now(),
        Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60),
        LOG_DIRECTORY_MAX_BYTES,
    )
}

fn cleanup_retention_at(
    file_dir: &Path,
    now: SystemTime,
    max_age: Duration,
    max_bytes: u64,
) -> Result<CleanupReport, String> {
    let mut files = Vec::new();
    let entries =
        std::fs::read_dir(file_dir).map_err(|error| format!("读取日志目录失败: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("读取日志目录项失败: {error}"))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.starts_with("ailight.log.") {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("读取日志文件类型失败: {error}"))?;
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取日志元数据失败: {error}"))?;
        files.push(LogFile {
            path: entry.path(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            size: metadata.len(),
        });
    }

    files.sort_by(|left, right| {
        left.modified
            .cmp(&right.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
    let protected = files.last().map(|file| file.path.clone());
    let mut report = CleanupReport {
        scanned_files: files.len(),
        remaining_bytes: files.iter().map(|file| file.size).sum(),
        ..CleanupReport::default()
    };

    for file in &files {
        if protected.as_ref() == Some(&file.path) {
            continue;
        }
        let expired = now
            .duration_since(file.modified)
            .map(|age| age > max_age)
            .unwrap_or(false);
        if expired {
            remove_log_file(file, &mut report)?;
        }
    }

    for file in &files {
        if report.remaining_bytes <= max_bytes {
            break;
        }
        if protected.as_ref() == Some(&file.path) || !file.path.exists() {
            continue;
        }
        remove_log_file(file, &mut report)?;
    }

    Ok(report)
}

fn remove_log_file(file: &LogFile, report: &mut CleanupReport) -> Result<(), String> {
    std::fs::remove_file(&file.path)
        .map_err(|error| format!("删除日志文件失败 {}: {error}", file.path.display()))?;
    report.removed_files += 1;
    report.removed_bytes = report.removed_bytes.saturating_add(file.size);
    report.remaining_bytes = report.remaining_bytes.saturating_sub(file.size);
    Ok(())
}

/// 必须由应用持有到进程退出，否则 non-blocking writer 可能来不及刷盘。
pub struct LoggingGuard {
    _file_guard: Option<WorkerGuard>,
}

/// 初始化全局日志。
///
/// - `file_dir`：Some(目录) → 每日滚动文件 `ailight.log` + stderr 双写；None → 仅 stderr
/// - `level`：日志级别（`error`/`warn`/`info`/`debug`/`trace`）
pub fn init(file_dir: Option<&std::path::Path>, level: &str) -> Result<LoggingGuard, String> {
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = fmt::Layer::new().with_writer(std::io::stderr);

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stderr_layer);

    match file_dir {
        Some(dir) => {
            std::fs::create_dir_all(dir).map_err(|e| format!("创建日志目录失败: {e}"))?;
            let file_appender = tracing_appender::rolling::daily(dir, "ailight.log");
            let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
            let subscriber = subscriber.with(fmt::Layer::new().with_writer(file_writer));
            tracing::subscriber::set_global_default(subscriber).map_err(|e| e.to_string())?;
            Ok(LoggingGuard {
                _file_guard: Some(guard),
            })
        }
        None => {
            tracing::subscriber::set_global_default(subscriber).map_err(|e| e.to_string())?;
            Ok(LoggingGuard { _file_guard: None })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ailight-logging-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn init_stderr_only() {
        // 全局 subscriber 只能设置一次；用专用 subscriber 验证不 panic
        let _guard = init(None, "info");
    }

    #[test]
    fn cleanup_uses_size_cap_and_protects_newest_log() {
        let dir = temp_log_dir("size");
        std::fs::write(dir.join("ailight.log.2026-01-01"), [0_u8; 8]).unwrap();
        std::fs::write(dir.join("ailight.log.2026-01-02"), [0_u8; 8]).unwrap();
        std::fs::write(dir.join("ailight.log.2026-01-03"), [0_u8; 8]).unwrap();

        let report = cleanup_retention_at(&dir, SystemTime::now(), Duration::MAX, 10).unwrap();

        assert_eq!(report.scanned_files, 3);
        assert_eq!(report.removed_files, 2);
        assert_eq!(report.remaining_bytes, 8);
        assert!(dir.join("ailight.log.2026-01-03").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_removes_expired_history_but_keeps_newest_log() {
        let dir = temp_log_dir("age");
        std::fs::write(dir.join("ailight.log.2026-01-01"), [0_u8; 8]).unwrap();
        std::fs::write(dir.join("ailight.log.2026-01-02"), [0_u8; 8]).unwrap();

        let report = cleanup_retention_at(
            &dir,
            SystemTime::now() + Duration::from_secs(1),
            Duration::ZERO,
            u64::MAX,
        )
        .unwrap();

        assert_eq!(report.removed_files, 1);
        assert!(!dir.join("ailight.log.2026-01-01").exists());
        assert!(dir.join("ailight.log.2026-01-02").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cleanup_ignores_unrelated_files_and_symlinks() {
        let dir = temp_log_dir("scope");
        std::fs::write(dir.join("ailight.log.2026-01-01"), [0_u8; 8]).unwrap();
        std::fs::write(dir.join("notes.txt"), [0_u8; 8]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("notes.txt"), dir.join("ailight.log.2026-01-00"))
            .unwrap();

        let report = cleanup_retention_at(&dir, SystemTime::now(), Duration::MAX, 0).unwrap();

        assert_eq!(report.scanned_files, 1);
        assert_eq!(report.removed_files, 0);
        assert!(dir.join("notes.txt").exists());
        #[cfg(unix)]
        assert!(dir.join("ailight.log.2026-01-00").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

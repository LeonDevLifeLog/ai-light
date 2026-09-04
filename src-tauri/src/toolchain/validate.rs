//! 候选验证原语（设计方案 §6.3 / §6.4 / §6.5 / §10）
//!
//! 所有子进程调用使用 executable + args 数组，不拼接 shell 字符串（§3.4）；
//! 超时 15 秒、stdout/stderr 各 64 KiB 上限、不加载候选目录中的任何 DLL/脚本/配置。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 版本探测超时（设计方案 §6.3）
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(15);
/// 安装/升级上限（npm 网络操作，远长于版本探测）
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(600);
/// 单流输出上限（设计方案 §6.3：64 KiB）
pub const OUTPUT_CAP: usize = 64 * 1024;
/// stderr 摘要回传前端的字符上限（脱敏后）
pub const STDERR_SUMMARY_CAP: usize = 500;

/// 受控执行的一次捕获结果
#[derive(Debug, Clone)]
pub struct Captured {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    /// 输出超限被截断（诊断用；测试断言截断行为）
    #[allow(dead_code)]
    pub truncated: bool,
}

impl Captured {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }

    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// stderr 摘要：截断 + 家目录脱敏（设计方案 §7 / §8.3：不得泄露完整环境）
pub fn stderr_summary(captured: &Captured, home: Option<&std::path::Path>) -> String {
    let text = captured.stderr_text();
    let text = text.trim();
    let mut summary: String = text.chars().take(STDERR_SUMMARY_CAP).collect();
    if text.chars().count() > STDERR_SUMMARY_CAP {
        summary.push('…');
    }
    super::model::sanitize_home(&summary, home)
}

fn read_capped(stream: &mut impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut truncated = false;
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => return (buf, truncated),
            Ok(n) => {
                let remaining = cap.saturating_sub(buf.len());
                if remaining == 0 {
                    // 达到上限后继续排空管道，避免子进程因 EPIPE/管道写满异常退出。
                    truncated = true;
                    continue;
                }
                let take = remaining.min(n);
                buf.extend_from_slice(&chunk[..take]);
                if take < n {
                    truncated = true;
                }
            }
            Err(_) => return (buf, true),
        }
    }
}

/// 受控执行：args 数组、超时、输出上限、stdin 关闭。
/// 调用方必须在 `spawn_blocking` 中使用（设计方案 §12.1）。
pub fn run_captured(cmd: &mut Command, timeout: Duration, cap: usize) -> std::io::Result<Captured> {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");

    let (stdout_tx, stdout_rx) = std::sync::mpsc::channel();
    let (stderr_tx, stderr_rx) = std::sync::mpsc::channel();
    let stdout_reader = std::thread::spawn(move || {
        let _ = stdout_tx.send(read_capped(&mut stdout_pipe, cap));
    });
    let stderr_reader = std::thread::spawn(move || {
        let _ = stderr_tx.send(read_capped(&mut stderr_pipe, cap));
    });

    let started = Instant::now();
    let mut status: Option<std::process::ExitStatus> = None;
    while started.elapsed() < timeout {
        match child.try_wait()? {
            Some(done) => {
                status = Some(done);
                break;
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let timed_out = status.is_none();
    if timed_out {
        let _ = child.kill();
        // 等待回收，避免僵尸进程；超时保护防止异常阻塞
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(done) = child.try_wait()? {
                status = Some(done);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let (stdout, stdout_truncated) = stdout_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or((Vec::new(), false));
    let (stderr, stderr_truncated) = stderr_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap_or((Vec::new(), false));
    // reader 线程只负责 send，join 仅回收资源
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();

    Ok(Captured {
        exit_code: status.map(|s| s.code().unwrap_or(-1)),
        stdout,
        stderr,
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
    })
}

/// 解析 `--version` 输出为 semver（兼容 `v` 前缀与换行；设计方案 §6.3）
pub fn parse_version_output(text: &str) -> Option<semver::Version> {
    let trimmed = text.trim().trim_start_matches('v').trim();
    semver::Version::parse(trimmed).ok()
}

/// Node 20 门槛（设计方案 §6.3 / §2.2：不支持 Node 20 以下）
pub fn node_meets_gate(version: &semver::Version) -> bool {
    version.major >= super::NODE_MAJOR_GATE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_detection_timeout_is_fifteen_seconds() {
        assert_eq!(VERSION_TIMEOUT, Duration::from_secs(15));
    }

    #[test]
    fn parses_version_output_with_prefix_and_noise() {
        assert_eq!(
            parse_version_output("v22.14.0\n"),
            Some(semver::Version::new(22, 14, 0))
        );
        assert_eq!(
            parse_version_output("10.9.2"),
            Some(semver::Version::new(10, 9, 2))
        );
        assert_eq!(parse_version_output("not a version"), None);
        assert_eq!(parse_version_output(""), None);
    }

    #[test]
    fn node_gate_rejects_below_20() {
        assert!(node_meets_gate(&semver::Version::new(20, 0, 0)));
        assert!(node_meets_gate(&semver::Version::new(22, 14, 0)));
        assert!(!node_meets_gate(&semver::Version::new(18, 20, 4)));
    }

    // 进程类测试依赖 POSIX 工具（sleep/sh/echo），仅在 unix 上运行；
    // Windows 下的超时/截断行为由三平台 CI 实机冒烟覆盖（设计方案 §14.4）。
    #[cfg(unix)]
    #[test]
    fn run_captured_enforces_timeout_and_caps() {
        // sleep 3s > 500ms 超时
        let mut cmd = Command::new("sleep");
        cmd.arg("3");
        let captured = run_captured(&mut cmd, Duration::from_millis(500), OUTPUT_CAP).unwrap();
        assert!(captured.timed_out);
        assert!(!captured.success());
    }

    #[cfg(unix)]
    #[test]
    fn run_captured_captures_success_output() {
        let mut cmd = Command::new("echo");
        cmd.arg("hello");
        let captured = run_captured(&mut cmd, VERSION_TIMEOUT, OUTPUT_CAP).unwrap();
        assert!(captured.success());
        assert_eq!(captured.stdout_text().trim(), "hello");
        assert!(!captured.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn run_captured_reports_nonzero_and_stderr() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo boom >&2; exit 3"]);
        let captured = run_captured(&mut cmd, VERSION_TIMEOUT, OUTPUT_CAP).unwrap();
        assert_eq!(captured.exit_code, Some(3));
        assert!(captured.stderr_text().contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn run_captured_truncates_oversized_output() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 200000 /dev/zero"]);
        let captured = run_captured(&mut cmd, VERSION_TIMEOUT, 1024).unwrap();
        assert!(captured.truncated);
        assert!(captured.stdout.len() <= 1024);
        assert!(captured.success());
    }

    #[test]
    fn stderr_summary_masks_home_and_truncates() {
        let mut captured = Captured {
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: "failed at /Users/alice/secret and more".as_bytes().to_vec(),
            timed_out: false,
            truncated: false,
        };
        let summary = stderr_summary(&captured, Some(std::path::Path::new("/Users/alice")));
        assert!(summary.contains("<HOME>"));
        assert!(!summary.contains("/Users/alice"));
        captured.stderr = vec![b'x'; 1000];
        let summary = stderr_summary(&captured, None);
        assert_eq!(summary.chars().count(), STDERR_SUMMARY_CAP + 1);
    }

    #[cfg(unix)]
    #[test]
    fn spawn_failure_surfaces_io_error() {
        let mut cmd = Command::new("definitely-not-a-real-binary-xyz");
        assert!(run_captured(&mut cmd, VERSION_TIMEOUT, OUTPUT_CAP).is_err());
    }
}

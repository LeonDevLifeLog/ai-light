//! ProcessRunner：所有安装/升级/Adapter 管理命令的受控执行（设计方案 §4 / §9 / §10）
//!
//! 1. executable + args 数组，禁止拼接 shell 字符串（§3.4 / §10.1）。
//! 2. 清理后的环境：移除 `NODE_OPTIONS` 与全部 `NPM_CONFIG_*`（§10.4 威胁评估），
//!    避免验证/运行 Adapter 时被注入模块或改写 registry 行为。
//! 3. 全部走同一份 ResolvedToolchain（§2.1 目标 4）。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::model::{ValidatedAdapter, ValidatedNpm, ValidatedNode};
use super::validate::{run_captured, Captured, INSTALL_TIMEOUT, VERSION_TIMEOUT};

/// 构造清理后的环境（继承基础环境，仅移除受污染键）
pub fn apply_clean_env(cmd: &mut Command) {
    let dirty: Vec<String> = std::env::vars_os()
        .filter_map(|(key, _)| key.to_str().map(str::to_string))
        .filter(|key| {
            key.eq_ignore_ascii_case("NODE_OPTIONS")
                || key.to_ascii_uppercase().starts_with("NPM_CONFIG_")
        })
        .collect();
    for key in dirty {
        cmd.env_remove(&key);
    }
}

fn base_command(executable: &Path) -> Command {
    let mut cmd = Command::new(executable);
    apply_clean_env(&mut cmd);
    cmd
}

/// `<node> <npm-cli.js> <args...>`；无法定位 npm-cli.js 时退回平台 launcher
/// （设计方案 §6.4：只有无法解析时才使用 launcher，args 仍为数组；
/// Windows `.cmd` 经 std 受控转义执行，不手工拼接 shell 字符串）
pub fn run_npm(node: &ValidatedNode, npm: &ValidatedNpm, args: &[&str], timeout: Duration) -> std::io::Result<Captured> {
    match &npm.cli_script {
        Some(cli) => run_captured(base_command(&node.path).arg(cli).args(args), timeout, super::validate::OUTPUT_CAP),
        None => run_captured(base_command(&npm.launcher_path()).args(args), timeout, super::validate::OUTPUT_CAP),
    }
}

/// `<node> <adapter-cli.js> <args...>`（设计方案 §6.5：统一稳定执行入口）
pub fn run_adapter(node: &ValidatedNode, adapter: &ValidatedAdapter, args: &[&str], timeout: Duration) -> std::io::Result<Captured> {
    run_captured(
        base_command(&node.path).arg(&adapter.script).args(args),
        timeout,
        super::validate::OUTPUT_CAP,
    )
}

/// `npm view <pkg> versions --json`（安装前解析兼容范围内的明确版本）
pub fn npm_view_versions(node: &ValidatedNode, npm: &ValidatedNpm, package: &str) -> std::io::Result<Captured> {
    run_npm(node, npm, &["view", package, "versions", "--json"], VERSION_TIMEOUT.max(Duration::from_secs(30)))
}

/// `npm install --global <pkg>@<version>`（设计方案 §9.1：指定明确版本）
pub fn npm_install_global(node: &ValidatedNode, npm: &ValidatedNpm, target: &str) -> std::io::Result<Captured> {
    run_npm(node, npm, &["install", "--global", target], INSTALL_TIMEOUT)
}

impl ValidatedNpm {
    /// launcher 兜底执行入口（npm-cli.js 不可用时）
    pub fn launcher_path(&self) -> PathBuf {
        self.launcher
            .clone()
            .unwrap_or_else(|| fallback_launcher(&self.prefix))
    }
}

fn fallback_launcher(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("npm.cmd")
    } else {
        prefix.join("bin/npm")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_env_removes_node_and_npm_config_keys() {
        // 验证过滤规则本身（大小写不敏感、前缀匹配）
        let keys = ["NODE_OPTIONS", "NPM_CONFIG_REGISTRY", "npm_config_prefix", "PATH", "HOME"];
        let removed: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|key| {
                key.eq_ignore_ascii_case("NODE_OPTIONS")
                    || key.to_ascii_uppercase().starts_with("NPM_CONFIG_")
            })
            .collect();
        assert_eq!(removed, vec!["NODE_OPTIONS", "NPM_CONFIG_REGISTRY", "npm_config_prefix"]);
    }

    #[test]
    fn npm_launcher_falls_back_to_prefix_layout() {
        let npm = ValidatedNpm {
            cli_script: None,
            launcher: None,
            version: semver::Version::new(10, 9, 2),
            prefix: PathBuf::from("/usr"),
            source: "test".into(),
            mixed_installation: false,
        };
        let launcher = npm.launcher_path();
        assert!(launcher.to_string_lossy().ends_with("bin/npm"));
    }
}

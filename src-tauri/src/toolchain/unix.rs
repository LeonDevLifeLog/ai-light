//! macOS / Linux 已知目录发现（设计方案 §6.2「macOS / Linux」节）
//!
//! 只读取稳定 shim/installs 目录，不启动 shell、不执行 profile、不递归扫描主目录。

use std::path::PathBuf;

use super::discovery::{self, Candidate};
use super::model::sources;

/// 已知安装目录（系统路径 / Apple Silicon Homebrew / MacPorts / Snap / Linuxbrew）
const SYSTEM_DIRS: &[&str] = &[
    "/usr/local/bin",
    "/usr/bin",
    "/opt/homebrew/bin",
    "/opt/local/bin",
    "/snap/bin",
];

pub fn node_candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    let exe = discovery::exe_name("node");

    // ---- 版本管理器激活入口（rank 20） ----
    // nvm：default alias → versions/node/<v>/bin/node
    if let Some(active) = nvm_active_node() {
        discovery::push_if_file(
            &mut out,
            active,
            sources::VERSION_MANAGER,
            discovery::rank::VERSION_MANAGER_ACTIVE,
        );
    }
    // fnm：multishell 符号链接目录
    if let Some(dir) = discovery::env_dir("FNM_MULTISHELL_PATH") {
        discovery::push_if_file(
            &mut out,
            dir.join(&exe),
            sources::VERSION_MANAGER,
            discovery::rank::VERSION_MANAGER_ACTIVE,
        );
    }
    // Volta / asdf / mise 的 shim 即当前激活入口
    for shim_dir in [volta_bin(), asdf_shims(), mise_shims()].into_iter().flatten() {
        discovery::push_if_file(
            &mut out,
            shim_dir.join(&exe),
            sources::VERSION_MANAGER,
            discovery::rank::VERSION_MANAGER_ACTIVE,
        );
    }

    // ---- 注册表/官方安装器等价位置（rank 40） ----
    for dir in SYSTEM_DIRS {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(dir).join(&exe),
            sources::COMMON_DIRECTORY,
            discovery::rank::REGISTRY,
        );
    }

    // ---- 其他版本管理器版本目录（rank 50，数量受限） ----
    for base in nvm_bases() {
        for version in discovery::version_dirs(&base.join("versions/node"), |name| {
            name.starts_with('v')
        }) {
            discovery::push_if_file(
                &mut out,
                version.join("bin").join(&exe),
                sources::VERSION_MANAGER,
                discovery::rank::FALLBACK,
            );
        }
    }
    for base in fnm_bases() {
        for version in
            discovery::version_dirs(&base.join("node-versions"), |name| name.starts_with('v'))
        {
            discovery::push_if_file(
                &mut out,
                version.join("installation/bin").join(&exe),
                sources::VERSION_MANAGER,
                discovery::rank::FALLBACK,
            );
        }
    }

    out
}

fn nvm_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(dir) = discovery::env_dir("NVM_DIR") {
        bases.push(dir);
    }
    if let Some(home) = discovery::home_dir() {
        bases.push(home.join(".nvm"));
    }
    bases.dedup();
    bases
}

/// nvm default alias 指向的激活版本（只读 alias 文本，不启动 shell）
fn nvm_active_node() -> Option<PathBuf> {
    for base in nvm_bases() {
        let alias = std::fs::read_to_string(base.join("alias/default")).ok()?;
        let version = alias.lines().next()?.trim();
        let version = version.strip_prefix('v').unwrap_or(version);
        if version.is_empty() {
            continue;
        }
        let candidate = base.join("versions/node").join(format!("v{version}")).join("bin/node");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn fnm_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(dir) = discovery::env_dir("FNM_DIR") {
        bases.push(dir);
    }
    if let Some(home) = discovery::home_dir() {
        bases.push(home.join(".local/share/fnm"));
        bases.push(home.join(".fnm"));
    }
    bases.sort();
    bases.dedup();
    bases
}

fn volta_bin() -> Option<PathBuf> {
    discovery::env_dir("VOLTA_HOME")
        .or_else(|| discovery::home_dir().map(|home| home.join(".volta")))
        .map(|volta| volta.join("bin"))
}

fn asdf_shims() -> Option<PathBuf> {
    discovery::env_dir("ASDF_DIR")
        .map(|dir| dir.join("shims"))
        .or_else(|| discovery::home_dir().map(|home| home.join(".asdf/shims")))
}

fn mise_shims() -> Option<PathBuf> {
    discovery::env_dir("MISE_DATA_DIR")
        .or_else(|| discovery::home_dir().map(|home| home.join(".local/share/mise")))
        .map(|base| base.join("shims"))
}

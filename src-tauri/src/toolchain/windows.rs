//! Windows 发现：注册表、常见安装目录、版本管理器（设计方案 §6.2「Windows」节）
//!
//! 注册表读取使用最小成熟 crate winreg（已在依赖树内，tauri→windows-registry 同款），
//! 兼容 32/64 位注册表视图；不解析 `reg query` 本地化文本（设计方案 §12.1）。

use std::path::PathBuf;

use super::discovery::{self, Candidate};
use super::model::sources;

pub fn node_candidates() -> Vec<Candidate> {
    let mut out = Vec::new();
    let exe = discovery::exe_name("node");

    // ---- 版本管理器激活入口（rank 20） ----
    // nvm-windows：当前 symlink（不假设其有效，设计方案 §6.2）
    if let Some(link) = discovery::env_dir("NVM_SYMLINK") {
        discovery::push_if_file(
            &mut out,
            link.join(&exe),
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
    // Volta：bin 下的 node shim 即当前激活版本
    for volta in volta_homes() {
        discovery::push_if_file(
            &mut out,
            volta.join("bin").join(&exe),
            sources::VERSION_MANAGER,
            discovery::rank::VERSION_MANAGER_ACTIVE,
        );
    }

    // ---- 注册表 / 官方安装器（rank 40） ----
    for path in registry_node_install_paths() {
        discovery::push_if_file(
            &mut out,
            path.join(&exe),
            sources::WINDOWS_REGISTRY,
            discovery::rank::REGISTRY,
        );
    }
    for base in [
        env_dir_join("ProgramFiles", "nodejs"),
        env_dir_join("ProgramFiles(x86)", "nodejs"),
        env_dir_join("LocalAppData", "Programs/nodejs"),
    ]
    .into_iter()
    .flatten()
    {
        discovery::push_if_file(
            &mut out,
            base.join(&exe),
            sources::COMMON_DIRECTORY,
            discovery::rank::REGISTRY,
        );
    }

    // ---- 版本管理器其他版本 / 常见目录（rank 50，数量受限） ----
    // nvm-windows：NVM_HOME 及其版本目录
    if let Some(nvm_home) = discovery::env_dir("NVM_HOME") {
        discovery::push_if_file(
            &mut out,
            nvm_home.join(&exe),
            sources::VERSION_MANAGER,
            discovery::rank::FALLBACK,
        );
        for version in discovery::version_dirs(&nvm_home, |name| name.starts_with('v')) {
            discovery::push_if_file(
                &mut out,
                version.join(&exe),
                sources::VERSION_MANAGER,
                discovery::rank::FALLBACK,
            );
        }
    }
    // fnm：%APPDATA%\fnm\node-versions\*\installation
    for base in fnm_bases() {
        for version in
            discovery::version_dirs(&base.join("node-versions"), |name| name.starts_with('v'))
        {
            discovery::push_if_file(
                &mut out,
                version.join("installation").join(&exe),
                sources::VERSION_MANAGER,
                discovery::rank::FALLBACK,
            );
        }
    }
    // Scoop：apps/nodejs*/current
    if let Some(home) = discovery::home_dir() {
        let scoop_apps = home.join("scoop/apps");
        for app in discovery::version_dirs(&scoop_apps, |name| name.starts_with("nodejs")) {
            discovery::push_if_file(
                &mut out,
                app.join("current").join(&exe),
                sources::COMMON_DIRECTORY,
                discovery::rank::FALLBACK,
            );
        }
    }
    // Chocolatey：仅检查其 shim（设计方案 §6.2：不递归扫描磁盘）
    if let Some(dir) = discovery::env_dir("ChocolateyInstall") {
        discovery::push_if_file(
            &mut out,
            dir.join("bin").join(&exe),
            sources::COMMON_DIRECTORY,
            discovery::rank::FALLBACK,
        );
    }

    out
}

/// OS 原生路径查询：`where.exe`（设计方案 §6.2 第 4 条）。
/// where.exe 位于 System32（GUI 进程始终可搜索），失败/超时返回空。
pub fn os_query_candidates(name: &str) -> Vec<Candidate> {
    let mut cmd = std::process::Command::new("where.exe");
    cmd.arg(name);
    let Ok(captured) = super::validate::run_captured(
        &mut cmd,
        super::validate::VERSION_TIMEOUT,
        super::validate::OUTPUT_CAP,
    ) else {
        return Vec::new();
    };
    if !captured.success() {
        return Vec::new();
    }
    captured
        .stdout_text()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let path = PathBuf::from(line);
            path.is_file()
                .then(|| Candidate::new(path, sources::OS_QUERY, discovery::rank::OS_QUERY))
        })
        .collect()
}

/// 注册表 InstallPath（HKLM/HKCU × 64/32 位视图 × Node.js/Nodejs 子键）
fn registry_node_install_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        use winreg::enums::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
        };
        use winreg::RegKey;
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for view in [KEY_READ | KEY_WOW64_64KEY, KEY_READ | KEY_WOW64_32KEY] {
                for subkey in ["SOFTWARE\\Node.js", "SOFTWARE\\Nodejs"] {
                    let Ok(key) = RegKey::predef(hive).open_subkey_with_flags(subkey, view) else {
                        continue;
                    };
                    if let Ok(path) = key.get_value::<String, _>("InstallPath") {
                        out.push(PathBuf::from(path));
                    }
                }
            }
        }
    }
    out
}

fn env_dir_join(env: &str, sub: &str) -> Option<PathBuf> {
    discovery::env_dir(env).map(|base| base.join(sub))
}

fn volta_homes() -> Vec<PathBuf> {
    let mut homes = Vec::new();
    if let Some(home) = discovery::env_dir("VOLTA_HOME") {
        homes.push(home);
    }
    if let Some(profile) = discovery::env_dir("USERPROFILE") {
        homes.push(profile.join(".volta"));
    }
    homes.sort();
    homes.dedup();
    homes
}

fn fnm_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(dir) = discovery::env_dir("FNM_DIR") {
        bases.push(dir);
    }
    if let Some(appdata) = discovery::env_dir("APPDATA") {
        bases.push(appdata.join("fnm"));
    }
    bases.sort();
    bases.dedup();
    bases
}

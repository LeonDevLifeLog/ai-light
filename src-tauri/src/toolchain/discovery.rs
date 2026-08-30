//! 跨平台候选发现（设计方案 §6.2 / §6.6）
//!
//! 发现阶段只生成候选，不做可用性结论（§3.2 发现与验证分离）；
//! 去重采用 Windows 大小写不敏感、其他平台大小写敏感的规范化绝对路径；
//! 版本目录扫描设数量上限，禁止递归扫描用户主目录。

use std::path::{Path, PathBuf};

use super::model::sources;

/// 每个版本管理器目录的扫描上限（设计方案 §6.2）
pub const MAX_VERSION_DIRS: usize = 32;

/// 候选排序档位（设计方案 §6.6）：用户 override > 上次已选 >
/// 当前激活的版本管理器入口 > 进程 PATH / OS 查询 > 注册表/官方安装 >
/// 其他版本管理器版本与常见目录
pub mod rank {
    pub const OVERRIDE: u32 = 0;
    pub const PREVIOUS: u32 = 10;
    pub const VERSION_MANAGER_ACTIVE: u32 = 20;
    pub const SAME_FAMILY: u32 = 20;
    pub const PROCESS_PATH: u32 = 30;
    /// OS_QUERY 仅供 windows.rs（where.exe）使用
    #[allow(dead_code)]
    pub const OS_QUERY: u32 = 30;
    pub const REGISTRY: u32 = 40;
    pub const FALLBACK: u32 = 50;
}

/// 一个待验证候选
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub source: &'static str,
    pub rank: u32,
}

impl Candidate {
    pub fn new(path: PathBuf, source: &'static str, rank: u32) -> Self {
        Self { path, source, rank }
    }
}

/// Windows 上可执行文件带 `.exe` 后缀
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// 当前进程 `PATH` 中的目录（不去重，供各发现器复用）
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|value| {
            std::env::split_paths(&value)
                .filter(|dir| !dir.as_os_str().is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// 读取环境变量目录（不存在/为空 → None）
pub fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 家目录（未解析成功 → None）
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// 在 PATH 目录中查找命令名（GUI 进程继承的 PATH，设计方案 §6.2 第 3 条）
pub fn find_on_path(name: &str) -> Vec<Candidate> {
    let exe = exe_name(name);
    path_dirs()
        .into_iter()
        .filter_map(|dir| {
            let path = dir.join(&exe);
            path.is_file()
                .then(|| Candidate::new(path, sources::PROCESS_PATH, rank::PROCESS_PATH))
        })
        .collect()
}

/// 若路径是普通文件则加入候选
pub fn push_if_file(out: &mut Vec<Candidate>, path: PathBuf, source: &'static str, rank: u32) {
    if path.is_file() {
        out.push(Candidate::new(path, source, rank));
    }
}

/// 读取 base 下符合版本目录格式的直接子目录（数量受限、不递归，设计方案 §6.2）
pub fn version_dirs(base: &Path, name_filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name_filter(name))
        })
        .collect();
    // 确定性顺序：按目录名排序后截断
    dirs.sort();
    dirs.truncate(MAX_VERSION_DIRS);
    dirs
}

/// 规范化去重键：存在则 canonicalize（解析 symlink/junction，设计方案 §10.8），
/// 否则取绝对路径；Windows 大小写不敏感。
pub fn dedup_key(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let canonical = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    if cfg!(windows) {
        let lowered = canonical.to_string_lossy().to_lowercase();
        PathBuf::from(lowered)
    } else {
        canonical
    }
}

/// 候选先去重再验证（设计方案 §6.2）；保留排序靠前的首个出现。
pub fn dedup(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by_key(|c| (c.rank, c.path.clone()));
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let key = dedup_key(&candidate.path);
        if !seen.contains(&key) {
            seen.push(key);
            out.push(candidate);
        }
    }
    out
}

/// npm 全局安装的包内脚本绝对路径（设计方案 §6.5 第 3 条）
pub fn adapter_script_in_prefix(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix
            .join("node_modules")
            .join("@ai-light")
            .join("adapter")
            .join("dist")
            .join("cli.js")
    } else {
        prefix.join("lib/node_modules/@ai-light/adapter/dist/cli.js")
    }
}

/// npm 全局 bin 中的 Adapter launcher（设计方案 §6.5 第 4 条）
pub fn adapter_launcher_in_prefix(prefix: &Path) -> PathBuf {
    if cfg!(windows) {
        prefix.join("ailight-adapter.cmd")
    } else {
        prefix.join("bin").join("ailight-adapter")
    }
}

/// 由 Node 安装树推导同族 npm-cli.js（设计方案 §6.4 优先路径）
pub fn npm_cli_in_node_tree(node_exe: &Path) -> Option<PathBuf> {
    let node_dir = node_exe.parent()?;
    let cli = if cfg!(windows) {
        node_dir.join("node_modules/npm/bin/npm-cli.js")
    } else {
        // /usr/bin/node → /usr/lib/...；/opt/homebrew/bin/node → /opt/homebrew/lib/...
        node_dir
            .parent()?
            .join("lib/node_modules/npm/bin/npm-cli.js")
    };
    cli.is_file().then_some(cli)
}

/// Windows npm `.cmd` shim 解析：提取其中的 `.js` 目标并把 `%~dp0`/`%dp0%` 替换为
/// shim 所在目录（设计方案 §6.4：优先用选定 Node 运行 npm-cli.js）。
/// 纯字符串解析、无 shell 参与；解析结果随后仍要经实际执行验证。
pub fn parse_cmd_shim_target(content: &str, shim_dir: &Path) -> Option<PathBuf> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("REM") || trimmed.starts_with("rem") || trimmed.starts_with("::") {
            continue;
        }
        for token in tokenize_cmd_line(trimmed) {
            // token 可能保留包裹引号（tokenize 不剥离），比较前先去掉
            let clean = token.trim_matches('"');
            if !clean.to_ascii_lowercase().ends_with(".js") {
                continue;
            }
            let expanded = clean
                .replace("%~dp0", &format!("{}", shim_dir.display()))
                .replace("%dp0%", &format!("{}", shim_dir.display()))
                .replace('%', "");
            let path = PathBuf::from(expanded.trim_matches('"'));
            return Some(path);
        }
    }
    None
}

/// 拆分 cmd 行 token（处理双引号包裹）
fn tokenize_cmd_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_dirs_filters_and_caps() {
        let base = std::env::temp_dir().join(format!(
            "ailight-disc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        for name in ["v20.11.0", "v22.14.0", "v18.0.0", "not-a-version", "cache"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
        }
        std::fs::write(base.join("v22.14.0.txt"), "").unwrap();
        let dirs = version_dirs(&base, |name| name.starts_with('v'));
        let names: Vec<String> = dirs
            .iter()
            .map(|d| d.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["v18.0.0", "v20.11.0", "v22.14.0"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn version_dirs_truncates_to_limit() {
        let base = std::env::temp_dir().join(format!(
            "ailight-disc-cap-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&base).unwrap();
        for index in 0..(MAX_VERSION_DIRS + 10) {
            std::fs::create_dir_all(base.join(format!("v9.{index}.0"))).unwrap();
        }
        assert_eq!(
            version_dirs(&base, |name| name.starts_with('v')).len(),
            MAX_VERSION_DIRS
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn dedup_keeps_higher_rank_and_is_deterministic() {
        let dup_a = Candidate::new(
            PathBuf::from("/opt/node"),
            sources::PROCESS_PATH,
            rank::PROCESS_PATH,
        );
        let dup_b = Candidate::new(
            PathBuf::from("/opt/node"),
            sources::COMMON_DIRECTORY,
            rank::REGISTRY,
        );
        let other = Candidate::new(
            PathBuf::from("/usr/bin/node"),
            sources::COMMON_DIRECTORY,
            rank::FALLBACK,
        );
        let out = dedup(vec![dup_b, dup_a, other]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].path, PathBuf::from("/opt/node"));
        assert_eq!(out[0].source, sources::PROCESS_PATH);
        assert_eq!(out[1].path, PathBuf::from("/usr/bin/node"));
    }

    #[test]
    fn parse_cmd_shim_extracts_js_target() {
        let content = "@ECHO off\r\nGOTO start\r\n:start\r\nSETLOCAL\r\n\
            IF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n)\
            \r\n\"%_prog%\" \"%dp0%\\node_modules\\npm\\bin\\npm-cli.js\" %*\r\n";
        let target =
            parse_cmd_shim_target(content, Path::new("C:\\Program Files\\nodejs")).unwrap();
        assert_eq!(
            target,
            PathBuf::from("C:\\Program Files\\nodejs\\node_modules\\npm\\bin\\npm-cli.js")
        );
    }

    #[test]
    fn parse_cmd_shim_ignores_comments_and_non_js() {
        assert_eq!(
            parse_cmd_shim_target("REM nothing here", Path::new("/x")),
            None
        );
        assert_eq!(
            parse_cmd_shim_target("node \"%~dp0\\bin\\run.js\" %*", Path::new("C:\\tools")),
            Some(PathBuf::from("C:\\tools\\bin\\run.js"))
        );
    }

    #[test]
    fn adapter_prefix_paths_follow_platform_layout() {
        let prefix = Path::new("/usr");
        let script = adapter_script_in_prefix(prefix);
        assert!(script
            .to_string_lossy()
            .contains("node_modules/@ai-light/adapter/dist/cli.js"));
        let launcher = adapter_launcher_in_prefix(prefix);
        assert!(launcher.to_string_lossy().ends_with("bin/ailight-adapter"));
    }
}

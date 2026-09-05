//! 工具链数据模型（设计方案 §5 / §6.6 / §7）
//!
//! `ToolchainDocument` 是 `toolchain.json` 的持久化 schema（overrides 是用户意图，
//! selected 是可再生的缓存）；`ToolchainStatus` 是 IPC 响应（设计方案 §7 响应示例）。

use semver::VersionReq;
use serde::{Deserialize, Serialize};

/// 持久化 schema 版本（设计方案 §5.2）
pub const TOOLCHAIN_SCHEMA_VERSION: u8 = 1;

/// Adapter npm 包名
pub const ADAPTER_PACKAGE: &str = "@ai-light/adapter";

/// Desktop 兼容的 Adapter 版本范围（设计方案 §17.3：Desktop 兼容范围 + 明确版本，
/// 不得无条件安装 latest）。Adapter Hook Protocol V1 → 0.x 系列。
/// npm 目标表达式（node-semver 空格分隔语法）
pub const ADAPTER_COMPAT_RANGE: &str = ">=0.1.6 <0.2.0";

/// Rust semver crate 形式的兼容范围（逗号分隔；node-semver 与 Rust 语法差异的适配）
pub fn adapter_compat_req() -> VersionReq {
    VersionReq::parse(&ADAPTER_COMPAT_RANGE.replace(' ', ", "))
        .expect("ADAPTER_COMPAT_RANGE 必须是合法 semver 范围")
}

/// 解析模式（设计方案 §5.2）：manual 允许只覆盖部分字段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolchainMode {
    Auto,
    Manual,
}

impl Default for ToolchainMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl ToolchainMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

/// 单个工具的用户 override（设计方案 §5.2）。`None` = 未覆盖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolchainOverrides {
    pub node: Option<String>,
    pub npm: Option<String>,
    pub adapter: Option<String>,
}

impl ToolchainOverrides {
    pub fn get(&self, kind: ToolKind) -> Option<&String> {
        match kind {
            ToolKind::Node => self.node.as_ref(),
            ToolKind::Npm => self.npm.as_ref(),
            ToolKind::Adapter => self.adapter.as_ref(),
        }
    }

    pub fn set(&mut self, kind: ToolKind, path: Option<String>) {
        let slot = match kind {
            ToolKind::Node => &mut self.node,
            ToolKind::Npm => &mut self.npm,
            ToolKind::Adapter => &mut self.adapter,
        };
        *slot = path;
    }

    pub fn is_empty(&self) -> bool {
        self.node.is_none() && self.npm.is_none() && self.adapter.is_none()
    }
}

/// `selected.node/npm` 缓存条目（设计方案 §5.2）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedTool {
    pub path: String,
    pub version: String,
    pub source: String,
}

/// `selected.adapter` 缓存条目：launcher 仅为诊断保留，稳定执行入口是 scriptPath
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAdapter {
    pub launcher_path: Option<String>,
    pub script_path: String,
    pub version: String,
    pub source: String,
}

/// `selected` 完整缓存（设计方案 §5.2）
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectedToolchain {
    pub node: Option<SelectedTool>,
    pub npm: Option<SelectedTool>,
    pub adapter: Option<SelectedAdapter>,
}

/// toolchain.json 文档（设计方案 §5.2）。未知字段在加载时忽略（serde default + deny 无关字段不启用）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolchainDocument {
    pub version: u8,
    pub mode: ToolchainMode,
    pub overrides: ToolchainOverrides,
    pub selected: SelectedToolchain,
    pub last_resolved_at: Option<String>,
}

impl ToolchainDocument {
    pub fn new() -> Self {
        Self {
            version: TOOLCHAIN_SCHEMA_VERSION,
            ..Self::default()
        }
    }
}

/// 工具类别（IPC `kind` 参数）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Node,
    Npm,
    Adapter,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Npm => "npm",
            Self::Adapter => "adapter",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "node" => Some(Self::Node),
            "npm" => Some(Self::Npm),
            "adapter" => Some(Self::Adapter),
            _ => None,
        }
    }
}

/// 解析结果状态全集（设计方案 §6.6）
pub mod states {
    pub const CHECKING: &str = "checking";
    pub const READY: &str = "ready";
    pub const NODE_MISSING: &str = "node_missing";
    pub const NODE_INCOMPATIBLE: &str = "node_incompatible";
    pub const NPM_MISSING: &str = "npm_missing";
    pub const ADAPTER_MISSING: &str = "adapter_missing";
    pub const ADAPTER_INCOMPATIBLE: &str = "adapter_incompatible";
    pub const INVALID_OVERRIDE: &str = "invalid_override";
    pub const AMBIGUOUS: &str = "ambiguous";
    pub const PERMISSION_DENIED: &str = "permission_denied";
    pub const STORE_INVALID: &str = "store_invalid";
}

/// 候选来源标识（设计方案 §5.2 source / §15 可观测性）
pub mod sources {
    pub const OVERRIDE: &str = "override";
    pub const PREVIOUS_SELECTED: &str = "previousSelected";
    pub const PROCESS_PATH: &str = "processPath";
    /// OS_QUERY / WINDOWS_REGISTRY 仅供 windows.rs 使用
    #[allow(dead_code)]
    pub const OS_QUERY: &str = "osQuery";
    #[allow(dead_code)]
    pub const WINDOWS_REGISTRY: &str = "windowsRegistry";
    pub const SIBLING_OF_NODE: &str = "siblingOfNode";
    pub const NPM_GLOBAL_PREFIX: &str = "npmGlobalPrefix";
    pub const COMMON_DIRECTORY: &str = "commonDirectory";
    pub const VERSION_MANAGER: &str = "versionManager";
}

/// 单个工具的 IPC 状态条目（设计方案 §7）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatusEntry {
    pub state: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub overridden: bool,
}

/// Adapter 状态条目：额外暴露 launcherPath（诊断用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterStatusEntry {
    pub state: String,
    pub path: Option<String>,
    pub launcher_path: Option<String>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub overridden: bool,
}

/// 可解释的诊断条目（设计方案 §7 / §8.3）。message 面向用户，可携带脱敏后的 detail。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainIssue {
    pub code: String,
    pub message: String,
    pub tool: Option<String>,
    pub recovery: Option<String>,
}

/// `get_toolchain_status` 响应（设计方案 §7）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainStatus {
    pub state: String,
    pub mode: ToolchainMode,
    pub summary: String,
    pub node: Option<ToolStatusEntry>,
    pub npm: Option<ToolStatusEntry>,
    pub adapter: Option<AdapterStatusEntry>,
    pub issues: Vec<ToolchainIssue>,
    pub checked_at: String,
}

/// 用户主动检查 Adapter 更新的结果。目标始终限制在 Desktop 兼容范围内。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdapterUpdateInfo {
    pub current_version: String,
    pub target_version: String,
    pub update_available: bool,
    pub compatible: bool,
}

/// 验证后的 Node 安装（同族推导的锚点）
#[derive(Debug, Clone)]
pub struct ValidatedNode {
    pub path: std::path::PathBuf,
    pub version: semver::Version,
    pub source: String,
}

/// 验证后的 npm：统一以「选定 Node + npm-cli.js」为稳定执行入口（设计方案 §6.4）
#[derive(Debug, Clone)]
pub struct ValidatedNpm {
    /// npm-cli.js 绝对路径（用选定 Node 执行）；`None` = 无法解析，退回 launcher
    pub cli_script: Option<std::path::PathBuf>,
    /// 平台 launcher（npm / npm.cmd），仅作 npm-cli.js 不可用时的兜底
    pub launcher: Option<std::path::PathBuf>,
    pub version: semver::Version,
    pub prefix: std::path::PathBuf,
    pub source: String,
    /// npm CLI 与选定 Node 不属于同一安装树（设计方案 §6.4 mixedInstallation）
    pub mixed_installation: bool,
}

/// 验证后的 Adapter：稳定执行入口是 `node + cli.js`
#[derive(Debug, Clone)]
pub struct ValidatedAdapter {
    pub script: std::path::PathBuf,
    pub launcher: Option<std::path::PathBuf>,
    pub version: semver::Version,
    pub source: String,
}

/// 一次成功解析的完整工具链（设计方案 §2.1 目标 4：所有操作使用同一份解析结果）
#[derive(Debug, Clone)]
pub struct ResolvedToolchain {
    pub node: ValidatedNode,
    pub npm: ValidatedNpm,
    pub adapter: Option<ValidatedAdapter>,
}

/// 把用户家目录前缀脱敏为 `<HOME>`（设计方案 §8.3 / §15）
pub fn sanitize_home(text: &str, home: Option<&std::path::Path>) -> String {
    let Some(home) = home else {
        return text.to_string();
    };
    let Some(home_str) = home.to_str() else {
        return text.to_string();
    };
    if home_str.is_empty() {
        return text.to_string();
    }
    text.replace(home_str, "<HOME>")
}

/// 组装一行摘要（设计方案 §8.1：Node.js 22.14.0 · npm 10.9.2 · Adapter 0.4.2）
pub fn build_summary(
    node: Option<&ValidatedNode>,
    npm: Option<&ValidatedNpm>,
    adapter: Option<&ValidatedAdapter>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    match node {
        Some(n) => parts.push(format!("Node.js {}", n.version)),
        None => parts.push("Node.js 未就绪".to_string()),
    }
    match npm {
        Some(n) => parts.push(format!("npm {}", n.version)),
        None => parts.push("npm 未就绪".to_string()),
    }
    match adapter {
        Some(a) => parts.push(format!("Adapter {}", a.version)),
        None => parts.push("Adapter 未安装".to_string()),
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrations_require_adapter_0_1_6_or_newer() {
        let requirement = adapter_compat_req();
        assert!(!requirement.matches(&semver::Version::new(0, 1, 5)));
        assert!(requirement.matches(&semver::Version::new(0, 1, 6)));
    }

    #[test]
    fn document_round_trips_and_defaults_version() {
        let mut doc = ToolchainDocument::new();
        doc.overrides.node = Some("C:\\Tools\\node.exe".into());
        let json = serde_json::to_string(&doc).unwrap();
        let back: ToolchainDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(back.version, TOOLCHAIN_SCHEMA_VERSION);
        assert_eq!(back.overrides.node.as_deref(), Some("C:\\Tools\\node.exe"));
        assert_eq!(back.mode, ToolchainMode::Auto);
    }

    #[test]
    fn document_ignores_unknown_fields_and_bad_schema_version() {
        // 未知字段忽略；version 不支持时仍解析成功，由 store 层决定恢复策略
        let json = r#"{"version":99,"futureField":true,"mode":"manual","overrides":{"node":"/x"}}"#;
        let doc: ToolchainDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.mode, ToolchainMode::Manual);
        assert_eq!(doc.overrides.node.as_deref(), Some("/x"));
    }

    #[test]
    fn sanitize_home_masks_home_prefix_only() {
        let home = std::path::Path::new("/Users/alice");
        assert_eq!(
            sanitize_home("found at /Users/alice/.nvm/node", Some(home)),
            "found at <HOME>/.nvm/node"
        );
        assert_eq!(sanitize_home("/opt/node", Some(home)), "/opt/node");
    }

    #[test]
    fn summary_lists_each_tool() {
        let node = ValidatedNode {
            path: "/usr/bin/node".into(),
            version: semver::Version::parse("22.14.0").unwrap(),
            source: "commonDirectory".into(),
        };
        let npm = ValidatedNpm {
            cli_script: Some("/usr/lib/node_modules/npm/bin/npm-cli.js".into()),
            launcher: None,
            version: semver::Version::parse("10.9.2").unwrap(),
            prefix: "/usr".into(),
            source: "siblingOfNode".into(),
            mixed_installation: false,
        };
        let adapter = ValidatedAdapter {
            script: "/usr/lib/node_modules/@ai-light/adapter/dist/cli.js".into(),
            launcher: None,
            version: semver::Version::parse("0.4.2").unwrap(),
            source: "npmGlobalPrefix".into(),
        };
        assert_eq!(
            build_summary(Some(&node), Some(&npm), Some(&adapter)),
            "Node.js 22.14.0 · npm 10.9.2 · Adapter 0.4.2"
        );
        assert_eq!(
            build_summary(None, None, None),
            "Node.js 未就绪 · npm 未就绪 · Adapter 未安装"
        );
    }

    #[test]
    fn tool_kind_parses_ipc_values() {
        assert_eq!(ToolKind::parse("node"), Some(ToolKind::Node));
        assert_eq!(ToolKind::parse("npm"), Some(ToolKind::Npm));
        assert_eq!(ToolKind::parse("adapter"), Some(ToolKind::Adapter));
        assert_eq!(ToolKind::parse("shell"), None);
    }
}

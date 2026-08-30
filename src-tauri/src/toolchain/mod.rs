//! ToolchainService（设计方案 §4 总体架构）
//!
//! 位于 Tauri 壳层而不是 `ailight-core`：处理 OS 可执行文件、用户目录与进程启动，
//! 属于平台集成；核心状态仲裁、主题与 BLE 不依赖 Node/npm。
//!
//! - CandidateDiscovery：生成 Node/npm/Adapter 候选（discovery / unix / windows）
//! - CandidateValidator：受控执行、版本与关联性验证（validate）
//! - ToolchainResolver：排序并选择同族工具链（本模块 resolve）
//! - ToolchainStore：toolchain.json 原子读写（store）
//! - ProcessRunner：所有安装/升级/Adapter 管理命令（runner）

pub mod discovery;
pub mod model;
pub mod runner;
pub mod store;
pub mod validate;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use model::states;
use model::sources;
use model::{
    build_summary, sanitize_home, AdapterStatusEntry, ResolvedToolchain, SelectedAdapter,
    SelectedTool, SelectedToolchain, ToolKind, ToolchainDocument, ToolchainIssue, ToolchainMode,
    ToolchainOverrides, ToolchainStatus, ToolStatusEntry, ValidatedAdapter, ValidatedNpm,
    ValidatedNode, ADAPTER_COMPAT_RANGE, ADAPTER_PACKAGE,
};

/// Node 最低主版本（设计方案 §2.2：不支持 Node 20 以下）
pub const NODE_MAJOR_GATE: u64 = 20;

/// 服务级错误（commands 层负责映射为 AppError，设计方案 §12.1）
#[derive(Debug)]
pub enum ToolchainError {
    InvalidOverride {
        kind: ToolKind,
        path: String,
        reason: String,
    },
    Resolution(Box<ToolchainStatus>),
    Io(String),
}

pub struct ToolchainService {
    inner: tokio::sync::Mutex<ServiceState>,
}

struct ServiceState {
    doc: ToolchainDocument,
    warning: Option<String>,
    cache: Option<Cached>,
}

struct Cached {
    at_ms: u64,
    status: ToolchainStatus,
    resolved: Option<Arc<ResolvedToolchain>>,
    fingerprints: Vec<(PathBuf, u64, u64)>,
}

/// 一次解析的产出（status + 需要回写的 selected 缓存）
struct ResolveOutcome {
    status: ToolchainStatus,
    resolved: Option<ResolvedToolchain>,
    persist_doc: Option<ToolchainDocument>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ToolchainService {
    /// 加载持久化文档（首次升级无 toolchain.json → 默认文档，设计方案 §13.1）
    pub fn new() -> Self {
        let (doc, warning) = store::load().unwrap_or_else(|error| {
            tracing::warn!("toolchain.json 读取失败，按默认配置继续: {error}");
            (ToolchainDocument::new(), Some(error))
        });
        Self {
            inner: tokio::sync::Mutex::new(ServiceState {
                doc,
                warning,
                cache: None,
            }),
        }
    }

    /// 查询状态。`force=false` 可用进程内缓存（内容信号失效，设计方案 §11）；
    /// `force=true` 强制复验，并合并等待期间的并发探测（§6.1）。
    pub async fn status(&self, force: bool) -> ToolchainStatus {
        let wait_started = now_ms();
        let mut state = self.inner.lock().await;
        if let Some(cache) = &state.cache {
            if !force && fingerprints_match(cache) {
                return cache.status.clone();
            }
            if force && cache.at_ms >= wait_started {
                return cache.status.clone();
            }
        }
        let doc = state.doc.clone();
        let warning = state.warning.clone();
        let outcome =
            tauri::async_runtime::spawn_blocking(move || resolve(&doc, warning.as_deref()))
                .await
                .unwrap_or_else(|error| {
                    tracing::error!("工具链解析任务失败: {error}");
                    emergency_outcome(&error.to_string())
                });
        if let Some(new_doc) = outcome.persist_doc {
            match store::save(&new_doc) {
                Ok(()) => state.doc = new_doc,
                Err(error) => tracing::warn!("toolchain.json 写入失败（保留内存态）: {error}"),
            }
        }
        let fingerprints = fingerprints_of(outcome.resolved.as_ref());
        let status = outcome.status.clone();
        state.cache = Some(Cached {
            at_ms: now_ms(),
            status,
            resolved: outcome.resolved.map(Arc::new),
            fingerprints,
        });
        outcome.status
    }

    /// 写操作前的强制复验（设计方案 §11：任何会写第三方配置/安装包的动作都强制验证）
    pub async fn resolved_for_write(
        &self,
        require_adapter: bool,
    ) -> Result<Arc<ResolvedToolchain>, ToolchainError> {
        let status = self.status(true).await;
        let resolved = self
            .inner
            .lock()
            .await
            .cache
            .as_ref()
            .and_then(|cache| cache.resolved.clone());
        let Some(chain) = resolved else {
            return Err(ToolchainError::Resolution(Box::new(status)));
        };
        if require_adapter && chain.adapter.is_none() {
            return Err(ToolchainError::Resolution(Box::new(status)));
        }
        Ok(chain)
    }

    /// 设置用户 override（设计方案 §5.2 / §7：返回字段级验证错误）。
    /// 路径失效时不静默删除：持久化为意图，状态标记 invalid_override。
    pub async fn set_overrides(
        &self,
        patch: ToolchainOverrides,
    ) -> Result<ToolchainStatus, ToolchainError> {
        let mut provided: Vec<(ToolKind, String)> = Vec::new();
        if let Some(path) = &patch.node {
            provided.push((ToolKind::Node, path.clone()));
        }
        if let Some(path) = &patch.npm {
            provided.push((ToolKind::Npm, path.clone()));
        }
        if let Some(path) = &patch.adapter {
            provided.push((ToolKind::Adapter, path.clone()));
        }
        for (kind, path) in &provided {
            if let Some(reason) = check_path_shape(Path::new(path)) {
                return Err(ToolchainError::InvalidOverride {
                    kind: *kind,
                    path: path.clone(),
                    reason,
                });
            }
        }

        // 先落盘再改内存；保存失败则不污染内存态
        let new_doc = {
            let state = self.inner.lock().await;
            let mut doc = state.doc.clone();
            for (kind, path) in &provided {
                doc.overrides.set(*kind, Some(path.clone()));
            }
            if !doc.overrides.is_empty() {
                doc.mode = ToolchainMode::Manual;
            }
            doc
        };
        store::save(&new_doc).map_err(ToolchainError::Io)?;
        {
            let mut state = self.inner.lock().await;
            state.doc = new_doc;
            state.warning = None;
            state.cache = None;
        }
        let status = self.status(true).await;
        // 字段级验证：本次设置的 override 未通过验证 → 返回错误（意图已保留，UI 展示）
        for (kind, path) in &provided {
            if tool_entry_state(&status, *kind).as_deref() != Some(states::READY) {
                let reason = status
                    .issues
                    .iter()
                    .find(|issue| issue.tool.as_deref() == Some(kind.as_str()))
                    .map(|issue| issue.message.clone())
                    .unwrap_or_else(|| "验证失败".to_string());
                return Err(ToolchainError::InvalidOverride {
                    kind: *kind,
                    path: path.clone(),
                    reason,
                });
            }
        }
        Ok(status)
    }

    /// 恢复自动检测（设计方案 §8.1「恢复自动检测」）
    pub async fn reset_overrides(&self) -> Result<ToolchainStatus, ToolchainError> {
        let new_doc = {
            let state = self.inner.lock().await;
            let mut doc = state.doc.clone();
            doc.overrides = ToolchainOverrides::default();
            doc.mode = ToolchainMode::Auto;
            doc.selected = SelectedToolchain::default();
            doc
        };
        store::save(&new_doc).map_err(ToolchainError::Io)?;
        {
            let mut state = self.inner.lock().await;
            state.doc = new_doc;
            state.warning = None;
            state.cache = None;
        }
        Ok(self.status(true).await)
    }

    /// 用户在原生文件选择器选定路径后：立即验证 → 通过才持久化为 override
    /// （设计方案 §7 / §8.2.7）
    pub async fn select_executable(
        &self,
        kind: ToolKind,
        picked: PathBuf,
    ) -> Result<ToolchainStatus, ToolchainError> {
        let path =
            std::fs::canonicalize(&picked).map_err(|error| ToolchainError::InvalidOverride {
                kind,
                path: picked.display().to_string(),
                reason: format!("无法解析路径: {error}"),
            })?;
        let path_text = path.display().to_string();
        if let Some(reason) = self.validate_candidate(kind, path).await? {
            return Err(ToolchainError::InvalidOverride {
                kind,
                path: path_text,
                reason,
            });
        }
        let new_doc = {
            let state = self.inner.lock().await;
            let mut doc = state.doc.clone();
            doc.overrides
                .set(kind, Some(path_text.clone()));
            doc.mode = ToolchainMode::Manual;
            doc
        };
        store::save(&new_doc).map_err(ToolchainError::Io)?;
        {
            let mut state = self.inner.lock().await;
            state.doc = new_doc;
            state.warning = None;
            state.cache = None;
        }
        Ok(self.status(true).await)
    }

    /// 未做任何修改时返回当前状态（文件选择取消不改变现有配置，设计方案 §8.2）
    pub async fn current_status(&self) -> ToolchainStatus {
        self.status(true).await
    }

    /// 安装 Adapter（设计方案 §9.1：node + npm-cli + 明确版本；§17.3 不装 latest）
    pub async fn install_adapter(&self) -> Result<Arc<ResolvedToolchain>, ToolchainError> {
        let chain = self.resolved_for_write(false).await?;
        let target = npm_install_target(&chain).map_err(ToolchainError::Io)?;
        tracing::info!(%target, "install.adapter");
        let package = format!("{ADAPTER_PACKAGE}@{target}");
        let package_in_task = package.clone();
        let node = chain.node.clone();
        let npm = chain.npm.clone();
        let captured = tauri::async_runtime::spawn_blocking(move || {
            runner::npm_install_global(&node, &npm, &package_in_task)
        })
            .await
            .map_err(|error| ToolchainError::Io(error.to_string()))?
            .map_err(|error| ToolchainError::Io(format!("npm 无法启动: {error}")))?;
        if !captured.success() {
            let home = dirs::home_dir();
            return Err(ToolchainError::Io(format!(
                "npm install --global {package} 失败: {}",
                validate::stderr_summary(&captured, home.as_deref())
            )));
        }
        // 安装后重新发现 Adapter 并验证（设计方案 §9.1）
        self.resolved_for_write(true).await
    }

    /// 对单个候选做立即验证（手动选择，设计方案 §8.2.7）
    async fn validate_candidate(
        &self,
        kind: ToolKind,
        path: PathBuf,
    ) -> Result<Option<String>, ToolchainError> {
        // npm/adapter 的验证以选定 Node 为执行锚点；Node 未就绪时先给出根因
        let node = if kind == ToolKind::Node {
            None
        } else {
            let chain = self
                .resolved_for_write(false)
                .await
                .map_err(|error| match error {
                    ToolchainError::Resolution(status) => ToolchainError::InvalidOverride {
                        kind,
                        path: path.display().to_string(),
                        reason: format!("需要先就绪 Node.js（{}）", status.summary),
                    },
                    other => other,
                })?;
            Some(chain.node.clone())
        };
        tauri::async_runtime::spawn_blocking(move || {
            validate_candidate_blocking(kind, &path, node.as_ref())
        })
        .await
        .map_err(|error| ToolchainError::Io(error.to_string()))
    }
}

impl Default for ToolchainService {
    fn default() -> Self {
        Self::new()
    }
}

fn tool_entry_state(status: &ToolchainStatus, kind: ToolKind) -> Option<String> {
    match kind {
        ToolKind::Node => status.node.as_ref().map(|entry| entry.state.clone()),
        ToolKind::Npm => status.npm.as_ref().map(|entry| entry.state.clone()),
        ToolKind::Adapter => status.adapter.as_ref().map(|entry| entry.state.clone()),
    }
}

/// 路径形态检查（不做语义验证，设计方案 §10.3）
fn check_path_shape(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some("路径不是绝对路径".to_string());
    }
    if !path.is_file() {
        return Some("文件不存在".to_string());
    }
    None
}

fn validate_candidate_blocking(
    kind: ToolKind,
    path: &Path,
    node: Option<&ValidatedNode>,
) -> Option<String> {
    match kind {
        ToolKind::Node => match validate_node_candidate(path) {
            Ok(version) => {
                if validate::node_meets_gate(&version) {
                    None
                } else {
                    Some(format!("Node.js {version} 低于最低要求 20（设计方案 §2.2）"))
                }
            }
            Err(reason) => Some(reason),
        },
        ToolKind::Npm => validate_npm_candidate(node?, path).err(),
        ToolKind::Adapter => validate_adapter_script(node?, path).err(),
    }
}

// ---------- 候选收集 ----------

fn collect_node_candidates(doc: &ToolchainDocument) -> Vec<discovery::Candidate> {
    let mut out = Vec::new();
    if let Some(override_path) = &doc.overrides.node {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(override_path),
            sources::OVERRIDE,
            discovery::rank::OVERRIDE,
        );
    }
    if let Some(previous) = &doc.selected.node {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(&previous.path),
            sources::PREVIOUS_SELECTED,
            discovery::rank::PREVIOUS,
        );
    }
    #[cfg(unix)]
    out.extend(unix::node_candidates());
    #[cfg(windows)]
    out.extend(windows::node_candidates());
    out.extend(discovery::find_on_path("node"));
    #[cfg(windows)]
    out.extend(windows::os_query_candidates("node"));
    discovery::dedup(out)
}

fn collect_npm_candidates(
    doc: &ToolchainDocument,
    node: &ValidatedNode,
) -> Vec<discovery::Candidate> {
    let mut out = Vec::new();
    if let Some(override_path) = &doc.overrides.npm {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(override_path),
            sources::OVERRIDE,
            discovery::rank::OVERRIDE,
        );
    }
    if let Some(previous) = &doc.selected.npm {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(&previous.path),
            sources::PREVIOUS_SELECTED,
            discovery::rank::PREVIOUS,
        );
    }
    // 同族优先：选定 Node 安装树内的 npm-cli.js（设计方案 §6.4）
    if let Some(cli) = discovery::npm_cli_in_node_tree(&node.path) {
        discovery::push_if_file(&mut out, cli, sources::SIBLING_OF_NODE, discovery::rank::SAME_FAMILY);
    }
    // canonicalize 后的 Node 树（symlink 场景，如 /usr/local/bin/node → Cellar）
    if let Ok(canonical) = std::fs::canonicalize(&node.path) {
        if let Some(cli) = discovery::npm_cli_in_node_tree(&canonical) {
            discovery::push_if_file(
                &mut out,
                cli,
                sources::SIBLING_OF_NODE,
                discovery::rank::SAME_FAMILY,
            );
        }
    }
    // Volta 的 npm-cli.js 固定位置
    out.extend(volta_npm_candidates(node));
    out.extend(discovery::find_on_path("npm"));
    #[cfg(windows)]
    out.extend(windows::os_query_candidates("npm"));
    discovery::dedup(out)
}

fn volta_npm_candidates(node: &ValidatedNode) -> Vec<discovery::Candidate> {
    let Some(volta) = discovery::env_dir("VOLTA_HOME")
        .or_else(|| discovery::home_dir().map(|home| home.join(".volta")))
    else {
        return Vec::new();
    };
    let node_canonical =
        std::fs::canonicalize(&node.path).unwrap_or_else(|_| node.path.clone());
    let under_volta = node_canonical
        .to_string_lossy()
        .to_lowercase()
        .starts_with(&volta.to_string_lossy().to_lowercase());
    if !under_volta {
        return Vec::new();
    }
    let cli = volta.join("tools/image/packages/npm/lib/node_modules/npm/bin/npm-cli.js");
    if cli.is_file() {
        vec![discovery::Candidate::new(
            cli,
            sources::SIBLING_OF_NODE,
            discovery::rank::SAME_FAMILY,
        )]
    } else {
        Vec::new()
    }
}

fn collect_adapter_candidates(
    doc: &ToolchainDocument,
    npm: Option<&ValidatedNpm>,
) -> Vec<discovery::Candidate> {
    let mut out = Vec::new();
    if let Some(override_path) = &doc.overrides.adapter {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(override_path),
            sources::OVERRIDE,
            discovery::rank::OVERRIDE,
        );
    }
    if let Some(previous) = &doc.selected.adapter {
        discovery::push_if_file(
            &mut out,
            PathBuf::from(&previous.script_path),
            sources::PREVIOUS_SELECTED,
            discovery::rank::PREVIOUS,
        );
    }
    // 开发/测试 override 统一映射进解析器（设计方案 §13.2，避免双事实源）
    if let Some(dev_bin) = std::env::var_os("AILIGHT_ADAPTER_BIN") {
        let dev = PathBuf::from(dev_bin);
        if dev.is_file() {
            out.push(discovery::Candidate::new(
                dev,
                sources::OVERRIDE,
                discovery::rank::OVERRIDE,
            ));
        }
    }
    if let Some(npm) = npm {
        // global prefix 内的包脚本（设计方案 §6.5 第 3 条）
        discovery::push_if_file(
            &mut out,
            discovery::adapter_script_in_prefix(&npm.prefix),
            sources::NPM_GLOBAL_PREFIX,
            discovery::rank::SAME_FAMILY,
        );
        // prefix 中的 launcher 反查实际脚本（第 4 条）
        discovery::push_if_file(
            &mut out,
            discovery::adapter_launcher_in_prefix(&npm.prefix),
            sources::NPM_GLOBAL_PREFIX,
            discovery::rank::PROCESS_PATH,
        );
    }
    out.extend(discovery::find_on_path("ailight-adapter"));
    discovery::dedup(out)
}

// ---------- 验证 ----------

fn validate_node_candidate(path: &Path) -> Result<semver::Version, String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version");
    let captured =
        validate::run_captured(&mut cmd, validate::VERSION_TIMEOUT, validate::OUTPUT_CAP)
            .map_err(|error| format!("无法启动: {error}"))?;
    if captured.timed_out {
        return Err("验证超时（3 秒）".to_string());
    }
    if !captured.success() {
        return Err(format!("退出码 {:?}", captured.exit_code));
    }
    validate::parse_version_output(&captured.stdout_text())
        .ok_or_else(|| "输出不是有效版本号".to_string())
}

/// npm 候选形态：优先解析到 npm-cli.js（设计方案 §6.4），否则按 launcher 执行
enum NpmForm {
    Cli(PathBuf),
    Launcher(PathBuf),
}

fn classify_npm_candidate(candidate: &Path) -> NpmForm {
    let canonical =
        std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    if canonical
        .extension()
        .map(|ext| ext == "js")
        .unwrap_or(false)
    {
        return NpmForm::Cli(canonical);
    }
    if canonical
        .extension()
        .map(|ext| ext == "cmd" || ext == "bat")
        .unwrap_or(false)
    {
        if let Ok(content) = std::fs::read_to_string(&canonical) {
            if let Some(dir) = canonical.parent() {
                if let Some(target) = discovery::parse_cmd_shim_target(&content, dir) {
                    if target.is_file() {
                        return NpmForm::Cli(target);
                    }
                }
            }
        }
    }
    NpmForm::Launcher(canonical)
}

fn validate_npm_candidate(node: &ValidatedNode, candidate: &Path) -> Result<ValidatedNpm, String> {
    let (cli, launcher) = match classify_npm_candidate(candidate) {
        NpmForm::Cli(cli) => (Some(cli), None),
        NpmForm::Launcher(launcher) => (None, Some(launcher)),
    };
    // --version（同执行形态，设计方案 §6.4）
    let version_captured = match &cli {
        Some(cli) => run_npm_cli(&node.path, cli, &["--version"])?,
        None => {
            let launcher = launcher.as_ref().expect("cli 或 launcher 必有其一");
            let mut cmd = Command::new(launcher);
            runner::apply_clean_env(&mut cmd);
            cmd.arg("--version");
            validate::run_captured(&mut cmd, validate::VERSION_TIMEOUT, validate::OUTPUT_CAP)
                .map_err(|error| format!("无法启动 npm launcher: {error}"))?
        }
    };
    if version_captured.timed_out {
        return Err("验证超时（3 秒）".to_string());
    }
    if !version_captured.success() {
        return Err(format!(
            "npm --version 失败（退出码 {:?}）",
            version_captured.exit_code
        ));
    }
    let npm_version = validate::parse_version_output(&version_captured.stdout_text())
        .ok_or_else(|| "npm 版本输出无效".to_string())?;

    // prefix --global（同执行形态）
    let prefix_captured = match &cli {
        Some(cli) => run_npm_cli(&node.path, cli, &["prefix", "--global"])?,
        None => {
            let launcher = launcher.as_ref().expect("cli 或 launcher 必有其一");
            let mut cmd = Command::new(launcher);
            runner::apply_clean_env(&mut cmd);
            cmd.args(["prefix", "--global"]);
            validate::run_captured(&mut cmd, validate::VERSION_TIMEOUT, validate::OUTPUT_CAP)
                .map_err(|error| format!("无法启动 npm launcher: {error}"))?
        }
    };
    let prefix_text = prefix_captured.stdout_text().trim().to_string();
    if !prefix_captured.success() || prefix_text.is_empty() {
        return Err("无法获取 npm 全局 prefix".to_string());
    }

    // mixedInstallation 判定（设计方案 §6.4）：npm CLI 是否位于选定 Node 安装树内
    let mixed = match &cli {
        Some(cli) => {
            let cli_canonical = std::fs::canonicalize(cli).unwrap_or_else(|_| cli.clone());
            !cli_canonical.starts_with(node_install_root(&node.path))
        }
        None => false,
    };
    Ok(ValidatedNpm {
        cli_script: cli,
        launcher,
        version: npm_version,
        prefix: PathBuf::from(prefix_text),
        source: String::new(),
        mixed_installation: mixed,
    })
}

fn run_npm_cli(node_path: &Path, cli: &Path, args: &[&str]) -> Result<validate::Captured, String> {
    let mut cmd = Command::new(node_path);
    runner::apply_clean_env(&mut cmd);
    cmd.arg(cli).args(args);
    validate::run_captured(&mut cmd, validate::VERSION_TIMEOUT, validate::OUTPUT_CAP)
        .map_err(|error| format!("无法用选定 Node 运行 npm-cli.js: {error}"))
}

/// Node 安装树根（同族判定基准，设计方案 §6.4）
/// Windows：nodejs 目录本身；unix：bin 的父目录（/usr/bin/node → /usr）
fn node_install_root(node_exe: &Path) -> PathBuf {
    let canonical =
        std::fs::canonicalize(node_exe).unwrap_or_else(|_| node_exe.to_path_buf());
    match canonical.parent() {
        Some(dir) if cfg!(windows) => dir.to_path_buf(),
        Some(bin_dir) => bin_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| bin_dir.to_path_buf()),
        None => canonical,
    }
}

fn validate_adapter_script(node: &ValidatedNode, script: &Path) -> Result<ValidatedAdapter, String> {
    if !script.is_file() {
        return Err("文件不存在".to_string());
    }
    let mut cmd = Command::new(&node.path);
    runner::apply_clean_env(&mut cmd);
    cmd.arg(script).args(["version", "--json"]);
    let captured =
        validate::run_captured(&mut cmd, validate::VERSION_TIMEOUT, validate::OUTPUT_CAP)
            .map_err(|error| format!("无法启动: {error}"))?;
    if captured.timed_out {
        return Err("验证超时（3 秒）".to_string());
    }
    if !captured.success() {
        return Err(format!("退出码 {:?}", captured.exit_code));
    }
    let value: serde_json::Value = serde_json::from_str(captured.stdout_text().trim())
        .map_err(|error| format!("JSON 输出无效: {error}"))?;
    if value.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return Err("ok != true".to_string());
    }
    let name = value
        .pointer("/data/name")
        .and_then(|item| item.as_str())
        .unwrap_or_default();
    if name != ADAPTER_PACKAGE {
        return Err(format!("包名不匹配: {name}"));
    }
    let version_text = value
        .pointer("/data/version")
        .and_then(|item| item.as_str())
        .ok_or("缺少 version 字段")?;
    let version =
        semver::Version::parse(version_text).map_err(|error| format!("版本号无效: {error}"))?;
    // 与 Desktop 支持的 Hook Protocol 有交集（设计方案 §6.5）
    let compat = model::adapter_compat_req();
    if !compat.matches(&version) {
        return Err(format!(
            "Adapter {version} 不在兼容范围 {ADAPTER_COMPAT_RANGE} 内"
        ));
    }
    Ok(ValidatedAdapter {
        script: script.to_path_buf(),
        launcher: None,
        version,
        source: String::new(),
    })
}

/// launcher 反查实际脚本（设计方案 §6.5 第 4 条）
fn adapter_script_from_launcher(launcher: &Path) -> Option<PathBuf> {
    let canonical =
        std::fs::canonicalize(launcher).unwrap_or_else(|_| launcher.to_path_buf());
    if canonical
        .extension()
        .map(|ext| ext == "js")
        .unwrap_or(false)
    {
        return Some(canonical);
    }
    if canonical
        .extension()
        .map(|ext| ext == "cmd" || ext == "bat")
        .unwrap_or(false)
    {
        let content = std::fs::read_to_string(&canonical).ok()?;
        let dir = canonical.parent()?;
        let target = discovery::parse_cmd_shim_target(&content, dir)?;
        return target.is_file().then_some(target);
    }
    None
}

// ---------- 解析主流程 ----------

fn resolve(doc: &ToolchainDocument, warning: Option<&str>) -> ResolveOutcome {
    let home = dirs::home_dir();
    let mut issues: Vec<ToolchainIssue> = Vec::new();
    if let Some(message) = warning {
        issues.push(ToolchainIssue {
            code: "TOOLCHAIN_STORE".to_string(),
            message: message.to_string(),
            tool: None,
            recovery: Some("可在设置中恢复自动检测".to_string()),
        });
    }

    let node_pick = pick_node(doc, &mut issues, home.as_deref());
    let (npm_pick, adapter_pick) = match node_pick.resolved {
        Some(ref node) => {
            let npm = pick_npm(doc, node, &mut issues, home.as_deref());
            let adapter = pick_adapter(doc, node, npm.resolved.as_ref(), &mut issues, home.as_deref());
            (npm, adapter)
        }
        None => (blocked_npm_pick(doc), blocked_adapter_pick(doc)),
    };

    let overall = overall_state(
        &node_pick.state,
        &npm_pick.state,
        &adapter_pick.state,
    );
    let summary = build_summary(
        node_pick.resolved.as_ref(),
        npm_pick.resolved.as_ref(),
        adapter_pick.resolved.as_ref(),
    );

    let status = ToolchainStatus {
        state: overall,
        mode: doc.mode,
        summary: sanitize_home(&summary, home.as_deref()),
        node: node_pick.entry,
        npm: npm_pick.entry,
        adapter: adapter_pick.entry,
        issues,
        checked_at: store::rfc3339_now(),
    };

    // resolved + selected 缓存回写（§5.2 selected 是可再生的缓存，仅在不一致时落盘）
    let resolved = node_pick
        .resolved
        .zip(npm_pick.resolved)
        .map(|(node, npm)| ResolvedToolchain {
            node,
            npm,
            adapter: adapter_pick.resolved,
        });
    let persist_doc = resolved.as_ref().and_then(|chain| {
        let next_selected = SelectedToolchain {
            node: Some(SelectedTool {
                path: chain.node.path.display().to_string(),
                version: chain.node.version.to_string(),
                source: chain.node.source.clone(),
            }),
            npm: Some(SelectedTool {
                path: chain
                    .npm
                    .cli_script
                    .as_deref()
                    .unwrap_or(&chain.npm.launcher_path())
                    .display()
                    .to_string(),
                version: chain.npm.version.to_string(),
                source: chain.npm.source.clone(),
            }),
            adapter: chain.adapter.as_ref().map(|adapter| SelectedAdapter {
                launcher_path: adapter.launcher.as_ref().map(|p| p.display().to_string()),
                script_path: adapter.script.display().to_string(),
                version: adapter.version.to_string(),
                source: adapter.source.clone(),
            }),
        };
        if doc.selected == next_selected && doc.last_resolved_at.is_some() {
            return None;
        }
        let mut next = doc.clone();
        next.selected = next_selected;
        next.last_resolved_at = Some(store::rfc3339_now());
        Some(next)
    });

    ResolveOutcome {
        status,
        resolved,
        persist_doc,
    }
}

fn emergency_outcome(error: &str) -> ResolveOutcome {
    ResolveOutcome {
        status: ToolchainStatus {
            state: states::CHECKING.to_string(),
            mode: ToolchainMode::Auto,
            summary: format!("工具链解析异常: {error}"),
            node: None,
            npm: None,
            adapter: None,
            issues: vec![ToolchainIssue {
                code: "INTERNAL".to_string(),
                message: error.to_string(),
                tool: None,
                recovery: Some("请重试或查看日志".to_string()),
            }],
            checked_at: store::rfc3339_now(),
        },
        resolved: None,
        persist_doc: None,
    }
}

/// 上游（Node）缺失时 npm/adapter 无法继续推导；占位条目不追加新 issue（根因已记录）
fn blocked_npm_pick(doc: &ToolchainDocument) -> ToolPick<ValidatedNpm> {
    ToolPick {
        state: states::NPM_MISSING.to_string(),
        resolved: None,
        entry: Some(ToolStatusEntry {
            state: states::NPM_MISSING.to_string(),
            path: None,
            version: None,
            source: None,
            overridden: doc.overrides.npm.is_some(),
        }),
    }
}

fn blocked_adapter_pick(doc: &ToolchainDocument) -> AdapterToolPick {
    AdapterToolPick {
        state: states::ADAPTER_MISSING.to_string(),
        resolved: None,
        entry: Some(AdapterStatusEntry {
            state: states::ADAPTER_MISSING.to_string(),
            path: None,
            launcher_path: None,
            version: None,
            source: None,
            overridden: doc.overrides.adapter.is_some(),
        }),
    }
}

fn overall_state(node: &str, npm: &str, adapter: &str) -> String {
    for (state, tool_state) in [
        (states::INVALID_OVERRIDE, node),
        (states::INVALID_OVERRIDE, npm),
        (states::INVALID_OVERRIDE, adapter),
        (states::NODE_MISSING, node),
        (states::NODE_INCOMPATIBLE, node),
        (states::PERMISSION_DENIED, node),
        (states::NPM_MISSING, npm),
        (states::PERMISSION_DENIED, npm),
        (states::ADAPTER_MISSING, adapter),
        (states::ADAPTER_INCOMPATIBLE, adapter),
        (states::PERMISSION_DENIED, adapter),
    ] {
        if tool_state == state {
            return state.to_string();
        }
    }
    states::READY.to_string()
}

struct ToolPick<T> {
    state: String,
    resolved: Option<T>,
    entry: Option<ToolStatusEntry>,
}

struct AdapterToolPick {
    state: String,
    resolved: Option<ValidatedAdapter>,
    entry: Option<AdapterStatusEntry>,
}

fn issue(
    code: &str,
    message: String,
    tool: Option<ToolKind>,
    recovery: Option<&str>,
) -> ToolchainIssue {
    ToolchainIssue {
        code: code.to_string(),
        message,
        tool: tool.map(|kind| kind.as_str().to_string()),
        recovery: recovery.map(str::to_string),
    }
}

fn pick_node(
    doc: &ToolchainDocument,
    issues: &mut Vec<ToolchainIssue>,
    home: Option<&Path>,
) -> ToolPick<ValidatedNode> {
    let candidates = collect_node_candidates(doc);
    if candidates.is_empty() {
        issues.push(issue(
            "NODE_NOT_FOUND",
            "未在任何已知位置发现 Node.js（PATH、注册表、版本管理器与常见目录均未命中）".to_string(),
            Some(ToolKind::Node),
            Some("安装 Node.js 20+，或点击「选择 Node」手动指定"),
        ));
        return ToolPick {
            state: states::NODE_MISSING.to_string(),
            resolved: None,
            entry: Some(ToolStatusEntry {
                state: states::NODE_MISSING.to_string(),
                path: None,
                version: None,
                source: None,
                overridden: doc.overrides.node.is_some(),
            }),
        };
    }
    let total = candidates.len();
    let mut saw_incompatible = false;
    for candidate in &candidates {
        let overridden = candidate.source == sources::OVERRIDE;
        match validate_node_candidate(&candidate.path) {
            Ok(version) => {
                if validate::node_meets_gate(&version) {
                    let version_text = version.to_string();
                    return ToolPick {
                        state: states::READY.to_string(),
                        resolved: Some(ValidatedNode {
                            path: candidate.path.clone(),
                            version,
                            source: candidate.source.to_string(),
                        }),
                        entry: Some(ToolStatusEntry {
                            state: states::READY.to_string(),
                            path: Some(sanitize_home(
                                &candidate.path.display().to_string(),
                                home,
                            )),
                            version: Some(version_text),
                            source: Some(candidate.source.to_string()),
                            overridden,
                        }),
                    };
                }
                saw_incompatible = true;
                issues.push(issue(
                    "NODE_INCOMPATIBLE",
                    format!(
                        "候选 {} 的 Node.js 版本 {version} 低于 20",
                        sanitize_home(&candidate.path.display().to_string(), home)
                    ),
                    Some(ToolKind::Node),
                    Some("切换或选择 Node 20+ 版本"),
                ));
            }
            Err(reason) => {
                issues.push(issue(
                    "NODE_VALIDATION_FAILED",
                    format!(
                        "候选 {} 验证失败: {reason}",
                        sanitize_home(&candidate.path.display().to_string(), home)
                    ),
                    Some(ToolKind::Node),
                    None,
                ));
                if overridden {
                    // 用户 override 失败：显式配置优先，不静默回退（设计方案 §3.1）
                    break;
                }
            }
        }
    }
    let override_failed = doc.overrides.node.is_some()
        && candidates
            .first()
            .map(|candidate| candidate.source)
            == Some(sources::OVERRIDE);
    let state = if override_failed {
        issues.push(issue(
            "TOOLCHAIN_OVERRIDE_INVALID",
            "手动指定的 Node 路径不可用".to_string(),
            Some(ToolKind::Node),
            Some("重新选择，或在设置中恢复自动检测"),
        ));
        states::INVALID_OVERRIDE.to_string()
    } else if saw_incompatible {
        states::NODE_INCOMPATIBLE.to_string()
    } else {
        states::NODE_MISSING.to_string()
    };
    issues.push(issue(
        "NODE_NOT_FOUND",
        format!("已检查 {total} 个候选，均未通过 Node.js 验证"),
        Some(ToolKind::Node),
        Some("安装 Node.js 20+，或点击「选择 Node」手动指定"),
    ));
    ToolPick {
        state: state.clone(),
        resolved: None,
        entry: Some(ToolStatusEntry {
            state,
            path: None,
            version: None,
            source: None,
            overridden: doc.overrides.node.is_some(),
        }),
    }
}

fn pick_npm(
    doc: &ToolchainDocument,
    node: &ValidatedNode,
    issues: &mut Vec<ToolchainIssue>,
    home: Option<&Path>,
) -> ToolPick<ValidatedNpm> {
    let missing = |state: &str, overridden: bool| ToolPick {
        state: state.to_string(),
        resolved: None,
        entry: Some(ToolStatusEntry {
            state: state.to_string(),
            path: None,
            version: None,
            source: None,
            overridden,
        }),
    };
    let candidates = collect_npm_candidates(doc, node);
    if candidates.is_empty() {
        issues.push(issue(
            "NPM_NOT_FOUND",
            "未发现与选定 Node 关联的 npm".to_string(),
            Some(ToolKind::Npm),
            Some("修复 Node 安装，或点击「选择 npm」手动指定"),
        ));
        return missing(states::NPM_MISSING, doc.overrides.npm.is_some());
    }
    for candidate in &candidates {
        let overridden = candidate.source == sources::OVERRIDE;
        match validate_npm_candidate(node, &candidate.path) {
            Ok(mut npm) => {
                if npm.mixed_installation && !overridden {
                    // 不自动选择混合安装族（设计方案 §6.4）
                    issues.push(issue(
                        "NPM_MIXED_INSTALLATION",
                        format!(
                            "npm 候选 {} 与选定 Node 不属于同一安装树（mixedInstallation）",
                            sanitize_home(&candidate.path.display().to_string(), home)
                        ),
                        Some(ToolKind::Npm),
                        Some("选择同安装族的 npm，或手动覆盖"),
                    ));
                    continue;
                }
                npm.source = candidate.source.to_string();
                let path_text = sanitize_home(&candidate.path.display().to_string(), home);
                let version_text = npm.version.to_string();
                let source_text = candidate.source.to_string();
                return ToolPick {
                    state: states::READY.to_string(),
                    resolved: Some(npm),
                    entry: Some(ToolStatusEntry {
                        state: states::READY.to_string(),
                        path: Some(path_text),
                        version: Some(version_text),
                        source: Some(source_text),
                        overridden,
                    }),
                };
            }
            Err(reason) => {
                issues.push(issue(
                    "NPM_VALIDATION_FAILED",
                    format!(
                        "npm 候选 {} 验证失败: {reason}",
                        sanitize_home(&candidate.path.display().to_string(), home)
                    ),
                    Some(ToolKind::Npm),
                    None,
                ));
                if overridden {
                    break;
                }
            }
        }
    }
    let override_failed = doc.overrides.npm.is_some()
        && candidates
            .first()
            .map(|candidate| candidate.source)
            == Some(sources::OVERRIDE);
    if override_failed {
        issues.push(issue(
            "TOOLCHAIN_OVERRIDE_INVALID",
            "手动指定的 npm 路径不可用".to_string(),
            Some(ToolKind::Npm),
            Some("重新选择，或在设置中恢复自动检测"),
        ));
        return missing(states::INVALID_OVERRIDE, true);
    }
    issues.push(issue(
        "NPM_NOT_FOUND",
        "未找到与选定 Node 同安装族的 npm".to_string(),
        Some(ToolKind::Npm),
        Some("修复 Node 安装，或点击「选择 npm」手动指定"),
    ));
    missing(states::NPM_MISSING, doc.overrides.npm.is_some())
}

fn pick_adapter(
    doc: &ToolchainDocument,
    node: &ValidatedNode,
    npm: Option<&ValidatedNpm>,
    issues: &mut Vec<ToolchainIssue>,
    home: Option<&Path>,
) -> AdapterToolPick {
    let placeholder = |state: &str, overridden: bool| AdapterToolPick {
        state: state.to_string(),
        resolved: None,
        entry: Some(AdapterStatusEntry {
            state: state.to_string(),
            path: None,
            launcher_path: None,
            version: None,
            source: None,
            overridden,
        }),
    };
    let candidates = collect_adapter_candidates(doc, npm);
    if candidates.is_empty() {
        issues.push(issue(
            "ADAPTER_NOT_FOUND",
            "未发现 @ai-light/adapter（npm 全局目录与 PATH 均未命中）".to_string(),
            Some(ToolKind::Adapter),
            Some("点击「连接」时将提示通过 npm 一键安装"),
        ));
        return placeholder(states::ADAPTER_MISSING, doc.overrides.adapter.is_some());
    }
    for candidate in &candidates {
        // launcher 先反查脚本（设计方案 §6.5 第 4 条）
        let script = if candidate
            .path
            .extension()
            .map(|ext| ext == "js")
            .unwrap_or(false)
        {
            Some(candidate.path.clone())
        } else {
            adapter_script_from_launcher(&candidate.path)
        };
        let Some(script) = script else {
            issues.push(issue(
                "ADAPTER_VALIDATION_FAILED",
                format!(
                    "launcher {} 无法反查实际脚本",
                    sanitize_home(&candidate.path.display().to_string(), home)
                ),
                Some(ToolKind::Adapter),
                None,
            ));
            continue;
        };
        match validate_adapter_script(node, &script) {
            Ok(adapter) => {
                let overridden = candidate.source == sources::OVERRIDE;
                let script_text = sanitize_home(&script.display().to_string(), home);
                let launcher_text = sanitize_home(&candidate.path.display().to_string(), home);
                let version_text = adapter.version.to_string();
                let source_text = candidate.source.to_string();
                let same_path = candidate.path == script;
                return AdapterToolPick {
                    state: states::READY.to_string(),
                    resolved: Some(ValidatedAdapter {
                        script,
                        launcher: (!same_path).then(|| candidate.path.clone()),
                        version: adapter.version,
                        source: source_text.clone(),
                    }),
                    entry: Some(AdapterStatusEntry {
                        state: states::READY.to_string(),
                        path: Some(script_text),
                        launcher_path: (!same_path).then_some(launcher_text),
                        version: Some(version_text),
                        source: Some(source_text),
                        overridden,
                    }),
                };
            }
            Err(reason) => {
                let incompatible = reason.contains("兼容范围");
                issues.push(issue(
                    if incompatible {
                        "ADAPTER_INCOMPATIBLE"
                    } else {
                        "ADAPTER_VALIDATION_FAILED"
                    },
                    format!(
                        "Adapter 候选 {} 验证失败: {reason}",
                        sanitize_home(&script.display().to_string(), home)
                    ),
                    Some(ToolKind::Adapter),
                    if incompatible {
                        Some("一键升级 Adapter 或选择其他版本")
                    } else {
                        None
                    },
                ));
                if candidate.source == sources::OVERRIDE {
                    break;
                }
            }
        }
    }
    let override_failed = doc.overrides.adapter.is_some()
        && candidates
            .first()
            .map(|candidate| candidate.source)
            == Some(sources::OVERRIDE);
    if override_failed {
        issues.push(issue(
            "TOOLCHAIN_OVERRIDE_INVALID",
            "手动指定的 Adapter 路径不可用".to_string(),
            Some(ToolKind::Adapter),
            Some("重新选择，或在设置中恢复自动检测"),
        ));
        return placeholder(states::INVALID_OVERRIDE, true);
    }
    let incompatible = issues
        .iter()
        .any(|item| item.code == "ADAPTER_INCOMPATIBLE");
    if incompatible {
        issues.push(issue(
            "ADAPTER_INCOMPATIBLE",
            "发现的 Adapter 均与 Desktop 兼容范围不匹配".to_string(),
            Some(ToolKind::Adapter),
            Some("一键升级或选择其他版本"),
        ));
        return placeholder(states::ADAPTER_INCOMPATIBLE, doc.overrides.adapter.is_some());
    }
    issues.push(issue(
        "ADAPTER_NOT_FOUND",
        "Adapter 候选均未通过验证".to_string(),
        Some(ToolKind::Adapter),
        Some("点击「连接」时将提示通过 npm 一键安装"),
    ));
    placeholder(states::ADAPTER_MISSING, doc.overrides.adapter.is_some())
}

// ---------- 安装目标版本（设计方案 §17.3） ----------

/// 在兼容范围内解析明确版本；registry 查询失败时退回兼容范围表达式（绝不盲装 latest）
fn npm_install_target(chain: &ResolvedToolchain) -> Result<String, String> {
    let home = dirs::home_dir();
    let node = chain.node.clone();
    let npm = chain.npm.clone();
    let captured = runner::npm_view_versions(&node, &npm, ADAPTER_PACKAGE)
        .map_err(|error| format!("npm 无法启动: {error}"))?;
    if captured.success() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(captured.stdout_text().trim())
        {
            let versions: Vec<semver::Version> = match value {
                serde_json::Value::Array(items) => items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .filter_map(|text| semver::Version::parse(text).ok())
                    .collect(),
                _ => Vec::new(),
            };
            let compat = model::adapter_compat_req();
            if let Some(best) = versions
                .iter()
                .filter(|version| compat.matches(version))
                .max()
            {
                return Ok(best.to_string());
            }
            return Err(format!(
                "registry 中没有满足兼容范围 {ADAPTER_COMPAT_RANGE} 的版本"
            ));
        }
    }
    tracing::warn!(
        "registry 版本查询失败（{}），退回兼容范围安装",
        validate::stderr_summary(&captured, home.as_deref())
    );
    Ok(ADAPTER_COMPAT_RANGE.to_string())
}

// ---------- 缓存指纹（设计方案 §11 内容信号失效） ----------

fn fingerprints_of(resolved: Option<&ResolvedToolchain>) -> Vec<(PathBuf, u64, u64)> {
    let Some(chain) = resolved else {
        return Vec::new();
    };
    let mut paths = vec![chain.node.path.clone()];
    paths.extend(chain.npm.cli_script.clone());
    paths.extend(chain.npm.launcher.clone());
    if let Some(adapter) = &chain.adapter {
        paths.push(adapter.script.clone());
        paths.extend(adapter.launcher.clone());
    }
    paths.into_iter().filter_map(fingerprint).collect()
}

fn fingerprint(path: PathBuf) -> Option<(PathBuf, u64, u64)> {
    let metadata = std::fs::metadata(&path).ok()?;
    let mtime = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    Some((path, metadata.len(), mtime))
}

fn fingerprints_match(cache: &Cached) -> bool {
    cache.fingerprints.iter().all(|(path, size, mtime)| {
        fingerprint(path.clone())
            .is_some_and(|(_, size_now, mtime_now)| size_now == *size && mtime_now == *mtime)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实执行链路冒烟：完整跑一遍发现 + 受控验证（不限机器是否装有 Node），
    /// 只断言解析器输出不变量；具体选中结果依机器环境而定。
    #[test]
    fn resolve_produces_consistent_status() {
        const KNOWN_STATES: &[&str] = &[
            states::READY,
            states::NODE_MISSING,
            states::NODE_INCOMPATIBLE,
            states::NPM_MISSING,
            states::ADAPTER_MISSING,
            states::ADAPTER_INCOMPATIBLE,
            states::INVALID_OVERRIDE,
            states::AMBIGUOUS,
            states::PERMISSION_DENIED,
            states::CHECKING,
        ];
        let doc = ToolchainDocument::new();
        let outcome = resolve(&doc, None);
        assert!(KNOWN_STATES.contains(&outcome.status.state.as_str()));
        assert!(!outcome.status.summary.is_empty());
        assert!(!outcome.status.checked_at.is_empty());
        // node 状态条目必须存在
        assert!(outcome.status.node.is_some());
        // resolved 与 status 一致：ready 时必须有完整链
        if outcome.status.state == states::READY {
            let chain = outcome.resolved.expect("ready 必须产出 resolved");
            assert!(chain.adapter.is_some());
            assert!(outcome.persist_doc.is_some());
        }
    }

    /// override 失败 → 不静默回退：状态 invalid_override（设计方案 §3.1）
    #[test]
    fn invalid_node_override_does_not_fall_back() {
        let dir = std::env::temp_dir().join(format!(
            "ailight-override-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        // 用一个存在但不是 node 的普通文件充当 override（存在性检查通过、验证失败）
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("fake-node");
        std::fs::write(&fake, b"not an executable").unwrap();
        // unix 下需要可执行位才会走到执行失败分支
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut doc = ToolchainDocument::new();
        doc.overrides.node = Some(fake.display().to_string());
        let outcome = resolve(&doc, None);
        assert_eq!(outcome.status.state, states::INVALID_OVERRIDE);
        assert!(outcome
            .status
            .issues
            .iter()
            .any(|issue| issue.code == "TOOLCHAIN_OVERRIDE_INVALID"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 兼容范围解析：registry 数组 → 兼容范围内最大版本
    #[test]
    fn compat_range_selects_max_matching_version() {
        let compat = model::adapter_compat_req();
        let versions = ["0.1.2", "0.2.0", "0.1.10", "1.0.0"];
        let best = versions
            .iter()
            .filter_map(|text| semver::Version::parse(text).ok())
            .filter(|version| compat.matches(version))
            .max()
            .expect("0.1.x 系列必须存在");
        assert_eq!(best.to_string(), "0.1.10");
    }
}

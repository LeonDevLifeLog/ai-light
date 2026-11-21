//! Tauri commands（ipc-contract V1.0 §2：P1 全部 14 个 + 工具链 4 个，设计方案 §7）

use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

use ailight_core::arbiter::ST_IDLE;
use ailight_core::ble::{self, BleDeviceInfo};
use ailight_core::config::{AppConfig, DeviceTransport, RememberedDevice};
use ailight_core::device::ConnectedDevice;
use ailight_core::engine::{self, EngineError};
use ailight_core::hook_server::{BusinessSnapshot, DeviceSnapshot, ServiceSnapshot, SharedState};
use ailight_core::theme::{self, ThemeFile};
use ailight_core::transport::{Transport, TransportError};
use ailight_core::usb;

use crate::toolchain::model::states;
use crate::toolchain::model::{
    AdapterUpdateInfo, ResolvedToolchain, ToolKind, ToolchainOverrides, ToolchainStatus,
};
use crate::toolchain::{runner, validate, ToolchainError};
use crate::AppState;

const UPDATE_REPOSITORY_URL: &str = "https://github.com/LeonDevLifeLog/ai-light";
const UPDATE_API_URLS: [&str; 4] = [
    "https://api.github.com/repos/LeonDevLifeLog/ai-light/releases/latest",
    "https://ghfast.top/https://api.github.com/repos/LeonDevLifeLog/ai-light/releases/latest",
    "https://gh-proxy.com/https://api.github.com/repos/LeonDevLifeLog/ai-light/releases/latest",
    "https://ghproxy.net/https://api.github.com/repos/LeonDevLifeLog/ai-light/releases/latest",
];
const UPDATE_DOWNLOAD_PREFIXES: [&str; 4] = [
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "",
];

fn update_asset_suffix() -> &'static str {
    if cfg!(target_os = "windows") {
        "x64-setup.exe"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64.dmg"
    } else if cfg!(target_os = "macos") {
        "x64.dmg"
    } else {
        "amd64.AppImage"
    }
}

// ---- 错误模型（ipc-contract §4 + 设计方案 §7 错误码扩展） ----

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    /// 结构化诊断字段（kind/path/source/reason 等，设计方案 §7；
    /// 不返回完整环境变量或 token）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

type CmdResult<T> = Result<T, AppError>;

fn err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError {
        code,
        message: message.into(),
        details: None,
    }
}

fn err_with_details(
    code: &'static str,
    message: impl Into<String>,
    details: serde_json::Value,
) -> AppError {
    AppError {
        code,
        message: message.into(),
        details: Some(details),
    }
}

fn internal(e: impl std::fmt::Display) -> AppError {
    err("INTERNAL", e.to_string())
}

fn shared(app: &AppHandle) -> std::sync::Arc<SharedState> {
    app.state::<AppState>().shared.clone()
}

#[tauri::command]
pub async fn fetch_latest_release() -> CmdResult<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("AI-Light update checker")
        .build()
        .map_err(internal)?;
    let mut requests = tokio::task::JoinSet::new();
    for url in UPDATE_API_URLS {
        let client = client.clone();
        requests.spawn(async move {
            let response = client.get(url).send().await.ok()?;
            if !response.status().is_success() {
                return None;
            }
            let value = response.json::<serde_json::Value>().await.ok()?;
            let valid = value
                .get("tag_name")
                .and_then(|item| item.as_str())
                .is_some()
                && value
                    .get("assets")
                    .and_then(|item| item.as_array())
                    .is_some();
            valid.then_some(value)
        });
    }
    while let Some(result) = requests.join_next().await {
        if let Ok(Some(mut value)) = result {
            requests.abort_all();
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "_asset_suffix".to_string(),
                    serde_json::Value::String(update_asset_suffix().to_string()),
                );
            }
            return Ok(value);
        }
    }
    Err(err(
        "UPDATE_CHECK_FAILED",
        "所有更新源暂时不可用，请稍后重试",
    ))
}

#[tauri::command]
pub async fn resolve_update_download_url(download_url: String) -> CmdResult<String> {
    let expected_prefix = format!("{UPDATE_REPOSITORY_URL}/releases/download/");
    if !download_url.starts_with(&expected_prefix) {
        return Err(err("BAD_REQUEST", "更新下载地址不属于 AI-Light 官方仓库"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .user_agent("AI-Light update downloader")
        .build()
        .map_err(internal)?;
    for prefix in UPDATE_DOWNLOAD_PREFIXES {
        let candidate = format!("{prefix}{download_url}");
        if client
            .head(&candidate)
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            return Ok(candidate);
        }
    }
    Ok(format!("{UPDATE_REPOSITORY_URL}/releases/latest"))
}

// ---- Adapter CLI 集成（统一走 ToolchainService / ProcessRunner，设计方案 §4） ----

fn valid_integration_tool(tool: &str) -> bool {
    matches!(
        tool,
        "claude-code" | "codex" | "qoder" | "trae" | "workbuddy"
    )
}

/// ToolchainError → AppError（设计方案 §7 错误码表）
fn map_toolchain_error(error: ToolchainError) -> AppError {
    match error {
        ToolchainError::InvalidOverride { kind, path, reason } => err_with_details(
            "TOOLCHAIN_OVERRIDE_INVALID",
            format!("{} 路径不可用: {reason}", kind_label(kind)),
            serde_json::json!({ "kind": kind.as_str(), "path": path, "reason": reason }),
        ),
        ToolchainError::Resolution(status) => resolution_error(&status),
        ToolchainError::StoreProtected(message) => err(
            "TOOLCHAIN_STORE_INVALID",
            format!("工具链配置受保护，请先恢复自动检测: {message}"),
        ),
        ToolchainError::Io(message) => internal(message),
    }
}

fn map_adapter_update_error(error: ToolchainError) -> AppError {
    match error {
        ToolchainError::Io(message) => err("ADAPTER_UPDATE_FAILED", message),
        other => map_toolchain_error(other),
    }
}

fn kind_label(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Node => "Node",
        ToolKind::Npm => "npm",
        ToolKind::Adapter => "Adapter",
    }
}

fn resolution_error(status: &ToolchainStatus) -> AppError {
    let (code, message) = match status.state.as_str() {
        states::NODE_MISSING => ("NODE_NOT_FOUND", "未找到可用的 Node.js（需要 20+）"),
        states::NODE_INCOMPATIBLE => (
            "NODE_INCOMPATIBLE",
            "Node.js 版本低于 20，请切换或选择兼容版本",
        ),
        states::NPM_MISSING => ("NPM_NOT_FOUND", "已发现 Node.js，但未找到关联的 npm"),
        states::INVALID_OVERRIDE => (
            "TOOLCHAIN_OVERRIDE_INVALID",
            "手动路径不可用，请重新选择或恢复自动检测",
        ),
        states::AMBIGUOUS => (
            "TOOLCHAIN_AMBIGUOUS",
            "多组候选无法安全决策，请手动选择一组 Node",
        ),
        states::PERMISSION_DENIED => ("TOOLCHAIN_PERMISSION_DENIED", "文件或子进程权限不足"),
        states::STORE_INVALID => (
            "TOOLCHAIN_STORE_INVALID",
            "工具链配置无法读取或版本不兼容，请先恢复自动检测",
        ),
        states::ADAPTER_MISSING => ("ADAPTER_NOT_FOUND", "Adapter 未安装，可在接入页一键安装"),
        states::ADAPTER_INCOMPATIBLE => ("ADAPTER_INCOMPATIBLE", "Adapter 版本不兼容，请升级"),
        _ => ("INTERNAL", "工具链状态异常"),
    };
    err_with_details(
        code,
        message,
        serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
    )
}

fn dev_output_to_value(stdout: &[u8], exit_code: Option<i32>) -> CmdResult<serde_json::Value> {
    let value = serde_json::from_slice::<serde_json::Value>(stdout).map_err(|error| {
        err(
            "ADAPTER_INVALID_OUTPUT",
            format!("Adapter 返回无效数据: {error}"),
        )
    })?;
    if exit_code != Some(0) || value.get("ok") != Some(&serde_json::Value::Bool(true)) {
        let message = value
            .pointer("/error/message")
            .and_then(|item| item.as_str())
            .unwrap_or("Adapter 命令执行失败");
        return Err(err("ADAPTER_COMMAND_FAILED", message));
    }
    Ok(value
        .get("data")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// 通过已解析工具链执行 Adapter 管理命令（稳定入口：node + cli.js，设计方案 §6.5）
async fn adapter_command_with_chain(
    chain: &Arc<ResolvedToolchain>,
    args: &[&str],
) -> CmdResult<serde_json::Value> {
    let adapter = chain
        .adapter
        .as_ref()
        .ok_or_else(|| err("ADAPTER_NOT_FOUND", "Adapter 未安装，可在接入页一键安装"))?;
    let node = chain.node.clone();
    let adapter = adapter.clone();
    let mut argv: Vec<String> = args.iter().map(|item| item.to_string()).collect();
    argv.push("--json".to_string());
    let captured = tauri::async_runtime::spawn_blocking(move || {
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        runner::run_adapter(&node, &adapter, &argv, validate::VERSION_TIMEOUT)
    })
    .await
    .map_err(internal)?
    .map_err(|error| {
        err(
            "ADAPTER_COMMAND_FAILED",
            format!("无法启动 Adapter: {error}"),
        )
    })?;
    captured_to_value(captured)
}

fn captured_to_value(captured: validate::Captured) -> CmdResult<serde_json::Value> {
    if captured.timed_out {
        return Err(err("EXECUTABLE_TIMEOUT", "Adapter 命令执行超时"));
    }
    dev_output_to_value(&captured.stdout, captured.exit_code)
}

#[tauri::command]
pub async fn get_integration_status(app: AppHandle, tool: String) -> CmdResult<serde_json::Value> {
    if !valid_integration_tool(&tool) {
        return Err(err("BAD_REQUEST", "不支持的接入工具"));
    }
    let service = &app.state::<AppState>().toolchain;
    // 只读查询：可用进程内缓存（设计方案 §11）；Adapter 缺失时返回结构化未连接状态（§7）
    let chain = service
        .resolved_for_write(false)
        .await
        .map_err(map_toolchain_error)?;
    let Some(_) = chain.adapter.as_ref() else {
        let status = service.status(false).await;
        return Ok(serde_json::json!({
            "connected": false,
            "managedCount": 0,
            "path": "",
            "reason": "adapter_missing",
            "toolchainState": status.state,
            "toolchainSummary": status.summary,
        }));
    };
    adapter_command_with_chain(&chain, &["detect", &tool]).await
}

#[tauri::command]
pub async fn install_integration(app: AppHandle, tool: String) -> CmdResult<serde_json::Value> {
    if !valid_integration_tool(&tool) {
        return Err(err("BAD_REQUEST", "不支持的接入工具"));
    }
    let service = &app.state::<AppState>().toolchain;
    // 写操作：强制复验（设计方案 §6.1 / §11）
    let chain = service
        .resolved_for_write(false)
        .await
        .map_err(map_toolchain_error)?;
    // Adapter 缺失：用已选 Node + npm CLI 安装后重新解析（设计方案 §7 / §9.1）
    let chain = if chain.adapter.is_none() {
        service
            .install_adapter()
            .await
            .map_err(map_toolchain_error)?
    } else {
        chain
    };
    adapter_command_with_chain(&chain, &["install", &tool]).await
}

#[tauri::command]
pub async fn uninstall_integration(app: AppHandle, tool: String) -> CmdResult<serde_json::Value> {
    if !valid_integration_tool(&tool) {
        return Err(err("BAD_REQUEST", "不支持的接入工具"));
    }
    let service = &app.state::<AppState>().toolchain;
    // 工具链损坏（Adapter 不可用）→ needs_repair 语义，不得误删其他 Hook（设计方案 §7）
    let chain = service
        .resolved_for_write(true)
        .await
        .map_err(|error| match error {
            ToolchainError::Resolution(status) if status.state == states::ADAPTER_MISSING => err(
                "ADAPTER_NOT_FOUND",
                "Adapter 不可用，无法安全卸载 Hook；请先修复运行环境后重试（needs_repair）",
            ),
            other => map_toolchain_error(other),
        })?;
    adapter_command_with_chain(&chain, &["uninstall", &tool]).await
}

// ---- 工具链域（设计方案 §7 IPC 契约） ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainOverridesPatch {
    pub node: Option<String>,
    pub npm: Option<String>,
    pub adapter: Option<String>,
}

#[tauri::command]
pub async fn get_toolchain_status(
    app: AppHandle,
    force: Option<bool>,
) -> CmdResult<ToolchainStatus> {
    Ok(app
        .state::<AppState>()
        .toolchain
        .status(force.unwrap_or(false))
        .await)
}

#[tauri::command]
pub async fn set_toolchain_overrides(
    app: AppHandle,
    patch: ToolchainOverridesPatch,
) -> CmdResult<ToolchainStatus> {
    let overrides = ToolchainOverrides {
        node: patch.node,
        npm: patch.npm,
        adapter: patch.adapter,
    };
    app.state::<AppState>()
        .toolchain
        .set_overrides(overrides)
        .await
        .map_err(map_toolchain_error)
}

#[tauri::command]
pub async fn reset_toolchain_overrides(app: AppHandle) -> CmdResult<ToolchainStatus> {
    app.state::<AppState>()
        .toolchain
        .reset_overrides()
        .await
        .map_err(map_toolchain_error)
}

#[tauri::command]
pub async fn select_executable(app: AppHandle, kind: String) -> CmdResult<ToolchainStatus> {
    let kind =
        ToolKind::parse(&kind).ok_or_else(|| err("BAD_REQUEST", format!("kind 非法: {kind}")))?;
    // 原生文件选择器由后端打开，前端不能传任意未确认路径冒充选择结果（设计方案 §7）
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || pick_executable(&handle, kind))
        .await
        .map_err(internal)?;
    let Some(path) = picked else {
        // 取消选择不改变现有配置（设计方案 §8.2）
        return Ok(app.state::<AppState>().toolchain.current_status().await);
    };
    app.state::<AppState>()
        .toolchain
        .select_executable(kind, path)
        .await
        .map_err(map_toolchain_error)
}

#[tauri::command]
pub async fn check_adapter_update(app: AppHandle) -> CmdResult<AdapterUpdateInfo> {
    app.state::<AppState>()
        .toolchain
        .check_adapter_update()
        .await
        .map_err(map_adapter_update_error)
}

#[tauri::command]
pub async fn upgrade_adapter(
    app: AppHandle,
    target_version: String,
) -> CmdResult<serde_json::Value> {
    let service = &app.state::<AppState>().toolchain;
    let chain = service
        .upgrade_adapter(&target_version)
        .await
        .map_err(map_adapter_update_error)?;
    let doctor = adapter_command_with_chain(&chain, &["doctor"]).await?;
    let toolchain = service.status(false).await;
    Ok(serde_json::json!({
        "doctor": doctor,
        "toolchain": toolchain,
    }))
}

fn pick_executable(app: &AppHandle, kind: ToolKind) -> Option<std::path::PathBuf> {
    let dialog = app.dialog().file().set_title(match kind {
        ToolKind::Node => "选择 Node.js 可执行文件（需要 20+）",
        ToolKind::Npm => "选择 npm（npm-cli.js 或平台 launcher）",
        ToolKind::Adapter => "选择 Adapter（dist/cli.js）",
    });
    let dialog = match kind {
        ToolKind::Node => {
            #[cfg(windows)]
            {
                dialog.add_filter("node 可执行文件", &["exe"])
            }
            #[cfg(not(windows))]
            {
                dialog
            }
        }
        ToolKind::Npm => {
            #[cfg(windows)]
            {
                dialog.add_filter("npm", &["cmd", "exe", "js"])
            }
            #[cfg(not(windows))]
            {
                dialog
            }
        }
        ToolKind::Adapter => {
            #[cfg(windows)]
            {
                dialog.add_filter("Adapter 脚本", &["cmd", "js"])
            }
            #[cfg(not(windows))]
            {
                dialog
            }
        }
    };
    dialog
        .blocking_pick_file()
        .and_then(|file| file.into_path().ok())
}

// ---- 状态查询 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateSnapshot {
    pub service: ServiceSnapshot,
    pub device: DeviceSnapshot,
    pub business: BusinessSnapshot,
    pub themes: Vec<String>,
    pub active_theme: String,
}

#[tauri::command]
pub fn get_app_state(app: AppHandle) -> CmdResult<AppStateSnapshot> {
    let s = shared(&app);
    let business = {
        let theme_name = s.theme_name.read().map(|t| t.clone()).unwrap_or_default();
        match s.arbiter.read() {
            Ok(guard) => {
                let b = guard.current();
                BusinessSnapshot {
                    state: b.state.clone(),
                    source: b.source.clone(),
                    session: b.session.clone(),
                    since_ts: b.since_ms,
                    theme: theme_name,
                }
            }
            Err(_) => BusinessSnapshot {
                state: ST_IDLE.into(),
                source: None,
                session: None,
                since_ts: 0,
                theme: theme_name,
            },
        }
    };
    let service = ServiceSnapshot {
        version: s.app_version.clone(),
        port: s.port.read().map(|p| *p).unwrap_or(25679),
        token_enabled: s.token.read().map(|t| t.is_some()).unwrap_or(false),
    };
    let device = s.device.read().map(|d| d.clone()).unwrap_or_default();
    let themes = theme::builtin_theme_names()
        .into_iter()
        .map(String::from)
        .collect();
    let active_theme = s.theme_name.read().map(|t| t.clone()).unwrap_or_default();
    Ok(AppStateSnapshot {
        service,
        device,
        business,
        themes,
        active_theme,
    })
}

// ---- 主题域 ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeMeta {
    pub name: String,
    pub builtin: bool,
}

#[tauri::command]
pub fn get_themes(app: AppHandle) -> CmdResult<Vec<ThemeMeta>> {
    let mut names: Vec<ThemeMeta> = theme::builtin_theme_names()
        .into_iter()
        .map(|n| ThemeMeta {
            name: n.to_string(),
            builtin: true,
        })
        .collect();
    if let Ok(dir) = user_theme_dir(&app) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let path = e.path();
                if let Some(name) = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.strip_suffix(".ailight-theme.json"))
                {
                    names.push(ThemeMeta {
                        name: name.to_string(),
                        builtin: false,
                    });
                }
            }
        }
    }
    Ok(names)
}

#[tauri::command]
pub fn get_theme(app: AppHandle, name: String) -> CmdResult<String> {
    if let Some(content) = builtin_theme_content(&name) {
        return Ok(content.to_string());
    }
    let dir = user_theme_dir(&app).map_err(internal)?;
    let path = dir.join(format!("{name}.ailight-theme.json"));
    std::fs::read_to_string(&path).map_err(|_| err("NOT_FOUND", format!("主题不存在: {name}")))
}

#[tauri::command]
pub fn set_active_theme(app: AppHandle, name: String) -> CmdResult<()> {
    let s = shared(&app);
    let previous = s
        .theme_name
        .read()
        .map(|value| value.clone())
        .unwrap_or_default();
    tracing::info!(event = "theme_change_requested", from_theme = %previous, to_theme = %name, "请求切换主题");
    // 校验主题存在且合法
    let theme = resolve_theme(&app, &name).map_err(|e| err("THEME_INVALID", e))?;
    *s.theme.write().map_err(|_| internal("theme 锁"))? = Some(theme);
    *s.theme_name
        .write()
        .map_err(|_| internal("theme_name 锁"))? = name.clone();
    // 持久化
    persist_active_theme(&app, &name)?;
    let _ = app.emit("theme-changed", serde_json::json!({ "name": name }));
    crate::tray::update_theme(&app, &name);
    tracing::info!(event = "theme_changed", from_theme = %previous, to_theme = %name, "主题切换完成");
    // 当前业务非 IDLE → 用新主题重放（幂等对齐，ipc-contract §2.2 副作用）
    let state_now = s
        .arbiter
        .read()
        .map(|g| g.current().state.clone())
        .unwrap_or_default();
    if state_now != ST_IDLE {
        let s2 = s.clone();
        tauri::async_runtime::spawn(async move {
            let signal_id = s2.next_signal_id();
            let state = s2
                .arbiter
                .read()
                .map(|guard| guard.current().state.clone())
                .unwrap_or_else(|_| "UNKNOWN".into());
            let _ = engine::compile_current(&s2).and_then(|scene| {
                s2.send_outbound(ailight_core::hook_server::OutboundScene {
                    scene,
                    signal_id,
                    state,
                })
                .map_err(EngineError::State)
            });
        });
    }
    Ok(())
}

#[tauri::command]
pub fn import_theme(app: AppHandle, content: String) -> CmdResult<String> {
    // 整体校验（ADR-0002 T-06）
    let theme = theme::load(&content).map_err(|e| err("THEME_INVALID", e.to_string()))?;
    let name = theme.theme.name.clone();
    if theme::builtin_theme_names().contains(&name.as_str()) {
        return Err(err("CONFLICT", format!("与内置主题同名: {name}")));
    }
    let dir = user_theme_dir(&app).map_err(internal)?;
    std::fs::create_dir_all(&dir).map_err(internal)?;
    std::fs::write(dir.join(format!("{name}.ailight-theme.json")), content).map_err(internal)?;
    tracing::info!(event = "theme_imported", theme = %name, "用户主题导入完成");
    Ok(name)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportThemeResult {
    pub status: &'static str,
    pub file_name: Option<String>,
}

#[tauri::command]
pub async fn export_theme(app: AppHandle, name: String) -> CmdResult<ExportThemeResult> {
    validate_export_theme_name(&name)?;
    let path = user_theme_dir(&app)
        .map_err(internal)?
        .join(format!("{name}.ailight-theme.json"));
    let content = match std::fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(internal(error)),
    };
    let content = prepare_theme_export(&name, content)?;
    let file_name = format!("{name}.ailight-theme.json");
    let destination = app
        .dialog()
        .file()
        .add_filter("AI-Light 主题", &["json"])
        .set_file_name(&file_name)
        .blocking_save_file();
    let Some(destination) = destination else {
        return Ok(ExportThemeResult {
            status: "cancelled",
            file_name: None,
        });
    };
    let destination = destination.into_path().map_err(internal)?;
    let exported_file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&file_name)
        .to_string();
    std::fs::write(destination, content).map_err(internal)?;
    Ok(ExportThemeResult {
        status: "exported",
        file_name: Some(exported_file_name),
    })
}

fn prepare_theme_export(name: &str, content: Option<String>) -> CmdResult<String> {
    validate_export_theme_name(name)?;
    let content = content.ok_or_else(|| err("NOT_FOUND", format!("主题不存在: {name}")))?;
    theme::load(&content).map_err(|error| err("THEME_INVALID", error.to_string()))?;
    Ok(content)
}

fn validate_export_theme_name(name: &str) -> CmdResult<()> {
    if !valid_theme_name(name) {
        return Err(err("BAD_REQUEST", format!("主题名称非法: {name}")));
    }
    if theme::builtin_theme_names().contains(&name) {
        return Err(err("THEME_BUILTIN", format!("内置主题不可导出: {name}")));
    }
    Ok(())
}

#[tauri::command]
pub fn delete_theme(app: AppHandle, name: String) -> CmdResult<serde_json::Value> {
    if theme::builtin_theme_names().contains(&name.as_str()) {
        return Err(err("THEME_BUILTIN", format!("内置主题不可删除: {name}")));
    }
    if !valid_theme_name(&name) {
        return Err(err("BAD_REQUEST", format!("主题名称非法: {name}")));
    }

    let path = user_theme_dir(&app)
        .map_err(internal)?
        .join(format!("{name}.ailight-theme.json"));
    if !path.is_file() {
        return Err(err("NOT_FOUND", format!("主题不存在: {name}")));
    }

    let is_active = shared(&app)
        .theme_name
        .read()
        .map(|current| current.as_str() == name)
        .map_err(|_| internal("theme_name 锁"))?;
    if is_active {
        set_active_theme(app.clone(), "default".into())?;
    }

    if let Err(error) = std::fs::remove_file(path) {
        if is_active {
            let _ = set_active_theme(app.clone(), name.clone());
        }
        return Err(internal(error));
    }
    tracing::info!(event = "theme_deleted", theme = %name, was_active = is_active, "用户主题删除完成");
    Ok(serde_json::json!({ "ok": true }))
}

// ---- 设备域 ----

#[tauri::command]
pub async fn scan_devices(_app: AppHandle) -> CmdResult<Vec<BleDeviceInfo>> {
    tracing::info!(
        event = "ble_scan_started",
        duration_seconds = 5,
        "开始扫描状态灯"
    );
    let adapter = tokio::time::timeout(Duration::from_secs(8), ble::default_adapter())
        .await
        .map_err(|_| internal("等待蓝牙适配器超时"))?
        .map_err(internal)?;
    let devices = ble::scan(&adapter, 5).await.map_err(internal)?;
    tracing::info!(
        event = "ble_scan_completed",
        discovered = devices.len(),
        recognized = devices.iter().filter(|device| device.recognized).count(),
        "状态灯扫描完成"
    );
    Ok(devices)
}

fn usb_vid_pid() -> CmdResult<Option<(u16, u16)>> {
    let runtime = if cfg!(debug_assertions) {
        (
            std::env::var("AILIGHT_USB_VID").ok(),
            std::env::var("AILIGHT_USB_PID").ok(),
        )
    } else {
        (None, None)
    };
    let pair = match (&runtime.0, &runtime.1) {
        (None, None) => (
            option_env!("AILIGHT_USB_VID"),
            option_env!("AILIGHT_USB_PID"),
        ),
        _ => (runtime.0.as_deref(), runtime.1.as_deref()),
    };
    fn parse_id(value: &str) -> Option<u16> {
        let hex = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .unwrap_or(value);
        u16::from_str_radix(hex, 16).ok().filter(|id| *id != 0)
    }
    match (
        pair.0.filter(|value| !value.is_empty()),
        pair.1.filter(|value| !value.is_empty()),
    ) {
        (None, None) => Ok(None),
        (Some(vid), Some(pid)) => parse_id(vid).zip(parse_id(pid)).map(Some).ok_or_else(|| {
            err(
                "BAD_REQUEST",
                "请为 AILIGHT_USB_VID 和 AILIGHT_USB_PID 同时配置有效的十六进制值",
            )
        }),
        _ => Err(err(
            "BAD_REQUEST",
            "请为 AILIGHT_USB_VID 和 AILIGHT_USB_PID 同时配置有效的十六进制值",
        )),
    }
}

fn usb_error(error: usb::Error) -> AppError {
    let code = match &error {
        usb::Error::NotFound => "NOT_FOUND",
        usb::Error::InvalidScene(_) => "BAD_REQUEST",
        usb::Error::Rejected { result: 0x04, .. } => "CONFLICT",
        usb::Error::Closed => "DEVICE_NOT_CONNECTED",
        _ => "INTERNAL",
    };
    err(code, error.to_string())
}

#[tauri::command]
pub async fn scan_usb_devices() -> CmdResult<Vec<usb::Candidate>> {
    match usb_vid_pid()? {
        Some((vid, pid)) => usb::scan(vid, pid).await.map_err(usb_error),
        None => Ok(vec![]),
    }
}

fn snapshot_for(device: &RememberedDevice) -> DeviceSnapshot {
    let mut snapshot = DeviceSnapshot {
        name: Some(device.name().into()),
        ..Default::default()
    };
    match device {
        RememberedDevice::Ble { address, .. } => {
            snapshot.transport = Some(DeviceTransport::Ble);
            snapshot.address = Some(address.clone());
        }
        RememberedDevice::Usb { usb_uid, .. } => {
            snapshot.transport = Some(DeviceTransport::Usb);
            snapshot.usb_uid = Some(usb_uid.clone());
        }
    }
    snapshot
}

fn publish_device(app: &AppHandle, snapshot: DeviceSnapshot, reason: Option<&str>) {
    #[derive(Clone, Serialize)]
    struct ConnectionEvent<'a> {
        #[serde(flatten)]
        device: &'a DeviceSnapshot,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<&'a str>,
    }
    *shared(app).device.write().unwrap() = snapshot.clone();
    let _ = app.emit(
        "device-connection-changed",
        ConnectionEvent {
            device: &snapshot,
            reason,
        },
    );
    crate::tray::update_device(app, snapshot.connected, snapshot.name.as_deref());
}

fn remember_device(app: &AppHandle, device: RememberedDevice) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let mut config = state.config.write().map_err(|_| internal("config 锁"))?;
    if config.remembered_device.as_ref() != Some(&device) {
        let mut updated = config.clone();
        updated.remembered_device = Some(device);
        persist_config(app, &updated)?;
        *config = updated;
        let _ = app.emit("config-changed", &*config);
    }
    Ok(())
}

async fn close_output(connection: &ConnectedDevice, switching: bool) -> CmdResult<()> {
    match connection {
        ConnectedDevice::Ble { io, transport } => {
            if switching {
                if let Err(error) = transport.reset_outputs().await {
                    tracing::warn!(%error, "切换设备前未能复位蓝牙灯牌");
                }
            }
            io.disconnect()
                .await
                .map_err(|e| err("DEVICE_DISCONNECT_FAILED", e.to_string()))?;
        }
        ConnectedDevice::Usb(session) => {
            match tokio::time::timeout(Duration::from_secs(4), session.close()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "USB 会话关闭应答不可用"),
                Err(_) => tracing::warn!("等待 USB 会话关闭超时"),
            }
        }
    }
    Ok(())
}

// The connection lock serializes attach, detach and recovery commits.
async fn connect_locked(
    app: &AppHandle,
    target: &RememberedDevice,
    generation: u64,
) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if state.exiting.load(Ordering::SeqCst) {
        return Err(internal("应用正在退出"));
    }
    let (connection, device, mut snapshot, ble_events) = match target {
        RememberedDevice::Ble { address, name } => {
            let addr = ble::normalize_address(address);
            let (mut io, actual_name, handshake) =
                tokio::time::timeout(Duration::from_secs(20), async {
                    let adapter = ble::default_adapter().await.map_err(internal)?;
                    let _ = ble::scan(&adapter, 4).await.map_err(internal)?;
                    let mut result = ble::connect_to_address(&adapter, &addr).await;
                    if matches!(result, Err(ble::BleError::DeviceNotFound(_))) {
                        let _ = ble::scan(&adapter, 4).await.map_err(internal)?;
                        result = ble::connect_to_address(&adapter, &addr).await;
                    }
                    result.map_err(|error| match error {
                        ble::BleError::DeviceNotFound(_) => err("NOT_FOUND", error.to_string()),
                        error => internal(error),
                    })
                })
                .await
                .map_err(|_| internal("蓝牙连接超时"))??;
            let device = RememberedDevice::Ble {
                address: addr,
                name: if name.is_empty() {
                    actual_name
                } else {
                    name.clone()
                },
            };
            let mut snapshot = snapshot_for(&device);
            let (major, minor, patch) = handshake.device_info.fw;
            snapshot.fw_version = Some(format!("{major}.{minor}.{patch}"));
            snapshot.hardware_variant = Some(handshake.device_info.hardware_variant);
            snapshot.capability_bits = Some(handshake.capabilities.capability_bits);
            if let Some(power) = handshake.power {
                snapshot.power_source = Some(power.power_source);
                snapshot.power_flags = Some(power.power_flags);
                snapshot.charge_state = Some(power.charge_state);
                snapshot.battery_mv = (power.battery_mv != 0xFFFF).then_some(power.battery_mv);
                snapshot.battery_percent =
                    (power.battery_percent != 0xFF).then_some(power.battery_percent);
            }
            let events = io.take_events();
            let io = Arc::new(io);
            let transport = Transport::new(io.clone(), None);
            (
                ConnectedDevice::Ble { io, transport },
                device,
                snapshot,
                events,
            )
        }
        RememberedDevice::Usb { usb_uid, .. } => {
            let (vid, pid) =
                usb_vid_pid()?.ok_or_else(|| err("BAD_REQUEST", "当前构建未登记 USB VID/PID"))?;
            let session = tokio::time::timeout(
                Duration::from_secs(10),
                usb::Session::open(vid, pid, usb_uid.to_ascii_uppercase()),
            )
            .await
            .map_err(|_| internal("USB 握手超时"))?
            .map_err(usb_error)?;
            let info = &session.handshake;
            let device = RememberedDevice::Usb {
                usb_uid: info.device.uid.clone(),
                name: info.name.clone(),
            };
            let mut snapshot = snapshot_for(&device);
            let (major, minor, patch) = info.device.firmware;
            snapshot.fw_version = Some(format!("{major}.{minor}.{patch}"));
            snapshot.hardware_revision = Some(info.device.hardware_revision);
            snapshot.capability_bits = Some(info.capabilities.capability_bits);
            (ConnectedDevice::Usb(session), device, snapshot, None)
        }
    };

    state.devices.set(Some(connection.clone()));
    let sync = async {
        match state.engine.resync().await {
            Err(EngineError::Transport(TransportError::Usb(error)))
                if error.is_scene_rejection() =>
            {
                tracing::warn!(%error, "USB 灯牌未应用当前主题场景");
            }
            Err(error) => return Err(internal(error)),
            Ok(()) => {}
        }
        if let ConnectedDevice::Usb(session) = &connection {
            let status = session.status().await.map_err(usb_error)?;
            snapshot.usb_fault_flags = Some(status.runtime.fault_flags);
            tracing::info!(
                event = "usb_status_read",
                scene_digest = status.output.scene_digest,
                lease_remaining_ms = status.runtime.lease_remaining_ms,
                fault_flags = status.runtime.fault_flags,
                "USB 灯牌状态读回成功"
            );
        }
        remember_device(app, device.clone())
    }
    .await;
    if let Err(error) = sync {
        state.devices.set(None);
        let _ = close_output(&connection, false).await;
        return Err(error);
    }

    snapshot.connected = true;
    publish_device(app, snapshot.clone(), None);
    if snapshot.power_flags.is_some() {
        emit_device_power(app, &snapshot);
    }
    if let Some(events) = ble_events {
        spawn_ble_events(app.clone(), events, device, generation);
    } else if let ConnectedDevice::Usb(session) = connection {
        if let (Some(uid), Some(flags)) = (&snapshot.usb_uid, snapshot.usb_fault_flags) {
            if flags != 0 {
                emit_usb_fault(app, uid, flags, flags);
            }
        }
        spawn_usb_monitor(app.clone(), session, device, generation);
    }
    tracing::info!(event = "device_connected", device_name = ?snapshot.name, generation, transport = ?snapshot.transport, "设备连接与状态对齐完成");
    Ok(())
}

fn emit_device_power(app: &AppHandle, snapshot: &DeviceSnapshot) {
    let _ = app.emit(
        "device-power-changed",
        serde_json::json!({
            "capabilityBits": snapshot.capability_bits,
            "batteryMv": snapshot.battery_mv,
            "batteryPercent": snapshot.battery_percent,
            "powerSource": snapshot.power_source,
            "chargeState": snapshot.charge_state,
            "powerFlags": snapshot.power_flags,
        }),
    );
}

fn spawn_ble_events(
    app: AppHandle,
    mut events: mpsc::UnboundedReceiver<ble::BleEvent>,
    device: RememberedDevice,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let state = app.state::<AppState>();
            let _guard = state.connection_lock.lock().await;
            if state.connection_generation.load(Ordering::SeqCst) != generation {
                return;
            }
            match event {
                ble::BleEvent::PowerChanged(power) => {
                    let snapshot = {
                        let mut snapshot = state.shared.device.write().unwrap();
                        snapshot.power_source = Some(power.power_source);
                        snapshot.power_flags = Some(power.power_flags);
                        snapshot.charge_state = Some(power.charge_state);
                        snapshot.battery_mv =
                            (power.battery_mv != 0xFFFF).then_some(power.battery_mv);
                        snapshot.battery_percent =
                            (power.battery_percent != 0xFF).then_some(power.battery_percent);
                        snapshot.clone()
                    };
                    emit_device_power(&app, &snapshot);
                }
                ble::BleEvent::Fault {
                    source,
                    code,
                    context,
                } => {
                    tracing::warn!("设备故障 source={source} code={code} context={context}");
                    let _ = app.emit(
                        "device-fault",
                        serde_json::json!({ "source": source, "code": code, "context": context }),
                    );
                }
                ble::BleEvent::ButtonEvent { event, duration_ms } => {
                    tracing::info!("设备按键 event={event} duration_ms={duration_ms}（V2 展示）");
                }
                ble::BleEvent::DeviceReady(_) => {}
                ble::BleEvent::Disconnected => {
                    link_lost(&app, device, generation, false);
                    return;
                }
            }
        }
    });
}

fn emit_usb_fault(app: &AppHandle, uid: &str, flags: u16, raised: u16) {
    if raised != 0 {
        tracing::warn!(fault_flags = flags, raised, "USB 灯牌报告故障");
    }
    let _ = app.emit(
        "device-fault",
        serde_json::json!({ "usbUid": uid, "faultFlags": flags, "raised": raised }),
    );
}

fn spawn_usb_monitor(
    app: AppHandle,
    session: usb::Session,
    device: RememberedDevice,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut updates = session.fault_updates();
        let closed = session.wait_closed();
        tokio::pin!(closed);
        loop {
            tokio::select! {
                biased;
                result = &mut closed => {
                    let _guard = state.connection_lock.lock().await;
                    if state.connection_generation.load(Ordering::SeqCst) == generation {
                        tracing::warn!(?result, "USB 灯牌连接中断");
                        link_lost(&app, device, generation, true);
                    }
                    return;
                }
                Ok(()) = updates.changed() => {
                    let _guard = state.connection_lock.lock().await;
                    if state.connection_generation.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let flags = *updates.borrow_and_update();
                    let previous = state.shared.device.write().unwrap().usb_fault_flags.replace(flags);
                    if let RememberedDevice::Usb { usb_uid, .. } = &device {
                        if previous != Some(flags) {
                            emit_usb_fault(&app, usb_uid, flags, flags & !previous.unwrap_or(0));
                        }
                    }
                }
            }
        }
    });
}

fn link_lost(app: &AppHandle, device: RememberedDevice, generation: u64, keep_waiting: bool) {
    app.state::<AppState>().devices.set(None);
    publish_device(
        app,
        DeviceSnapshot {
            reconnecting: true,
            ..snapshot_for(&device)
        },
        Some("link_lost"),
    );
    spawn_recovery(app.clone(), device, generation, keep_waiting);
}

fn spawn_recovery(app: AppHandle, device: RememberedDevice, generation: u64, keep_waiting: bool) {
    tauri::async_runtime::spawn(async move {
        for attempt in 1u32.. {
            let delay = if keep_waiting {
                ble::startup_recovery_delay_secs(attempt, rand::random::<u8>())
            } else {
                ble::reconnect_delay_secs(attempt)
            };
            tokio::time::sleep(Duration::from_secs(delay)).await;
            let state = app.state::<AppState>();
            let _guard = state.connection_lock.lock().await;
            if state.connection_generation.load(Ordering::SeqCst) != generation
                || state.devices.current().is_some()
            {
                return;
            }
            match connect_locked(&app, &device, generation).await {
                Ok(()) => return,
                Err(error) => {
                    tracing::warn!(device = device.name(), attempt, error = %error.message, "设备恢复失败")
                }
            }
            if attempt == ble::FAST_RECOVERY_ATTEMPTS {
                let snapshot = DeviceSnapshot {
                    waiting_for_device: keep_waiting,
                    ..snapshot_for(&device)
                };
                let reason = if keep_waiting {
                    "startup_waiting"
                } else {
                    "reconnect_failed"
                };
                publish_device(&app, snapshot, Some(reason));
                if !keep_waiting {
                    return;
                }
            }
        }
    });
}

pub(crate) async fn connect_at_startup(app: AppHandle, device: RememberedDevice) {
    tokio::time::sleep(Duration::from_secs(1)).await;
    let state = app.state::<AppState>();
    let _guard = state.connection_lock.lock().await;
    if state
        .connection_generation
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    if let Err(error) = connect_locked(&app, &device, 1).await {
        tracing::warn!(error = %error.message, "启动时连接设备失败");
        publish_device(
            &app,
            DeviceSnapshot {
                reconnecting: true,
                ..snapshot_for(&device)
            },
            None,
        );
        spawn_recovery(app.clone(), device, 1, true);
    }
}

#[tauri::command]
pub async fn connect_device(
    app: AppHandle,
    address: Option<String>,
    usb_uid: Option<String>,
) -> CmdResult<()> {
    let target = match (address, usb_uid) {
        (Some(address), None) => RememberedDevice::Ble {
            address: ble::normalize_address(&address),
            name: String::new(),
        },
        (None, Some(uid))
            if uid.len() == 24 && uid.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            RememberedDevice::Usb {
                usb_uid: uid.to_ascii_uppercase(),
                name: String::new(),
            }
        }
        _ => return Err(err("BAD_REQUEST", "请选择一个蓝牙地址或 USB 设备")),
    };
    let state = app.state::<AppState>();
    let _guard = state.connection_lock.lock().await;
    let snapshot = state.shared.device.read().unwrap().clone();
    let selected = snapshot_for(&target);
    if snapshot.connected
        && snapshot.address == selected.address
        && snapshot.usb_uid == selected.usb_uid
    {
        return Ok(());
    }
    let generation = detach_locked(&app, true).await?;
    if snapshot.connected || snapshot.reconnecting || snapshot.waiting_for_device {
        publish_device(&app, DeviceSnapshot::default(), None);
    }
    connect_locked(&app, &target, generation).await
}

#[tauri::command]
pub async fn read_usb_status(app: AppHandle) -> CmdResult<usb::Status> {
    let state = app.state::<AppState>();
    let _guard = state.connection_lock.lock().await;
    match state.devices.current() {
        Some(ConnectedDevice::Usb(session)) => session.status().await.map_err(usb_error),
        _ => Err(err("DEVICE_NOT_CONNECTED", "请先连接 USB 灯牌")),
    }
}

async fn detach_locked(app: &AppHandle, switching: bool) -> CmdResult<u64> {
    let state = app.state::<AppState>();
    if let Some(connection) = state.devices.take() {
        if let Err(error) = close_output(&connection, switching).await {
            state.devices.set(Some(connection));
            return Err(error);
        }
    }
    Ok(state.connection_generation.fetch_add(1, Ordering::SeqCst) + 1)
}

pub(crate) fn close_usb_on_exit(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.exiting.store(true, Ordering::SeqCst);
    if let Some(connection @ ConnectedDevice::Usb(_)) = state.devices.current() {
        // Native macOS Quit reaches Exit directly. The HID worker can finish without the UI loop.
        let _ = tauri::async_runtime::block_on(close_output(&connection, false));
    }
}

#[tauri::command]
pub async fn disconnect_device(app: AppHandle) -> CmdResult<serde_json::Value> {
    let state = app.state::<AppState>();
    let _guard = state.connection_lock.lock().await;
    detach_locked(&app, false).await?;
    publish_device(&app, DeviceSnapshot::default(), Some("manual_disconnect"));
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn forget_device(app: AppHandle) -> CmdResult<serde_json::Value> {
    let state = app.state::<AppState>();
    let _guard = state.connection_lock.lock().await;
    detach_locked(&app, false).await?;
    let mut config = state.config.write().map_err(|_| internal("config 锁"))?;
    let mut updated = config.clone();
    updated.remembered_device = None;
    if let Err(error) = persist_config(&app, &updated) {
        publish_device(&app, DeviceSnapshot::default(), Some("manual_disconnect"));
        return Err(error);
    }
    *config = updated;
    let _ = app.emit("config-changed", &*config);
    publish_device(&app, DeviceSnapshot::default(), Some("forgotten"));
    Ok(serde_json::json!({ "ok": true }))
}

// ---- 控制域 ----

#[tauri::command]
pub fn trigger_state(
    app: AppHandle,
    state: String,
    _meta: Option<serde_json::Value>,
) -> CmdResult<bool> {
    let s = shared(&app);
    let applied = engine::process_event(&s, "manual", &state, None, None).map_err(internal)?;
    Ok(applied)
}

#[tauri::command]
pub async fn preview_scene(
    app: AppHandle,
    state: String,
    theme: Option<String>,
    content: Option<String>,
) -> CmdResult<()> {
    let app_state = app.state::<AppState>();
    if app_state.devices.current().is_none() {
        return Err(err("DEVICE_NOT_CONNECTED", "请先连接设备后再试听灯效"));
    }
    let engine = &app_state.engine;
    if let Some(content) = content {
        let draft = theme::load(&content).map_err(|e| err("THEME_INVALID", e.to_string()))?;
        engine
            .preview_theme(&draft, &state)
            .await
            .map_err(output_error)
    } else {
        engine
            .preview(&state, theme.as_deref())
            .await
            .map_err(output_error)
    }
}

#[tauri::command]
pub async fn reset_outputs(app: AppHandle) -> CmdResult<()> {
    let engine = &app.state::<AppState>().engine;
    engine.reset().await.map_err(output_error)?;
    let _ = app.emit(
        "business-state-changed",
        serde_json::json!({ "state": ST_IDLE }),
    );
    Ok(())
}

fn output_error(error: EngineError) -> AppError {
    match error {
        EngineError::Transport(TransportError::Usb(error)) => usb_error(error),
        error => internal(error),
    }
}

// ---- 配置域 ----

#[tauri::command]
pub fn get_config(app: AppHandle) -> CmdResult<AppConfig> {
    let state = app.state::<AppState>();
    state
        .config
        .read()
        .map(|c| c.clone())
        .map_err(|_| internal("config 锁"))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigPatch {
    pub token: Option<String>,
    pub autostart: Option<bool>,
    pub badge_orientation: Option<String>,
    pub theme_mode: Option<String>,
    pub port_preference: Option<u16>,
}

#[tauri::command]
pub async fn update_config(app: AppHandle, patch: ConfigPatch) -> CmdResult<AppConfig> {
    let s = shared(&app);
    let state = app.state::<AppState>();
    tracing::info!(
        event = "config_update_requested",
        changes_token = patch.token.is_some(),
        changes_autostart = patch.autostart.is_some(),
        changes_orientation = patch.badge_orientation.is_some(),
        changes_theme_mode = patch.theme_mode.is_some(),
        "请求更新配置"
    );

    if patch.port_preference.is_some() {
        return Err(err("BAD_REQUEST", "服务端口由 AI-Light 自动管理"));
    }

    let mut cfg = state.config.write().map_err(|_| internal("config 锁"))?;

    if let Some(token) = &patch.token {
        cfg.token = token.clone();
        let runtime_token = if token.is_empty() {
            crate::storage::runtime_token()
        } else {
            token.clone()
        };
        *s.token.write().map_err(|_| internal("token 锁"))? = Some(runtime_token.clone());
        *state
            .runtime_token
            .write()
            .map_err(|_| internal("runtime token 锁"))? = runtime_token.clone();
        let port = s.port.read().map(|value| *value).unwrap_or(25_679);
        crate::storage::write_runtime(
            port,
            &runtime_token,
            env!("CARGO_PKG_VERSION"),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_millis() as u64)
                .unwrap_or(0),
        )
        .map_err(internal)?;
    }
    if let Some(autostart) = patch.autostart {
        // 先 OS 后 config（设计方案 D-06）：OS 登录项为唯一事实源，
        // enable/disable 成功才写缓存；失败返回 AUTOSTART_FAILED，config 保持不变。
        let autolaunch = app.autolaunch();
        let os_result = if autostart {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(e) = os_result {
            return Err(err("AUTOSTART_FAILED", format!("开机自启设置失败: {e}")));
        }
        cfg.autostart = autostart;
    }
    if let Some(orientation) = &patch.badge_orientation {
        if orientation != "horizontal" && orientation != "vertical" {
            return Err(err(
                "BAD_REQUEST",
                format!("badgeOrientation 非法: {orientation}"),
            ));
        }
        cfg.badge_orientation = orientation.clone();
        crate::tray::update_orientation(&app, orientation);
    }
    if let Some(mode) = &patch.theme_mode {
        if mode != "light" && mode != "dark" && mode != "system" {
            return Err(err("BAD_REQUEST", format!("themeMode 非法: {mode}")));
        }
        cfg.theme_mode = mode.clone();
    }
    persist_config(&app, &cfg)?;
    let _ = app.emit("config-changed", &*cfg);
    tracing::info!(event = "config_updated", "配置更新完成");
    Ok(cfg.clone())
}

// ---- 内部辅助 ----

fn user_theme_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let _ = app;
    crate::storage::themes_dir()
}

fn builtin_theme_content(name: &str) -> Option<&'static str> {
    theme::BUILTIN_THEMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, c)| *c)
}

fn valid_theme_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn resolve_theme(app: &AppHandle, name: &str) -> Result<ThemeFile, String> {
    if let Some(content) = builtin_theme_content(name) {
        return theme::load(content).map_err(|e| e.to_string());
    }
    let dir = user_theme_dir(app)?;
    let path = dir.join(format!("{name}.ailight-theme.json"));
    let content = std::fs::read_to_string(&path).map_err(|_| format!("主题不存在: {name}"))?;
    theme::load(&content).map_err(|e| e.to_string())
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let _ = app;
    crate::storage::config_path()
}

fn persist_config(app: &AppHandle, cfg: &AppConfig) -> CmdResult<()> {
    let path = config_path(app).map_err(internal)?;
    crate::storage::write_private_file(&path, cfg.to_json()).map_err(internal)
}

fn persist_active_theme(app: &AppHandle, name: &str) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if let Ok(mut cfg) = state.config.write() {
        cfg.active_theme = name.to_string();
        persist_config(app, &cfg)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{prepare_theme_export, valid_integration_tool, valid_theme_name};

    #[test]
    fn integration_tool_allowlist_includes_supported_tools() {
        assert!(valid_integration_tool("qoder"));
        assert!(valid_integration_tool("trae"));
        assert!(!valid_integration_tool("unknown"));
    }

    #[test]
    fn user_theme_name_rejects_path_components() {
        assert!(valid_theme_name("my-theme_2"));
        assert!(!valid_theme_name("../default"));
        assert!(!valid_theme_name("nested/theme"));
        assert!(!valid_theme_name(""));
    }

    #[test]
    fn export_rejects_invalid_builtin_missing_and_corrupt_themes() {
        assert_eq!(
            prepare_theme_export("../theme", None).unwrap_err().code,
            "BAD_REQUEST"
        );
        assert_eq!(
            prepare_theme_export("default", None).unwrap_err().code,
            "THEME_BUILTIN"
        );
        assert_eq!(
            prepare_theme_export("mine", None).unwrap_err().code,
            "NOT_FOUND"
        );
        assert_eq!(
            prepare_theme_export("mine", Some("{broken".into()))
                .unwrap_err()
                .code,
            "THEME_INVALID"
        );
    }

    #[test]
    fn export_preserves_valid_user_theme_content() {
        let content = r#"{"theme":{"name":"mine","version":1},"scenes":{"off":{"leds":[null,null,null]}},"states":{"IDLE":{"scene":"off"}}}"#;
        assert_eq!(
            prepare_theme_export("mine", Some(content.into())).unwrap(),
            content
        );
    }
}

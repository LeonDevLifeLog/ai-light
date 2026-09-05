//! Tauri commands（ipc-contract V1.0 §2：P1 全部 14 个 + 工具链 4 个，设计方案 §7）

use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::mpsc;

use ailight_core::arbiter::ST_IDLE;
use ailight_core::ble::{self, BleDeviceInfo};
use ailight_core::config::{AppConfig, RememberedDevice};
use ailight_core::engine::{self, EngineError};
use ailight_core::hook_server::{BusinessSnapshot, DeviceSnapshot, ServiceSnapshot, SharedState};
use ailight_core::theme::{self, ThemeFile};

use crate::toolchain::model::states;
use crate::toolchain::model::{
    AdapterUpdateInfo, ResolvedToolchain, ToolKind, ToolchainOverrides, ToolchainStatus,
};
use crate::toolchain::{runner, validate, ToolchainError};
use crate::AppState;

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

// ---- Adapter CLI 集成（统一走 ToolchainService / ProcessRunner，设计方案 §4） ----

fn valid_integration_tool(tool: &str) -> bool {
    matches!(tool, "claude-code" | "codex" | "qoder" | "workbuddy")
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
    // 当前业务非 IDLE → 用新主题重放（幂等对齐，ipc-contract §2.2 副作用）
    let state_now = s
        .arbiter
        .read()
        .map(|g| g.current().state.clone())
        .unwrap_or_default();
    if state_now != ST_IDLE {
        let s2 = s.clone();
        tauri::async_runtime::spawn(async move {
            let _ = engine::compile_current(&s2)
                .and_then(|scene| s2.send_outbound(scene).map_err(EngineError::State));
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
    Ok(serde_json::json!({ "ok": true }))
}

// ---- 设备域 ----

#[tauri::command]
pub async fn scan_devices(_app: AppHandle) -> CmdResult<Vec<BleDeviceInfo>> {
    let adapter = ble::default_adapter().await.map_err(internal)?;
    // recognized（广播名前缀，协议 §2.1）由 ble::scan 计算；服务 UUID 识别在连接后
    ble::scan(&adapter, 5).await.map_err(internal)
}

/// 连接设备（供 command 与启动自动连接共用）
pub(crate) async fn connect_device_internal(
    app: &AppHandle,
    address: &str,
    name: &str,
    generation: u64,
) -> Result<(), String> {
    const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
    let addr_norm = ble::normalize_address(address);
    tracing::info!(address = %addr_norm, generation, "BLE 连接请求开始");

    let result = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let state = app.state::<AppState>();
        let _connection_guard = state.connection_lock.lock().await;
        if state.connection_generation.load(Ordering::SeqCst) != generation {
            return Err("连接请求已取消".to_string());
        }

        tracing::info!(address = %addr_norm, "BLE 获取默认适配器");
        let adapter = ble::default_adapter().await.map_err(|e| e.to_string())?;
        tracing::info!(address = %addr_norm, "BLE 扫描设备（4 秒）");
        let _ = ble::scan(&adapter, 4).await.map_err(|e| e.to_string())?;
        tracing::info!(address = %addr_norm, "BLE 扫描完成，开始连接与握手");
        let (ble_io, actual_name, handshake) = ble::connect_to_address(&adapter, &addr_norm)
            .await
            .map_err(|e| e.to_string())?;
        let display_name = if name.is_empty() {
            actual_name
        } else {
            name.to_string()
        };
        attach_device(
            app,
            ble_io,
            handshake,
            addr_norm.clone(),
            display_name,
            generation,
        )
        .await
    })
    .await
    .map_err(|_| format!("连接超时（{} 秒）", CONNECT_TIMEOUT.as_secs()))?;

    match &result {
        Ok(()) => tracing::info!(address = %addr_norm, generation, "BLE 连接请求完成"),
        Err(error) => tracing::warn!(address = %addr_norm, generation, %error, "BLE 连接请求失败"),
    }
    result
}

/// 连接成功后的统一装配：快照 → 事件 → 持久化 → 热切换 → resync → 设备事件循环
pub(crate) async fn attach_device(
    app: &AppHandle,
    mut ble_io: ble::BleIo,
    handshake: ble::HandshakeInfo,
    address: String,
    name: String,
    generation: u64,
) -> Result<(), String> {
    let s = shared(app);
    let state = app.state::<AppState>();

    if state.connection_generation.load(Ordering::SeqCst) != generation {
        ble_io.disconnect().await.map_err(|e| e.to_string())?;
        return Err("连接请求已取消".into());
    }

    // 设备快照（含握手信息：固件 / 硬件变体 / 电量）
    {
        let mut dev = s.device.write().map_err(|_| "device 锁".to_string())?;
        dev.connected = true;
        dev.reconnecting = false;
        dev.address = Some(address.clone());
        dev.name = Some(name.clone());
        dev.fw_version = Some(format!(
            "{}.{}.{}",
            handshake.device_info.fw.0, handshake.device_info.fw.1, handshake.device_info.fw.2
        ));
        dev.hardware_variant = Some(handshake.device_info.hardware_variant);
        dev.capability_bits = Some(handshake.capabilities.capability_bits);
        if let Some(p) = &handshake.power {
            dev.power_source = Some(p.power_source);
            dev.power_flags = Some(p.power_flags);
            dev.charge_state = Some(p.charge_state);
            dev.battery_mv = (p.battery_mv != 0xFFFF).then_some(p.battery_mv);
            dev.battery_percent = (p.battery_percent != 0xFF).then_some(p.battery_percent);
        }
    }
    let _ = app.emit(
        "device-connection-changed",
        serde_json::json!({
            "connected": true,
            "reconnecting": false,
            "address": address,
            "name": name,
        }),
    );
    crate::tray::update_device(app, true, Some(&name));
    if let Some(p) = &handshake.power {
        let _ = app.emit(
            "device-power-changed",
            serde_json::json!({
                "capabilityBits": handshake.capabilities.capability_bits,
                "batteryMv": (p.battery_mv != 0xFFFF).then_some(p.battery_mv),
                "batteryPercent": (p.battery_percent != 0xFF).then_some(p.battery_percent),
                "powerSource": p.power_source,
                "chargeState": p.charge_state,
                "powerFlags": p.power_flags,
            }),
        );
    }

    // 记住设备
    if let Ok(mut cfg) = state.config.write() {
        cfg.remembered_device = Some(RememberedDevice {
            address: address.clone(),
            name: name.clone(),
        });
        persist_config(app, &cfg).map_err(|e| e.message)?;
    }

    // 热切换设备（Engine 无需重建）
    let events_rx = ble_io.take_events();
    let ble_io = std::sync::Arc::new(ble_io);
    state.device_io.set(Some(ble_io.clone())).await;
    *state.active_ble.lock().await = Some(ble_io);
    // 重连对齐：重发当前业务 SCENE（协议 §15.5）
    state.engine.resync().await.map_err(|e| e.to_string())?;

    if let Some(rx) = events_rx {
        spawn_device_event_loop(app.clone(), rx, address, name, generation);
    }
    Ok(())
}

/// 消费设备主动事件：电源变化 / 故障 → Tauri events；断开 → 快照 + 退避重连
fn spawn_device_event_loop(
    app: AppHandle,
    mut events: mpsc::UnboundedReceiver<ble::BleEvent>,
    address: String,
    name: String,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = events.recv().await {
            if app
                .state::<AppState>()
                .connection_generation
                .load(Ordering::SeqCst)
                != generation
            {
                return;
            }
            match ev {
                ble::BleEvent::PowerChanged(p) => {
                    let s = shared(&app);
                    let capability_bits = if let Ok(mut dev) = s.device.write() {
                        dev.power_source = Some(p.power_source);
                        dev.power_flags = Some(p.power_flags);
                        dev.charge_state = Some(p.charge_state);
                        dev.battery_mv = (p.battery_mv != 0xFFFF).then_some(p.battery_mv);
                        dev.battery_percent =
                            (p.battery_percent != 0xFF).then_some(p.battery_percent);
                        dev.capability_bits
                    } else {
                        None
                    };
                    let _ = app.emit(
                        "device-power-changed",
                        serde_json::json!({
                            "capabilityBits": capability_bits,
                            "batteryMv": (p.battery_mv != 0xFFFF).then_some(p.battery_mv),
                            "batteryPercent": (p.battery_percent != 0xFF).then_some(p.battery_percent),
                            "powerSource": p.power_source,
                            "chargeState": p.charge_state,
                            "powerFlags": p.power_flags,
                        }),
                    );
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
                ble::BleEvent::DeviceReady(_) => {
                    // 握手阶段已消费；此处分支仅为穷尽匹配
                }
                ble::BleEvent::Disconnected => {
                    tracing::warn!("设备断开: {name}");
                    let s = shared(&app);
                    if let Ok(mut dev) = s.device.write() {
                        dev.connected = false;
                        dev.reconnecting = true;
                        dev.power_source = None;
                        dev.power_flags = None;
                        dev.charge_state = None;
                        dev.battery_mv = None;
                        dev.battery_percent = None;
                    }
                    let state = app.state::<AppState>();
                    state.device_io.set(None).await;
                    *state.active_ble.lock().await = None;
                    let _ = app.emit(
                        "device-connection-changed",
                        serde_json::json!({
                            "connected": false,
                            "reconnecting": true,
                            "reason": "link_lost",
                            "address": address,
                            "name": name,
                        }),
                    );
                    crate::tray::update_device(&app, false, None);
                    spawn_reconnect(app, address, name, 1, generation);
                    return;
                }
            }
        }
    });
}

/// 断连退避重连：延迟后重试 connect_device_internal；期间已手动连接则放弃
pub(crate) fn spawn_reconnect(
    app: AppHandle,
    address: String,
    name: String,
    attempt: u32,
    generation: u64,
) {
    const MAX_RECONNECT_ATTEMPTS: u32 = 5;
    if app
        .state::<AppState>()
        .connection_generation
        .load(Ordering::SeqCst)
        != generation
    {
        return;
    }
    if attempt > MAX_RECONNECT_ATTEMPTS {
        tracing::warn!("设备 {name} 重连达到上限，停止自动重连");
        if let Ok(mut dev) = shared(&app).device.write() {
            dev.reconnecting = false;
        }
        let _ = app.emit(
            "device-connection-changed",
            serde_json::json!({
                "connected": false,
                "reconnecting": false,
                "reason": "reconnect_failed",
                "address": address,
                "name": name,
            }),
        );
        return;
    }
    if let Ok(mut dev) = shared(&app).device.write() {
        dev.connected = false;
        dev.reconnecting = true;
        dev.address = Some(address.clone());
        dev.name = Some(name.clone());
    }
    tauri::async_runtime::spawn(async move {
        let delay = std::time::Duration::from_secs(ble::reconnect_delay_secs(attempt));
        tracing::info!("{delay:?} 后尝试重连 {name}（第 {attempt}/5 次）");
        tokio::time::sleep(delay).await;
        let state = app.state::<AppState>();
        if state.connection_generation.load(Ordering::SeqCst) != generation {
            return;
        }
        if state.device_io.is_connected().await {
            return;
        }
        match connect_device_internal(&app, &address, &name, generation).await {
            Ok(()) => tracing::info!("设备 {name} 重连成功"),
            Err(e) => {
                tracing::warn!("设备 {name} 重连第 {attempt} 次失败: {e}");
                spawn_reconnect(app, address, name, attempt + 1, generation);
            }
        }
    });
}

#[tauri::command]
pub async fn connect_device(app: AppHandle, address: String) -> CmdResult<()> {
    let generation = app
        .state::<AppState>()
        .connection_generation
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    connect_device_internal(&app, &address, "", generation)
        .await
        .map_err(internal)
}

async fn disconnect_current(app: &AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let current = state.active_ble.lock().await.clone();
    if let Some(ble) = current {
        ble.disconnect()
            .await
            .map_err(|e| err("DEVICE_DISCONNECT_FAILED", e.to_string()))?;
    }

    state.connection_generation.fetch_add(1, Ordering::SeqCst);
    state.device_io.set(None).await;
    *state.active_ble.lock().await = None;
    if let Ok(mut device) = state.shared.device.write() {
        *device = DeviceSnapshot::default();
    }
    Ok(())
}

fn emit_disconnected(app: &AppHandle, reason: &str) {
    let _ = app.emit(
        "device-connection-changed",
        serde_json::json!({
            "connected": false,
            "reconnecting": false,
            "reason": reason,
            "address": null,
            "name": null,
        }),
    );
    crate::tray::update_device(app, false, None);
}

#[tauri::command]
pub async fn disconnect_device(app: AppHandle) -> CmdResult<serde_json::Value> {
    disconnect_current(&app).await?;
    emit_disconnected(&app, "manual_disconnect");
    Ok(serde_json::json!({ "ok": true }))
}

#[tauri::command]
pub async fn forget_device(app: AppHandle) -> CmdResult<serde_json::Value> {
    disconnect_current(&app).await?;
    let state = app.state::<AppState>();
    let mut candidate = state
        .config
        .read()
        .map_err(|_| internal("config 锁"))?
        .clone();
    candidate.remembered_device = None;
    if let Err(e) = persist_config(&app, &candidate) {
        emit_disconnected(&app, "manual_disconnect");
        return Err(e);
    }
    *state.config.write().map_err(|_| internal("config 锁"))? = candidate.clone();
    let _ = app.emit("config-changed", &candidate);
    emit_disconnected(&app, "forgotten");
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
    if !app_state.device_io.is_connected().await {
        return Err(err("DEVICE_NOT_CONNECTED", "请先连接设备后再试听灯效"));
    }
    let engine = &app_state.engine;
    if let Some(content) = content {
        let draft = theme::load(&content).map_err(|e| err("THEME_INVALID", e.to_string()))?;
        engine.preview_theme(&draft, &state).await.map_err(internal)
    } else {
        engine
            .preview(&state, theme.as_deref())
            .await
            .map_err(internal)
    }
}

#[tauri::command]
pub async fn reset_outputs(app: AppHandle) -> CmdResult<()> {
    let engine = &app.state::<AppState>().engine;
    engine.reset().await.map_err(internal)?;
    let _ = app.emit(
        "business-state-changed",
        serde_json::json!({ "state": ST_IDLE }),
    );
    Ok(())
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
    fn integration_tool_allowlist_includes_qoder() {
        assert!(valid_integration_tool("qoder"));
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

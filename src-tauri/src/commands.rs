//! Tauri commands（ipc-contract V1.0 §2：P1 全部 12 个）

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use ailight_core::arbiter::{ArbitrationMode, ST_IDLE};
use ailight_core::ble::{self, BleDeviceInfo};
use ailight_core::config::{AppConfig, RememberedDevice};
use ailight_core::engine::{self, EngineError};
use ailight_core::hook_server::{
    BusinessSnapshot, DeviceSnapshot, ServiceSnapshot, SharedState,
};
use ailight_core::theme::{self, ThemeFile};

use crate::AppState;

// ---- 错误模型（ipc-contract §4） ----

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

type CmdResult<T> = Result<T, AppError>;

fn err(code: &'static str, message: impl Into<String>) -> AppError {
    AppError { code, message: message.into() }
}

fn internal(e: impl std::fmt::Display) -> AppError {
    err("INTERNAL", e.to_string())
}

fn shared(app: &AppHandle) -> std::sync::Arc<SharedState> {
    app.state::<AppState>().shared.clone()
}

// ---- 状态查询 ----

#[derive(Serialize)]
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
        port: s.port.read().map(|p| *p).unwrap_or(47800),
        token_enabled: s.token.read().map(|t| t.is_some()).unwrap_or(false),
    };
    let device = s.device.read().map(|d| d.clone()).unwrap_or_default();
    let themes = theme::builtin_theme_names().into_iter().map(String::from).collect();
    let active_theme = s.theme_name.read().map(|t| t.clone()).unwrap_or_default();
    Ok(AppStateSnapshot { service, device, business, themes, active_theme })
}

// ---- 主题域 ----

#[derive(Serialize)]
pub struct ThemeMeta {
    pub name: String,
    pub builtin: bool,
}

#[tauri::command]
pub fn get_themes(app: AppHandle) -> CmdResult<Vec<ThemeMeta>> {
    let mut names: Vec<ThemeMeta> = theme::builtin_theme_names()
        .into_iter()
        .map(|n| ThemeMeta { name: n.to_string(), builtin: true })
        .collect();
    if let Ok(dir) = user_theme_dir(&app) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().and_then(|x| x.to_str()) == Some("ailight-theme") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(ThemeMeta { name: name.to_string(), builtin: false });
                    }
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
    *s.theme_name.write().map_err(|_| internal("theme_name 锁"))? = name.clone();
    // 持久化
    persist_active_theme(&app, &name)?;
    let _ = app.emit("theme-changed", serde_json::json!({ "name": name }));
    // 当前业务非 IDLE → 用新主题重放（幂等对齐，ipc-contract §2.2 副作用）
    let state_now = s.arbiter.read().map(|g| g.current().state.clone()).unwrap_or_default();
    if state_now != ST_IDLE {
        let s2 = s.clone();
        tauri::async_runtime::spawn(async move {
            let _ = engine::compile_current(&s2).and_then(|scene| {
                s2.send_outbound(scene)
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
    Ok(name)
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
) -> Result<(), String> {
    let s = shared(app);
    let adapter = ble::default_adapter().await.map_err(|e| e.to_string())?;
    let _ = ble::scan(&adapter, 4).await.map_err(|e| e.to_string())?;
    let addr_norm = ble::normalize_address(address);
    let (ble_io, actual_name) =
        ble::connect_to_address(&adapter, &addr_norm).await.map_err(|e| e.to_string())?;
    let display_name = if name.is_empty() { actual_name } else { name.to_string() };

    // 热切换设备（Engine 无需重建）
    let state = app.state::<AppState>();
    state.device_io.set(Some(std::sync::Arc::new(ble_io))).await;

    // 设备快照 + 事件
    {
        let mut dev = s.device.write().map_err(|_| "device 锁".to_string())?;
        dev.connected = true;
        dev.address = Some(addr_norm.clone());
        dev.name = Some(display_name.clone());
    }
    let _ = app.emit(
        "device-connection-changed",
        serde_json::json!({ "connected": true, "address": addr_norm, "name": display_name }),
    );
    // 记住设备
    if let Ok(mut cfg) = state.config.write() {
        cfg.remembered_device =
            Some(RememberedDevice { address: addr_norm.clone(), name: display_name });
        persist_config(app, &cfg).map_err(|e| e.message)?;
    }
    // 重连对齐：重发当前业务 SCENE（协议 §15.5）
    state.engine.resync().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn connect_device(app: AppHandle, address: String) -> CmdResult<()> {
    let name = address.clone();
    connect_device_internal(&app, &address, &name).await.map_err(internal)
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
pub async fn preview_scene(app: AppHandle, state: String, theme: Option<String>) -> CmdResult<()> {
    let engine = &app.state::<AppState>().engine;
    engine.preview(&state, theme.as_deref()).await.map_err(internal)
}

#[tauri::command]
pub async fn reset_outputs(app: AppHandle) -> CmdResult<()> {
    let engine = &app.state::<AppState>().engine;
    engine.reset().await.map_err(internal)?;
    let _ = app.emit("business-state-changed", serde_json::json!({ "state": ST_IDLE }));
    Ok(())
}

// ---- 配置域 ----

#[tauri::command]
pub fn get_config(app: AppHandle) -> CmdResult<AppConfig> {
    let state = app.state::<AppState>();
    state.config.read().map(|c| c.clone()).map_err(|_| internal("config 锁"))
}

#[derive(serde::Deserialize)]
pub struct ConfigPatch {
    pub arbitration_mode: Option<String>,
    pub token: Option<String>,
    pub autostart: Option<bool>,
}

#[tauri::command]
pub fn update_config(app: AppHandle, patch: ConfigPatch) -> CmdResult<AppConfig> {
    let s = shared(&app);
    let state = app.state::<AppState>();
    let mut cfg = state.config.write().map_err(|_| internal("config 锁"))?;

    if let Some(mode) = &patch.arbitration_mode {
        let m = ArbitrationMode::from_str(mode)
            .ok_or_else(|| err("BAD_REQUEST", format!("arbitration_mode 非法: {mode}")))?;
        cfg.arbitration_mode = mode.clone();
        // 立即生效（引擎热切换）
        state.engine.set_arbitration_mode(m);
    }
    if let Some(token) = &patch.token {
        cfg.token = token.clone();
        *s.token.write().map_err(|_| internal("token 锁"))? =
            if token.is_empty() { None } else { Some(token.clone()) };
    }
    if let Some(autostart) = patch.autostart {
        cfg.autostart = autostart;
    }
    persist_config(&app, &cfg)?;
    Ok(cfg.clone())
}

// ---- 内部辅助 ----

fn user_theme_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("themes"))
        .map_err(|e| e.to_string())
}

fn builtin_theme_content(name: &str) -> Option<&'static str> {
    theme::BUILTIN_THEMES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
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
    app.path()
        .app_config_dir()
        .map(|d| d.join("config.json"))
        .map_err(|e| e.to_string())
}

fn persist_config(app: &AppHandle, cfg: &AppConfig) -> CmdResult<()> {
    let path = config_path(app).map_err(internal)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(internal)?;
    }
    std::fs::write(&path, cfg.to_json()).map_err(internal)
}

fn persist_active_theme(app: &AppHandle, name: &str) -> CmdResult<()> {
    let state = app.state::<AppState>();
    if let Ok(mut cfg) = state.config.write() {
        cfg.active_theme = name.to_string();
        persist_config(app, &cfg)?;
    }
    Ok(())
}

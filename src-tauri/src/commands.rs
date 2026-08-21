//! Tauri commands（ipc-contract V1.0 §2：P1 全部 12 个）

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tokio::sync::mpsc;

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
#[serde(rename_all = "camelCase")]
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
                if let Some(name) = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .and_then(|value| value.strip_suffix(".ailight-theme.json"))
                {
                    names.push(ThemeMeta { name: name.to_string(), builtin: false });
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
    crate::tray::update_theme(&app, &name);
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
    let adapter = ble::default_adapter().await.map_err(|e| e.to_string())?;
    let _ = ble::scan(&adapter, 4).await.map_err(|e| e.to_string())?;
    let addr_norm = ble::normalize_address(address);
    let (ble_io, actual_name, handshake) = ble::connect_to_address(&adapter, &addr_norm)
        .await
        .map_err(|e| e.to_string())?;
    let display_name = if name.is_empty() { actual_name } else { name.to_string() };
    attach_device(app, ble_io, handshake, addr_norm, display_name).await
}

/// 连接成功后的统一装配：快照 → 事件 → 持久化 → 热切换 → resync → 设备事件循环
pub(crate) async fn attach_device(
    app: &AppHandle,
    mut ble_io: ble::BleIo,
    handshake: ble::HandshakeInfo,
    address: String,
    name: String,
) -> Result<(), String> {
    let s = shared(app);
    let state = app.state::<AppState>();

    // 设备快照（含握手信息：固件 / 硬件变体 / 电量）
    {
        let mut dev = s.device.write().map_err(|_| "device 锁".to_string())?;
        dev.connected = true;
        dev.address = Some(address.clone());
        dev.name = Some(name.clone());
        dev.fw_version = Some(format!(
            "{}.{}.{}",
            handshake.device_info.fw.0, handshake.device_info.fw.1, handshake.device_info.fw.2
        ));
        dev.hardware_variant = Some(handshake.device_info.hardware_variant);
        if let Some(p) = &handshake.power {
            dev.power_source = Some(p.power_source);
            dev.power_flags = Some(p.power_flags);
            dev.charge_state = Some(p.charge_state);
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
                "batteryPercent": (p.battery_percent != 0xFF).then_some(p.battery_percent),
                "powerSource": p.power_source,
                "chargeState": p.charge_state,
                "powerFlags": p.power_flags,
            }),
        );
    }

    // 记住设备
    if let Ok(mut cfg) = state.config.write() {
        cfg.remembered_device =
            Some(RememberedDevice { address: address.clone(), name: name.clone() });
        persist_config(app, &cfg).map_err(|e| e.message)?;
    }

    // 热切换设备（Engine 无需重建）
    let events_rx = ble_io.take_events();
    state.device_io.set(Some(std::sync::Arc::new(ble_io))).await;
    // 重连对齐：重发当前业务 SCENE（协议 §15.5）
    state.engine.resync().await.map_err(|e| e.to_string())?;

    if let Some(rx) = events_rx {
        spawn_device_event_loop(app.clone(), rx, address, name);
    }
    Ok(())
}

/// 消费设备主动事件：电源变化 / 故障 → Tauri events；断开 → 快照 + 退避重连
fn spawn_device_event_loop(
    app: AppHandle,
    mut events: mpsc::UnboundedReceiver<ble::BleEvent>,
    address: String,
    name: String,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = events.recv().await {
            match ev {
                ble::BleEvent::PowerChanged(p) => {
                    let s = shared(&app);
                    if let Ok(mut dev) = s.device.write() {
                        dev.power_source = Some(p.power_source);
                        dev.power_flags = Some(p.power_flags);
                        dev.charge_state = Some(p.charge_state);
                        dev.battery_percent = (p.battery_percent != 0xFF).then_some(p.battery_percent);
                    }
                    let _ = app.emit(
                        "device-power-changed",
                        serde_json::json!({
                            "batteryPercent": (p.battery_percent != 0xFF).then_some(p.battery_percent),
                            "powerSource": p.power_source,
                            "chargeState": p.charge_state,
                            "powerFlags": p.power_flags,
                        }),
                    );
                }
                ble::BleEvent::Fault { source, code, context } => {
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
                        dev.power_source = None;
                        dev.power_flags = None;
                        dev.charge_state = None;
                        dev.battery_percent = None;
                    }
                    let state = app.state::<AppState>();
                    state.device_io.set(None).await;
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
                    spawn_reconnect(app, address, name, 1);
                    return;
                }
            }
        }
    });
}

/// 断连退避重连：延迟后重试 connect_device_internal；期间已手动连接则放弃
pub(crate) fn spawn_reconnect(app: AppHandle, address: String, name: String, attempt: u32) {
    const MAX_RECONNECT_ATTEMPTS: u32 = 5;
    if attempt > MAX_RECONNECT_ATTEMPTS {
        tracing::warn!("设备 {name} 重连达到上限，停止自动重连");
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
    tauri::async_runtime::spawn(async move {
        let delay = std::time::Duration::from_secs(ble::reconnect_delay_secs(attempt));
        tracing::info!("{delay:?} 后尝试重连 {name}（第 {attempt}/5 次）");
        tokio::time::sleep(delay).await;
        let state = app.state::<AppState>();
        if state.device_io.is_connected().await {
            return;
        }
        match connect_device_internal(&app, &address, &name).await {
            Ok(()) => tracing::info!("设备 {name} 重连成功"),
            Err(e) => {
                tracing::warn!("设备 {name} 重连第 {attempt} 次失败: {e}");
                spawn_reconnect(app, address, name, attempt + 1);
            }
        }
    });
}

#[tauri::command]
pub async fn connect_device(app: AppHandle, address: String) -> CmdResult<()> {
    connect_device_internal(&app, &address, "").await.map_err(internal)
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
    let engine = &app.state::<AppState>().engine;
    if let Some(content) = content {
        let draft = theme::load(&content).map_err(|e| err("THEME_INVALID", e.to_string()))?;
        engine.preview_theme(&draft, &state).await.map_err(internal)
    } else {
        engine.preview(&state, theme.as_deref()).await.map_err(internal)
    }
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
#[serde(rename_all = "camelCase")]
pub struct ConfigPatch {
    pub arbitration_mode: Option<String>,
    pub token: Option<String>,
    pub autostart: Option<bool>,
    pub badge_orientation: Option<String>,
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
            return Err(err("BAD_REQUEST", format!("badgeOrientation 非法: {orientation}")));
        }
        cfg.badge_orientation = orientation.clone();
        crate::tray::update_orientation(&app, orientation);
    }
    persist_config(&app, &cfg)?;
    let _ = app.emit("config-changed", &*cfg);
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

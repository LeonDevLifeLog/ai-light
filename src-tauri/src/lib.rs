//! AI-Light Tauri 应用入口：装配 core 模块、注册 commands/events

mod commands;
mod storage;
mod toolchain;
mod tray;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use ailight_core::ble::DeviceIo;
use ailight_core::config::{AppConfig, DEFAULT_PORT};
use ailight_core::engine::Engine;
use ailight_core::hook_server::SharedState;
use ailight_core::{logging, theme};

/// 应用级共享状态（KAD-03：Rust 唯一事实源）
pub struct AppState {
    pub shared: Arc<SharedState>,
    pub engine: Engine,
    pub device_io: Arc<DeviceIo>,
    pub config: RwLock<AppConfig>,
    pub hook_server: tokio::sync::Mutex<Option<ailight_core::hook_server::HookServer>>,
    pub active_ble: tokio::sync::Mutex<Option<Arc<ailight_core::ble::BleIo>>>,
    pub connection_lock: tokio::sync::Mutex<()>,
    pub connection_generation: AtomicU64,
    pub runtime_token: RwLock<String>,
    /// Node/npm/Adapter 工具链解析（设计方案 §4：ToolchainService）
    pub toolchain: toolchain::ToolchainService,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 单实例（KAD-06）：聚焦已有窗口
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // 开机自启（设计方案 D-03/D-07）：官方插件，macOS 走 LaunchAgent；
        // `--autostart` 参数使登录启动可辨识，本期行为与手动启动一致。
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--autostart"])
                .build(),
        )
        .setup(|app| {
            // macOS：仅菜单栏常驻（Accessory），Dock 不显示图标——关窗 = 隐藏、退出只经托盘
            // （KAD-06：托盘常驻与窗口生命周期解耦，避免从 Dock 退出连带终止托盘）
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 日志（KAD-05）
            let log_dir = storage::logs_dir().ok();
            let _ = logging::init(log_dir.as_deref(), "info");

            // 配置加载（KAD-04）
            let legacy_config_dir = app.path().app_config_dir()?;
            storage::migrate_legacy_config(&legacy_config_dir).map_err(std::io::Error::other)?;
            let cfg_path = storage::config_path().map_err(std::io::Error::other)?;
            let (mut config, warn) = if cfg_path.exists() {
                match std::fs::read_to_string(&cfg_path) {
                    Ok(c) => AppConfig::load(&c),
                    Err(e) => (AppConfig::default(), Some(format!("读取 config 失败: {e}"))),
                }
            } else {
                (AppConfig::default(), None)
            };
            if let Some(w) = warn {
                eprintln!("config: {w}");
            }

            // 共享状态 + 主题
            let shared = SharedState::new(env!("CARGO_PKG_VERSION"), now_ms);
            let theme = theme::load_builtin(&config.active_theme)
                .or_else(|| theme::load_builtin("default"))
                .expect("内置 default 主题必须合法");
            *shared.theme.write().unwrap() = Some(theme);
            *shared.theme_name.write().unwrap() = config.active_theme.clone();
            let runtime_token = if config.token.is_empty() {
                storage::runtime_token()
            } else {
                config.token.clone()
            };
            *shared.token.write().unwrap() = Some(runtime_token.clone());

            // 设备代理 + 引擎
            let device_io = DeviceIo::new();
            // Engine::new 内部使用 tokio::spawn，必须处于 runtime 上下文；
            // setup 回调运行在 AppKit 主线程（不在 runtime 上下文），故显式 enter。
            // 见 docs/decisions/ADR-0003 / docs/specs/architecture.md KAD-08。
            let engine = {
                let _guard = tauri::async_runtime::handle().inner().enter();
                Engine::new(shared.clone(), device_io.clone())
            };

            // events 桥接：轮询仲裁状态（含 hold 回落）→ 前端 emit
            let handle = app.handle().clone();
            let ev_shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                let mut last: Option<ailight_core::arbiter::BusinessState> = None;
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    let _ = ev_shared.tick(); // hold 回落
                    let cur = ev_shared.arbiter.read().map(|g| g.current().clone()).ok();
                    if cur.is_some() && cur != last {
                        let c = cur.clone().unwrap();
                        let _ = handle.emit("business-state-changed", &c);
                        crate::tray::update_status(&handle, &c.state);
                        last = cur;
                    }
                }
            });

            // 开机自启校准（设计方案 D-05）：OS 登录项为唯一事实源，config 只做启动校准缓存。
            // 插件 setup 已在 Builder::build 阶段（initialize_plugins）完成，此处可安全读取；
            // is_enabled 失败不阻塞启动，保留本地缓存值。
            match app.autolaunch().is_enabled() {
                Ok(os_enabled) => {
                    if os_enabled != config.autostart {
                        tracing::info!(
                            os_enabled,
                            cached = config.autostart,
                            "autostart 校准：以 OS 登录项为准"
                        );
                        config.autostart = os_enabled;
                    }
                }
                Err(e) => eprintln!("autostart 校准失败（保留本地缓存）: {e}"),
            }

            let preferred_port = DEFAULT_PORT;
            app.manage(AppState {
                shared,
                engine,
                device_io,
                config: RwLock::new(config),
                hook_server: tokio::sync::Mutex::new(None),
                active_ble: tokio::sync::Mutex::new(None),
                connection_lock: tokio::sync::Mutex::new(()),
                connection_generation: AtomicU64::new(0),
                runtime_token: RwLock::new(runtime_token),
                toolchain: toolchain::ToolchainService::new(),
            });

            // L1 HTTP 接入服务：启动期允许从首选端口向后退避。
            let hs_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let state = hs_handle.state::<AppState>();
                match ailight_core::hook_server::serve(state.shared.clone(), preferred_port).await {
                    Ok(server) => {
                        let port = server.port;
                        let mut slot = state.hook_server.lock().await;
                        if slot.is_some() {
                            server.stop();
                            return;
                        }
                        if let Ok(mut current) = state.shared.port.write() {
                            *current = port;
                        }
                        *slot = Some(server);
                        let runtime_token = state
                            .runtime_token
                            .read()
                            .map(|value| value.clone())
                            .unwrap_or_default();
                        if let Err(error) = storage::write_runtime(
                            port,
                            &runtime_token,
                            env!("CARGO_PKG_VERSION"),
                            now_ms(),
                        ) {
                            tracing::error!("写入 Adapter runtime 失败: {error}");
                        }
                        tracing::info!("hook server 127.0.0.1:{port}");
                    }
                    Err(e) => eprintln!("hook server 启动失败: {e}"),
                }
            });

            // 托盘常驻（KAD-06）：图标 + 菜单 + 动态状态文字
            let tray_state = tray::init(app.handle())?;
            app.manage(tray_state);
            {
                let app_state = app.state::<AppState>();
                let cfg = app_state.config.read().unwrap();
                let handle = app.handle();
                crate::tray::update_theme(handle, &cfg.active_theme);
                crate::tray::update_orientation(handle, &cfg.badge_orientation);
            }

            // 启动后自动连接记住的设备
            let auto_handle = app.handle().clone();
            let remembered = app
                .state::<AppState>()
                .config
                .read()
                .map(|c| c.remembered_device.clone())
                .unwrap_or(None);
            tauri::async_runtime::spawn(async move {
                if let Some(dev) = remembered {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let generation = auto_handle
                        .state::<AppState>()
                        .connection_generation
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                        + 1;
                    match commands::connect_device_internal(
                        &auto_handle,
                        &dev.address,
                        &dev.name,
                        generation,
                    )
                    .await
                    {
                        Ok(()) => tracing::info!("已自动连接设备 {}", dev.name),
                        Err(e) => {
                            tracing::warn!("自动连接失败: {e}，进入退避重连");
                            let _ = auto_handle.emit(
                                "device-connection-changed",
                                serde_json::json!({
                                    "connected": false,
                                    "reconnecting": true,
                                    "address": dev.address.clone(),
                                    "name": dev.name.clone(),
                                }),
                            );
                            commands::spawn_reconnect(
                                auto_handle,
                                dev.address,
                                dev.name,
                                1,
                                generation,
                            );
                        }
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // 关窗 = 隐藏（KAD-06；托盘"退出"才是真退出）
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::get_themes,
            commands::get_theme,
            commands::set_active_theme,
            commands::import_theme,
            commands::export_theme,
            commands::delete_theme,
            commands::scan_devices,
            commands::connect_device,
            commands::disconnect_device,
            commands::forget_device,
            commands::trigger_state,
            commands::preview_scene,
            commands::reset_outputs,
            commands::get_config,
            commands::update_config,
            commands::get_integration_status,
            commands::install_integration,
            commands::uninstall_integration,
            commands::get_toolchain_status,
            commands::set_toolchain_overrides,
            commands::reset_toolchain_overrides,
            commands::select_executable,
            commands::check_adapter_update,
            commands::upgrade_adapter,
            commands::fetch_latest_release,
            commands::resolve_update_download_url,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 启动即显示主窗口（产品形态：打开程序时窗口同时打开）
            if let tauri::RunEvent::Ready = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            if let tauri::RunEvent::Exit = event {
                storage::remove_runtime();
            }
        });
}

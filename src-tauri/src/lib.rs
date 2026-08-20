//! AI-Light Tauri 应用入口：装配 core 模块、注册 commands/events

mod commands;

use std::sync::{Arc, RwLock};

use tauri::{Emitter, Manager};

use ailight_core::arbiter::ArbitrationMode;
use ailight_core::ble::DeviceIo;
use ailight_core::config::AppConfig;
use ailight_core::engine::Engine;
use ailight_core::hook_server::SharedState;
use ailight_core::{logging, theme};

/// 应用级共享状态（KAD-03：Rust 唯一事实源）
pub struct AppState {
    pub shared: Arc<SharedState>,
    pub engine: Engine,
    pub device_io: Arc<DeviceIo>,
    pub config: RwLock<AppConfig>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 单实例（KAD-06）：聚焦已有窗口
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            // 日志（KAD-05）
            let _ = logging::init(app.path().app_log_dir().ok().as_deref(), "info");

            // 配置加载（KAD-04）
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let cfg_path = config_dir.join("config.json");
            let (config, warn) = if cfg_path.exists() {
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
            let mode =
                ArbitrationMode::from_str(&config.arbitration_mode).unwrap_or(ArbitrationMode::Priority);
            let shared = SharedState::new(env!("CARGO_PKG_VERSION"), mode, now_ms);
            let theme = theme::load_builtin(&config.active_theme)
                .or_else(|| theme::load_builtin("default"))
                .expect("内置 default 主题必须合法");
            *shared.theme.write().unwrap() = Some(theme);
            *shared.theme_name.write().unwrap() = config.active_theme.clone();
            *shared.token.write().unwrap() =
                if config.token.is_empty() { None } else { Some(config.token.clone()) };

            // 设备代理 + 引擎
            let device_io = DeviceIo::new();
            // Engine::new 内部使用 tokio::spawn，必须处于 runtime 上下文；
            // setup 回调运行在 AppKit 主线程（不在 runtime 上下文），故显式 enter。
            // 见 docs/decisions/ADR-0003 / docs/specs/architecture.md KAD-08。
            let engine = {
                let _guard = tauri::async_runtime::handle().inner().enter();
                Engine::new(shared.clone(), device_io.clone())
            };

            // L1 HTTP 接入服务
            let hs_shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                match ailight_core::hook_server::serve(hs_shared).await {
                    Ok((port, _)) => tracing::info!("hook server 127.0.0.1:{port}"),
                    Err(e) => eprintln!("hook server 启动失败: {e}"),
                }
            });

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
                        last = cur;
                    }
                }
            });

            app.manage(AppState { shared, engine, device_io, config: RwLock::new(config) });

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
                    match commands::connect_device_internal(&auto_handle, &dev.address, &dev.name).await
                    {
                        Ok(()) => tracing::info!("已自动连接设备 {}", dev.name),
                        Err(e) => tracing::warn!("自动连接失败: {e}"),
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
            commands::scan_devices,
            commands::connect_device,
            commands::trigger_state,
            commands::preview_scene,
            commands::reset_outputs,
            commands::get_config,
            commands::update_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

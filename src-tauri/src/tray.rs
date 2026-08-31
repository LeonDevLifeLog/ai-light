//! 托盘常驻（KAD-06 / ui-design §9.4~9.5）：图标 + 菜单 + 动态状态联动
//!
//! - 菜单由 Rust 侧构建（Tauri v2 惯例），动态文字/勾选经 handle 直接更新
//! - 事件源：业务状态轮询（lib.rs）、theme-changed / update_config / 设备连接断开（commands.rs）
//! - 图标：独立单色模板图，macOS 自动适配浅色 / 深色菜单栏

use tauri::menu::{CheckMenuItem, Menu, MenuItem, Submenu};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// 托盘句柄：持有 TrayIcon 防止被回收 + 动态更新所需的菜单项
pub struct TrayState {
    #[allow(dead_code)]
    pub tray: TrayIcon<Wry>,
    pub status: MenuItem<Wry>,
    pub theme: MenuItem<Wry>,
    pub device: MenuItem<Wry>,
    pub orient_h: CheckMenuItem<Wry>,
    pub orient_v: CheckMenuItem<Wry>,
}

/// 构建托盘与菜单（ui-design §9.5 结构），返回句柄供调用方 manage
pub fn init(app: &AppHandle) -> tauri::Result<TrayState> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "当前状态：IDLE", false, None::<&str>)?;
    let theme = MenuItem::with_id(app, "theme", "当前主题：default", false, None::<&str>)?;
    let device = MenuItem::with_id(app, "device", "设备：未连接", false, None::<&str>)?;
    let orient_h = CheckMenuItem::with_id(app, "orient-h", "横向", true, true, None::<&str>)?;
    let orient_v = CheckMenuItem::with_id(app, "orient-v", "纵向", true, false, None::<&str>)?;
    let orient_sub = Submenu::with_items(app, "徽章朝向", true, &[&orient_h, &orient_v])?;
    let open_config = MenuItem::with_id(app, "config", "打开配置", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show,
            &status,
            &theme,
            &device,
            &orient_sub,
            &open_config,
            &quit,
        ],
    )?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("./icons/tray-icon.png"))
        .icon_as_template(true)
        .tooltip("AI-Light")
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, &event))
        .build(app)?;

    Ok(TrayState {
        tray,
        status,
        theme,
        device,
        orient_h,
        orient_v,
    })
}

fn handle_menu_event(app: &AppHandle, event: &tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "show" => show_main_window(app),
        "config" => {
            show_main_window(app);
            // 前端 AppShell 订阅后跳转 /devices
            let _ = app.emit("open-config", ());
        }
        "orient-h" => set_orientation(app, "horizontal"),
        "orient-v" => set_orientation(app, "vertical"),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 托盘切朝向：走 update_config 统一路径（持久化 + 勾选同步 + emit config-changed）
fn set_orientation(app: &AppHandle, orientation: &str) {
    let patch = crate::commands::ConfigPatch {
        token: None,
        autostart: None,
        badge_orientation: Some(orientation.to_string()),
        theme_mode: None,
        port_preference: None,
    };
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::commands::update_config(handle, patch).await {
            tracing::warn!("托盘切换徽章朝向失败: {e:?}");
        }
    });
}

// ---- 动态更新（事件源调用） ----

pub fn update_status(app: &AppHandle, state: &str) {
    if let Some(t) = app.try_state::<TrayState>() {
        let _ = t.status.set_text(format!("当前状态：{state}"));
    }
}

pub fn update_theme(app: &AppHandle, name: &str) {
    if let Some(t) = app.try_state::<TrayState>() {
        let _ = t.theme.set_text(format!("当前主题：{name}"));
    }
}

pub fn update_device(app: &AppHandle, connected: bool, name: Option<&str>) {
    if let Some(t) = app.try_state::<TrayState>() {
        let text = if connected {
            format!("设备：{}", name.unwrap_or("已连接"))
        } else {
            "设备：未连接".to_string()
        };
        let _ = t.device.set_text(text);
    }
}

pub fn update_orientation(app: &AppHandle, orientation: &str) {
    if let Some(t) = app.try_state::<TrayState>() {
        let _ = t.orient_h.set_checked(orientation == "horizontal");
        let _ = t.orient_v.set_checked(orientation == "vertical");
    }
}

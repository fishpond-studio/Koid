//! 系统托盘 + 窗口关闭行为（close_mode: hide / quit / ask）
//!
//! 关闭拦截：
//! - `quit`：放行，正常退出
//! - `hide`：阻止关闭，隐藏到系统托盘
//! - `ask`（默认，未配置时）：阻止关闭，向前端发 `koid://close-ask` 事件，
//!   由前端弹询问框，用户选择后经 `resolve_close` 决定本次行为并记忆

use crate::services::settings::{self, KEY_CLOSE_MODE};
use crate::state::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};

/// 读取当前关闭行为，未配置时默认 `ask`（第一次关闭会询问）
fn close_mode(app: &AppHandle) -> String {
    let state = app.state::<AppState>();
    let Ok(conn) = state.db() else {
        return "ask".to_string();
    };
    settings::get(&conn, KEY_CLOSE_MODE).unwrap_or_else(|| "ask".to_string())
}

/// 创建托盘并注册主窗口关闭拦截（在应用 setup 中调用一次）
pub fn setup(app: &mut tauri::App) -> tauri::Result<()> {
    let handle = app.handle().clone();

    // 托盘菜单：显示 / 退出
    let show = MenuItem::with_id(&handle, "show", "显示 Koid", true, None::<&str>)?;
    let quit = MenuItem::with_id(&handle, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(&handle, &[&show, &quit])?;

    let tray = TrayIconBuilder::new()
        .icon(handle.default_window_icon().unwrap().clone())
        .tooltip("Koid")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击托盘图标 → 显示主窗口
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    // 保持托盘存活（不 drop）
    app.manage(tray);

    // 主窗口关闭拦截
    if let Some(win) = app.get_webview_window("main") {
        let handle = app.handle().clone();
        let win_ = win.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match close_mode(&handle).as_str() {
                    "quit" => {
                        // 放行：正常退出
                    }
                    "hide" => {
                        api.prevent_close();
                        let _ = win_.hide();
                    }
                    _ => {
                        // ask：拦截并交由前端询问
                        api.prevent_close();
                        let _ = handle.emit("koid://close-ask", ());
                    }
                }
            }
        });
    }

    Ok(())
}

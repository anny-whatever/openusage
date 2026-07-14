use std::ffi::OsStr;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

mod app_state;
pub mod contracts;
pub mod platform;
pub mod providers;
pub mod runtime;

const MAIN_WINDOW_LABEL: &str = "main";
const SHOW_MENU_ID: &str = "show";
const QUIT_MENU_ID: &str = "quit";
const TRAY_ICON_SIZE: u32 = 32;

#[derive(Debug, Eq, PartialEq)]
enum ShellMenuAction {
    Show,
    Quit,
    Ignore,
}

#[tauri::command]
async fn get_app_bootstrap(
    state: tauri::State<'_, app_state::AppState>,
) -> Result<app_state::AppBootstrap, String> {
    state.bootstrap().await
}

#[tauri::command]
async fn save_settings(
    state: tauri::State<'_, app_state::AppState>,
    settings: runtime::settings::Settings,
) -> Result<app_state::AppBootstrap, String> {
    state.save_settings(settings).await
}

#[tauri::command]
async fn save_api_key(
    state: tauri::State<'_, app_state::AppState>,
    provider_id: String,
    key: String,
) -> Result<providers::api_key::ApiKeyStatus, String> {
    state.save_api_key(&provider_id, key).await
}

#[tauri::command]
async fn delete_api_key(
    state: tauri::State<'_, app_state::AppState>,
    provider_id: String,
) -> Result<providers::api_key::ApiKeyStatus, String> {
    state.delete_api_key(&provider_id).await
}

#[tauri::command]
fn report_ui_error(kind: String) -> Result<(), String> {
    if kind.is_empty()
        || kind.len() > 64
        || !kind
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'-' | b'_'))
    {
        return Err("Invalid interface error category.".to_owned());
    }
    eprintln!("OpenUsage interface error category: {kind}");
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Err("main window is unavailable".to_owned());
    };

    window.center().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn toggle_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return Err("main window is unavailable".to_owned());
    };

    match window.is_visible().map_err(|error| error.to_string())? {
        true => window.hide().map_err(|error| error.to_string()),
        false => show_main_window(app),
    }
}

fn report_shell_error(action: &str, error: impl std::fmt::Display) {
    eprintln!("OpenUsage Windows shell action '{action}' failed: {error}");
}

fn has_launch_argument(expected: &OsStr) -> bool {
    std::env::args_os().any(|argument| argument == expected)
}

fn should_show_on_launch() -> bool {
    has_launch_argument(OsStr::new("--show"))
}

fn should_verify_quit_on_launch() -> bool {
    has_launch_argument(OsStr::new("--verify-quit"))
}

fn quit_application(app: &tauri::AppHandle) {
    app.exit(0);
}

fn shell_menu_action(menu_id: &str) -> ShellMenuAction {
    match menu_id {
        SHOW_MENU_ID => ShellMenuAction::Show,
        QUIT_MENU_ID => ShellMenuAction::Quit,
        _ => ShellMenuAction::Ignore,
    }
}

fn tray_icon() -> Image<'static> {
    let mut pixels = vec![0_u8; (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize];
    let bars = [(6_u32, 8_u32), (14, 14), (22, 20)];

    for (left, height) in bars {
        let top = TRAY_ICON_SIZE - 5 - height;
        for y in top..(TRAY_ICON_SIZE - 5) {
            for x in left..(left + 5) {
                let offset = ((y * TRAY_ICON_SIZE + x) * 4) as usize;
                pixels[offset] = 255;
                pixels[offset + 1] = 255;
                pixels[offset + 2] = 255;
                pixels[offset + 3] = 255;
            }
        }
    }

    Image::new_owned(pixels, TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_app_bootstrap,
            save_settings,
            save_api_key,
            delete_api_key,
            report_ui_error
        ])
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW_LABEL
                && let WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                if let Err(error) = window.hide() {
                    report_shell_error("hide-on-close", error);
                }
            }
        })
        .setup(|app| {
            let paths = platform::paths::WindowsPaths::from_environment()?;
            let state = tauri::async_runtime::block_on(app_state::AppState::load(paths))
                .map_err(std::io::Error::other)?;
            app.manage(state);
            let show_item =
                MenuItem::with_id(app, SHOW_MENU_ID, "Show OpenUsage", true, None::<&str>)?;
            let quit_item =
                MenuItem::with_id(app, QUIT_MENU_ID, "Quit OpenUsage", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon())
                .tooltip("OpenUsage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match shell_menu_action(event.id().as_ref()) {
                    ShellMenuAction::Show => {
                        if let Err(error) = show_main_window(app) {
                            report_shell_error("show-menu", error);
                        }
                    }
                    ShellMenuAction::Quit => quit_application(app),
                    ShellMenuAction::Ignore => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                        && let Err(error) = toggle_main_window(tray.app_handle())
                    {
                        report_shell_error("tray-toggle", error);
                    }
                })
                .build(app)?;

            if should_show_on_launch()
                && let Err(error) = show_main_window(app.handle())
            {
                report_shell_error("show-on-launch", error);
            }

            if should_verify_quit_on_launch() {
                quit_application(app.handle());
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run OpenUsage Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_icon_has_a_bounded_rgba_buffer() {
        let icon = tray_icon();

        assert_eq!(icon.width(), TRAY_ICON_SIZE);
        assert_eq!(icon.height(), TRAY_ICON_SIZE);
        assert_eq!(
            icon.rgba().len(),
            (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize
        );
    }

    #[test]
    fn tray_menu_ids_map_to_explicit_shell_actions() {
        assert_eq!(shell_menu_action(SHOW_MENU_ID), ShellMenuAction::Show);
        assert_eq!(shell_menu_action(QUIT_MENU_ID), ShellMenuAction::Quit);
        assert_eq!(shell_menu_action("unknown"), ShellMenuAction::Ignore);
    }

    #[test]
    fn interface_error_reporting_accepts_categories_not_payloads() {
        assert!(report_ui_error("TypeError".to_owned()).is_ok());
        assert!(report_ui_error("token=value".to_owned()).is_err());
        assert!(report_ui_error("x".repeat(65)).is_err());
    }
}

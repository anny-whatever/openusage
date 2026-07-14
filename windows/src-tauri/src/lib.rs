use std::ffi::OsStr;

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WindowEvent};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScaffoldStatus {
    platform: &'static str,
    architecture: &'static str,
    trusted_backend: bool,
}

#[tauri::command]
fn get_scaffold_status() -> ScaffoldStatus {
    ScaffoldStatus {
        platform: "windows",
        architecture: std::env::consts::ARCH,
        trusted_backend: true,
    }
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
        .invoke_handler(tauri::generate_handler![get_scaffold_status])
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
    fn scaffold_status_exposes_only_platform_metadata() {
        let status = get_scaffold_status();

        assert_eq!(status.platform, "windows");
        assert!(!status.architecture.is_empty());
        assert!(status.trusted_backend);
    }

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
}

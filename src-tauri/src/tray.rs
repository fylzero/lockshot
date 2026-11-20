//! The menu bar (macOS) / notification area (Windows, Linux) icon, which is
//! Lockshot's home: there is no window until a capture or Settings opens one.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::settings::Hotkeys;

pub const TRAY_ID: &str = "lockshot-tray";

fn item(app: &AppHandle, id: &str, label: &str, accelerator: &str) -> tauri::Result<MenuItem<Wry>> {
    let accelerator = (!accelerator.is_empty()).then_some(accelerator);
    /* A combo the menu cannot display should not take the whole menu down. */
    MenuItem::with_id(app, id, label, true, accelerator)
        .or_else(|_| MenuItem::with_id(app, id, label, true, None::<&str>))
}

pub fn menu(app: &AppHandle, hotkeys: &Hotkeys) -> tauri::Result<Menu<Wry>> {
    Menu::with_items(
        app,
        &[
            &item(app, "area", "Capture Area", &hotkeys.area)?,
            &item(app, "screen", "Capture Screen", &hotkeys.screen)?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "editor", "Open Lockshot", &hotkeys.editor)?,
            &item(app, "hotkeys", "Hotkeys…", "")?,
            &item(app, "settings", "Settings…", "")?,
            &item(app, "updates", "Check for Updates…", "")?,
            &PredefinedMenuItem::separator(app)?,
            &item(app, "quit", "Quit Lockshot", "")?,
        ],
    )
}

fn icon() -> tauri::Result<Image<'static>> {
    /*
     * macOS draws a black-and-alpha template image in whatever color the menu
     * bar needs. Windows and Linux trays do not, so they get the full-color
     * mark, which reads on both light and dark taskbars.
     */
    if cfg!(target_os = "macos") {
        Image::from_bytes(include_bytes!("../icons/tray-template@2x.png"))
    } else {
        Image::from_bytes(include_bytes!("../icons/tray-color.png"))
    }
}

pub fn build(app: &AppHandle, hotkeys: &Hotkeys) -> tauri::Result<TrayIcon> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon()?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Lockshot")
        .menu(&menu(app, hotkeys)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "quit" => app.exit(0),
                "settings" => crate::show_main(app, Some("/settings/general")),
                "hotkeys" => crate::show_main(app, Some("/settings/hotkeys")),
                "updates" => {
                    crate::show_main(app, None);
                    let _ = app.emit_to("main", "check-for-updates", ());
                }
                action => crate::run_action(app, action),
            }
        })
        .build(app)
}

/// Show the current combos next to the menu items after a change.
pub fn refresh(app: &AppHandle, hotkeys: &Hotkeys) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), menu(app, hotkeys)) {
        let _ = tray.set_menu(Some(menu));
    }
}

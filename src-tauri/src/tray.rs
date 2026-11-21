//! The menu bar (macOS) / notification area (Windows, Linux) icon, which is
//! Lockshot's home: there is no window until a capture or Settings opens one.

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Emitter, Wry};

use crate::settings::Settings;

pub const TRAY_ID: &str = "lockshot-tray";

fn item(app: &AppHandle, id: &str, label: &str, accelerator: &str) -> tauri::Result<MenuItem<Wry>> {
    let accelerator = (!accelerator.is_empty()).then_some(accelerator);
    /* A combo the menu cannot display should not take the whole menu down. */
    MenuItem::with_id(app, id, label, true, accelerator)
        .or_else(|_| MenuItem::with_id(app, id, label, true, None::<&str>))
}

pub const QUICK_SAVE_ID: &str = "quick-save";

pub fn menu(app: &AppHandle, settings: &Settings) -> tauri::Result<Menu<Wry>> {
    let hotkeys = &settings.hotkeys;
    Menu::with_items(
        app,
        &[
            &item(app, "area", "Capture Area", &hotkeys.area)?,
            &item(app, "window", "Capture Window", &hotkeys.window)?,
            &item(app, "scroll", "Scrolling Capture", &hotkeys.scroll)?,
            &item(app, "screen", "Capture Screen", &hotkeys.screen)?,
            &PredefinedMenuItem::separator(app)?,
            &CheckMenuItem::with_id(
                app,
                QUICK_SAVE_ID,
                "Save Without Editing",
                true,
                settings.quick_save,
                None::<&str>,
            )?,
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

fn tooltip(settings: &Settings) -> &'static str {
    if settings.quick_save {
        "Lockshot: saving captures without editing"
    } else {
        "Lockshot"
    }
}

pub fn build(app: &AppHandle, settings: &Settings) -> tauri::Result<TrayIcon> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon()?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(tooltip(settings))
        .menu(&menu(app, settings)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            match id {
                "quit" => app.exit(0),
                "settings" => crate::show_main(app, Some("/settings/general")),
                "hotkeys" => crate::show_main(app, Some("/settings/hotkeys")),
                QUICK_SAVE_ID => crate::quick_save::toggle(app),
                "updates" => {
                    crate::show_main(app, None);
                    let _ = app.emit_to("main", "check-for-updates", ());
                }
                action => crate::run_action(app, action),
            }
        })
        .build(app)
}

/// Show the current combos and Save Without Editing state after a change.
pub fn refresh(app: &AppHandle, settings: &Settings) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), menu(app, settings)) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(tooltip(settings)));
    }
}

//! The macOS menu bar, modelled on Shipyard's. Its main job is ⌘W: Tauri's
//! default menu binds it to Close Window, and menu shortcuts fire before the
//! page sees the key, so ⌘W closed the window from any tab. Here it asks the
//! page to close the active tab instead, and the page only hides the window
//! once Home is the active tab.
//!
//! Windows and Linux get no menu: an app-wide menu there would also attach
//! to the capture overlays and draw a menu bar over the screen. The page
//! handles Ctrl+W itself on those platforms.

use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const CLOSE_TAB_OR_WINDOW_ID: &str = "close-tab-or-window";
pub const OPEN_SETTINGS_ID: &str = "open-settings";
pub const CHECK_FOR_UPDATES_ID: &str = "check-for-updates";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg_info = app.package_info();
    let config = app.config();
    let about_metadata = AboutMetadata {
        name: Some(pkg_info.name.clone()),
        version: Some(pkg_info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };

    let close = MenuItem::with_id(
        app,
        CLOSE_TAB_OR_WINDOW_ID,
        "Close",
        true,
        Some("CmdOrCtrl+W"),
    )?;
    let close_window_item =
        MenuItem::with_id(app, CLOSE_TAB_OR_WINDOW_ID, "Close", true, None::<&str>)?;
    let settings = MenuItem::with_id(
        app,
        OPEN_SETTINGS_ID,
        "Settings…",
        true,
        Some("CmdOrCtrl+,"),
    )?;
    let check_updates = MenuItem::with_id(
        app,
        CHECK_FOR_UPDATES_ID,
        "Check for Updates…",
        true,
        None::<&str>,
    )?;

    Menu::with_items(
        app,
        &[
            &Submenu::with_items(
                app,
                pkg_info.name.clone(),
                true,
                &[
                    &PredefinedMenuItem::about(app, None, Some(about_metadata))?,
                    &check_updates,
                    &PredefinedMenuItem::separator(app)?,
                    &settings,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(app, "File", true, &[&close])?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(app, None)?,
                    &PredefinedMenuItem::maximize(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &close_window_item,
                ],
            )?,
        ],
    )
}

pub fn handle_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    /* ⌘W in a capture overlay is not meant for the editor's tabs. */
    if id == CLOSE_TAB_OR_WINDOW_ID && !window.is_focused().unwrap_or(false) {
        return;
    }
    match id {
        CLOSE_TAB_OR_WINDOW_ID | OPEN_SETTINGS_ID | CHECK_FOR_UPDATES_ID => {
            let _ = window.emit(id, ());
        }
        _ => {}
    }
}

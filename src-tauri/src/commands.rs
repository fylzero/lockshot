use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::ipc::{InvokeBody, Request};
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

use crate::autostart;
use crate::capture::{self, CaptureReady, OverlayStart, Rect};
use crate::export;
use crate::hotkeys;
use crate::platform::{self, PermissionStatus};
use crate::settings::{self, Hotkeys, Settings, ACTIONS};
use crate::tray;

pub struct AppState {
    pub settings: Mutex<Settings>,
    /// Hotkeys that could not be registered at launch, shown in Settings.
    pub hotkey_errors: Mutex<Vec<String>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    settings: Settings,
    default_save_dir: String,
    settings_path: String,
    formats: Vec<&'static str>,
    os: &'static str,
    hotkey_errors: Vec<String>,
}

#[tauri::command]
pub fn bootstrap(app: AppHandle, state: State<AppState>) -> Bootstrap {
    Bootstrap {
        settings: state.settings.lock().unwrap().clone(),
        default_save_dir: settings::default_save_dir(&app).display().to_string(),
        settings_path: settings::path(&app)
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        formats: export::supported_formats(),
        os: platform::os_name(),
        hotkey_errors: state.hotkey_errors.lock().unwrap().clone(),
    }
}

/// Save everything except the hotkeys, which only change through
/// `set_hotkey` so the OS registrations stay in step.
#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<AppState>,
    next: Settings,
) -> Result<Settings, String> {
    let mut current = state.settings.lock().unwrap();
    let hotkeys = current.hotkeys.clone();
    let mut next = next;
    next.hotkeys = hotkeys;
    if !export::FORMATS.contains(&next.format.as_str()) {
        next.format = "png".into();
    }
    next.jpeg_quality = next.jpeg_quality.clamp(1, 100);
    if !settings::SCROLL_DIRECTIONS.contains(&next.scroll_direction.as_str()) {
        next.scroll_direction = "down".into();
    }
    next.scroll_delay_ms = next.scroll_delay_ms.clamp(
        *settings::SCROLL_DELAY_MS.start(),
        *settings::SCROLL_DELAY_MS.end(),
    );
    next.scroll_max_pages = next.scroll_max_pages.clamp(
        *settings::SCROLL_MAX_PAGES.start(),
        *settings::SCROLL_MAX_PAGES.end(),
    );
    if next.launch_at_login != current.launch_at_login {
        autostart::apply(&app, next.launch_at_login)?;
    }
    settings::save(&app, &next)?;
    let quick_save_changed = next.quick_save != current.quick_save;
    *current = next.clone();
    if quick_save_changed {
        tray::refresh(&app, &next);
    }
    Ok(next)
}

fn store_hotkeys(app: &AppHandle, state: &AppState, hotkeys: Hotkeys) -> Result<Settings, String> {
    let mut current = state.settings.lock().unwrap();
    current.hotkeys = hotkeys;
    settings::save(app, &current)?;
    tray::refresh(app, &current);
    state.hotkey_errors.lock().unwrap().clear();
    Ok(current.clone())
}

#[tauri::command]
pub fn set_hotkey(
    app: AppHandle,
    state: State<AppState>,
    action: String,
    accelerator: String,
) -> Result<Settings, String> {
    let accelerator = accelerator.trim().to_string();
    let mut hotkeys = state.settings.lock().unwrap().hotkeys.clone();
    hotkeys::rebind(&app, &hotkeys, &action, &accelerator)?;
    hotkeys.set(&action, accelerator);
    store_hotkeys(&app, &state, hotkeys)
}

#[tauri::command]
pub fn reset_hotkeys(app: AppHandle, state: State<AppState>) -> Result<Settings, String> {
    let defaults = Hotkeys::default();
    let mut hotkeys = state.settings.lock().unwrap().hotkeys.clone();
    let mut errors = Vec::new();
    for action in ACTIONS {
        let wanted = defaults.get(action).unwrap_or_default().to_string();
        match hotkeys::rebind(&app, &hotkeys, action, &wanted) {
            Ok(()) => {
                hotkeys.set(action, wanted);
            }
            Err(err) => errors.push(err),
        }
    }
    let settings = store_hotkeys(&app, &state, hotkeys)?;
    if errors.is_empty() {
        Ok(settings)
    } else {
        Err(errors.join(" "))
    }
}

/// Register the current hotkeys again, after macOS has given one back.
#[tauri::command]
pub fn reregister_hotkeys(app: AppHandle, state: State<AppState>) -> Vec<String> {
    let hotkeys = state.settings.lock().unwrap().hotkeys.clone();
    let errors = hotkeys::reregister_all(&app, &hotkeys);
    *state.hotkey_errors.lock().unwrap() = errors.clone();
    errors
}

#[tauri::command]
pub fn pause_hotkeys(app: AppHandle, paused: bool) {
    hotkeys::set_paused(&app, paused);
}

#[tauri::command]
pub fn validate_hotkey(accelerator: String) -> Result<(), String> {
    hotkeys::parse(&accelerator).map(|_| ())
}

/// Start a capture from a button in the window, which first gets out of the
/// way so it is not in the shot.
#[tauri::command]
pub async fn capture(app: AppHandle, mode: String) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        if main.is_visible().unwrap_or(false) {
            let _ = main.hide();
            /* Give the compositor a moment to take the window off screen. */
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
    }
    match mode.as_str() {
        "area" => capture::start_area(&app),
        "screen" => capture::capture_screen(&app),
        "window" => capture::start_window(&app),
        "scroll" => capture::start_scroll(&app),
        _ => Err("Unknown capture mode.".into()),
    }
}

#[tauri::command]
pub async fn open_image(app: AppHandle, path: String) -> Result<(), String> {
    capture::open_file(&app, std::path::Path::new(&path))
}

#[tauri::command]
pub fn finish_area(
    app: AppHandle,
    window: WebviewWindow,
    session: String,
    rect: Option<Rect>,
) -> Result<(), String> {
    capture::finish_area(&app, window.label(), &session, rect)
}

#[tauri::command]
pub fn finish_window(
    app: AppHandle,
    window: WebviewWindow,
    session: String,
    window_id: u32,
) -> Result<(), String> {
    capture::finish_window(&app, window.label(), &session, window_id)
}

#[tauri::command]
pub fn finish_scroll(
    app: AppHandle,
    window: WebviewWindow,
    session: String,
    window_id: u32,
    x: f64,
    y: f64,
) -> Result<(), String> {
    capture::finish_scroll(&app, window.label(), &session, window_id, x, y)
}

/// The Stop button on the Scrolling Capture notice.
#[tauri::command]
pub fn scroll_stop() {
    crate::scrolling::stop();
}

#[tauri::command]
pub fn overlay_pending(app: AppHandle, window: WebviewWindow) -> Option<OverlayStart> {
    capture::overlay_pending(&app, window.label())
}

#[tauri::command]
pub fn cancel_area(app: AppHandle) {
    capture::cancel_area(&app);
}

#[tauri::command]
pub fn release_capture(app: AppHandle, id: String) {
    capture::release(&app, &id);
}

#[tauri::command]
pub fn latest_capture(app: AppHandle) -> Option<CaptureReady> {
    capture::latest(&app)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveOptions {
    format: String,
    /// An exact path from a Save As dialog, written over if it exists.
    path: Option<String>,
    /// Otherwise a folder (empty for the default) and a file name without
    /// extension, made unique if taken.
    dir: Option<String>,
    name: Option<String>,
}

/**
 * Write the edited image. The body is raw bytes: a little-endian u32 length,
 * that many bytes of JSON SaveOptions, then the PNG. Raw keeps a large
 * screenshot from being inflated into a JSON number array.
 */
/// Split a raw save body into its options and the PNG after them.
fn split_save_body(request: &Request) -> Result<(SaveOptions, Vec<u8>), String> {
    let InvokeBody::Raw(body) = request.body() else {
        return Err("Expected the image as raw bytes.".into());
    };
    if body.len() < 4 {
        return Err("The image data is empty.".into());
    }
    let json_len = u32::from_le_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let json = body
        .get(4..4 + json_len)
        .ok_or("The save options are cut short.")?;
    let options: SaveOptions = serde_json::from_slice(json)
        .map_err(|err| format!("Could not read the save options: {err}"))?;
    Ok((options, body[4 + json_len..].to_vec()))
}

#[tauri::command]
pub fn save_image(
    app: AppHandle,
    state: State<AppState>,
    request: Request,
) -> Result<String, String> {
    let (options, png) = split_save_body(&request)?;
    let png = png.as_slice();

    let settings = state.settings.lock().unwrap().clone();
    let ext = export::extension(&options.format);
    let path = match options.path.filter(|p| !p.trim().is_empty()) {
        Some(path) => PathBuf::from(path),
        None => {
            let dir = match options.dir.filter(|d| !d.trim().is_empty()) {
                Some(dir) => PathBuf::from(dir),
                None => settings::resolved_save_dir(&app, &settings),
            };
            let stem = export::sanitize_file_stem(options.name.as_deref().unwrap_or("Screenshot"));
            export::unique_path(&dir, &stem, ext)
        }
    };
    export::write(png, &options.format, settings.jpeg_quality, &path)?;
    Ok(path.display().to_string())
}

/**
 * Drag the edited image out of the window as a real file, the way Skitch's
 * drag handle works: the file is written to a private temporary folder under
 * the export name and format, then handed to the OS as a native drag, so it
 * can be dropped on the Desktop, a Finder or Explorer window, a browser
 * upload field, a chat, and so on. Same raw body as `save_image`.
 */
#[tauri::command]
pub async fn drag_image(
    window: WebviewWindow,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> Result<(), String> {
    let (options, png) = split_save_body(&request)?;
    let quality = state.settings.lock().unwrap().jpeg_quality;
    let stem = export::sanitize_file_stem(options.name.as_deref().unwrap_or("Screenshot"));
    let path = export::write_drag_file(&png, &options.format, quality, &stem)?;
    let preview = export::drag_preview(&png)?;

    let (tx, rx) = std::sync::mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let item = drag::DragItem::Files(vec![path]);
            let icon = drag::Image::Raw(preview);
            let start = || -> Result<(), String> {
                #[cfg(target_os = "linux")]
                let handle = target.gtk_window().map_err(|err| err.to_string())?;
                #[cfg(target_os = "linux")]
                let handle = &handle;
                #[cfg(not(target_os = "linux"))]
                let handle = &target;
                drag::start_drag(handle, item, icon, |_, _| {}, drag::Options::default())
                    .map_err(|err| err.to_string())
            };
            let _ = tx.send(start());
        })
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|_| "The drag did not start.".to_string())?
        .map_err(|err| format!("Could not start the drag: {err}"))
}

#[tauri::command]
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|err| format!("Could not show the file: {err}"))
}

#[tauri::command]
pub fn reveal_settings_file(app: AppHandle) -> Result<(), String> {
    let path = settings::path(&app)?;
    if !path.exists() {
        let current = app.state::<AppState>().settings.lock().unwrap().clone();
        settings::save(&app, &current)?;
    }
    reveal_path(app, path.display().to_string())
}

/// The settings as the JSON page shows them.
#[tauri::command]
pub fn settings_json(state: State<AppState>) -> Result<String, String> {
    settings::to_json(&state.settings.lock().unwrap())
}

/**
 * Replace every setting with JSON from the editor. Nothing changes unless all
 * of it is valid and every hotkey can be registered.
 */
#[tauri::command]
pub fn apply_settings_json(
    app: AppHandle,
    state: State<AppState>,
    raw: String,
) -> Result<Settings, String> {
    let next = settings::from_json(&raw)?;
    if !export::supported_formats().contains(&next.format.as_str()) {
        return Err(format!(
            "\"format\": {} is not available on this computer.",
            next.format
        ));
    }
    let mut current = state.settings.lock().unwrap();
    let hotkeys_changed = ACTIONS
        .iter()
        .any(|a| current.hotkeys.get(a) != next.hotkeys.get(a));
    let quick_save_changed = current.quick_save != next.quick_save;
    hotkeys::replace_all(&app, &current.hotkeys, &next.hotkeys)?;
    if let Err(err) = settings::save(&app, &next) {
        let _ = hotkeys::replace_all(&app, &next.hotkeys, &current.hotkeys);
        return Err(err);
    }
    if next.launch_at_login != current.launch_at_login {
        if let Err(err) = autostart::apply(&app, next.launch_at_login) {
            let _ = hotkeys::replace_all(&app, &next.hotkeys, &current.hotkeys);
            let _ = settings::save(&app, &current);
            return Err(err);
        }
    }
    *current = next.clone();
    if hotkeys_changed || quick_save_changed {
        tray::refresh(&app, &next);
    }
    if hotkeys_changed {
        state.hotkey_errors.lock().unwrap().clear();
    }
    Ok(next)
}

/// Biggest file Import will read; real settings are well under 1 KB.
const IMPORT_LIMIT: u64 = 1024 * 1024;

/// Read a settings file to show in the editor. It is checked but not applied
/// until the user saves.
#[tauri::command]
pub fn import_settings_json(path: String) -> Result<String, String> {
    let size = std::fs::metadata(&path)
        .map_err(|err| format!("Could not read {path}: {err}"))?
        .len();
    if size > IMPORT_LIMIT {
        return Err("That file is too big to be Lockshot settings.".into());
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|err| format!("Could not read {path}: {err}"))?;
    settings::to_json(&settings::from_json(&raw)?)
}

/// Write the saved settings to a file of the user's choosing.
#[tauri::command]
pub fn export_settings_json(state: State<AppState>, path: String) -> Result<(), String> {
    let raw = settings::to_json(&state.settings.lock().unwrap())?;
    std::fs::write(&path, raw).map_err(|err| format!("Could not write {path}: {err}"))
}

#[tauri::command]
pub fn permission_status() -> PermissionStatus {
    platform::permission_status()
}

#[tauri::command]
pub fn request_screen_permission() -> PermissionStatus {
    platform::request_screen_permission();
    platform::permission_status()
}

/// Scrolling Capture on macOS: show the Accessibility prompt.
#[tauri::command]
pub fn request_input_permission() -> PermissionStatus {
    platform::request_input_permission();
    platform::permission_status()
}

#[tauri::command]
pub fn open_system_settings(app: AppHandle, target: String) -> Result<(), String> {
    let url =
        platform::settings_url(&target).ok_or("There is no settings page for that on this OS.")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| format!("Could not open system settings: {err}"))
}

/// The OS's own screenshot shortcuts and whether each is still on, so the
/// app can warn when a Lockshot hotkey is still owned by the system.
#[tauri::command]
pub fn system_screenshot_shortcuts() -> Vec<crate::system_shortcuts::SystemShortcut> {
    crate::system_shortcuts::screenshot_shortcuts()
}

/// Turn macOS's own screenshot shortcuts (by id) off, or back on to undo,
/// after the user confirmed it in the app. Returns the list as it now stands.
#[tauri::command]
pub fn set_system_screenshot_shortcuts(
    ids: Vec<String>,
    enabled: bool,
) -> Result<Vec<crate::system_shortcuts::SystemShortcut>, String> {
    crate::system_shortcuts::set_screenshot_shortcuts(&ids, enabled)
}

/// Hide the editor window back to the tray.
#[tauri::command]
pub fn hide_main(app: AppHandle) {
    crate::hide_main(&app);
}

/// The save notice to show, for a toast window that just loaded.
#[tauri::command]
pub fn toast_pending(app: AppHandle) -> Option<crate::quick_save::Toast> {
    crate::quick_save::pending(&app)
}

#[tauri::command]
pub fn toast_dismiss(app: AppHandle, id: String) {
    crate::quick_save::dismiss(&app, &id);
}

#[tauri::command]
pub fn toast_edit(app: AppHandle, id: String) {
    crate::quick_save::edit(&app, &id);
}

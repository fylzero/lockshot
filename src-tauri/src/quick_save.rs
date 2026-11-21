//! Save Without Editing: when it is on, a capture is written straight to a
//! file in the default format instead of opening in the editor, and a small
//! floating toast says where it went. The toast's Edit button opens that
//! capture in the editor after all, for the shots that need markup.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, Timelike};
use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::capture::{self, CaptureReady};
use crate::commands::AppState;
use crate::settings::{self, Settings};
use crate::{export, tray};

pub const TOAST_LABEL: &str = "toast";
/// Logical size of the toast window, and its gap from the screen edge.
const TOAST_WIDTH: f64 = 400.0;
const TOAST_HEIGHT: f64 = 84.0;
const TOAST_MARGIN: f64 = 16.0;
/// The thumbnail is drawn at 80×56 points; this is twice that for Retina.
const THUMB_WIDTH: u32 = 160;
const THUMB_HEIGHT: u32 = 112;
const DEFAULT_TEMPLATE: &str = "Screenshot-{date}-{time}";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    width: u32,
    height: u32,
    /// Raw RGBA, small enough to send as JSON.
    data: Vec<u8>,
}

/// What the toast window shows: a saved file, a Scrolling Capture in
/// progress or finished, or why saving failed.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toast {
    id: String,
    /// The capture, kept so Edit can open it. Freed when the toast goes.
    capture: Option<CaptureReady>,
    path: Option<String>,
    file_name: Option<String>,
    thumbnail: Option<Thumbnail>,
    copied: bool,
    error: Option<String>,
    /// Scrolling Capture: pages saved so far, or in all once it is done.
    pages: Option<u32>,
    /// Scrolling Capture is still going; the toast offers Stop.
    running: bool,
    /// The pointer is held still while it runs, so only Esc stops it.
    pointer_locked: bool,
    /// Scrolling Capture has stopped and is writing the long image and text.
    saving: bool,
    /// What it is doing while saving, such as "Stitching the long image…".
    stage: Option<String>,
    /// Why a Scrolling Capture ended, such as "Reached the end".
    note: Option<String>,
    /// What a finished Scrolling Capture saved, such as "Saved 12 pages
    /// and a long image".
    title: Option<String>,
}

#[derive(Default)]
pub struct ToastState {
    current: Mutex<Option<Toast>>,
    /// The part of the screen a Scrolling Capture is capturing, as x, y,
    /// width and height in fractions of the display. The toast keeps out of
    /// it so it never ends up in the pages.
    avoid: Mutex<Option<(f64, f64, f64, f64)>>,
    /// No corner was clear of that part, so the toast sits on top of it.
    covering: AtomicBool,
    /// Where place() last put the toast, so a Scrolling Capture's Saving
    /// and finished notices stay where its progress notice was.
    spot: Mutex<Option<(i32, i32)>>,
}

/// Scrolling Capture: keep the toast out of this part of the display, or
/// stop avoiding anything with None.
pub fn set_avoid(app: &AppHandle, rect: Option<(f64, f64, f64, f64)>) {
    let state = app.state::<ToastState>();
    *state.avoid.lock().unwrap() = rect;
    if rect.is_none() {
        state.covering.store(false, Ordering::SeqCst);
    }
}

/**
 * Scrolling Capture is about to capture its area: if the toast is on top of
 * it, move it off the screen until the next update puts it back. Moved, not
 * hidden: hiding and showing it again on macOS can leave the finished
 * notice hidden. Returns whether it moved it.
 */
pub fn park_if_covering(app: &AppHandle) -> bool {
    if !app.state::<ToastState>().covering.load(Ordering::SeqCst) {
        return false;
    }
    let Some(window) = app.get_webview_window(TOAST_LABEL) else {
        return false;
    };
    window
        .set_position(tauri::PhysicalPosition::new(-32_000, -32_000))
        .is_ok()
}

/// The settings to save with, when Save Without Editing is on.
pub fn enabled(app: &AppHandle) -> Option<Settings> {
    let settings = app.state::<AppState>().settings.lock().ok()?.clone();
    settings.quick_save.then_some(settings)
}

/// Flip Save Without Editing from the tray menu.
pub fn toggle(app: &AppHandle) {
    let state = app.state::<AppState>();
    let next = {
        let mut current = state.settings.lock().unwrap();
        current.quick_save = !current.quick_save;
        if let Err(err) = settings::save(app, &current) {
            eprintln!("{err}");
        }
        current.clone()
    };
    tray::refresh(app, &next);
    let _ = app.emit_to("main", "settings-changed", &next);
}

/**
 * Write the capture to the quick save folder and show the toast. Encoding a
 * large display takes a moment, so it runs off the thread that captured.
 */
pub fn save(app: &AppHandle, image: RgbaImage, settings: Settings) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let id = uuid::Uuid::new_v4().to_string();
        let toast = match write(&app, &image, &settings) {
            Ok((path, stem)) => {
                let copied = settings.copy_on_capture && copy(&app, &image);
                let thumbnail = thumbnail(&image);
                let ready = capture::store(&app, image, Some(stem.clone()), false);
                Toast {
                    id,
                    capture: Some(ready),
                    file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
                    path: Some(path.display().to_string()),
                    thumbnail: Some(thumbnail),
                    copied,
                    ..Default::default()
                }
            }
            Err(err) => Toast {
                id,
                error: Some(err),
                ..Default::default()
            },
        };
        show(&app, toast);
    });
}

fn write(
    app: &AppHandle,
    image: &RgbaImage,
    settings: &Settings,
) -> Result<(std::path::PathBuf, String), String> {
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|err| format!("Could not encode the capture: {err}"))?;
    let dir = settings::resolved_quick_save_dir(app, settings);
    let stem = file_stem(
        &settings.filename_template,
        Local::now(),
        image.width(),
        image.height(),
    );
    let path = export::unique_path(&dir, &stem, export::extension(&settings.format));
    export::write(&png, &settings.format, settings.jpeg_quality, &path)?;
    let saved_stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or(stem);
    Ok((path, saved_stem))
}

fn copy(app: &AppHandle, image: &RgbaImage) -> bool {
    let pixels = tauri::image::Image::new(image.as_raw(), image.width(), image.height());
    app.clipboard().write_image(&pixels).is_ok()
}

fn thumbnail(image: &RgbaImage) -> Thumbnail {
    let scale = f64::min(
        THUMB_WIDTH as f64 / image.width() as f64,
        THUMB_HEIGHT as f64 / image.height() as f64,
    )
    .min(1.0);
    let width = ((image.width() as f64 * scale).round() as u32).max(1);
    let height = ((image.height() as f64 * scale).round() as u32).max(1);
    let small = image::imageops::thumbnail(image, width, height);
    Thumbnail {
        width,
        height,
        data: small.into_raw(),
    }
}

/**
 * The file name template filled in, the same way the editor does it
 * (src/filename.ts): dots in the time, since Windows does not allow colons.
 */
pub fn file_stem(template: &str, when: DateTime<Local>, width: u32, height: u32) -> String {
    let template = match template.trim() {
        "" => DEFAULT_TEMPLATE,
        t => t,
    };
    let date = format!("{}-{:02}-{:02}", when.year(), when.month(), when.day());
    let time = format!(
        "{:02}.{:02}.{:02}",
        when.hour(),
        when.minute(),
        when.second()
    );
    let value = |key: &str| -> Option<String> {
        Some(match key {
            "date" => date.clone(),
            "time" => time.clone(),
            "year" => when.year().to_string(),
            "month" => format!("{:02}", when.month()),
            "day" => format!("{:02}", when.day()),
            "hour" => format!("{:02}", when.hour()),
            "minute" => format!("{:02}", when.minute()),
            "second" => format!("{:02}", when.second()),
            "width" => width.to_string(),
            "height" => height.to_string(),
            _ => return None,
        })
    };
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close)
                if after[..close]
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_') =>
            {
                let key = &after[..close];
                match value(key) {
                    Some(v) => out.push_str(&v),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    export::sanitize_file_stem(&out)
}

/// Make the toast window the first time; later toasts reuse it.
fn toast_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(TOAST_LABEL) {
        return Ok(window);
    }
    WebviewWindowBuilder::new(app, TOAST_LABEL, WebviewUrl::App("index.html".into()))
        .title("Lockshot")
        .inner_size(TOAST_WIDTH, TOAST_HEIGHT)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        /* A toast should never take the keyboard from the app in front. */
        .focused(false)
        .focusable(false)
        .visible_on_all_workspaces(true)
        .accept_first_mouse(true)
        .build()
        .map_err(|err| format!("Could not open the save notice: {err}"))
}

/// Create the hidden toast window ahead of time, so the first one is instant.
pub fn prewarm(app: &AppHandle) {
    let _ = toast_window(app);
}

/**
 * Bottom-right of the display under the pointer, clear of the Dock or
 * taskbar. Never under the pointer: Scrolling Capture scrolls whatever is
 * there, and the toast would catch the scroll. While a Scrolling Capture
 * runs it also keeps out of the area being captured, trying the other
 * corners in turn.
 */
fn place(app: &AppHandle, window: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let _ = window.set_size(tauri::LogicalSize::new(TOAST_WIDTH, TOAST_HEIGHT));
    let Some(monitor) = monitor else {
        return;
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let (w, h) = (TOAST_WIDTH * scale, TOAST_HEIGHT * scale);
    let margin = TOAST_MARGIN * scale;
    let (left, top) = (
        area.position.x as f64 + margin,
        area.position.y as f64 + margin,
    );
    let right = area.position.x as f64 + area.size.width as f64 - w - margin;
    let bottom = area.position.y as f64 + area.size.height as f64 - h - margin;
    let corners = [(right, bottom), (right, top), (left, bottom), (left, top)];

    let state = app.state::<ToastState>();
    let avoid = state.avoid.lock().unwrap().map(|(fx, fy, fw, fh)| {
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
        (mx + fx * mw, my + fy * mh, fw * mw, fh * mh)
    });
    let pointer = app.cursor_position().ok().map(|p| (p.x, p.y));
    let free_of_pointer = |&(x, y): &(f64, f64)| {
        !pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    };
    let free_of_area = |&(x, y): &(f64, f64)| {
        !avoid
            .is_some_and(|(ax, ay, aw, ah)| x < ax + aw && ax < x + w && y < ay + ah && ay < y + h)
    };
    let spot = corners
        .into_iter()
        .find(|c| free_of_pointer(c) && free_of_area(c));
    state
        .covering
        .store(spot.is_none() && avoid.is_some(), Ordering::SeqCst);
    /* Nowhere clear: stay away from the pointer at least. */
    let (x, y) = spot
        .or_else(|| corners.into_iter().find(free_of_pointer))
        .unwrap_or(corners[0]);
    let spot = (x.round() as i32, y.round() as i32);
    *state.spot.lock().unwrap() = Some(spot);
    let _ = window.set_position(tauri::PhysicalPosition::new(spot.0, spot.1));
}

/// Where the toast goes when it is shown.
#[derive(Clone, Copy, PartialEq)]
enum Placement {
    /// A corner clear of the pointer and of the area being captured.
    Corner,
    /// Where the last toast was put. Picking a corner again once the
    /// mouse is free can land somewhere else, and the notice would jump.
    Same,
}

fn show(app: &AppHandle, toast: Toast) {
    show_at(app, toast, Placement::Corner);
}

fn show_at(app: &AppHandle, toast: Toast, placement: Placement) {
    let previous = app
        .state::<ToastState>()
        .current
        .lock()
        .unwrap()
        .replace(toast.clone());
    /* A newer capture replaces the toast; the older one is already saved. */
    if let Some(ready) = previous.and_then(|t| t.capture) {
        capture::release(app, &ready.id);
    }
    let window = match toast_window(app) {
        Ok(window) => window,
        Err(err) => {
            eprintln!("{err}");
            return;
        }
    };
    let spot = *app.state::<ToastState>().spot.lock().unwrap();
    match spot.filter(|_| placement == Placement::Same) {
        /* Also brings it back if it was moved off-screen for a capture. */
        Some((x, y)) => {
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
        None => place(app, &window),
    }
    let _ = window.emit_to(TOAST_LABEL, "toast-show", &toast);
    let _ = window.show();
}

/// Scrolling Capture: how many pages are saved so far, with the latest.
pub fn show_scroll_progress(app: &AppHandle, pages: u32, latest: &RgbaImage, pointer_locked: bool) {
    show(
        app,
        Toast {
            id: uuid::Uuid::new_v4().to_string(),
            thumbnail: Some(thumbnail(latest)),
            pages: Some(pages),
            running: true,
            pointer_locked,
            ..Default::default()
        },
    );
}

/// Scrolling Capture stopped and is writing the long image and text;
/// `stage` says which.
pub fn show_scroll_saving(
    app: &AppHandle,
    pages: u32,
    latest: &RgbaImage,
    note: &str,
    stage: &str,
) {
    show_at(
        app,
        Toast {
            id: uuid::Uuid::new_v4().to_string(),
            thumbnail: Some(thumbnail(latest)),
            pages: Some(pages),
            running: true,
            saving: true,
            stage: Some(stage.to_string()),
            note: Some(note.to_string()),
            ..Default::default()
        },
        Placement::Same,
    );
}

/// Scrolling Capture finished: the folder of pages, and why it stopped.
pub fn show_scroll_done(
    app: &AppHandle,
    dir: String,
    title: String,
    pages: u32,
    first: Option<RgbaImage>,
    note: String,
) {
    let file_name = std::path::Path::new(&dir)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    show_at(
        app,
        Toast {
            id: uuid::Uuid::new_v4().to_string(),
            path: Some(dir),
            file_name,
            thumbnail: first.as_ref().map(thumbnail),
            pages: Some(pages),
            note: Some(note),
            title: Some(title),
            ..Default::default()
        },
        Placement::Same,
    );
}

pub fn show_error(app: &AppHandle, error: String) {
    show(
        app,
        Toast {
            id: uuid::Uuid::new_v4().to_string(),
            error: Some(error),
            ..Default::default()
        },
    );
}

/// The toast the window should show, for a page that just loaded.
pub fn pending(app: &AppHandle) -> Option<Toast> {
    app.state::<ToastState>().current.lock().unwrap().clone()
}

/// Take the toast down if it is still the one with this id.
fn take(app: &AppHandle, id: &str) -> Option<Toast> {
    let state = app.state::<ToastState>();
    let mut current = state.current.lock().unwrap();
    if current.as_ref().is_some_and(|t| t.id == id) {
        if let Some(window) = app.get_webview_window(TOAST_LABEL) {
            let _ = window.hide();
        }
        current.take()
    } else {
        None
    }
}

/// The toast timed out or was closed: forget its capture.
pub fn dismiss(app: &AppHandle, id: &str) {
    if let Some(ready) = take(app, id).and_then(|t| t.capture) {
        capture::release(app, &ready.id);
    }
}

/// Edit: open the saved capture in a new editor tab.
pub fn edit(app: &AppHandle, id: &str) {
    if let Some(ready) = take(app, id).and_then(|t| t.capture) {
        capture::open_stored(app, ready);
    }
}

#[cfg(test)]
mod tests {
    use super::file_stem;
    use chrono::{Local, TimeZone};

    #[test]
    fn fills_in_the_template_like_the_editor() {
        let when = Local.with_ymd_and_hms(2026, 11, 19, 20, 41, 7).unwrap();
        assert_eq!(
            file_stem("Screenshot-{date}-{time}", when, 1440, 900),
            "Screenshot-2026-11-19-20.41.07"
        );
        assert_eq!(
            file_stem(
                "{year}{month}{day} {width}x{height} {nope}",
                when,
                1440,
                900
            ),
            "20261119 1440x900 {nope}"
        );
        assert_eq!(file_stem("", when, 1, 1), "Screenshot-2026-11-19-20.41.07");
        assert_eq!(file_stem("a/{hour}:{minute}", when, 1, 1), "a-20-41");
        assert_eq!(file_stem("{ unclosed", when, 1, 1), "{ unclosed");
    }
}

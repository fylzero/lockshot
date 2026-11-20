//! Taking screenshots. A capture grabs every monitor first, then opens one
//! borderless overlay per monitor showing that frozen image, so the selection
//! is drawn over exactly what gets cropped and the overlays themselves never
//! end up in the shot.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::http::{Request, Response, StatusCode};
use tauri::{
    AppHandle, Emitter, Manager, UriSchemeContext, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use xcap::Monitor;

use crate::platform;

pub const OVERLAY_PREFIX: &str = "overlay-";

struct MonitorShot {
    image: Arc<RgbaImage>,
}

struct Session {
    id: String,
    shots: Vec<MonitorShot>,
}

#[derive(Default)]
pub struct CaptureState {
    session: Mutex<Option<Session>>,
    /// Finished captures the editor can load, by id.
    images: Mutex<HashMap<String, Arc<RgbaImage>>>,
    latest: Mutex<Option<String>>,
    names: Mutex<HashMap<String, String>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureReady {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// For an opened file, its name without extension; captures get a
    /// name from the template in the editor instead.
    pub name: Option<String>,
}

/// Tells an overlay page which frozen display to show.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStart {
    pub session: String,
    pub index: usize,
    pub width: u32,
    pub height: u32,
}

#[derive(Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct Bounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn monitor_bounds(monitor: &Monitor) -> Option<Bounds> {
    Some(Bounds {
        x: monitor.x().ok()?,
        y: monitor.y().ok()?,
        width: monitor.width().ok()?,
        height: monitor.height().ok()?,
    })
}

fn contains(bounds: &Bounds, (px, py): (i32, i32)) -> bool {
    px >= bounds.x
        && py >= bounds.y
        && px < bounds.x + bounds.width as i32
        && py < bounds.y + bounds.height as i32
}

fn capture_err(err: impl std::fmt::Display) -> String {
    format!("Could not capture the screen: {err}")
}

fn ensure_permission(app: &AppHandle) -> Result<(), String> {
    if platform::can_capture() {
        return Ok(());
    }
    platform::request_screen_permission();
    crate::show_main(app, Some("/settings/permissions"));
    Err("Lockshot needs Screen Recording permission before it can capture.".into())
}

fn store(app: &AppHandle, image: RgbaImage, name: Option<String>) -> CaptureReady {
    let state = app.state::<CaptureState>();
    let id = uuid::Uuid::new_v4().to_string();
    let ready = CaptureReady {
        id: id.clone(),
        width: image.width(),
        height: image.height(),
        name: name.clone(),
    };
    if let Some(name) = name {
        state.names.lock().unwrap().insert(id.clone(), name);
    }
    let mut images = state.images.lock().unwrap();
    /*
     * Each editor tab frees its capture when closed (release). This cap only
     * guards against tabs that never say so; screenshots are large.
     */
    if images.len() >= 32 {
        let latest = state.latest.lock().unwrap().clone();
        images.retain(|key, _| Some(key) == latest.as_ref());
    }
    images.insert(id.clone(), Arc::new(image));
    *state.latest.lock().unwrap() = Some(id);
    ready
}

/// Hand a finished capture to the editor window and bring it forward.
fn deliver(app: &AppHandle, image: RgbaImage, name: Option<String>) {
    let ready = store(app, image, name);
    /* The page opens a new editor tab for it when the event arrives. */
    crate::show_main(app, None);
    let _ = app.emit_to("main", "capture-ready", ready);
}

/// The whole monitor under the pointer, straight to the editor.
pub fn capture_screen(app: &AppHandle) -> Result<(), String> {
    ensure_permission(app)?;
    let monitors = Monitor::all().map_err(capture_err)?;
    let point = platform::cursor_point(app);
    let monitor = monitors
        .iter()
        .find(|m| point.is_some_and(|p| monitor_bounds(m).is_some_and(|b| contains(&b, p))))
        .or_else(|| monitors.iter().find(|m| m.is_primary().unwrap_or(false)))
        .or(monitors.first())
        .ok_or_else(|| capture_err("no displays found"))?;
    let image = monitor.capture_image().map_err(capture_err)?;
    deliver(app, image, None);
    Ok(())
}

/// Load an image file from disk into the editor, as if it had been captured.
pub fn open_file(app: &AppHandle, path: &std::path::Path) -> Result<(), String> {
    let image = crate::export::read_image(path)?;
    let name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty());
    deliver(app, image, name);
    Ok(())
}

fn overlay_label(index: usize) -> String {
    format!("{OVERLAY_PREFIX}{index}")
}

/// The display index an overlay window covers, from "overlay-<index>".
pub fn parse_overlay_label(label: &str) -> Option<usize> {
    label.strip_prefix(OVERLAY_PREFIX)?.parse().ok()
}

/// Print how long each step of a capture took, in dev builds.
fn lap(start: Instant, stage: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[capture] {stage}: {} ms", start.elapsed().as_millis());
    }
}

/**
 * Get the overlay window for a display, creating it the first time, and put
 * it over the display. Overlays are kept (hidden) between captures and made
 * ahead of time at launch, because building a webview and loading the page
 * is most of what made area capture slow to appear.
 */
fn place_overlay(
    app: &AppHandle,
    index: usize,
    bounds: &Bounds,
    focus: bool,
) -> Result<WebviewWindow, String> {
    let label = overlay_label(index);
    let window = match app.get_webview_window(&label) {
        Some(window) => window,
        None => {
            let builder =
                WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
                    .title("Lockshot Capture")
                    .decorations(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .shadow(false)
                    .visible(false)
                    .focused(focus)
                    .visible_on_all_workspaces(true)
                    /*
                     * Lockshot is a menu bar app, so the overlay appears without
                     * being the active window. Without this, macOS spends the
                     * first click activating it and the drag never reaches the page.
                     */
                    .accept_first_mouse(true);
            /* On macOS place it at its final frame from the start (points). */
            #[cfg(target_os = "macos")]
            let builder = builder
                .position(bounds.x as f64, bounds.y as f64)
                .inner_size(bounds.width as f64, bounds.height as f64);
            builder
                .build()
                .map_err(|err| format!("Could not open the capture overlay: {err}"))?
        }
    };
    /*
     * xcap reports bounds in points on macOS and physical pixels elsewhere,
     * which is what each platform's window placement expects. Displays can
     * be rearranged between captures, so this runs every time.
     */
    if cfg!(target_os = "macos") {
        let _ = window.set_position(tauri::LogicalPosition::new(bounds.x, bounds.y));
        let _ = window.set_size(tauri::LogicalSize::new(bounds.width, bounds.height));
    } else {
        let _ = window.set_position(tauri::PhysicalPosition::new(bounds.x, bounds.y));
        let _ = window.set_size(tauri::PhysicalSize::new(bounds.width, bounds.height));
    }
    platform::raise_overlay(&window);
    Ok(window)
}

/// Create the hidden overlays for the current displays, so the first area
/// capture opens as fast as later ones.
pub fn prewarm(app: &AppHandle) {
    let Ok(monitors) = Monitor::all() else {
        return;
    };
    for (index, monitor) in monitors.iter().enumerate() {
        if let Some(bounds) = monitor_bounds(monitor) {
            let _ = place_overlay(app, index, &bounds, false);
        }
    }
}

/// Freeze every monitor and show the selection overlays over them.
pub fn start_area(app: &AppHandle) -> Result<(), String> {
    let start = Instant::now();
    ensure_permission(app)?;
    cancel_area(app);

    let monitors = Monitor::all().map_err(capture_err)?;
    let point = platform::cursor_point(app);
    let mut shots = Vec::new();
    let mut placements = Vec::new();
    for monitor in &monitors {
        let Some(bounds) = monitor_bounds(monitor) else {
            continue;
        };
        let image = monitor.capture_image().map_err(capture_err)?;
        let under_pointer = point.is_some_and(|p| contains(&bounds, p));
        shots.push(MonitorShot {
            image: Arc::new(image),
        });
        placements.push((bounds, under_pointer));
    }
    if shots.is_empty() {
        return Err(capture_err("no displays found"));
    }
    lap(start, "displays captured");

    let id = uuid::Uuid::new_v4().simple().to_string();
    let starts: Vec<OverlayStart> = shots
        .iter()
        .enumerate()
        .map(|(index, shot)| OverlayStart {
            session: id.clone(),
            index,
            width: shot.image.width(),
            height: shot.image.height(),
        })
        .collect();
    *app.state::<CaptureState>().session.lock().unwrap() = Some(Session {
        id: id.clone(),
        shots,
    });

    let count = placements.len();
    for ((bounds, focus), overlay) in placements.into_iter().zip(starts) {
        place_overlay(app, overlay.index, &bounds, focus)?;
        /* A freshly created page asks with overlay_pending instead. */
        let _ = app.emit_to(overlay_label(overlay.index), "overlay-start", overlay);
    }
    /* Displays that went away keep their overlay hidden. */
    for (label, window) in app.webview_windows() {
        if parse_overlay_label(&label).is_some_and(|index| index >= count) {
            let _ = window.hide();
        }
    }
    lap(start, "overlays told to show");
    Ok(())
}

fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_PREFIX) {
            let _ = window.hide();
        }
    }
}

pub fn cancel_area(app: &AppHandle) {
    *app.state::<CaptureState>().session.lock().unwrap() = None;
    close_overlays(app);
}

/// What an overlay page should show right now, for a page that loaded after
/// the capture started and missed the event.
pub fn overlay_pending(app: &AppHandle, label: &str) -> Option<OverlayStart> {
    let index = parse_overlay_label(label)?;
    let state = app.state::<CaptureState>();
    let session = state.session.lock().unwrap();
    let current = session.as_ref()?;
    let shot = current.shots.get(index)?;
    Some(OverlayStart {
        session: current.id.clone(),
        index,
        width: shot.image.width(),
        height: shot.image.height(),
    })
}

/**
 * Crop the frozen monitor image to the selection. The rect comes in as
 * fractions of the overlay so the page never has to know the image's pixel
 * density.
 */
pub fn finish_area(
    app: &AppHandle,
    label: &str,
    session_id: &str,
    rect: Option<Rect>,
) -> Result<(), String> {
    let index = parse_overlay_label(label).ok_or("That capture overlay is not recognised.")?;
    let image = {
        let state = app.state::<CaptureState>();
        let mut session = state.session.lock().unwrap();
        if session.as_ref().is_none_or(|s| s.id != session_id) {
            return Err("This capture has already finished.".into());
        }
        let current = session.take().expect("checked above");
        current
            .shots
            .into_iter()
            .nth(index)
            .ok_or("That display is no longer available.")?
            .image
    };
    close_overlays(app);

    let cropped = match rect {
        None => Arc::try_unwrap(image).unwrap_or_else(|shared| (*shared).clone()),
        Some(rect) => {
            let (w, h) = (image.width() as f64, image.height() as f64);
            let (iw, ih) = (image.width(), image.height());
            let x = ((rect.x.clamp(0.0, 1.0) * w).round() as u32).min(iw - 1);
            let y = ((rect.y.clamp(0.0, 1.0) * h).round() as u32).min(ih - 1);
            let cw = ((rect.width.max(0.0) * w).round() as u32).clamp(1, iw - x);
            let ch = ((rect.height.max(0.0) * h).round() as u32).clamp(1, ih - y);
            image::imageops::crop_imm(image.as_ref(), x, y, cw, ch).to_image()
        }
    };
    deliver(app, cropped, None);
    Ok(())
}

/// Forget a capture whose editor tab was closed.
pub fn release(app: &AppHandle, id: &str) {
    let state = app.state::<CaptureState>();
    state.images.lock().unwrap().remove(id);
    state.names.lock().unwrap().remove(id);
    let mut latest = state.latest.lock().unwrap();
    if latest.as_deref() == Some(id) {
        *latest = None;
    }
}

pub fn latest(app: &AppHandle) -> Option<CaptureReady> {
    let state = app.state::<CaptureState>();
    let id = state.latest.lock().unwrap().clone()?;
    let images = state.images.lock().unwrap();
    let image = images.get(&id)?;
    let name = state.names.lock().unwrap().get(&id).cloned();
    Some(CaptureReady {
        width: image.width(),
        height: image.height(),
        name,
        id,
    })
}

/// Decode %XX escapes; anything malformed is kept as is.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/**
 * Serves capture pixels to the pages: `capture://localhost/image/<id>` for
 * finished captures and `capture://localhost/session/<id>/<index>` for the
 * frozen monitors behind the overlays, as raw RGBA bytes.
 */
pub fn protocol<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let app = ctx.app_handle();
    let state = app.state::<CaptureState>();
    /* The page builds these URLs with convertFileSrc, which percent-encodes
    the whole path, slashes included, so decode before splitting. */
    let path = percent_decode(request.uri().path());
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let image = match parts.as_slice() {
        ["image", id] => state.images.lock().unwrap().get(*id).cloned(),
        ["session", id, index] => state.session.lock().unwrap().as_ref().and_then(|s| {
            (s.id == *id)
                .then(|| index.parse::<usize>().ok())
                .flatten()
                .and_then(|i| s.shots.get(i).map(|shot| shot.image.clone()))
        }),
        _ => None,
    };
    let Some(image) = image else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("Access-Control-Allow-Origin", "*")
            .body(Vec::new())
            .unwrap();
    };
    /*
     * Raw RGBA rather than PNG: encoding several megapixels per display took
     * longer than everything else in a capture. The page knows the size and
     * draws the bytes straight into a canvas.
     */
    Response::builder()
        .header("Content-Type", "application/octet-stream")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(image.as_raw().clone())
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::{parse_overlay_label, percent_decode};

    #[test]
    fn decodes_convert_file_src_paths() {
        assert_eq!(percent_decode("/image%2Fabc-123"), "/image/abc-123");
        assert_eq!(
            percent_decode("/session%2Fdeadbeef%2F0"),
            "/session/deadbeef/0"
        );
        assert_eq!(percent_decode("/image/abc"), "/image/abc");
        assert_eq!(percent_decode("/bad%zz%"), "/bad%zz%");
    }

    #[test]
    fn parses_overlay_labels() {
        assert_eq!(parse_overlay_label("overlay-2"), Some(2));
        assert_eq!(parse_overlay_label("overlay-x"), None);
        assert_eq!(parse_overlay_label("main"), None);
    }
}

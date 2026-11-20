//! Taking screenshots. A capture grabs every monitor first, then opens one
//! borderless overlay per monitor showing that frozen image, so the selection
//! is drawn over exactly what gets cropped and the overlays themselves never
//! end up in the shot.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbaImage};
use serde::{Deserialize, Serialize};
use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, UriSchemeContext, WebviewUrl, WebviewWindowBuilder};
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
    /* Only the latest few are ever reopened; screenshots are large. */
    if images.len() >= 8 {
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
    crate::show_main(app, Some(&format!("/editor/{}", ready.id)));
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

/// Freeze every monitor and open the selection overlays over them.
pub fn start_area(app: &AppHandle) -> Result<(), String> {
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

    let id = uuid::Uuid::new_v4().simple().to_string();
    *app.state::<CaptureState>().session.lock().unwrap() = Some(Session {
        id: id.clone(),
        shots,
    });

    for (index, (bounds, focus)) in placements.into_iter().enumerate() {
        let label = format!("{OVERLAY_PREFIX}{id}-{index}");
        let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
            .title("Lockshot Capture")
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .focused(focus)
            .visible_on_all_workspaces(true);
        let window = builder
            .build()
            .map_err(|err| format!("Could not open the capture overlay: {err}"))?;
        /*
         * xcap reports bounds in points on macOS and physical pixels
         * elsewhere, which is what each platform's window placement expects.
         */
        if cfg!(target_os = "macos") {
            let _ = window.set_position(tauri::LogicalPosition::new(bounds.x, bounds.y));
            let _ = window.set_size(tauri::LogicalSize::new(bounds.width, bounds.height));
        } else {
            let _ = window.set_position(tauri::PhysicalPosition::new(bounds.x, bounds.y));
            let _ = window.set_size(tauri::PhysicalSize::new(bounds.width, bounds.height));
        }
        platform::raise_overlay(&window);
    }
    Ok(())
}

fn close_overlays(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(OVERLAY_PREFIX) {
            let _ = window.destroy();
        }
    }
}

pub fn cancel_area(app: &AppHandle) {
    *app.state::<CaptureState>().session.lock().unwrap() = None;
    close_overlays(app);
}

/// Parse "overlay-<session>-<index>" into its parts.
pub fn parse_overlay_label(label: &str) -> Option<(String, usize)> {
    let rest = label.strip_prefix(OVERLAY_PREFIX)?;
    let (session, index) = rest.rsplit_once('-')?;
    Some((session.to_string(), index.parse().ok()?))
}

/**
 * Crop the frozen monitor image to the selection. The rect comes in as
 * fractions of the overlay so the page never has to know the image's pixel
 * density.
 */
pub fn finish_area(app: &AppHandle, label: &str, rect: Option<Rect>) -> Result<(), String> {
    let (session_id, index) =
        parse_overlay_label(label).ok_or("That capture overlay is not recognised.")?;
    let image = {
        let state = app.state::<CaptureState>();
        let mut session = state.session.lock().unwrap();
        let current = session.take().ok_or("This capture has already finished.")?;
        if current.id != session_id {
            return Err("This capture has already finished.".into());
        }
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

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Sub)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| err.to_string())?;
    Ok(out.into_inner())
}

/**
 * Serves capture images to the pages: `capture://localhost/image/<id>` for
 * finished captures and `capture://localhost/session/<id>/<index>` for the
 * frozen monitors behind the overlays.
 */
pub fn protocol<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let app = ctx.app_handle();
    let state = app.state::<CaptureState>();
    let parts: Vec<&str> = request.uri().path().trim_matches('/').split('/').collect();
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
    match encode_png(&image) {
        Ok(bytes) => Response::builder()
            .header("Content-Type", "image/png")
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
            .body(bytes)
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(err.into_bytes())
            .unwrap(),
    }
}

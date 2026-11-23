//! Taking screenshots. A capture grabs every monitor first, then opens one
//! borderless overlay per monitor showing that frozen image, so the selection
//! is drawn over exactly what gets cropped and the overlays themselves never
//! end up in the shot. Capture Window uses the same overlays to pick a
//! window, then captures that window on its own, and Scrolling Capture uses
//! them to pick the window, or drag out the part of the screen, it scrolls.

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

use crate::{platform, window_list};

pub const OVERLAY_PREFIX: &str = "overlay-";

struct MonitorShot {
    image: Arc<RgbaImage>,
    bounds: Bounds,
    /// Capture Window and Scrolling Capture: the windows on this display,
    /// front to back.
    windows: Vec<WindowTarget>,
}

struct Session {
    id: String,
    mode: OverlayMode,
    shots: Vec<MonitorShot>,
}

impl Session {
    /// The shot of a display, by the display id in its overlay's label.
    fn index_of(&self, display: u32) -> Option<usize> {
        self.shots.iter().position(|shot| shot.bounds.id == display)
    }

    fn start(&self, index: usize) -> Option<OverlayStart> {
        let shot = self.shots.get(index)?;
        Some(OverlayStart {
            session: self.id.clone(),
            index,
            width: shot.image.width(),
            height: shot.image.height(),
            mode: self.mode,
            windows: shot.windows.clone(),
        })
    }
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

/// What a capture overlay does with a click.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OverlayMode {
    /// Drag to select part of the display.
    Area,
    /// Click the window to capture.
    Window,
    /// Click the part of a window that scrolls, or drag over part of the
    /// display, to scroll and capture it page by page.
    Scroll,
}

/// A window an overlay can highlight, in fractions of its display.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowTarget {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub label: String,
}

/// Tells an overlay page which frozen display to show.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayStart {
    pub session: String,
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub mode: OverlayMode,
    pub windows: Vec<WindowTarget>,
}

/// A point on an overlay, in fractions of its display.
#[derive(Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy)]
struct Bounds {
    /// The system's id for the display, which names its overlay window.
    id: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn monitor_bounds(monitor: &Monitor) -> Option<Bounds> {
    Some(Bounds {
        id: monitor.id().ok()?,
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

/// Keep a capture for the editor to load. `latest` marks it as the one a
/// page that is still loading should open.
pub fn store(
    app: &AppHandle,
    image: RgbaImage,
    name: Option<String>,
    latest: bool,
) -> CaptureReady {
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
    if latest {
        *state.latest.lock().unwrap() = Some(id);
    }
    ready
}

/**
 * Hand a finished capture to the editor window and bring it forward, or,
 * with Save Without Editing on, write it straight to a file. Opened files
 * (which come with a name) always go to the editor.
 */
fn deliver(app: &AppHandle, image: RgbaImage, name: Option<String>) {
    if name.is_none() {
        if let Some(settings) = crate::quick_save::enabled(app) {
            crate::quick_save::save(app, image, settings);
            return;
        }
    }
    let ready = store(app, image, name, true);
    open_stored(app, ready);
}

/// Open a stored capture in a new editor tab.
pub fn open_stored(app: &AppHandle, ready: CaptureReady) {
    *app.state::<CaptureState>().latest.lock().unwrap() = Some(ready.id.clone());
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

/**
 * Each display keeps its own overlay window, named after the display rather
 * than its place in the list. The list can come back in another order (a
 * different main display, a lid opened or closed), and an overlay that had
 * to move to a display of another size or density could end up stretched
 * over the wrong screen (issue #31).
 */
fn overlay_label(display: u32) -> String {
    format!("{OVERLAY_PREFIX}{display}")
}

/// The display id an overlay window covers, from "overlay-<display id>".
pub fn parse_overlay_label(label: &str) -> Option<u32> {
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
fn place_overlay(app: &AppHandle, bounds: &Bounds, focus: bool) -> Result<WebviewWindow, String> {
    let label = overlay_label(bounds.id);
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
     * Displays can be rearranged between captures, so this runs every time.
     * macOS sets the whole frame from the display's NSScreen at once. xcap
     * reports bounds in physical pixels elsewhere, which is what window
     * placement there expects; the size goes last so a change of display
     * density on the way cannot leave the window scaled.
     */
    #[cfg(target_os = "macos")]
    platform::place_overlay(
        &window,
        bounds.id,
        (bounds.x, bounds.y, bounds.width, bounds.height),
    );
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.set_position(tauri::PhysicalPosition::new(bounds.x, bounds.y));
        let _ = window.set_size(tauri::PhysicalSize::new(bounds.width, bounds.height));
    }
    Ok(window)
}

/// Create the hidden overlays for the current displays, so the first area
/// capture opens as fast as later ones.
pub fn prewarm(app: &AppHandle) {
    let Ok(monitors) = Monitor::all() else {
        return;
    };
    for monitor in &monitors {
        if let Some(bounds) = monitor_bounds(monitor) {
            let _ = place_overlay(app, &bounds, false);
        }
    }
}

/// The windows that overlap a display, placed in fractions of it.
fn windows_on(bounds: &Bounds, windows: &[window_list::AppWindow]) -> Vec<WindowTarget> {
    let (bx, by) = (bounds.x as f64, bounds.y as f64);
    let (bw, bh) = (bounds.width as f64, bounds.height as f64);
    windows
        .iter()
        .filter(|w| w.x < bx + bw && w.y < by + bh && w.x + w.width > bx && w.y + w.height > by)
        .map(|w| WindowTarget {
            id: w.id,
            x: (w.x - bx) / bw,
            y: (w.y - by) / bh,
            width: w.width / bw,
            height: w.height / bh,
            label: w.label.clone(),
        })
        .collect()
}

/// Freeze every monitor and show the selection overlays over them.
pub fn start_area(app: &AppHandle) -> Result<(), String> {
    start_overlays(app, OverlayMode::Area)
}

/// Freeze every monitor and let the user click the window to capture.
pub fn start_window(app: &AppHandle) -> Result<(), String> {
    start_overlays(app, OverlayMode::Window)
}

/**
 * Scrolling Capture: let the user select the part of the screen to scroll. It
 * needs to move the pointer and scroll, which macOS gates behind
 * Accessibility and Wayland does not allow. The hotkey pressed again while
 * one is running stops it.
 */
pub fn start_scroll(app: &AppHandle) -> Result<(), String> {
    if crate::scrolling::is_running() {
        crate::scrolling::stop();
        return Ok(());
    }
    match platform::input_permission() {
        "unsupported" => {
            return Err("Scrolling Capture needs an X11 session: Wayland does not let apps scroll other apps' windows.".into())
        }
        "denied" => {
            platform::request_input_permission();
            crate::show_main(app, Some("/settings/permissions"));
            return Err("Lockshot needs Accessibility permission to scroll other apps' windows.".into());
        }
        _ => {}
    }
    start_overlays(app, OverlayMode::Scroll)
}

fn start_overlays(app: &AppHandle, mode: OverlayMode) -> Result<(), String> {
    let start = Instant::now();
    ensure_permission(app)?;
    cancel_area(app);

    let windows = if mode != OverlayMode::Area {
        window_list::all()
    } else {
        Vec::new()
    };
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
            bounds,
            windows: windows_on(&bounds, &windows),
        });
        placements.push((bounds, under_pointer));
    }
    if shots.is_empty() {
        return Err(capture_err("no displays found"));
    }
    lap(start, "displays captured");

    let session = Session {
        id: uuid::Uuid::new_v4().simple().to_string(),
        mode,
        shots,
    };
    let starts: Vec<OverlayStart> = (0..session.shots.len())
        .filter_map(|index| session.start(index))
        .collect();
    *app.state::<CaptureState>().session.lock().unwrap() = Some(session);

    let displays: Vec<u32> = placements.iter().map(|(bounds, _)| bounds.id).collect();
    for ((bounds, focus), overlay) in placements.into_iter().zip(starts) {
        place_overlay(app, &bounds, focus)?;
        /* A freshly created page asks with overlay_pending instead. */
        let _ = app.emit_to(overlay_label(bounds.id), "overlay-start", overlay);
    }
    /* Displays that went away keep their overlay hidden. */
    for (label, window) in app.webview_windows() {
        if parse_overlay_label(&label).is_some_and(|display| !displays.contains(&display)) {
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

/// Put an overlay back over its display, for a page that found its window
/// was not the shape of the display it shows.
pub fn refit_overlay(app: &AppHandle, label: &str) -> Result<(), String> {
    let display = parse_overlay_label(label).ok_or("That capture overlay is not recognised.")?;
    let bounds = {
        let state = app.state::<CaptureState>();
        let session = state.session.lock().unwrap();
        let session = session
            .as_ref()
            .ok_or("This capture has already finished.")?;
        let index = session
            .index_of(display)
            .ok_or("That display is no longer available.")?;
        session.shots[index].bounds
    };
    place_overlay(app, &bounds, false).map(|_| ())
}

/// What an overlay page should show right now, for a page that loaded after
/// the capture started and missed the event.
pub fn overlay_pending(app: &AppHandle, label: &str) -> Option<OverlayStart> {
    let display = parse_overlay_label(label)?;
    let state = app.state::<CaptureState>();
    let session = state.session.lock().unwrap();
    let session = session.as_ref()?;
    session.start(session.index_of(display)?)
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
    let image = take_shot(app, label, session_id)?.image;
    close_overlays(app);

    let cropped = match rect {
        None => Arc::try_unwrap(image).unwrap_or_else(|shared| (*shared).clone()),
        Some(rect) => crop(&image, &rect),
    };
    deliver(app, cropped, None);
    Ok(())
}

/// End the session and hand back the frozen display the overlay covered.
fn take_shot(app: &AppHandle, label: &str, session_id: &str) -> Result<MonitorShot, String> {
    let display = parse_overlay_label(label).ok_or("That capture overlay is not recognised.")?;
    let state = app.state::<CaptureState>();
    let mut session = state.session.lock().unwrap();
    if session.as_ref().is_none_or(|s| s.id != session_id) {
        return Err("This capture has already finished.".into());
    }
    let current = session.take().expect("checked above");
    current
        .shots
        .into_iter()
        .find(|shot| shot.bounds.id == display)
        .ok_or_else(|| "That display is no longer available.".into())
}

/// Part of an image, from a rect in fractions of it.
fn crop(image: &RgbaImage, rect: &Rect) -> RgbaImage {
    let (w, h) = (image.width() as f64, image.height() as f64);
    let (iw, ih) = (image.width(), image.height());
    let x = ((rect.x.clamp(0.0, 1.0) * w).round() as u32).min(iw - 1);
    let y = ((rect.y.clamp(0.0, 1.0) * h).round() as u32).min(ih - 1);
    let right = (rect.x + rect.width).clamp(0.0, 1.0) * w;
    let bottom = (rect.y + rect.height).clamp(0.0, 1.0) * h;
    let cw = ((right.round() as u32).saturating_sub(x)).clamp(1, iw - x);
    let ch = ((bottom.round() as u32).saturating_sub(y)).clamp(1, ih - y);
    image::imageops::crop_imm(image, x, y, cw, ch).to_image()
}

/**
 * Capture the window the user clicked, on its own, so windows in front of
 * it and the overlays never show. If the system will not capture it alone,
 * fall back to cutting it out of the frozen display.
 */
pub fn finish_window(
    app: &AppHandle,
    label: &str,
    session_id: &str,
    window_id: u32,
) -> Result<(), String> {
    let shot = take_shot(app, label, session_id)?;
    close_overlays(app);
    let target = shot
        .windows
        .iter()
        .find(|w| w.id == window_id)
        .ok_or("That window is no longer available.")?;
    /* X11 reads window pixels off the screen, so let the overlay go first. */
    if cfg!(target_os = "linux") {
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    let image = match window_list::capture(window_id) {
        Ok(image) if image.width() > 1 && image.height() > 1 => image,
        _ => crop(
            &shot.image,
            &Rect {
                x: target.x,
                y: target.y,
                width: target.width,
                height: target.height,
            },
        ),
    };
    deliver(app, image, None);
    Ok(())
}

/**
 * Scrolling Capture: the user selected the part of the screen to scroll, as
 * fractions of the display the overlay covered, or clicked for all of it.
 * `spot`: they picked a window by clicking the part of it that scrolls, and
 * `rect` is that window; the scroll stays on the spot they clicked.
 */
pub fn finish_scroll(
    app: &AppHandle,
    label: &str,
    session_id: &str,
    rect: Option<Rect>,
    spot: Option<Point>,
) -> Result<(), String> {
    let shot = take_shot(app, label, session_id)?;
    close_overlays(app);
    let rect = rect.unwrap_or(Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    });
    let b = shot.bounds;
    let (bw, bh) = (b.width as f64, b.height as f64);
    let left = (rect.x.clamp(0.0, 1.0) * bw).round() as u32;
    let top = (rect.y.clamp(0.0, 1.0) * bh).round() as u32;
    let right = (((rect.x + rect.width).clamp(0.0, 1.0) * bw).round() as u32).min(b.width);
    let bottom = (((rect.y + rect.height).clamp(0.0, 1.0) * bh).round() as u32).min(b.height);
    if right < left + 8 || bottom < top + 8 {
        return Err("Select a larger area to scroll.".into());
    }
    /* A spot that somehow fell outside the area is dropped for the usual
    search from the middle. */
    let spot = spot
        .map(|p| {
            (
                (p.x.clamp(0.0, 1.0) * bw).round() as u32,
                (p.y.clamp(0.0, 1.0) * bh).round() as u32,
            )
        })
        .filter(|&(x, y)| (left..right).contains(&x) && (top..bottom).contains(&y));
    /* The window under where it will scroll: the one clicked, or the one
    under the middle of a dragged area, front to back as the overlay showed. */
    let (fx, fy) = match spot {
        Some((x, y)) => (x as f64 / bw, y as f64 / bh),
        None => (
            (left + right) as f64 / 2.0 / bw,
            (top + bottom) as f64 / 2.0 / bh,
        ),
    };
    let window = shot
        .windows
        .iter()
        .find(|w| fx >= w.x && fy >= w.y && fx < w.x + w.width && fy < w.y + w.height)
        .map(|w| crate::scrolling::Window {
            id: w.id,
            x: w.x * bw,
            y: w.y * bh,
            width: w.width * bw,
            height: w.height * bh,
        });
    crate::quick_save::set_avoid(app, Some((rect.x, rect.y, rect.width, rect.height)));
    crate::scrolling::start(
        app,
        crate::scrolling::Target {
            display: (b.x, b.y),
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
            spot,
            window,
        },
    )
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

//! Where the main window opens: on the virtual desktop the user is on, and
//! on the screen they captured on (or the one picked in Settings), instead
//! of wherever it was last shown.

use tauri::{AppHandle, Manager, Monitor, WebviewWindow};

use crate::commands::AppState;
use crate::platform;

/// A rect in the units window placement takes on this platform: points on
/// macOS, physical pixels elsewhere. The same units `cursor_point` uses.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn contains(&self, (px, py): (f64, f64)) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.width && py < self.y + self.height
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// macOS places windows in points, so physical values from Tauri are scaled
/// back down; everywhere else they are used as they are.
fn native(value: f64, scale: f64) -> f64 {
    if cfg!(target_os = "macos") {
        value / scale
    } else {
        value
    }
}

fn monitor_rect(monitor: &Monitor) -> Rect {
    let scale = monitor.scale_factor();
    let (pos, size) = (monitor.position(), monitor.size());
    Rect {
        x: native(pos.x as f64, scale),
        y: native(pos.y as f64, scale),
        width: native(size.width as f64, scale),
        height: native(size.height as f64, scale),
    }
}

/// The part of a screen not covered by the menu bar, Dock or taskbar.
fn work_rect(monitor: &Monitor) -> Rect {
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    Rect {
        x: native(area.position.x as f64, scale),
        y: native(area.position.y as f64, scale),
        width: native(area.size.width as f64, scale),
        height: native(area.size.height as f64, scale),
    }
}

/**
 * A name for each screen, as Settings lists them. Two screens of the same
 * model share a name, so the second gets " (2)" and so on, in the order the
 * system lists them.
 */
fn labels(names: &[String]) -> Vec<String> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let repeat = names[..index].iter().filter(|other| *other == name).count();
            if repeat == 0 {
                name.clone()
            } else {
                format!("{name} ({})", repeat + 1)
            }
        })
        .collect()
}

fn monitor_names(monitors: &[Monitor]) -> Vec<String> {
    let names: Vec<String> = monitors
        .iter()
        .enumerate()
        .map(|(index, m)| {
            m.name()
                .filter(|name| !name.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| format!("Screen {}", index + 1))
        })
        .collect();
    labels(&names)
}

/// The connected screens, by the names Settings shows and saves.
pub fn screen_names(app: &AppHandle) -> Vec<String> {
    app.available_monitors()
        .map(|monitors| monitor_names(&monitors))
        .unwrap_or_default()
}

/// The screen the window should open on: the one picked in Settings if it
/// is connected, else the one under the pointer, else the primary one.
fn target_monitor(app: &AppHandle) -> Option<Monitor> {
    let monitors = app.available_monitors().ok()?;
    let locked = app
        .try_state::<AppState>()
        .map(|state| state.settings.lock().unwrap().editor_screen.clone())
        .unwrap_or_default();
    if !locked.is_empty() {
        let names = monitor_names(&monitors);
        if let Some(index) = names.iter().position(|name| *name == locked) {
            return monitors.into_iter().nth(index);
        }
    }
    let point = platform::cursor_point(app).map(|(x, y)| (x as f64, y as f64));
    if let Some(point) = point {
        if let Some(index) = monitors.iter().position(|m| monitor_rect(m).contains(point)) {
            return monitors.into_iter().nth(index);
        }
    }
    app.primary_monitor().ok().flatten()
}

/**
 * Where to put a window of `size` (native units) so it is centred on
 * `area`, shrunk to fit if the screen is smaller than the window.
 */
fn centred(area: Rect, width: f64, height: f64) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2.0,
        y: area.y + (area.height - height) / 2.0,
        width,
        height,
    }
}

/**
 * Move the main window to the target screen, keeping its size, unless it is
 * already there (then the user's own placement stays). Run before showing.
 */
pub fn place_main(app: &AppHandle, window: &WebviewWindow) {
    if window.is_fullscreen().unwrap_or(false) {
        return;
    }
    let Some(monitor) = target_monitor(app) else {
        return;
    };
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let current = Rect {
        x: native(pos.x as f64, scale),
        y: native(pos.y as f64, scale),
        width: native(size.width as f64, scale),
        height: native(size.height as f64, scale),
    };
    if monitor_rect(&monitor).contains(current.center()) {
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        let _ = window.unmaximize();
    }
    /*
     * Keep the window the same size on screen: in points on macOS, and on
     * other systems scaled from this screen's pixel density to the new one.
     */
    let ratio = if cfg!(target_os = "macos") {
        1.0
    } else {
        monitor.scale_factor() / scale
    };
    let next = centred(work_rect(&monitor), current.width * ratio, current.height * ratio);
    if cfg!(target_os = "macos") {
        let _ = window.set_position(tauri::LogicalPosition::new(next.x, next.y));
        if next.width < current.width || next.height < current.height {
            let _ = window.set_size(tauri::LogicalSize::new(next.width, next.height));
        }
    } else {
        let _ = window.set_position(tauri::PhysicalPosition::new(
            next.x.round() as i32,
            next.y.round() as i32,
        ));
        if next.width < current.width * ratio || next.height < current.height * ratio {
            let _ = window.set_size(tauri::PhysicalSize::new(
                next.width.round() as u32,
                next.height.round() as u32,
            ));
        }
    }
    if maximized {
        let _ = window.maximize();
    }
}

/**
 * Where to put a window of `width` × `height` (native units, frame included)
 * that was at `current`: centred where it was, shrunk to the screen if it is
 * bigger, and slid back onto the screen if it would hang off an edge.
 */
fn resized(area: Rect, current: Rect, width: f64, height: f64) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let (cx, cy) = current.center();
    Rect {
        x: (cx - width / 2.0).clamp(area.x, area.x + area.width - width),
        y: (cy - height / 2.0).clamp(area.y, area.y + area.height - height),
        width,
        height,
    }
}

/**
 * Size the main window so its content area is `width` × `height` logical
 * pixels, as big as the screen it is on allows. A full-screen window, or a
 * maximized one off macOS, is left alone: the user chose that size.
 */
pub fn size_main(window: &WebviewWindow, width: f64, height: f64) {
    if window.is_fullscreen().unwrap_or(false) {
        return;
    }
    /* macOS calls a window that fills the screen "zoomed"; that is not a choice to keep. */
    if !cfg!(target_os = "macos") && window.is_maximized().unwrap_or(false) {
        return;
    }
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let (Ok(pos), Ok(outer), Ok(inner)) = (
        window.outer_position(),
        window.outer_size(),
        window.inner_size(),
    ) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let current = Rect {
        x: native(pos.x as f64, scale),
        y: native(pos.y as f64, scale),
        width: native(outer.width as f64, scale),
        height: native(outer.height as f64, scale),
    };
    /* The title bar and borders, which the content does not get. */
    let frame_w = native(outer.width.saturating_sub(inner.width) as f64, scale);
    let frame_h = native(outer.height.saturating_sub(inner.height) as f64, scale);
    /* Logical pixels are points on macOS and scaled pixels elsewhere. */
    let to_native = if cfg!(target_os = "macos") { 1.0 } else { scale };
    let next = resized(
        work_rect(&monitor),
        current,
        width * to_native + frame_w,
        height * to_native + frame_h,
    );
    let (content_w, content_h) = (next.width - frame_w, next.height - frame_h);
    if cfg!(target_os = "macos") {
        let _ = window.set_size(tauri::LogicalSize::new(content_w, content_h));
        let _ = window.set_position(tauri::LogicalPosition::new(next.x, next.y));
    } else {
        let _ = window.set_size(tauri::PhysicalSize::new(
            content_w.round() as u32,
            content_h.round() as u32,
        ));
        let _ = window.set_position(tauri::PhysicalPosition::new(
            next.x.round() as i32,
            next.y.round() as i32,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_screens_that_share_a_name() {
        let names = ["DELL", "Built-in", "DELL", "DELL"].map(String::from);
        assert_eq!(
            labels(&names),
            ["DELL", "Built-in", "DELL (2)", "DELL (3)"].map(String::from)
        );
    }

    #[test]
    fn resizes_around_the_window_and_stays_on_screen() {
        let area = Rect {
            x: 0.0,
            y: 25.0,
            width: 1600.0,
            height: 900.0,
        };
        let current = Rect {
            x: 400.0,
            y: 200.0,
            width: 800.0,
            height: 600.0,
        };
        /* Grows around its centre while there is room. */
        assert_eq!(
            resized(area, current, 1000.0, 600.0),
            Rect {
                x: 300.0,
                y: 200.0,
                width: 1000.0,
                height: 600.0
            }
        );
        /* Slides back on screen rather than hang off the bottom. */
        let low = Rect {
            y: 300.0,
            ..current
        };
        assert_eq!(
            resized(area, low, 800.0, 800.0),
            Rect {
                x: 400.0,
                y: 125.0,
                width: 800.0,
                height: 800.0
            }
        );
        /* Never bigger than the screen. */
        assert_eq!(resized(area, current, 3000.0, 2000.0), area);
    }

    #[test]
    fn centres_and_shrinks_to_fit() {
        let area = Rect {
            x: 1000.0,
            y: 25.0,
            width: 1600.0,
            height: 900.0,
        };
        assert_eq!(
            centred(area, 1200.0, 800.0),
            Rect {
                x: 1200.0,
                y: 75.0,
                width: 1200.0,
                height: 800.0
            }
        );
        assert_eq!(
            centred(area, 2000.0, 1000.0),
            Rect {
                x: 1000.0,
                y: 25.0,
                width: 1600.0,
                height: 900.0
            }
        );
    }
}

//! The border around the area a Scrolling Capture is capturing. It is four
//! thin windows just outside the area, so it never shows up in the pages and
//! never catches the scroll. Separate windows rather than one see-through
//! window, since see-through windows need macOS's private API. Between
//! scrolls the pointer rests on it (see scrolling.rs), so it takes the
//! pointer rather than letting it through to the app underneath.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const FRAME_PREFIX: &str = "frame-";
/// How thick the border is, in points on macOS and physical pixels elsewhere.
pub const THICKNESS: i32 = 4;

/// An area on the desktop, in the units xcap reports displays in (points on
/// macOS, physical pixels elsewhere).
#[derive(Clone, Copy)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// The four strips around `area`, leaving out any that would fall outside
/// `display` (an area against the screen's edge has no border on that side).
fn strips(area: Area, display: Area) -> Vec<Area> {
    let t = THICKNESS;
    let candidates = [
        /* Top and bottom span the corners too. */
        Area {
            x: area.x - t,
            y: area.y - t,
            width: area.width + 2 * t,
            height: t,
        },
        Area {
            x: area.x - t,
            y: area.y + area.height,
            width: area.width + 2 * t,
            height: t,
        },
        Area {
            x: area.x - t,
            y: area.y,
            width: t,
            height: area.height,
        },
        Area {
            x: area.x + area.width,
            y: area.y,
            width: t,
            height: area.height,
        },
    ];
    candidates
        .into_iter()
        .filter_map(|s| clip(s, display))
        .collect()
}

fn clip(strip: Area, display: Area) -> Option<Area> {
    let left = strip.x.max(display.x);
    let top = strip.y.max(display.y);
    let right = (strip.x + strip.width).min(display.x + display.width);
    let bottom = (strip.y + strip.height).min(display.y + display.height);
    (right > left && bottom > top).then_some(Area {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

/// Frame `area` on `display`. Call from a worker thread, not a command.
pub fn show(app: &AppHandle, area: Area, display: Area) {
    let strips = strips(area, display);
    for (index, strip) in strips.iter().enumerate() {
        let label = format!("{FRAME_PREFIX}{index}");
        let window = match app.get_webview_window(&label) {
            Some(window) => window,
            None => {
                let builder =
                    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
                        .title("Lockshot Scrolling Capture")
                        .decorations(false)
                        .resizable(false)
                        .always_on_top(true)
                        .skip_taskbar(true)
                        .shadow(false)
                        .visible(false)
                        .focused(false)
                        .focusable(false)
                        .visible_on_all_workspaces(true)
                        .background_color(tauri::window::Color(255, 255, 255, 255));
                /* On macOS make it at its final frame from the start (points). */
                #[cfg(target_os = "macos")]
                let builder = builder
                    .position(strip.x as f64, strip.y as f64)
                    .inner_size(strip.width as f64, strip.height as f64);
                match builder.build() {
                    Ok(window) => window,
                    Err(err) => {
                        eprintln!("Could not open the scrolling capture border: {err}");
                        return;
                    }
                }
            }
        };
        /* Hold the resting pointer, so no app under the border reacts to it. */
        let _ = window.set_ignore_cursor_events(false);
        /*
         * Size first, then position: macOS keeps a window's bottom-left
         * corner when it resizes, so resizing after placing it moves it.
         */
        if cfg!(target_os = "macos") {
            let _ = window.set_size(tauri::LogicalSize::new(strip.width, strip.height));
            let _ = window.set_position(tauri::LogicalPosition::new(strip.x, strip.y));
        } else {
            let _ = window.set_size(tauri::PhysicalSize::new(strip.width, strip.height));
            let _ = window.set_position(tauri::PhysicalPosition::new(strip.x, strip.y));
        }
        let _ = window.show();
    }
    /* Strips left over from a bigger frame stay hidden. */
    for (label, window) in app.webview_windows() {
        let index = label
            .strip_prefix(FRAME_PREFIX)
            .and_then(|i| i.parse::<usize>().ok());
        if index.is_some_and(|i| i >= strips.len()) {
            let _ = window.hide();
        }
    }
}

pub fn hide(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(FRAME_PREFIX) {
            let _ = window.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Area = Area {
        x: 0,
        y: 0,
        width: 1000,
        height: 800,
    };

    #[test]
    fn frames_the_area_from_outside() {
        let area = Area {
            x: 100,
            y: 100,
            width: 300,
            height: 200,
        };
        let s = strips(area, DISPLAY);
        assert_eq!(s.len(), 4);
        for strip in &s {
            let inside = strip.x < area.x + area.width
                && area.x < strip.x + strip.width
                && strip.y < area.y + area.height
                && area.y < strip.y + strip.height;
            assert!(!inside, "a strip overlaps the area");
        }
    }

    #[test]
    fn no_border_past_the_screen_edge() {
        assert!(strips(DISPLAY, DISPLAY).is_empty());
        let top_left = Area {
            x: 0,
            y: 0,
            width: 300,
            height: 200,
        };
        assert_eq!(strips(top_left, DISPLAY).len(), 2);
    }
}

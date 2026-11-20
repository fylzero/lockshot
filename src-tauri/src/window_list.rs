//! The app windows on screen that Capture Window can pick from, front to
//! back. Each comes with its bounds in the same units xcap uses for
//! monitors on that platform (points on macOS, physical pixels elsewhere),
//! so they line up with the overlays.

/// A window that can be captured on its own.
#[derive(Clone, Debug)]
pub struct AppWindow {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// "App — Title", or just the app when the window has no title.
    pub label: String,
}

/// Windows smaller than this (tooltips, status items, slivers) are skipped.
const MIN_SIZE: f64 = 40.0;

fn label(app: &str, title: &str) -> String {
    match (app.trim(), title.trim()) {
        ("", title) => title.to_string(),
        (app, "") => app.to_string(),
        (app, title) if app == title => app.to_string(),
        (app, title) => format!("{app} — {title}"),
    }
}

/**
 * macOS: read the window list straight from Core Graphics. xcap's own list
 * includes the menu bar, Dock and other system layers and re-reads the whole
 * list for every property, so it is both noisier and slower for this.
 */
#[cfg(target_os = "macos")]
pub fn all() -> Vec<AppWindow> {
    use std::ffi::c_void;

    use objc2_core_foundation::{CFDictionary, CFNumber, CFNumberType, CFString, CGRect};
    use objc2_core_graphics::{
        CGRectMakeWithDictionaryRepresentation, CGWindowListCopyWindowInfo, CGWindowListOption,
    };

    unsafe fn value(dict: &CFDictionary, key: &str) -> *const c_void {
        let key = CFString::from_str(key);
        dict.value((key.as_ref() as *const CFString).cast())
    }
    unsafe fn number(dict: &CFDictionary, key: &str) -> Option<f64> {
        let ptr = value(dict, key) as *const CFNumber;
        let mut out = 0f64;
        (!ptr.is_null()
            && (*ptr).value(CFNumberType::DoubleType, &mut out as *mut f64 as *mut c_void))
        .then_some(out)
    }
    unsafe fn string(dict: &CFDictionary, key: &str) -> String {
        let ptr = value(dict, key) as *const CFString;
        if ptr.is_null() {
            String::new()
        } else {
            (*ptr).to_string()
        }
    }

    let own_pid = std::process::id() as f64;
    let Some(list) = CGWindowListCopyWindowInfo(
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
        0,
    ) else {
        return Vec::new();
    };
    let mut windows = Vec::new();
    for i in 0..list.count() {
        unsafe {
            let ptr = list.value_at_index(i) as *const CFDictionary;
            if ptr.is_null() {
                continue;
            }
            let dict = &*ptr;
            /* Layer 0 is ordinary app windows; the menu bar, Dock, status
            items and the like all sit on higher layers. */
            if number(dict, "kCGWindowLayer") != Some(0.0)
                || number(dict, "kCGWindowAlpha").is_some_and(|a| a <= 0.0)
                || number(dict, "kCGWindowSharingState") == Some(0.0)
                || number(dict, "kCGWindowOwnerPID") == Some(own_pid)
            {
                continue;
            }
            let Some(id) = number(dict, "kCGWindowNumber") else {
                continue;
            };
            let bounds = value(dict, "kCGWindowBounds") as *const CFDictionary;
            let mut rect = CGRect::default();
            if bounds.is_null() || !CGRectMakeWithDictionaryRepresentation(Some(&*bounds), &mut rect)
            {
                continue;
            }
            if rect.size.width < MIN_SIZE || rect.size.height < MIN_SIZE {
                continue;
            }
            windows.push(AppWindow {
                id: id as u32,
                x: rect.origin.x,
                y: rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
                label: label(
                    &string(dict, "kCGWindowOwnerName"),
                    &string(dict, "kCGWindowName"),
                ),
            });
        }
    }
    windows
}

#[cfg(not(target_os = "macos"))]
pub fn all() -> Vec<AppWindow> {
    let Ok(list) = xcap::Window::all() else {
        return Vec::new();
    };
    let own_pid = std::process::id();
    let mut windows: Vec<(i32, AppWindow)> = list
        .iter()
        .filter_map(|w| {
            if w.is_minimized().unwrap_or(false) || w.pid().ok()? == own_pid {
                return None;
            }
            let (width, height) = (w.width().ok()? as f64, w.height().ok()? as f64);
            if width < MIN_SIZE || height < MIN_SIZE {
                return None;
            }
            let app = w.app_name().unwrap_or_default();
            let title = w.title().unwrap_or_default();
            /* The Windows desktop itself. */
            if title == "Program Manager" {
                return None;
            }
            Some((
                w.z().unwrap_or(0),
                AppWindow {
                    id: w.id().ok()?,
                    x: w.x().ok()? as f64,
                    y: w.y().ok()? as f64,
                    width,
                    height,
                    label: label(&app, &title),
                },
            ))
        })
        .collect();
    /* Higher z is nearer the front. */
    windows.sort_by(|a, b| b.0.cmp(&a.0));
    windows.into_iter().map(|(_, w)| w).collect()
}

/// Capture one window on its own, unobscured by whatever is in front of it.
pub fn capture(id: u32) -> Result<image::RgbaImage, String> {
    let window = xcap::Window::all()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|w| w.id().ok() == Some(id))
        .ok_or("That window has closed.")?;
    window.capture_image().map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::label;

    #[test]
    fn labels_windows() {
        assert_eq!(label("Safari", "Apple"), "Safari — Apple");
        assert_eq!(label("Finder", ""), "Finder");
        assert_eq!(label("", "Untitled"), "Untitled");
        assert_eq!(label("Notes", "Notes"), "Notes");
    }
}

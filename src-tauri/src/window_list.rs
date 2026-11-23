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
            && (*ptr).value(
                CFNumberType::DoubleType,
                &mut out as *mut f64 as *mut c_void,
            ))
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
            if bounds.is_null()
                || !CGRectMakeWithDictionaryRepresentation(Some(&*bounds), &mut rect)
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
    windows.sort_by_key(|a| std::cmp::Reverse(a.0));
    windows.into_iter().map(|(_, w)| w).collect()
}

/// Capture one window on its own, unobscured by whatever is in front of it.
pub fn capture(id: u32) -> Result<image::RgbaImage, String> {
    find(id)?.capture_image().map_err(|err| err.to_string())
}

/// The window with this id, to capture again and again.
pub fn find(id: u32) -> Result<xcap::Window, String> {
    xcap::Window::all()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|w| w.id().ok() == Some(id))
        .ok_or_else(|| "That window has closed.".to_string())
}

/// The id of the frontmost app window (not Lockshot's) under a point on
/// the desktop, in the units xcap uses.
pub fn frontmost_at(x: f64, y: f64) -> Option<u32> {
    all()
        .into_iter()
        .find(|w| x >= w.x && y >= w.y && x < w.x + w.width && y < w.y + w.height)
        .map(|w| w.id)
}

/**
 * Bring a window to the front, so what was in front of it (another app,
 * or Lockshot's own window, which comes forward when the capture overlay
 * closes) no longer covers it. Best effort: does nothing if the window is
 * gone or the system refuses.
 *
 * macOS raises it through Accessibility, which Scrolling Capture already
 * has, finding the app's window by its frame since Accessibility has no
 * public way to look one up by window number; then makes its app active,
 * since a window of an app in the background is only raised among that
 * app's own windows.
 */
#[cfg(target_os = "macos")]
pub fn bring_to_front(id: u32) {
    use std::ffi::c_void;

    use objc2_core_foundation::{CFRetained, CFString};

    type Ref = *const c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> Ref;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementSetAttributeValue(element: Ref, attribute: Ref, value: Ref) -> i32;
        fn AXUIElementPerformAction(element: Ref, action: Ref) -> i32;
        fn AXValueGetValue(value: Ref, kind: u32, out: *mut c_void) -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFBooleanTrue: Ref;
        fn CFArrayGetCount(array: Ref) -> isize;
        fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
        fn CFRelease(cf: Ref);
    }
    /// kAXValueCGPointType and kAXValueCGSizeType; a CGSize has the same
    /// layout as Point.
    const POINT: u32 = 1;
    const SIZE: u32 = 2;

    fn name(s: &str) -> CFRetained<CFString> {
        CFString::from_str(s)
    }
    fn ptr(s: &CFRetained<CFString>) -> Ref {
        (&**s as *const CFString).cast()
    }
    /// An attribute holding a point or size, read into a Point.
    unsafe fn pair(element: Ref, attribute: &str, kind: u32) -> Option<Point> {
        let attribute = name(attribute);
        let mut value: Ref = std::ptr::null();
        if AXUIElementCopyAttributeValue(element, ptr(&attribute), &mut value) != 0
            || value.is_null()
        {
            return None;
        }
        let mut out = Point::default();
        let ok = AXValueGetValue(value, kind, (&mut out as *mut Point).cast());
        CFRelease(value);
        ok.then_some(out)
    }

    let Ok(window) = find(id) else {
        return;
    };
    let (Ok(pid), Ok(x), Ok(y), Ok(w), Ok(h)) = (
        window.pid(),
        window.x(),
        window.y(),
        window.width(),
        window.height(),
    ) else {
        return;
    };
    unsafe {
        let app = AXUIElementCreateApplication(pid as i32);
        if app.is_null() {
            return;
        }
        let windows_name = name("AXWindows");
        let mut windows: Ref = std::ptr::null();
        if AXUIElementCopyAttributeValue(app, ptr(&windows_name), &mut windows) == 0
            && !windows.is_null()
        {
            let close = |a: f64, b: f64| (a - b).abs() <= 2.0;
            for i in 0..CFArrayGetCount(windows) {
                let element = CFArrayGetValueAtIndex(windows, i);
                let (Some(at), Some(size)) = (
                    pair(element, "AXPosition", POINT),
                    pair(element, "AXSize", SIZE),
                ) else {
                    continue;
                };
                if close(at.x, x as f64)
                    && close(at.y, y as f64)
                    && close(size.x, w as f64)
                    && close(size.y, h as f64)
                {
                    AXUIElementSetAttributeValue(element, ptr(&name("AXMain")), kCFBooleanTrue);
                    AXUIElementPerformAction(element, ptr(&name("AXRaise")));
                    break;
                }
            }
            CFRelease(windows);
        }
        AXUIElementSetAttributeValue(app, ptr(&name("AXFrontmost")), kCFBooleanTrue);
        CFRelease(app);
    }
}

/// Windows lets the app the user last clicked in (Lockshot's overlay, just
/// now) hand the foreground to another window.
#[cfg(target_os = "windows")]
pub fn bring_to_front(id: u32) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    let hwnd = HWND(id as usize as *mut std::ffi::c_void);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// X11 window managers differ too much to raise a window reliably.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn bring_to_front(_id: u32) {}

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

#[cfg(test)]
mod raise_check {
    #[test]
    #[ignore]
    fn raises_the_second_window_and_puts_it_back() {
        let list = super::all();
        let (first, second) = (&list[0], &list[1]);
        eprintln!("front: {} / second: {}", first.label, second.label);
        super::bring_to_front(second.id);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let now = super::all();
        eprintln!("after raise, front is: {}", now[0].label);
        super::bring_to_front(first.id);
        std::thread::sleep(std::time::Duration::from_millis(500));
        eprintln!("restored, front is: {}", super::all()[0].label);
        assert_eq!(now[0].id, second.id);
    }
}

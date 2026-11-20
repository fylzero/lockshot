//! The OS-specific bits: screen-recording permission, the pointer position in
//! the coordinate space xcap reports monitors in, and lifting the capture
//! overlay above the macOS menu bar.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    /// "granted", "denied", or "not-required" where the OS has no such gate.
    pub screen: &'static str,
    pub os: &'static str,
    /// Linux sessions under Wayland, where global hotkeys cannot be grabbed.
    pub wayland: bool,
}

pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland")))
}

pub fn permission_status() -> PermissionStatus {
    PermissionStatus {
        screen: screen_permission(),
        os: os_name(),
        wayland: is_wayland(),
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGPreflightScreenCaptureAccess() -> bool;
        pub fn CGRequestScreenCaptureAccess() -> bool;
        pub fn CGEventCreate(source: *const c_void) -> *mut c_void;
        pub fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
    }
}

#[cfg(target_os = "macos")]
fn screen_permission() -> &'static str {
    if unsafe { mac::CGPreflightScreenCaptureAccess() } {
        "granted"
    } else {
        "denied"
    }
}

#[cfg(not(target_os = "macos"))]
fn screen_permission() -> &'static str {
    "not-required"
}

/// Whether a capture would see other apps' windows rather than just the
/// wallpaper, which is what macOS hands back without the permission.
pub fn can_capture() -> bool {
    screen_permission() != "denied"
}

/**
 * Ask macOS for Screen Recording access. The first call shows the system
 * prompt and adds Lockshot to the list in System Settings; after that macOS
 * stays silent and the user has to flip the switch there.
 */
#[cfg(target_os = "macos")]
pub fn request_screen_permission() -> bool {
    unsafe { mac::CGRequestScreenCaptureAccess() }
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_permission() -> bool {
    true
}

/// The pointer in the units xcap uses for monitor bounds: points on macOS,
/// physical pixels elsewhere.
#[cfg(target_os = "macos")]
pub fn cursor_point(_app: &tauri::AppHandle) -> Option<(i32, i32)> {
    unsafe {
        let event = mac::CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let point = mac::CGEventGetLocation(event);
        mac::CFRelease(event as *const _);
        Some((point.x.round() as i32, point.y.round() as i32))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn cursor_point(app: &tauri::AppHandle) -> Option<(i32, i32)> {
    app.cursor_position()
        .ok()
        .map(|p| (p.x.round() as i32, p.y.round() as i32))
}

/**
 * Lift a window above the menu bar and Dock. Floating windows (what
 * always-on-top gives) sit below the menu bar, so the overlay would leave a
 * strip of live screen at the top that could not be selected.
 */
#[cfg(target_os = "macos")]
pub fn raise_overlay(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// NSScreenSaverWindowLevel
    const LEVEL: isize = 1000;
    /*
     * Captures run on a worker thread, and AppKit aborts the app if a window
     * is touched off the main thread, so hop over to it.
     */
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(ns_window) = target.ns_window() else {
            return;
        };
        let ns_window = ns_window as *mut AnyObject;
        if ns_window.is_null() {
            return;
        }
        unsafe {
            let _: () = msg_send![ns_window, setLevel: LEVEL];
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn raise_overlay(_window: &tauri::WebviewWindow) {}

/**
 * Put the app icon back on the Dock after switching from menu-bar-only to a
 * regular app. Bundled builds get it from the .app; an unbundled dev binary
 * only has the icon Tauri set at launch, which macOS drops while the app is
 * an accessory. Must run on the main thread.
 */
#[cfg(target_os = "macos")]
pub fn restore_dock_icon() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let bytes: &[u8] = include_bytes!("../icons/icon.png");
    unsafe {
        let data: *mut AnyObject = msg_send![
            class!(NSData),
            dataWithBytes: bytes.as_ptr() as *const std::ffi::c_void,
            length: bytes.len()
        ];
        if data.is_null() {
            return;
        }
        let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: image];
    }
}

/**
 * Give an unbundled dev binary its proper name in Cmd+Tab, the Dock and
 * Activity Monitor. LaunchServices names it after the executable file and
 * does not reliably read the Info.plist Tauri embeds, so the app switcher
 * shows "lockshot" or "exec". Bundled builds get the name from the .app.
 * Uses the same private LaunchServices call as Chromium's SetProcessName,
 * looked up at runtime so nothing private is linked.
 */
#[cfg(all(target_os = "macos", debug_assertions))]
pub fn set_dev_display_name(name: &str) {
    use std::ffi::{c_char, c_int, c_void, CString};

    type CFTypeRef = *const c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            s: *const c_char,
            encoding: u32,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }
    extern "C" {
        fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    const RTLD_LAZY: c_int = 1;
    const UTF8: u32 = 0x0800_0100;
    /// kLSDefaultSessionID
    const SESSION: c_int = -2;

    unsafe {
        let path = c"/System/Library/Frameworks/CoreServices.framework/CoreServices";
        let lib = dlopen(path.as_ptr(), RTLD_LAZY);
        if lib.is_null() {
            return;
        }
        let get_asn = dlsym(lib, c"_LSGetCurrentApplicationASN".as_ptr());
        let set_item = dlsym(lib, c"_LSSetApplicationInformationItem".as_ptr());
        let key = dlsym(lib, c"_kLSDisplayNameKey".as_ptr()) as *const CFTypeRef;
        if get_asn.is_null() || set_item.is_null() || key.is_null() {
            return;
        }
        let get_asn: extern "C" fn() -> CFTypeRef = std::mem::transmute(get_asn);
        let set_item: extern "C" fn(c_int, CFTypeRef, CFTypeRef, CFTypeRef, *mut CFTypeRef) -> i32 =
            std::mem::transmute(set_item);
        let asn = get_asn();
        let Ok(cname) = CString::new(name) else {
            return;
        };
        let value = CFStringCreateWithCString(std::ptr::null(), cname.as_ptr(), UTF8);
        if asn.is_null() || value.is_null() {
            return;
        }
        set_item(SESSION, asn, *key, value, std::ptr::null_mut());
        CFRelease(value);
    }
}

/// The page in the OS settings for each thing the setup guide points at.
pub fn settings_url(target: &str) -> Option<&'static str> {
    match (os_name(), target) {
        ("macos", "screen-recording") => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        }
        ("macos", "keyboard-shortcuts") => {
            Some("x-apple.systempreferences:com.apple.preference.keyboard?Shortcuts")
        }
        ("windows", "keyboard-shortcuts") => Some("ms-settings:easeofaccess-keyboard"),
        ("windows", "screen-recording") => Some("ms-settings:privacy-graphicscaptureprogrammatic"),
        _ => None,
    }
}

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
    /// Scrolling Capture's control of the mouse: "granted", "denied" (macOS
    /// Accessibility is off), "unsupported" (Wayland) or "not-required".
    pub input: &'static str,
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

pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland")))
}

pub fn permission_status() -> PermissionStatus {
    PermissionStatus {
        screen: screen_permission(),
        os: os_name(),
        wayland: is_wayland(),
        input: input_permission(),
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
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGEventCreate(source: *const c_void) -> *mut c_void;
        pub fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        pub fn CGWarpMouseCursorPosition(point: CGPoint) -> i32;
        pub fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
        pub fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub fn CFRelease(cf: *const c_void);
    }
}

/**
 * macOS: keep the mouse from moving the pointer, clicking or scrolling
 * while Scrolling Capture runs, whichever app is active. Unhooking the
 * pointer from the mouse (CGAssociateMouseAndMouseCursorPosition) only
 * lasts while Lockshot is the active app, and the run makes the app it
 * scrolls active. So an event tap drops the mouse's own events for the run
 * and puts the pointer back where Lockshot holds it, letting through the
 * moves and scrolls Lockshot posts itself through enigo. The tap runs on its own thread;
 * macOS turns it off by itself if Lockshot stops answering, and removes it
 * when Lockshot quits, so the mouse can never stay stuck.
 */
#[cfg(target_os = "macos")]
mod mouse_block {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, Ordering};
    use std::sync::Mutex;

    type Ref = *const c_void;
    type Callback = extern "C" fn(Ref, u32, Ref, *mut c_void) -> Ref;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events: u64,
            callback: Callback,
            info: *mut c_void,
        ) -> Ref;
        fn CGEventTapEnable(tap: Ref, enable: bool);
        fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopCommonModes: Ref;
        fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
        fn CFMachPortInvalidate(port: Ref);
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRun();
        fn CFRunLoopStop(run_loop: Ref);
    }

    /// kCGHIDEventTap, kCGHeadInsertEventTap, kCGEventTapOptionDefault.
    const HID_TAP: u32 = 0;
    const HEAD: u32 = 0;
    const FILTER: u32 = 0;
    /// kCGEventSourceUserData: enigo marks every event it posts with
    /// enigo::EVENT_MARKER here, which is how Lockshot's own get through.
    const USER_DATA: u32 = 42;
    /// The mouse's buttons, moves, drags and scroll wheel.
    const BUTTONS: [u32; 6] = [1, 2, 3, 4, 25, 26];
    const MOVES: [u32; 4] = [5, 6, 7, 27];
    const SCROLL: u32 = 22;
    /// Sent when macOS turned the tap off, for being slow or for input.
    const DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const DISABLED_BY_INPUT: u32 = 0xFFFF_FFFF;

    static ON: AtomicBool = AtomicBool::new(false);
    /// Where the pointer is held, as x and y in points packed into one value.
    static POINT: AtomicI64 = AtomicI64::new(0);
    static TAP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    static RUN_LOOP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
    /// Start and stop one at a time.
    static GUARD: Mutex<()> = Mutex::new(());

    fn pack(x: i32, y: i32) -> i64 {
        ((x as i64) << 32) | (y as u32 as i64)
    }

    fn held() -> super::mac::CGPoint {
        let v = POINT.load(Ordering::SeqCst);
        super::mac::CGPoint {
            x: (v >> 32) as i32 as f64,
            y: v as i32 as f64,
        }
    }

    extern "C" fn filter(_proxy: Ref, kind: u32, event: Ref, _info: *mut c_void) -> Ref {
        if kind == DISABLED_BY_TIMEOUT || kind == DISABLED_BY_INPUT {
            let tap = TAP.load(Ordering::SeqCst);
            if ON.load(Ordering::SeqCst) && !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }
        if !ON.load(Ordering::SeqCst)
            || unsafe { CGEventGetIntegerValueField(event, USER_DATA) }
                == enigo::EVENT_MARKER as i64
        {
            return event;
        }
        if MOVES.contains(&kind) {
            unsafe { super::mac::CGWarpMouseCursorPosition(held()) };
            return std::ptr::null();
        }
        if BUTTONS.contains(&kind) || kind == SCROLL {
            return std::ptr::null();
        }
        event
    }

    /// Hold the pointer at `(x, y)`, starting the tap if it is not running.
    /// Returns whether the tap is running.
    pub fn start(x: i32, y: i32) -> bool {
        POINT.store(pack(x, y), Ordering::SeqCst);
        let _guard = GUARD.lock().unwrap();
        if ON.load(Ordering::SeqCst) {
            return true;
        }
        let mask = BUTTONS
            .iter()
            .chain(&MOVES)
            .chain(&[SCROLL])
            .fold(0u64, |mask, kind| mask | (1 << kind));
        let tap =
            unsafe { CGEventTapCreate(HID_TAP, HEAD, FILTER, mask, filter, std::ptr::null_mut()) };
        if tap.is_null() {
            return false;
        }
        TAP.store(tap as *mut c_void, Ordering::SeqCst);
        ON.store(true, Ordering::SeqCst);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let port = tap as usize;
        std::thread::spawn(move || unsafe {
            let tap = port as Ref;
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            let run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            RUN_LOOP.store(run_loop as *mut c_void, Ordering::SeqCst);
            let _ = ready_tx.send(());
            CFRunLoopRun();
            CGEventTapEnable(tap, false);
            CFMachPortInvalidate(tap);
            super::mac::CFRelease(source);
            super::mac::CFRelease(tap);
        });
        /* Wait for the run loop, so a stop right after this finds it. */
        let _ = ready_rx.recv_timeout(std::time::Duration::from_secs(1));
        true
    }

    /// Give the mouse back. Safe to call when not running.
    pub fn stop() {
        let _guard = GUARD.lock().unwrap();
        if !ON.swap(false, Ordering::SeqCst) {
            return;
        }
        TAP.store(std::ptr::null_mut(), Ordering::SeqCst);
        let run_loop = RUN_LOOP.swap(std::ptr::null_mut(), Ordering::SeqCst);
        if !run_loop.is_null() {
            unsafe { CFRunLoopStop(run_loop) };
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn packs_negative_points() {
            for (x, y) in [(0, 0), (-1440, 900), (3000, -25)] {
                super::POINT.store(super::pack(x, y), std::sync::atomic::Ordering::SeqCst);
                let p = super::held();
                assert_eq!((p.x as i32, p.y as i32), (x, y));
            }
        }
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

/**
 * Whether Scrolling Capture may move the pointer and scroll. macOS asks for
 * Accessibility access before an app can send input to others; Wayland does
 * not let apps do it at all.
 */
#[cfg(target_os = "macos")]
pub fn input_permission() -> &'static str {
    if unsafe { mac::AXIsProcessTrusted() } {
        "granted"
    } else {
        "denied"
    }
}

#[cfg(not(target_os = "macos"))]
pub fn input_permission() -> &'static str {
    if is_wayland() {
        "unsupported"
    } else {
        "not-required"
    }
}

/// Show macOS's Accessibility prompt, which also adds Lockshot to the list
/// in System Settings. Silent once the user has answered it.
#[cfg(target_os = "macos")]
pub fn request_input_permission() {
    let settings = enigo::Settings {
        open_prompt_to_get_permissions: true,
        ..Default::default()
    };
    let _ = enigo::Enigo::new(&settings);
}

#[cfg(not(target_os = "macos"))]
pub fn request_input_permission() {}

/// Windows: put the pointer at a point in physical pixels on the virtual
/// desktop. enigo's own move only knows the primary display.
#[cfg(target_os = "windows")]
pub fn set_cursor(x: i32, y: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
}

/**
 * Scrolling Capture: hold the pointer at `(x, y)` so moving the mouse does
 * nothing until unlock_pointer; call it again to hold it somewhere else.
 * Lockshot's own moves and scroll events still go through. macOS drops the
 * mouse's own events (see mouse_block), and also unhooks the pointer from
 * the mouse while Lockshot is the active app. Windows pins it to that spot,
 * but lets go by itself on Ctrl+Alt+Del or an app switch, so call this
 * again before every scroll. Returns whether the pointer is locked; Linux
 * does not lock it.
 */
#[cfg(target_os = "macos")]
pub fn lock_pointer(x: i32, y: i32) -> bool {
    let blocked = mouse_block::start(x, y);
    let unhooked = unsafe {
        mac::CGWarpMouseCursorPosition(mac::CGPoint {
            x: x as f64,
            y: y as f64,
        });
        mac::CGAssociateMouseAndMouseCursorPosition(0) == 0
    };
    blocked || unhooked
}

#[cfg(target_os = "windows")]
pub fn lock_pointer(x: i32, y: i32) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::ClipCursor;
    let rect = RECT {
        left: x,
        top: y,
        right: x + 1,
        bottom: y + 1,
    };
    unsafe { ClipCursor(Some(&rect as *const RECT)).is_ok() }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn lock_pointer(_x: i32, _y: i32) -> bool {
    false
}

/// Whether Esc is held down right now, whichever app has the keyboard.
#[cfg(target_os = "macos")]
pub fn escape_down() -> bool {
    /// kCGEventSourceStateHIDSystemState, and the Esc key code.
    const HID_SYSTEM_STATE: i32 = 1;
    const ESCAPE: u16 = 0x35;
    unsafe { mac::CGEventSourceKeyState(HID_SYSTEM_STATE, ESCAPE) }
}

#[cfg(target_os = "windows")]
pub fn escape_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
    unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 }
}

/// Linux relies on the Esc hotkey alone.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn escape_down() -> bool {
    false
}

/// Give the mouse back after lock_pointer. Safe to call when not locked.
#[cfg(target_os = "macos")]
pub fn unlock_pointer() {
    mouse_block::stop();
    unsafe {
        mac::CGAssociateMouseAndMouseCursorPosition(1);
    }
}

#[cfg(target_os = "windows")]
pub fn unlock_pointer() {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::ClipCursor(None);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn unlock_pointer() {}

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
 * Put a capture overlay exactly over its display and lift it above the menu
 * bar and Dock. Floating windows (what always-on-top gives) sit below the
 * menu bar, so the overlay would leave a strip of live screen at the top that
 * could not be selected.
 *
 * The frame is set in one go from the display's own NSScreen. Moving and
 * then resizing (what Tauri's set_position and set_size do) keeps the
 * bottom-left corner fixed during the resize, so an overlay that changed to
 * a display of another height ended up shifted, part on the wrong screen
 * (issue #31). `bounds` (points, top-left origin) is only used if no
 * NSScreen matches the display.
 */
#[cfg(target_os = "macos")]
pub fn place_overlay(window: &tauri::WebviewWindow, display_id: u32, bounds: (i32, i32, u32, u32)) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{NSNumber, NSPoint, NSRect, NSSize, NSString};

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
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let screens = NSScreen::screens(mtm);
        let key = NSString::from_str("NSScreenNumber");
        let matched = screens.iter().find(|screen| {
            screen
                .deviceDescription()
                .objectForKey(&key)
                .and_then(|number| number.downcast::<NSNumber>().ok())
                .is_some_and(|number| number.unsignedIntValue() == display_id)
        });
        let frame = match matched {
            Some(screen) => screen.frame(),
            None => {
                /* AppKit puts the origin at the bottom-left of the first screen. */
                let Some(primary) = screens.iter().next() else {
                    return;
                };
                let (x, y, width, height) = bounds;
                let flip = primary.frame().size.height;
                NSRect::new(
                    NSPoint::new(x as f64, flip - y as f64 - height as f64),
                    NSSize::new(width as f64, height as f64),
                )
            }
        };
        unsafe {
            let _: () = msg_send![ns_window, setFrame: frame, display: false];
            let _: () = msg_send![ns_window, setLevel: LEVEL];
        }
    });
}

/**
 * Show a window fully transparent, or make it opaque again. A capture
 * overlay is kept hidden between captures, and macOS does not repaint a
 * hidden webview, so showing it straight away could flash the last capture's
 * frozen screen for a frame before the new one is drawn (issue #40). It is
 * shown see-through first and made opaque once the new frame has rendered.
 */
#[cfg(target_os = "macos")]
pub fn show_window_with_alpha(window: &tauri::WebviewWindow, alpha: f64, show: bool) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(ns_window) = target.ns_window() {
            let ns_window = ns_window as *mut AnyObject;
            if !ns_window.is_null() {
                unsafe {
                    /*
                     * A window at alpha 0 does not count as under the mouse,
                     * and it becomes key while still see-through, so AppKit
                     * put up the cursor of whatever was behind it: the arrow,
                     * until the mouse moved (issue #44). Taking mouse events
                     * regardless keeps it under the pointer from the start.
                     */
                    let _: () = msg_send![ns_window, setIgnoresMouseEvents: false];
                    let _: () = msg_send![ns_window, setAlphaValue: alpha];
                }
            }
        }
        /* After the alpha, on the same main thread turn, so it never shows opaque. */
        if show {
            let _ = target.show();
        }
    });
}

/*
 * Answers AppKit's cursorUpdate: with the crosshair. It owns a tracking area
 * over each capture overlay's webview; see `crosshair_over`.
 */
#[cfg(target_os = "macos")]
objc2::define_class!(
    #[unsafe(super(objc2::runtime::NSObject))]
    #[name = "LockshotCrosshair"]
    struct CrosshairOwner;

    impl CrosshairOwner {
        #[unsafe(method(cursorUpdate:))]
        fn cursor_update(&self, _event: *mut objc2::runtime::AnyObject) {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            unsafe {
                let cursor: *mut AnyObject = msg_send![class!(NSCursor), crosshairCursor];
                if !cursor.is_null() {
                    let _: () = msg_send![cursor, set];
                }
            }
        }
    }
);

/**
 * Show the crosshair over a capture overlay as soon as it appears. The page
 * asks for it in CSS, but WebKit only applies that on a mouse move. When the
 * overlay becomes the key window (and Lockshot the active app), AppKit asks
 * the view under the pointer for its cursor with cursorUpdate:, and WebKit
 * does not answer, so AppKit put up the arrow. Under a still pointer it stayed
 * the arrow until the mouse moved (issue #44). A cursor-update tracking area
 * over the whole webview answers with the crosshair. Call once per overlay.
 */
#[cfg(target_os = "macos")]
pub fn crosshair_over(window: &tauri::WebviewWindow) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, AnyThread};
    use objc2_foundation::NSRect;

    /// NSTrackingCursorUpdate | NSTrackingActiveAlways | NSTrackingInVisibleRect
    const OPTIONS: usize = 0x04 | 0x80 | 0x200;
    let _ = window.with_webview(|webview| unsafe {
        let view = webview.inner() as *mut AnyObject;
        if view.is_null() {
            return;
        }
        /* A tracking area does not keep its owner, so this one lives for good. */
        let owner: Retained<CrosshairOwner> = msg_send![CrosshairOwner::alloc(), init];
        let owner = Retained::into_raw(owner) as *mut AnyObject;
        let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
        let area: *mut AnyObject = msg_send![
            area,
            initWithRect: NSRect::ZERO,
            options: OPTIONS,
            owner: owner,
            userInfo: std::ptr::null_mut::<AnyObject>()
        ];
        if area.is_null() {
            return;
        }
        let _: () = msg_send![view, addTrackingArea: area];
        let _: () = msg_send![area, release];
    });
}

#[cfg(not(target_os = "macos"))]
pub fn crosshair_over(_window: &tauri::WebviewWindow) {}

#[cfg(not(target_os = "macos"))]
pub fn show_window_with_alpha(window: &tauri::WebviewWindow, _alpha: f64, show: bool) {
    if show {
        let _ = window.show();
    }
}

/**
 * Let the main window follow the user between Spaces: when it is shown it
 * moves to the Space they are on, instead of macOS switching them to the
 * Space it was last on. Set once; must run on the main thread.
 */
#[cfg(target_os = "macos")]
pub fn follow_active_space(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// NSWindowCollectionBehaviorMoveToActiveSpace
    const MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as *mut AnyObject;
    if ns_window.is_null() {
        return;
    }
    unsafe {
        let behavior: usize = msg_send![ns_window, collectionBehavior];
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior | MOVE_TO_ACTIVE_SPACE];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn follow_active_space(_window: &tauri::WebviewWindow) {}

/**
 * Windows: a hidden window stays on the virtual desktop it was last shown
 * on, and showing it switches the user over there. Move it to the desktop
 * of the window in front (the app they captured, or the one they were in)
 * first. Leaves it alone when it is already on the current desktop or that
 * desktop cannot be told. Must run on the main thread, where COM is set up.
 */
#[cfg(target_os = "windows")]
pub fn move_to_current_desktop(window: &tauri::WebviewWindow) {
    use windows::core::GUID;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let Ok(handle) = window.hwnd() else {
        return;
    };
    let hwnd = HWND(handle.0 as _);
    unsafe {
        let Ok(manager) =
            CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL)
        else {
            return;
        };
        if manager
            .IsWindowOnCurrentVirtualDesktop(hwnd)
            .map_or(true, |on| on.as_bool())
        {
            return;
        }
        let front = GetForegroundWindow();
        if front.is_invalid() || front == hwnd {
            return;
        }
        match manager.GetWindowDesktopId(front) {
            Ok(desktop) if desktop != GUID::zeroed() => {
                let _ = manager.MoveWindowToDesktop(hwnd, &desktop);
            }
            _ => {}
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn move_to_current_desktop(_window: &tauri::WebviewWindow) {}

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

    // The macOS squircle icon, the same one the bundled .app shows.
    let bytes: &[u8] = include_bytes!("../icons/icon.icns");
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
        ("macos", "accessibility") => {
            Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        }
        ("macos", "keyboard-shortcuts") => {
            Some("x-apple.systempreferences:com.apple.preference.keyboard?Shortcuts")
        }
        ("windows", "keyboard-shortcuts") => Some("ms-settings:easeofaccess-keyboard"),
        ("windows", "screen-recording") => Some("ms-settings:privacy-graphicscaptureprogrammatic"),
        _ => None,
    }
}

/// Run with --ignored and LOCKSHOT_MOVER set to a program that posts a
/// mouse move to the x and y it is given: while held, a move from another
/// process (as the mouse's own are) must not move the pointer, and
/// Lockshot's own must.
#[cfg(all(test, target_os = "macos"))]
mod lock_check {
    #[test]
    #[ignore]
    fn holds_the_pointer_against_other_moves() {
        use enigo::{Coordinate, Enigo, Mouse, Settings};
        let mover = std::env::var("LOCKSHOT_MOVER").expect("LOCKSHOT_MOVER");
        let at = |s: &str| {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let p = unsafe {
                let e = super::mac::CGEventCreate(std::ptr::null());
                let p = super::mac::CGEventGetLocation(e);
                super::mac::CFRelease(e as *const _);
                p
            };
            eprintln!("{s}: pointer at {:.0}, {:.0}", p.x, p.y);
            (p.x.round() as i32, p.y.round() as i32)
        };
        assert!(super::lock_pointer(400, 400), "not locked");
        let held = at("held");
        std::process::Command::new(&mover)
            .args(["900", "700"])
            .status()
            .unwrap();
        let after_other = at("after another process moved it");
        let mut enigo = Enigo::new(&Settings::default()).unwrap();
        enigo.move_mouse(600, 500, Coordinate::Abs).unwrap();
        let after_own = at("after Lockshot moved it");
        super::lock_pointer(600, 500);
        std::thread::sleep(std::time::Duration::from_millis(1500));
        super::unlock_pointer();
        std::process::Command::new(&mover)
            .args(["700", "450"])
            .status()
            .unwrap();
        let freed = at("after unlock, another process moved it");
        assert_eq!(held, (400, 400));
        assert_eq!(after_other, (400, 400));
        assert_eq!(after_own, (600, 500));
        assert_eq!(freed, (700, 450));
    }
}

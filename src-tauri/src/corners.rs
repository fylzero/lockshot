use tauri::WebviewWindow;

/// Clips the window to macOS's rounded window corners.
///
/// macOS doesn't round a transparent window's content, so the see-through
/// background (and the blur layer behind it, see `blur.rs`) would fill the
/// square corners outside the rounded border. Rounding the window's frame
/// view clips everything inside it.
#[cfg(target_os = "macos")]
pub fn round(window: &WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool, Sel};

    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(ns_window) = target.ns_window() else {
            return;
        };
        if ns_window.is_null() {
            return;
        }
        // SAFETY: ns_window is the live NSWindow behind this Tauri window, and
        // AppKit is only touched here on the main thread.
        unsafe {
            let ns_window = &*(ns_window as *const AnyObject);
            let content: *mut AnyObject = msg_send![ns_window, contentView];
            if content.is_null() {
                return;
            }
            let frame: *mut AnyObject = msg_send![&*content, superview];
            if frame.is_null() {
                return;
            }
            let frame = &*frame;

            // Match the system's radius, which changes between macOS versions.
            let responds: Bool =
                msg_send![frame, respondsToSelector: Sel::register(c"_cornerRadius")];
            let radius: f64 = if responds.as_bool() {
                msg_send![frame, _cornerRadius]
            } else {
                10.0
            };

            let _: () = msg_send![frame, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![frame, layer];
            if layer.is_null() {
                return;
            }
            let _: () = msg_send![&*layer, setCornerRadius: radius];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
            let _: () = msg_send![ns_window, invalidateShadow];
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn round(_window: &WebviewWindow) {}

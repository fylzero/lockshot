use tauri::WebviewWindow;

/// Blurs whatever shows through the window's see-through background.
///
/// - macOS: a real blur radius, from a window-server backdrop layer behind the
///   web view (there is no public API that takes a radius).
/// - Windows: the system Acrylic blur, which has no strength, so any radius
///   above zero turns it on.
/// - Linux: no standard way to blur behind a window, so this does nothing.
#[tauri::command]
pub fn set_window_blur(window: WebviewWindow, radius: u32) -> Result<(), String> {
    apply(&window, radius.min(64))
}

#[cfg(target_os = "macos")]
fn apply(window: &WebviewWindow, radius: u32) -> Result<(), String> {
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let Ok(ns_window) = target.ns_window() else {
                return;
            };
            if ns_window.is_null() {
                return;
            }
            // SAFETY: ns_window is the live NSWindow behind this Tauri window,
            // and AppKit is only touched here on the main thread.
            unsafe { macos::set_blur(&*(ns_window as *const objc2::runtime::AnyObject), radius) };
        })
        .map_err(|err| format!("Could not set the window blur: {err}"))
}

/// The blur is a backdrop layer behind the web view: the same window-server
/// blur that NSVisualEffectView uses, but with a radius we choose and no tint.
/// Being a layer inside the window, it is clipped to the window's rounded
/// corners (see `corners.rs`). The older whole-window blur call
/// (CGSSetWindowBackgroundBlurRadius) ignores those corners and paints the
/// blur into them, so it is only the fallback if the layer is unavailable.
#[cfg(target_os = "macos")]
mod macos {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Size {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        origin: Point,
        size: Size,
    }
    unsafe impl Encode for Point {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for Size {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for Rect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
    }

    const VIEW_ID: &std::ffi::CStr = c"lockshot-blur";
    /// NSViewWidthSizable | NSViewHeightSizable
    const FILL_PARENT: usize = 2 | 16;
    /// NSWindowBelow
    const BELOW: isize = -1;

    unsafe fn ns_string(s: &std::ffi::CStr) -> *mut AnyObject {
        msg_send![AnyClass::get(c"NSString").unwrap(), stringWithUTF8String: s.as_ptr()]
    }

    /// Finds the blur view added earlier, or adds it behind the web view.
    unsafe fn blur_view(content: &AnyObject, layer_class: &AnyClass) -> *mut AnyObject {
        let id = ns_string(VIEW_ID);
        let subviews: *mut AnyObject = msg_send![content, subviews];
        let count: usize = msg_send![&*subviews, count];
        for i in 0..count {
            let view: *mut AnyObject = msg_send![&*subviews, objectAtIndex: i];
            let view_id: *mut AnyObject = msg_send![&*view, identifier];
            if !view_id.is_null() {
                let same: bool = msg_send![&*view_id, isEqualToString: id];
                if same {
                    return view;
                }
            }
        }

        let layer: *mut AnyObject = msg_send![layer_class, layer];
        let _: () = msg_send![&*layer, setWindowServerAware: true];

        let bounds: Rect = msg_send![content, bounds];
        let view: *mut AnyObject = msg_send![AnyClass::get(c"NSView").unwrap(), alloc];
        let view: *mut AnyObject = msg_send![view, initWithFrame: bounds];
        let _: () = msg_send![&*view, setIdentifier: id];
        // A layer-hosting view: set the layer first, then turn layers on.
        let _: () = msg_send![&*view, setLayer: layer];
        let _: () = msg_send![&*view, setWantsLayer: true];
        let _: () = msg_send![&*view, setAutoresizingMask: FILL_PARENT];
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![content, addSubview: view, positioned: BELOW, relativeTo: nil];
        // The superview keeps it alive now.
        let _: () = msg_send![&*view, release];
        view
    }

    pub unsafe fn set_blur(ns_window: &AnyObject, radius: u32) {
        let content: *mut AnyObject = msg_send![ns_window, contentView];
        let (Some(layer_class), Some(filter_class), false) = (
            AnyClass::get(c"CABackdropLayer"),
            AnyClass::get(c"CAFilter"),
            content.is_null(),
        ) else {
            return fallback(ns_window, radius);
        };
        let view = &*blur_view(&*content, layer_class);
        let _: () = msg_send![view, setHidden: radius == 0];
        if radius == 0 {
            return;
        }

        let filter: *mut AnyObject =
            msg_send![filter_class, filterWithType: ns_string(c"gaussianBlur")];
        let amount: *mut AnyObject =
            msg_send![AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: radius as f64];
        let _: () = msg_send![&*filter, setValue: amount, forKey: ns_string(c"inputRadius")];
        let filters: *mut AnyObject =
            msg_send![AnyClass::get(c"NSArray").unwrap(), arrayWithObject: filter];
        let layer: *mut AnyObject = msg_send![view, layer];
        let _: () = msg_send![&*layer, setFilters: filters];
    }

    unsafe fn fallback(ns_window: &AnyObject, radius: u32) {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGSMainConnectionID() -> i32;
            fn CGSSetWindowBackgroundBlurRadius(connection: i32, window: u32, radius: i32) -> i32;
        }
        let number: isize = msg_send![ns_window, windowNumber];
        CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), number as u32, radius as i32);
    }
}

#[cfg(windows)]
fn apply(window: &WebviewWindow, radius: u32) -> Result<(), String> {
    use tauri::window::{Effect, EffectsBuilder};

    let effects = (radius > 0).then(|| EffectsBuilder::new().effect(Effect::Acrylic).build());
    window
        .set_effects(effects)
        .map_err(|err| format!("Could not set the window blur: {err}"))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn apply(_window: &WebviewWindow, _radius: u32) -> Result<(), String> {
    Ok(())
}

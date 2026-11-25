/*!
 * Dragging the editor's file icon out of the window as a real file.
 *
 * The drag is armed the instant the icon is pressed, while the image is
 * rendered and written in the background: it is needed only at the drop.
 * Waiting for the file first (a few hundred milliseconds for a big capture)
 * lost every quick flick. On macOS the drag starts from inside AppKit's own
 * mouse-dragged event, the way native apps do it.
 *
 * Finder never copies a plain file dragged in from another app on current
 * macOS: it takes the drop and writes nothing. So the drag carries a file
 * promise, which Finder fulfils by asking Lockshot to write the file into
 * the folder it was dropped on, alongside the plain file for browsers, chats
 * and other apps that read it in place.
 */

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::WebviewWindow;

use crate::export;

/// What `arm` reports back to the editor.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DragStatus {
    /// The drag began (macOS starts it on the first move past the threshold).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Started,
    /// The button came up before the mouse moved: it was only a click.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Released,
    /// Call `start` once the mouse moves (Windows, Linux).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Ready,
}

enum FileState {
    Writing,
    Ready { preview: Vec<u8> },
    Failed(String),
}

/// The file one press of the icon drags, written while the drag gets going.
pub struct DragFile {
    id: String,
    path: PathBuf,
    state: Mutex<FileState>,
    changed: Condvar,
}

impl DragFile {
    /// Wait for the file to be written; returns its drag preview.
    fn wait(&self, timeout: Duration) -> Result<Vec<u8>, String> {
        let state = self.state.lock().unwrap();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |s| matches!(s, FileState::Writing))
            .unwrap();
        match &*state {
            FileState::Ready { preview } => Ok(preview.clone()),
            FileState::Failed(err) => Err(err.clone()),
            FileState::Writing => Err("The image took too long to write.".into()),
        }
    }

    /// The drag preview, if the file is written already.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn preview_now(&self) -> Option<Vec<u8>> {
        match &*self.state.lock().unwrap() {
            FileState::Ready { preview } => Some(preview.clone()),
            _ => None,
        }
    }

    fn finish(&self, result: Result<Vec<u8>, String>) {
        *self.state.lock().unwrap() = match result {
            Ok(preview) => FileState::Ready { preview },
            Err(err) => FileState::Failed(err),
        };
        self.changed.notify_all();
    }
}

/// The last few presses' files, newest last; a late write for an older one
/// must not take the place of the newest.
static RECENT: Mutex<Vec<Arc<DragFile>>> = Mutex::new(Vec::new());
/// The file of the press being dragged (Windows, Linux).
static ARMED: Mutex<Option<Arc<DragFile>>> = Mutex::new(None);
const KEEP: usize = 4;

/**
 * The file for press `id`, named `stem` in `format`. A press's two calls,
 * arming and writing, may arrive in either order; whichever comes first
 * makes it.
 */
pub fn file_for(id: &str, stem: &str, format: &str) -> Arc<DragFile> {
    let mut recent = RECENT.lock().unwrap();
    if let Some(file) = recent.iter().find(|f| f.id == id) {
        return file.clone();
    }
    let keep: Vec<_> = recent.iter().map(|f| f.path.clone()).collect();
    let path = export::drag_path(stem, format, &keep);
    /* macOS leaves a file out of a drag if nothing is at its path when the
    drag begins, so hold the spot until the image is written over it. */
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::File::create(&path);
    let file = Arc::new(DragFile {
        id: id.to_string(),
        path,
        state: Mutex::new(FileState::Writing),
        changed: Condvar::new(),
    });
    recent.push(file.clone());
    if recent.len() > KEEP {
        recent.remove(0);
    }
    file
}

/// Write the image for press `id` and its drag preview. Blocks.
pub fn write(file: &DragFile, png: &[u8], format: &str, quality: u8) {
    file.finish(
        export::write(png, format, quality, &file.path).and_then(|()| export::drag_preview(png)),
    );
}

/**
 * Get the drag of `file` going as soon as the icon is pressed. On macOS, wait
 * for the mouse to move with the button held and start the drag right then.
 */
pub async fn arm(window: WebviewWindow, file: Arc<DragFile>) -> Result<DragStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let target = window.clone();
        window
            .run_on_main_thread(move || mac::arm(&target, file, tx))
            .map_err(|err| err.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv())
            .await
            .map_err(|err| err.to_string())?
            .map_err(|_| "The drag did not start.".to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        *ARMED.lock().unwrap() = Some(file);
        Ok(DragStatus::Ready)
    }
}

/// Start the drag of the current press's file once written (Windows, Linux).
pub async fn start(window: WebviewWindow) -> Result<(), String> {
    let file = ARMED
        .lock()
        .unwrap()
        .take()
        .ok_or("There is no file to drag.")?;
    let preview = tauri::async_runtime::spawn_blocking({
        let file = file.clone();
        move || file.wait(Duration::from_secs(30))
    })
    .await
    .map_err(|err| err.to_string())??;
    let path = file.path.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let item = drag::DragItem::Files(vec![path]);
            let icon = drag::Image::Raw(preview);
            let start = || -> Result<(), String> {
                #[cfg(target_os = "linux")]
                let handle = target.gtk_window().map_err(|err| err.to_string())?;
                #[cfg(target_os = "linux")]
                let handle = &handle;
                #[cfg(not(target_os = "linux"))]
                let handle = &target;
                drag::start_drag(handle, item, icon, |_, _| {}, drag::Options::default())
                    .map_err(|err| err.to_string())
            };
            let _ = tx.send(start());
        })
        .map_err(|err| err.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|_| "The drag did not start.".to_string())?
        .map_err(|err| format!("Could not start the drag: {err}"))
}

/// Where the file icon is in the window, in CSS pixels: x, y, width, height.
pub type HandleRect = [f64; 4];

/**
 * Tell AppKit where the file icon is, so a press on it reaches the page while
 * the window is in the background (macOS). Windows and Linux already pass a
 * click on a background window through.
 */
pub fn set_handle(window: WebviewWindow, rect: Option<HandleRect>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let ns_window = window.ns_window().map_err(|err| err.to_string())? as usize;
        window
            .with_webview(move |webview| first_mouse::set(ns_window, webview.inner(), rect))
            .map_err(|err| err.to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, rect);
        Ok(())
    }
}

/**
 * A click on a background window only activates it on macOS; the page never
 * sees the press, so grabbing the icon of a Lockshot window behind another
 * app did nothing. The webview says yes to that first click over the icon
 * alone (everywhere else it would draw, crop or press a button by surprise),
 * and asks AppKit to hold the window where it is: a drag leaves it behind
 * the app it is dropped into, and a plain click brings it forward as usual.
 */
#[cfg(target_os = "macos")]
mod first_mouse {
    use super::HandleRect;
    use std::ffi::c_void;
    use std::sync::{Mutex, Once, OnceLock};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::{sel, ClassType};
    use objc2_app_kit::{NSEvent, NSView};

    /// The icon's window (its pointer) and where the icon is in it.
    static HANDLE: Mutex<Option<(usize, HandleRect)>> = Mutex::new(None);
    static ACCEPT: OnceLock<Imp> = OnceLock::new();
    static DELAY: OnceLock<Imp> = OnceLock::new();
    static INSTALL: Once = Once::new();

    type Handler = unsafe extern "C-unwind" fn(&NSView, Sel, *mut NSEvent) -> Bool;

    /// `window` is the NSWindow and `webview` its WKWebView, both as pointers.
    pub fn set(ptr: usize, webview: *mut c_void, rect: Option<HandleRect>) {
        /* wry names its webview class itself, so take the class from the webview. */
        INSTALL.call_once(|| match unsafe { (webview as *const AnyObject).as_ref() } {
            Some(view) => install(view.class()),
            None => eprintln!("Lockshot: no webview to let the file icon take a first click."),
        });
        let mut handle = HANDLE.lock().unwrap();
        match rect {
            Some(rect) => *handle = Some((ptr, rect)),
            /* Only the window that set it may clear it. */
            None if handle.is_some_and(|(p, _)| p == ptr) => *handle = None,
            None => {}
        }
    }

    /// Whether `event` is a press on the icon in `view`'s window.
    fn over_handle(view: &NSView, event: *mut NSEvent) -> bool {
        let Some(event) = (unsafe { event.as_ref() }) else {
            return false;
        };
        let Some((ptr, [x, y, w, h])) = *HANDLE.lock().unwrap() else {
            return false;
        };
        let Some(window) = view.window() else {
            return false;
        };
        if Retained::as_ptr(&window) as usize != ptr {
            return false;
        }
        let at = view.convertPoint_fromView(event.locationInWindow(), None);
        let top = if view.isFlipped() {
            at.y
        } else {
            view.bounds().size.height - at.y
        };
        at.x >= x && at.x <= x + w && top >= y && top <= y + h
    }

    unsafe extern "C-unwind" fn accepts_first_mouse(
        view: &NSView,
        sel: Sel,
        event: *mut NSEvent,
    ) -> Bool {
        if over_handle(view, event) {
            return Bool::YES;
        }
        match ACCEPT.get() {
            Some(original) => unsafe {
                std::mem::transmute::<Imp, Handler>(*original)(view, sel, event)
            },
            None => Bool::NO,
        }
    }

    unsafe extern "C-unwind" fn should_delay_window_ordering(
        view: &NSView,
        sel: Sel,
        event: *mut NSEvent,
    ) -> Bool {
        if over_handle(view, event) {
            return Bool::YES;
        }
        match DELAY.get() {
            Some(inherited) => unsafe {
                std::mem::transmute::<Imp, Handler>(*inherited)(view, sel, event)
            },
            None => Bool::NO,
        }
    }

    /// Put both answers on wry's webview class.
    fn install(class: &AnyClass) {
        let class = class as *const AnyClass as *mut AnyClass;
        let accept = sel!(acceptsFirstMouse:);
        let delay = sel!(shouldDelayWindowOrderingForEvent:);
        unsafe {
            let types = objc2::ffi::method_getTypeEncoding(objc2::ffi::class_getInstanceMethod(
                NSView::class(),
                accept,
            ));
            let imp = std::mem::transmute::<Handler, Imp>(accepts_first_mouse);
            if let Some(original) = objc2::ffi::class_replaceMethod(class, accept, imp, types) {
                let _ = ACCEPT.set(original);
            }
            /* WryWebView inherits this one: keep the inherited answer for every other press. */
            let method = objc2::ffi::class_getInstanceMethod(class, delay);
            if let Some(inherited) = method.as_ref() {
                let _ = DELAY.set(inherited.implementation());
            }
            let imp = std::mem::transmute::<Handler, Imp>(should_delay_window_ordering);
            objc2::ffi::class_replaceMethod(
                class,
                delay,
                imp,
                objc2::ffi::method_getTypeEncoding(objc2::ffi::class_getInstanceMethod(
                    NSView::class(),
                    delay,
                )),
            );
        }
    }

    /// The window is held back for this press: keep it there for the drag.
    pub fn keep_behind() {
        unsafe {
            let app: Retained<objc2::runtime::AnyObject> =
                objc2::msg_send![objc2::class!(NSApplication), sharedApplication];
            let _: () = objc2::msg_send![&app, preventWindowOrdering];
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{DragFile, DragStatus};
    use std::cell::{Cell, RefCell};
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use std::time::Duration;

    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{
        define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message,
    };
    use objc2_app_kit::{
        NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
        NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSFilePromiseProvider,
        NSFilePromiseProviderDelegate, NSImage, NSPasteboard, NSPasteboardWriting, NSWindow,
    };
    use objc2_foundation::{NSArray, NSData, NSError, NSPoint, NSRect, NSString, NSURL};
    use tauri::WebviewWindow;

    /// How far, in points, the mouse moves before a press becomes a drag.
    const THRESHOLD: f64 = 3.0;

    /// Where a drag began: the window and the point in it.
    struct SourceIvars {
        window: Retained<NSWindow>,
        at: NSPoint,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "LockshotDragSource"]
        #[ivars = SourceIvars]
        struct DragSource;

        unsafe impl NSObjectProtocol for DragSource {}

        unsafe impl NSDraggingSource for DragSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn operation_mask(
                &self,
                _session: &NSDraggingSession,
                context: NSDraggingContext,
            ) -> NSDragOperation {
                /* Other apps and Finder get a copy; the file is ours to give. */
                if context == NSDraggingContext::OutsideApplication {
                    NSDragOperation::Copy
                } else {
                    NSDragOperation::None
                }
            }

            #[unsafe(method(draggingSession:endedAtPoint:operation:))]
            fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, _op: NSDragOperation) {
                self.release_page();
            }
        }
    );

    /// The file a drag carries, for the promise and its delegate.
    struct FileIvars {
        file: Arc<DragFile>,
    }

    impl FileIvars {
        fn name(&self) -> String {
            self.file
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    }

    define_class!(
        /// Writes the promised file where Finder asks, from any thread.
        #[unsafe(super(NSObject))]
        #[name = "LockshotPromiseDelegate"]
        #[ivars = FileIvars]
        struct PromiseDelegate;

        unsafe impl NSObjectProtocol for PromiseDelegate {}

        unsafe impl NSFilePromiseProviderDelegate for PromiseDelegate {
            #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
            fn file_name(
                &self,
                _provider: &NSFilePromiseProvider,
                _type: &NSString,
            ) -> Retained<NSString> {
                NSString::from_str(&self.ivars().name())
            }

            #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
            fn write(
                &self,
                _provider: &NSFilePromiseProvider,
                url: &NSURL,
                done: &DynBlock<dyn Fn(*mut NSError)>,
            ) {
                /* Finder calls this off the main thread, so it can wait for the file. */
                let file = &self.ivars().file;
                let result = file
                    .wait(Duration::from_secs(30))
                    .and_then(|_| url.path().ok_or_else(|| "no path".to_string()))
                    .and_then(|to| {
                        std::fs::copy(&file.path, to.to_string()).map_err(|e| e.to_string())
                    });
                match result {
                    Ok(_) => done.call((std::ptr::null_mut(),)),
                    Err(_) => {
                        let error = NSError::new(1, &NSString::from_str("Lockshot"));
                        done.call((Retained::as_ptr(&error) as *mut NSError,));
                    }
                }
            }
        }
    );

    define_class!(
        /// A file promise that also offers the plain file, for apps that read it in place.
        #[unsafe(super(NSFilePromiseProvider, NSObject))]
        #[name = "LockshotFilePromise"]
        #[ivars = FileIvars]
        struct FilePromise;

        unsafe impl NSObjectProtocol for FilePromise {}

        unsafe impl NSPasteboardWriting for FilePromise {
            #[unsafe(method_id(writableTypesForPasteboard:))]
            fn writable_types(&self, pasteboard: &NSPasteboard) -> Retained<NSArray<NSString>> {
                let types: Retained<NSArray<NSString>> =
                    unsafe { msg_send![super(self), writableTypesForPasteboard: pasteboard] };
                let mut all = types.to_vec();
                all.push(NSString::from_str(FILE_URL));
                NSArray::from_retained_slice(&all)
            }

            #[unsafe(method_id(pasteboardPropertyListForType:))]
            fn property_list(&self, kind: &NSString) -> Option<Retained<AnyObject>> {
                if kind.to_string() == FILE_URL {
                    /* AppKit asks for this as the drag begins and never again, so
                    hand over the file without waiting for the image: it is written
                    over the placeholder well before a drop, a few hundred
                    milliseconds at most for a big capture. */
                    file_url(&self.ivars().file.path)
                        .absoluteString()
                        .map(|url| Retained::into_super(Retained::into_super(url)))
                } else {
                    unsafe { msg_send![super(self), pasteboardPropertyListForType: kind] }
                }
            }
        }
    );

    const FILE_URL: &str = "public.file-url";

    fn file_url(path: &std::path::Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), false)
    }

    /// The type identifier Finder files the promised image under.
    fn uti(path: &std::path::Path) -> &'static str {
        match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "jpg" | "jpeg" => "public.jpeg",
            "heic" => "public.heic",
            "tiff" | "tif" => "public.tiff",
            "gif" => "com.compuserve.gif",
            "bmp" => "com.microsoft.bmp",
            "pdf" => "com.adobe.pdf",
            _ => "public.png",
        }
    }

    fn file_icon(path: &std::path::Path) -> Retained<NSImage> {
        let ext = NSString::from_str(path.extension().and_then(|e| e.to_str()).unwrap_or("png"));
        let image: Retained<NSImage> = unsafe {
            let workspace: Retained<AnyObject> =
                msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            msg_send![&workspace, iconForFileType: &*ext]
        };
        image.setSize(objc2_foundation::NSSize::new(64.0, 64.0));
        image
    }

    impl DragSource {
        fn new(window: &NSWindow, at: NSPoint, mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(SourceIvars {
                window: window.retain(),
                at,
            });
            unsafe { msg_send![super(this), init] }
        }

        /**
         * The drag swallows the mouse-up, so the page never hears the button
         * come up and treats the next press as part of the old one: every other
         * grab of the icon did nothing. Hand it the mouse-up it missed.
         */
        fn release_page(&self) {
            let SourceIvars { window, at } = self.ivars();
            unsafe {
                let app: Retained<AnyObject> =
                    msg_send![objc2::class!(NSApplication), sharedApplication];
                let current: Option<Retained<NSEvent>> = msg_send![&app, currentEvent];
                let up: Option<Retained<NSEvent>> = msg_send![
                    objc2::class!(NSEvent),
                    mouseEventWithType: NSEventType::LeftMouseUp,
                    location: *at,
                    modifierFlags: NSEventModifierFlags::empty(),
                    timestamp: current.map(|e| e.timestamp()).unwrap_or(0.0),
                    windowNumber: window.windowNumber(),
                    context: std::ptr::null::<AnyObject>(),
                    eventNumber: 0isize,
                    clickCount: 1isize,
                    pressure: 0.0f32
                ];
                if let Some(up) = up {
                    window.sendEvent(&up);
                }
            }
        }
    }

    thread_local! {
        /// The last drag's source and promise delegate, kept alive until the next
        /// drag: Finder may write the promised file after the drag ends.
        static SOURCE: RefCell<Option<(Retained<DragSource>, Retained<PromiseDelegate>)>> =
            const { RefCell::new(None) };
        /// The event monitor waiting for the mouse to move, if one is armed.
        static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    }

    fn disarm() {
        if let Some(monitor) = MONITOR.with(|m| m.borrow_mut().take()) {
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }

    pub fn arm(
        window: &WebviewWindow,
        file: Arc<DragFile>,
        tx: Sender<Result<DragStatus, String>>,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = tx.send(Err("The drag must start on the main thread.".into()));
            return;
        };
        disarm();
        /* Too late: the button is already up, so this was a click. */
        if NSEvent::pressedMouseButtons() & 1 == 0 {
            let _ = tx.send(Ok(DragStatus::Released));
            return;
        }
        let ns_window = match window.ns_window() {
            Ok(ptr) => unsafe { Retained::retain(ptr as *mut NSWindow) },
            Err(err) => {
                let _ = tx.send(Err(err.to_string()));
                return;
            }
        };
        let Some(ns_window) = ns_window else {
            let _ = tx.send(Err("Could not find the editor window.".into()));
            return;
        };

        let anchor = NSEvent::mouseLocation();
        let reply = Rc::new(Cell::new(Some(tx)));
        let send = move |status: Result<DragStatus, String>| {
            if let Some(tx) = reply.take() {
                let _ = tx.send(status);
            }
        };

        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            match event_ref.r#type() {
                NSEventType::LeftMouseUp => {
                    send(Ok(DragStatus::Released));
                    /* Last: removing the monitor can free this handler and what it holds. */
                    disarm();
                    event.as_ptr()
                }
                NSEventType::LeftMouseDragged => {
                    let now = NSEvent::mouseLocation();
                    let far = (now.x - anchor.x).hypot(now.y - anchor.y) >= THRESHOLD;
                    if !far {
                        return event.as_ptr();
                    }
                    send(begin(&ns_window, event_ref, &file, mtm).map(|()| DragStatus::Started));
                    disarm();
                    /* AppKit tracks the mouse from here; the page sees no more of this press. */
                    std::ptr::null_mut()
                }
                _ => event.as_ptr(),
            }
        });
        let mask = NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp;
        match unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) } {
            Some(monitor) => MONITOR.with(|m| *m.borrow_mut() = Some(monitor)),
            /* Dropping the handler drops the sender, which reports the failure. */
            None => drop(handler),
        }
    }

    fn begin(
        window: &NSWindow,
        event: &NSEvent,
        file: &Arc<DragFile>,
        mtm: MainThreadMarker,
    ) -> Result<(), String> {
        let path = &file.path;
        let view = window
            .contentView()
            .ok_or("The editor window has no view.")?;

        let delegate = PromiseDelegate::alloc().set_ivars(FileIvars { file: file.clone() });
        let delegate: Retained<PromiseDelegate> = unsafe { msg_send![super(delegate), init] };
        let promise = FilePromise::alloc().set_ivars(FileIvars { file: file.clone() });
        let promise: Retained<FilePromise> = unsafe {
            msg_send![
                super(promise),
                initWithFileType: &*NSString::from_str(uti(path)),
                delegate: ProtocolObject::<dyn NSFilePromiseProviderDelegate>::from_ref(&*delegate)
            ]
        };
        let item = NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            ProtocolObject::<dyn NSPasteboardWriting>::from_ref(&*promise),
        );
        /* The thumbnail if the file is written already, else the format's file icon. */
        let image = file
            .preview_now()
            .and_then(|bytes| NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&bytes)))
            .unwrap_or_else(|| file_icon(path));
        let size = image.size();
        let at = view.convertPoint_fromView(event.locationInWindow(), None);
        let frame = NSRect::new(
            NSPoint::new(at.x - size.width / 2.0, at.y - size.height / 2.0),
            size,
        );
        unsafe { item.setDraggingFrame_contents(frame, Some(&image)) };

        let source = DragSource::new(window, event.locationInWindow(), mtm);
        let items = NSArray::from_retained_slice(&[item]);
        let session = view.beginDraggingSessionWithItems_event_source(
            &items,
            event,
            ProtocolObject::from_ref(&*source),
        );
        /* Pressed while in the background: stay there, behind the app it is dropped into. */
        if !window.isKeyWindow() {
            super::first_mouse::keep_behind();
        }
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
        SOURCE.with(|s| *s.borrow_mut() = Some((source, delegate)));
        Ok(())
    }
}

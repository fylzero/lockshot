//! Scrolling Capture: select part of the screen, and Lockshot scrolls it a
//! page at a time, saving a screenshot of that area after each scroll, until
//! the content stops moving. Meant for long chat threads (Slack, Discord, Teams) and
//! other pages whose lists are virtualized, so the full history is never in
//! the page source at once.
//!
//! The area is either dragged out, or a window picked by clicking the part
//! of it that scrolls. The scroll is a real scroll-wheel event sent with
//! the pointer at that spot (the middle of a dragged area), so it reaches
//! the pane under it whether or not the window has keyboard focus. In a
//! dragged area, if nothing under the middle scrolls (the area takes in a
//! still window or a header too), other spots are tried until one does; a
//! clicked spot is kept to, so the scroll never wanders to another pane.
//!
//! The pointer only visits the area to scroll. Straight after each scroll
//! it moves onto the border Lockshot draws around the area and waits there
//! while the page is captured, so nothing in the pages shows the hover look
//! (a highlighted row, a message's action bar, a tooltip). Scrolling by
//! dragging the scroll bar instead would not need the visit, but scroll
//! bars are too often hidden until hovered (macOS, Slack, Discord), and a
//! dragged thumb moves by amounts that change as a virtualized list loads.
//!
//! The window under the scroll spot is brought to the front first: the app
//! picked, rather than whatever covered part of it, or Lockshot's own
//! window, which comes forward when the selection overlay closes. If
//! another app covers the spot later, the window is brought back. Pages are
//! read off that window alone when the system allows (macOS, Windows), so
//! a window or notice that pops up in front of it never lands in them;
//! otherwise off the screen.
//!
//! Every run goes to its own folder of numbered files, and the pages are
//! joined into one long image as they come in (see stitch.rs); on macOS the
//! text can be read off them too (see ocr.rs). Esc, the Stop button, or
//! moving the mouse ends it early.

use std::io::Cursor;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::Local;
use enigo::{Axis, Enigo, Mouse};
use image::RgbaImage;
use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::settings::{self, Settings};
use crate::stitch::{Pane, Stitcher};
use crate::{export, frame, hotkeys, ocr, platform, quick_save, window_list};

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static SKIP_TEXT: AtomicBool = AtomicBool::new(false);

/// The action the temporary Esc hotkey runs while a capture is going.
pub const STOP_ACTION: &str = "scroll-stop";
const STOP_KEY: &str = "Escape";

/// How far the pointer may drift from where Lockshot put it before the run
/// counts the mouse as moved by the user, in the units xcap uses.
const POINTER_SLACK: i32 = 24;
/// After a scroll that changed nothing, wait this much longer and try once
/// more: virtualized lists often load the next batch only after a pause.
const STALL_WAIT: Duration = Duration::from_millis(1500);
/// Aim to scroll this share of the scrolling pane, so pages overlap a little.
const PAGE_SHARE: f64 = 0.7;
/// Give a window brought to the front this long to get there.
const RAISE_WAIT: Duration = Duration::from_millis(300);
/// Leave the pointer on the scroll spot this long after a scroll before
/// moving it away: Windows hands a wheel event to whatever window is under
/// the pointer when it gets round to it, not when it was sent.
const SCROLL_LANDS: Duration = Duration::from_millis(60);
/// Give a hover look (a row highlight, an action bar) at least this long to
/// fade once the pointer has moved off the area, before capturing.
const HOVER_FADES: Duration = Duration::from_millis(200);
/// Keep the resting spot this far from the display's edges, where macOS
/// shows a hidden menu bar or Dock and runs hot corners.
const EDGE_MARGIN: i32 = 8;

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Esc, the Stop button, or a second press of the hotkey.
pub fn stop() {
    debug_log("stop requested");
    STOP.store(true, Ordering::SeqCst);
    /* Give the mouse back at once, not after the files are written. */
    platform::unlock_pointer();
}

/// The Skip button on the Saving notice: stop reading the text and keep
/// what was read so far.
pub fn skip_text() {
    debug_log("skip reading the text");
    SKIP_TEXT.store(true, Ordering::SeqCst);
}

/// The area to scroll and capture, in the units xcap reports displays in
/// (points on macOS, physical pixels elsewhere).
pub struct Target {
    /// The display's top-left corner on the desktop, to find it again.
    pub display: (i32, i32),
    /// The area, relative to the display.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Where the user clicked to pick a window, relative to the display:
    /// scroll there and only there. None for a dragged area.
    pub spot: Option<(u32, u32)>,
    /// The window under the scroll spot, if any.
    pub window: Option<Window>,
}

/// The window being scrolled: its id, and its bounds relative to the
/// display (it may reach past the display's edges).
pub struct Window {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where in the area to try scrolling, as fractions of its width and
/// height: the middle first, then the middle of each side's half, then the
/// quarters, until one of them moves the content.
const SPOTS: [(f64, f64); 9] = [
    (0.5, 0.5),
    (0.5, 0.75),
    (0.5, 0.25),
    (0.25, 0.5),
    (0.75, 0.5),
    (0.25, 0.75),
    (0.75, 0.75),
    (0.25, 0.25),
    (0.75, 0.25),
];

impl Target {
    /// The spots to try scrolling at, on the desktop, best first.
    fn spots(&self) -> Vec<(i32, i32)> {
        if let Some((x, y)) = self.spot {
            return vec![(self.display.0 + x as i32, self.display.1 + y as i32)];
        }
        SPOTS
            .iter()
            .map(|&(fx, fy)| {
                (
                    self.display.0 + self.x as i32 + (self.width as f64 * fx).round() as i32,
                    self.display.1 + self.y as i32 + (self.height as f64 * fy).round() as i32,
                )
            })
            .collect()
    }

    /**
     * Where the pointer waits while a page is captured, on the desktop: in
     * the middle of one side of the border frame.rs draws just outside the
     * area. That border is Lockshot's own window, so no app under it sees
     * the pointer and shows a hover look or a tooltip that could reach into
     * the area. None when every side is against the display's edge (the
     * whole screen), where the pointer has to stay on the scroll spot.
     */
    fn rest_spot(&self, display_width: u32, display_height: u32) -> Option<(i32, i32)> {
        let (x, y) = (self.x as i32, self.y as i32);
        let (w, h) = (self.width as i32, self.height as i32);
        let (dw, dh) = (display_width as i32, display_height as i32);
        let half = frame::THICKNESS / 2;
        let fits = |px: i32, py: i32| {
            (EDGE_MARGIN..dw - EDGE_MARGIN).contains(&px)
                && (EDGE_MARGIN..dh - EDGE_MARGIN).contains(&py)
        };
        /* Right first: the side a list's scroll bar is on, so the shortest trip. */
        [
            (x + w + half, y + h / 2),
            (x - 1 - half, y + h / 2),
            (x + w / 2, y + h + half),
            (x + w / 2, y - 1 - half),
        ]
        .into_iter()
        .find(|&(px, py)| fits(px, py))
        .map(|(px, py)| (self.display.0 + px, self.display.1 + py))
    }
}

/// Start the run on its own thread. Returns at once.
pub fn start(app: &AppHandle, target: Target) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("A scrolling capture is already running.".into());
    }
    STOP.store(false, Ordering::SeqCst);
    SKIP_TEXT.store(false, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        /* Give the mouse back however the run ends, even on a panic. */
        let _unlock = UnlockOnDrop;
        let esc = hotkeys::register_extra(&app, STOP_ACTION, STOP_KEY)
            .inspect_err(|err| debug_log(&format!("Esc hotkey not registered: {err}")))
            .is_ok();
        /*
         * The Esc hotkey does not always get through (it did not on macOS
         * with the pointer locked), so also watch the key itself.
         */
        std::thread::spawn(|| {
            while RUNNING.load(Ordering::SeqCst) {
                if platform::escape_down() {
                    debug_log("Esc is down: stopping");
                    stop();
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        });
        let outcome = run(&app, &target);
        if esc {
            hotkeys::unregister_extra(&app, STOP_KEY);
        }
        frame::hide(&app);
        quick_save::set_avoid(&app, None);
        RUNNING.store(false, Ordering::SeqCst);
        match outcome {
            Ok(done) => quick_save::show_scroll_done(
                &app,
                done.dir.display().to_string(),
                done.title,
                done.pages,
                done.thumbnail,
                done.note,
            ),
            Err(err) => quick_save::show_error(&app, err),
        }
    });
    Ok(())
}

struct UnlockOnDrop;

impl Drop for UnlockOnDrop {
    fn drop(&mut self) {
        platform::unlock_pointer();
    }
}

struct Done {
    dir: PathBuf,
    /// What was saved, such as "Saved 12 pages and a long image".
    title: String,
    pages: u32,
    thumbnail: Option<RgbaImage>,
    note: String,
}

/// Moves the pointer, holds it where it was put, and sends scroll-wheel steps.
struct Pointer {
    enigo: Enigo,
    /// Whether the pointer is held still between moves (macOS, Windows).
    locked: bool,
    /// Where Lockshot last put the pointer, to tell when the user moves it.
    at: Option<(i32, i32)>,
}

impl Pointer {
    fn new() -> Result<Self, String> {
        let settings = enigo::Settings {
            open_prompt_to_get_permissions: false,
            ..Default::default()
        };
        let enigo = Enigo::new(&settings)
            .map_err(|err| format!("Lockshot could not control the mouse to scroll: {err}"))?;
        Ok(Self {
            enigo,
            locked: false,
            at: None,
        })
    }

    /// Put the pointer at `point` and hold it there; on Linux, which does
    /// not hold it, just move it. Returns whether it is held.
    fn hold(&mut self, point: (i32, i32)) -> bool {
        self.move_to(point);
        self.locked = platform::lock_pointer(point.0, point.1);
        self.at = Some(point);
        self.locked
    }

    /**
     * Move the held pointer to `point`, as a real move, so the app it
     * leaves drops its hover look and the one it lands on routes the next
     * scroll there, then hold it there. Windows will not move a pointer
     * out of its hold, so the hold is let go for the move there; holding
     * again also takes back a hold Windows let go of by itself
     * (Ctrl+Alt+Del, an app switch). macOS keeps holding: its own moves
     * get through.
     */
    fn go(&mut self, point: (i32, i32)) {
        if self.locked && cfg!(target_os = "windows") {
            platform::unlock_pointer();
        }
        self.move_to(point);
        if self.locked && !STOP.load(Ordering::SeqCst) {
            platform::lock_pointer(point.0, point.1);
        }
        self.at = Some(point);
    }

    /// Whether the pointer is now away from where Lockshot put it: the
    /// user took the mouse back. Never while it is held: then the mouse is
    /// meant to do nothing, and the next move puts the pointer back anyway.
    fn moved_by_user(&self, app: &AppHandle) -> bool {
        if self.locked {
            return false;
        }
        let (Some((px, py)), Some((x, y))) = (self.at, platform::cursor_point(app)) else {
            return false;
        };
        (x - px).abs() > POINTER_SLACK || (y - py).abs() > POINTER_SLACK
    }

    fn move_to(&mut self, (x, y): (i32, i32)) {
        #[cfg(target_os = "windows")]
        platform::set_cursor(x, y);
        #[cfg(not(target_os = "windows"))]
        let _ = self.enigo.move_mouse(x, y, enigo::Coordinate::Abs);
    }

    /// Scroll by `amount` units: positive moves the content up (towards the
    /// end), negative down. Points on macOS, wheel notches elsewhere.
    fn scroll(&mut self, amount: i32) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        let result = self.enigo.smooth_scroll(amount, Axis::Vertical);
        #[cfg(not(target_os = "macos"))]
        let result = self.enigo.scroll(amount, Axis::Vertical);
        result.map_err(|err| format!("Could not scroll: {err}"))
    }
}

/// Scroll step sizes, in the units Pointer::scroll takes.
struct Step {
    amount: f64,
    min: f64,
    max: f64,
    /// How many image pixels one unit moved the content last time, to
    /// tell the true shift from look-alikes on content that repeats.
    px_per_unit: Option<f64>,
}

impl Step {
    /// `scale`: image pixels per point of the area (2 on Retina).
    fn new(area_height: f64, scale: f64) -> Self {
        if cfg!(target_os = "macos") {
            /* Pixel scrolling: start at a third of the area, safely under
            a page. A point of scroll is usually a point of content. */
            Step {
                amount: (area_height * 0.3).max(40.0),
                min: 20.0,
                max: 8000.0,
                px_per_unit: Some(scale),
            }
        } else {
            /* Wheel notches, about three lines each. */
            Step {
                amount: 3.0,
                min: 1.0,
                max: 60.0,
                px_per_unit: None,
            }
        }
    }

    /// Roughly how far the next step should move the content, in image pixels.
    fn expected(&self) -> Option<usize> {
        self.px_per_unit
            .map(|r| (r * self.units() as f64).round() as usize)
    }

    fn units(&self) -> i32 {
        self.amount.round().max(1.0) as i32
    }

    /// The last step moved the content `moved` image pixels in a pane about
    /// `pane` pixels tall: aim the next one at most of the pane.
    fn adjust(&mut self, moved: usize, pane: usize) {
        if moved == 0 {
            return;
        }
        self.px_per_unit = Some(moved as f64 / self.units() as f64);
        let wanted = pane as f64 * PAGE_SHARE;
        /* Change gently, in case a measurement was off. */
        let factor = (wanted / moved as f64).clamp(0.25, 3.0);
        self.amount = (self.amount * factor).clamp(self.min, self.max);
    }

    /// The content changed but no overlap was found, so the step may have
    /// skipped past part of it. Go smaller.
    fn shrink(&mut self) {
        self.amount = (self.amount / 2.0).max(self.min);
    }
}

fn run(app: &AppHandle, target: &Target) -> Result<Done, String> {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let monitor = find_display(target.display)?;
    let spots = target.spots();
    let mut spot = 0;
    let mut point = spots[0];
    let display_width = monitor.width().map_err(|err| err.to_string())?;
    let display_height = monitor.height().map_err(|err| err.to_string())?;
    frame::show(
        app,
        frame::Area {
            x: target.display.0 + target.x as i32,
            y: target.display.1 + target.y as i32,
            width: target.width as i32,
            height: target.height as i32,
        },
        frame::Area {
            x: target.display.0,
            y: target.display.1,
            width: display_width as i32,
            height: display_height as i32,
        },
    );
    let rest = target.rest_spot(display_width, display_height);
    debug_log(&format!("scroll at {point:?}, rest at {rest:?}"));
    let mut pointer = Pointer::new()?;
    let up = settings.scroll_direction == "up";
    let direction = if up { -1 } else { 1 };
    let delay = Duration::from_millis(settings.scroll_delay_ms as u64);
    /* With the pointer moved off, also give the hover look time to go. */
    let settle = if rest.is_some() {
        delay.max(SCROLL_LANDS + HOVER_FADES)
    } else {
        delay
    };
    let max_pages = settings.scroll_max_pages.max(1);

    /* Hold the pointer off the area until the run ends, visiting the
    scroll spot only to scroll; on Linux moving the mouse stops the run
    instead. */
    let locked = pointer.hold(rest.unwrap_or(point));
    if let Some(window) = &target.window {
        window_list::bring_to_front(window.id);
    }

    /* The area is read off the screen, so let the overlay go and the
    window come forward first. */
    std::thread::sleep(Duration::from_millis(250).max(settle).max(RAISE_WAIT));
    let screen = capture_screen(app, &monitor, target)?;
    let source = Source::pick(target, &screen);
    /* Every page from the same source, so they compare exactly. */
    let first = match source {
        Source::Screen => screen,
        Source::Window { .. } => capture(app, &monitor, target, &source)?,
    };

    let dir = output_dir(app, &settings)?;
    let keep_pages = settings.scroll_output != "image";
    let mut writer = Writer::new(dir.clone(), &settings, max_pages, keep_pages);
    /* The text is read off the joined rows, so the stitcher runs whenever
    either is wanted. */
    let keep_image = settings.scroll_output != "pages";
    let reader = (settings.scroll_text && ocr::available()).then(|| ocr::Reader::start(up));
    let mut stitcher = Stitcher::new(
        dir.clone(),
        &settings.format,
        settings.jpeg_quality,
        up,
        keep_image,
        reader,
    );
    let mut thumb_first = Some(first.clone());
    let mut last_frame = Frame::new(&first);
    writer.save(&first)?;
    quick_save::show_scroll_progress(app, writer.pages, &first, locked);
    let mut last = first;

    let scale = last.height() as f64 / target.height.max(1) as f64;
    let mut step = Step::new(target.height as f64, scale);
    let mut stalls = 0;
    /* Whether anything has scrolled yet: until then a scroll that moves
    nothing means the spot is wrong, not that the end was reached. */
    let mut scrolled = false;
    let note;
    loop {
        if STOP.load(Ordering::SeqCst) {
            note = "Stopped".to_string();
            break;
        }
        if writer.pages >= max_pages {
            note = format!("Stopped at the {max_pages}-page limit");
            break;
        }
        /* The user took the mouse back: treat it as Stop. */
        if pointer.moved_by_user(app) {
            note = "Stopped".to_string();
            break;
        }
        /* Another app came up over the spot: bring the window back, or
        the scroll would go to that app. */
        if let Some(window) = &target.window {
            if window_list::frontmost_at(point.0 as f64, point.1 as f64) != Some(window.id) {
                debug_log("something covers the scroll spot: bringing the window back");
                window_list::bring_to_front(window.id);
                sleep_unless_stopped(RAISE_WAIT);
            }
        }
        pointer.go(point);
        pointer.scroll(step.units() * direction)?;
        let started = Instant::now();
        if let Some(rest) = rest {
            sleep_unless_stopped(SCROLL_LANDS);
            pointer.go(rest);
        }
        let wait = if stalls > 0 {
            settle + STALL_WAIT
        } else {
            settle
        };
        sleep_unless_stopped(wait.saturating_sub(started.elapsed()));
        if STOP.load(Ordering::SeqCst) {
            note = "Stopped".to_string();
            break;
        }

        let next = capture(app, &monitor, target, &source)?;
        if next.dimensions() != last.dimensions() {
            note = "Stopped because the display changed".to_string();
            break;
        }
        let frame = Frame::new(&next);
        let change = compare(&last_frame, &frame, up, step.expected());
        debug_log(&format!(
            "scrolled {} units: {}",
            step.units(),
            match &change {
                Change::Moved(s) => format!("moved {} px in a {} px pane", s.moved, s.pane),
                Change::Changed => "changed, no overlap found".into(),
                Change::Same => "nothing moved".into(),
            }
        ));
        if matches!(change, Change::Same) {
            if !scrolled && spot + 1 < spots.len() {
                spot += 1;
                point = spots[spot];
                debug_log(&format!("nothing scrolled there: trying spot {spot}"));
                continue;
            }
            if !scrolled {
                /* No spot moved anything: one last, longer try at the first. */
                point = spots[0];
            }
            stalls += 1;
            if stalls >= 2 {
                note = if !scrolled && target.spot.is_some() {
                    "Nothing scrolled where you clicked".to_string()
                } else if !scrolled {
                    "Nothing in the area scrolled".to_string()
                } else if up {
                    "Reached the top".to_string()
                } else {
                    "Reached the end".to_string()
                };
                break;
            }
            continue;
        }
        stalls = 0;
        scrolled = true;
        let pane = match &change {
            Change::Moved(shift) => {
                step.adjust(shift.moved, shift.pane);
                let width = next.width();
                stitcher.set_columns(
                    (width * shift.strips.start as u32 / STRIPS)
                        ..(width * shift.strips.end as u32 / STRIPS),
                );
                Some(Pane {
                    moved: shift.moved,
                    top: shift.top,
                    bottom: shift.top + shift.pane,
                })
            }
            _ => {
                step.shrink();
                None
            }
        };
        stitcher.add(&last, &next, pane)?;
        debug_log(&format!(
            "page {} saved, next step {} units",
            writer.pages + 1,
            step.units()
        ));
        writer.save(&next)?;
        quick_save::show_scroll_progress(app, writer.pages, &next, locked);
        last_frame = frame;
        last = next;
    }

    /*
     * Joining the long image and reading the text can take a while: say it
     * stopped, and give back the mouse and the screen, straight away.
     */
    platform::unlock_pointer();
    frame::hide(app);
    quick_save::set_avoid(app, None);
    quick_save::show_scroll_saving(app, writer.pages, &last, &note, "Saving…", false);

    let mut saved = Vec::new();
    if keep_pages {
        saved.push(if writer.pages == 1 {
            "1 page".to_string()
        } else {
            format!("{} pages", writer.pages)
        });
    }
    let pages = writer.pages;
    let (files, text) = stitcher.finish(
        &last,
        &mut |stage, skippable| {
            quick_save::show_scroll_saving(app, pages, &last, &note, &stage, skippable)
        },
        &|| SKIP_TEXT.load(Ordering::SeqCst),
    )?;
    match files.len() {
        0 => {}
        1 => saved.push("a long image".to_string()),
        n => saved.push(format!("a long image in {n} parts")),
    }
    /* Skipped before anything was read: there is no text to save. */
    if let Some(text) = text.filter(|t| t.whole || !t.empty) {
        let path = writer.dir.join("Text.txt");
        std::fs::write(&path, text.text)
            .map_err(|err| format!("Could not write {}: {err}", path.display()))?;
        saved.push(
            if text.whole {
                "the text"
            } else {
                "part of the text"
            }
            .to_string(),
        );
    }

    /* Read top to bottom: when scrolling up, the last page is the top. */
    if up {
        if keep_pages {
            writer.reverse()?;
        }
        thumb_first = Some(last);
    }
    Ok(Done {
        dir: writer.dir,
        title: format!("Saved {}", join_and(&saved)),
        pages: writer.pages,
        thumbnail: thumb_first,
        note,
    })
}

/// "a", "a and b", "a, b and c".
fn join_and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// Each step of a run, in dev builds, for tuning on real apps: printed,
/// and appended to lockshot-scrolling.log in the temp folder.
fn debug_log(line: &str) {
    if cfg!(debug_assertions) {
        use std::io::Write;
        eprintln!("[scrolling] {line}");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("lockshot-scrolling.log"))
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

fn sleep_unless_stopped(total: Duration) {
    let until = Instant::now() + total;
    while Instant::now() < until && !STOP.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(25).min(until - Instant::now()));
    }
}

fn find_display((x, y): (i32, i32)) -> Result<xcap::Monitor, String> {
    xcap::Monitor::all()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|m| m.x().ok() == Some(x) && m.y().ok() == Some(y))
        .ok_or_else(|| "That display is no longer available.".to_string())
}

/// Where each page is read from.
enum Source {
    /// The area of the screen, with the progress notice moved off it.
    Screen,
    /// The window alone, cut to the area: `crop` is x, y, width, height
    /// in the window image's pixels.
    Window {
        window: xcap::Window,
        crop: (u32, u32, u32, u32),
    },
}

/// Below this mean grey difference, the window's own image cut to the area
/// counts as the same picture as the screen there.
const SAME_PICTURE: f64 = 2.0;

impl Source {
    /**
     * Read pages off the window when it can be captured on its own and the
     * area lies inside it, so windows that come up in front never spoil
     * them. Checked against `screen`, the area captured off the screen
     * just now with the window in front: the cut must line up with it
     * exactly, which also catches window images with frames or borders
     * trimmed differently from their bounds (Windows).
     */
    fn pick(target: &Target, screen: &RgbaImage) -> Source {
        if cfg!(target_os = "linux") {
            /* X11 reads window pixels off the screen anyway. */
            return Source::Screen;
        }
        let Some(w) = &target.window else {
            return Source::Screen;
        };
        let inside = target.x as f64 >= w.x - 0.5
            && target.y as f64 >= w.y - 0.5
            && (target.x + target.width) as f64 <= w.x + w.width + 0.5
            && (target.y + target.height) as f64 <= w.y + w.height + 0.5;
        if !inside {
            debug_log("area reaches past the window: reading the screen");
            return Source::Screen;
        }
        let found = window_list::find(w.id).and_then(|window| {
            let image = window.capture_image().map_err(|err| err.to_string())?;
            Ok((window, image))
        });
        let (window, image) = match found {
            Ok(found) => found,
            Err(err) => {
                debug_log(&format!("window not captured ({err}): reading the screen"));
                return Source::Screen;
            }
        };
        let scale = image.width() as f64 / w.width.max(1.0);
        let crop = (
            ((target.x as f64 - w.x) * scale).round().max(0.0) as u32,
            ((target.y as f64 - w.y) * scale).round().max(0.0) as u32,
            screen.width(),
            screen.height(),
        );
        let source = Source::Window { window, crop };
        let fits = match source.cut(&image) {
            Some(cut) => {
                let off = misfit(
                    &Frame::new(screen),
                    &Frame::new(&cut),
                    0,
                    0..cut.height() as usize,
                    0..cut.width() as usize,
                );
                debug_log(&format!("window image cut to the area differs by {off:.2}"));
                off < SAME_PICTURE
            }
            None => false,
        };
        if fits {
            debug_log("reading pages off the window");
            source
        } else {
            debug_log("window image does not line up: reading the screen");
            Source::Screen
        }
    }

    /// The area out of a capture of the window, or None if it no longer
    /// holds it (the window shrank).
    fn cut(&self, image: &RgbaImage) -> Option<RgbaImage> {
        let Source::Window {
            crop: (x, y, w, h), ..
        } = *self
        else {
            return None;
        };
        (x + w <= image.width() && y + h <= image.height())
            .then(|| image::imageops::crop_imm(image, x, y, w, h).to_image())
    }
}

/// The selected area as it is now.
fn capture(
    app: &AppHandle,
    monitor: &xcap::Monitor,
    target: &Target,
    source: &Source,
) -> Result<RgbaImage, String> {
    if let Source::Window { window, .. } = source {
        let image = window
            .capture_image()
            .map_err(|err| format!("Could not capture the window: {err}"))?;
        return source
            .cut(&image)
            .ok_or_else(|| "The window got smaller than the area being captured.".to_string());
    }
    capture_screen(app, monitor, target)
}

/// The selected area as it is on screen now, with the toast out of the way.
fn capture_screen(
    app: &AppHandle,
    monitor: &xcap::Monitor,
    target: &Target,
) -> Result<RgbaImage, String> {
    if quick_save::park_if_covering(app) {
        std::thread::sleep(Duration::from_millis(120));
    }
    let image = monitor
        .capture_region(target.x, target.y, target.width, target.height)
        .map_err(|err| format!("Could not capture the screen: {err}"))?;
    if image.width() < 2 || image.height() < 2 {
        return Err("The selected area could not be captured.".into());
    }
    Ok(image)
}

/// A new folder for this run, named from the file name template.
fn output_dir(app: &AppHandle, settings: &Settings) -> Result<PathBuf, String> {
    let parent = settings::resolved_quick_save_dir(app, settings);
    let stem = quick_save::file_stem(&settings.filename_template, Local::now(), 0, 0);
    let dir = unique_dir(&parent, &format!("{stem} Scrolling"));
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("Could not create {}: {err}", dir.display()))?;
    Ok(dir)
}

fn unique_dir(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| parent.join(format!("{name} {n}")))
        .find(|p| !p.exists())
        .expect("an unused folder name")
}

/// Writes the pages as 001.png, 002.png… in the default format.
struct Writer {
    dir: PathBuf,
    format: String,
    quality: u8,
    digits: usize,
    pages: u32,
    /// Write the pages, or only count them (just the long image is kept).
    keep: bool,
}

impl Writer {
    fn new(dir: PathBuf, settings: &Settings, max_pages: u32, keep: bool) -> Self {
        Self {
            keep,
            dir,
            format: settings.format.clone(),
            quality: settings.jpeg_quality,
            digits: max_pages.to_string().len().max(3),
            pages: 0,
        }
    }

    fn path(&self, page: u32, prefix: &str) -> PathBuf {
        let ext = export::extension(&self.format);
        self.dir
            .join(format!("{prefix}{page:0width$}.{ext}", width = self.digits))
    }

    fn save(&mut self, image: &RgbaImage) -> Result<(), String> {
        if !self.keep {
            self.pages += 1;
            return Ok(());
        }
        let mut png = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|err| format!("Could not encode the capture: {err}"))?;
        let path = self.path(self.pages + 1, "");
        export::write(&png, &self.format, self.quality, &path)?;
        self.pages += 1;
        Ok(())
    }

    /// Renumber so the first file is the last page taken.
    fn reverse(&self) -> Result<(), String> {
        let rename = |from: &Path, to: &Path| {
            std::fs::rename(from, to).map_err(|err| format!("Could not rename a page: {err}"))
        };
        for page in 1..=self.pages {
            rename(&self.path(page, ""), &self.path(page, "."))?;
        }
        for page in 1..=self.pages {
            rename(&self.path(page, "."), &self.path(self.pages + 1 - page, ""))?;
        }
        Ok(())
    }
}

/// The area is compared in this many vertical strips.
const STRIPS: u32 = 8;

/**
 * A captured page's rows, hashed separately in vertical strips. Comparing
 * whole rows would hide the movement: a sidebar beside the scrolling pane
 * stays put, so its half of every row never lines up with the moved half.
 * In strips, the ones over the pane line up and the rest are ignored.
 */
struct Frame {
    strips: Vec<Vec<Row>>,
    /// The page in grey, to check a measured move against the pixels
    /// themselves: exact row hashes can be thrown by faint noise.
    grey: Vec<u8>,
    width: usize,
    height: usize,
}

impl Frame {
    fn new(image: &RgbaImage) -> Self {
        let width = image.width();
        let strips = (0..STRIPS)
            .map(|i| {
                let x0 = width * i / STRIPS;
                let x1 = (width * (i + 1) / STRIPS).max(x0 + 1).min(width);
                (0..image.height()).map(|y| row(image, y, x0, x1)).collect()
            })
            .collect();
        let grey = image
            .pixels()
            .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
            .collect();
        Frame {
            strips,
            grey,
            width: width as usize,
            height: image.height() as usize,
        }
    }
}

/// How badly `after` matches `before` with the content moved up `moved`
/// rows: the mean grey difference between `after[y]` and `before[y +
/// moved]` over `rows` and `columns` of `after`. 0 is a perfect match.
fn misfit(
    before: &Frame,
    after: &Frame,
    moved: usize,
    rows: Range<usize>,
    columns: Range<usize>,
) -> f64 {
    let height = before.height.min(after.height);
    let rows = rows.start..rows.end.min(height.saturating_sub(moved));
    let columns = columns.start..columns.end.min(before.width).min(after.width);
    if rows.is_empty() || columns.is_empty() {
        return f64::MAX;
    }
    let mut total = 0u64;
    for y in rows.clone() {
        let a = &after.grey[y * after.width..][columns.clone()];
        let b = &before.grey[(y + moved) * before.width..][columns.clone()];
        total += a
            .iter()
            .zip(b)
            .map(|(p, q)| p.abs_diff(*q) as u64)
            .sum::<u64>();
    }
    total as f64 / (rows.len() * columns.len()) as f64
}

/// Below this mean grey difference two pages count as the same picture.
/// Identical captures can still differ faintly (smoothing, a fading
/// scroll bar), which is enough to throw the exact row hashes.
const STILL_NOISE: f64 = 1.0;

/// A row of a strip: its hash, and whether it is one flat colour (blank
/// rows match anywhere, so they are no use for finding the scroll).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Row {
    hash: u64,
    flat: bool,
}

fn row(image: &RgbaImage, y: u32, x0: u32, x1: u32) -> Row {
    let width = image.width() as usize;
    let raw = image.as_raw();
    let start = (y as usize * width + x0 as usize) * 4;
    let end = (y as usize * width + x1 as usize) * 4;
    let pixels = &raw[start..end];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in pixels {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let (pixels4, _) = pixels.as_chunks::<4>();
    let flat = pixels4.iter().all(|p| p == &pixels4[0]);
    Row { hash, flat }
}

/// What a scroll did to the area.
enum Change {
    /// The content moved by a measurable amount.
    Moved(Shift),
    /// Much of it changed, but no overlap was found: the step may have
    /// skipped past some content, or the app swapped in new messages.
    Changed,
    /// Nothing moved; at most a caret blinked or a badge changed.
    Same,
}

/// Compare two pages. `up`: the scroll was towards the top, so the
/// content moved down the window.
fn compare(before: &Frame, after: &Frame, up: bool, expected: Option<usize>) -> Change {
    /* Measured as content moving up from `older` to `newer`. */
    let (older, newer) = if up { (after, before) } else { (before, after) };
    if let Some(shift) = find_shift(&older.strips, &newer.strips, expected) {
        /*
         * On a list of look-alike rows a page that did not move at all can
         * still line up at some offset. Keep the move only if the pixels
         * fit better moved than left where they were.
         */
        let strips = STRIPS as usize;
        let columns =
            newer.width * shift.strips.start / strips..newer.width * shift.strips.end / strips;
        let rows = shift.top..shift.top + shift.pane - shift.moved;
        let moved = misfit(older, newer, shift.moved, rows.clone(), columns.clone());
        let still = misfit(older, newer, 0, rows, columns);
        if moved < still {
            return Change::Moved(shift);
        }
        debug_log(&format!(
            "  shift {} px rejected: fits {moved:.2} moved, {still:.2} in place",
            shift.moved
        ));
    }
    if misfit(before, after, 0, 0..after.height, 0..after.width) < STILL_NOISE {
        return Change::Same;
    }
    let (mut compared, mut differing) = (0usize, 0usize);
    for (b, a) in before.strips.iter().zip(&after.strips) {
        for (rb, ra) in b.iter().zip(a) {
            if rb.flat && ra.flat {
                continue;
            }
            compared += 1;
            if rb != ra {
                differing += 1;
            }
        }
    }
    if compared >= 8 && differing * 2 >= compared {
        Change::Changed
    } else {
        Change::Same
    }
}

struct Shift {
    /// How far the content moved, in image pixels.
    moved: usize,
    /// Roughly where the moving pane starts, in rows from the top.
    top: usize,
    /// The strips that moved: the pane's columns, in eighths of the width.
    strips: Range<usize>,
    /// Roughly how tall the moving pane is, in image pixels.
    pane: usize,
}

/**
 * How far the content moved up from `before` to `after`: the offset d where
 * `after[y]` matches `before[y + d]` for the most rows with content.
 *
 * Each row votes for the offsets its hash appears at, and the votes of all
 * the strips that moved are added up. Content that repeats (the same text
 * on every line, a column of identical avatars) votes for many offsets, so
 * one strip alone can be fooled, but only the true offset gets votes from
 * every strip. Strips that mostly match where they were (a sidebar) are
 * left out.
 */
fn find_shift(before: &[Vec<Row>], after: &[Vec<Row>], expected: Option<usize>) -> Option<Shift> {
    use std::collections::HashMap;

    let height = before.iter().chain(after).map(Vec::len).min().unwrap_or(0);
    let moving: Vec<usize> = (0..before.len().min(after.len()))
        .filter(|&i| {
            let (b, a) = (&before[i], &after[i]);
            let content = (0..height).filter(|&y| !a[y].flat).count();
            let in_place = (0..height).filter(|&y| !a[y].flat && a[y] == b[y]).count();
            content > 0 && in_place * 2 < content
        })
        .collect();

    let mut votes = vec![0usize; height];
    let mut positions: Vec<HashMap<u64, Vec<usize>>> = Vec::new();
    for &i in &moving {
        let mut at: HashMap<u64, Vec<usize>> = HashMap::new();
        for (y, r) in before[i].iter().enumerate().take(height) {
            if !r.flat {
                at.entry(r.hash).or_default().push(y);
            }
        }
        /*
         * A row that appears on every line votes for every multiple of the
         * line height, the true offset among them, so it still helps. One
         * that appears far more often (a scroll bar track) only adds noise.
         */
        at.retain(|_, list| list.len() <= 64);
        for (y, r) in after[i].iter().enumerate().take(height) {
            if r.flat {
                continue;
            }
            for &from in at.get(&r.hash).map(Vec::as_slice).unwrap_or(&[]) {
                if from > y {
                    votes[from - y] += 1;
                }
            }
        }
        positions.push(at);
    }
    /*
     * Rows with content that could take part at each offset: those in the
     * part of `after` that still overlaps `before`. counted[k] covers the
     * first k rows.
     */
    let mut counted = vec![0usize; height + 1];
    for y in 0..height {
        counted[y + 1] = counted[y]
            + moving
                .iter()
                .zip(&positions)
                .filter(|(&i, at)| !after[i][y].flat && at.contains_key(&after[i][y].hash))
                .count();
    }
    /*
     * Score each offset by the share of those rows that agree, not by the
     * raw count: on repeating content a small offset overlaps more rows and
     * collects more votes without being right. The true offset has nearly
     * every row agree. Among offsets that do about as well, take the one
     * nearest what this step should have moved.
     */
    let score = |d: usize| votes[d] as f64 / counted[height - d].max(1) as f64;
    let plausible = |d: &usize| votes[*d] >= 12;
    let best = (1..height).filter(plausible).map(score).fold(0.0, f64::max);
    if best < 0.5 {
        return None;
    }
    let near_best = (1..height)
        .filter(plausible)
        .filter(|&d| score(d) >= best * 0.9);
    let moved = match expected {
        Some(e) => near_best.min_by_key(|&d| d.abs_diff(e))?,
        None => near_best.max_by_key(|&d| votes[d])?,
    };
    debug_log(&format!(
        "  shift {moved} px (expected {expected:?}), {:.0}% of rows agree, {} strips moving",
        score(moved) * 100.0,
        moving.len()
    ));
    /* The pane spans the rows that moved, plus the distance they moved. */
    let matched = (0..height - moved).filter(|&y| {
        moving
            .iter()
            .any(|&i| !after[i][y].flat && after[i][y] == before[i][y + moved])
    });
    let (mut top, mut bottom) = (usize::MAX, 0);
    for y in matched {
        top = top.min(y);
        bottom = bottom.max(y);
    }
    if top > bottom {
        return None;
    }
    Some(Shift {
        moved,
        top,
        strips: moving[0]..moving[moving.len() - 1] + 1,
        pane: bottom - top + moved,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of text lines down the left side, blank on the right, with a
    /// static sidebar strip on the far left, like a chat app.
    fn page(height: u32, offset: u32) -> RgbaImage {
        RgbaImage::from_fn(800, height, |x, y| {
            if x < 100 {
                /* Sidebar: does not scroll. */
                let v = ((y * 13 + x) % 200) as u8;
                return image::Rgba([v, 40, 90, 255]);
            }
            let line = y + offset;
            if x > 400 || line.is_multiple_of(7) {
                image::Rgba([255, 255, 255, 255])
            } else {
                let v = ((line * 31 + x * (line % 5 + 1)) % 251) as u8;
                image::Rgba([v, v.wrapping_mul(3), 255 - v, 255])
            }
        })
    }

    #[test]
    fn finds_how_far_the_content_scrolled() {
        let before = Frame::new(&page(400, 0));
        let after = Frame::new(&page(400, 120));
        match compare(&before, &after, false, None) {
            Change::Moved(shift) => {
                assert_eq!(shift.moved, 120);
                assert!(shift.pane >= 380, "{}", shift.pane);
            }
            _ => panic!("expected a move"),
        }
        /* Scrolling up: the earlier page is further down the content. */
        match compare(&after, &before, true, None) {
            Change::Moved(shift) => assert_eq!(shift.moved, 120),
            _ => panic!("expected a move up"),
        }
    }

    /// Like page(), but the right part of every line is the same text, so
    /// that strip repeats every 7 rows.
    fn repeating_page(height: u32, offset: u32) -> RgbaImage {
        let mut image = page(height, offset);
        for (x, y, p) in image.enumerate_pixels_mut() {
            let line = y + offset;
            if (200..400).contains(&x) && !line.is_multiple_of(7) {
                let v = ((line % 7) * 30 + x % 17) as u8;
                *p = image::Rgba([v, v, 255 - v, 255]);
            }
        }
        image
    }

    #[test]
    fn repeating_text_does_not_fool_it() {
        let before = Frame::new(&repeating_page(400, 0));
        let after = Frame::new(&repeating_page(400, 90));
        match compare(&before, &after, false, Some(84)) {
            Change::Moved(shift) => assert_eq!(shift.moved, 90),
            _ => panic!("expected a move"),
        }
    }

    #[test]
    fn spots_start_in_the_middle_and_stay_inside() {
        let target = Target {
            display: (100, 50),
            x: 200,
            y: 100,
            width: 400,
            height: 800,
            spot: None,
            window: None,
        };
        let spots = target.spots();
        assert_eq!(spots[0], (500, 550));
        assert_eq!(spots[1], (500, 750));
        for (x, y) in spots {
            assert!((300..700).contains(&x) && (150..950).contains(&y));
        }
    }

    #[test]
    fn a_clicked_spot_is_the_only_one() {
        let target = Target {
            display: (100, 50),
            x: 200,
            y: 100,
            width: 400,
            height: 800,
            spot: Some((520, 300)),
            window: None,
        };
        assert_eq!(target.spots(), vec![(620, 350)]);
    }

    #[test]
    fn the_pointer_rests_on_the_border_outside_the_area() {
        let mut target = Target {
            display: (100, 50),
            x: 200,
            y: 100,
            width: 400,
            height: 800,
            spot: None,
            window: None,
        };
        /* The right side's border, halfway down, on the desktop. */
        assert_eq!(target.rest_spot(1920, 1080), Some((100 + 602, 50 + 500)));
        /* Against the right edge: the left side. */
        target.x = 1520;
        assert_eq!(target.rest_spot(1920, 1080), Some((100 + 1517, 50 + 500)));
        /* The whole display: nowhere off the area. */
        let whole = Target {
            display: (0, 0),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            spot: None,
            window: None,
        };
        assert_eq!(whole.rest_spot(1920, 1080), None);
        /* A border only a few points from the display's edge is skipped
        (hot corners, a hidden Dock or menu bar). */
        let full_width = Target {
            display: (0, 0),
            x: 0,
            y: 3,
            width: 1920,
            height: 1000,
            spot: None,
            window: None,
        };
        assert_eq!(full_width.rest_spot(1920, 1080), Some((960, 1005)));
    }

    /// The end of a list of look-alike rows (a GitHub runs page): the last
    /// scroll moved nothing, but faint noise made every row hash differ,
    /// and the rows lined up at a false offset that was stitched on again.
    #[test]
    fn faint_noise_on_a_page_that_did_not_move_is_not_a_scroll() {
        let page = repeating_page(400, 0);
        let mut noisy = page.clone();
        for (x, y, p) in noisy.enumerate_pixels_mut() {
            if (x + y) % 3 == 0 {
                p[2] ^= 1;
            }
        }
        let (a, b) = (Frame::new(&page), Frame::new(&noisy));
        assert!(matches!(compare(&a, &b, false, Some(84)), Change::Same));
        assert!(matches!(compare(&a, &b, true, Some(84)), Change::Same));
        /* The pixel check still keeps a real move on look-alike rows. */
        let moved = Frame::new(&repeating_page(400, 90));
        match compare(&a, &moved, false, Some(84)) {
            Change::Moved(shift) => assert_eq!(shift.moved, 90),
            _ => panic!("expected a move"),
        }
    }

    #[test]
    fn same_pages_are_unchanged() {
        let a = Frame::new(&page(300, 0));
        assert!(matches!(compare(&a, &a, false, None), Change::Same));
        assert!(matches!(compare(&a, &a, true, None), Change::Same));
    }

    #[test]
    fn a_jump_with_no_overlap_still_counts_as_changed() {
        let a = Frame::new(&page(300, 0));
        let b = Frame::new(&page(300, 5000));
        assert!(matches!(compare(&a, &b, false, None), Change::Changed));
    }

    #[test]
    fn step_aims_at_most_of_the_pane() {
        let mut step = Step {
            amount: 100.0,
            min: 1.0,
            max: 10_000.0,
            px_per_unit: None,
        };
        /* 100 units moved 250 px in a 1000 px pane: aim for 700 px. */
        step.adjust(250, 1000);
        assert_eq!(step.units(), 280);
        /* A far bigger jump is taken in steps of at most 3×. */
        step.adjust(10, 1000);
        assert_eq!(step.units(), 840);
        step.shrink();
        assert_eq!(step.units(), 420);
    }

    /// Run with --ignored, and LOCKSHOT_PAGE_A and LOCKSHOT_PAGE_B set to two
    /// pages of a real run, to check the comparison on real pixels.
    #[test]
    #[ignore]
    fn compare_saved_captures() {
        let open = |var: &str| {
            image::open(std::env::var(var).expect(var))
                .unwrap()
                .to_rgba8()
        };
        let (a, b) = (open("LOCKSHOT_PAGE_A"), open("LOCKSHOT_PAGE_B"));
        let differing: Vec<u32> = (0..a.height())
            .filter(|&y| {
                (0..a.width()).any(|x| {
                    let (p, q) = (a.get_pixel(x, y).0, b.get_pixel(x, y).0);
                    (0..3).any(|c| p[c].abs_diff(q[c]) > 40)
                })
            })
            .collect();
        eprintln!(
            "{} rows differ clearly, first {:?}, last {:?}",
            differing.len(),
            differing.first(),
            differing.last()
        );
        let (fa, fb) = (Frame::new(&a), Frame::new(&b));
        eprintln!(
            "misfit in place {:.3}",
            misfit(&fa, &fb, 0, 0..fa.height, 0..fa.width)
        );
        let result = match compare(&fa, &fb, false, Some(600)) {
            Change::Moved(s) => format!("moved {} px, pane {} px", s.moved, s.pane),
            Change::Changed => "changed".into(),
            Change::Same => "same".into(),
        };
        eprintln!("{result}");
    }
}

/// Run with --ignored and LOCKSHOT_LONG_IMAGE set to a tall render of a
/// chat page: it is cut into overlapping window captures (a fixed header,
/// sidebar and message box around a scrolling pane) and run through the
/// same measuring, stitching and text reading as a real run.
#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use image::imageops;

    const HEADER: u32 = 60;
    const PANE: u32 = 600;
    const FOOTER: u32 = 80;
    const SIDEBAR: u32 = 160;

    fn window(doc: &RgbaImage, offset: u32) -> RgbaImage {
        let width = doc.width();
        let mut out = RgbaImage::from_pixel(
            width,
            HEADER + PANE + FOOTER,
            image::Rgba([240, 240, 245, 255]),
        );
        let pane = imageops::crop_imm(doc, 0, offset, width, PANE).to_image();
        imageops::replace(&mut out, &pane, 0, HEADER as i64);
        let sidebar = imageops::crop_imm(doc, 0, 0, SIDEBAR, PANE).to_image();
        imageops::replace(&mut out, &sidebar, 0, HEADER as i64);
        for x in 300..340 {
            out.put_pixel(x, HEADER / 2, image::Rgba([0, 0, 0, 255]));
            out.put_pixel(x, HEADER + PANE + FOOTER / 2, image::Rgba([0, 0, 0, 255]));
        }
        out
    }

    fn run_pages(doc: &RgbaImage, offsets: &[u32], up: bool) -> (RgbaImage, String) {
        let dir = std::env::temp_dir().join(format!("lockshot-pipeline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let reader = ocr::available().then(|| ocr::Reader::start(up));
        let mut stitcher = Stitcher::new(dir.clone(), "png", 90, up, true, reader);
        let pages: Vec<RgbaImage> = offsets.iter().map(|&o| window(doc, o)).collect();
        for pair in pages.windows(2) {
            let (last, next) = (&pair[0], &pair[1]);
            let Change::Moved(shift) = compare(&Frame::new(last), &Frame::new(next), up, None)
            else {
                panic!("expected a move");
            };
            let width = next.width();
            stitcher.set_columns(
                (width * shift.strips.start as u32 / STRIPS)
                    ..(width * shift.strips.end as u32 / STRIPS),
            );
            let pane = Pane {
                moved: shift.moved,
                top: shift.top,
                bottom: shift.top + shift.pane,
            };
            stitcher.add(last, next, Some(pane)).unwrap();
        }
        let (files, text) = stitcher
            .finish(pages.last().unwrap(), &mut |_, _| {}, &|| false)
            .unwrap();
        let image = image::open(&files[0]).unwrap().to_rgba8();
        std::fs::remove_dir_all(dir).unwrap();
        (image, text.map(|t| t.text).unwrap_or_default())
    }

    #[test]
    #[ignore]
    fn stitches_and_reads_a_long_page() {
        let path = std::env::var("LOCKSHOT_LONG_IMAGE").expect("LOCKSHOT_LONG_IMAGE");
        let doc = image::open(path).unwrap().to_rgba8();
        let offsets = [0, 420, 853, 1254, 1690, 2111, 2530, 2948, 3380];
        let last = *offsets.last().unwrap();
        let expected_height = HEADER + last + PANE + FOOTER;
        let check = |image: &RgbaImage| {
            assert_eq!(image.height(), expected_height);
            /* The message column matches the page row for row. */
            let content =
                imageops::crop_imm(image, SIDEBAR, HEADER, doc.width() - SIDEBAR, last + PANE)
                    .to_image();
            let original =
                imageops::crop_imm(&doc, SIDEBAR, 0, doc.width() - SIDEBAR, last + PANE).to_image();
            assert!(
                content == original,
                "stitched content differs from the page"
            );
        };
        let (down, text) = run_pages(&doc, &offsets, false);
        check(&down);
        eprintln!("--- down ---\n{text}");
        let reversed: Vec<u32> = offsets.iter().rev().copied().collect();
        let (up, text_up) = run_pages(&doc, &reversed, true);
        check(&up);
        for (name, text) in [("down", &text), ("up", &text_up)] {
            let missing: Vec<u32> = (1..=60)
                .filter(|n| !text.contains(&format!("Message {n}:")))
                .collect();
            eprintln!(
                "{name}: {} lines, misread {missing:?}",
                text.lines().count()
            );
            assert!(missing.len() <= 2, "{name} misread {missing:?}");
        }
    }
}

#[cfg(test)]
mod source_check {
    use super::*;

    /// Run with --ignored: the frontmost window's own image, cut to a part
    /// of it, must line up with the screen there.
    #[test]
    #[ignore]
    fn the_front_window_lines_up_with_the_screen() {
        let w = &window_list::all()[0];
        let monitor = xcap::Monitor::all()
            .unwrap()
            .into_iter()
            .find(|m| {
                let (x, y) = (m.x().unwrap() as f64, m.y().unwrap() as f64);
                let (mw, mh) = (m.width().unwrap() as f64, m.height().unwrap() as f64);
                w.x >= x && w.y >= y && w.x < x + mw && w.y < y + mh
            })
            .unwrap();
        let (mx, my) = (monitor.x().unwrap(), monitor.y().unwrap());
        let window = Window {
            id: w.id,
            x: w.x - mx as f64,
            y: w.y - my as f64,
            width: w.width,
            height: w.height,
        };
        let target = Target {
            display: (mx, my),
            x: (window.x + 40.0) as u32,
            y: (window.y + 60.0) as u32,
            width: (w.width - 80.0).min(600.0) as u32,
            height: (w.height - 120.0).min(500.0) as u32,
            spot: None,
            window: Some(window),
        };
        let screen = monitor
            .capture_region(target.x, target.y, target.width, target.height)
            .unwrap();
        eprintln!("window {}", w.label);
        assert!(matches!(
            Source::pick(&target, &screen),
            Source::Window { .. }
        ));
    }
}

//! Scrolling Capture: pick a window, and Lockshot scrolls it a page at a time,
//! saving a screenshot of the window after each scroll, until the content
//! stops moving. Meant for long chat threads (Slack, Discord, Teams) and
//! other pages whose lists are virtualized, so the full history is never in
//! the page source at once.
//!
//! The scroll is a real scroll-wheel event sent with the pointer over the
//! spot the user clicked, so it reaches the pane under it whether or not the
//! window has keyboard focus. Every run goes to its own folder of numbered
//! files, and the pages are joined into one long image as they come in (see
//! stitch.rs); on macOS the text can be read off them too (see ocr.rs).
//! Esc, the Stop button, or moving the mouse ends it early.

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
use crate::{export, hotkeys, ocr, platform, quick_save};

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

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

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Esc, the Stop button, or a second press of the hotkey.
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// Where to scroll and what to capture, in the units xcap reports windows in
/// (points on macOS, physical pixels elsewhere).
pub struct Target {
    pub window_id: u32,
    pub point: (i32, i32),
    /// For sizing the first scroll step.
    pub window_height: f64,
}

/// Start the run on its own thread. Returns at once.
pub fn start(app: &AppHandle, target: Target) -> Result<(), String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("A scrolling capture is already running.".into());
    }
    STOP.store(false, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        let esc = hotkeys::register_extra(&app, STOP_ACTION, STOP_KEY)
            .inspect_err(|err| eprintln!("{err}"))
            .is_ok();
        let outcome = run(&app, &target);
        if esc {
            hotkeys::unregister_extra(&app, STOP_KEY);
        }
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

struct Done {
    dir: PathBuf,
    /// What was saved, such as "Saved 12 pages and a long image".
    title: String,
    pages: u32,
    thumbnail: Option<RgbaImage>,
    note: String,
}

/// Moves the pointer and sends scroll-wheel steps.
struct Pointer {
    enigo: Enigo,
}

impl Pointer {
    fn new() -> Result<Self, String> {
        let settings = enigo::Settings {
            open_prompt_to_get_permissions: false,
            ..Default::default()
        };
        let enigo = Enigo::new(&settings).map_err(|err| {
            format!("Lockshot could not control the mouse to scroll the window: {err}")
        })?;
        Ok(Self { enigo })
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
        result.map_err(|err| format!("Could not scroll the window: {err}"))
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
    /// `scale`: image pixels per point of the window (2 on Retina).
    fn new(window_height: f64, scale: f64) -> Self {
        if cfg!(target_os = "macos") {
            /* Pixel scrolling: start at a third of the window, safely under
            a page. A point of scroll is usually a point of content. */
            Step {
                amount: (window_height * 0.3).max(40.0),
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
    let window = find_window(target.window_id)?;
    let mut pointer = Pointer::new()?;
    let up = settings.scroll_direction == "up";
    let direction = if up { -1 } else { 1 };
    let delay = Duration::from_millis(settings.scroll_delay_ms as u64);
    let max_pages = settings.scroll_max_pages.max(1);

    /* X11 reads window pixels off the screen, so let the overlay go first. */
    std::thread::sleep(Duration::from_millis(150));
    let first = capture(&window)?;

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
    quick_save::show_scroll_progress(app, writer.pages, &first);
    let mut last = first;

    let scale = if target.window_height > 0.0 {
        last.height() as f64 / target.window_height
    } else {
        1.0
    };
    let mut step = Step::new(target.window_height, scale);
    let mut stalls = 0;
    let mut placed = false;
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
        if placed {
            if let Some((x, y)) = platform::cursor_point(app) {
                let (px, py) = target.point;
                if (x - px).abs() > POINTER_SLACK || (y - py).abs() > POINTER_SLACK {
                    note = "Stopped".to_string();
                    break;
                }
            }
        }
        pointer.move_to(target.point);
        placed = true;
        pointer.scroll(step.units() * direction)?;
        let started = Instant::now();
        let wait = if stalls > 0 {
            delay + STALL_WAIT
        } else {
            delay
        };
        sleep_unless_stopped(wait.saturating_sub(started.elapsed()));
        if STOP.load(Ordering::SeqCst) {
            note = "Stopped".to_string();
            break;
        }

        let next = capture(&window)?;
        if next.dimensions() != last.dimensions() {
            note = "Stopped because the window changed size".to_string();
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
            stalls += 1;
            if stalls >= 2 {
                note = if up {
                    "Reached the top".to_string()
                } else {
                    "Reached the end".to_string()
                };
                break;
            }
            continue;
        }
        stalls = 0;
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
        quick_save::show_scroll_progress(app, writer.pages, &next);
        last_frame = frame;
        last = next;
    }

    let mut saved = Vec::new();
    if keep_pages {
        saved.push(if writer.pages == 1 {
            "1 page".to_string()
        } else {
            format!("{} pages", writer.pages)
        });
    }
    let (files, text) = stitcher.finish(&last)?;
    match files.len() {
        0 => {}
        1 => saved.push("a long image".to_string()),
        n => saved.push(format!("a long image in {n} parts")),
    }
    if let Some(text) = text {
        let path = writer.dir.join("Text.txt");
        std::fs::write(&path, text)
            .map_err(|err| format!("Could not write {}: {err}", path.display()))?;
        saved.push("the text".to_string());
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

fn find_window(id: u32) -> Result<xcap::Window, String> {
    xcap::Window::all()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|w| w.id().ok() == Some(id))
        .ok_or_else(|| "That window has closed.".to_string())
}

fn capture(window: &xcap::Window) -> Result<RgbaImage, String> {
    let image = window
        .capture_image()
        .map_err(|err| format!("Could not capture the window: {err}"))?;
    if image.width() < 2 || image.height() < 2 {
        return Err("The window could not be captured. Is it minimized?".into());
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

/// The window is compared in this many vertical strips.
const STRIPS: u32 = 8;

/**
 * A captured page's rows, hashed separately in vertical strips. Comparing
 * whole rows would hide the movement: a sidebar beside the scrolling pane
 * stays put, so its half of every row never lines up with the moved half.
 * In strips, the ones over the pane line up and the rest are ignored.
 */
struct Frame {
    strips: Vec<Vec<Row>>,
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
        Frame { strips }
    }
}

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

/// What a scroll did to the window.
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
    let shift = if up {
        find_shift(&after.strips, &before.strips, expected)
    } else {
        find_shift(&before.strips, &after.strips, expected)
    };
    if let Some(shift) = shift {
        return Change::Moved(shift);
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

    /// Run with --ignored against two captures of a real window, saved by
    /// examples/scrolltest.rs, to check the comparison on real pixels.
    #[test]
    #[ignore]
    fn compare_saved_captures() {
        let a = image::open("/tmp/claude-501/scroll-a.png")
            .unwrap()
            .to_rgba8();
        let b = image::open("/tmp/claude-501/scroll-b.png")
            .unwrap()
            .to_rgba8();
        let result = match compare(&Frame::new(&a), &Frame::new(&b), false, Some(600)) {
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
        let (files, text) = stitcher.finish(pages.last().unwrap()).unwrap();
        let image = image::open(&files[0]).unwrap().to_rgba8();
        std::fs::remove_dir_all(dir).unwrap();
        (image, text.unwrap_or_default())
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

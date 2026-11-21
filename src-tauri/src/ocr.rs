//! Scrolling Capture: read the text off the long image, so a chat thread
//! can be saved as a text file as well as pictures. macOS only for now,
//! through Apple's Vision framework, which runs on the device.
//!
//! The rows the stitcher joins are fed in as they come, and read on a worker
//! thread in overlapping chunks while the run carries on scrolling. Reading
//! the joined rows rather than the pages means no line is ever cut by the
//! edge of the scrolling pane. Each line belongs to the chunk that holds its
//! middle, so a line on a chunk boundary is read whole, once.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use image::{imageops, RgbaImage};

/// Rows each chunk owns.
const CHUNK: u32 = 1200;
/// Rows read above and below what a chunk owns, so lines on a boundary are
/// read whole. Taller than any line of chat text, even on Retina.
const MARGIN: u32 = 80;
/// Chunks that may wait for the reader before the run slows down to let it
/// catch up.
const QUEUE: usize = 4;

pub fn available() -> bool {
    imp::available()
}

/**
 * Read a small built-in picture of text, so the first Scrolling Capture
 * after an install or update does not wait on Vision. macOS loads its text
 * models the first time each new build of the app reads text, which took
 * 15 to 50 seconds in testing; after that, a page takes a fraction of a
 * second. The picture needs real text: a blank one only loads half of it.
 */
pub fn warm_up() {
    if !available() {
        return;
    }
    let bytes = include_bytes!("../assets/ocr-warmup.png");
    match image::load_from_memory(bytes) {
        Ok(picture) => {
            if let Err(err) = imp::recognize(&picture.to_rgba8()) {
                eprintln!("Could not warm up text reading: {err}");
            }
        }
        Err(err) => eprintln!("Could not load the text warm-up picture: {err}"),
    }
}

/// A piece of text found in an image, in pixels from its top left.
#[derive(Clone, Debug)]
pub struct Found {
    pub text: String,
    pub x: f64,
    /// Middle of the text, top down.
    pub y: f64,
    pub height: f64,
}

struct Job {
    image: RgbaImage,
    /// Keep text whose middle falls in these rows of `image`.
    keep: Range<u32>,
}

/// Takes the long image's rows in scroll order and reads them in chunks.
pub struct Reader {
    /// Rows arrive bottom up and upside down when scrolling up.
    up: bool,
    /// Rows not read yet (plus the margin above them), in scroll order.
    buffer: Option<RgbaImage>,
    /// The first chunk owns its top rows too; later ones leave them to the
    /// chunk before.
    started: bool,
    /// The columns to read: the scrolling pane, leaving out sidebars.
    columns: Option<Range<u32>>,
    jobs: Option<SyncSender<Job>>,
    worker: Option<JoinHandle<Vec<Vec<String>>>>,
    /// Chunks sent to the worker, and chunks it has read, to show progress.
    sent: AtomicUsize,
    read: Arc<AtomicUsize>,
}

impl Reader {
    pub fn start(up: bool) -> Self {
        let (tx, rx) = sync_channel::<Job>(QUEUE);
        let read = Arc::new(AtomicUsize::new(0));
        let counter = read.clone();
        let worker = std::thread::spawn(move || {
            rx.into_iter()
                .map(|job| {
                    let block = match imp::recognize(&job.image) {
                        Ok(found) => lines(found, &job.keep),
                        Err(err) => {
                            eprintln!("Could not read text: {err}");
                            Vec::new()
                        }
                    };
                    counter.fetch_add(1, Ordering::SeqCst);
                    block
                })
                .collect()
        });
        Self {
            up,
            buffer: None,
            started: false,
            columns: None,
            jobs: Some(tx),
            worker: Some(worker),
            sent: AtomicUsize::new(0),
            read,
        }
    }

    pub fn set_columns(&mut self, columns: Range<u32>) {
        if !columns.is_empty() {
            self.columns = Some(columns);
        }
    }

    /// The next rows of the long image, as the stitcher holds them.
    pub fn push(&mut self, rows: &RgbaImage) {
        let buffer = match self.buffer.take() {
            None => rows.clone(),
            Some(old) => {
                let mut joined = RgbaImage::new(old.width(), old.height() + rows.height());
                imageops::replace(&mut joined, &old, 0, 0);
                imageops::replace(&mut joined, rows, 0, old.height() as i64);
                joined
            }
        };
        self.buffer = Some(buffer);
        let lead = if self.started { MARGIN } else { 0 };
        while self
            .buffer
            .as_ref()
            .is_some_and(|b| b.height() >= lead + CHUNK + MARGIN)
        {
            let buffer = self.buffer.take().unwrap();
            let end = lead + CHUNK + MARGIN;
            self.send(
                &buffer,
                0..end,
                (if self.started { MARGIN } else { 0 })..(end - MARGIN),
            );
            self.started = true;
            /* Keep the margin read below this chunk; the next chunk reads
            it again as its own top margin. */
            let from = end - 2 * MARGIN;
            self.buffer = Some(
                imageops::crop_imm(&buffer, 0, from, buffer.width(), buffer.height() - from)
                    .to_image(),
            );
        }
    }

    /// Read what is left. One block of lines per chunk, in reading order.
    /// `progress` hears how many chunks are read out of how many, whenever
    /// that changes, while it waits.
    pub fn finish(mut self, mut progress: impl FnMut(usize, usize)) -> Vec<Vec<String>> {
        if let Some(buffer) = self.buffer.take() {
            let start = if self.started {
                MARGIN.min(buffer.height())
            } else {
                0
            };
            self.send(&buffer, 0..buffer.height(), start..buffer.height());
        }
        self.jobs.take();
        let total = self.sent.load(Ordering::SeqCst);
        let mut shown = None;
        while self.worker.as_ref().is_some_and(|w| !w.is_finished()) {
            let read = self.read.load(Ordering::SeqCst);
            if shown != Some(read) {
                progress(read, total);
                shown = Some(read);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let mut blocks = self
            .worker
            .take()
            .and_then(|worker| worker.join().ok())
            .unwrap_or_default();
        /* Chunks were read in scroll order: bottom up when scrolling up. */
        if self.up {
            blocks.reverse();
        }
        blocks
    }

    /// Read `rows` of `buffer`, keeping text in `keep` (both in scroll order).
    fn send(&self, buffer: &RgbaImage, rows: Range<u32>, keep: Range<u32>) {
        let Some(jobs) = &self.jobs else { return };
        if keep.is_empty() {
            return;
        }
        let columns = self.columns.clone().unwrap_or(0..buffer.width());
        let columns = columns.start.min(buffer.width())..columns.end.min(buffer.width());
        let mut image = imageops::crop_imm(
            buffer,
            columns.start,
            rows.start,
            columns.len() as u32,
            rows.len() as u32,
        )
        .to_image();
        let mut keep = (keep.start - rows.start)..(keep.end - rows.start);
        if self.up {
            imageops::flip_vertical_in_place(&mut image);
            let height = image.height();
            keep = (height - keep.end)..(height - keep.start);
        }
        if jobs.send(Job { image, keep }).is_ok() {
            self.sent.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// The text kept from one chunk, grouped into lines top down, with pieces
/// side by side on a line (a name and a time) joined by two spaces.
fn lines(mut found: Vec<Found>, keep: &Range<u32>) -> Vec<String> {
    found
        .retain(|f| f.y >= keep.start as f64 && f.y < keep.end as f64 && !f.text.trim().is_empty());
    found.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut rows: Vec<Vec<Found>> = Vec::new();
    for f in found {
        match rows.last_mut() {
            Some(row) if (f.y - row[0].y).abs() < row[0].height.min(f.height) * 0.5 => row.push(f),
            _ => rows.push(vec![f]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            row.iter()
                .map(|f| f.text.trim())
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect()
}

/// Text file contents from the blocks of lines, in reading order.
pub fn text(blocks: &[Vec<String>]) -> String {
    let mut text = blocks
        .iter()
        .flatten()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io::Cursor;

    use image::RgbaImage;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyClass, NSObjectProtocol};
    use objc2::{sel, AnyThread};
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    use super::Found;

    pub fn available() -> bool {
        AnyClass::get(c"VNRecognizeTextRequest").is_some()
    }

    pub fn recognize(image: &RgbaImage) -> Result<Vec<Found>, String> {
        if !available() {
            return Err("Text recognition needs macOS 10.15 or later.".into());
        }
        /* Uncompressed TIFF: quick to write, and ImageIO reads it as is. */
        let mut tiff = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut tiff), image::ImageFormat::Tiff)
            .map_err(|err| err.to_string())?;
        let (width, height) = (image.width() as f64, image.height() as f64);
        autoreleasepool(|_| {
            let data = NSData::with_bytes(&tiff);
            let handler = VNImageRequestHandler::initWithData_options(
                VNImageRequestHandler::alloc(),
                &data,
                &NSDictionary::new(),
            );
            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);
            /* macOS 13 and later; older systems use English. */
            if request.respondsToSelector(sel!(setAutomaticallyDetectsLanguage:)) {
                request.setAutomaticallyDetectsLanguage(true);
            }
            let as_request: Retained<VNRequest> =
                Retained::into_super(Retained::into_super(request.clone()));
            let requests = NSArray::from_retained_slice(&[as_request]);
            handler
                .performRequests_error(&requests)
                .map_err(|err| err.localizedDescription().to_string())?;
            let mut found = Vec::new();
            for observation in request.results().unwrap_or_default().iter() {
                let Some(best) = observation.topCandidates(1).firstObject() else {
                    continue;
                };
                /* Normalized, from the bottom left. */
                let rect = unsafe { observation.boundingBox() };
                found.push(Found {
                    text: best.string().to_string(),
                    x: rect.origin.x * width,
                    y: (1.0 - rect.origin.y - rect.size.height / 2.0) * height,
                    height: rect.size.height * height,
                });
            }
            Ok(found)
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use image::RgbaImage;

    use super::Found;

    pub fn available() -> bool {
        false
    }

    pub fn recognize(_image: &RgbaImage) -> Result<Vec<Found>, String> {
        Err("Reading text is only available on macOS.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slow the first time a new test build runs: that is what it saves.
    #[test]
    #[ignore]
    fn warm_up_picture_has_text_to_read() {
        if !available() {
            return;
        }
        let picture = image::load_from_memory(include_bytes!("../assets/ocr-warmup.png"))
            .unwrap()
            .to_rgba8();
        let found = imp::recognize(&picture).unwrap();
        let text: Vec<_> = found.iter().map(|f| f.text.as_str()).collect();
        assert!(text.join(" ").contains("Lockshot"), "{text:?}");
    }

    fn at(text: &str, x: f64, y: f64) -> Found {
        Found {
            text: text.into(),
            x,
            y,
            height: 20.0,
        }
    }

    #[test]
    fn groups_pieces_into_lines_and_keeps_only_new_rows() {
        let found = vec![
            at("10:42", 300.0, 52.0),
            at("Alice", 40.0, 50.0),
            at("old line", 40.0, 10.0),
            at("hello there", 40.0, 80.0),
            at("next page", 40.0, 130.0),
        ];
        assert_eq!(
            lines(found, &(30..120)),
            vec!["Alice  10:42", "hello there"]
        );
    }

    #[test]
    fn text_joins_the_blocks() {
        let blocks = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string()],
        ];
        assert_eq!(text(&blocks), "a\nb\nc\n");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod vision_tests {
    /// Run with --ignored and LOCKSHOT_OCR_IMAGE set to an image of text.
    #[test]
    #[ignore]
    fn reads_an_image() {
        let path = std::env::var("LOCKSHOT_OCR_IMAGE").expect("LOCKSHOT_OCR_IMAGE");
        let image = image::open(path).unwrap().to_rgba8();
        let mut reader = super::Reader::start(false);
        reader.push(&image);
        eprintln!("{}", super::text(&reader.finish(|_, _| {})));
    }
}

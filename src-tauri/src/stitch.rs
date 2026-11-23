//! Scrolling Capture: join the pages into one long image as they come in.
//!
//! Every scroll is measured (see `scrolling::find_shift`), so the run knows
//! how far the content moved and which rows of the window scroll. The first
//! page goes in down to the bottom of the scrolling pane; each page after it
//! adds only the rows that came into view; the last page adds what sits below
//! the pane, such as a chat app's message box. Rows are kept in memory only
//! until a part is full, so a long run never holds the whole image at once.
//!
//! When scrolling up, the same logic runs on the pages turned upside down,
//! and each finished part is turned back.
//!
//! The rows also go to the text reader (ocr.rs), when the text is wanted.

use std::io::Cursor;
use std::ops::Range;
use std::path::{Path, PathBuf};

use image::{imageops, RgbaImage};

use crate::{export, ocr};

/// Start a new part once this many rows are waiting. Under JPEG's limit of
/// 65,535 even with a tall window's rows on top, and small enough that image
/// viewers open it without trouble.
const PART_ROWS: u32 = 30_000;
const NAME: &str = "Long image";

/// How a scroll moved the content, in rows of the window as captured (top
/// down, whichever way it scrolled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pane {
    /// How far the content moved.
    pub moved: usize,
    /// First and last row of the scrolling pane.
    pub top: usize,
    pub bottom: usize,
}

/// The text read off the long image.
pub struct Text {
    pub text: String,
    /// Nothing was read, or nothing found.
    pub empty: bool,
    /// False when reading was skipped before the end.
    pub whole: bool,
}

pub struct Stitcher {
    up: bool,
    /// Write the image. Off, the rows only go to the text reader.
    image: bool,
    text: Option<ocr::Reader>,
    dir: PathBuf,
    format: String,
    quality: u8,
    /// Last row of content already added, in the newest page, counted in
    /// scroll order (from the bottom when scrolling up). None until the
    /// first page is in.
    edge: Option<usize>,
    /// The last pane measured, in scroll order.
    pane: Option<(usize, usize)>,
    /// Rows waiting for the current part, in scroll order.
    segments: Vec<RgbaImage>,
    rows: u32,
    parts: Vec<PathBuf>,
}

impl Stitcher {
    pub fn new(
        dir: PathBuf,
        format: &str,
        quality: u8,
        up: bool,
        image: bool,
        text: Option<ocr::Reader>,
    ) -> Self {
        /* Only PNG and JPEG suit an image this tall: GIF, HEIC and PDF
        cap the size far lower or bloat it. */
        let format = if matches!(format, "jpg" | "jpeg") {
            "jpg"
        } else {
            "png"
        };
        Self {
            up,
            image,
            text,
            dir,
            format: format.into(),
            quality,
            edge: None,
            pane: None,
            segments: Vec::new(),
            rows: 0,
            parts: Vec::new(),
        }
    }

    /// A scroll from `prev` to `next`. `pane` is None when the content
    /// changed but no overlap was found, so the step may have skipped some;
    /// the whole pane of `next` goes in then.
    pub fn add(
        &mut self,
        prev: &RgbaImage,
        next: &RgbaImage,
        pane: Option<Pane>,
    ) -> Result<(), String> {
        let height = next.height() as usize;
        let pane = pane.map(|p| self.ordered(p, height));
        let edge = match self.edge {
            Some(edge) => edge,
            None => {
                /* The first page, down to the bottom of the pane. */
                let end = pane.map_or(height, |p| p.bottom + 1);
                self.push(prev, 0, end)?;
                end - 1
            }
        };
        match pane {
            Some(p) => {
                self.pane = Some((p.top, p.bottom));
                /* What followed the edge is now `moved` rows higher. */
                let start = (edge + 1).saturating_sub(p.moved);
                let end = p.bottom + 1;
                if end > start {
                    self.push(next, start, end)?;
                    self.edge = Some(end - 1);
                } else {
                    self.edge = Some(edge - p.moved);
                }
            }
            None => {
                let (top, bottom) = self.pane.unwrap_or((0, height - 1));
                self.push(next, top, bottom + 1)?;
                self.edge = Some(bottom);
            }
        }
        Ok(())
    }

    /// The columns of the scrolling pane, for reading text.
    pub fn set_columns(&mut self, columns: Range<u32>) {
        if let Some(text) = self.text.as_mut() {
            text.set_columns(columns);
        }
    }

    /// Add what is left of the last page and write the image. Returns the
    /// files written, and the text when it was read. `stage` hears what it
    /// is doing, such as "Stitching the long image…", for the Saving notice,
    /// and whether that can be skipped (reading the text can). Once
    /// `skip_text` says so, reading stops and the text so far is kept.
    pub fn finish(
        mut self,
        last: &RgbaImage,
        stage: &mut dyn FnMut(String, bool),
        skip_text: &dyn Fn() -> bool,
    ) -> Result<(Vec<PathBuf>, Option<Text>), String> {
        if self.image {
            stage("Stitching the long image…".into(), false);
        }
        let height = last.height() as usize;
        match self.edge {
            None => self.push(last, 0, height)?,
            Some(edge) if edge + 1 < height => self.push(last, edge + 1, height)?,
            Some(_) => {}
        }
        self.flush()?;
        let text = self.text.take().map(|reader| {
            let (blocks, whole) = reader.finish(
                |read, total| {
                    stage(
                        if total > 1 {
                            format!(
                                "Reading the text (OCR), part {} of {total}…",
                                (read + 1).min(total)
                            )
                        } else {
                            "Reading the text (OCR)…".into()
                        },
                        true,
                    )
                },
                skip_text,
            );
            Text {
                empty: blocks.iter().all(Vec::is_empty),
                text: ocr::text(&blocks),
                whole,
            }
        });
        /* Parts were written in scroll order: when scrolling up, the last
        one is the top of the content. */
        if self.up {
            self.parts.reverse();
        }
        let ext = export::extension(&self.format).to_string();
        let count = self.parts.len();
        let mut files = Vec::with_capacity(count);
        for (i, from) in self.parts.iter().enumerate() {
            let name = if count == 1 {
                format!("{NAME}.{ext}")
            } else {
                format!("{NAME} {}.{ext}", i + 1)
            };
            let to = unique_file(&self.dir, &name);
            std::fs::rename(from, &to)
                .map_err(|err| format!("Could not name the long image: {err}"))?;
            files.push(to);
        }
        Ok((files, text))
    }

    /// A pane in scroll order: counted from the bottom when scrolling up.
    fn ordered(&self, pane: Pane, height: usize) -> Pane {
        let bottom = pane.bottom.min(height - 1);
        let top = pane.top.min(bottom);
        if self.up {
            Pane {
                moved: pane.moved,
                top: height - 1 - bottom,
                bottom: height - 1 - top,
            }
        } else {
            Pane {
                moved: pane.moved,
                top,
                bottom,
            }
        }
    }

    /// Add rows `start..end` of `image` (in scroll order) to the current part.
    fn push(&mut self, image: &RgbaImage, start: usize, end: usize) -> Result<(), String> {
        let height = image.height();
        let (start, end) = (start as u32, (end as u32).min(height));
        let rows = if self.up {
            (height - end)..(height - start)
        } else {
            start..end
        };
        if rows.is_empty() || (!self.image && self.text.is_none()) {
            return Ok(());
        }
        let mut segment =
            imageops::crop_imm(image, 0, rows.start, image.width(), rows.len() as u32).to_image();
        if self.up {
            imageops::flip_vertical_in_place(&mut segment);
        }
        if let Some(text) = self.text.as_mut() {
            text.push(&segment);
        }
        if !self.image {
            return Ok(());
        }
        self.rows += segment.height();
        self.segments.push(segment);
        if self.rows >= PART_ROWS {
            self.flush()?;
        }
        Ok(())
    }

    /// Write the waiting rows as the next part.
    fn flush(&mut self) -> Result<(), String> {
        if self.segments.is_empty() {
            return Ok(());
        }
        let width = self.segments[0].width();
        let mut part = RgbaImage::new(width, self.rows);
        let mut y = 0;
        for segment in self.segments.drain(..) {
            imageops::replace(&mut part, &segment, 0, y as i64);
            y += segment.height();
        }
        self.rows = 0;
        if self.up {
            imageops::flip_vertical_in_place(&mut part);
        }
        let mut png = Vec::new();
        part.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|err| format!("Could not encode the long image: {err}"))?;
        let path = self.dir.join(format!(
            ".long-{}.{}",
            self.parts.len() + 1,
            export::extension(&self.format)
        ));
        export::write(&png, &self.format, self.quality, &path)?;
        self.parts.push(path);
        Ok(())
    }
}

fn unique_file(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unused file name")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows `from..from + height` of a tall document: a header row, the
    /// document's own rows, and a footer row, each row filled with its
    /// number so the result can be checked row by row.
    fn window(from: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(4, height, |_, y| {
            if y == 0 {
                image::Rgba([255, 0, 0, 255])
            } else if y == height - 1 {
                image::Rgba([0, 0, 255, 255])
            } else {
                let n = from + y;
                image::Rgba([(n % 256) as u8, (n / 256) as u8, 0, 255])
            }
        })
    }

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lockshot-stitch-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Rows of the stitched file as the numbers window() wrote.
    fn rows_of(path: &Path) -> Vec<i64> {
        let image = image::open(path).unwrap().to_rgba8();
        (0..image.height())
            .map(|y| {
                let p = image.get_pixel(0, y);
                match p.0 {
                    [255, 0, 0, 255] => -1,
                    [0, 0, 255, 255] => -2,
                    [a, b, _, _] => a as i64 + b as i64 * 256,
                }
            })
            .collect()
    }

    #[test]
    fn scrolling_down_adds_each_new_row_once() {
        let out = dir("down");
        let mut s = Stitcher::new(out.clone(), "png", 90, false, true, None);
        /* A 10-row window: header at 0, pane 1..=8, footer at 9. */
        let pages = [window(0, 10), window(5, 10), window(11, 10)];
        let pane = |moved| {
            Some(Pane {
                moved,
                top: 1,
                bottom: 8,
            })
        };
        s.add(&pages[0], &pages[1], pane(5)).unwrap();
        s.add(&pages[1], &pages[2], pane(6)).unwrap();
        let (files, _) = s.finish(&pages[2], &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(files.len(), 1);
        let mut expected = vec![-1];
        expected.extend(1..=19);
        expected.push(-2);
        assert_eq!(rows_of(&files[0]), expected);
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn scrolling_up_builds_the_image_top_down() {
        let out = dir("up");
        let mut s = Stitcher::new(out.clone(), "png", 90, true, true, None);
        /* Content moves down the window as it scrolls up. */
        let pages = [window(20, 10), window(15, 10), window(9, 10)];
        let pane = |moved| {
            Some(Pane {
                moved,
                top: 1,
                bottom: 8,
            })
        };
        s.add(&pages[0], &pages[1], pane(5)).unwrap();
        s.add(&pages[1], &pages[2], pane(6)).unwrap();
        let (files, _) = s.finish(&pages[2], &mut |_, _| {}, &|| false).unwrap();
        let mut expected = vec![-1];
        expected.extend(10..=28);
        expected.push(-2);
        assert_eq!(rows_of(&files[0]), expected);
        std::fs::remove_dir_all(out).unwrap();
    }

    #[test]
    fn one_page_is_saved_whole() {
        let out = dir("one");
        let s = Stitcher::new(out.clone(), "gif", 90, false, true, None);
        let page = window(0, 10);
        let (files, _) = s.finish(&page, &mut |_, _| {}, &|| false).unwrap();
        assert_eq!(files[0].file_name().unwrap(), "Long image.png");
        assert_eq!(rows_of(&files[0]).len(), 10);
        std::fs::remove_dir_all(out).unwrap();
    }
}

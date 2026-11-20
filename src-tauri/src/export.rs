//! Writing an annotated screenshot to disk in the format the user picked.
//! The editor always hands over a lossless PNG of the final image; everything
//! else is converted here.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::codecs::gif::GifEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage, RgbaImage};

pub const FORMATS: [&str; 7] = ["png", "jpg", "heic", "tiff", "gif", "bmp", "pdf"];

/// Formats this machine can write. HEIC needs an OS encoder.
pub fn supported_formats() -> Vec<&'static str> {
    FORMATS
        .iter()
        .copied()
        .filter(|f| *f != "heic" || heic_encoder().is_some())
        .collect()
}

enum HeicEncoder {
    /// macOS ships an HEIC encoder behind `sips`.
    Sips,
    /// libheif's command-line encoder, where installed.
    HeifEnc,
}

fn on_path(binary: &str) -> bool {
    Command::new(binary).arg("--version").output().is_ok()
}

fn heic_encoder() -> Option<HeicEncoder> {
    if cfg!(target_os = "macos") {
        return Some(HeicEncoder::Sips);
    }
    on_path("heif-enc").then_some(HeicEncoder::HeifEnc)
}

pub fn extension(format: &str) -> &str {
    match format {
        "jpg" | "jpeg" => "jpg",
        "tiff" | "tif" => "tiff",
        other => other,
    }
}

/// Drop characters that are not allowed in file names on any of the
/// three platforms, so a template like "{date}/{time}" cannot escape the folder.
pub fn sanitize_file_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "Screenshot".into()
    } else {
        trimmed.to_string()
    }
}

/// `dir/stem.ext`, or `dir/stem-2.ext` and so on when that name is taken.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|p| !p.exists())
        .expect("an unused file name")
}

/// The folder that holds files being dragged out of the editor.
pub fn drag_dir() -> PathBuf {
    std::env::temp_dir().join("lockshot-drag")
}

/**
 * Write the image for a drag-out as `stem.ext` in a fresh folder, so the
 * dropped file keeps the exact export name. Earlier drags are cleared first;
 * a drop target has read or copied its file by the time the next drag begins.
 */
pub fn write_drag_file(png: &[u8], format: &str, quality: u8, stem: &str) -> Result<PathBuf, String> {
    let root = drag_dir();
    let _ = fs::remove_dir_all(&root);
    let path = root
        .join(uuid::Uuid::new_v4().simple().to_string())
        .join(format!("{stem}.{}", extension(format)));
    write(png, format, quality, &path)?;
    Ok(path)
}

/// A small PNG thumbnail of the image to show under the cursor while dragging.
pub fn drag_preview(png: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|err| format!("Could not read the edited image: {err}"))?;
    encode_with(&image.thumbnail(160, 160), ImageFormat::Png)
}

fn flatten(image: &RgbaImage) -> RgbImage {
    /* Formats without alpha get the transparent bits on white. */
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y).0;
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

fn write_err(err: impl std::fmt::Display) -> String {
    format!("Could not write the image: {err}")
}

/// Convert the editor's PNG to `format` and write it to `path`.
pub fn write(png: &[u8], format: &str, quality: u8, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(write_err)?;
    }
    if format == "png" {
        return fs::write(path, png).map_err(write_err);
    }
    let decoded = image::load_from_memory_with_format(png, ImageFormat::Png)
        .map_err(|err| format!("Could not read the edited image: {err}"))?
        .to_rgba8();

    let bytes = match format {
        "jpg" | "jpeg" => {
            let mut out = Cursor::new(Vec::new());
            JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
                .encode_image(&flatten(&decoded))
                .map_err(write_err)?;
            out.into_inner()
        }
        "tiff" | "tif" => encode_with(&DynamicImage::ImageRgba8(decoded), ImageFormat::Tiff)?,
        "bmp" => encode_with(
            &DynamicImage::ImageRgb8(flatten(&decoded)),
            ImageFormat::Bmp,
        )?,
        "gif" => {
            let mut out = Vec::new();
            {
                let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
                encoder
                    .encode(
                        decoded.as_raw(),
                        decoded.width(),
                        decoded.height(),
                        image::ExtendedColorType::Rgba8,
                    )
                    .map_err(write_err)?;
            }
            out
        }
        "pdf" => pdf(&decoded)?,
        "heic" => return write_heic(png, path),
        other => return Err(format!("Lockshot cannot export .{other} files.")),
    };
    fs::write(path, bytes).map_err(write_err)
}

/**
 * Read an image file for editing. HEIC has no Rust decoder, so on macOS it
 * goes through `sips` to a temporary PNG first.
 */
pub fn read_image(path: &Path) -> Result<RgbaImage, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let read_err = |err: &dyn std::fmt::Display| {
        format!(
            "Could not open {}: {err}",
            path.file_name().unwrap_or_default().to_string_lossy()
        )
    };
    if ext == "heic" || ext == "heif" {
        if !cfg!(target_os = "macos") {
            return Err(read_err(&"HEIC images can only be opened on macOS"));
        }
        let temp =
            std::env::temp_dir().join(format!("lockshot-{}.png", uuid::Uuid::new_v4().simple()));
        let output = Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(path)
            .arg("--out")
            .arg(&temp)
            .output()
            .map_err(|err| read_err(&err))?;
        let result = if output.status.success() {
            image::open(&temp)
                .map(|img| img.to_rgba8())
                .map_err(|err| read_err(&err))
        } else {
            Err(read_err(
                &String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ))
        };
        let _ = fs::remove_file(&temp);
        return result;
    }
    image::ImageReader::open(path)
        .map_err(|err| read_err(&err))?
        .with_guessed_format()
        .map_err(|err| read_err(&err))?
        .decode()
        .map(|img| img.to_rgba8())
        .map_err(|err| read_err(&err))
}

fn encode_with(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).map_err(write_err)?;
    Ok(out.into_inner())
}

fn write_heic(png: &[u8], path: &Path) -> Result<(), String> {
    let encoder = heic_encoder().ok_or(
        "HEIC export needs an HEIC encoder. On Linux install libheif (the heif-enc command); \
         on Windows pick another format.",
    )?;
    let temp = std::env::temp_dir().join(format!("lockshot-{}.png", uuid::Uuid::new_v4().simple()));
    fs::write(&temp, png).map_err(write_err)?;
    let output = match encoder {
        HeicEncoder::Sips => Command::new("sips")
            .args(["-s", "format", "heic", "-s", "formatOptions", "high"])
            .arg(&temp)
            .arg("--out")
            .arg(path)
            .output(),
        HeicEncoder::HeifEnc => Command::new("heif-enc")
            .args(["-q", "90", "-o"])
            .arg(path)
            .arg(&temp)
            .output(),
    };
    let _ = fs::remove_file(&temp);
    let output = output.map_err(|err| format!("Could not run the HEIC encoder: {err}"))?;
    if !output.status.success() || !path.exists() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("The HEIC encoder failed. {detail}")
            .trim()
            .to_string());
    }
    Ok(())
}

fn zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).map_err(write_err)?;
    encoder.finish().map_err(write_err)
}

/**
 * A one-page PDF holding the image losslessly (Flate-compressed RGB plus an
 * alpha soft mask when the image has transparency). The page is sized at
 * 144 pixels per inch, which prints a Retina capture at its on-screen size.
 */
pub fn pdf(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let (w, h) = (image.width(), image.height());
    let rgb: Vec<u8> = image.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let has_alpha = image.pixels().any(|p| p[3] != 255);
    let page_w = w as f64 * 72.0 / 144.0;
    let page_h = h as f64 * 72.0 / 144.0;

    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec());
    objects.push(
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w:.2} {page_h:.2}] \
             /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
    );
    let content = format!("q {page_w:.2} 0 0 {page_h:.2} 0 0 cm /Im0 Do Q");
    objects.push(stream(
        &format!("<< /Length {} >>", content.len()),
        content.as_bytes(),
    ));

    let image_data = zlib(&rgb)?;
    let smask = if has_alpha { " /SMask 6 0 R" } else { "" };
    objects.push(stream(
        &format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /FlateDecode /Length {}{smask} >>",
            image_data.len()
        ),
        &image_data,
    ));
    if has_alpha {
        let alpha: Vec<u8> = image.pixels().map(|p| p[3]).collect();
        let alpha_data = zlib(&alpha)?;
        objects.push(stream(
            &format!(
                "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray \
                 /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>",
                alpha_data.len()
            ),
            &alpha_data,
        ));
    }

    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = dict.as_bytes().to_vec();
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png() -> Vec<u8> {
        let image = RgbaImage::from_fn(8, 6, |x, y| {
            image::Rgba([x as u8 * 30, y as u8 * 40, 90, if x == 0 { 0 } else { 255 }])
        });
        encode_with(&DynamicImage::ImageRgba8(image), ImageFormat::Png).unwrap()
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_file_stem("a/b:c"), "a-b-c");
        assert_eq!(sanitize_file_stem("  ..  "), "Screenshot");
        assert_eq!(
            sanitize_file_stem("Screenshot-2026-11-19 20.01.02"),
            "Screenshot-2026-11-19 20.01.02"
        );
    }

    #[test]
    fn writes_every_raster_format() {
        let dir =
            std::env::temp_dir().join(format!("lockshot-test-{}", uuid::Uuid::new_v4().simple()));
        let png = sample_png();
        for format in ["png", "jpg", "tiff", "gif", "bmp", "pdf"] {
            let path = dir.join(format!("shot.{}", extension(format)));
            write(&png, format, 90, &path).unwrap();
            let bytes = fs::read(&path).unwrap();
            assert!(bytes.len() > 20, "{format} is empty");
            if format == "pdf" {
                assert!(bytes.starts_with(b"%PDF-1.4"));
                assert!(bytes.ends_with(b"%%EOF\n"));
            } else {
                let read = image::load_from_memory(&bytes).unwrap();
                assert_eq!((read.width(), read.height()), (8, 6), "{format}");
            }
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_images_back_for_editing() {
        let dir =
            std::env::temp_dir().join(format!("lockshot-test-{}", uuid::Uuid::new_v4().simple()));
        let png = sample_png();
        for format in ["png", "jpg", "bmp", "gif", "tiff"] {
            let path = dir.join(format!("shot.{}", extension(format)));
            write(&png, format, 90, &path).unwrap();
            let image = read_image(&path).unwrap();
            assert_eq!((image.width(), image.height()), (8, 6), "{format}");
        }
        assert!(read_image(&dir.join("missing.png")).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn drag_files_keep_the_export_name() {
        let png = sample_png();
        let path = write_drag_file(&png, "jpg", 90, "My shot").unwrap();
        assert_eq!(path.file_name().unwrap(), "My shot.jpg");
        assert!(path.starts_with(drag_dir()));
        let preview = drag_preview(&png).unwrap();
        assert!(image::load_from_memory(&preview).is_ok());
    }

    #[test]
    fn unique_paths_do_not_overwrite() {
        let dir =
            std::env::temp_dir().join(format!("lockshot-test-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.png"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "a", "png"), dir.join("a-2.png"));
        let _ = fs::remove_dir_all(dir);
    }
}

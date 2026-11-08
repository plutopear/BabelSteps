//! How screenshots are stored on disk (parity item PSR-1).
//!
//! Screens are kept whole and at full resolution: downscaling makes small UI
//! text hard to read, and cropping to a window is a separate choice (A6). The
//! saving comes from the encoding:
//!
//! - Most screens are flat UI: large single-colour areas and sharp text. PNG
//!   keeps them pixel-exact, and with real compression (deflate level 3 and
//!   adaptive filtering) a 5120x1440 UI screen is about 0.5-1.8 MB, against
//!   1.7-4.5 MB for the fast setting used before. JPEG is no smaller on these
//!   and blurs text, so it is not used for them.
//! - Screens that are largely photo-like (wallpaper, photos, video) barely
//!   compress as PNG (7 MB and more at 5120x1440). For those, JPEG at quality
//!   85 without chroma subsampling is 4-5 times smaller and keeps text sharp
//!   enough to read.
//!
//! The choice is made from the pixels before encoding, because the file name
//! (and so its extension) is part of the step before the writer encodes it.
//! Measurements are in research/m1-results.md, 'Screenshot size (PSR-1)'.
//!
//! Neither encoder writes metadata: PNG files hold only IHDR, IDAT and IEND,
//! JPEG files only the JFIF header, tables and image data.

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, ImageFormat, RgbImage};

/// The two formats screenshots are stored in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotFormat {
    Png,
    Jpeg,
}

impl ShotFormat {
    /// File extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ShotFormat::Png => "png",
            ShotFormat::Jpeg => "jpg",
        }
    }

    /// The format a file name says it holds, from its extension.
    pub fn from_name(name: &str) -> Option<ShotFormat> {
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(ShotFormat::Png),
            "jpg" | "jpeg" => Some(ShotFormat::Jpeg),
            _ => None,
        }
    }

    /// The format of encoded bytes, from their signature. Anything else is `None`.
    pub fn sniff(bytes: &[u8]) -> Option<ShotFormat> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ShotFormat::Png)
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(ShotFormat::Jpeg)
        } else {
            None
        }
    }
}

/// JPEG quality for photo-like screens. At 85 with full-resolution colour,
/// 12 px text next to a photo stayed clean when viewed at 2x zoom; 80 was
/// visibly softer.
pub const JPEG_QUALITY: u8 = 85;

/// Share of sampled pixels that must look photo-like before JPEG is used.
/// UI screens measured 0.04-0.07; a screen with photo content over a fifth of
/// its width measured 0.23, where JPEG was under half the PNG size.
pub const PHOTO_SHARE_FOR_JPEG: f64 = 0.20;

/// Rows sampled for [`photo_share`]: every 4th, enough for a stable estimate in
/// a few milliseconds at 5120x1440.
const SAMPLE_ROW_STEP: usize = 4;

/// The largest per-channel step between neighbouring pixels still counted as
/// photo-like. Text and UI edges jump further; flat areas do not change at all.
const SMOOTH_STEP: u8 = 32;

/// How hard the PNG encoder works.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// Deflate level 3 with adaptive filters: about 2.5-4.5x smaller than
    /// `Hurry` on UI screens, for 3-4x the time (about 60-100 ms at 5120x1440).
    Normal,
    /// The fastest setting, for when screenshots are queuing up and memory for
    /// new ones is running short.
    Hurry,
}

/// Share (0.0-1.0) of sampled pixels that differ from their left neighbour by
/// a small, non-zero step, as pixels in photos and gradients do. Flat UI areas
/// do not change from pixel to pixel, and text edges change sharply.
/// `bytes_per_pixel` is 3 or 4; only the first three channels are compared, in
/// any order (RGB or BGRA).
pub fn photo_share(pixels: &[u8], width: u32, height: u32, bytes_per_pixel: usize) -> f64 {
    let (w, h) = (width as usize, height as usize);
    if w < 2 || h == 0 || bytes_per_pixel < 3 || pixels.len() < w * h * bytes_per_pixel {
        return 0.0;
    }
    let stride = w * bytes_per_pixel;
    let (mut smooth, mut total) = (0u64, 0u64);
    for y in (0..h).step_by(SAMPLE_ROW_STEP) {
        let row = &pixels[y * stride..(y + 1) * stride];
        for x in 1..w {
            let (a, b) = (&row[(x - 1) * bytes_per_pixel..], &row[x * bytes_per_pixel..]);
            let d = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
            if d > 0 && d <= SMOOTH_STEP {
                smooth += 1;
            }
            total += 1;
        }
    }
    smooth as f64 / total as f64
}

/// The format to store a screen in. See the module notes.
pub fn choose_format(pixels: &[u8], width: u32, height: u32, bytes_per_pixel: usize) -> ShotFormat {
    if photo_share(pixels, width, height, bytes_per_pixel) > PHOTO_SHARE_FOR_JPEG {
        ShotFormat::Jpeg
    } else {
        ShotFormat::Png
    }
}

fn check_len(rgb: &[u8], width: u32, height: u32) -> Result<(), String> {
    if rgb.len() as u64 != u64::from(width) * u64::from(height) * 3 {
        return Err(format!("{} bytes do not make a {width}x{height} RGB image", rgb.len()));
    }
    Ok(())
}

/// Encode tightly packed RGB as PNG. No metadata chunks are written.
pub fn encode_png(rgb: &[u8], width: u32, height: u32, effort: Effort) -> Result<Vec<u8>, String> {
    check_len(rgb, width, height)?;
    let (compression, filter) = match effort {
        Effort::Normal => (CompressionType::Level(3), FilterType::Adaptive),
        Effort::Hurry => (CompressionType::Fast, FilterType::Adaptive),
    };
    let mut out = Vec::with_capacity(rgb.len() / 8);
    PngEncoder::new_with_quality(&mut out, compression, filter)
        .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Encode tightly packed RGB as JPEG at [`JPEG_QUALITY`], without chroma
/// subsampling. Only the JFIF header is written: no EXIF or comments.
pub fn encode_jpeg(rgb: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    check_len(rgb, width, height)?;
    if width > u32::from(u16::MAX) || height > u32::from(u16::MAX) {
        return Err(format!("{width}x{height} is too large for JPEG"));
    }
    let mut out = Vec::with_capacity(rgb.len() / 16);
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .write_image(rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Encode in the given format, at normal effort for PNG.
pub fn encode(format: ShotFormat, rgb: &[u8], width: u32, height: u32, effort: Effort) -> Result<Vec<u8>, String> {
    match format {
        ShotFormat::Png => encode_png(rgb, width, height, effort),
        ShotFormat::Jpeg => encode_jpeg(rgb, width, height),
    }
}

/// Decode a stored screenshot (PNG or JPEG only) to RGB. Returns the format too.
pub fn decode(bytes: &[u8]) -> Result<(ShotFormat, RgbImage), String> {
    let format = ShotFormat::sniff(bytes).ok_or("not a PNG or JPEG image")?;
    let image_format = match format {
        ShotFormat::Png => ImageFormat::Png,
        ShotFormat::Jpeg => ImageFormat::Jpeg,
    };
    let img = image::load_from_memory_with_format(bytes, image_format).map_err(|e| e.to_string())?;
    if !img.color().has_alpha() {
        return Ok((format, img.to_rgb8()));
    }
    // Images added by hand can be transparent. Our screenshots never are.
    // Lay them on white, as a page would show them.
    let rgba = img.to_rgba8();
    let rgb = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let over_white = |c: u8| ((u16::from(c) * u16::from(a) + 255 * (255 - u16::from(a)) + 127) / 255) as u8;
        image::Rgb([over_white(r), over_white(g), over_white(b)])
    });
    Ok((format, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat UI-like screen: grey background, a white panel, dark "text" bars.
    fn ui(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| {
            if (40..w - 40).contains(&x) && (30..h - 30).contains(&y) {
                if y % 16 < 9 && x % 7 < 5 && (x / 60) % 5 != 4 {
                    image::Rgb([30, 30, 30])
                } else {
                    image::Rgb([255, 255, 255])
                }
            } else {
                image::Rgb([243, 243, 243])
            }
        })
    }

    /// A photo-like area: a gradient with per-pixel noise.
    fn photo(w: u32, h: u32) -> RgbImage {
        let mut s: u32 = 1;
        RgbImage::from_fn(w, h, |x, y| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let n = ((s >> 16) % 21) as i32 - 10;
            let base = [(x * 200 / w) as i32 + 30, (y * 200 / h) as i32 + 20, 120];
            image::Rgb(base.map(|v| (v + n).clamp(0, 255) as u8))
        })
    }

    #[test]
    fn ui_screens_stay_png_and_photo_screens_become_jpeg() {
        let u = ui(800, 400);
        let p = photo(800, 400);
        assert!(photo_share(u.as_raw(), 800, 400, 3) < 0.05);
        assert!(photo_share(p.as_raw(), 800, 400, 3) > 0.8);
        assert_eq!(choose_format(u.as_raw(), 800, 400, 3), ShotFormat::Png);
        assert_eq!(choose_format(p.as_raw(), 800, 400, 3), ShotFormat::Jpeg);
    }

    #[test]
    fn a_small_photo_on_a_ui_screen_stays_png() {
        let mut img = ui(1000, 400);
        let p = photo(100, 400);
        image::imageops::replace(&mut img, &p, 0, 0); // a tenth of the width
        assert_eq!(choose_format(img.as_raw(), 1000, 400, 3), ShotFormat::Png);
        let p = photo(400, 400);
        image::imageops::replace(&mut img, &p, 0, 0); // two fifths
        assert_eq!(choose_format(img.as_raw(), 1000, 400, 3), ShotFormat::Jpeg);
    }

    #[test]
    fn photo_share_reads_bgra_and_ignores_alpha() {
        let p = photo(64, 32);
        let mut bgra = Vec::new();
        for (i, px) in p.pixels().enumerate() {
            bgra.extend_from_slice(&[px[2], px[1], px[0], (i % 251) as u8]);
        }
        let rgb_share = photo_share(p.as_raw(), 64, 32, 3);
        assert!((photo_share(&bgra, 64, 32, 4) - rgb_share).abs() < 1e-9);
        // Alpha noise alone does not make a flat screen photo-like.
        let flat: Vec<u8> = (0..64 * 32).flat_map(|i| [9, 9, 9, (i % 7) as u8]).collect();
        assert_eq!(photo_share(&flat, 64, 32, 4), 0.0);
    }

    #[test]
    fn photo_share_tolerates_bad_input() {
        assert_eq!(photo_share(&[], 0, 0, 3), 0.0);
        assert_eq!(photo_share(&[1, 2, 3], 1, 1, 3), 0.0);
        assert_eq!(photo_share(&[1, 2, 3], 100, 100, 3), 0.0, "short buffer");
        assert_eq!(photo_share(&[0; 12], 2, 2, 2), 0.0, "too few channels");
    }

    #[test]
    fn png_is_lossless_and_smaller_than_hurried() {
        let u = ui(1200, 600);
        let normal = encode_png(u.as_raw(), 1200, 600, Effort::Normal).unwrap();
        let hurry = encode_png(u.as_raw(), 1200, 600, Effort::Hurry).unwrap();
        assert!(normal.len() < hurry.len(), "normal {} vs hurry {}", normal.len(), hurry.len());
        for bytes in [&normal, &hurry] {
            let (f, back) = decode(bytes).unwrap();
            assert_eq!(f, ShotFormat::Png);
            assert_eq!(back.as_raw(), u.as_raw(), "PNG must be pixel-exact");
        }
    }

    #[test]
    fn jpeg_is_much_smaller_on_photos_and_stays_close() {
        let p = photo(800, 400);
        let png = encode_png(p.as_raw(), 800, 400, Effort::Normal).unwrap();
        let jpg = encode_jpeg(p.as_raw(), 800, 400).unwrap();
        assert!(jpg.len() * 2 < png.len(), "jpeg {} vs png {}", jpg.len(), png.len());
        let (f, back) = decode(&jpg).unwrap();
        assert_eq!(f, ShotFormat::Jpeg);
        assert_eq!(back.dimensions(), (800, 400));
        let mean_err: f64 = back.as_raw().iter().zip(p.as_raw()).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>()
            / p.as_raw().len() as f64;
        assert!(mean_err < 8.0, "mean error {mean_err}");
    }

    /// Segment markers of a JPEG up to the image data.
    fn jpeg_markers(b: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 2;
        while i + 4 <= b.len() && b[i] == 0xFF {
            let m = b[i + 1];
            out.push(m);
            if m == 0xDA {
                break;
            }
            i += 2 + (usize::from(b[i + 2]) << 8 | usize::from(b[i + 3]));
        }
        out
    }

    #[test]
    fn jpeg_has_no_metadata_and_full_colour_resolution() {
        let p = photo(64, 32);
        let jpg = encode_jpeg(p.as_raw(), 64, 32).unwrap();
        let markers = jpeg_markers(&jpg);
        assert_eq!(markers.last(), Some(&0xDA), "reached the image data");
        let app: Vec<u8> = markers.iter().copied().filter(|m| (0xE0..=0xEF).contains(m) || *m == 0xFE).collect();
        assert_eq!(app, vec![0xE0], "only the JFIF header: no EXIF, ICC or comment");
        // SOF0: every component is sampled 1x1 (no chroma subsampling).
        let sof = jpg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        let n = usize::from(jpg[sof + 9]);
        assert_eq!(n, 3);
        for c in 0..n {
            assert_eq!(jpg[sof + 11 + c * 3], 0x11, "component {c} sampling");
        }
    }

    #[test]
    fn png_has_only_the_three_essential_chunks() {
        let png = encode_png(ui(50, 40).as_raw(), 50, 40, Effort::Normal).unwrap();
        let names: Vec<String> = crate::testing::png_chunks(&png).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn names_and_signatures() {
        assert_eq!(ShotFormat::from_name("images/shot-1.png"), Some(ShotFormat::Png));
        assert_eq!(ShotFormat::from_name("shot.JPG"), Some(ShotFormat::Jpeg));
        assert_eq!(ShotFormat::from_name("shot.jpeg"), Some(ShotFormat::Jpeg));
        assert_eq!(ShotFormat::from_name("shot.gif"), None);
        assert_eq!(ShotFormat::from_name("png"), None);
        for f in [ShotFormat::Png, ShotFormat::Jpeg] {
            assert_eq!(ShotFormat::from_name(&format!("x.{}", f.extension())), Some(f));
        }
        assert_eq!(decode(b"GIF89a....").unwrap_err(), "not a PNG or JPEG image");
        assert!(decode(b"\x89PNG\r\n\x1a\ntruncated").is_err());
    }

    #[test]
    fn transparent_images_are_laid_on_white() {
        let img = image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([0, 0, 0, 0]) } else { image::Rgba([0, 0, 0, 255]) });
        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(img.as_raw(), 2, 1, image::ExtendedColorType::Rgba8).unwrap();
        let (_, rgb) = decode(&png).unwrap();
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(rgb.get_pixel(1, 0).0, [0, 0, 0]);
    }

    #[test]
    fn wrong_buffer_sizes_are_refused() {
        assert!(encode_png(&[0; 10], 2, 2, Effort::Normal).is_err());
        assert!(encode_jpeg(&[0; 10], 2, 2).is_err());
        assert!(encode_jpeg(&[], 70_000, 0).is_err());
    }
}

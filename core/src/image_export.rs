//! Image preparation for export. Every exported image is decoded, has its
//! redaction boxes painted solid black (never blurred: blur can leave text
//! length readable), and is re-encoded from raw pixels in the format it came
//! in (PNG or JPEG, see [`crate::screenshot`]), so its file name stays right.
//! Re-encoding drops all metadata (EXIF, text chunks, colour profiles): PNG
//! output holds only IHDR, IDAT and IEND chunks, JPEG output only the JFIF
//! header, tables and image data.
//!
//! With `max_width`, wider images are scaled down after the boxes are painted,
//! so scaling only ever blends black into the edges, never hidden pixels.

use crate::export::ExportError;
use crate::model::Rect;
use crate::screenshot::{self, Effort, ShotFormat};
use image::imageops::{resize, FilterType};

/// Width for printed (HTML and PDF) images. Letter pages print about 7.5 inches
/// wide, so this is over 250 dpi; full-size ultrawide shots made printing slow.
pub const PRINT_MAX_WIDTH: u32 = 2000;

pub fn prepare_image(src: &[u8], redactions: &[Rect], max_width: Option<u32>) -> Result<Vec<u8>, ExportError> {
    let (format, img) = paint_and_scale(src, redactions, max_width)?;
    let (width, height) = img.dimensions();
    screenshot::encode(format, img.as_raw(), width, height, Effort::Normal).map_err(ExportError::Image)
}

/// For printed exports (HTML, PDF), which are the files people email: like
/// [`prepare_image`], but encoded as the smaller of JPEG and PNG (256 colours when the
/// palette keeps the picture, as on UI screens; full colour otherwise), so the file can
/// change format. This is what brings a typical 30-step PDF under 10 MB
/// (research/m1-results.md, 'Export size target').
pub fn prepare_print_image(src: &[u8], redactions: &[Rect], max_width: Option<u32>) -> Result<(ShotFormat, Vec<u8>), ExportError> {
    let (_, img) = paint_and_scale(src, redactions, max_width)?;
    let (width, height) = img.dimensions();
    let jpg = screenshot::encode_jpeg(img.as_raw(), width, height).map_err(ExportError::Image)?;
    // Photo-like pictures: JPEG wins by far, so the PNG encoders are skipped
    // (each costs 0.1-0.3 s at print size, and D5 allows 10 s for 100 steps).
    if screenshot::choose_format(img.as_raw(), width, height, 3) == ShotFormat::Jpeg {
        return Ok((ShotFormat::Jpeg, jpg));
    }
    // A palette PNG, when accepted, is almost always smaller than full colour.
    let png = match screenshot::encode_palette_png(img.as_raw(), width, height).map_err(ExportError::Image)? {
        Some(p) => p,
        None => screenshot::encode_png(img.as_raw(), width, height, Effort::Normal).map_err(ExportError::Image)?,
    };
    Ok(if jpg.len() < png.len() { (ShotFormat::Jpeg, jpg) } else { (ShotFormat::Png, png) })
}

/// Decode, paint the redaction boxes black, then shrink to `max_width`.
fn paint_and_scale(src: &[u8], redactions: &[Rect], max_width: Option<u32>) -> Result<(ShotFormat, image::RgbImage), ExportError> {
    let (format, mut img) = screenshot::decode(src).map_err(ExportError::Image)?;
    let (width, height) = img.dimensions();

    for r in redactions {
        // i64 math: x + w can overflow i32 for hostile input.
        let x0 = i64::from(r.x).clamp(0, i64::from(width));
        let y0 = i64::from(r.y).clamp(0, i64::from(height));
        let x1 = (i64::from(r.x) + i64::from(r.w)).clamp(0, i64::from(width));
        let y1 = (i64::from(r.y) + i64::from(r.h)).clamp(0, i64::from(height));
        if r.w <= 0 || r.h <= 0 || x0 >= x1 || y0 >= y1 {
            continue;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                img.put_pixel(x as u32, y as u32, image::Rgb([0, 0, 0]));
            }
        }
    }

    if let Some(max) = max_width.filter(|&m| m > 0 && width > m) {
        let h = ((u64::from(height) * u64::from(max) + u64::from(width) / 2) / u64::from(width)).max(1) as u32;
        img = resize(&img, max, h, FilterType::CatmullRom);
    }
    Ok((format, img))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::{png_chunks, png_with_extra_chunks, solid_png};
    use image::codecs::png::PngEncoder;
    use image::{ImageEncoder, ImageFormat};

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(png, ImageFormat::Png).unwrap().to_rgba8()
    }

    #[test]
    fn paints_black_inside_and_leaves_outside_alone() {
        let src = solid_png(20, 10, [200, 100, 50, 255]);
        let out = decode(&prepare_image(&src, &[Rect { x: 5, y: 2, w: 4, h: 3 }], None).unwrap());
        for y in 0..10 {
            for x in 0..20 {
                let inside = (5..9).contains(&x) && (2..5).contains(&y);
                let want = if inside { [0, 0, 0, 255] } else { [200, 100, 50, 255] };
                assert_eq!(out.get_pixel(x, y).0, want, "pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn hostile_rects_do_not_panic_and_are_clamped() {
        let src = solid_png(10, 10, [9, 9, 9, 255]);
        let rects = [
            Rect { x: -5, y: -5, w: 8, h: 8 },
            Rect { x: 8, y: 8, w: 1000, h: 1000 },
            Rect { x: i32::MAX, y: i32::MAX, w: i32::MAX, h: i32::MAX },
            Rect { x: i32::MIN, y: i32::MIN, w: i32::MAX, h: i32::MAX },
            Rect { x: 0, y: 0, w: -4, h: 4 },
            Rect { x: 0, y: 0, w: 0, h: 0 },
            Rect { x: 100, y: 100, w: 5, h: 5 },
        ];
        let out = decode(&prepare_image(&src, &rects, None).unwrap());
        // (-5,-5,8,8) covers 0..3; negative width is ignored.
        assert_eq!(out.get_pixel(2, 2).0, [0, 0, 0, 255]);
        assert_eq!(out.get_pixel(9, 9).0, [0, 0, 0, 255]);
        assert_eq!(out.get_pixel(5, 5).0, [9, 9, 9, 255]);
    }

    #[test]
    fn output_has_only_critical_chunks() {
        let src = png_with_extra_chunks(&solid_png(6, 6, [1, 2, 3, 255]));
        let src_names: Vec<String> = png_chunks(&src).into_iter().map(|(n, _)| n).collect();
        for want in ["tEXt", "iTXt", "eXIf"] {
            assert!(src_names.iter().any(|n| n == want), "test input must contain {want}");
        }
        let out = prepare_image(&src, &[], None).unwrap();
        for (name, data) in png_chunks(&out) {
            assert!(["IHDR", "IDAT", "IEND", "PLTE"].contains(&name.as_str()), "unexpected chunk {name}");
            assert!(!String::from_utf8_lossy(&data).contains("SECRET-META"));
        }
        assert!(!out.windows(11).any(|w| w == b"SECRET-META"));
    }

    #[test]
    fn wide_images_shrink_to_max_width_keeping_aspect() {
        let out = decode(&prepare_image(&solid_png(500, 141, [7, 7, 7, 255]), &[], Some(100)).unwrap());
        assert_eq!(out.dimensions(), (100, 28));
        let out = decode(&prepare_image(&solid_png(80, 30, [7, 7, 7, 255]), &[], Some(100)).unwrap());
        assert_eq!(out.dimensions(), (80, 30), "narrow images are never enlarged");
    }

    #[test]
    fn scaling_never_blends_redacted_pixels_into_the_output() {
        // White page with a red "secret" block that is redacted exactly. Every
        // output pixel must be grey (black blended with white), never reddish.
        let mut src = image::RgbaImage::from_pixel(400, 100, image::Rgba([255, 255, 255, 255]));
        for y in 30..60 {
            for x in 101..203 {
                src.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let mut png = Vec::new();
        PngEncoder::new(&mut png).write_image(src.as_raw(), 400, 100, image::ExtendedColorType::Rgba8).unwrap();
        let redact = [Rect { x: 101, y: 30, w: 102, h: 30 }];
        let out = decode(&prepare_image(&png, &redact, Some(150)).unwrap());
        assert_eq!(out.width(), 150);
        let mut saw_black = false;
        for p in out.pixels() {
            let [r, g, b, _] = p.0;
            assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "coloured pixel {:?}", p.0);
            saw_black |= r < 10;
        }
        assert!(saw_black, "the redaction box must survive scaling");
        // The check can fail: without the redaction the red shows through.
        let leaked = decode(&prepare_image(&png, &[], Some(150)).unwrap());
        assert!(leaked.pixels().any(|p| u16::from(p.0[0]) > u16::from(p.0[1]) + 50));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(prepare_image(b"not a png", &[], None), Err(ExportError::Image(_))));
    }

    /// A JPEG of a flat colour with an EXIF segment carrying a marker string.
    fn jpeg_with_exif(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
        let plain = screenshot::encode_jpeg(img.as_raw(), w, h).unwrap();
        let payload = b"Exif\0\0SECRET-META";
        let len = (payload.len() + 2) as u16;
        let mut out = plain[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
        out.extend_from_slice(&plain[2..]);
        out
    }

    #[test]
    fn jpeg_stays_jpeg_loses_metadata_and_gets_black_boxes() {
        let src = jpeg_with_exif(64, 32, [200, 100, 50]);
        assert!(src.windows(11).any(|w| w == b"SECRET-META"), "test input must carry the marker");
        let out = prepare_image(&src, &[Rect { x: 16, y: 8, w: 24, h: 16 }], None).unwrap();
        assert_eq!(screenshot::ShotFormat::sniff(&out), Some(screenshot::ShotFormat::Jpeg));
        assert!(!out.windows(11).any(|w| w == b"SECRET-META"), "EXIF must be dropped");
        assert!(!out.windows(4).any(|w| w == b"Exif"));
        let img = image::load_from_memory_with_format(&out, ImageFormat::Jpeg).unwrap().to_rgb8();
        // Lossy: the box interior is near black, the far corner near the source colour.
        let [r, g, b] = img.get_pixel(28, 16).0;
        assert!(r < 12 && g < 12 && b < 12, "inside the box: {r},{g},{b}");
        let [r, g, b] = img.get_pixel(2, 2).0;
        assert!(r.abs_diff(200) < 12 && g.abs_diff(100) < 12 && b.abs_diff(50) < 12, "outside: {r},{g},{b}");
    }

    #[test]
    fn jpeg_shrinks_for_print_like_png() {
        let src = jpeg_with_exif(500, 141, [7, 7, 7]);
        let out = prepare_image(&src, &[], Some(100)).unwrap();
        let img = image::load_from_memory_with_format(&out, ImageFormat::Jpeg).unwrap();
        assert_eq!((img.width(), img.height()), (100, 28));
    }

    #[test]
    fn png_output_is_rgb_and_compressed() {
        // RGBA input with an opaque alpha channel comes out as 8-bit RGB.
        let out = prepare_image(&solid_png(300, 200, [1, 2, 3, 255]), &[], None).unwrap();
        let (_, ihdr) = png_chunks(&out).into_iter().next().unwrap();
        assert_eq!(ihdr[9], 2, "colour type 2 (RGB)");
        assert!(out.len() < 2000, "a flat 300x200 image compresses well: {} bytes", out.len());
    }

    #[test]
    fn print_images_take_the_smaller_format_and_keep_black_boxes() {
        // Flat colour: PNG is tiny, so it stays PNG.
        let (f, out) = prepare_print_image(&solid_png(300, 200, [1, 2, 3, 255]), &[], None).unwrap();
        assert_eq!(f, ShotFormat::Png);
        assert_eq!(ShotFormat::sniff(&out), Some(ShotFormat::Png));
        // Busy pattern: JPEG wins, and the box is still black after scaling.
        let mut seed: u32 = 5;
        let noisy = image::RgbaImage::from_fn(800, 400, |x, y| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let n = ((seed >> 16) % 31) as u8;
            image::Rgba([100 + n + (x / 8) as u8, 120 + n, 90 + n + (y / 8) as u8, 255])
        });
        let mut src = Vec::new();
        PngEncoder::new(&mut src).write_image(noisy.as_raw(), 800, 400, image::ExtendedColorType::Rgba8).unwrap();
        let (f, out) = prepare_print_image(&src, &[Rect { x: 0, y: 0, w: 400, h: 400 }], Some(400)).unwrap();
        assert_eq!(f, ShotFormat::Jpeg);
        assert_eq!(ShotFormat::sniff(&out), Some(f));
        let img = image::load_from_memory_with_format(&out, ImageFormat::Jpeg).unwrap().to_rgb8();
        assert_eq!(img.dimensions(), (400, 200));
        let [r, g, b] = img.get_pixel(80, 100).0;
        assert!(r < 12 && g < 12 && b < 12, "inside the box: {r},{g},{b}");
        assert!(!out.windows(4).any(|w| w == b"Exif"));
    }
}

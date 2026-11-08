//! Image preparation for export. Every exported image is decoded, has its
//! redaction boxes painted solid black (never blurred: blur can leave text
//! length readable), and is re-encoded from raw pixels in the format it came
//! in (PNG or JPEG, see [`crate::screenshot`]), so its file name stays right.
//! Re-encoding drops all metadata (EXIF, text chunks, colour profiles): PNG
//! output holds only IHDR, IDAT and IEND chunks, JPEG output only the JFIF
//! header, tables and image data.
//!
//! With a [`Shrink`], wider images are scaled down after the boxes are painted,
//! so scaling only ever blends black into the edges, never hidden pixels.

use crate::export::ExportError;
use crate::model::Rect;
use crate::screenshot::{self, Effort};
use image::imageops::{resize, FilterType};

/// When an image is scaled down: only if wider than `above`, and then to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shrink {
    pub above: u32,
    pub to: u32,
}

/// Size of printed (HTML and PDF) images. WebView2 re-compresses every PDF
/// image itself, so pixel count is what sets the PDF size (PSR-1, 2026-11-08).
/// Wide shots (ultrawide, 2560 px and up) go to 1600 px, over 200 dpi across a
/// Letter page: 2000 px gave about 11 MB per 30 real steps against the 10 MB
/// target. Images up to 2000 px wide are printed at their own size, because a
/// mild shrink (1920 to 1600) blurred sharp edges into in-between colours and
/// made the PDF self-test's 1920x540 export 9 times larger (145 MB vs 16 MB).
pub const PRINT_SHRINK: Shrink = Shrink { above: 2000, to: 1600 };

pub fn prepare_image(src: &[u8], redactions: &[Rect], shrink: Option<Shrink>) -> Result<Vec<u8>, ExportError> {
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

    if let Some(max) = shrink.filter(|s| s.to > 0 && width > s.above.max(s.to)).map(|s| s.to) {
        let h = ((u64::from(height) * u64::from(max) + u64::from(width) / 2) / u64::from(width)).max(1) as u32;
        img = resize(&img, max, h, FilterType::CatmullRom);
    }
    let (width, height) = img.dimensions();
    // Print images (HTML, PDF) use the fastest PNG setting. WebView2 decodes
    // every image and re-compresses the pixels itself when it writes the PDF
    // (measured 2026-11-08: no JPEG survived into the PDF), so our compression
    // does not change the PDF size, only the export time (D5: 10 s per 100
    // steps). Markdown exports are kept as files, so they are compressed.
    let effort = if shrink.is_some() { Effort::Hurry } else { Effort::Normal };
    screenshot::encode(format, img.as_raw(), width, height, effort).map_err(ExportError::Image)
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
        let out = decode(&prepare_image(&solid_png(500, 141, [7, 7, 7, 255]), &[], Some(Shrink { above: 100, to: 100 })).unwrap());
        assert_eq!(out.dimensions(), (100, 28));
        let out = decode(&prepare_image(&solid_png(80, 30, [7, 7, 7, 255]), &[], Some(Shrink { above: 100, to: 100 })).unwrap());
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
        let out = decode(&prepare_image(&png, &redact, Some(Shrink { above: 150, to: 150 })).unwrap());
        assert_eq!(out.width(), 150);
        let mut saw_black = false;
        for p in out.pixels() {
            let [r, g, b, _] = p.0;
            assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "coloured pixel {:?}", p.0);
            saw_black |= r < 10;
        }
        assert!(saw_black, "the redaction box must survive scaling");
        // The check can fail: without the redaction the red shows through.
        let leaked = decode(&prepare_image(&png, &[], Some(Shrink { above: 150, to: 150 })).unwrap());
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
        let out = prepare_image(&src, &[], Some(Shrink { above: 100, to: 100 })).unwrap();
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
    fn print_shrink_leaves_images_up_to_the_limit_alone() {
        let w = |src_w: u32| decode(&prepare_image(&solid_png(src_w, 20, [7, 7, 7, 255]), &[], Some(PRINT_SHRINK)).unwrap()).width();
        assert_eq!(w(1920), 1920, "1080p screens are not resampled");
        assert_eq!(w(2000), 2000);
        assert_eq!(w(2001), 1600);
        assert_eq!(w(5120), 1600);
    }
}

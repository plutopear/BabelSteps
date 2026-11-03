//! Image preparation for export. Every exported image is decoded, has its
//! redaction boxes painted solid black (never blurred: blur can leave text
//! length readable), and is re-encoded from raw pixels. Re-encoding drops all
//! metadata (EXIF, text chunks, colour profiles), so the output holds only
//! IHDR, IDAT and IEND chunks.

use crate::export::ExportError;
use crate::model::Rect;
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, ImageFormat};

pub fn prepare_image(src_png: &[u8], redactions: &[Rect]) -> Result<Vec<u8>, ExportError> {
    let decoded = image::load_from_memory_with_format(src_png, ImageFormat::Png)
        .map_err(|e| ExportError::Image(e.to_string()))?;
    let mut img = decoded.to_rgba8();
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
                img.put_pixel(x as u32, y as u32, image::Rgba([0, 0, 0, 255]));
            }
        }
    }

    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| ExportError::Image(e.to_string()))?;
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::{png_chunks, png_with_extra_chunks, solid_png};

    fn decode(png: &[u8]) -> image::RgbaImage {
        image::load_from_memory_with_format(png, ImageFormat::Png).unwrap().to_rgba8()
    }

    #[test]
    fn paints_black_inside_and_leaves_outside_alone() {
        let src = solid_png(20, 10, [200, 100, 50, 255]);
        let out = decode(&prepare_image(&src, &[Rect { x: 5, y: 2, w: 4, h: 3 }]).unwrap());
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
        let out = decode(&prepare_image(&src, &rects).unwrap());
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
        let out = prepare_image(&src, &[]).unwrap();
        for (name, data) in png_chunks(&out) {
            assert!(["IHDR", "IDAT", "IEND", "PLTE"].contains(&name.as_str()), "unexpected chunk {name}");
            assert!(!String::from_utf8_lossy(&data).contains("SECRET-META"));
        }
        assert!(!out.windows(11).any(|w| w == b"SECRET-META"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(prepare_image(b"not a png", &[]), Err(ExportError::Image(_))));
    }
}

//! CPU frame type and the image operations done before a frame is written:
//! password black-out, element outline, click marker, PNG encoding.

use anyhow::Result;
use babelsteps_core::Rect;

/// Top-down BGRA pixels with `stride = width * 4`. `origin` is in virtual-desktop pixels.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub bgra: Vec<u8>,
}

/// Frames hold screen content, so `Debug` shows only the geometry.
impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{} at {},{})", self.width, self.height, self.origin_x, self.origin_y)
    }
}

impl Frame {
    /// RGB of a pixel given in virtual-desktop coordinates.
    #[cfg_attr(not(feature = "selftest"), allow(dead_code))]
    pub fn sample(&self, sx: i32, sy: i32) -> Option<[u8; 3]> {
        let (x, y) = (sx - self.origin_x, sy - self.origin_y);
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]])
    }

    /// A screen rectangle clipped to this frame, in frame pixels: (x0, y0, x1, y1).
    fn clip(&self, r: &Rect) -> Option<(usize, usize, usize, usize)> {
        let x0 = (i64::from(r.x) - i64::from(self.origin_x)).clamp(0, i64::from(self.width));
        let y0 = (i64::from(r.y) - i64::from(self.origin_y)).clamp(0, i64::from(self.height));
        let x1 = (i64::from(r.x) + i64::from(r.w) - i64::from(self.origin_x)).clamp(0, i64::from(self.width));
        let y1 = (i64::from(r.y) + i64::from(r.h) - i64::from(self.origin_y)).clamp(0, i64::from(self.height));
        (r.w > 0 && r.h > 0 && x0 < x1 && y0 < y1).then_some((x0 as usize, y0 as usize, x1 as usize, y1 as usize))
    }

    /// Whether a screen rectangle overlaps this frame at all.
    #[cfg_attr(not(feature = "selftest"), allow(dead_code))]
    pub fn overlaps(&self, r: &Rect) -> bool {
        self.clip(r).is_some()
    }

    /// Keep only the part of the frame inside a screen rectangle (A6: one
    /// window). `None` if the rectangle misses the frame. The cut-away pixels
    /// are dropped with the old buffer, before anything is written.
    pub fn crop(self, r: &Rect) -> Option<Frame> {
        let (x0, y0, x1, y1) = self.clip(r)?;
        let (w, h) = (x1 - x0, y1 - y0);
        if (w, h) == (self.width as usize, self.height as usize) {
            return Some(self);
        }
        let stride = self.width as usize * 4;
        let mut bgra = Vec::with_capacity(w * h * 4);
        for y in y0..y1 {
            bgra.extend_from_slice(&self.bgra[y * stride + x0 * 4..y * stride + x1 * 4]);
        }
        Some(Frame {
            width: w as u32,
            height: h as u32,
            origin_x: self.origin_x + x0 as i32,
            origin_y: self.origin_y + y0 as i32,
            bgra,
        })
    }

    /// Paint a screen rectangle solid black. The original pixels are overwritten
    /// in place, so they can never reach the disk.
    pub fn black_out(&mut self, r: &Rect) {
        let Some((x0, y0, x1, y1)) = self.clip(r) else { return };
        let w = self.width as usize;
        for y in y0..y1 {
            for px in self.bgra[(y * w + x0) * 4..(y * w + x1) * 4].chunks_exact_mut(4) {
                px.copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }

    fn put(&mut self, x: i64, y: i64, bgr: [u8; 3]) {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.bgra[i..i + 4].copy_from_slice(&[bgr[0], bgr[1], bgr[2], 255]);
    }

    /// A green outline around the clicked element (A2), 3 px wide, drawn just outside it.
    pub fn outline(&mut self, r: &Rect) {
        const GREEN: [u8; 3] = [0, 200, 0];
        let (ox, oy) = (i64::from(self.origin_x), i64::from(self.origin_y));
        let (x0, y0) = (i64::from(r.x) - ox - 3, i64::from(r.y) - oy - 3);
        let (x1, y1) = (x0 + i64::from(r.w) + 6, y0 + i64::from(r.h) + 6);
        if r.w <= 0 || r.h <= 0 || x1 - x0 > 20_000 || y1 - y0 > 20_000 {
            return;
        }
        for t in 0..3 {
            for x in x0..x1 {
                self.put(x, y0 + t, GREEN);
                self.put(x, y1 - 1 - t, GREEN);
            }
            for y in y0..y1 {
                self.put(x0 + t, y, GREEN);
                self.put(x1 - 1 - t, y, GREEN);
            }
        }
    }

    /// A red ring around a virtual-desktop point. Our own click marker (A2): Steps
    /// Recorder marks only the element, with a green rectangle.
    pub fn draw_click_marker(&mut self, sx: i32, sy: i32) {
        let (cx, cy) = (i64::from(sx - self.origin_x), i64::from(sy - self.origin_y));
        const R_OUT: i64 = 20;
        const R_IN: i64 = 16;
        for dy in -R_OUT..=R_OUT {
            for dx in -R_OUT..=R_OUT {
                let d2 = dx * dx + dy * dy;
                if d2 <= R_OUT * R_OUT && d2 >= R_IN * R_IN {
                    self.put(cx + dx, cy + dy, [0, 0, 255]);
                }
            }
        }
    }

    fn to_rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(self.width as usize * self.height as usize * 3);
        for px in self.bgra.chunks_exact(4) {
            rgb.extend_from_slice(&[px[2], px[1], px[0]]);
        }
        rgb
    }

    /// Encode as RGB PNG. Only IHDR, IDAT and IEND are written: no metadata.
    pub fn encode_png(&self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.bgra.len() / 8);
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut writer = enc.write_header()?;
            writer.write_image_data(&self.to_rgb())?;
            writer.finish()?;
        }
        Ok(out)
    }

    /// Read a PNG written by `encode_png` back from disk, so checks test the real file.
    #[cfg(feature = "selftest")]
    pub fn load_png(path: &std::path::Path, origin_x: i32, origin_y: i32) -> Result<Frame> {
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
        let mut reader = decoder.read_info()?;
        let size = reader.output_buffer_size().ok_or_else(|| anyhow::anyhow!("PNG too large"))?;
        let mut buf = vec![0u8; size];
        let info = reader.next_frame(&mut buf)?;
        let mut bgra = Vec::with_capacity(info.width as usize * info.height as usize * 4);
        for px in buf[..info.buffer_size()].chunks_exact(3) {
            bgra.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
        Ok(Frame { width: info.width, height: info.height, origin_x, origin_y, bgra })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, ox: i32, oy: i32) -> Frame {
        Frame { width: w, height: h, origin_x: ox, origin_y: oy, bgra: vec![200; (w * h * 4) as usize] }
    }

    #[test]
    fn black_out_uses_screen_coordinates_and_clips() {
        // A frame for a monitor left of the primary one, at x = -100.
        let mut f = solid(100, 50, -100, 0);
        f.black_out(&Rect { x: -10, y: 40, w: 1000, h: 1000 });
        assert_eq!(f.sample(-10, 40), Some([0, 0, 0]));
        assert_eq!(f.sample(-1, 49), Some([0, 0, 0]));
        assert_eq!(f.sample(-11, 40), Some([200, 200, 200]));
        assert_eq!(f.sample(-10, 39), Some([200, 200, 200]));
    }

    #[test]
    fn crop_keeps_screen_coordinates() {
        // A frame for a monitor left of the primary one, with one marked pixel.
        let mut f = solid(100, 50, -100, 0);
        f.black_out(&Rect { x: -50, y: 20, w: 1, h: 1 });
        let c = f.crop(&Rect { x: -60, y: 10, w: 1000, h: 20 }).unwrap();
        assert_eq!((c.width, c.height, c.origin_x, c.origin_y), (60, 20, -60, 10));
        assert_eq!(c.bgra.len(), 60 * 20 * 4);
        assert_eq!(c.sample(-50, 20), Some([0, 0, 0]));
        assert_eq!(c.sample(-51, 20), Some([200, 200, 200]));
        assert_eq!(c.sample(-61, 20), None, "cut away");
        // Missing the frame, or empty: no frame at all.
        assert!(solid(10, 10, 0, 0).crop(&Rect { x: 10, y: 0, w: 5, h: 5 }).is_none());
        assert!(solid(10, 10, 0, 0).crop(&Rect { x: 2, y: 2, w: 0, h: 5 }).is_none());
        assert!(solid(10, 10, 0, 0).crop(&Rect { x: i32::MAX, y: i32::MAX, w: i32::MAX, h: i32::MAX }).is_none());
    }

    #[test]
    fn hostile_rects_do_nothing_and_do_not_panic() {
        let mut f = solid(10, 10, 0, 0);
        for r in [
            Rect { x: i32::MAX, y: i32::MAX, w: i32::MAX, h: i32::MAX },
            Rect { x: i32::MIN, y: i32::MIN, w: i32::MAX, h: 1 },
            Rect { x: 2, y: 2, w: -5, h: 5 },
            Rect { x: 50, y: 50, w: 5, h: 5 },
        ] {
            f.black_out(&r);
            f.outline(&r);
        }
        assert!(f.bgra.chunks_exact(4).all(|p| p[0] == 200), "nothing inside the frame was touched");
        assert!(!f.overlaps(&Rect { x: 50, y: 50, w: 5, h: 5 }));
    }

    #[test]
    fn png_round_trip_keeps_black_box() {
        let mut f = solid(20, 10, 0, 0);
        f.black_out(&Rect { x: 5, y: 2, w: 4, h: 3 });
        let png = f.encode_png().unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        let at = |x: usize, y: usize| &buf[(y * 20 + x) * 3..(y * 20 + x) * 3 + 3];
        assert_eq!(at(5, 2), [0, 0, 0]);
        assert_eq!(at(8, 4), [0, 0, 0]);
        assert_eq!(at(9, 4), [200, 200, 200]);
    }
}

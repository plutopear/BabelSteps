//! CPU frame type and small image operations: click marker, redaction, PNG encoding.

use anyhow::Result;
use std::io::Write;

/// Top-down BGRA pixels with `stride = width * 4`. `origin` is in virtual-desktop pixels.
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub origin_x: i32,
    pub origin_y: i32,
    pub bgra: Vec<u8>,
}

impl Frame {
    /// RGB of a pixel given in virtual-desktop coordinates.
    pub fn sample(&self, sx: i32, sy: i32) -> Option<[u8; 3]> {
        let (x, y) = (sx - self.origin_x, sy - self.origin_y);
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some([self.bgra[i + 2], self.bgra[i + 1], self.bgra[i]])
    }

    fn put(&mut self, x: i32, y: i32, bgr: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.bgra[i] = bgr[0];
        self.bgra[i + 1] = bgr[1];
        self.bgra[i + 2] = bgr[2];
        self.bgra[i + 3] = 255;
    }

    /// Draw a red ring around a virtual-desktop point (Steps Recorder's click marker).
    pub fn draw_click_marker(&mut self, sx: i32, sy: i32) {
        let (cx, cy) = (sx - self.origin_x, sy - self.origin_y);
        const R_OUT: i32 = 20;
        const R_IN: i32 = 16;
        for dy in -R_OUT..=R_OUT {
            for dx in -R_OUT..=R_OUT {
                let d2 = dx * dx + dy * dy;
                if d2 <= R_OUT * R_OUT && d2 >= R_IN * R_IN {
                    self.put(cx + dx, cy + dy, [0, 0, 255]);
                }
            }
        }
    }

    /// Black out a rectangle given in virtual-desktop coordinates. The original
    /// pixels are overwritten, so they can never reach the disk.
    pub fn black_out(&mut self, sx: i32, sy: i32, w: i32, h: i32) {
        let (x0, y0) = (sx - self.origin_x, sy - self.origin_y);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                self.put(x, y, [0, 0, 0]);
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

    /// Lossless WebP, for the storage-size comparison.
    pub fn encode_webp(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        image_webp::WebPEncoder::new(&mut out).encode(
            &self.to_rgb(),
            self.width,
            self.height,
            image_webp::ColorType::Rgb8,
        )?;
        Ok(out)
    }

    /// Copy of a rectangle given in virtual-desktop coordinates, clipped to the frame.
    pub fn crop(&self, sx: i32, sy: i32, w: i32, h: i32) -> Frame {
        let x0 = (sx - self.origin_x).clamp(0, self.width as i32);
        let y0 = (sy - self.origin_y).clamp(0, self.height as i32);
        let x1 = (sx - self.origin_x + w).clamp(0, self.width as i32);
        let y1 = (sy - self.origin_y + h).clamp(0, self.height as i32);
        let (cw, ch) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut bgra = Vec::with_capacity(cw * ch * 4);
        for y in y0 as usize..y1 as usize {
            let start = (y * self.width as usize + x0 as usize) * 4;
            bgra.extend_from_slice(&self.bgra[start..start + cw * 4]);
        }
        Frame { width: cw as u32, height: ch as u32, origin_x: sx, origin_y: sy, bgra }
    }

    /// Read a PNG written by `encode_png` back from disk, so checks test the real file.
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

    /// Encode as PNG, returning the bytes.
    pub fn encode_png(&self, level: png::Compression) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.bgra.len() / 8);
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(level);
            let mut writer = enc.write_header()?;
            writer.write_image_data(&self.to_rgb())?;
            writer.finish()?;
        }
        out.flush()?;
        Ok(out)
    }
}

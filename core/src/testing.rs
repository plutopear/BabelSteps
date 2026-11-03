//! Test support: PNG builders and inspectors, a self-cleaning temp folder, and
//! a generator for recordings of N steps with real PNG screenshots (for the
//! later 100-step performance test). Nothing here runs unless a test calls it.

use crate::model::*;
use image::codecs::png::PngEncoder;
use image::{ImageEncoder, RgbaImage};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A temp folder removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "babelsteps-{label}-{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn encode(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .expect("encode png");
    out
}

/// A PNG filled with one colour.
pub fn solid_png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    encode(&RgbaImage::from_pixel(w, h, image::Rgba(rgba)))
}

/// A PNG with a cheap per-pixel pattern (so it does not compress to nothing).
pub fn pattern_png(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let v = x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)).wrapping_add(seed.wrapping_mul(7));
        // Never pure black, so black pixels in an export always come from redaction.
        image::Rgba([(v & 0xff) as u8 | 1, ((v >> 3) & 0xff) as u8 | 1, ((v >> 5) & 0xff) as u8 | 1, 255])
    });
    encode(&img)
}

/// Split a PNG into `(chunk name, data)` pairs, checking the signature.
pub fn png_chunks(png: &[u8]) -> Vec<(String, Vec<u8>)> {
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let mut i = 8;
    let mut v = Vec::new();
    while i < png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        let name = String::from_utf8_lossy(&png[i + 4..i + 8]).to_string();
        v.push((name, png[i + 8..i + 8 + len].to_vec()));
        i += 12 + len;
    }
    v
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(name: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = Vec::new();
    c.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = name.to_vec();
    body.extend_from_slice(data);
    c.extend_from_slice(&body);
    c.extend_from_slice(&crc32(&body).to_be_bytes());
    c
}

/// Insert tEXt, iTXt and eXIf chunks (each containing `SECRET-META`) right
/// after IHDR, with valid CRCs.
pub fn png_with_extra_chunks(png: &[u8]) -> Vec<u8> {
    let ihdr_end = 8 + 12 + 13;
    let mut out = png[..ihdr_end].to_vec();
    out.extend(chunk(b"tEXt", b"Comment\0SECRET-META"));
    out.extend(chunk(b"iTXt", b"Comment\0\0\0\0\0SECRET-META"));
    out.extend(chunk(b"eXIf", b"MM\0*SECRET-META"));
    out.extend_from_slice(&png[ihdr_end..]);
    out
}

/// Write a bundle of `n` click steps, each with a `w` x `h` PNG screenshot at
/// `images/step-NNN.png` under `dir`, and return the recording. Each step has
/// one redaction box. Keep `n`, `w`, `h` small in normal test runs.
pub fn generate_bundle(dir: &Path, n: usize, w: u32, h: u32) -> Recording {
    std::fs::create_dir_all(dir.join("images")).expect("create images dir");
    let mut rec = Recording::new("generated", "Generated recording", 0);
    for i in 1..=n {
        let rel = format!("images/step-{i:03}.png");
        std::fs::write(dir.join(&rel), pattern_png(w, h, i as u32)).expect("write png");
        rec.blocks.push(Block::Step(StepBlock {
            id: format!("s{i}"),
            action: Action::Click { button: MouseButton::Left, x: 1, y: 1 },
            title: format!("Left-click at (1, {i})"),
            description: String::new(),
            screenshot: Some(rel),
            target: None,
            redactions: vec![Rect { x: (w / 4) as i32, y: (h / 4) as i32, w: (w / 2) as i32, h: (h / 2) as i32 }],
            captured_unix_ms: i as u64,
            screenshot_withheld: false,
        }));
    }
    rec
}

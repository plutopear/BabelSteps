//! GDI `BitBlt` capture: the fallback path, and the comparison for "capture on demand".

use crate::imgops::Frame;
use anyhow::{bail, Result};
use windows::Win32::Graphics::Gdi::*;

pub fn capture_rect(x: i32, y: i32, w: i32, h: i32) -> Result<Frame> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            bail!("GetDC failed");
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));

        let result = (|| -> Result<Frame> {
            BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT)?;
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = w;
            info.bmiHeader.biHeight = -h; // Negative means top-down.
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;
            let mut bgra = vec![0u8; w as usize * h as usize * 4];
            let lines = GetDIBits(
                mem,
                bmp,
                0,
                h as u32,
                Some(bgra.as_mut_ptr() as *mut _),
                &mut info,
                DIB_RGB_COLORS,
            );
            if lines == 0 {
                bail!("GetDIBits failed");
            }
            Ok(Frame { width: w as u32, height: h as u32, origin_x: x, origin_y: y, bgra })
        })();

        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

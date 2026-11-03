//! Monitor enumeration in physical pixels (the process is per-monitor DPI aware).

use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

const MONITORINFOF_PRIMARY_BIT: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Monitor {
    #[serde(skip)]
    pub handle: isize,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn hmonitor(&self) -> HMONITOR {
        HMONITOR(self.handle as *mut _)
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

unsafe extern "system" fn enum_cb(hmon: HMONITOR, _hdc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<Monitor>);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(hmon, &mut info).as_bool() {
        let r = info.rcMonitor;
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        out.push(Monitor {
            handle: hmon.0 as isize,
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
            dpi: dx,
            primary: info.dwFlags & MONITORINFOF_PRIMARY_BIT != 0,
        });
    }
    BOOL(1)
}

pub fn enumerate() -> Vec<Monitor> {
    let mut out: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(enum_cb), LPARAM(&mut out as *mut _ as isize));
    }
    out
}

/// Index of the monitor containing a point, falling back to the first monitor.
pub fn index_for(monitors: &[Monitor], x: i32, y: i32) -> usize {
    monitors.iter().position(|m| m.contains(x, y)).unwrap_or(0)
}

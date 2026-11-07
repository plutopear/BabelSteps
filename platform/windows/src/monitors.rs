//! Monitors in physical pixels, and the DPI awareness that makes them physical.
//!
//! Hook coordinates, UI Automation bounds and capture frames only agree when the
//! thread is per-monitor DPI aware. If they disagree, a password field's black
//! box lands in the wrong place, so recording refuses to start without it.

use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetDpiForMonitor, GetThreadDpiAwarenessContext, SetThreadDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};

const MONITORINFOF_PRIMARY_BIT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
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

/// Index of the monitor containing a point.
pub fn index_for(monitors: &[Monitor], x: i32, y: i32) -> Option<usize> {
    monitors.iter().position(|m| m.contains(x, y))
}

/// Make the calling thread per-monitor DPI aware (V2, or V1 on older systems).
/// Every thread that handles coordinates calls this first.
pub fn make_thread_dpi_aware() {
    unsafe {
        if SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).0.is_null() {
            let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
        }
    }
}

/// True if the calling thread sees physical pixels.
pub fn per_monitor_dpi_aware() -> bool {
    unsafe {
        let ctx = GetThreadDpiAwarenessContext();
        AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).as_bool()
            || AreDpiAwarenessContextsEqual(ctx, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE).as_bool()
    }
}

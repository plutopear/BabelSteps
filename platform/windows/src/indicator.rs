//! Recording cue on the capture target (C2): a thin coloured frame around every
//! monitor while recording (red), or paused (amber).
//!
//! Safety rules for a window that sits on top of everything being recorded:
//! - It is excluded from screen capture (`WDA_EXCLUDEFROMCAPTURE`), so it never
//!   appears in a screenshot. If Windows refuses that, no frame is shown at all
//!   and `start` returns an error: a cue that could end up in the document is
//!   worse than none. (The tray icon still shows that recording is on.)
//! - Every pixel is click-through (layered + transparent) and it never takes focus,
//!   so it cannot swallow a click or move the keyboard focus.
//! - It is only a frame (a window region with the middle cut out), so it covers
//!   a few pixels at the screen edge and nothing else.
//!
//! One frame window per monitor, placed in virtual-desktop pixels (monitors left
//! of or above the primary have negative coordinates). The thread is per-monitor
//! DPI aware, so sizes are physical pixels and the thickness scales per monitor.
//! When monitors are plugged in, removed or rearranged (`WM_DISPLAYCHANGE`), the
//! frames are rebuilt for the new layout.
//!
//! Multi-monitor behaviour is reviewed and its geometry is unit tested, but it
//! has not been run on real multi-monitor hardware.

use crate::monitors;
use anyhow::{anyhow, Result};
use crossbeam_channel::bounded;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::{self, JoinHandle};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CombineRgn, CreateRectRgn, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, SetWindowRgn, PAINTSTRUCT,
    RGN_DIFF,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, PostThreadMessageW,
    PeekMessageW, RegisterClassExW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage,
    HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, SWP_NOACTIVATE, WINDOW_EX_STYLE, WM_DISPLAYCHANGE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_ERASEBKGND, WM_PAINT, WM_QUIT, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const MSG_SET_PAUSED: u32 = WM_APP + 1;
const MSG_REBUILD: u32 = WM_APP + 2;
/// Frame thickness at 96 DPI; scaled for the monitor.
const THICKNESS_96: i32 = 4;
/// COLORREF is 0x00BBGGRR. Matches the tray dots.
const RED: u32 = 0x001E1EDC;
const AMBER: u32 = 0x0000A0E6;

/// Where the frame's hole is, inside a window of `w` by `h` pixels on a monitor
/// at `dpi`: (thickness, left, top, right, bottom). On a monitor too small to
/// have a hole the whole window is the frame.
pub(crate) fn frame_geometry(w: i32, h: i32, dpi: u32) -> (i32, i32, i32, i32, i32) {
    let t = (THICKNESS_96 * dpi.max(96) as i32 / 96).max(THICKNESS_96);
    if w <= 2 * t || h <= 2 * t {
        return (t, 0, 0, 0, 0);
    }
    (t, t, t, w - t, h - t)
}

static COLOR: AtomicU32 = AtomicU32::new(RED);

pub struct Indicator {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Indicator {
    /// Show the frame on every monitor. Errors if it cannot be kept out of screenshots.
    pub fn start() -> Result<Indicator> {
        Self::start_with(true, true)
    }

    /// Self-test control only: the frame WITHOUT the capture exclusion, to prove
    /// the test can see a frame that is not excluded.
    #[cfg(feature = "selftest")]
    pub fn start_unexcluded() -> Result<Indicator> {
        Self::start_with(false, true)
    }

    /// Self-test control only: a frame that is NOT click-through, to prove the
    /// edge-click check can see a frame swallow clicks.
    #[cfg(feature = "selftest")]
    pub fn start_opaque_to_clicks() -> Result<Indicator> {
        Self::start_with(true, false)
    }

    fn start_with(exclude: bool, click_through: bool) -> Result<Indicator> {
        COLOR.store(RED, Ordering::Relaxed);
        let (tx, rx) = bounded::<Result<u32, String>>(1);
        let join = thread::Builder::new().name("capture-cue".into()).spawn(move || unsafe { run(tx, exclude, click_through) })?;
        match rx.recv() {
            Ok(Ok(thread_id)) => Ok(Indicator { thread_id, join: Some(join) }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(anyhow!(e))
            }
            Err(_) => Err(anyhow!("the recording cue thread exited early")),
        }
    }

    pub fn set_paused(&self, paused: bool) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, MSG_SET_PAUSED, WPARAM(paused as usize), LPARAM(0));
        }
    }
}

impl Drop for Indicator {
    fn drop(&mut self) {
        if let Some(j) = self.join.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = j.join();
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut rc = Default::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let brush = CreateSolidBrush(COLORREF(COLOR.load(Ordering::Relaxed)));
            FillRect(dc, &rc, brush);
            let _ = DeleteObject(brush.into());
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // Monitors came, went or moved: the thread rebuilds the frames.
            let _ = PostThreadMessageW(GetCurrentThreadId(), MSG_REBUILD, WPARAM(0), LPARAM(0));
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// One frame window per monitor. On error nothing is left on screen.
unsafe fn create_frames(
    class: windows::core::PCWSTR,
    inst: HINSTANCE,
    exclude: bool,
    click_through: bool,
) -> Result<Vec<HWND>, String> {
    let mut windows: Vec<HWND> = Vec::new();
    for m in monitors::enumerate() {
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED
                | if click_through { WS_EX_TRANSPARENT } else { WINDOW_EX_STYLE(0) }
                | WS_EX_TOPMOST
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            class,
            w!(""),
            WS_POPUP,
            m.x,
            m.y,
            m.w,
            m.h,
            None,
            None,
            Some(inst),
            None,
        ) {
            Ok(h) => h,
            Err(_) => continue,
        };
        // Out of screenshots first; only then can it be shown.
        if exclude && SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_err() {
            let _ = DestroyWindow(hwnd);
            for h in windows.drain(..) {
                let _ = DestroyWindow(h);
            }
            return Err("this version of Windows cannot keep the recording frame out of screenshots, so it is not shown".into());
        }
        // Pin the exact rectangle: a monitor with a different scale from the
        // primary can otherwise resize a window when it is first placed.
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), m.x, m.y, m.w, m.h, SWP_NOACTIVATE);
        let (_t, l, tp, r, b) = frame_geometry(m.w, m.h, m.dpi);
        let outer = CreateRectRgn(0, 0, m.w, m.h);
        if r > l && b > tp {
            let inner = CreateRectRgn(l, tp, r, b);
            CombineRgn(Some(outer), Some(outer), Some(inner), RGN_DIFF);
            let _ = DeleteObject(inner.into());
        }
        // The window owns `outer` from here on.
        SetWindowRgn(hwnd, Some(outer), true);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        windows.push(hwnd);
    }
    if windows.is_empty() {
        return Err("no monitor to draw the recording frame on".into());
    }
    Ok(windows)
}

unsafe fn run(ready: crossbeam_channel::Sender<Result<u32, String>>, exclude: bool, click_through: bool) {
    monitors::make_thread_dpi_aware();
    let fail = |m: &str| {
        let _ = ready.send(Err(m.to_string()));
    };
    let inst = match GetModuleHandleW(None) {
        Ok(h) => HINSTANCE(h.0),
        Err(_) => return fail("no module handle"),
    };
    let class = w!("BabelStepsRecordingCue");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: inst,
        lpszClassName: class,
        ..Default::default()
    };
    // A second registration (a later recording) returns 0 with "class exists"; that is fine.
    let _ = RegisterClassExW(&wc);

    let mut windows = match create_frames(class, inst, exclude, click_through) {
        Ok(w) => w,
        Err(e) => return fail(&e),
    };
    let _ = ready.send(Ok(GetCurrentThreadId()));

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        if msg.message == MSG_SET_PAUSED {
            COLOR.store(if msg.wParam.0 != 0 { AMBER } else { RED }, Ordering::Relaxed);
            for h in &windows {
                let _ = InvalidateRect(Some(*h), None, false);
            }
            continue;
        }
        if msg.message == MSG_REBUILD {
            // Drop any further queued rebuilds (one display change sends several).
            let mut extra = MSG::default();
            while PeekMessageW(&mut extra, None, MSG_REBUILD, MSG_REBUILD, PM_REMOVE).as_bool() {}
            for h in windows.drain(..) {
                let _ = DestroyWindow(h);
            }
            // If the exclusion is refused now, no frame is shown at all.
            windows = create_frames(class, inst, exclude, click_through).unwrap_or_default();
            continue;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    for h in windows {
        let _ = DestroyWindow(h);
    }
}

#[cfg(test)]
mod tests {
    use super::frame_geometry;

    #[test]
    fn thickness_scales_with_dpi_and_never_drops_below_four() {
        assert_eq!(frame_geometry(1920, 1080, 96).0, 4);
        assert_eq!(frame_geometry(1920, 1080, 144).0, 6);
        assert_eq!(frame_geometry(3840, 2160, 192).0, 8);
        assert_eq!(frame_geometry(1920, 1080, 0).0, 4, "a bad DPI reading falls back to 96");
    }

    #[test]
    fn the_hole_leaves_exactly_the_frame_on_every_edge() {
        let (t, l, tp, r, b) = frame_geometry(2560, 1440, 120);
        assert_eq!((l, tp, r, b), (t, t, 2560 - t, 1440 - t));
    }

    #[test]
    fn a_tiny_monitor_is_all_frame_not_a_negative_hole() {
        let (_, l, tp, r, b) = frame_geometry(6, 6, 96);
        assert!(r <= l || b <= tp, "no hole");
    }
}

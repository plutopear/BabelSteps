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
//! Monitors are read once at start; a monitor plugged in during a recording gets
//! no frame until the next recording.

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
    RegisterClassExW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, ShowWindow, TranslateMessage,
    LWA_ALPHA, MSG, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_APP, WM_ERASEBKGND, WM_PAINT, WM_QUIT, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const MSG_SET_PAUSED: u32 = WM_APP + 1;
/// Frame thickness at 96 DPI; scaled for the monitor.
const THICKNESS_96: i32 = 4;
/// COLORREF is 0x00BBGGRR. Matches the tray dots.
const RED: u32 = 0x001E1EDC;
const AMBER: u32 = 0x0000A0E6;

static COLOR: AtomicU32 = AtomicU32::new(RED);

pub struct Indicator {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Indicator {
    /// Show the frame on every monitor. Errors if it cannot be kept out of screenshots.
    pub fn start() -> Result<Indicator> {
        COLOR.store(RED, Ordering::Relaxed);
        let (tx, rx) = bounded::<Result<u32, String>>(1);
        let join = thread::Builder::new().name("capture-cue".into()).spawn(move || unsafe { run(tx) })?;
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
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn run(ready: crossbeam_channel::Sender<Result<u32, String>>) {
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

    let mut windows: Vec<HWND> = Vec::new();
    for m in monitors::enumerate() {
        let hwnd = match CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
        if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_err() {
            let _ = DestroyWindow(hwnd);
            for h in windows.drain(..) {
                let _ = DestroyWindow(h);
            }
            return fail("this version of Windows cannot keep the recording frame out of screenshots, so it is not shown");
        }
        let t = (THICKNESS_96 * m.dpi.max(96) as i32 / 96).max(THICKNESS_96);
        let outer = CreateRectRgn(0, 0, m.w, m.h);
        let inner = CreateRectRgn(t, t, m.w - t, m.h - t);
        CombineRgn(Some(outer), Some(outer), Some(inner), RGN_DIFF);
        let _ = DeleteObject(inner.into());
        // The window owns `outer` from here on.
        SetWindowRgn(hwnd, Some(outer), true);
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        windows.push(hwnd);
    }
    if windows.is_empty() {
        return fail("no monitor to draw the recording frame on");
    }
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
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    for h in windows {
        let _ = DestroyWindow(h);
    }
}

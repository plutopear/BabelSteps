//! Self-test only: a small window owned by the test, so synthetic input only ever
//! lands on our own controls. Its background turns from green to red on a click,
//! which shows whether a screenshot holds the state before the click.
//!
//! Safety, because this runs on a PC someone may be using:
//! - every click is one `SendInput` call that also puts the cursor back;
//! - a click is sent only if this window is what is under the point;
//! - a key is sent only while this window has keyboard focus.

use anyhow::{anyhow, Result};
use crossbeam_channel::bounded;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static BG_RED: AtomicBool = AtomicBool::new(false);
/// Clicks do not take keyboard focus unless the test is about to type.
static ALLOW_ACTIVATE: AtomicBool = AtomicBool::new(false);
/// The next background click minimizes the window, as some apps hide on a click.
static MINIMIZE_ON_CLICK: AtomicBool = AtomicBool::new(false);

const ID_USER: i32 = 101;
const ID_PASS: i32 = 102;
const ID_SAVE: i32 = 103;
const BN_CLICKED_CODE: usize = 0;

/// Client-area points with nothing drawn over the background.
const PROBE_CLIENT: (i32, i32) = (20, 210);
const BACKGROUND_CLIENT: (i32, i32) = (300, 210);

pub const GREEN: [u8; 3] = [0, 160, 0];
pub const RED: [u8; 3] = [220, 0, 0];

fn colorref(c: [u8; 3]) -> COLORREF {
    COLORREF(c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(if ALLOW_ACTIVATE.load(Ordering::Relaxed) { MA_ACTIVATE } else { MA_NOACTIVATE } as isize),
        WM_ERASEBKGND => {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let color = if BG_RED.load(Ordering::Relaxed) { RED } else { GREEN };
            let brush = CreateSolidBrush(colorref(color));
            FillRect(HDC(wparam.0 as *mut _), &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            LRESULT(1)
        }
        // A click on the bare background reacts on the press and repaints at once:
        // the hardest case for "show the state before the click".
        WM_LBUTTONDOWN => {
            BG_RED.store(true, Ordering::Relaxed);
            let _ = InvalidateRect(Some(hwnd), None, true);
            let _ = UpdateWindow(hwnd);
            if MINIMIZE_ON_CLICK.swap(false, Ordering::Relaxed) {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            let code = (wparam.0 >> 16) & 0xffff;
            if id == ID_SAVE && code == BN_CLICKED_CODE {
                BG_RED.store(true, Ordering::Relaxed);
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub struct TestWindow {
    hwnd: isize,
    join: Option<thread::JoinHandle<()>>,
}

fn root_at(x: i32, y: i32) -> isize {
    unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT).0 as isize }
}

impl TestWindow {
    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    pub fn spawn() -> Result<Self> {
        let (tx, rx) = bounded::<std::result::Result<isize, String>>(1);
        let join = thread::Builder::new().name("test-window".into()).spawn(move || unsafe {
            crate::monitors::make_thread_dpi_aware();
            match create_window() {
                Ok(hwnd) => {
                    let _ = tx.send(Ok(hwnd.0 as isize));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("{e:#}")));
                }
            }
        })?;
        let hwnd = rx.recv()?.map_err(|e| anyhow!(e))?;
        Ok(Self { hwnd, join: Some(join) })
    }

    fn control_rect(&self, id: i32) -> Result<RECT> {
        unsafe {
            let child = GetDlgItem(Some(self.handle()), id)?;
            let mut r = RECT::default();
            GetWindowRect(child, &mut r)?;
            Ok(r)
        }
    }

    fn center(r: RECT) -> (i32, i32) {
        ((r.left + r.right) / 2, (r.top + r.bottom) / 2)
    }

    /// Screen rectangle of a control as (x, y, w, h).
    fn xywh(r: RECT) -> (i32, i32, i32, i32) {
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }

    pub fn user_center(&self) -> Result<(i32, i32)> {
        Ok(Self::center(self.control_rect(ID_USER)?))
    }
    pub fn pass_center(&self) -> Result<(i32, i32)> {
        Ok(Self::center(self.control_rect(ID_PASS)?))
    }
    pub fn save_center(&self) -> Result<(i32, i32)> {
        Ok(Self::center(self.control_rect(ID_SAVE)?))
    }
    pub fn user_rect(&self) -> Result<(i32, i32, i32, i32)> {
        Ok(Self::xywh(self.control_rect(ID_USER)?))
    }
    pub fn pass_rect(&self) -> Result<(i32, i32, i32, i32)> {
        Ok(Self::xywh(self.control_rect(ID_PASS)?))
    }

    /// Screen rectangle of the client area (inside the frame) as (x, y, w, h).
    /// Crops use this: the outer rectangle includes a margin that shows
    /// whatever is behind the window.
    pub fn client_rect(&self) -> Result<(i32, i32, i32, i32)> {
        let mut rc = RECT::default();
        unsafe { GetClientRect(self.handle(), &mut rc)? };
        let (x, y) = self.client_point((0, 0))?;
        Ok((x, y, rc.right, rc.bottom))
    }

    fn client_point(&self, p: (i32, i32)) -> Result<(i32, i32)> {
        let mut pt = POINT { x: p.0, y: p.1 };
        if !unsafe { ClientToScreen(self.handle(), &mut pt) }.as_bool() {
            return Err(anyhow!("ClientToScreen failed"));
        }
        Ok((pt.x, pt.y))
    }

    pub fn background_point(&self) -> Result<(i32, i32)> {
        self.client_point(BACKGROUND_CLIENT)
    }
    pub fn probe_point(&self) -> Result<(i32, i32)> {
        self.client_point(PROBE_CLIENT)
    }

    /// Let later clicks give this window keyboard focus. Needed before typing.
    pub fn allow_focus(&self, allow: bool) {
        ALLOW_ACTIVATE.store(allow, Ordering::Relaxed);
    }

    pub fn has_focus(&self) -> bool {
        unsafe { GetForegroundWindow() }.0 as isize == self.hwnd
    }

    /// Click only if this window is what is actually under the point.
    pub fn safe_click(&self, x: i32, y: i32) -> Result<()> {
        for _ in 0..50 {
            if root_at(x, y) == self.hwnd {
                click(x, y);
                return Ok(());
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
        Err(anyhow!("the test window is covered at ({x}, {y}); not clicking"))
    }

    /// Type lowercase letters and digits as real key presses (virtual keys, the
    /// path a physical keyboard takes), only while this window has focus.
    pub fn safe_type_keys(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            if !(ch.is_ascii_lowercase() || ch.is_ascii_digit()) {
                return Err(anyhow!("safe_type_keys only types a-z and 0-9"));
            }
            if !self.has_focus() {
                return Err(anyhow!("the test window lost focus; stopped typing"));
            }
            let vk = VIRTUAL_KEY(ch.to_ascii_uppercase() as u16);
            let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } },
            };
            unsafe {
                SendInput(&[key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)], std::mem::size_of::<INPUT>() as i32);
            }
            thread::sleep(std::time::Duration::from_millis(15));
        }
        Ok(())
    }

    pub fn set_red(&self, red: bool) {
        BG_RED.store(red, Ordering::Relaxed);
        unsafe {
            let _ = InvalidateRect(Some(self.handle()), None, true);
        }
    }

    pub fn is_red(&self) -> bool {
        BG_RED.load(Ordering::Relaxed)
    }

    pub fn minimize_on_next_click(&self) {
        MINIMIZE_ON_CLICK.store(true, Ordering::Relaxed);
    }

    pub fn is_minimized(&self) -> bool {
        unsafe { IsIconic(self.handle()) }.as_bool()
    }

    /// Bring the window back without giving it focus.
    pub fn restore(&self) {
        unsafe {
            let _ = ShowWindow(self.handle(), SW_SHOWNOACTIVATE);
        }
    }

    /// True if nothing else covers the client area now (sampled on a grid), so
    /// a crop of it cannot show another window.
    pub fn unobscured(&self) -> bool {
        let Ok((x, y, w, h)) = self.client_rect() else { return false };
        (1..=3).all(|i| (1..=3).all(|j| root_at(x + w * i / 4, y + h * j / 4) == self.hwnd))
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(self.handle()), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

unsafe fn create_window() -> Result<HWND> {
    let inst = HINSTANCE(GetModuleHandleW(None)?.0);
    let class = w!("BabelStepsCaptureSelfTest");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: inst,
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    if RegisterClassExW(&wc) == 0 {
        return Err(anyhow!("RegisterClassExW failed"));
    }
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST,
        class,
        w!("BabelSteps capture self-test"),
        WS_OVERLAPPED | WS_CAPTION | WS_VISIBLE,
        200,
        200,
        520,
        300,
        None,
        None,
        Some(inst),
        None,
    )?;
    let child = |class: PCWSTR, text: PCWSTR, style: WINDOW_STYLE, x, y, cx, cy, id: i32| {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            cx,
            cy,
            Some(hwnd),
            Some(HMENU(id as isize as *mut _)),
            Some(inst),
            None,
        )
    };
    child(w!("STATIC"), w!("User name:"), WINDOW_STYLE(0), 10, 20, 100, 24, 0)?;
    child(w!("EDIT"), w!(""), WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 120, 20, 300, 24, ID_USER)?;
    child(w!("STATIC"), w!("Password:"), WINDOW_STYLE(0), 10, 60, 100, 24, 0)?;
    child(
        w!("EDIT"),
        w!(""),
        WS_BORDER | WINDOW_STYLE(ES_PASSWORD as u32) | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        120,
        60,
        300,
        24,
        ID_PASS,
    )?;
    child(w!("BUTTON"), w!("Save"), WINDOW_STYLE(BS_PUSHBUTTON as u32), 120, 100, 100, 30, ID_SAVE)?;
    let _ = UpdateWindow(hwnd);
    Ok(hwnd)
}

/// Move, press, release and move back as one `SendInput` call, so real mouse
/// movement cannot redirect the click and the cursor returns at once.
fn click(x: i32, y: i32) {
    unsafe {
        let mut original = POINT::default();
        let _ = GetCursorPos(&mut original);
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2);
        let nx = |px: i32| (((px - vx) as i64 * 65535 + (vw as i64 - 1) / 2) / (vw as i64 - 1)) as i32;
        let ny = |py: i32| (((py - vy) as i64 * 65535 + (vh as i64 - 1) / 2) / (vh as i64 - 1)) as i32;
        let event = |dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, dwFlags: flags, ..Default::default() } },
        };
        let absolute = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        let inputs = [
            event(nx(x), ny(y), absolute),
            event(0, 0, MOUSEEVENTF_LEFTDOWN),
            event(0, 0, MOUSEEVENTF_LEFTUP),
            event(nx(original.x), ny(original.y), absolute),
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// F24 presses and releases. F24 types nothing and is on no normal keyboard, so
/// it loads the keyboard hook without affecting any application.
pub fn f24_burst(presses: usize) {
    let mut inputs = Vec::with_capacity(presses * 2);
    for _ in 0..presses {
        for flags in [KEYBD_EVENT_FLAGS(0), KEYEVENTF_KEYUP] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_F24, dwFlags: flags, ..Default::default() } },
            });
        }
    }
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

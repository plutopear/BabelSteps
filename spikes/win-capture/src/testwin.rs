//! A small window owned by the self-test, so synthetic clicks only ever land on
//! our own controls. Its background turns from green to red when "Save" is clicked,
//! which lets the test see whether a screenshot shows the state before the click.

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

const ID_USER: isize = 101;
const ID_PASS: isize = 102;
const ID_SAVE: isize = 103;
const BN_CLICKED_CODE: usize = 0;

/// Client-area point with nothing drawn over the background.
const PROBE_CLIENT: (i32, i32) = (20, 210);

pub const GREEN: [u8; 3] = [0, 160, 0];
pub const RED: [u8; 3] = [220, 0, 0];

fn colorref(c: [u8; 3]) -> COLORREF {
    COLORREF(c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => {
            if ALLOW_ACTIVATE.load(Ordering::Relaxed) {
                LRESULT(MA_ACTIVATE as isize)
            } else {
                LRESULT(MA_NOACTIVATE as isize)
            }
        }
        WM_ERASEBKGND => {
            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);
            let color = if BG_RED.load(Ordering::Relaxed) { RED } else { GREEN };
            let brush = CreateSolidBrush(colorref(color));
            FillRect(HDC(wparam.0 as *mut _), &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
            LRESULT(1)
        }
        // A click on the bare background reacts on the press, then repaints at once.
        // This is the hardest case for "show the state before the click".
        WM_LBUTTONDOWN => {
            BG_RED.store(true, Ordering::Relaxed);
            let _ = InvalidateRect(Some(hwnd), None, true);
            let _ = UpdateWindow(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as isize;
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

impl TestWindow {
    fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    pub fn spawn() -> Result<Self> {
        let (tx, rx) = bounded::<Result<isize, String>>(1);
        let join = thread::Builder::new().name("test-window".into()).spawn(move || unsafe {
            let result = create_window();
            match result {
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

    /// Screen coordinates of the control's center.
    fn control_center(&self, id: isize) -> Result<(i32, i32)> {
        unsafe {
            let child = GetDlgItem(Some(self.handle()), id as i32)?;
            let mut r = RECT::default();
            GetWindowRect(child, &mut r)?;
            Ok(((r.left + r.right) / 2, (r.top + r.bottom) / 2))
        }
    }

    pub fn user_center(&self) -> Result<(i32, i32)> {
        self.control_center(ID_USER)
    }
    pub fn pass_center(&self) -> Result<(i32, i32)> {
        self.control_center(ID_PASS)
    }
    pub fn save_center(&self) -> Result<(i32, i32)> {
        self.control_center(ID_SAVE)
    }

    /// Screen rectangle of the whole window, as (x, y, w, h).
    pub fn rect(&self) -> Result<(i32, i32, i32, i32)> {
        unsafe {
            let mut r = RECT::default();
            GetWindowRect(self.handle(), &mut r)?;
            Ok((r.left, r.top, r.right - r.left, r.bottom - r.top))
        }
    }

    /// Screen coordinates of a bare background point, away from the probe point.
    pub fn background_point(&self) -> Result<(i32, i32)> {
        unsafe {
            let mut p = POINT { x: 300, y: 210 };
            if !ClientToScreen(self.handle(), &mut p).as_bool() {
                return Err(anyhow!("ClientToScreen failed"));
            }
            Ok((p.x, p.y))
        }
    }

    /// Screen coordinates of the background probe point.
    pub fn probe_point(&self) -> Result<(i32, i32)> {
        unsafe {
            let mut p = POINT { x: PROBE_CLIENT.0, y: PROBE_CLIENT.1 };
            if !ClientToScreen(self.handle(), &mut p).as_bool() {
                return Err(anyhow!("ClientToScreen failed"));
            }
            Ok((p.x, p.y))
        }
    }

    /// Let later clicks give this window keyboard focus. Needed before typing.
    pub fn allow_focus(&self, allow: bool) {
        ALLOW_ACTIVATE.store(allow, Ordering::Relaxed);
    }

    /// Click only if this window is what is actually under the point, so a click can
    /// never land in another application (for example a picture-in-picture video).
    pub fn safe_click(&self, x: i32, y: i32) -> Result<()> {
        for _ in 0..50 {
            let under = crate::uia::root_at(x, y);
            if under.map(|h| h.0 as isize) == Some(self.hwnd) {
                click(x, y);
                return Ok(());
            }
            thread_sleep(100);
        }
        Err(anyhow!("test window is covered at ({x}, {y}); not clicking"))
    }

    /// Right-click, with the same protection as `safe_click`.
    pub fn safe_right_click(&self, x: i32, y: i32) -> Result<()> {
        if crate::uia::root_at(x, y).map(|h| h.0 as isize) != Some(self.hwnd) {
            return Err(anyhow!("test window is covered at ({x}, {y}); not clicking"));
        }
        click_with(x, y, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP);
        Ok(())
    }

    /// Press Ctrl+A, only while this window has keyboard focus.
    pub fn safe_ctrl_a(&self) -> Result<()> {
        if unsafe { GetForegroundWindow() }.0 as isize != self.hwnd {
            return Err(anyhow!("test window lost focus; not sending Ctrl+A"));
        }
        let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, dwFlags: flags, ..Default::default() } },
        };
        let none = KEYBD_EVENT_FLAGS(0);
        let inputs = [
            key(VK_CONTROL, none),
            key(VK_A, none),
            key(VK_A, KEYEVENTF_KEYUP),
            key(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }

    /// Type only while this window has keyboard focus, one character at a time, so the
    /// text can never go to another application if focus moves mid-way.
    pub fn safe_type(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let focused = unsafe { GetForegroundWindow() };
            if focused.0 as isize != self.hwnd {
                return Err(anyhow!("test window lost focus; stopped typing"));
            }
            let mut units = [0u16; 2];
            type_units(ch.encode_utf16(&mut units));
        }
        Ok(())
    }

    pub fn set_red(&self, red: bool) {
        BG_RED.store(red, Ordering::Relaxed);
        unsafe {
            let _ = InvalidateRect(Some(self.handle()), None, true);
        }
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
    let module = GetModuleHandleW(None)?;
    let inst = HINSTANCE(module.0);
    let class = w!("BabelStepsSpikeTarget");
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
        w!("BabelSteps Spike Target"),
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

    let child = |class: PCWSTR, text: PCWSTR, style: WINDOW_STYLE, x, y, cx, cy, id: isize| {
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
            Some(HMENU(id as *mut _)),
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

pub fn click(x: i32, y: i32) {
    click_with(x, y, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP);
}

/// Move, press, release, and move back as one `SendInput` call. Windows does not let
/// other input interleave within one call, so real mouse movement cannot redirect the
/// click, and the cursor is back where it started as soon as the call completes.
fn click_with(x: i32, y: i32, down: MOUSE_EVENT_FLAGS, up: MOUSE_EVENT_FLAGS) {
    unsafe {
        let mut original = POINT::default();
        let _ = GetCursorPos(&mut original);
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2);
        // Normalized coordinates, rounded so the event lands on the intended pixel.
        let nx = |px: i32| (((px - vx) as i64 * 65535 + (vw as i64 - 1) / 2) / (vw as i64 - 1)) as i32;
        let ny = |py: i32| (((py - vy) as i64 * 65535 + (vh as i64 - 1) / 2) / (vh as i64 - 1)) as i32;
        let event = |dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, dwFlags: flags, ..Default::default() } },
        };
        let absolute = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        let inputs = [
            event(nx(x), ny(y), absolute),
            event(0, 0, down),
            event(0, 0, up),
            event(nx(original.x), ny(original.y), absolute),
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Send a burst of F24 presses and releases. F24 types nothing and is on no normal
/// keyboard, so it exercises the keyboard hook without affecting any application.
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

fn type_units(units: &[u16]) {
    for unit in units {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            let input = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wScan: *unit, dwFlags: flags, ..Default::default() } },
            };
            unsafe {
                SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            }
        }
        thread_sleep(10);
    }
}

fn thread_sleep(ms: u64) {
    thread::sleep(std::time::Duration::from_millis(ms));
}

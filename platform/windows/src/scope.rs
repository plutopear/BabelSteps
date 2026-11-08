//! What a recording covers (A6, Q13): the whole screen by default, or one
//! monitor, or one window.
//!
//! Out of scope means not recorded at all: a click elsewhere makes no step and
//! no screenshot, and keys typed into another window are dropped. With one
//! window, each screenshot is cut down to that window (plus the menu or dialog
//! that was clicked) before analysis, so the rest of the screen never reaches
//! the disk. Windows lying on top of the chosen one still show inside its area.

use crate::monitors::Monitor;
use crate::uia::process_name;
use anyhow::{bail, Result};
use babelsteps_core::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, GW_OWNER, WS_CAPTION, WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Every monitor (Q13 default).
    #[default]
    Screen,
    /// One monitor, by its device name (`\\.\DISPLAY2`), which survives a
    /// resolution change.
    Monitor(String),
    /// One top-level window, with the menus, drop-downs and dialogs it opens.
    Window(isize),
}

/// A monitor the user can choose.
#[derive(Debug, Clone)]
pub struct MonitorChoice {
    pub device: String,
    /// 1 for the main monitor, then left to right.
    pub number: usize,
    pub primary: bool,
    pub width: i32,
    pub height: i32,
}

/// A window the user can choose. The title is shown in the picker only and is
/// never stored in a recording (A3).
#[derive(Debug, Clone)]
pub struct WindowChoice {
    pub id: isize,
    pub title: String,
    /// Process name without `.exe`, such as `notepad`.
    pub app: String,
}

/// Monitors, main first, then left to right and top to bottom.
pub fn monitor_choices() -> Vec<MonitorChoice> {
    let mut mons = crate::monitors::enumerate();
    mons.sort_by_key(|m| (!m.primary, m.x, m.y));
    mons.iter()
        .enumerate()
        .map(|(i, m)| MonitorChoice { device: m.device_name(), number: i + 1, primary: m.primary, width: m.w, height: m.h })
        .collect()
}

/// Windows as Alt+Tab would list them (visible, titled, not tool windows),
/// top of the stack first, leaving out this app's own windows.
pub fn window_choices() -> Vec<WindowChoice> {
    let mut found: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_cb), LPARAM(&mut found as *mut _ as isize));
    }
    let own = unsafe { GetCurrentProcessId() };
    found
        .into_iter()
        .filter_map(|id| {
            let h = HWND(id as *mut _);
            let pid = window_pid(h);
            if pid == own || !listable(h) {
                return None;
            }
            let title = window_title(h);
            if title.trim().is_empty() {
                return None;
            }
            Some(WindowChoice { id, title, app: process_name(pid) })
        })
        .collect()
}

unsafe extern "system" fn collect_cb(hwnd: HWND, data: LPARAM) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<isize>);
    out.push(hwnd.0 as isize);
    BOOL(1)
}

/// Shell windows that look like app windows but are the desktop or taskbar.
const SHELL_CLASSES: [&str; 4] = ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

fn listable(h: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
            return false;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        if cloaked != 0 {
            return false; // On another virtual desktop, or a suspended store app.
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        let owned = GetWindow(h, GW_OWNER).is_ok_and(|o| !o.is_invalid());
        let app_window = ex & WS_EX_APPWINDOW.0 != 0;
        if !app_window && (owned || ex & WS_EX_TOOLWINDOW.0 != 0) {
            return false;
        }
        !SHELL_CLASSES.contains(&class_name(h).as_str())
    }
}

fn class_name(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn window_title(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn window_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

/// Refuses a scope that cannot be recorded right now: a window that has
/// closed or belongs to this app, or a monitor that is no longer connected.
pub fn validate(scope: &Scope) -> Result<()> {
    match scope {
        Scope::Screen => Ok(()),
        Scope::Monitor(device) => {
            if crate::monitors::enumerate().iter().any(|m| &m.device_name() == device) {
                Ok(())
            } else {
                bail!("that monitor is no longer connected")
            }
        }
        Scope::Window(id) => {
            let h = HWND(*id as *mut _);
            if *id == 0 || !unsafe { IsWindow(Some(h)) }.as_bool() {
                bail!("that window has closed");
            }
            if window_pid(h) == unsafe { GetCurrentProcessId() } {
                bail!("BabelSteps cannot record its own window");
            }
            Ok(())
        }
    }
}

/// Why the scope can no longer be recorded, if it cannot.
pub(crate) fn lost(scope: &Scope, monitors: &[Monitor]) -> Option<&'static str> {
    match scope {
        Scope::Screen => None,
        Scope::Monitor(device) => (!monitors.iter().any(|m| &m.device_name() == device))
            .then_some("The chosen monitor is not connected, so nothing is being recorded. Stop the recording, or reconnect the monitor."),
        Scope::Window(id) => (!unsafe { IsWindow(Some(HWND(*id as *mut _))) }.as_bool())
            .then_some("The chosen window has closed, so nothing more is being recorded. Stop the recording."),
    }
}

/// Whether a top-level window counts as part of `target`: the window itself,
/// a window it owns (its dialogs, at any depth), or a pop-up of the same app
/// without a title bar (its menus, drop-downs and tooltips).
pub(crate) fn window_in_scope(root: isize, target: isize) -> bool {
    let (r, t) = (HWND(root as *mut _), HWND(target as *mut _));
    if root == 0 || target == 0 || !unsafe { IsWindow(Some(t)) }.as_bool() {
        return false;
    }
    if root == target {
        return true;
    }
    let mut w = r;
    for _ in 0..16 {
        match unsafe { GetWindow(w, GW_OWNER) } {
            Ok(o) if !o.is_invalid() => {
                if o == t {
                    return true;
                }
                w = o;
            }
            _ => break,
        }
    }
    if window_pid(r) != window_pid(t) {
        return false;
    }
    let style = unsafe { GetWindowLongPtrW(r, GWL_STYLE) } as u32;
    let ex = unsafe { GetWindowLongPtrW(r, GWL_EXSTYLE) } as u32;
    style & WS_POPUP.0 != 0 && style & WS_CAPTION.0 != WS_CAPTION.0 && ex & WS_EX_APPWINDOW.0 == 0
}

/// A window's visible bounds in physical pixels, without the invisible resize
/// borders where Windows reports them.
pub fn window_bounds(hwnd: isize) -> Option<Rect> {
    let h = HWND(hwnd as *mut _);
    let mut r = RECT::default();
    let dwm = unsafe {
        DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32)
    };
    if dwm.is_err() && unsafe { GetWindowRect(h, &mut r) }.is_err() {
        return None;
    }
    let rect = Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };
    (!rect.is_empty()).then_some(rect)
}

/// The area a window-scope screenshot keeps: the chosen window, plus the
/// in-scope window that was used (a menu can hang past the window's edge).
pub(crate) fn crop_area(target: isize, used: Option<isize>) -> Option<Rect> {
    let base = window_bounds(target)?;
    let extra = used.filter(|&u| u != target && window_in_scope(u, target)).and_then(window_bounds);
    Some(extra.map_or(base, |e| base.union(&e)))
}

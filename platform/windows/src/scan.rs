//! Where password fields are, and the three password signals for one event.
//!
//! The frame shows the screen before the event, but UI Automation can only be
//! asked afterwards. By then a window may have closed, been minimized, hidden,
//! moved, or covered. So:
//!
//! - The capture thread records which windows were visible when it grabbed the
//!   frame ([`snapshot`]). Analysis scans that list, not the screen as it is later.
//! - A window that changed since the frame (minimized, hidden, moved) cannot be
//!   scanned as it was. Its fields from a recent scan of it at the same position
//!   are used; with none, the scan counts as failed and the screenshot is withheld.
//! - Every field found by any scan is also remembered for [`REMEMBER_MS`] and
//!   blacked out in every frame. A field that moved is blacked out in both
//!   places; too much black is safe.

use crate::uia::ElementInfo;
use babelsteps_core::password::{PasswordSignals, Signal};
use babelsteps_core::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{GetWindowRgnBox, GDI_REGION_TYPE};
use std::collections::HashMap;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TRANSPARENT,
};

/// System window classes that cannot hold a password field: menus, tooltips and
/// drop shadows. They come and go with every menu click, so treating them as
/// unknown would withhold every screenshot of a menu.
const NO_FIELDS: [&str; 3] = ["#32768", "tooltips_class32", "SysShadow"];

unsafe fn cannot_hold_fields(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let n = GetClassNameW(hwnd, &mut buf);
    let class = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    NO_FIELDS.contains(&class.as_str())
}

/// How long a password field found by a scan keeps being blacked out everywhere.
pub const REMEMBER_MS: f64 = 10_000.0;
/// How long a window's own scan stands in for it once it has changed.
pub const WINDOW_MEMORY_MS: f64 = 60_000.0;

/// A top-level window and its rectangle at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WinState {
    pub hwnd: isize,
    pub rect: Rect,
}

/// Tolerance, in pixels, when checking that a field lies inside a found field.
const SLACK: i32 = 2;

struct EnumState {
    area: Rect,
    out: Vec<WinState>,
    /// Visible bounds of opaque windows above the current one (z-order is top first).
    occluders: Vec<Rect>,
    hidden: usize,
    /// Windows whose place in the z-order may have just changed. They never hide
    /// anything, because the frame shows the screen from before that change.
    never_occlude: Vec<isize>,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, data: LPARAM) -> BOOL {
    let st = &mut *(data.0 as *mut EnumState);
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
    if cloaked != 0 {
        return BOOL(1); // On another virtual desktop, or a hidden UWP shell window.
    }
    let mut r = RECT::default();
    if GetWindowRect(hwnd, &mut r).is_err() {
        return BOOL(1);
    }
    let wr = Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };
    let Some(on_area) = clip(&wr, &st.area) else { return BOOL(1) };
    // Entirely behind one opaque window above it: none of its pixels, password
    // fields included, can be in the frame.
    if covered(&on_area, &st.occluders) {
        st.hidden += 1;
        return BOOL(1);
    }
    if cannot_hold_fields(hwnd) {
        // Nothing to scan, and it hides nothing: a menu that opened on the
        // mouse press was not on screen in the frame.
        return BOOL(1);
    }
    st.out.push(WinState { hwnd: hwnd.0 as isize, rect: wr });
    if st.never_occlude.contains(&(hwnd.0 as isize)) {
        return BOOL(1);
    }
    if let Some(vis) = opaque_bounds(hwnd) {
        st.occluders.push(vis);
    }
    BOOL(1)
}

/// The window's visible bounds, if it certainly paints all of them. Layered,
/// click-through and DirectComposition (`WS_EX_NOREDIRECTIONBITMAP`, such as a
/// translucent terminal) windows may be see-through, and a window with a region
/// may be any shape, so those hide nothing.
unsafe fn opaque_bounds(hwnd: HWND) -> Option<Rect> {
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex & (WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0 | WS_EX_NOREDIRECTIONBITMAP.0) != 0 {
        return None;
    }
    let mut region = RECT::default();
    if GetWindowRgnBox(hwnd, &mut region) != GDI_REGION_TYPE(0) {
        return None; // Has a window region.
    }
    // Without the invisible resize borders that GetWindowRect includes.
    let mut r = RECT::default();
    DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32).ok()?;
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    (w > 0 && h > 0).then_some(Rect { x: r.left, y: r.top, w, h })
}

fn clip(r: &Rect, area: &Rect) -> Option<Rect> {
    let x0 = i64::from(r.x).max(i64::from(area.x));
    let y0 = i64::from(r.y).max(i64::from(area.y));
    let x1 = (i64::from(r.x) + i64::from(r.w)).min(i64::from(area.x) + i64::from(area.w));
    let y1 = (i64::from(r.y) + i64::from(r.h)).min(i64::from(area.y) + i64::from(area.h));
    (x0 < x1 && y0 < y1).then(|| Rect { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as i32, h: (y1 - y0) as i32 })
}

/// Visible, non-minimized top-level windows with some part on `area` (a monitor)
/// that is not hidden behind an opaque window, as they are right now. Returns
/// them and how many were skipped as hidden. `never_occlude` lists windows that
/// may have just come to the front (the clicked window, the foreground window).
/// The capture thread calls this right after grabbing a frame; it takes a few
/// milliseconds and makes no cross-process calls.
pub fn snapshot(area: Rect, never_occlude: &[isize]) -> (Vec<WinState>, usize) {
    let mut st = EnumState { area, out: Vec::new(), occluders: Vec::new(), hidden: 0, never_occlude: never_occlude.to_vec() };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut st as *mut _ as isize));
    }
    (st.out, st.hidden)
}

/// The window as it is now, or `None` if it is gone, hidden, minimized or cloaked.
pub fn current_state(hwnd: isize) -> Option<WinState> {
    let h = HWND(hwnd as *mut _);
    unsafe {
        if !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return None;
        }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4);
        if cloaked != 0 {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        Some(WinState { hwnd, rect: Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top } })
    }
}

/// Splits a frame's window list into windows that can be scanned as they were
/// (unchanged since the frame) and windows that have changed since.
pub fn split_changed(snap: &[WinState], now: impl Fn(isize) -> Option<WinState>) -> (Vec<WinState>, Vec<WinState>) {
    snap.iter().partition(|w| now(w.hwnd).as_ref() == Some(*w))
}

pub fn point_in(x: i32, y: i32, rects: &[Rect]) -> bool {
    rects.iter().any(|r| {
        let (x, y) = (i64::from(x), i64::from(y));
        x >= i64::from(r.x) && y >= i64::from(r.y) && x < i64::from(r.x) + i64::from(r.w) && y < i64::from(r.y) + i64::from(r.h)
    })
}

/// True if `r` lies inside one of `rects`, within a couple of pixels.
pub fn covered(r: &Rect, rects: &[Rect]) -> bool {
    rects.iter().any(|f| {
        r.x >= f.x - SLACK
            && r.y >= f.y - SLACK
            && i64::from(r.x) + i64::from(r.w) <= i64::from(f.x) + i64::from(f.w) + i64::from(SLACK)
            && i64::from(r.y) + i64::from(r.h) <= i64::from(f.y) + i64::from(f.h) + i64::from(SLACK)
    })
}

/// Password fields seen recently, with when they were last seen (QPC ticks),
/// and each window's last scan.
#[derive(Default)]
pub struct PasswordMemory {
    seen: Vec<(Rect, i64)>,
    windows: HashMap<isize, (Rect, Vec<Rect>, i64)>,
}

impl PasswordMemory {
    /// One window's scan: its rectangle then, the fields found, and when.
    pub fn remember_window(&mut self, w: &WinState, fields: &[Rect], t: i64) {
        self.remember(fields, t);
        self.windows.insert(w.hwnd, (w.rect, fields.to_vec(), t));
    }

    /// Fields from every window scanned within `WINDOW_MEMORY_MS` of `t` that
    /// overlap `area`, as they were at that scan. Covers windows closed or
    /// changed after the frame (Alt+F4 acts on key-down, a page navigates away)
    /// that the frame may still show.
    pub fn recent_window_fields(&self, area: &Rect, t: i64, ms_per_tick: f64) -> Vec<Rect> {
        self.windows
            .values()
            .filter(|(_, _, at)| ((t - at).abs() as f64 * ms_per_tick) < WINDOW_MEMORY_MS)
            .flat_map(|(_, fields, _)| fields.iter().copied())
            .filter(|f| clip(f, area).is_some())
            .collect()
    }

    /// The fields of a window from a scan made while it had the same rectangle,
    /// within `WINDOW_MEMORY_MS` of `t`. `None` if there is no such scan.
    pub fn window_fields(&self, w: &WinState, t: i64, ms_per_tick: f64) -> Option<Vec<Rect>> {
        let (rect, fields, at) = self.windows.get(&w.hwnd)?;
        let fresh = ((t - at).abs() as f64 * ms_per_tick) < WINDOW_MEMORY_MS;
        (rect == &w.rect && fresh).then(|| fields.clone())
    }

    pub fn remember(&mut self, rects: &[Rect], t: i64) {
        for r in rects {
            match self.seen.iter_mut().find(|(s, _)| s == r) {
                Some(entry) => entry.1 = entry.1.max(t),
                None => self.seen.push((*r, t)),
            }
        }
    }

    /// Fields seen within `REMEMBER_MS` of time `t`, before or after it.
    pub fn around(&mut self, t: i64, ms_per_tick: f64) -> Vec<Rect> {
        self.seen.retain(|(_, seen)| ((t - seen) as f64 * ms_per_tick) < REMEMBER_MS);
        self.seen.iter().map(|(r, _)| *r).collect()
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }
}

fn signal(info: Option<&ElementInfo>) -> Signal {
    match info.and_then(|i| i.is_password) {
        None => Signal::Unknown,
        Some(true) => Signal::Yes,
        Some(false) => Signal::No,
    }
}

/// The three signals for one event.
///
/// - `element`: the element under the click, or for a key, the element at the
///   centre of the focused element (a second, independent lookup).
/// - `focused`: the focused element.
/// - `window_scan`: `Unknown` if the scan failed. Otherwise, if either lookup
///   reports a password field, `Yes` only when every such field lies inside a
///   field the scan found (so its pixels are known to be blacked out). If
///   neither does, `Yes` when the point is inside a found field, which catches
///   Edge's wrong first answer.
pub fn signals(
    element: Option<&ElementInfo>,
    focused: Option<&ElementInfo>,
    scanned: Option<&[Rect]>,
    point: Option<(i32, i32)>,
) -> PasswordSignals {
    let (e, f) = (signal(element), signal(focused));
    let window_scan = match scanned {
        None => Signal::Unknown,
        Some(found) => {
            let reported: Vec<Option<Rect>> = [(e, element), (f, focused)]
                .iter()
                .filter(|(s, _)| *s == Signal::Yes)
                .map(|(_, i)| i.and_then(|i| i.bounds))
                .collect();
            let inside = if reported.is_empty() {
                point.is_some_and(|(x, y)| point_in(x, y, found))
            } else {
                reported.iter().all(|b| b.is_some_and(|b| covered(&b, found)))
            };
            if inside {
                Signal::Yes
            } else {
                Signal::No
            }
        }
    };
    PasswordSignals { element: e, window_scan, focused: f }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(is_password: Option<bool>, bounds: Option<Rect>) -> ElementInfo {
        ElementInfo { name: String::new(), control_type: String::new(), process_name: String::new(), is_password, bounds }
    }
    const FIELD: Rect = Rect { x: 100, y: 100, w: 200, h: 30 };

    #[test]
    fn unreadable_property_is_unknown_not_no() {
        let s = signals(Some(&el(None, None)), Some(&el(Some(false), None)), Some(&[]), Some((0, 0)));
        assert_eq!(s.element, Signal::Unknown);
        assert!(!s.text_allowed() && !s.screenshot_allowed());
    }

    #[test]
    fn failed_scan_withholds_everything() {
        let s = signals(Some(&el(Some(false), None)), Some(&el(Some(false), None)), None, Some((0, 0)));
        assert_eq!(s.window_scan, Signal::Unknown);
        assert!(!s.text_allowed() && !s.screenshot_allowed());
    }

    #[test]
    fn missing_lookups_are_unknown() {
        let s = signals(None, None, Some(&[]), Some((0, 0)));
        assert!(!s.text_allowed() && !s.screenshot_allowed());
    }

    #[test]
    fn reported_field_found_by_scan_allows_the_redacted_screenshot() {
        let s = signals(Some(&el(Some(true), Some(FIELD))), Some(&el(Some(true), Some(FIELD))), Some(&[FIELD]), Some((150, 110)));
        assert_eq!(s.window_scan, Signal::Yes);
        assert!(s.screenshot_allowed() && !s.text_allowed());
    }

    #[test]
    fn reported_field_the_scan_missed_withholds_the_screenshot() {
        let other = Rect { x: 500, y: 500, w: 100, h: 20 };
        let s = signals(Some(&el(Some(true), Some(FIELD))), Some(&el(Some(false), None)), Some(&[other]), Some((150, 110)));
        assert_eq!(s.window_scan, Signal::No);
        assert!(!s.screenshot_allowed());
    }

    #[test]
    fn reported_field_without_bounds_cannot_be_confirmed() {
        let s = signals(Some(&el(Some(true), None)), Some(&el(Some(false), None)), Some(&[FIELD]), Some((150, 110)));
        assert!(!s.screenshot_allowed());
    }

    #[test]
    fn wrong_first_answer_is_caught_by_the_point_inside_a_found_field() {
        // Edge in M0: the element said "not a password" for a real one.
        let s = signals(Some(&el(Some(false), Some(FIELD))), Some(&el(Some(false), None)), Some(&[FIELD]), Some((150, 110)));
        assert_eq!(s.window_scan, Signal::Yes);
        assert!(!s.text_allowed());
        assert!(s.screenshot_allowed(), "the field was found, so it is blacked out");
    }

    #[test]
    fn ordinary_click_allows_both() {
        let s = signals(Some(&el(Some(false), None)), Some(&el(Some(false), None)), Some(&[FIELD]), Some((10, 10)));
        assert!(s.text_allowed() && s.screenshot_allowed());
    }

    #[test]
    fn memory_keeps_fields_for_ten_seconds_either_side() {
        let mut m = PasswordMemory::default();
        m.remember(&[FIELD], 1_000);
        let ms_per_tick = 1.0; // 1 tick = 1 ms here
        assert_eq!(m.around(5_000, ms_per_tick), vec![FIELD]);
        assert_eq!(m.around(10_999, ms_per_tick), vec![FIELD]);
        assert!(m.around(11_001, ms_per_tick).is_empty());
    }

    #[test]
    fn changed_windows_are_split_out() {
        let a = WinState { hwnd: 1, rect: FIELD };
        let b = WinState { hwnd: 2, rect: Rect { x: 0, y: 0, w: 50, h: 50 } };
        let c = WinState { hwnd: 3, rect: Rect { x: 9, y: 9, w: 50, h: 50 } };
        // a unchanged, b minimized (gone), c moved.
        let now = |h: isize| match h {
            1 => Some(a),
            3 => Some(WinState { hwnd: 3, rect: Rect { x: 500, y: 9, w: 50, h: 50 } }),
            _ => None,
        };
        let (live, changed) = split_changed(&[a, b, c], now);
        assert_eq!(live, vec![a]);
        assert_eq!(changed, vec![b, c]);
    }

    #[test]
    fn a_changed_window_uses_its_scan_only_from_the_same_position() {
        let w = WinState { hwnd: 7, rect: Rect { x: 0, y: 0, w: 400, h: 300 } };
        let mut m = PasswordMemory::default();
        m.remember_window(&w, &[FIELD], 1_000);
        assert_eq!(m.window_fields(&w, 2_000, 1.0), Some(vec![FIELD]));
        let moved = WinState { hwnd: 7, rect: Rect { x: 10, y: 0, w: 400, h: 300 } };
        assert_eq!(m.window_fields(&moved, 2_000, 1.0), None, "scanned at another position");
        assert_eq!(m.window_fields(&w, 1_000 + 60_001, 1.0), None, "too old");
        assert_eq!(m.window_fields(&WinState { hwnd: 8, ..w }, 2_000, 1.0), None, "never scanned");
    }

    #[test]
    fn fields_of_windows_since_closed_are_still_blacked_out() {
        let w = WinState { hwnd: 7, rect: Rect { x: 0, y: 0, w: 400, h: 300 } };
        let mut m = PasswordMemory::default();
        m.remember_window(&w, &[FIELD], 1_000);
        let screen = Rect { x: 0, y: 0, w: 1920, h: 1080 };
        let other = Rect { x: 3000, y: 0, w: 1920, h: 1080 };
        assert_eq!(m.recent_window_fields(&screen, 40_000, 1.0), vec![FIELD], "past the 10 s field memory");
        assert!(m.recent_window_fields(&other, 40_000, 1.0).is_empty(), "another monitor");
        assert!(m.recent_window_fields(&screen, 70_000, 1.0).is_empty(), "past 60 s");
    }

    #[test]
    fn geometry_helpers_handle_extremes() {
        let huge = Rect { x: i32::MAX - 1, y: i32::MAX - 1, w: i32::MAX, h: i32::MAX };
        assert!(clip(&huge, &FIELD).is_none());
        assert!(!point_in(i32::MAX, i32::MAX, &[FIELD]));
        assert!(covered(&Rect { x: 99, y: 99, w: 202, h: 32 }, &[FIELD]), "within slack");
        assert!(!covered(&Rect { x: 90, y: 100, w: 200, h: 30 }, &[FIELD]));
    }
}

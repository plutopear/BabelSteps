//! Spike 3: UI Automation lookup of the element under a click, with `IsPassword`.
//! These calls cross process boundaries, so they never run on the hook thread.

use anyhow::Result;
use babelsteps_core::{Rect, Target};
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, VARIANT_TRUE};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, TreeScope_Descendants, UIA_IsPasswordPropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetWindowTextW, WindowFromPoint, GA_ROOT};

pub struct Uia {
    automation: IUIAutomation,
}

impl Uia {
    /// Must be created on the thread that uses it. Initializes COM on that thread.
    pub fn new() -> Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { automation })
        }
    }

    pub fn target_at(&self, x: i32, y: i32) -> Result<Target> {
        unsafe {
            let el = self.automation.ElementFromPoint(POINT { x, y })?;
            let name = el.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            let control_type = el
                .CurrentControlType()
                .map(|c| control_type_name(c.0).to_string())
                .unwrap_or_default();
            let is_password = el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
            let bounds = el.CurrentBoundingRectangle().ok().map(|r| Rect {
                x: r.left,
                y: r.top,
                w: r.right - r.left,
                h: r.bottom - r.top,
            });
            let process_name = el.CurrentProcessId().ok().map(|p| process_name(p as u32)).unwrap_or_default();
            Ok(Target { name, control_type, window_title: Some(window_title_at(x, y)), process_name, is_password, bounds })
        }
    }
}

impl Uia {
    /// Bounds of every password field inside a top-level window. A password field
    /// shows its length even when it is not the thing being clicked, so every
    /// screenshot needs this, not just clicks on the field itself.
    pub fn password_rects(&self, root: HWND) -> Result<Vec<Rect>> {
        unsafe {
            let el = self.automation.ElementFromHandle(root)?;
            // A VARIANT holding the boolean `true`.
            let mut value = VARIANT::default();
            (*value.Anonymous.Anonymous).vt = VT_BOOL;
            (*value.Anonymous.Anonymous).Anonymous.boolVal = VARIANT_TRUE;
            let cond = self.automation.CreatePropertyCondition(UIA_IsPasswordPropertyId, &value)?;
            let found = el.FindAll(TreeScope_Descendants, &cond)?;
            let mut out = Vec::new();
            for i in 0..found.Length()? {
                if let Ok(r) = found.GetElement(i).and_then(|e| e.CurrentBoundingRectangle()) {
                    out.push(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top });
                }
            }
            Ok(out)
        }
    }
}

impl Uia {
    /// The element at a point and its ancestors, as (type, name, is_password).
    pub fn chain_at(&self, x: i32, y: i32, depth: usize) -> Result<Vec<(String, String, bool)>> {
        unsafe {
            let walker = self.automation.RawViewWalker()?;
            let mut el = self.automation.ElementFromPoint(POINT { x, y })?;
            let mut out = Vec::new();
            for _ in 0..depth {
                out.push((
                    el.CurrentControlType().map(|c| control_type_name(c.0).to_string()).unwrap_or_default(),
                    el.CurrentName().map(|b| b.to_string()).unwrap_or_default(),
                    el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false),
                ));
                match walker.GetParentElement(&el) {
                    Ok(p) => el = p,
                    Err(_) => break,
                }
            }
            Ok(out)
        }
    }

    /// The element holding keyboard focus, as (type, name, is_password). This is what
    /// decides whether a keystroke is going into a password field.
    pub fn focused(&self) -> Result<(String, String, bool)> {
        unsafe {
            let el = self.automation.GetFocusedElement()?;
            Ok((
                el.CurrentControlType().map(|c| control_type_name(c.0).to_string()).unwrap_or_default(),
                el.CurrentName().map(|b| b.to_string()).unwrap_or_default(),
                el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false),
            ))
        }
    }
}

/// Top-level window under a screen point.
pub fn root_at(x: i32, y: i32) -> Option<HWND> {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.is_invalid() {
            return None;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        (!root.is_invalid()).then_some(root)
    }
}

fn window_title_at(x: i32, y: i32) -> String {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.is_invalid() {
            return String::new();
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(root, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn control_type_name(id: i32) -> &'static str {
    match id {
        50000 => "Button",
        50001 => "Calendar",
        50002 => "Checkbox",
        50003 => "Combo box",
        50004 => "Edit",
        50005 => "Link",
        50006 => "Image",
        50007 => "List item",
        50008 => "List",
        50009 => "Menu",
        50010 => "Menu bar",
        50011 => "Menu item",
        50012 => "Progress bar",
        50013 => "Radio button",
        50014 => "Scroll bar",
        50015 => "Slider",
        50016 => "Spinner",
        50017 => "Status bar",
        50018 => "Tab",
        50019 => "Tab item",
        50020 => "Text",
        50021 => "Toolbar",
        50022 => "Tooltip",
        50023 => "Tree",
        50024 => "Tree item",
        50025 => "Custom",
        50026 => "Group",
        50027 => "Thumb",
        50028 => "Data grid",
        50029 => "Data item",
        50030 => "Document",
        50031 => "Split button",
        50032 => "Window",
        50033 => "Pane",
        50034 => "Header",
        50035 => "Header item",
        50036 => "Table",
        50037 => "Title bar",
        50038 => "Separator",
        _ => "",
    }
}

//! UI Automation: what was clicked or focused, and where password fields are.
//!
//! These calls cross into other processes and can be slow (a password scan took
//! about 270 ms on a large page) or hang outright in a misbehaving app. So they
//! run on a worker thread, and every call has a time limit. When a call times
//! out, its worker is abandoned and a fresh one serves the next call; the caller
//! treats the answer as unknown, which the password check turns into "drop the
//! text, withhold the screenshot".
//!
//! A property that cannot be read is unknown, never "not a password".

use anyhow::{anyhow, Result};
use babelsteps_core::Rect;
use crossbeam_channel::{bounded, unbounded, Sender};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, VARIANT_TRUE};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants, UIA_IsPasswordPropertyId,
};

/// What UI Automation says about one element.
#[derive(Clone)]
pub struct ElementInfo {
    pub name: String,
    pub control_type: String,
    pub process_name: String,
    /// `None` when the property could not be read.
    pub is_password: Option<bool>,
    pub bounds: Option<Rect>,
}

pub struct Uia {
    automation: IUIAutomation,
}

impl Uia {
    fn new() -> Result<Self> {
        unsafe {
            let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { automation })
        }
    }

    fn info(el: &IUIAutomationElement) -> ElementInfo {
        unsafe {
            let bounds = el.CurrentBoundingRectangle().ok().and_then(|r| {
                let (w, h) = (r.right - r.left, r.bottom - r.top);
                (w > 0 && h > 0).then_some(Rect { x: r.left, y: r.top, w, h })
            });
            ElementInfo {
                name: el.CurrentName().map(|b| b.to_string()).unwrap_or_default(),
                control_type: el.CurrentControlType().map(|c| control_type_name(c.0).to_string()).unwrap_or_default(),
                process_name: el.CurrentProcessId().ok().map(|p| process_name(p as u32)).unwrap_or_default(),
                is_password: el.CurrentIsPassword().ok().map(|b| b.as_bool()),
                bounds,
            }
        }
    }

    pub fn element_at(&self, x: i32, y: i32) -> Result<ElementInfo> {
        let el = unsafe { self.automation.ElementFromPoint(POINT { x, y })? };
        Ok(Self::info(&el))
    }

    pub fn focused(&self) -> Result<ElementInfo> {
        let el = unsafe { self.automation.GetFocusedElement()? };
        Ok(Self::info(&el))
    }

    /// Bounds of every password field in each of the given top-level windows.
    /// Fails if any window cannot be searched: its fields could be on screen.
    pub fn password_rects(&self, windows: &[isize]) -> Result<Vec<(isize, Vec<Rect>)>> {
        unsafe {
            let mut value = VARIANT::default();
            (*value.Anonymous.Anonymous).vt = VT_BOOL;
            (*value.Anonymous.Anonymous).Anonymous.boolVal = VARIANT_TRUE;
            let cond = self.automation.CreatePropertyCondition(UIA_IsPasswordPropertyId, &value)?;
            let mut out = Vec::new();
            for &hwnd in windows {
                let mut fields = Vec::new();
                let root = self
                    .automation
                    .ElementFromHandle(HWND(hwnd as *mut _))
                    .map_err(|e| anyhow!("a window could not be searched: {e}"))?;
                let found = root.FindAll(TreeScope_Descendants, &cond)?;
                for i in 0..found.Length()? {
                    let el = found.GetElement(i)?;
                    // A field whose bounds cannot be read cannot be blacked out.
                    let r = el.CurrentBoundingRectangle()?;
                    let (w, h) = (r.right - r.left, r.bottom - r.top);
                    if w > 0 && h > 0 {
                        fields.push(Rect { x: r.left, y: r.top, w, h });
                    }
                }
                out.push((hwnd, fields));
            }
            Ok(out)
        }
    }
}

type Job = Box<dyn FnOnce(Option<&Uia>) + Send>;

/// Test-only fault injection, shared with the pipeline.
#[derive(Default)]
pub struct UiaFaults {
    /// Added to every call, to force timeouts.
    pub delay_ms: AtomicU64,
    /// Every call fails, as if UI Automation were unavailable.
    pub fail: AtomicBool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UiaStats {
    pub calls: u64,
    pub timeouts: u64,
    pub errors: u64,
    /// Calls refused because too many workers were stuck.
    pub refused: u64,
}

pub struct UiaPool {
    tx: Option<Sender<Job>>,
    stuck: Arc<AtomicUsize>,
    faults: Arc<UiaFaults>,
    pub stats: UiaStats,
}

/// Abandoned workers allowed at once. Beyond this every call answers "unknown"
/// at once instead of piling up threads.
const MAX_STUCK: usize = 4;

impl UiaPool {
    pub fn new(faults: Arc<UiaFaults>) -> Self {
        UiaPool { tx: None, stuck: Arc::new(AtomicUsize::new(0)), faults, stats: UiaStats::default() }
    }

    fn spawn(&mut self) -> Option<Sender<Job>> {
        let (tx, rx) = unbounded::<Job>();
        let built = thread::Builder::new().name("uia-worker".into()).spawn(move || {
            crate::monitors::make_thread_dpi_aware();
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let uia = Uia::new().ok();
            for job in rx {
                job(uia.as_ref());
            }
            drop(uia);
            if com {
                unsafe { CoUninitialize() };
            }
        });
        built.ok().map(|_| tx)
    }

    /// Run `f` on the worker with a time limit. `None` means unknown: an error,
    /// a timeout, or no worker available.
    pub fn call<R, F>(&mut self, timeout: Duration, f: F) -> Option<R>
    where
        R: Send + 'static,
        F: FnOnce(&Uia) -> Result<R> + Send + 'static,
    {
        self.stats.calls += 1;
        if self.tx.is_none() {
            if self.stuck.load(Ordering::SeqCst) >= MAX_STUCK {
                self.stats.refused += 1;
                return None;
            }
            self.tx = self.spawn();
        }
        let tx = self.tx.as_ref()?;
        let (reply_tx, reply_rx) = bounded::<Result<R>>(1);
        let faults = self.faults.clone();
        let job: Job = Box::new(move |uia| {
            let delay = faults.delay_ms.load(Ordering::Relaxed);
            if delay > 0 {
                thread::sleep(Duration::from_millis(delay));
            }
            let r = match uia {
                _ if faults.fail.load(Ordering::Relaxed) => Err(anyhow!("injected failure")),
                Some(u) => f(u),
                None => Err(anyhow!("UI Automation is unavailable")),
            };
            let _ = reply_tx.send(r);
        });
        if tx.send(job).is_err() {
            self.tx = None;
            self.stats.errors += 1;
            return None;
        }
        match reply_rx.recv_timeout(timeout) {
            Ok(Ok(r)) => Some(r),
            Ok(Err(_)) => {
                self.stats.errors += 1;
                None
            }
            Err(_) => {
                // Abandon this worker: it exits when its stuck call returns,
                // because its queue is closed. A fresh one serves the next call.
                self.stats.timeouts += 1;
                self.tx = None;
                let stuck = self.stuck.clone();
                stuck.fetch_add(1, Ordering::SeqCst);
                let _ = thread::Builder::new().name("uia-reaper".into()).spawn(move || {
                    let _ = reply_rx.recv();
                    stuck.fetch_sub(1, Ordering::SeqCst);
                });
                None
            }
        }
    }
}

pub(crate) fn process_name(pid: u32) -> String {
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
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

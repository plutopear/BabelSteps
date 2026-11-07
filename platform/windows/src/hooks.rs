//! Low-level mouse and keyboard hooks, plus foreground-window changes, on one
//! dedicated thread with its own message loop.
//!
//! All mouse and keyboard input on the machine waits on the hook callbacks, and
//! Windows silently removes a hook whose callback is too slow. So the callbacks
//! only timestamp the event and push it to a bounded, non-allocating channel.
//!
//! Because removal is silent, [`HookHandle::check_alive`] compares the system's
//! last-input time with our last callback, and reinstalls the hooks when input
//! arrived that we never saw.

use crate::clock;
use anyhow::{anyhow, bail, Result};
use babelsteps_core::MouseButton;
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

/// One raw input event. `t` is the QueryPerformanceCounter time of the callback.
/// No `Debug`: it carries the code of every key pressed.
#[derive(Clone, Copy)]
pub enum RawEvent {
    MouseDown { button: MouseButton, x: i32, y: i32, t: i64, injected: bool },
    KeyDown { vk: u32, scan: u32, extended: bool, t: i64, injected: bool },
    KeyUp { vk: u32, scan: u32, t: i64, injected: bool },
    /// A different top-level window came to the foreground.
    Foreground { hwnd: isize, t: i64 },
}

impl RawEvent {
    pub fn t(&self) -> i64 {
        match *self {
            RawEvent::MouseDown { t, .. } | RawEvent::KeyDown { t, .. } | RawEvent::KeyUp { t, .. } | RawEvent::Foreground { t, .. } => t,
        }
    }
}

const QUEUE_CAPACITY: usize = 8192;
const LLKHF_EXTENDED_BIT: u32 = 0x01;
const LLKHF_INJECTED_BIT: u32 = 0x10;
const LLMHF_INJECTED_BIT: u32 = 0x01;
const WM_REHOOK: u32 = WM_APP + 1;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
static MAX_TICKS: AtomicI64 = AtomicI64::new(0);
static TOTAL_TICKS: AtomicI64 = AtomicI64::new(0);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static LAST_CALLBACK: AtomicI64 = AtomicI64::new(0);
static REHOOKS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub struct HookStats {
    pub callbacks: u64,
    pub mean_us: f64,
    pub max_us: f64,
    /// Events lost because the queue was full. Must stay 0.
    pub dropped: u64,
    /// Times the hooks were found removed by Windows and reinstalled.
    pub rehooks: u64,
}

pub fn stats() -> HookStats {
    let callbacks = CALLBACKS.load(Ordering::Relaxed);
    let total = TOTAL_TICKS.load(Ordering::Relaxed);
    HookStats {
        callbacks,
        mean_us: if callbacks == 0 { 0.0 } else { clock::to_us(total) / callbacks as f64 },
        max_us: clock::to_us(MAX_TICKS.load(Ordering::Relaxed)),
        dropped: DROPPED.load(Ordering::Relaxed),
        rehooks: REHOOKS.load(Ordering::Relaxed),
    }
}

fn channel() -> &'static (Sender<RawEvent>, Receiver<RawEvent>) {
    static CH: OnceLock<(Sender<RawEvent>, Receiver<RawEvent>)> = OnceLock::new();
    CH.get_or_init(|| bounded(QUEUE_CAPACITY))
}

#[inline]
fn record(t0: i64) {
    let dt = clock::now() - t0;
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
    TOTAL_TICKS.fetch_add(dt, Ordering::Relaxed);
    MAX_TICKS.fetch_max(dt, Ordering::Relaxed);
    LAST_CALLBACK.store(t0, Ordering::Relaxed);
}

#[inline]
fn push(ev: RawEvent) {
    if channel().0.try_send(ev).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let t0 = clock::now();
    if code == HC_ACTION as i32 {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        let injected = info.flags & LLMHF_INJECTED_BIT != 0;
        let button = match wparam.0 as u32 {
            WM_LBUTTONDOWN => Some(MouseButton::Left),
            WM_RBUTTONDOWN => Some(MouseButton::Right),
            WM_MBUTTONDOWN => Some(MouseButton::Middle),
            _ => None, // Moves and button-ups are not steps; they cost only this match.
        };
        if let Some(button) = button {
            push(RawEvent::MouseDown { button, x, y, t: t0, injected });
        }
    }
    record(t0);
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let t0 = clock::now();
    if code == HC_ACTION as i32 {
        let info = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let injected = info.flags.0 & LLKHF_INJECTED_BIT != 0;
        let extended = info.flags.0 & LLKHF_EXTENDED_BIT != 0;
        let (vk, scan) = (info.vkCode, info.scanCode);
        match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => push(RawEvent::KeyDown { vk, scan, extended, t: t0, injected }),
            WM_KEYUP | WM_SYSKEYUP => push(RawEvent::KeyUp { vk, scan, t: t0, injected }),
            _ => {}
        }
    }
    record(t0);
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn foreground_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    push(RawEvent::Foreground { hwnd: hwnd.0 as isize, t: clock::now() });
}

pub struct HookHandle {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
    started: i64,
}

struct Hooks {
    mouse: HHOOK,
    keyboard: HHOOK,
}

unsafe fn install(inst: HINSTANCE) -> std::result::Result<Hooks, String> {
    let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(inst), 0);
    let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(inst), 0);
    match (mouse, keyboard) {
        (Ok(mouse), Ok(keyboard)) => Ok(Hooks { mouse, keyboard }),
        (m, k) => {
            if let Ok(m) = &m {
                let _ = UnhookWindowsHookEx(*m);
            }
            if let Ok(k) = &k {
                let _ = UnhookWindowsHookEx(*k);
            }
            Err(format!("SetWindowsHookExW failed: mouse={:?} keyboard={:?}", m.err(), k.err()))
        }
    }
}

/// Install the hooks. Only one set can be active in the process.
pub fn start() -> Result<(HookHandle, Receiver<RawEvent>)> {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        bail!("a recording is already running");
    }
    clock::freq(); // Initialize before a callback can run.
    for counter in [&CALLBACKS, &DROPPED, &REHOOKS] {
        counter.store(0, Ordering::Relaxed);
    }
    MAX_TICKS.store(0, Ordering::Relaxed);
    TOTAL_TICKS.store(0, Ordering::Relaxed);
    let rx = channel().1.clone();
    while rx.try_recv().is_ok() {} // Drain anything left from an earlier recording.

    let (ready_tx, ready_rx) = bounded::<std::result::Result<u32, String>>(1);
    let spawned = thread::Builder::new().name("input-hooks".into()).spawn(move || unsafe {
        crate::monitors::make_thread_dpi_aware();
        // All mouse and keyboard input on the machine waits on this thread.
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let tid = GetCurrentThreadId();
        let inst = match GetModuleHandleW(None) {
            Ok(m) => HINSTANCE(m.0),
            Err(e) => {
                let _ = ready_tx.send(Err(format!("GetModuleHandleW: {e}")));
                return;
            }
        };
        let mut hooks = match install(inst) {
            Ok(h) => h,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let fg = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(foreground_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        LAST_CALLBACK.store(clock::now(), Ordering::Relaxed);
        let _ = ready_tx.send(Ok(tid));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.hwnd.is_invalid() && msg.message == WM_REHOOK {
                let _ = UnhookWindowsHookEx(hooks.mouse);
                let _ = UnhookWindowsHookEx(hooks.keyboard);
                match install(inst) {
                    Ok(h) => {
                        hooks = h;
                        REHOOKS.fetch_add(1, Ordering::Relaxed);
                    }
                    // Nothing more can be recorded; the watchdog keeps reporting it.
                    Err(_) => break,
                }
                continue;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(hooks.mouse);
        let _ = UnhookWindowsHookEx(hooks.keyboard);
        if !fg.is_invalid() {
            let _ = UnhookWinEvent(fg);
        }
    });
    let join = match spawned {
        Ok(j) => j,
        Err(e) => {
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(e.into());
        }
    };
    let thread_id = match ready_rx.recv() {
        Ok(Ok(tid)) => tid,
        Ok(Err(e)) => {
            let _ = join.join();
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(anyhow!(e));
        }
        Err(_) => {
            let _ = join.join();
            ACTIVE.store(false, Ordering::SeqCst);
            bail!("the input hook thread exited early");
        }
    };
    Ok((HookHandle { thread_id, join: Some(join), started: clock::now() }, rx))
}

impl HookHandle {
    /// Returns false if input reached the system that our hooks never saw, in
    /// which case the hooks are reinstalled. Call about once a second.
    pub fn check_alive(&self) -> bool {
        unsafe {
            let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
            if !GetLastInputInfo(&mut info).as_bool() {
                return true; // Cannot tell; do not churn the hooks.
            }
            let input_age_ms = f64::from(GetTickCount().wrapping_sub(info.dwTime));
            let now = clock::now();
            let since_start_ms = clock::to_ms(now - self.started);
            let callback_age_ms = clock::to_ms(now - LAST_CALLBACK.load(Ordering::Relaxed));
            // Input newer than our last callback by over a second, after start.
            let missed = callback_age_ms > input_age_ms + 1000.0 && input_age_ms < since_start_ms;
            if missed {
                let _ = PostThreadMessageW(self.thread_id, WM_REHOOK, WPARAM(0), LPARAM(0));
                LAST_CALLBACK.store(now, Ordering::Relaxed); // One reinstall per miss.
            }
            !missed
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(j) = self.join.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = j.join();
            ACTIVE.store(false, Ordering::SeqCst);
        }
    }
}

impl Drop for HookHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

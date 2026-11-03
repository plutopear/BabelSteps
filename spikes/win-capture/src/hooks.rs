//! Spike 1: low-level mouse and keyboard hooks on a dedicated thread.
//!
//! Windows removes a low-level hook whose callback is too slow, so the callback
//! only timestamps the event and pushes it to a bounded, non-allocating channel.
//! Everything else happens on the worker thread.

use crate::clock;
use anyhow::{anyhow, Result};
use babelsteps_core::MouseButton;
use crossbeam_channel::{bounded, Receiver, Sender};
use serde::Serialize;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    MouseDown { button: MouseButton, x: i32, y: i32, t: i64, injected: bool },
    MouseUp { button: MouseButton, x: i32, y: i32, t: i64, injected: bool },
    KeyDown { vk: u32, scan: u32, t: i64, injected: bool },
    KeyUp { vk: u32, scan: u32, t: i64, injected: bool },
}

const QUEUE_CAPACITY: usize = 4096;
const LLKHF_INJECTED_BIT: u32 = 0x10;
const LLMHF_INJECTED_BIT: u32 = 0x01;

/// Upper bounds, in microseconds, of the callback-duration histogram buckets.
pub const BUCKETS_US: [f64; 7] = [10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, f64::INFINITY];

struct Stats {
    callbacks: AtomicU64,
    total_ticks: AtomicI64,
    max_ticks: AtomicI64,
    dropped: AtomicU64,
    buckets: [AtomicU64; 7],
}

static STATS: Stats = Stats {
    callbacks: AtomicU64::new(0),
    total_ticks: AtomicI64::new(0),
    max_ticks: AtomicI64::new(0),
    dropped: AtomicU64::new(0),
    buckets: [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ],
};

#[derive(Debug, Clone, Serialize)]
pub struct HookStats {
    pub callbacks: u64,
    pub mean_us: f64,
    pub max_us: f64,
    pub dropped: u64,
    /// Count per bucket, matching `BUCKETS_US`.
    pub histogram: Vec<u64>,
}

pub fn stats() -> HookStats {
    let callbacks = STATS.callbacks.load(Ordering::Relaxed);
    let total = STATS.total_ticks.load(Ordering::Relaxed);
    HookStats {
        callbacks,
        mean_us: if callbacks == 0 { 0.0 } else { clock::to_us(total) / callbacks as f64 },
        max_us: clock::to_us(STATS.max_ticks.load(Ordering::Relaxed)),
        dropped: STATS.dropped.load(Ordering::Relaxed),
        histogram: STATS.buckets.iter().map(|b| b.load(Ordering::Relaxed)).collect(),
    }
}

fn channel() -> &'static (Sender<InputEvent>, Receiver<InputEvent>) {
    static CH: OnceLock<(Sender<InputEvent>, Receiver<InputEvent>)> = OnceLock::new();
    CH.get_or_init(|| bounded(QUEUE_CAPACITY))
}

#[inline]
fn record(t0: i64) {
    let dt = clock::now() - t0;
    STATS.callbacks.fetch_add(1, Ordering::Relaxed);
    STATS.total_ticks.fetch_add(dt, Ordering::Relaxed);
    STATS.max_ticks.fetch_max(dt, Ordering::Relaxed);
    let us = clock::to_us(dt);
    for (i, limit) in BUCKETS_US.iter().enumerate() {
        if us < *limit {
            STATS.buckets[i].fetch_add(1, Ordering::Relaxed);
            break;
        }
    }
}

#[inline]
fn push(ev: InputEvent) {
    if channel().0.try_send(ev).is_err() {
        STATS.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let t0 = clock::now();
    if code == HC_ACTION as i32 {
        let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
        let (x, y) = (info.pt.x, info.pt.y);
        let injected = info.flags & LLMHF_INJECTED_BIT != 0;
        let t = t0;
        let ev = match wparam.0 as u32 {
            WM_LBUTTONDOWN => Some(InputEvent::MouseDown { button: MouseButton::Left, x, y, t, injected }),
            WM_RBUTTONDOWN => Some(InputEvent::MouseDown { button: MouseButton::Right, x, y, t, injected }),
            WM_MBUTTONDOWN => Some(InputEvent::MouseDown { button: MouseButton::Middle, x, y, t, injected }),
            WM_LBUTTONUP => Some(InputEvent::MouseUp { button: MouseButton::Left, x, y, t, injected }),
            WM_RBUTTONUP => Some(InputEvent::MouseUp { button: MouseButton::Right, x, y, t, injected }),
            WM_MBUTTONUP => Some(InputEvent::MouseUp { button: MouseButton::Middle, x, y, t, injected }),
            _ => None, // Mouse moves are frequent and not needed; they cost only this match.
        };
        if let Some(ev) = ev {
            push(ev);
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
        let (vk, scan) = (info.vkCode, info.scanCode);
        let ev = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(InputEvent::KeyDown { vk, scan, t: t0, injected }),
            WM_KEYUP | WM_SYSKEYUP => Some(InputEvent::KeyUp { vk, scan, t: t0, injected }),
            _ => None,
        };
        if let Some(ev) = ev {
            push(ev);
        }
    }
    record(t0);
    CallNextHookEx(None, code, wparam, lparam)
}

pub struct HookHandle {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

/// Install both hooks. Returns the handle and the event receiver.
pub fn start() -> Result<(HookHandle, Receiver<InputEvent>)> {
    clock::freq(); // Initialize before the callback can run.
    let rx = channel().1.clone();
    while rx.try_recv().is_ok() {} // Drain anything left from an earlier session.

    let (ready_tx, ready_rx) = bounded::<Result<u32, String>>(1);
    let join = thread::Builder::new().name("input-hooks".into()).spawn(move || unsafe {
        // All mouse and keyboard input on the machine waits on this thread.
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
        let tid = GetCurrentThreadId();
        let module = match GetModuleHandleW(None) {
            Ok(m) => m,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("GetModuleHandleW: {e}")));
                return;
            }
        };
        let inst = HINSTANCE(module.0);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(inst), 0);
        let keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(inst), 0);
        let (mouse, keyboard) = match (mouse, keyboard) {
            (Ok(m), Ok(k)) => (m, k),
            (m, k) => {
                let _ = ready_tx.send(Err(format!("SetWindowsHookExW: mouse={:?} keyboard={:?}", m.err(), k.err())));
                return;
            }
        };
        let _ = ready_tx.send(Ok(tid));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnhookWindowsHookEx(mouse);
        let _ = UnhookWindowsHookEx(keyboard);
    })?;

    let thread_id = ready_rx
        .recv()
        .map_err(|e| anyhow!("hook thread exited early: {e}"))?
        .map_err(|e| anyhow!(e))?;
    Ok((HookHandle { thread_id, join: Some(join) }, rx))
}

impl HookHandle {
    pub fn stop(mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

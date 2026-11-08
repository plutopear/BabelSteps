//! Lock, sleep and sign-out while recording (PSR-10).
//!
//! While the screen is locked or the machine sleeps, nothing the person does is
//! theirs to document, and the frames in the capture history are stale: the first
//! click after unlocking must not be given a picture from before the lock and
//! labelled "before the click". A hidden window on its own thread hears the
//! session and power messages and tells the capture loop, which then
//!
//! - records nothing while away (the hooks may or may not see lock-screen input),
//! - ends any typing run, and forgets which modifier keys it thought were held,
//! - on return, rebuilds capture, so the history starts empty and a click that
//!   beats the first fresh frame is flagged as not verified.
//!
//! The pure part, [`Away`], is separate so it can be tested without Windows messages.

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use std::thread::{self, JoinHandle};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, RegisterClassExW,
    TranslateMessage, MSG, WINDOW_EX_STYLE, WM_CLOSE, WNDCLASSEXW, WS_POPUP,
};

/// `WM_WTSSESSION_CHANGE` and the `wParam` values the recorder acts on.
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WTS_CONSOLE_CONNECT: usize = 1;
const WTS_CONSOLE_DISCONNECT: usize = 2;
const WTS_REMOTE_CONNECT: usize = 3;
const WTS_REMOTE_DISCONNECT: usize = 4;
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;
/// `WM_POWERBROADCAST` and its `wParam` values.
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMSUSPEND: usize = 0x4;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const PBT_APMRESUMESUSPEND: usize = 0x7;

/// What the system told us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    /// The screen locked, or this session left the console (fast user switching,
    /// a remote session took over).
    Locked,
    Unlocked,
    /// The machine is going to sleep.
    Suspend,
    /// The machine woke.
    Resume,
}

/// Whether the person is away: locked out or asleep. Either one is enough.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Away {
    locked: bool,
    asleep: bool,
}

impl Away {
    pub fn is_away(&self) -> bool {
        self.locked || self.asleep
    }

    /// Apply an event. `Some(true)` when this made the person away, `Some(false)`
    /// when this brought them back, `None` when nothing changed. Waking from sleep
    /// onto a lock screen stays away until the unlock.
    pub fn apply(&mut self, e: SessionEvent) -> Option<bool> {
        let before = self.is_away();
        match e {
            SessionEvent::Locked => self.locked = true,
            SessionEvent::Unlocked => self.locked = false,
            SessionEvent::Suspend => self.asleep = true,
            SessionEvent::Resume => self.asleep = false,
        }
        let after = self.is_away();
        (before != after).then_some(after)
    }
}

/// The recorder only needs the events that change `Away`; this decodes a window message.
fn decode(msg: u32, wparam: usize) -> Option<SessionEvent> {
    match (msg, wparam) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT) => Some(SessionEvent::Locked),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT) => Some(SessionEvent::Unlocked),
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => Some(SessionEvent::Suspend),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => Some(SessionEvent::Resume),
        _ => None,
    }
}

/// Events go to the sender of the thread that is running; one hidden window at a time.
static SINK: std::sync::Mutex<Option<(u64, Sender<(i64, SessionEvent)>)>> = std::sync::Mutex::new(None);
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub struct SessionWatcher {
    hwnd: isize,
    /// Which watcher owns `SINK`, so an old one dropping late cannot unplug a newer one.
    id: u64,
    join: Option<JoinHandle<()>>,
    /// Whether the system accepted the session notification (power messages need no registration).
    pub session_notices: bool,
}

/// Start listening. Events carry the time they were heard (QPC), so the capture
/// loop can line them up with input.
pub fn watch() -> Result<(SessionWatcher, Receiver<(i64, SessionEvent)>)> {
    let (tx, rx) = unbounded();
    let (ready_tx, ready_rx) = bounded::<Result<(isize, bool), String>>(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some((id, tx));
    let join = thread::Builder::new().name("session-watch".into()).spawn(move || unsafe { run(ready_tx) })?;
    let (hwnd, session_notices) = ready_rx
        .recv()
        .map_err(|_| anyhow!("the session watcher exited early"))?
        .map_err(|e| anyhow!(e))?;
    Ok((SessionWatcher { hwnd, id, join: Some(join), session_notices }, rx))
}

impl SessionWatcher {
    /// Self-test: deliver the same message the system sends, through the real window.
    #[cfg(feature = "selftest")]
    pub fn simulate(&self, e: SessionEvent) {
        let (msg, wparam) = match e {
            SessionEvent::Locked => (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK),
            SessionEvent::Unlocked => (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK),
            SessionEvent::Suspend => (WM_POWERBROADCAST, PBT_APMSUSPEND),
            SessionEvent::Resume => (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC),
        };
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(0));
        }
    }
}

impl Drop for SessionWatcher {
    fn drop(&mut self) {
        if let Some(j) = self.join.take() {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            let _ = j.join();
        }
        let mut sink = SINK.lock().unwrap_or_else(|e| e.into_inner());
        if sink.as_ref().is_some_and(|(id, _)| *id == self.id) {
            *sink = None;
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(e) = decode(msg, wparam.0) {
        if let Some((_, tx)) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send((crate::clock::now(), e));
        }
        return LRESULT(0);
    }
    if msg == WM_CLOSE {
        let _ = DestroyWindow(hwnd);
        windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn run(ready: Sender<Result<(isize, bool), String>>) {
    let fail = |m: &str| {
        let _ = ready.send(Err(m.to_string()));
    };
    let inst = match GetModuleHandleW(None) {
        Ok(h) => HINSTANCE(h.0),
        Err(_) => return fail("no module handle"),
    };
    let class = w!("BabelStepsSessionWatcher");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: inst,
        lpszClassName: class,
        ..Default::default()
    };
    let _ = RegisterClassExW(&wc); // A later recording registers again; "class exists" is fine.
    // A real top-level window that is never shown: power broadcasts do not reach
    // message-only windows.
    let hwnd = match CreateWindowExW(WINDOW_EX_STYLE(0), class, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst), None) {
        Ok(h) => h,
        Err(_) => return fail("could not create the session watcher window"),
    };
    // 0 is NOTIFY_FOR_THIS_SESSION.
    let session_notices = WTSRegisterSessionNotification(hwnd, 0).is_ok();
    let _ = ready.send(Ok((hwnd.0 as isize, session_notices)));

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    if session_notices {
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_and_unlock_make_one_change_each() {
        let mut a = Away::default();
        assert_eq!(a.apply(SessionEvent::Locked), Some(true));
        assert_eq!(a.apply(SessionEvent::Locked), None, "a repeat is not a change");
        assert!(a.is_away());
        assert_eq!(a.apply(SessionEvent::Unlocked), Some(false));
        assert!(!a.is_away());
    }

    #[test]
    fn waking_onto_a_lock_screen_stays_away_until_unlock() {
        let mut a = Away::default();
        assert_eq!(a.apply(SessionEvent::Locked), Some(true));
        assert_eq!(a.apply(SessionEvent::Suspend), None);
        assert_eq!(a.apply(SessionEvent::Resume), None, "still locked");
        assert!(a.is_away());
        assert_eq!(a.apply(SessionEvent::Unlocked), Some(false));
    }

    #[test]
    fn sleep_without_a_lock_returns_on_resume() {
        let mut a = Away::default();
        assert_eq!(a.apply(SessionEvent::Suspend), Some(true));
        assert_eq!(a.apply(SessionEvent::Resume), Some(false));
    }

    #[test]
    fn unlock_with_no_lock_is_nothing() {
        let mut a = Away::default();
        assert_eq!(a.apply(SessionEvent::Unlocked), None);
        assert_eq!(a.apply(SessionEvent::Resume), None);
    }

    #[test]
    fn messages_decode_to_events() {
        assert_eq!(decode(WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK), Some(SessionEvent::Locked));
        assert_eq!(decode(WM_WTSSESSION_CHANGE, WTS_CONSOLE_DISCONNECT), Some(SessionEvent::Locked));
        assert_eq!(decode(WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK), Some(SessionEvent::Unlocked));
        assert_eq!(decode(WM_POWERBROADCAST, PBT_APMSUSPEND), Some(SessionEvent::Suspend));
        assert_eq!(decode(WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC), Some(SessionEvent::Resume));
        assert_eq!(decode(WM_POWERBROADCAST, 0xA), None, "battery and AC changes are not ours");
        assert_eq!(decode(WM_WTSSESSION_CHANGE, 5), None, "logon alone changes nothing");
    }
}

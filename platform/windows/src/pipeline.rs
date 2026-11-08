//! The recording pipeline: four threads, so nothing slow sits in front of
//! anything that must be fast.
//!
//! ```text
//! input-hooks ──raw events──▶ capture ──events + frames──▶ analysis ──images, saves──▶ writer
//!  (timestamp,                (reads back the frame       (UI Automation, password     (PNG/JPEG, disk)
//!   queue only)                before each event)          check, redaction, steps)
//! ```
//!
//! - Capture never waits on analysis, so a 270 ms password scan cannot push the
//!   next click's frame past the history.
//! - Analysis redacts every frame before it leaves for the writer, and only
//!   frames whose step is final are written. A frame whose step is withheld,
//!   or that `core`'s normalizer discards, is dropped in memory and never
//!   reaches the disk.
//! - Frames in flight are capped, so a 5120x1440 screen cannot exhaust memory.

use crate::capture::{CaptureStats, Capturer, Grabbed};
use crate::clock;
use crate::frame::Frame;
use crate::hooks::{self, HookHandle, HookStats, RawEvent};
use crate::hotkeys::Hotkey;
use crate::keys::KeyTracker;
use crate::monitors;
use crate::scan::{self, PasswordMemory, WinState};
use crate::scope::{self, Scope};
use crate::uia::{ElementInfo, UiaFaults, UiaPool, UiaStats};
use anyhow::{bail, Result};
use babelsteps_core::normalize::{Event, Key, Mods, Normalizer, NormalizerConfig};
use babelsteps_core::pii::CustomRules;
use babelsteps_core::screenshot::{Effort, ShotFormat};
use babelsteps_core::storage::{Bundle, IMAGES_DIR};
use babelsteps_core::{Block, MouseButton, Recording, Rect, Target};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, WindowFromPoint, GA_ROOT,
};

/// Bytes of frames allowed between capture and disk at once: 6 frames of a
/// 5120x1440 screen, 24 of a 1920x1080 one. Capture never waits for room: a
/// late grab would show the wrong moment. Over budget, the step gets no image.
const FRAME_BUDGET_BYTES: usize = 200 * 1024 * 1024;
const LOOKUP_TIMEOUT: Duration = Duration::from_millis(1500);
const SCAN_TIMEOUT: Duration = Duration::from_millis(3000);
/// While typing, the focused window's scan is reused for this long.
const KEY_SCAN_REUSE_MS: f64 = 1000.0;
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// Every visible window is rescanned this often, so the memory of password
/// fields is fresh when a window changes right after an input.
const REFRESH_EVERY: Duration = Duration::from_secs(5);

pub const NOTE_NO_SCREENSHOT: &str = "Screenshot unavailable.";
pub const NOTE_NOT_VERIFIED: &str = "This screenshot may show the moment after the action.";

/// Adds a note after any description already there (such as "Text not recorded").
fn add_note(s: &mut babelsteps_core::StepBlock, note: &str) {
    if s.description.is_empty() {
        s.description = note.to_string();
    } else {
        s.description = format!("{} {note}", s.description);
    }
}

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    /// Q4: off by default. When off, typed text is never buffered.
    pub keystrokes_enabled: bool,
    pub custom: CustomRules,
    /// Act only on synthetic input. The self-test sets this, so whatever the
    /// person at the machine does is neither recorded nor counted.
    pub only_injected: bool,
    /// Ignore clicks on this process's windows and keys typed into them, as
    /// Steps Recorder ignores its own window. The self-test turns this off,
    /// because its test window lives in the same process.
    pub ignore_own_windows: bool,
    /// Key combinations never recorded: the app's own hotkeys.
    pub ignore_keys: Vec<Hotkey>,
    /// Screen areas whose clicks are never recorded: the app's own tray icon,
    /// which belongs to the taskbar rather than to this process. The app keeps
    /// it up to date while recording.
    pub ignore_areas: Option<Arc<Mutex<Vec<Rect>>>>,
    /// Clicks on taskbar elements whose name starts with this are never
    /// recorded (the tray icon in the hidden-icons flyout, named after its tooltip).
    pub ignore_shell_elements_named: Option<String>,
    /// A6: the whole screen (default), one monitor, or one window. Input
    /// outside it is not recorded, and window screenshots are cut to the window.
    pub scope: Scope,
    /// A3 opt-in, off by default: read the window title and name it in click
    /// steps. When off, no title is read at all.
    pub show_window_titles: bool,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        RecorderConfig {
            keystrokes_enabled: false,
            custom: CustomRules::none(),
            only_injected: false,
            ignore_own_windows: true,
            ignore_keys: Vec::new(),
            ignore_areas: None,
            ignore_shell_elements_named: None,
            scope: Scope::Screen,
            show_window_titles: false,
        }
    }
}

/// What the capture thread needs to know from the config.
#[derive(Clone)]
struct CaptureOpts {
    only_injected: bool,
    keystrokes_enabled: bool,
    ignore_own_windows: bool,
    ignore_keys: Vec<Hotkey>,
    ignore_areas: Option<Arc<Mutex<Vec<Rect>>>>,
    scope: Scope,
}

/// Fault injection for the self-test. Inert unless something sets it.
#[derive(Default)]
pub struct Faults {
    pub uia: Arc<UiaFaults>,
    /// Every password scan fails.
    pub scan_fail: AtomicBool,
    /// Added before analysing each event, to prove analysis cannot delay capture.
    pub analysis_delay_ms: AtomicU64,
    /// Capture with GDI only, to prove "not verified" steps are marked.
    pub force_gdi: AtomicBool,
}

#[derive(Debug, Clone, Default)]
pub struct RecorderStatus {
    pub steps: usize,
    pub paused: bool,
    pub withheld: usize,
    /// Steps whose screenshot may show the moment after the action.
    pub not_verified: usize,
    /// Events recorded without a screenshot (no frame, or no free slot).
    pub no_screenshot: usize,
    /// Bytes of frames waiting for analysis or the disk.
    pub frame_bytes_in_flight: usize,
    /// Frames not taken because the budget was full.
    pub frames_over_budget: u64,
    /// Slowest analysis of one event so far, in ms (lookups, scan, redaction).
    pub analysis_ms_max: f64,
    pub analysis_ms_mean: f64,
    /// Windows searched by the last full password scan, and windows skipped as hidden.
    pub last_scan_windows: usize,
    pub last_scan_hidden: usize,
    /// Events analysed so far (clicks and recorded keys).
    pub events: usize,
    pub password_fields_remembered: usize,
    pub capture: CaptureStats,
    /// Continuous capture is running, so screenshots can show the state before each action.
    pub before_state_available: bool,
    pub uia: UiaStats,
    /// Set when the chosen window has closed or the chosen monitor is gone.
    pub scope_lost: Option<String>,
    /// Things the user should know, such as "screenshots withheld: a window could not be checked".
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct RecorderReport {
    pub recording: Recording,
    pub status: RecorderStatus,
    pub hooks: HookStats,
    pub write_errors: Vec<String>,
    pub elapsed: Duration,
    /// Clicks and key presses lost during this recording because the hook
    /// queue was full (PSR-5 "missed"). `hooks.dropped` counts since the app started.
    pub missed_inputs: u64,
}

enum Control {
    Pause,
    Resume,
    Comment(String),
    Manual(String),
    Stop,
}

/// Returns its bytes to the budget when dropped, whichever path the frame takes.
struct Ticket(Arc<AtomicUsize>, usize);
impl Drop for Ticket {
    fn drop(&mut self) {
        self.0.fetch_sub(self.1, Ordering::SeqCst);
    }
}

struct Shot {
    frame: Frame,
    before_verified: bool,
    /// Windows visible on the frame's monitor when it was grabbed.
    windows: Vec<WinState>,
    hidden: usize,
    _ticket: Ticket,
}

enum Captured {
    Click { button: MouseButton, x: i32, y: i32, t: i64, shot: Option<Shot> },
    Key { key: Key, mods: Mods, t: i64, shot: Option<Shot> },
    Foreground { hwnd: isize, t: i64 },
    Control(Control),
}

enum WriteJob {
    Image { name: String, frame: Frame, _ticket: Ticket },
    Save(Box<Recording>),
    Finish(Sender<(Bundle, Vec<String>)>),
}

pub struct Recorder {
    /// Control messages carry the time they were sent, so they line up with input.
    ctrl: Sender<(i64, Control)>,
    hooks: Arc<Mutex<Option<HookHandle>>>,
    status: Arc<Mutex<RecorderStatus>>,
    #[cfg_attr(not(feature = "selftest"), allow(dead_code))]
    faults: Arc<Faults>,
    capture: Option<JoinHandle<()>>,
    analysis: Option<JoinHandle<Recording>>,
    writer_done: Receiver<(Bundle, Vec<String>)>,
    started: Instant,
    /// `hooks::stats().dropped` when this recording started.
    dropped_at_start: u64,
}

impl Recorder {
    /// Start recording into `bundle`, appending steps to `recording`.
    pub fn start(bundle: Bundle, recording: Recording, config: RecorderConfig) -> Result<Recorder> {
        monitors::make_thread_dpi_aware();
        if !monitors::per_monitor_dpi_aware() {
            // Hook points, element bounds and frames would disagree, and black
            // boxes would miss the password fields.
            bail!("recording needs per-monitor DPI awareness, which this process does not have");
        }
        scope::validate(&config.scope, config.ignore_own_windows)?;
        let faults = Arc::new(Faults::default());
        let status = Arc::new(Mutex::new(RecorderStatus::default()));
        let in_flight = Arc::new(AtomicUsize::new(0));

        let (write_tx, write_rx) = unbounded::<WriteJob>();
        let (done_tx, done_rx) = bounded::<(Bundle, Vec<String>)>(1);
        let writer_budget = in_flight.clone();
        thread::Builder::new().name("step-writer".into()).spawn(move || writer_loop(bundle, write_rx, writer_budget))?;

        let opts = CaptureOpts {
            only_injected: config.only_injected,
            keystrokes_enabled: config.keystrokes_enabled,
            ignore_own_windows: config.ignore_own_windows,
            ignore_keys: config.ignore_keys.clone(),
            ignore_areas: config.ignore_areas.clone(),
            scope: config.scope.clone(),
        };
        let (cap_tx, cap_rx) = unbounded::<Captured>();
        let analysis = {
            let (faults, status) = (faults.clone(), status.clone());
            let write_tx = write_tx.clone();
            thread::Builder::new().name("step-analysis".into()).spawn(move || {
                let mut a = Analysis::new(config, recording, faults, status, write_tx);
                a.run(cap_rx);
                let _ = a.write.send(WriteJob::Save(Box::new(a.recording.clone())));
                let _ = a.write.send(WriteJob::Finish(done_tx));
                a.recording
            })?
        };
        drop(write_tx);

        let dropped_at_start = hooks::stats().dropped;
        let (hook_handle, raw_rx) = match hooks::start() {
            Ok(h) => h,
            Err(e) => {
                drop(cap_tx); // Lets analysis and the writer finish.
                let _ = analysis.join();
                return Err(e);
            }
        };
        let hooks = Arc::new(Mutex::new(Some(hook_handle)));
        let (ctrl_tx, ctrl_rx) = unbounded::<(i64, Control)>();
        let capture = {
            let (faults, status, hooks) = (faults.clone(), status.clone(), hooks.clone());
            thread::Builder::new().name("step-capture".into()).spawn(move || {
                capture_loop(raw_rx, ctrl_rx, cap_tx, hooks, faults, status, in_flight, opts);
            })?
        };
        Ok(Recorder {
            ctrl: ctrl_tx,
            hooks,
            status,
            faults,
            capture: Some(capture),
            analysis: Some(analysis),
            writer_done: done_rx,
            started: Instant::now(),
            dropped_at_start,
        })
    }

    fn send(&self, c: Control) {
        let _ = self.ctrl.send((clock::now(), c));
    }
    pub fn pause(&self) {
        self.send(Control::Pause);
    }
    pub fn resume(&self) {
        self.send(Control::Resume);
    }
    pub fn comment(&self, text: &str) {
        self.send(Control::Comment(text.to_string()));
    }
    pub fn manual_step(&self, title: &str) {
        self.send(Control::Manual(title.to_string()));
    }

    pub fn status(&self) -> RecorderStatus {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Fault injection, for the self-test only.
    #[cfg(feature = "selftest")]
    pub fn faults(&self) -> &Faults {
        &self.faults
    }

    /// Stop, finish every step, write the remaining images, and save. Returns the
    /// bundle so the caller can keep using it.
    pub fn stop(mut self) -> Result<(Bundle, RecorderReport)> {
        // Stop the hooks first, so no input arrives after the stop request.
        if let Some(h) = self.hooks.lock().unwrap_or_else(|e| e.into_inner()).take() {
            h.stop();
        }
        self.send(Control::Stop);
        if let Some(c) = self.capture.take() {
            let _ = c.join();
        }
        let recording = match self.analysis.take().map(JoinHandle::join) {
            Some(Ok(r)) => r,
            _ => bail!("the analysis thread failed"),
        };
        let (bundle, write_errors) = self
            .writer_done
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| anyhow::anyhow!("the image writer did not finish"))?;
        let hook_stats = hooks::stats();
        let report = RecorderReport {
            recording,
            status: self.status(),
            missed_inputs: hook_stats.dropped.saturating_sub(self.dropped_at_start),
            hooks: hook_stats,
            write_errors,
            elapsed: self.started.elapsed(),
        };
        Ok((bundle, report))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Dropped without `stop`: still remove the hooks and let threads end.
        if let Some(h) = self.hooks.lock().unwrap_or_else(|e| e.into_inner()).take() {
            h.stop();
        }
        self.send(Control::Stop);
    }
}

fn own_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

fn window_pid(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn is_own_window_at(x: i32, y: i32) -> bool {
    window_pid(unsafe { WindowFromPoint(POINT { x, y }) }) == own_pid()
}

fn root_at(x: i32, y: i32) -> isize {
    unsafe { GetAncestor(WindowFromPoint(POINT { x, y }), GA_ROOT) }.0 as isize
}

/// Caption of a top-level window, `None` when it has none. For another
/// process's window this reads the stored caption without sending it a
/// message, so a hung app cannot stall analysis.
fn window_title(hwnd: isize) -> Option<String> {
    if hwnd == 0 {
        return None;
    }
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(HWND(hwnd as *mut _), &mut buf) };
    let t = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
    (!t.trim().is_empty()).then_some(t)
}

fn foreground_is_own() -> bool {
    window_pid(unsafe { GetForegroundWindow() }) == own_pid()
}

#[allow(clippy::too_many_arguments)]
fn capture_loop(
    raw: Receiver<RawEvent>,
    ctrl: Receiver<(i64, Control)>,
    out: Sender<Captured>,
    hooks: Arc<Mutex<Option<HookHandle>>>,
    faults: Arc<Faults>,
    status: Arc<Mutex<RecorderStatus>>,
    in_flight: Arc<AtomicUsize>,
    opts: CaptureOpts,
) {
    monitors::make_thread_dpi_aware();
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
    let mut capturer = Capturer::start();
    let mut keys = KeyTracker::new();
    let mut paused = false;
    let mut last_was_text = false;
    let mut last_check = Instant::now();
    // An input event read ahead while lining up a control message.
    let mut held: Option<RawEvent> = None;

    let over_budget = Arc::new(AtomicU64::new(0));
    let over = over_budget.clone();
    let budget = in_flight.clone();
    let mut grab = move |capturer: &mut Capturer, x: i32, y: i32, t: i64, keep: Option<Vec<Rect>>| -> Option<Shot> {
        let mons = capturer.monitors();
        let bytes = monitors::index_for(mons, x, y).map_or(0, |i| mons[i].w.max(0) as usize * mons[i].h.max(0) as usize * 4);
        if budget.load(Ordering::SeqCst) + bytes > FRAME_BUDGET_BYTES {
            over.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        capturer.force_gdi = faults.force_gdi.load(Ordering::Relaxed);
        budget.fetch_add(bytes, Ordering::SeqCst);
        let ticket = Ticket(budget.clone(), bytes);
        match capturer.grab(x, y, t) {
            Ok(Grabbed { frame, before_verified }) => {
                // One window (A6): cut the frame down before anything else
                // sees it, so the scan and the writer only get the window.
                let frame = match keep {
                    Some(areas) => {
                        // No area (the window vanished): the step has no picture.
                        let first = *areas.first()?;
                        let bbox = areas.iter().fold(first, |a, r| a.union(r));
                        let mut f = frame.crop(&bbox)?;
                        if areas.len() > 1 {
                            f.keep_only(&areas);
                        }
                        f
                    }
                    None => frame,
                };
                // The windows on screen now, a few ms after the input, which is
                // as close to the frame as can be had.
                let area = frame_area(&frame);
                let moved = [unsafe { GetForegroundWindow() }.0 as isize, root_at(x, y)];
                let (windows, hidden) = scan::snapshot(area, &moved);
                Some(Shot { frame, before_verified, windows, hidden, _ticket: ticket })
            }
            Err(_) => None,
        }
    };

    loop {
        if last_check.elapsed() > Duration::from_secs(1) {
            last_check = Instant::now();
            if let Some(h) = hooks.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                h.check_alive();
            }
            publish_capture(&status, &capturer, &in_flight, &over_budget);
            let lost = scope::lost(&opts.scope, capturer.monitors()).map(str::to_string);
            status.lock().unwrap_or_else(|e| e.into_inner()).scope_lost = lost;
        }
        if let Some(ev) = held.take() {
            handle_raw(ev, paused, &opts, &mut keys, &mut last_was_text, &mut capturer, &mut grab, &out);
            continue;
        }
        let ev = select! {
            recv(raw) -> ev => match ev { Ok(e) => e, Err(_) => break },
            recv(ctrl) -> c => {
                let Ok((sent, c)) = c else { break };
                let stop = matches!(c, Control::Stop);
                // Input from before this message comes first: a click just before
                // Pause is recorded, and a comment lands between the right steps.
                while let Ok(ev) = raw.try_recv() {
                    if ev.t() <= sent || stop {
                        handle_raw(ev, paused, &opts, &mut keys, &mut last_was_text, &mut capturer, &mut grab, &out);
                    } else {
                        held = Some(ev);
                        break;
                    }
                }
                match c {
                    Control::Pause => paused = true,
                    Control::Resume => paused = false,
                    _ => {}
                }
                if stop {
                    // Hooks are already removed; everything they queued is handled.
                    publish_capture(&status, &capturer, &in_flight, &over_budget);
                    let _ = out.send(Captured::Control(Control::Stop));
                    return;
                }
                let _ = out.send(Captured::Control(c));
                continue;
            },
            default(Duration::from_millis(250)) => continue,
        };
        handle_raw(ev, paused, &opts, &mut keys, &mut last_was_text, &mut capturer, &mut grab, &out);
    }
    let _ = out.send(Captured::Control(Control::Stop));
}

fn publish_capture(status: &Mutex<RecorderStatus>, capturer: &Capturer, in_flight: &AtomicUsize, over_budget: &AtomicU64) {
    let mut st = status.lock().unwrap_or_else(|e| e.into_inner());
    st.capture = capturer.stats.clone();
    st.capture.wgc_frames = capturer.frames_received();
    st.before_state_available = capturer.wgc_running();
    st.frame_bytes_in_flight = in_flight.load(Ordering::SeqCst);
    st.frames_over_budget = over_budget.load(Ordering::Relaxed);
}

#[allow(clippy::too_many_arguments)]
fn handle_raw(
    ev: RawEvent,
    paused: bool,
    opts: &CaptureOpts,
    keys: &mut KeyTracker,
    last_was_text: &mut bool,
    capturer: &mut Capturer,
    grab: &mut impl FnMut(&mut Capturer, i32, i32, i64, Option<Vec<Rect>>) -> Option<Shot>,
    out: &Sender<Captured>,
) {
    match ev {
        RawEvent::MouseDown { button, x, y, t, injected } => {
            let in_ignored_area = opts
                .ignore_areas
                .as_ref()
                .is_some_and(|a| scan::point_in(x, y, &a.lock().unwrap_or_else(|e| e.into_inner())));
            if (opts.only_injected && !injected) || paused || in_ignored_area || (opts.ignore_own_windows && is_own_window_at(x, y)) {
                return;
            }
            let crop = match &opts.scope {
                Scope::Screen => None,
                Scope::Monitor(device) => {
                    let mons = capturer.monitors();
                    if !monitors::index_for(mons, x, y).is_some_and(|i| &mons[i].device_name() == device) {
                        *last_was_text = false;
                        return; // On another monitor: not recorded.
                    }
                    None
                }
                Scope::Window(target) => {
                    let root = root_at(x, y);
                    if !scope::window_in_scope(root, *target) {
                        *last_was_text = false;
                        return; // On another window: not recorded.
                    }
                    Some(scope::keep_areas(*target, Some(root)))
                }
            };
            *last_was_text = false;
            let shot = grab(capturer, x, y, t, crop);
            let _ = out.send(Captured::Click { button, x, y, t, shot });
        }
        RawEvent::KeyUp { vk, scan, injected, .. } => {
            if !(opts.only_injected && !injected) {
                keys.key_up(vk, scan);
            }
        }
        RawEvent::KeyDown { vk, scan, extended, t, injected } => {
            if opts.only_injected && !injected {
                return;
            }
            let Some((key, mods)) = keys.key_down(vk, scan, extended) else { return };
            let is_hotkey = opts
                .ignore_keys
                .iter()
                .any(|h| h.vk == vk && h.ctrl == mods.ctrl && h.alt == mods.alt && h.shift == mods.shift && h.win == mods.meta);
            if is_hotkey || paused || (opts.ignore_own_windows && foreground_is_own()) {
                return;
            }
            let crop = match &opts.scope {
                Scope::Screen => None,
                Scope::Monitor(device) => {
                    let mons = capturer.monitors();
                    let on = focus_point(mons).and_then(|(x, y)| monitors::index_for(mons, x, y)).is_some_and(|i| &mons[i].device_name() == device);
                    if !on {
                        *last_was_text = false; // Back in scope, the first key gets a frame.
                        return;
                    }
                    None
                }
                Scope::Window(target) => {
                    let fg = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) }.0 as isize;
                    if !scope::window_in_scope(fg, *target) {
                        *last_was_text = false;
                        return; // Typed into another window: not recorded.
                    }
                    Some(scope::keep_areas(*target, Some(fg)))
                }
            };
            let shortcut = mods.ctrl || mods.alt || mods.meta;
            let is_text = matches!(key, Key::Text(_)) && !shortcut;
            // A frame only for keys that can start a step: the first key of a run
            // of text, a shortcut, or (with keystrokes on) a named key. Unused
            // frames never reach disk, but each one costs a full-screen readback.
            let wants_frame = if is_text {
                !*last_was_text
            } else if shortcut {
                true
            } else {
                opts.keystrokes_enabled && !matches!(key, Key::Backspace)
            };
            *last_was_text = is_text;
            let shot = if wants_frame { grab_focus(capturer, grab, t, crop) } else { None };
            let _ = out.send(Captured::Key { key, mods, t, shot });
        }
        RawEvent::Foreground { hwnd, t } => {
            *last_was_text = false;
            if opts.only_injected {
                return; // Not synthetic: it is the person at the machine switching windows.
            }
            if !opts.ignore_own_windows || window_pid(HWND(hwnd as *mut _)) != own_pid() {
                let _ = out.send(Captured::Foreground { hwnd, t });
            }
        }
    }
}

/// The frame for a key press: the monitor holding the foreground window.
fn grab_focus(
    capturer: &mut Capturer,
    grab: &mut impl FnMut(&mut Capturer, i32, i32, i64, Option<Vec<Rect>>) -> Option<Shot>,
    t: i64,
    keep: Option<Vec<Rect>>,
) -> Option<Shot> {
    let point = focus_point(capturer.monitors())?;
    grab(capturer, point.0, point.1, t, keep)
}

/// A point on the monitor that holds the foreground window: its centre, or,
/// when the centre is off-screen, the centre of its part on the monitor it
/// overlaps most. `None` if it is on no monitor.
fn focus_point(monitors: &[monitors::Monitor]) -> Option<(i32, i32)> {
    let mut r = windows::Win32::Foundation::RECT::default();
    let fg = unsafe { GetForegroundWindow() };
    if unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(fg, &mut r) }.is_err() {
        return None;
    }
    let cx = ((i64::from(r.left) + i64::from(r.right)) / 2) as i32;
    let cy = ((i64::from(r.top) + i64::from(r.bottom)) / 2) as i32;
    let mons: Vec<_> = monitors.iter().copied().filter(|m| m.w > 0 && m.h > 0).collect();
    if monitors::index_for(&mons, cx, cy).is_some() {
        return Some((cx, cy));
    }
    let win = Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top };
    let part = mons
        .iter()
        .filter_map(|m| win.intersect(&Rect { x: m.x, y: m.y, w: m.w, h: m.h }))
        .max_by_key(|p| i64::from(p.w) * i64::from(p.h))?;
    Some((part.x + part.w / 2, part.y + part.h / 2))
}

struct KeyScan {
    hwnd: isize,
    at: i64,
    rects: Vec<Rect>,
}

/// Processes that draw the taskbar and its tray flyouts.
const SHELL_PROCESSES: [&str; 3] = ["explorer", "ShellExperienceHost", "ShellHost"];

struct Analysis {
    ignore_shell_named: Option<String>,
    show_window_titles: bool,
    norm: Normalizer,
    recording: Recording,
    uia: UiaPool,
    memory: PasswordMemory,
    faults: Arc<Faults>,
    status: Arc<Mutex<RecorderStatus>>,
    write: Sender<WriteJob>,
    pending: HashMap<String, Shot>,
    next_image: u64,
    id_prefix: String,
    start_qpc: i64,
    start_unix_ms: u64,
    ms_per_tick: f64,
    dirty: bool,
    last_save: Instant,
    key_scan: Option<KeyScan>,
    timing: Timing,
    last_refresh: Instant,
    counts: NoteCounts,
}

#[derive(Default)]
struct NoteCounts {
    not_verified: usize,
    no_screenshot: usize,
}

#[derive(Default)]
struct Timing {
    events: u64,
    total_ms: f64,
    max_ms: f64,
    scan_windows: usize,
    scan_hidden: usize,
}

impl Analysis {
    fn new(
        config: RecorderConfig,
        recording: Recording,
        faults: Arc<Faults>,
        status: Arc<Mutex<RecorderStatus>>,
        write: Sender<WriteJob>,
    ) -> Self {
        let start_unix_ms = clock::unix_ms();
        Analysis {
            ignore_shell_named: config.ignore_shell_elements_named.clone(),
            show_window_titles: config.show_window_titles,
            norm: Normalizer::new(NormalizerConfig {
                keystrokes_enabled: config.keystrokes_enabled,
                custom: config.custom,
                show_window_titles: config.show_window_titles,
            }),
            recording,
            uia: UiaPool::new(faults.uia.clone()),
            memory: PasswordMemory::default(),
            faults,
            status,
            write,
            pending: HashMap::new(),
            next_image: 1,
            // Steps from different recording sessions in one document never share ids.
            id_prefix: format!("r{start_unix_ms:x}-"),
            start_qpc: clock::now(),
            start_unix_ms,
            ms_per_tick: 1000.0 / clock::freq() as f64,
            dirty: false,
            last_save: Instant::now(),
            key_scan: None,
            timing: Timing::default(),
            last_refresh: Instant::now(),
            counts: NoteCounts::default(),
        }
    }

    fn unix_ms(&self, t: i64) -> u64 {
        let delta = ((t - self.start_qpc) as f64 * self.ms_per_tick) as i64;
        (self.start_unix_ms as i64 + delta).max(0) as u64
    }

    fn run(&mut self, rx: Receiver<Captured>) {
        monitors::make_thread_dpi_aware();
        self.refresh_all(); // Know every window's fields before the first input.
        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Captured::Control(Control::Stop)) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                Ok(c) => {
                    let delay = self.faults.analysis_delay_ms.load(Ordering::Relaxed);
                    if delay > 0 && !matches!(c, Captured::Control(_)) {
                        thread::sleep(Duration::from_millis(delay));
                    }
                    self.handle(c);
                }
                Err(_) => {}
            }
            // Also during steady input, so remembered fields never go stale.
            if self.last_refresh.elapsed() > REFRESH_EVERY {
                self.refresh_all();
            }
            if self.dirty && self.last_save.elapsed() > SAVE_EVERY {
                self.save();
            }
        }
        let blocks = self.norm.finish();
        self.emit(blocks);
        // Anything still pending belongs to no step: dropped, never written.
        self.pending.clear();
        self.publish();
    }

    fn save(&mut self) {
        let _ = self.write.send(WriteJob::Save(Box::new(self.recording.clone())));
        self.dirty = false;
        self.last_save = Instant::now();
    }

    fn handle(&mut self, c: Captured) {
        let started = Instant::now();
        let input = matches!(c, Captured::Click { .. } | Captured::Key { .. });
        match c {
            Captured::Click { button, x, y, t, shot } => self.on_click(button, x, y, t, shot),
            Captured::Key { key, mods, t, shot } => self.on_key(key, mods, t, shot),
            Captured::Foreground { hwnd, t } => {
                let blocks = self.norm.push(Event::FocusChanged { t_ms: self.unix_ms(t) });
                self.emit(blocks);
                // Asking early also makes browsers build their accessibility
                // tree before the first click.
                if let Some(w) = scan::current_state(hwnd) {
                    let _ = self.scan_windows(&[w], t);
                }
            }
            Captured::Control(Control::Pause) => {
                let blocks = self.norm.push(Event::Pause);
                self.emit(blocks);
                self.status.lock().unwrap_or_else(|e| e.into_inner()).paused = true;
            }
            Captured::Control(Control::Resume) => {
                let blocks = self.norm.push(Event::Resume);
                self.emit(blocks);
                self.status.lock().unwrap_or_else(|e| e.into_inner()).paused = false;
            }
            Captured::Control(Control::Comment(text)) => {
                let t_ms = clock::unix_ms();
                let blocks = self.norm.push(Event::Comment { t_ms, text });
                self.emit(blocks);
            }
            Captured::Control(Control::Manual(title)) => {
                let t_ms = clock::unix_ms();
                let blocks = self.norm.push(Event::ManualStep { t_ms, title });
                self.emit(blocks);
            }
            Captured::Control(Control::Stop) => {}
        }
        if input {
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            self.timing.events += 1;
            self.timing.total_ms += ms;
            self.timing.max_ms = self.timing.max_ms.max(ms);
        }
        self.publish();
    }

    /// Scan these windows as they are now and remember each one's fields.
    /// False if the scan failed or timed out.
    fn scan_windows(&mut self, windows: &[WinState], t: i64) -> bool {
        if self.faults.scan_fail.load(Ordering::Relaxed) {
            return false;
        }
        if windows.is_empty() {
            return true;
        }
        let list: Vec<isize> = windows.iter().map(|w| w.hwnd).collect();
        let Some(found) = self.uia.call(SCAN_TIMEOUT, move |u| u.password_rects(&list)) else { return false };
        let when = t.max(clock::now());
        for (hwnd, fields) in found {
            if let Some(w) = windows.iter().find(|w| w.hwnd == hwnd) {
                self.memory.remember_window(w, &fields, when);
            }
        }
        true
    }

    /// Every visible window on every monitor, while idle and at the start.
    fn refresh_all(&mut self) {
        self.last_refresh = Instant::now();
        for m in monitors::enumerate() {
            let area = Rect { x: m.x, y: m.y, w: m.w, h: m.h };
            let (windows, _) = scan::snapshot(area, &[]);
            let _ = self.scan_windows(&windows, clock::now());
        }
    }

    /// Fields to black out in a frame whose windows were `snap`, plus everything
    /// remembered around `t`. `None` (unknown) if a scan failed, or a window that
    /// changed since the frame has no scan from its old position.
    ///
    /// Windows that appeared since the frame are scanned too, so that a menu or
    /// dialog opened by this input is known when the next input closes it.
    fn scan_frame(&mut self, snap: &[WinState], hidden: usize, area: Rect, point: Option<(i32, i32)>, t: i64) -> Option<Vec<Rect>> {
        self.timing.scan_windows = snap.len();
        self.timing.scan_hidden = hidden;
        let (mut live, changed) = scan::split_changed(snap, scan::current_state);
        // Before this scan replaces them: what every recent window's fields were.
        let mut extra = self.memory.recent_window_fields(&area, t, self.ms_per_tick);
        for w in &changed {
            extra.extend(self.memory.window_fields(w, t, self.ms_per_tick)?);
        }
        let mut moved = vec![unsafe { GetForegroundWindow() }.0 as isize];
        if let Some((x, y)) = point {
            moved.push(root_at(x, y));
        }
        for w in scan::snapshot(area, &moved).0 {
            if !live.iter().any(|l| l.hwnd == w.hwnd) {
                live.push(w);
            }
        }
        if !self.scan_windows(&live, t) {
            return None;
        }
        let mut all = self.memory.around(t, self.ms_per_tick);
        all.extend(extra);
        Some(all)
    }

    /// The scan for an event without a frame: only its signals need it.
    fn scan_now(&mut self, area: Rect, t: i64, point: Option<(i32, i32)>) -> Option<Vec<Rect>> {
        let mut moved = vec![unsafe { GetForegroundWindow() }.0 as isize];
        if let Some((x, y)) = point {
            moved.push(root_at(x, y));
        }
        let (windows, hidden) = scan::snapshot(area, &moved);
        self.scan_frame(&windows, hidden, area, point, t)
    }

    fn redact_and_keep(
        &mut self,
        mut shot: Shot,
        rects: &[Rect],
        reported: &[Rect],
        outline: Option<Rect>,
        marker: Option<(i32, i32)>,
    ) -> String {
        for r in rects.iter().chain(reported) {
            shot.frame.black_out(r);
        }
        if let Some(o) = outline {
            shot.frame.outline(&o);
        }
        if let Some((x, y)) = marker {
            shot.frame.draw_click_marker(x, y);
        }
        // PNG for UI, JPEG for photo-like screens (PSR-1). Decided here, after the
        // black-out, because the name is part of the step before the writer runs.
        let ext = shot.frame.stored_format().extension();
        let name = format!("shot-{}{:06}.{ext}", self.id_prefix, self.next_image);
        self.next_image += 1;
        let path = format!("{IMAGES_DIR}/{name}");
        self.pending.insert(path.clone(), shot);
        path
    }

    fn on_click(&mut self, button: MouseButton, x: i32, y: i32, t: i64, shot: Option<Shot>) {
        let lookups = self.uia.call(LOOKUP_TIMEOUT, move |u| Ok((u.element_at(x, y).ok(), u.focused().ok())));
        let (element, focused) = lookups.unwrap_or((None, None));
        if let (Some(prefix), Some(e)) = (&self.ignore_shell_named, &element) {
            let shell = SHELL_PROCESSES.iter().any(|p| e.process_name.eq_ignore_ascii_case(p));
            if shell && e.name.starts_with(prefix.as_str()) {
                return; // A click on our own tray icon; its frame is dropped unwritten.
            }
        }
        let scanned = match (&shot, monitor_area_at(x, y)) {
            (Some(s), _) => self.scan_frame(&s.windows.clone(), s.hidden, frame_area(&s.frame), Some((x, y)), t),
            (None, Some(a)) => self.scan_now(a, t, Some((x, y))),
            (None, None) => None,
        };
        let signals = scan::signals(element.as_ref(), focused.as_ref(), scanned.as_deref(), Some((x, y)));
        let had_frame = shot.is_some();
        let screenshot = match (shot, &scanned) {
            (Some(s), Some(rects)) if signals.screenshot_allowed() => {
                let reported = reported_bounds(&[element.as_ref(), focused.as_ref()]);
                let outline = element.as_ref().filter(|e| e.is_password == Some(false)).and_then(|e| e.bounds);
                Some(self.redact_and_keep(s, rects, &reported, outline, Some((x, y))))
            }
            _ => None, // Dropped here: a withheld frame is never written.
        };
        // Read now, next to the element lookup above, so both describe the same moment.
        let window_title = self.show_window_titles.then(|| window_title(root_at(x, y))).flatten();
        let target = element.as_ref().map(|e| to_target(e, window_title));
        let event = Event::Click { t_ms: self.unix_ms(t), button, x, y, target, signals, screenshot };
        let mut blocks = self.norm.push(event);
        if !had_frame {
            if let Some(Block::Step(s)) = blocks.last_mut() {
                if s.screenshot.is_none() && !s.screenshot_withheld {
                    add_note(s, NOTE_NO_SCREENSHOT);
                    self.counts.no_screenshot += 1;
                }
            }
        }
        self.emit(blocks);
    }

    fn on_key(&mut self, key: Key, mods: Mods, t: i64, shot: Option<Shot>) {
        let lookups = self.uia.call(LOOKUP_TIMEOUT, move |u| {
            let focused = u.focused().ok();
            // A second, independent lookup: the element at the focused element's centre.
            let element = focused
                .as_ref()
                .and_then(|f| f.bounds)
                .and_then(|b| u.element_at(b.x + b.w / 2, b.y + b.h / 2).ok());
            Ok((element, focused))
        });
        let (element, focused) = lookups.unwrap_or((None, None));
        let point = focused.as_ref().and_then(|f| f.bounds).map(|b| (b.x + b.w / 2, b.y + b.h / 2));

        let scanned = if let Some(s) = &shot {
            // This frame may be written, so every window it shows is scanned.
            self.scan_frame(&s.windows.clone(), s.hidden, frame_area(&s.frame), None, t)
        } else {
            self.scan_foreground(t)
        };
        let signals = scan::signals(element.as_ref(), focused.as_ref(), scanned.as_deref(), point);
        let screenshot = match (shot, &scanned) {
            (Some(s), Some(rects)) if signals.screenshot_allowed() => {
                let reported = reported_bounds(&[element.as_ref(), focused.as_ref()]);
                Some(self.redact_and_keep(s, rects, &reported, None, None))
            }
            _ => None,
        };
        // Typing steps never name the window (A3 covers clicks), so no title is read.
        let target = focused.as_ref().map(|f| to_target(f, None));
        let event = Event::Key { t_ms: self.unix_ms(t), key, mods, target, signals, screenshot };
        let blocks = self.norm.push(event);
        self.emit(blocks);
    }

    /// The foreground window's scan, reused for a second while typing so that
    /// analysis keeps up with fast typists.
    fn scan_foreground(&mut self, t: i64) -> Option<Vec<Rect>> {
        let fg = unsafe { GetForegroundWindow() }.0 as isize;
        let reuse = self
            .key_scan
            .as_ref()
            .is_some_and(|k| k.hwnd == fg && ((clock::now() - k.at) as f64 * self.ms_per_tick) < KEY_SCAN_REUSE_MS);
        if !reuse {
            let w = scan::current_state(fg)?;
            if !self.scan_windows(&[w], t) {
                return None;
            }
            let rects = self.memory.window_fields(&w, clock::now(), self.ms_per_tick).unwrap_or_default();
            self.key_scan = Some(KeyScan { hwnd: fg, at: clock::now(), rects });
        }
        let mut all = self.memory.around(t, self.ms_per_tick);
        if let Some(k) = &self.key_scan {
            all.extend(k.rects.iter().copied());
        }
        Some(all)
    }

    /// Handle finished steps: give them ids, write their images, drop the rest.
    fn emit(&mut self, blocks: Vec<Block>) {
        for mut block in blocks {
            if let Block::Step(s) = &mut block {
                s.id = format!("{}{}", self.id_prefix, s.id);
                if let Some(path) = s.screenshot.clone() {
                    match self.pending.remove(&path) {
                        Some(shot) => {
                            if !shot.before_verified {
                                add_note(s, NOTE_NOT_VERIFIED);
                                self.counts.not_verified += 1;
                            }
                            let name = path.trim_start_matches(&format!("{IMAGES_DIR}/")).to_string();
                            let _ = self.write.send(WriteJob::Image { name, frame: shot.frame, _ticket: shot._ticket });
                        }
                        None => {
                            s.screenshot = None;
                            add_note(s, NOTE_NO_SCREENSHOT);
                            self.counts.no_screenshot += 1;
                        }
                    }
                }
            }
            self.recording.blocks.push(block);
            self.dirty = true;
        }
        // Screenshots the normalizer will never use: dropped in memory.
        for path in self.norm.take_discarded_screenshots() {
            self.pending.remove(&path);
        }
    }

    fn publish(&mut self) {
        let mut st = self.status.lock().unwrap_or_else(|e| e.into_inner());
        let steps: Vec<_> = self
            .recording
            .blocks
            .iter()
            .filter_map(|b| if let Block::Step(s) = b { Some(s) } else { None })
            .filter(|s| s.id.starts_with(&self.id_prefix))
            .collect();
        st.steps = steps.len();
        st.withheld = steps.iter().filter(|s| s.screenshot_withheld).count();
        st.not_verified = self.counts.not_verified;
        st.no_screenshot = self.counts.no_screenshot;
        st.events = self.timing.events as usize;
        st.uia = self.uia.stats;
        st.password_fields_remembered = self.memory.len();
        st.analysis_ms_max = self.timing.max_ms;
        st.analysis_ms_mean = if self.timing.events == 0 { 0.0 } else { self.timing.total_ms / self.timing.events as f64 };
        st.last_scan_windows = self.timing.scan_windows;
        st.last_scan_hidden = self.timing.scan_hidden;
        let mut warnings = Vec::new();
        if let Some(lost) = &st.scope_lost {
            warnings.push(lost.clone());
        }
        if st.frames_over_budget > 0 {
            warnings.push(format!("{} screenshot(s) skipped: the computer could not keep up", st.frames_over_budget));
        }
        if st.withheld > 0 {
            warnings.push(format!("{} screenshot(s) withheld: a password check could not finish", st.withheld));
        }
        if !st.before_state_available && st.events > 1 {
            warnings.push("Screen capture is not running: screenshots may show the moment after each action".to_string());
        }
        if st.not_verified > 0 {
            warnings.push(format!("{} screenshot(s) may show the moment after the action", st.not_verified));
        }
        st.warnings = warnings;
    }
}

fn frame_area(f: &Frame) -> Rect {
    Rect { x: f.origin_x, y: f.origin_y, w: f.width as i32, h: f.height as i32 }
}

fn monitor_area_at(x: i32, y: i32) -> Option<Rect> {
    let mons = monitors::enumerate();
    let m = mons.get(monitors::index_for(&mons, x, y)?)?;
    Some(Rect { x: m.x, y: m.y, w: m.w, h: m.h })
}

/// Bounds of lookups that report a password field. Blacked out too, as a second
/// line of defence on top of the scan.
fn reported_bounds(infos: &[Option<&ElementInfo>]) -> Vec<Rect> {
    infos.iter().flatten().filter(|i| i.is_password == Some(true)).filter_map(|i| i.bounds).collect()
}

/// `window_title` is `None` unless the user opted in (A3): titles can hold
/// document names and other private text.
fn to_target(e: &ElementInfo, window_title: Option<String>) -> Target {
    Target {
        name: e.name.clone(),
        control_type: e.control_type.clone(),
        window_title,
        process_name: e.process_name.clone(),
        // The normalizer also marks the target from the signals, unknowns included.
        is_password: e.is_password == Some(true),
        bounds: e.bounds,
    }
}

/// PNG encoding normally takes 60-100 ms per 5120x1440 frame, several times the
/// fastest setting. Once frames in flight use more than this share of the
/// budget, the writer switches to the fastest setting, so that a burst of
/// clicks gets bigger files rather than missing screenshots.
fn effort_for(in_flight_bytes: usize) -> Effort {
    if in_flight_bytes > FRAME_BUDGET_BYTES / 2 {
        Effort::Hurry
    } else {
        Effort::Normal
    }
}

fn writer_loop(bundle: Bundle, rx: Receiver<WriteJob>, in_flight: Arc<AtomicUsize>) {
    let mut errors = Vec::new();
    for job in rx {
        match job {
            WriteJob::Image { name, frame, _ticket } => {
                let effort = effort_for(in_flight.load(Ordering::SeqCst));
                let format = ShotFormat::from_name(&name).unwrap_or(ShotFormat::Png);
                let result = frame.encode(format, effort).and_then(|b| bundle.write_image(&name, &b).map_err(Into::into));
                if let Err(e) = result {
                    errors.push(format!("{name}: {e}"));
                }
            }
            WriteJob::Save(rec) => {
                if let Err(e) = bundle.save(&rec) {
                    errors.push(format!("save: {e}"));
                }
            }
            WriteJob::Finish(reply) => {
                let _ = reply.send((bundle, errors));
                return;
            }
        }
    }
}

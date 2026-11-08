//! Recording (A1, A4, A5, C2, C3): start, pause, resume, comment and stop, from
//! the UI, the global hotkeys and the tray.
//!
//! While recording, the open bundle belongs to the recorder; it comes back on
//! stop. Commands that would touch the bundle meanwhile refuse with `Busy`.
//! Closing the window or quitting stops and saves first.

use crate::commands::{lock_current, AppState, OpenedRecording};
use crate::error::{AppError, AppResult, ErrorKind};
use babelsteps_core::settings::{HotkeySettings, Settings};
use babelsteps_core::storage::Bundle;
use babelsteps_windows::hotkeys::{self, Hotkey};
use babelsteps_windows::indicator::Indicator;
use babelsteps_windows::scope;
use babelsteps_windows::{Recorder, RecorderConfig, Scope};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, State};

/// The four hotkeys from the settings (Q14 defaults: Ctrl+Alt+Shift+R/P/N/X), in action order.
fn current_hotkeys(app: &AppHandle) -> [babelsteps_core::settings::HotkeySpec; 4] {
    let settings = app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    settings.hotkeys.parse().unwrap_or_else(|_| HotkeySettings::default().parse().expect("defaults are valid"))
}

pub struct Active {
    recorder: Recorder,
    path: PathBuf,
    keystrokes: bool,
    /// C2 frame on each screen; None if hidden by setting or it could not be shown.
    cue: Option<Indicator>,
    cue_warning: Option<String>,
    /// A6: what this recording covers, for the recording panel.
    scope: ScopeInfo,
}

/// What a recording covers, in the UI's terms.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScopeInfo {
    /// "screen", "monitor" or "window".
    pub kind: &'static str,
    /// "Monitor 2" or the window's app name. Never the window title (A3).
    pub label: String,
}

#[derive(Default)]
pub struct RecState {
    pub(crate) active: Mutex<Option<Active>>,
    /// Bumped by the "add note" hotkey, so the UI opens its note box.
    note_requests: AtomicU64,
    /// The editor has edits not yet saved. A hotkey start then asks the UI to
    /// save and start (`start_requests`) instead of starting from the older saved copy.
    editor_unsaved: AtomicBool,
    start_requests: AtomicU64,
    hotkeys_ok: Mutex<Vec<bool>>,
    /// Keeps the hotkey thread alive; replaced when the shortcuts change.
    hotkey_handle: Mutex<Option<hotkeys::Hotkeys>>,
    tray: Mutex<Option<TrayIcon>>,
    /// Loaded at start-up; changed by `save_settings`.
    pub(crate) settings: Mutex<Settings>,
    /// Plain-language result of the last stop (screenshots removed, size
    /// warning), shown until the user dismisses it.
    notice: Mutex<Option<String>>,
    /// Where the tray icon is, so the recorder ignores clicks on it.
    tray_area: Arc<Mutex<Vec<babelsteps_core::Rect>>>,
    /// Held while the window is rebuilt from the tray.
    window_build: Mutex<()>,
    /// Set just before we destroy a window whose hardening failed.
    window_refused: AtomicBool,
    /// `MainWindow` as a number: whether the main window may be shown.
    main_state: std::sync::atomic::AtomicU8,
}

/// Whether the main window's WebView2 hardening is confirmed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MainWindow {
    /// Being built (or none): not shown yet.
    Pending = 0,
    /// Hardening confirmed: may be shown.
    Ready = 1,
    /// Hardening failed: stays hidden until destroyed.
    Refused = 2,
}

pub fn set_main_window_state(app: &AppHandle, s: MainWindow) {
    app.state::<RecState>().main_state.store(s as u8, Ordering::Relaxed);
}

pub fn main_window_pending(app: &AppHandle) -> bool {
    main_window_state(app) == MainWindow::Pending
}

fn main_window_state(app: &AppHandle) -> MainWindow {
    match app.state::<RecState>().main_state.load(Ordering::Relaxed) {
        1 => MainWindow::Ready,
        2 => MainWindow::Refused,
        _ => MainWindow::Pending,
    }
}

fn lock_active(state: &RecState) -> std::sync::MutexGuard<'_, Option<Active>> {
    state.active.lock().unwrap_or_else(|e| e.into_inner())
}

/// True while a recording runs. Commands that touch the open bundle check this.
pub fn is_recording(app: &AppHandle) -> bool {
    lock_active(&app.state::<RecState>()).is_some()
}

pub fn busy_error() -> AppError {
    AppError::new(ErrorKind::Busy, "a recording is in progress; stop it first")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyInfo {
    pub action: String,
    pub keys: String,
    /// False if another app already uses this combination.
    pub registered: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub active: bool,
    pub paused: bool,
    pub keystrokes: bool,
    pub path: Option<String>,
    pub steps: usize,
    pub withheld: usize,
    pub warnings: Vec<String>,
    pub note_requests: u64,
    pub start_requests: u64,
    pub notice: Option<String>,
    pub hotkeys: Vec<HotkeyInfo>,
    pub scope: ScopeInfo,
}

fn status_of(app: &AppHandle) -> RecordingStatus {
    let rs = app.state::<RecState>();
    let ok = rs.hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let hotkeys = current_hotkeys(app)
        .iter()
        .enumerate()
        .map(|(i, spec)| HotkeyInfo {
            action: HotkeySettings::ACTIONS[i].to_string(),
            keys: spec.label(),
            registered: ok.get(i).copied().unwrap_or(false),
        })
        .collect();
    let guard = lock_active(&rs);
    let mut st = RecordingStatus {
        active: false,
        paused: false,
        keystrokes: false,
        path: None,
        steps: 0,
        withheld: 0,
        warnings: Vec::new(),
        note_requests: rs.note_requests.load(Ordering::Relaxed),
        start_requests: rs.start_requests.load(Ordering::Relaxed),
        notice: rs.notice.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        hotkeys,
        scope: ScopeInfo { kind: "screen", label: String::new() },
    };
    if let Some(a) = guard.as_ref() {
        let s = a.recorder.status();
        st.active = true;
        st.paused = s.paused;
        st.keystrokes = a.keystrokes;
        st.path = Some(a.path.to_string_lossy().into_owned());
        st.steps = s.steps;
        st.withheld = s.withheld;
        st.warnings = s.warnings;
        if s.elevated_in_front {
            st.warnings.push(
                "The window in front runs as administrator, so clicks in it are not recorded. To record administrator windows, start BabelSteps as administrator.".into(),
            );
        }
        if s.away {
            st.warnings.push("Not recording: the screen is locked or the computer is asleep.".into());
        }
        st.warnings.extend(a.cue_warning.clone());
        st.scope = a.scope.clone();
    }
    st
}

/// What the Record dialog asks for (A6, Q13). Hotkey starts use the whole screen.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CaptureTarget {
    #[default]
    Screen,
    Monitor { device: String },
    Window { id: i64 },
}

impl CaptureTarget {
    fn scope(&self) -> AppResult<Scope> {
        Ok(match self {
            CaptureTarget::Screen => Scope::Screen,
            CaptureTarget::Monitor { device } => Scope::Monitor(device.clone()),
            CaptureTarget::Window { id } => Scope::Window(
                isize::try_from(*id).map_err(|_| AppError::new(ErrorKind::InvalidArgument, "not a window"))?,
            ),
        })
    }
}

fn scope_info(scope: &Scope) -> ScopeInfo {
    match scope {
        Scope::Screen => ScopeInfo { kind: "screen", label: String::new() },
        Scope::Monitor(device) => {
            let label = scope::monitor_choices()
                .iter()
                .find(|m| &m.device == device)
                .map(monitor_label)
                .unwrap_or_else(|| "one monitor".into());
            ScopeInfo { kind: "monitor", label }
        }
        Scope::Window(id) => {
            let app = scope::window_choices().into_iter().find(|w| w.id == *id).map(|w| w.app).unwrap_or_default();
            ScopeInfo { kind: "window", label: app }
        }
    }
}

fn monitor_label(m: &scope::MonitorChoice) -> String {
    if m.primary {
        format!("Main monitor ({} × {})", m.width, m.height)
    } else {
        format!("Monitor {} ({} × {})", m.number, m.width, m.height)
    }
}

fn start(app: &AppHandle, keystrokes: bool, target: &CaptureTarget) -> AppResult<()> {
    let scope = target.scope()?;
    if let Err(e) = scope::validate(&scope, true) {
        return Err(AppError::new(ErrorKind::InvalidArgument, format!("{e}")));
    }
    // Lists windows, so done before taking the recording lock.
    let info = scope_info(&scope);
    let rs = app.state::<RecState>();
    let mut active = lock_active(&rs);
    if active.is_some() {
        return Err(busy_error());
    }
    // B14: never record with custom rules that did not build, because the user
    // would believe their words were hidden. The message names no word.
    let custom = rs
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .custom_rules()
        .map_err(|m| AppError::new(ErrorKind::InvalidArgument, format!("{m}. Fix it in Settings before recording")))?;
    let state = app.state::<AppState>();
    let mut current = lock_current(&state);
    let bundle = match current.take() {
        Some(b) => b,
        // Started by hotkey with nothing open: a new recording in the library.
        None => Bundle::create(&crate::commands::default_library(app)?, "Recording", &crate::commands::new_id(), crate::commands::now_ms())?,
    };
    let path = bundle.path().to_path_buf();
    let mut recording = match bundle.load() {
        Ok(r) => r,
        Err(e) => {
            *current = Some(bundle);
            return Err(e.into());
        }
    };
    // PSR-3: a new recording starts with the typed-text line; the recorder
    // saves it with the steps.
    babelsteps_core::notice::apply_typed_text_notice(&mut recording, keystrokes);
    let (show_border, show_window_titles) = {
        let s = rs.settings.lock().unwrap_or_else(|e| e.into_inner());
        (s.show_capture_border, s.show_window_titles)
    };
    let ignore_keys = current_hotkeys(app).iter().map(Hotkey::from_spec).collect();
    let config = RecorderConfig {
        keystrokes_enabled: keystrokes,
        ignore_keys,
        ignore_areas: Some(rs.tray_area.clone()),
        // The tray tooltip starts with this. A colon cannot appear in file
        // names, so File Explorer items are never matched.
        ignore_shell_elements_named: Some("BabelSteps:".to_string()),
        scope: scope.clone(),
        show_window_titles,
        custom,
        ..RecorderConfig::default()
    };
    let scope = info;
    match Recorder::start(bundle, recording, config) {
        Ok(recorder) => {
            let (cue, cue_warning) = if show_border {
                match Indicator::start() {
                    Ok(c) => (Some(c), None),
                    Err(e) => (None, Some(format!("The recording frame around the screen is not shown: {e}."))),
                }
            } else {
                (None, None)
            };
            *active = Some(Active { recorder, path, keystrokes, cue, cue_warning, scope });
            Ok(())
        }
        Err(e) => {
            // The bundle went into the failed start; open it again.
            *current = Bundle::open(&path).ok();
            Err(AppError::new(ErrorKind::Storage, format!("recording could not start: {e}")))
        }
    }
}

/// Stop, save, and reopen the bundle as the current recording.
fn stop(app: &AppHandle) -> AppResult<OpenedRecording> {
    let taken = lock_active(&app.state::<RecState>()).take();
    let Some(a) = taken else { return Err(AppError::new(ErrorKind::InvalidArgument, "no recording is running")) };
    let path = a.path.clone();
    let reopen = |app: &AppHandle| keep_current(app, Bundle::open(&path).ok());
    let (bundle, report) = match a.recorder.stop() {
        Ok(done) => done,
        Err(e) => {
            // Keep the document open whatever happened to the recorder.
            reopen(app);
            return Err(AppError::new(ErrorKind::Storage, format!("recording did not stop cleanly: {e}")));
        }
    };
    let mut recording = match bundle.load() {
        Ok(r) => r,
        Err(e) => {
            keep_current(app, Some(bundle));
            return Err(e.into());
        }
    };
    let settings = app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // PSR-5: the counts are shown after every recording, before any retention notice.
    let s = &report.status;
    let mut notice = babelsteps_core::notice::recording_summary(s.steps, s.withheld, s.no_screenshot, report.missed_inputs);
    if s.away_periods > 0 {
        // PSR-10: say that nothing was recorded while away, so a gap is not a surprise.
        let n = s.away_periods;
        notice.push_str(&format!(
            " The screen was locked or the computer asleep {n} time{}; nothing is recorded then.",
            if n == 1 { "" } else { "s" }
        ));
    }
    if let Some(more) = apply_retention(&bundle, &mut recording, &settings) {
        notice.push(' ');
        notice.push_str(&more);
    }
    *app.state::<RecState>().notice.lock().unwrap_or_else(|e| e.into_inner()) = Some(notice);
    let path = bundle.path().to_string_lossy().into_owned();
    keep_current(app, Some(bundle));
    Ok(OpenedRecording { path, recording })
}

/// After a stop, the stopped recording stays open for the editor. With no
/// window (closed to the tray) there is no editor to show it, so it is closed:
/// the next start shortcut then makes a new recording instead of adding steps
/// to one the user cannot see.
fn keep_current(app: &AppHandle, bundle: Option<Bundle>) {
    let window = app.get_webview_window("main").is_some() && main_window_state(app) != MainWindow::Refused;
    *lock_current(&app.state::<AppState>()) = if window { bundle } else { None };
}

/// A8: keep screenshots for the newest N steps only. The document is saved
/// first and files are deleted after, so a failure never leaves a step that
/// points at a missing picture. Returns a notice for the user, if any.
fn apply_retention(bundle: &Bundle, recording: &mut babelsteps_core::model::Recording, settings: &Settings) -> Option<String> {
    let Some(keep) = settings.keep_screenshots else {
        // Unlimited (Q12): warn when the pictures take a lot of space.
        let mb = dir_size(&bundle.images_dir()) / (1024 * 1024);
        return (mb >= u64::from(settings.size_warning_mb)).then(|| {
            format!(
                "This recording's screenshots take about {mb} MB. In Settings you can keep screenshots for only the newest steps."
            )
        });
    };
    let out = match bundle.apply_retention(recording, keep as usize) {
        Ok(o) => o,
        Err(_) => return Some("The limit on kept screenshots could not be applied because the recording could not be saved. Nothing was removed.".into()),
    };
    if out.removed == 0 {
        return None;
    }
    let (removed, failed) = (out.removed, out.failed);
    let mut msg = format!(
        "Kept screenshots for the newest {keep} steps. Removed the screenshots of {} older {}.",
        removed,
        if removed == 1 { "step" } else { "steps" }
    );
    if failed > 0 {
        msg.push_str(&format!(" {failed} picture file(s) could not be deleted and remain in the recording folder."));
    }
    Some(msg)
}

fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

fn with_active(app: &AppHandle, f: impl FnOnce(&Recorder)) -> AppResult<()> {
    let rs = app.state::<RecState>();
    let guard = lock_active(&rs);
    let a = guard.as_ref().ok_or_else(|| AppError::new(ErrorKind::InvalidArgument, "no recording is running"))?;
    f(&a.recorder);
    Ok(())
}

fn toggle_pause(app: &AppHandle) {
    let _ = with_active(app, |r| if r.status().paused { r.resume() } else { r.pause() });
}

fn show_main(app: &AppHandle) {
    show_main_for(app, false);
}

/// Bring the window back. With "keep running in the tray" the window is
/// destroyed on close (that frees the WebView2 browser engine, most of the
/// idle memory), so build a new one. `for_note` opens it with the note box focused.
fn show_main_for(app: &AppHandle, for_note: bool) {
    if let Some(w) = app.get_webview_window("main") {
        // Only a window whose hardening is confirmed may be shown. A pending
        // one shows itself when ready; a refused one stays hidden until destroyed.
        if main_window_state(app) == MainWindow::Ready {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        return;
    }
    // Build off the calling thread: this runs from tray-menu handlers on the
    // event-loop thread, where building a WebView2 window can deadlock.
    let app = app.clone();
    std::thread::spawn(move || {
        let rs = app.state::<RecState>();
        // One rebuild at a time, so two quick clicks do not race for the label.
        let _one = rs.window_build.lock().unwrap_or_else(|e| e.into_inner());
        if app.get_webview_window("main").is_some() {
            return;
        }
        // On success the window shows and focuses itself once hardened.
        match crate::build_main_window(&app, for_note) {
            Ok(_) => {}
            Err(e) => {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                app.dialog()
                    .message(format!("BabelSteps could not open its window. Quit it from the tray menu and start it again.\n\n{e}"))
                    .title("Window did not open")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
            }
        }
    });
}

/// Called when the main window is gone. Its editor can no longer save first,
/// and the recording it had open is closed, so a start shortcut pressed later
/// (window closed to the tray) makes a new recording instead of adding steps
/// to a document the user can no longer see. A rebuilt window opens on the library.
pub fn main_window_destroyed(app: &AppHandle) {
    set_main_window_state(app, MainWindow::Pending);
    let rs = app.state::<RecState>();
    rs.editor_unsaved.store(false, Ordering::Relaxed);
    let active = lock_active(&rs);
    if active.is_none() {
        *crate::commands::lock_current(&app.state::<crate::commands::AppState>()) = None;
    }
}

/// Whether an exit with no code (the last window closed) should be refused so
/// the app stays in the tray. Closing the window stops any recording first, so
/// a recording still running here means Windows itself is ending the app (sign
/// out, shut down): then exit, which stops and saves.
pub fn stay_in_tray(app: &AppHandle) -> bool {
    let rs = app.state::<RecState>();
    let on = rs.settings.lock().unwrap_or_else(|e| e.into_inner()).close_to_tray;
    // A window we closed ourselves (hardening refused) is not Windows ending
    // the app, so a running recording keeps going in the tray.
    let refused = rs.window_refused.swap(false, Ordering::Relaxed);
    on && (refused || !is_recording(app))
}

/// Marks the next last-window exit as caused by our own refusal of a window.
pub fn note_window_refused(app: &AppHandle) {
    app.state::<RecState>().window_refused.store(true, Ordering::Relaxed);
}

/// Stop if recording. Used before the window closes or the app quits.
/// On failure the caller keeps the app open and tells the user (see `report_stop_failure`).
pub fn stop_if_recording(app: &AppHandle) -> AppResult<()> {
    if is_recording(app) {
        stop(app).map(|_| ())
    } else {
        Ok(())
    }
}

/// Tell the user that stopping on close did not work, so the close is not silent.
pub fn report_stop_failure(app: &AppHandle, err: &AppError) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    app.dialog()
        .message(format!(
            "BabelSteps could not stop and save the recording, so it stayed open. \
             Your steps so far are kept in the recording folder.\n\n{err}"
        ))
        .title("Recording not saved cleanly")
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Stop if recording; on failure tell the user. True when nothing is left running.
pub fn stop_and_report(app: &AppHandle) -> bool {
    match stop_if_recording(app) {
        Ok(()) => true,
        Err(e) => {
            report_stop_failure(app, &e);
            false
        }
    }
}

// ---- commands ----------------------------------------------------------------

#[tauri::command]
pub fn clear_notice(app: AppHandle) {
    *app.state::<RecState>().notice.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[tauri::command]
pub fn recording_status(app: AppHandle) -> RecordingStatus {
    status_of(&app)
}

/// The editor reports whether it holds unsaved edits.
#[tauri::command]
pub fn set_editor_unsaved(app: AppHandle, unsaved: bool) {
    app.state::<RecState>().editor_unsaved.store(unsaved, Ordering::Relaxed);
}

#[tauri::command]
pub fn start_recording(app: AppHandle, keystrokes: bool, target: Option<CaptureTarget>) -> AppResult<RecordingStatus> {
    start(&app, keystrokes, &target.unwrap_or_default())?;
    Ok(status_of(&app))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorOption {
    pub device: String,
    pub label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowOption {
    pub id: i64,
    /// "Title (app)". Shown in the picker only, never stored.
    pub label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTargets {
    pub monitors: Vec<MonitorOption>,
    pub windows: Vec<WindowOption>,
}

/// The monitors and windows the Record dialog offers (A6).
#[tauri::command]
pub fn list_capture_targets() -> CaptureTargets {
    let monitors = scope::monitor_choices()
        .iter()
        .map(|m| MonitorOption { device: m.device.clone(), label: monitor_label(m) })
        .collect();
    let windows = scope::window_choices()
        .into_iter()
        .map(|w| {
            let mut title: String = w.title.trim().chars().take(80).collect();
            if w.title.trim().chars().count() > 80 {
                title.push('…');
            }
            let label = if w.app.is_empty() { title } else { format!("{title} ({})", w.app) };
            WindowOption { id: w.id as i64, label }
        })
        .collect();
    CaptureTargets { monitors, windows }
}

#[tauri::command]
pub fn pause_recording(app: AppHandle) -> AppResult<()> {
    with_active(&app, Recorder::pause)
}

#[tauri::command]
pub fn resume_recording(app: AppHandle) -> AppResult<()> {
    with_active(&app, Recorder::resume)
}

#[tauri::command]
pub fn add_comment(app: AppHandle, text: String) -> AppResult<()> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > 2000 {
        return Err(AppError::new(ErrorKind::InvalidArgument, "a note must be 1 to 2000 characters"));
    }
    with_active(&app, |r| r.comment(text))
}

#[tauri::command]
pub async fn stop_recording(app: AppHandle) -> AppResult<OpenedRecording> {
    tauri::async_runtime::spawn_blocking(move || stop(&app))
        .await
        .map_err(|e| AppError::new(ErrorKind::Storage, format!("stop task failed: {e}")))?
}

// ---- hotkeys and tray ----------------------------------------------------------

/// Register the global hotkeys from the settings and act on them. Called at
/// start-up and again when the shortcuts change; the old registration ends first.
pub fn setup_hotkeys(app: &AppHandle) {
    let rs = app.state::<RecState>();
    // Dropping the old handle unregisters its keys and ends its action thread.
    drop(rs.hotkey_handle.lock().unwrap_or_else(|e| e.into_inner()).take());
    let keys: Vec<Hotkey> = current_hotkeys(app).iter().map(Hotkey::from_spec).collect();
    let Ok((handle, pressed, ok)) = hotkeys::register(&keys) else {
        *rs.hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()) = Vec::new();
        return;
    };
    *rs.hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()) = ok;
    *rs.hotkey_handle.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    let app = app.clone();
    std::thread::Builder::new()
        .name("hotkey-actions".into())
        .spawn(move || {
            for index in pressed {
                match index {
                    0 => {
                        let rs = app.state::<RecState>();
                        if is_recording(&app) {
                            let _ = stop(&app);
                        } else if rs.editor_unsaved.load(Ordering::Relaxed) {
                            // The editor saves its edits, then starts (keystrokes off).
                            rs.start_requests.fetch_add(1, Ordering::Relaxed);
                        } else {
                            // Keystrokes stay off and the whole screen is recorded
                            // unless chosen in the app.
                            let _ = start(&app, false, &CaptureTarget::Screen);
                        }
                    }
                    1 => toggle_pause(&app),
                    2 => {
                        if is_recording(&app) {
                            app.state::<RecState>().note_requests.fetch_add(1, Ordering::Relaxed);
                            show_main_for(&app, true);
                        }
                    }
                    3 => {
                        stop_and_report(&app);
                    }
                    _ => {}
                }
            }
        })
        .ok();
}

/// Record where the tray icon is, in physical pixels, with a little margin.
fn update_tray_area(app: &AppHandle) {
    let rs = app.state::<RecState>();
    let rect = rs.tray.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.rect().ok().flatten());
    let area = match rect {
        Some(tauri::Rect { position: tauri::Position::Physical(p), size: tauri::Size::Physical(s) }) => {
            vec![babelsteps_core::Rect { x: p.x - 2, y: p.y - 2, w: s.width as i32 + 4, h: s.height as i32 + 4 }]
        }
        _ => Vec::new(),
    };
    *rs.tray_area.lock().unwrap_or_else(|e| e.into_inner()) = area;
}

/// A filled circle, for the tray: grey idle, red recording, amber paused.
fn dot(rgb: [u8; 3]) -> Image<'static> {
    const N: u32 = 32;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let a = ((c - d + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
        }
    }
    Image::new_owned(px, N, N)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayLook {
    Idle,
    Recording { keystrokes: bool },
    Paused { keystrokes: bool },
}

/// The always-visible indicator (C2): a tray icon whose colour and tooltip show
/// whether recording is on, and whether keystrokes are recorded (Q4).
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show BabelSteps", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause or resume recording", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop recording", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit BabelSteps", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &sep1, &pause, &stop_item, &sep2, &quit])?;
    let tray = TrayIconBuilder::with_id("babelsteps")
        .icon(dot([128, 128, 128]))
        .tooltip("BabelSteps: not recording")
        .menu(&menu)
        // Right-click opens the menu; double-click opens the window. A menu on
        // left-click would swallow the double-click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "pause" => toggle_pause(app),
            "stop" => {
                stop_and_report(app);
            }
            "quit" => {
                // A failed save keeps the app open and says so.
                if stop_and_report(app) {
                    app.exit(0);
                }
            }
            _ => {}
        })
        // Double-click the icon to bring the window back (after a close to the tray).
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::DoubleClick { button: tauri::tray::MouseButton::Left, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    *app.state::<RecState>().tray.lock().unwrap_or_else(|e| e.into_inner()) = Some(tray);

    // Keep the icon in step with the recorder, whoever started or stopped it.
    let app = app.clone();
    std::thread::Builder::new()
        .name("tray-status".into())
        .spawn(move || {
            let mut last: Option<TrayLook> = None;
            loop {
                std::thread::sleep(Duration::from_millis(300));
                update_tray_area(&app);
                let st = status_of(&app);
                let look = match (st.active, st.paused) {
                    (false, _) => TrayLook::Idle,
                    (true, true) => TrayLook::Paused { keystrokes: st.keystrokes },
                    (true, false) => TrayLook::Recording { keystrokes: st.keystrokes },
                };
                if last == Some(look) {
                    continue;
                }
                last = Some(look);
                if let Some(a) = lock_active(&app.state::<RecState>()).as_ref() {
                    if let Some(c) = &a.cue {
                        c.set_paused(matches!(look, TrayLook::Paused { .. }));
                    }
                }
                let (icon, tip) = match look {
                    TrayLook::Idle => (dot([128, 128, 128]), "BabelSteps: not recording".to_string()),
                    TrayLook::Paused { keystrokes } => (
                        dot([230, 160, 0]),
                        format!("BabelSteps: recording paused. Keystrokes {}", if keystrokes { "on" } else { "off" }),
                    ),
                    TrayLook::Recording { keystrokes } => (
                        dot([220, 30, 30]),
                        format!("BabelSteps: recording. Keystrokes {}", if keystrokes { "recording" } else { "off" }),
                    ),
                };
                let rs = app.state::<RecState>();
                let guard = rs.tray.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(t) = guard.as_ref() {
                    let _ = t.set_icon(Some(icon));
                    let _ = t.set_tooltip(Some(tip));
                }
            }
        })
        .ok();
    Ok(())
}

/// Refuses while a recording runs: the open bundle belongs to the recorder.
pub fn ensure_idle(rec: &State<'_, RecState>) -> AppResult<()> {
    if lock_active(rec).is_some() {
        return Err(busy_error());
    }
    Ok(())
}

// ---- settings ------------------------------------------------------------------

fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Storage, format!("no app data folder: {e}")))?;
    Ok(dir.join("settings.json"))
}

/// Read the settings file at start-up. A missing or damaged file gives the defaults.
pub fn load_settings(app: &AppHandle) {
    if let Ok(p) = settings_path(app) {
        *app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()) = Settings::load(&p);
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Saves and applies. Returns warnings to show: shortcuts another app already
/// uses, and shortcuts likely to clash with other programs.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> AppResult<Vec<String>> {
    settings.validate().map_err(|m| AppError::new(ErrorKind::InvalidArgument, m))?;
    let mut settings = settings;
    let hotkeys_changed;
    settings.hotkeys = settings.hotkeys.normalized().map_err(|m| AppError::new(ErrorKind::InvalidArgument, m))?;
    let rs = app.state::<RecState>();
    let path = settings_path(&app)?;
    {
        // `start` holds this lock while it reads the shortcuts, so a recording
        // cannot begin between the check and the change.
        let active = lock_active(&rs);
        let old = rs.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        hotkeys_changed = old.hotkeys != settings.hotkeys;
        // The running recorder ignores the old shortcuts; change them between recordings.
        if active.is_some() && hotkeys_changed {
            return Err(busy_error());
        }
        settings.save(&path).map_err(|e| AppError::new(ErrorKind::Storage, format!("settings were not saved: {e}")))?;
        *rs.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    }
    if hotkeys_changed {
        setup_hotkeys(&app);
    }
    Ok(hotkey_warnings(&app))
}

fn hotkey_warnings(app: &AppHandle) -> Vec<String> {
    let ok = app.state::<RecState>().hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut out = Vec::new();
    for (i, spec) in current_hotkeys(app).iter().enumerate() {
        let action = HotkeySettings::ACTIONS[i];
        if !ok.get(i).copied().unwrap_or(false) {
            out.push(format!("{} for \"{action}\" is already used by Windows or another app, so it will not work. Pick another shortcut.", spec.label()));
        } else if spec.is_weak() {
            out.push(format!("{} for \"{action}\" has fewer than three modifier keys and may clash with browsers or other apps.", spec.label()));
        }
    }
    out
}

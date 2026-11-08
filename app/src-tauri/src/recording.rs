//! Recording (A1, A4, A5, C2, C3): start, pause, resume, comment and stop, from
//! the UI, the global hotkeys and the tray.
//!
//! While recording, the open bundle belongs to the recorder; it comes back on
//! stop. Commands that would touch the bundle meanwhile refuse with `Busy`.
//! Closing the window or quitting stops and saves first.

use crate::commands::{lock_current, AppState, OpenedRecording};
use crate::error::{AppError, AppResult, ErrorKind};
use babelsteps_core::settings::Settings;
use babelsteps_core::storage::Bundle;
use babelsteps_windows::hotkeys::{self, Hotkey};
use babelsteps_windows::{Recorder, RecorderConfig};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, State};

/// Q14 defaults: Ctrl+Alt+Shift+R start/stop, P pause, N add note, X panic stop.
const HOTKEYS: [(&str, char); 4] = [("Start or stop recording", 'R'), ("Pause or resume", 'P'), ("Add a note", 'N'), ("Stop now", 'X')];

pub struct Active {
    recorder: Recorder,
    path: PathBuf,
    keystrokes: bool,
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
    tray: Mutex<Option<TrayIcon>>,
    /// Loaded at start-up; changed by `save_settings`.
    pub(crate) settings: Mutex<Settings>,
    /// Plain-language result of the last stop (screenshots removed, size
    /// warning), shown until the user dismisses it.
    notice: Mutex<Option<String>>,
    /// Where the tray icon is, so the recorder ignores clicks on it.
    tray_area: Arc<Mutex<Vec<babelsteps_core::Rect>>>,
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
}

fn status_of(app: &AppHandle) -> RecordingStatus {
    let rs = app.state::<RecState>();
    let ok = rs.hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let hotkeys = HOTKEYS
        .iter()
        .enumerate()
        .map(|(i, (action, key))| HotkeyInfo {
            action: action.to_string(),
            keys: format!("Ctrl+Alt+Shift+{key}"),
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
    }
    st
}

fn start(app: &AppHandle, keystrokes: bool) -> AppResult<()> {
    let rs = app.state::<RecState>();
    let mut active = lock_active(&rs);
    if active.is_some() {
        return Err(busy_error());
    }
    let state = app.state::<AppState>();
    let mut current = lock_current(&state);
    let bundle = match current.take() {
        Some(b) => b,
        // Started by hotkey with nothing open: a new recording in the library.
        None => Bundle::create(&crate::commands::default_library(app)?, "Recording", &crate::commands::new_id(), crate::commands::now_ms())?,
    };
    let path = bundle.path().to_path_buf();
    let recording = match bundle.load() {
        Ok(r) => r,
        Err(e) => {
            *current = Some(bundle);
            return Err(e.into());
        }
    };
    let ignore_keys = HOTKEYS.iter().map(|(_, k)| Hotkey::ctrl_alt_shift(*k)).collect();
    let config = RecorderConfig {
        keystrokes_enabled: keystrokes,
        ignore_keys,
        ignore_areas: Some(rs.tray_area.clone()),
        // The tray tooltip starts with this. A colon cannot appear in file
        // names, so File Explorer items are never matched.
        ignore_shell_elements_named: Some("BabelSteps:".to_string()),
        ..RecorderConfig::default()
    };
    match Recorder::start(bundle, recording, config) {
        Ok(recorder) => {
            *active = Some(Active { recorder, path, keystrokes });
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
    let reopen = |app: &AppHandle| *lock_current(&app.state::<AppState>()) = Bundle::open(&path).ok();
    let bundle = match a.recorder.stop() {
        Ok((bundle, _report)) => bundle,
        Err(e) => {
            // Keep the document open whatever happened to the recorder.
            reopen(app);
            return Err(AppError::new(ErrorKind::Storage, format!("recording did not stop cleanly: {e}")));
        }
    };
    let mut recording = match bundle.load() {
        Ok(r) => r,
        Err(e) => {
            *lock_current(&app.state::<AppState>()) = Some(bundle);
            return Err(e.into());
        }
    };
    let settings = app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let notice = apply_retention(&bundle, &mut recording, &settings);
    *app.state::<RecState>().notice.lock().unwrap_or_else(|e| e.into_inner()) = notice;
    let path = bundle.path().to_string_lossy().into_owned();
    *lock_current(&app.state::<AppState>()) = Some(bundle);
    Ok(OpenedRecording { path, recording })
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
    let mut trimmed = recording.clone();
    let removed = trimmed.apply_retention(keep as usize);
    if removed.is_empty() {
        return None;
    }
    if bundle.save(&trimmed).is_err() {
        return Some("The limit on kept screenshots could not be applied because the recording could not be saved. Nothing was removed.".into());
    }
    *recording = trimmed;
    let failed = removed.iter().filter(|p| bundle.remove_image(p).is_err()).count();
    let mut msg = format!(
        "Kept screenshots for the newest {keep} steps. Removed the screenshots of {} older {}.",
        removed.len(),
        if removed.len() == 1 { "step" } else { "steps" }
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
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
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
pub fn start_recording(app: AppHandle, keystrokes: bool) -> AppResult<RecordingStatus> {
    start(&app, keystrokes)?;
    Ok(status_of(&app))
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

/// Register the global hotkeys and act on them for the life of the app.
pub fn setup_hotkeys(app: &AppHandle) {
    let keys: Vec<Hotkey> = HOTKEYS.iter().map(|(_, k)| Hotkey::ctrl_alt_shift(*k)).collect();
    let Ok((handle, pressed, ok)) = hotkeys::register(&keys) else { return };
    *app.state::<RecState>().hotkeys_ok.lock().unwrap_or_else(|e| e.into_inner()) = ok;
    let app = app.clone();
    std::thread::Builder::new()
        .name("hotkey-actions".into())
        .spawn(move || {
            let _keep = handle; // Unregistered when this thread ends with the app.
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
                            let _ = start(&app, false); // Keystrokes stay off unless chosen in the app.
                        }
                    }
                    1 => toggle_pause(&app),
                    2 => {
                        if is_recording(&app) {
                            app.state::<RecState>().note_requests.fetch_add(1, Ordering::Relaxed);
                            show_main(&app);
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

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> AppResult<()> {
    settings.validate().map_err(|m| AppError::new(ErrorKind::InvalidArgument, m))?;
    let path = settings_path(&app)?;
    settings.save(&path).map_err(|e| AppError::new(ErrorKind::Storage, format!("settings were not saved: {e}")))?;
    *app.state::<RecState>().settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
    Ok(())
}

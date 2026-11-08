//! Tauri commands. Thin wrappers over `babelsteps-core`. They never log, and
//! errors carry kinds and paths only, never document content.
//!
//! There is deliberately no general "read a file" command: screenshots can
//! only be read from the currently open recording's `images/` folder.

use crate::error::{AppError, AppResult, ErrorKind};
use crate::pdf::{export_pdf_with, PdfExportError};
use babelsteps_core::export::{export_markdown_with as core_export_markdown, validate_image_path, ExportReport};
use babelsteps_core::export_options::ExportOptions;
use babelsteps_core::model::Recording;
use babelsteps_core::storage::{delete_bundle, library, Bundle};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::recording::{ensure_idle, RecState};
use tauri::{AppHandle, Manager, State};

#[derive(Default)]
pub struct AppState {
    /// The one recording that is open, if any.
    current: Mutex<Option<Bundle>>,
    /// One export (and so one print job) at a time.
    export_lock: Mutex<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingSummary {
    pub path: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    pub step_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedBundle {
    pub path: String,
    pub reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryListing {
    pub folder: String,
    pub recordings: Vec<RecordingSummary>,
    pub skipped: Vec<SkippedBundle>,
}

#[derive(Serialize, Debug, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub steps: usize,
    pub images_written: usize,
    pub images_with_redactions: usize,
    pub withheld: usize,
}

impl From<ExportReport> for ExportSummary {
    fn from(r: ExportReport) -> Self {
        ExportSummary {
            steps: r.steps,
            images_written: r.images_written,
            images_with_redactions: r.images_with_redactions,
            withheld: r.withheld,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedRecording {
    pub path: String,
    pub recording: Recording,
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(crate) fn new_id() -> String {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:x}-{:x}-{:x}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

pub(crate) fn default_library(app: &AppHandle) -> AppResult<PathBuf> {
    let base = app
        .path()
        .local_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Storage, format!("no local data folder: {e}")))?;
    Ok(base.join("BabelSteps"))
}

fn require_absolute(p: &str, what: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(p);
    if p.is_empty() || !path.is_absolute() {
        return Err(AppError::new(ErrorKind::InvalidArgument, format!("{what} must be a full path")));
    }
    Ok(path)
}

fn no_recording() -> AppError {
    AppError::new(ErrorKind::NoRecordingOpen, "no recording is open")
}

pub(crate) fn lock_current(state: &AppState) -> std::sync::MutexGuard<'_, Option<Bundle>> {
    // A panic elsewhere must not lock the user out of their recording.
    state.current.lock().unwrap_or_else(|e| e.into_inner())
}

// ---- library -------------------------------------------------------------

#[tauri::command]
pub fn get_default_library_folder(app: AppHandle) -> AppResult<String> {
    let dir = default_library(&app)?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.to_string_lossy().into_owned())
}

/// List recordings in `folder`, or in the default library folder when `None`.
#[tauri::command]
pub fn list_library(app: AppHandle, folder: Option<String>) -> AppResult<LibraryListing> {
    let dir = match folder {
        Some(f) => require_absolute(&f, "the library folder")?,
        None => {
            let d = default_library(&app)?;
            std::fs::create_dir_all(&d)?;
            d
        }
    };
    let listing = library::list(&dir)?;
    Ok(LibraryListing {
        folder: dir.to_string_lossy().into_owned(),
        recordings: listing
            .recordings
            .into_iter()
            .map(|s| RecordingSummary {
                path: s.path.to_string_lossy().into_owned(),
                title: s.title,
                created: s.created,
                updated: s.updated,
                step_count: s.step_count,
            })
            .collect(),
        skipped: listing
            .skipped
            .into_iter()
            .map(|(p, reason)| SkippedBundle { path: p.to_string_lossy().into_owned(), reason })
            .collect(),
    })
}

/// Create a new empty recording and make it the open one.
#[tauri::command]
pub fn create_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    rec: State<'_, RecState>,
    folder: Option<String>,
    title: String,
) -> AppResult<OpenedRecording> {
    ensure_idle(&rec)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::new(ErrorKind::InvalidArgument, "the title is empty"));
    }
    let dir = match folder {
        Some(f) => require_absolute(&f, "the library folder")?,
        None => default_library(&app)?,
    };
    let bundle = Bundle::create(&dir, &title, &new_id(), now_ms())?;
    let recording = bundle.load()?;
    let path = bundle.path().to_string_lossy().into_owned();
    *lock_current(&state) = Some(bundle);
    Ok(OpenedRecording { path, recording })
}

/// Open a recording folder, load it, and make it the open one.
#[tauri::command]
pub fn open_recording(state: State<'_, AppState>, rec: State<'_, RecState>, path: String) -> AppResult<OpenedRecording> {
    ensure_idle(&rec)?;
    let p = require_absolute(&path, "the recording path")?;
    let bundle = Bundle::open(&p)?;
    let recording = bundle.load()?;
    let path = bundle.path().to_string_lossy().into_owned();
    *lock_current(&state) = Some(bundle);
    Ok(OpenedRecording { path, recording })
}

#[tauri::command]
pub fn close_recording(state: State<'_, AppState>, rec: State<'_, RecState>) {
    if ensure_idle(&rec).is_err() {
        return;
    }
    *lock_current(&state) = None;
}

/// Save the whole document to the open recording.
#[tauri::command]
pub fn save_recording(state: State<'_, AppState>, rec: State<'_, RecState>, recording: Recording) -> AppResult<()> {
    ensure_idle(&rec)?;
    let guard = lock_current(&state);
    let bundle = guard.as_ref().ok_or_else(no_recording)?;
    bundle.save(&recording)?;
    Ok(())
}

/// Change the title of the open recording (the folder keeps its name).
#[tauri::command]
pub fn rename_recording(state: State<'_, AppState>, rec: State<'_, RecState>, title: String) -> AppResult<()> {
    ensure_idle(&rec)?;
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::new(ErrorKind::InvalidArgument, "the title is empty"));
    }
    let guard = lock_current(&state);
    let bundle = guard.as_ref().ok_or_else(no_recording)?;
    bundle.rename(title)?;
    Ok(())
}

/// Permanently delete a recording folder. Core refuses anything that is not a
/// `.bsrec` folder containing `recording.db`. If it is the open one, it is
/// closed first so the database file is released.
#[tauri::command]
pub fn delete_recording(state: State<'_, AppState>, rec: State<'_, RecState>, path: String) -> AppResult<()> {
    ensure_idle(&rec)?;
    let p = require_absolute(&path, "the recording path")?;
    {
        let mut guard = lock_current(&state);
        if guard.as_ref().is_some_and(|b| same_path(b.path(), &p)) {
            *guard = None;
        }
    }
    delete_bundle(&p)?;
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

// ---- screenshots -----------------------------------------------------------

/// Read one screenshot from the open recording. `path` is the block's
/// `images/<name>` path. Returns raw bytes (an ArrayBuffer in JS).
#[tauri::command]
pub fn read_screenshot(state: State<'_, AppState>, path: String) -> AppResult<tauri::ipc::Response> {
    if !validate_image_path(&path) {
        return Err(AppError::new(ErrorKind::InvalidArgument, "not a valid screenshot path"));
    }
    let guard = lock_current(&state);
    let bundle = guard.as_ref().ok_or_else(no_recording)?;
    let root = std::fs::canonicalize(bundle.path())?;
    let full = std::fs::canonicalize(bundle.path().join(&path)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::new(ErrorKind::NotFound, "screenshot file is missing")
        } else {
            e.into()
        }
    })?;
    // Symlinks must not lead out of the recording's own images folder.
    if !full.starts_with(root.join("images")) {
        return Err(AppError::new(ErrorKind::InvalidArgument, "not a valid screenshot path"));
    }
    Ok(tauri::ipc::Response::new(std::fs::read(&full)?))
}

// ---- export ----------------------------------------------------------------

fn outside_bundle(bundle: &Bundle, target: &Path) -> AppResult<()> {
    let root = std::fs::canonicalize(bundle.path())?;
    // Check the nearest existing ancestor, since the target may not exist yet.
    let mut probe = target;
    loop {
        if let Ok(c) = std::fs::canonicalize(probe) {
            if c.starts_with(&root) {
                return Err(AppError::new(ErrorKind::InvalidArgument, "choose a folder outside the recording"));
            }
            return Ok(());
        }
        match probe.parent() {
            Some(p) => probe = p,
            None => return Ok(()),
        }
    }
}

/// What the user ticked in the export dialog. Both options are off by default
/// (PSR-2, PSR-6).
#[derive(Deserialize, Debug, Clone, Copy, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ExportChoices {
    pub step_times: bool,
    pub technical_details: bool,
    /// Minutes east of UTC when the recording was made, from the webview's clock.
    pub utc_offset_minutes: i32,
}

impl ExportChoices {
    /// The options for the core exporters. The operating system and app version
    /// are only looked up for the technical details, and never name the user or machine.
    pub fn to_options(self) -> ExportOptions {
        let details = self.technical_details;
        ExportOptions {
            step_times: self.step_times,
            technical_details: details,
            // Real offsets are within -12 h to +14 h.
            utc_offset_minutes: self.utc_offset_minutes.clamp(-14 * 60, 14 * 60),
            os: if details { os_description() } else { String::new() },
            app: if details { format!("BabelSteps {}", env!("CARGO_PKG_VERSION")) } else { String::new() },
        }
    }
}

#[cfg(windows)]
fn os_description() -> String {
    babelsteps_windows::os::os_description()
}

#[cfg(not(windows))]
fn os_description() -> String {
    String::new()
}

/// Export the saved document as `document.md` plus `images/` into `out_dir`.
#[tauri::command]
pub async fn export_markdown(app: AppHandle, out_dir: String, choices: Option<ExportChoices>) -> AppResult<ExportSummary> {
    let opts = choices.unwrap_or_default().to_options();
    let out = require_absolute(&out_dir, "the export folder")?;
    ensure_idle(&app.state::<RecState>())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let _busy = state
            .export_lock
            .try_lock()
            .map_err(|_| AppError::new(ErrorKind::Busy, "another export is running"))?;
        let (rec, dir) = {
            let guard = lock_current(&state);
            let bundle = guard.as_ref().ok_or_else(no_recording)?;
            outside_bundle(bundle, &out)?;
            (bundle.load()?, bundle.path().to_path_buf())
        };
        Ok(core_export_markdown(&rec, &dir, &out, &opts)?.into())
    })
    .await
    .map_err(|e| AppError::new(ErrorKind::Export, format!("export task failed: {e}")))?
}

/// Export the saved document as a US Letter PDF.
#[tauri::command]
pub async fn export_pdf(
    app: AppHandle,
    out_path: String,
    header_footer: bool,
    choices: Option<ExportChoices>,
) -> AppResult<ExportSummary> {
    let opts = choices.unwrap_or_default().to_options();
    let out = require_absolute(&out_path, "the PDF path")?;
    if !out.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return Err(AppError::new(ErrorKind::InvalidArgument, "the file name must end in .pdf"));
    }
    ensure_idle(&app.state::<RecState>())?;
    tauri::async_runtime::spawn_blocking(move || run_pdf_export(&app, &out, header_footer, &opts))
        .await
        .map_err(|e| AppError::new(ErrorKind::Pdf, format!("export task failed: {e}")))?
}

pub fn run_pdf_export(app: &AppHandle, out: &Path, header_footer: bool, opts: &ExportOptions) -> AppResult<ExportSummary> {
    let state = app.state::<AppState>();
    let _busy = state
        .export_lock
        .try_lock()
        .map_err(|_| AppError::new(ErrorKind::Busy, "another export is running"))?;
    // Snapshot the saved document, then release the lock so saves are not blocked while printing.
    let (rec, dir) = {
        let guard = lock_current(&state);
        let bundle = guard.as_ref().ok_or_else(no_recording)?;
        outside_bundle(bundle, out)?;
        (bundle.load()?, bundle.path().to_path_buf())
    };
    let printer = crate::pdf::default_printer(app);
    export_pdf_with(printer.as_ref(), &rec, &dir, out, header_footer, Some(String::new()), opts)
        .map(Into::into)
        .map_err(map_pdf_error)
}

pub fn map_pdf_error(e: PdfExportError) -> AppError {
    match e {
        PdfExportError::Prepare(e) => e.into(),
        PdfExportError::Print(e) => AppError::new(
            ErrorKind::Pdf,
            format!(
                "PDF export failed: {e}. As a fallback, export to a folder as HTML or Markdown instead; no window was shown."
            ),
        ),
        PdfExportError::Io(m) => AppError::new(ErrorKind::Storage, m),
    }
}

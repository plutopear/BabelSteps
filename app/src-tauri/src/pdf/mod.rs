//! PDF export (B8, Q7, D5). The document is prepared as HTML by `core` (images
//! validated, redactions burned in, metadata stripped) into a fresh temp
//! folder, printed to PDF by the platform webview behind [`PdfPrinter`], and
//! the result is moved into place. The printer writes inside the temp folder,
//! so even a print that finishes after a timeout never lands next to the
//! user's file. The temp folder is removed afterwards, and leftovers (a killed
//! process, files the webview still held) are swept at startup and before
//! every export. The final file only appears once printing has fully succeeded.

#[cfg(windows)]
pub mod windows;

use babelsteps_core::export::{prepare_html_export, ExportReport};
use babelsteps_core::model::Recording;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// US Letter, inches (Q7).
pub const PAGE_WIDTH_IN: f64 = 8.5;
pub const PAGE_HEIGHT_IN: f64 = 11.0;
pub const MARGIN_IN: f64 = 0.75;

#[derive(Debug)]
pub struct PdfError(pub String);

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PdfError {}

pub struct PdfJob {
    /// The prepared `document.html`.
    pub html_file: PathBuf,
    /// Where the printer writes the PDF. Not final: the caller renames it.
    pub out_pdf: PathBuf,
    /// Shown in the page header when `header_footer` is on.
    pub title: String,
    pub header_footer: bool,
    /// Footer URI text. Always `Some("")` in production, so no file path can
    /// appear. `None` leaves the engine default; only the self-test uses it
    /// (as a negative control showing that the check can fail).
    pub footer_uri: Option<String>,
}

/// Platform seam: print a local HTML file to a PDF file.
pub trait PdfPrinter {
    fn print_to_pdf(&self, job: &PdfJob) -> Result<(), PdfError>;
}

/// The printer for this platform.
#[cfg(windows)]
pub fn default_printer(app: &tauri::AppHandle) -> Box<dyn PdfPrinter> {
    Box::new(windows::WebView2Printer { app: app.clone() })
}

/// No printer yet on this platform (macOS comes later via WKWebView).
#[cfg(not(windows))]
pub fn default_printer(_app: &tauri::AppHandle) -> Box<dyn PdfPrinter> {
    struct Unsupported;
    impl PdfPrinter for Unsupported {
        fn print_to_pdf(&self, _job: &PdfJob) -> Result<(), PdfError> {
            Err(PdfError("PDF export is not available on this platform yet".into()))
        }
    }
    Box::new(Unsupported)
}

/// A temp folder removed on drop (success, failure, or panic).
pub struct TempFolder(PathBuf);

impl TempFolder {
    pub fn new(label: &str) -> std::io::Result<TempFolder> {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "babelsteps-{label}-{}-{}-{nanos}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p)?;
        Ok(TempFolder(p))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        // The print window is destroyed asynchronously and can hold files open
        // for a moment, which makes removal fail on Windows. Retry briefly; the
        // sweep catches anything still left.
        for wait_ms in [0, 50, 100, 200, 400, 600, 800] {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
        }
    }
}

const PDF_TEMP_PREFIX: &str = "babelsteps-pdf-";
/// Another running copy of the app may be exporting; its folder is left alone
/// until it is clearly abandoned (longer than every print timeout together).
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Removes PDF temp folders left behind by earlier exports. They hold redacted
/// screenshots and printed pages. Folders of this process are always removed
/// (exports run one at a time, so none is in use); other processes' folders
/// only once they are older than [`STALE_AFTER`]. Returns how many were removed.
pub fn sweep_stale_temp() -> usize {
    sweep_stale_temp_in(&std::env::temp_dir(), std::process::id(), STALE_AFTER)
}

fn sweep_stale_temp_in(dir: &Path, own_pid: u32, stale_after: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(PDF_TEMP_PREFIX)) else { continue };
        // Exactly our `<pid>-<n>-<nanos>`, so another program's folder is never touched.
        let fields: Vec<&str> = rest.split('-').collect();
        if fields.len() != 3 || !fields.iter().all(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit())) {
            continue;
        }
        // Never follow a link out of the temp folder.
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else { continue };
        if !meta.is_dir() {
            continue;
        }
        let ours = rest.split('-').next().and_then(|p| p.parse::<u32>().ok()) == Some(own_pid);
        let old = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age >= stale_after);
        if (ours || old) && std::fs::remove_dir_all(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[derive(Debug)]
pub enum PdfExportError {
    Prepare(babelsteps_core::export::ExportError),
    Print(PdfError),
    Io(String),
}

/// Removes a half-written file unless disarmed.
struct PartGuard(PathBuf, bool);
impl Drop for PartGuard {
    fn drop(&mut self) {
        if self.1 {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// Export `rec` (whose images live in `source_dir`) to `out_path`.
pub fn export_pdf_with(
    printer: &dyn PdfPrinter,
    rec: &Recording,
    source_dir: &Path,
    out_path: &Path,
    header_footer: bool,
    footer_uri: Option<String>,
) -> Result<ExportReport, PdfExportError> {
    sweep_stale_temp();
    let temp = TempFolder::new("pdf").map_err(|e| PdfExportError::Io(e.to_string()))?;
    let report = prepare_html_export(rec, source_dir, temp.path()).map_err(PdfExportError::Prepare)?;

    let printed = temp.path().join("printed.pdf");
    let job = PdfJob {
        html_file: temp.path().join("document.html"),
        out_pdf: printed.clone(),
        title: rec.title.clone(),
        header_footer,
        footer_uri,
    };
    printer.print_to_pdf(&job).map_err(PdfExportError::Print)?;
    if !printed.is_file() {
        return Err(PdfExportError::Print(PdfError("the printer reported success but wrote no file".into())));
    }

    // Move next to the destination as `.part` (copy if temp is on another
    // drive), then rename, so a half-copied file never has the final name.
    let mut part_name = out_path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    part_name.push(".part");
    let part = out_path.with_file_name(part_name);
    let mut guard = PartGuard(part.clone(), true);
    let io = |e: std::io::Error| PdfExportError::Io(format!("could not write the PDF: {e}"));
    if std::fs::rename(&printed, &part).is_err() {
        std::fs::copy(&printed, &part).map_err(io)?;
    }
    std::fs::rename(&part, out_path).map_err(io)?;
    guard.1 = false;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use babelsteps_core::testing::{generate_bundle, TempDir};

    // The leftover check counts temp folders of this process, so tests must not overlap.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Fake {
        fail: bool,
    }
    impl PdfPrinter for Fake {
        fn print_to_pdf(&self, job: &PdfJob) -> Result<(), PdfError> {
            assert!(job.html_file.is_file());
            // A late write after a timeout must land in the swept temp folder.
            assert_eq!(job.out_pdf.parent(), job.html_file.parent(), "printer must write inside the temp folder");
            std::fs::write(&job.out_pdf, b"%PDF-fake").unwrap();
            if self.fail {
                Err(PdfError("boom".into()))
            } else {
                Ok(())
            }
        }
    }

    fn temp_leftovers() -> usize {
        let prefix = format!("babelsteps-pdf-{}-", std::process::id());
        std::fs::read_dir(std::env::temp_dir())
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with(&prefix))
            .count()
    }

    #[test]
    fn success_renames_and_cleans_up() {
        let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let src = TempDir::new("pdfsrc");
        let out = TempDir::new("pdfout");
        let rec = generate_bundle(src.path(), 2, 40, 20);
        let dest = out.path().join("a.pdf");
        let rep = export_pdf_with(&Fake { fail: false }, &rec, src.path(), &dest, false, Some(String::new())).unwrap();
        assert_eq!(rep.steps, 2);
        assert_eq!(std::fs::read(&dest).unwrap(), b"%PDF-fake");
        assert!(!out.path().join("a.pdf.part").exists());
        assert_eq!(temp_leftovers(), 0);
    }

    #[test]
    fn failure_leaves_no_file_and_no_temp_folder() {
        let _s = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let src = TempDir::new("pdfsrc");
        let out = TempDir::new("pdfout");
        let rec = generate_bundle(src.path(), 2, 40, 20);
        let dest = out.path().join("a.pdf");
        let r = export_pdf_with(&Fake { fail: true }, &rec, src.path(), &dest, true, Some(String::new()));
        assert!(matches!(r, Err(PdfExportError::Print(_))));
        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0, "no partial file");
        assert_eq!(temp_leftovers(), 0);
    }

    #[test]
    fn sweep_removes_own_and_abandoned_folders_only() {
        let tmp = TempDir::new("sweep");
        let own = 4242u32;
        let mk = |name: &str| {
            let p = tmp.path().join(name);
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("step-001.png"), b"x").unwrap();
            p
        };
        let mine = mk(&format!("babelsteps-pdf-{own}-0-1"));
        let other = mk("babelsteps-pdf-77-0-1");
        let unrelated = mk("babelsteps-other-4242-0-1");
        let not_our_pattern = mk("babelsteps-pdf-notes");
        std::fs::write(tmp.path().join("babelsteps-pdf-4242-0-2"), b"x").unwrap();

        // Another process's fresh folder may be an export in progress.
        assert_eq!(sweep_stale_temp_in(tmp.path(), own, std::time::Duration::from_secs(3600)), 1);
        assert!(!mine.exists());
        assert!(other.exists() && unrelated.exists());
        assert!(tmp.path().join("babelsteps-pdf-4242-0-2").exists(), "files are not folders");

        // Once old enough it is abandoned.
        assert_eq!(sweep_stale_temp_in(tmp.path(), own, std::time::Duration::ZERO), 1);
        assert!(!other.exists() && unrelated.exists() && not_our_pattern.exists());
    }

    #[cfg(windows)]
    #[test]
    fn sweep_never_follows_a_junction() {
        let tmp = TempDir::new("sweepj");
        let target = tmp.path().join("precious");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"keep").unwrap();
        let link = tmp.path().join("babelsteps-pdf-1-0-1");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok && link.exists(), "could not create the junction");
        assert_eq!(sweep_stale_temp_in(tmp.path(), 1, std::time::Duration::ZERO), 0);
        assert!(target.join("keep.txt").exists());
        assert!(link.exists(), "the junction itself is left alone");
        std::fs::remove_dir(&link).unwrap();
    }
}

//! PDF export (B8, Q7, D5). The document is prepared as HTML by `core` (images
//! validated, redactions burned in, metadata stripped) into a fresh temp
//! folder, printed to PDF by the platform webview behind [`PdfPrinter`], and
//! the result is moved into place. The temp folder is always removed, and the
//! final file only appears once printing has fully succeeded.

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
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let temp = TempFolder::new("pdf").map_err(|e| PdfExportError::Io(e.to_string()))?;
    let report = prepare_html_export(rec, source_dir, temp.path()).map_err(PdfExportError::Prepare)?;

    let mut part_name = out_path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    part_name.push(".part");
    let part = out_path.with_file_name(part_name);
    let mut guard = PartGuard(part.clone(), true);

    let job = PdfJob {
        html_file: temp.path().join("document.html"),
        out_pdf: part.clone(),
        title: rec.title.clone(),
        header_footer,
        footer_uri,
    };
    printer.print_to_pdf(&job).map_err(PdfExportError::Print)?;
    if !part.is_file() {
        return Err(PdfExportError::Print(PdfError("the printer reported success but wrote no file".into())));
    }
    std::fs::rename(&part, out_path).map_err(|e| PdfExportError::Io(format!("could not write the PDF: {e}")))?;
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
}

//! Export to a folder: `document.md` or `document.html`, plus `images/`.
//!
//! Every image is read from the recording folder, passed through
//! [`prepare_image`] (redactions burned in, metadata stripped) and written to
//! the output folder. Originals are never copied. Nothing is written until
//! every image has been validated and prepared, so a failed export leaves no
//! half-finished folder behind (apart from an empty output folder).

use crate::html::to_html;
use crate::image_export::{prepare_image, prepare_print_image, PRINT_MAX_WIDTH};
use crate::markdown::to_markdown;
use crate::model::{Block, Recording, Rect};
use crate::screenshot::ShotFormat;
use crate::storage::is_reserved_device_name;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

pub enum ExportError {
    Io(String),
    Image(String),
    /// A screenshot path that could leave the recording folder (or the images folder).
    UnsafePath { owner: String, path: String },
    /// A referenced image that is not in the recording folder.
    MissingImage { owner: String, path: String },
    /// Two blocks use one image path but need different redactions.
    ConflictingRedactions { owner: String, path: String },
    /// Two image paths that differ only in letter case. Windows and macOS would
    /// write both to one file.
    CaseCollision { owner: String, path: String },
    /// The output folder already holds a previous export (`document.md`,
    /// `document.html` or `images`). Nothing is overwritten or deleted.
    OutputNotEmpty { name: String },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Io(m) => write!(f, "file error: {m}"),
            ExportError::Image(m) => write!(f, "could not process image: {m}"),
            ExportError::UnsafePath { owner, path } => write!(f, "{owner}: unsafe image path {path:?}"),
            ExportError::MissingImage { owner, path } => write!(f, "{owner}: image file is missing ({path})"),
            ExportError::ConflictingRedactions { owner, path } => {
                write!(f, "{owner}: image {path:?} is shared with different redactions")
            }
            ExportError::CaseCollision { owner, path } => {
                write!(f, "{owner}: image {path:?} differs from another image only in letter case")
            }
            ExportError::OutputNotEmpty { name } => {
                write!(f, "the output folder already contains {name}; choose an empty folder")
            }
        }
    }
}

/// Kind and owner only: image paths can come from imported Markdown (user text),
/// and I/O messages can quote paths. `Display` is for showing the user.
impl fmt::Debug for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, owner) = match self {
            ExportError::Io(_) => ("Io", None),
            ExportError::Image(_) => ("Image", None),
            ExportError::UnsafePath { owner, .. } => ("UnsafePath", Some(owner)),
            ExportError::MissingImage { owner, .. } => ("MissingImage", Some(owner)),
            ExportError::ConflictingRedactions { owner, .. } => ("ConflictingRedactions", Some(owner)),
            ExportError::CaseCollision { owner, .. } => ("CaseCollision", Some(owner)),
            ExportError::OutputNotEmpty { .. } => ("OutputNotEmpty", None),
        };
        match owner {
            Some(owner) => write!(f, "ExportError::{kind} ({owner})"),
            None => write!(f, "ExportError::{kind}"),
        }
    }
}

impl std::error::Error for ExportError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExportReport {
    pub steps: usize,
    pub images_written: usize,
    pub images_with_redactions: usize,
    pub withheld: usize,
}

/// Accept only `images/<name>...` made of plain names joined by `/`. Rejects
/// absolute paths, drive prefixes, backslashes, `..`, `.`, empty parts, and
/// names Windows would treat specially.
pub fn validate_image_path(p: &str) -> bool {
    if p.is_empty() || p.contains(['\\', ':', '\0', '?', '*', '"', '<', '>', '|']) || p.starts_with('/') {
        return false;
    }
    let parts: Vec<&str> = p.split('/').collect();
    if parts.len() < 2 || parts[0] != "images" {
        return false;
    }
    parts.iter().all(|c| {
        !c.is_empty()
            && *c != "."
            && *c != ".."
            && !c.ends_with(['.', ' '])
            && !c.chars().any(|ch| ch.is_control())
            && !is_reserved_device_name(c)
    })
}

struct Prepared {
    files: Vec<(String, Vec<u8>)>,
    report: ExportReport,
    /// Print exports only: source image path to the exported one.
    renamed: HashMap<String, String>,
}

/// Markdown keeps each image full size and in its own format. Print (HTML and
/// PDF) shrinks images to [`PRINT_MAX_WIDTH`] and stores each as the smaller of
/// PNG and JPEG, under new names (`images/print-0001.jpg`), so the format can
/// change without two outputs colliding.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Output {
    Markdown,
    Print,
}

fn prepare_all(rec: &Recording, source_dir: &Path, output: Output) -> Result<Prepared, ExportError> {
    let root = std::fs::canonicalize(source_dir).map_err(|e| ExportError::Io(format!("recording folder: {e}")))?;
    let mut report = ExportReport::default();
    let mut jobs: Vec<Job> = Vec::new();
    // Keyed on the lowercased path: the output folder may be case-insensitive.
    let mut seen: HashMap<String, (String, Vec<Rect>)> = HashMap::new();

    let mut step_no = 0usize;
    for block in &rec.blocks {
        let (owner, path, redactions): (String, &str, &[Rect]) = match block {
            Block::Step(s) => {
                step_no += 1;
                report.steps += 1;
                if s.screenshot_withheld {
                    // Privacy: a withheld step never emits an image, whatever `screenshot` says.
                    report.withheld += 1;
                    continue;
                }
                match &s.screenshot {
                    Some(p) => (format!("step {step_no}"), p.as_str(), s.redactions.as_slice()),
                    None => continue,
                }
            }
            Block::Image { id, image, .. } => (format!("image block {id}"), image.as_str(), &[][..]),
            _ => continue,
        };

        if !validate_image_path(path) {
            return Err(ExportError::UnsafePath { owner, path: path.to_string() });
        }
        if let Some((prev_path, prev)) = seen.get(&path.to_lowercase()) {
            if prev_path != path {
                return Err(ExportError::CaseCollision { owner, path: path.to_string() });
            }
            if prev.as_slice() != redactions {
                return Err(ExportError::ConflictingRedactions { owner, path: path.to_string() });
            }
            continue;
        }

        let missing = || ExportError::MissingImage { owner: owner.clone(), path: path.to_string() };
        let full = std::fs::canonicalize(source_dir.join(path)).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { missing() } else { ExportError::Io(format!("{owner}: {e}")) }
        })?;
        // Defence in depth against symlinks pointing out of the bundle.
        if !full.starts_with(&root) {
            return Err(ExportError::UnsafePath { owner, path: path.to_string() });
        }
        if !redactions.is_empty() {
            report.images_with_redactions += 1;
        }
        seen.insert(path.to_lowercase(), (path.to_string(), redactions.to_vec()));
        jobs.push(Job { owner, path: path.to_string(), full, redactions: redactions.to_vec() });
    }

    let mut files = Vec::with_capacity(jobs.len());
    let mut renamed = HashMap::new();
    for (i, (job, prepared)) in jobs.iter().zip(prepare_parallel(&jobs, output)).enumerate() {
        let (format, bytes) = prepared?;
        let rel = match (output, format) {
            (Output::Print, Some(f)) => {
                let rel = format!("images/print-{:04}.{}", i + 1, f.extension());
                renamed.insert(job.path.clone(), rel.clone());
                rel
            }
            _ => job.path.clone(),
        };
        files.push((rel, bytes));
    }
    report.images_written = files.len();
    Ok(Prepared { files, report, renamed })
}

/// A copy of `rec` whose image references point at the renamed export files.
/// Paths that were not exported (withheld steps) are left as they are; the
/// HTML writer never shows those.
fn with_renamed_images(rec: &Recording, renamed: &HashMap<String, String>) -> Recording {
    let mut out = rec.clone();
    for block in &mut out.blocks {
        match block {
            Block::Step(s) => {
                if let Some(new) = s.screenshot.as_ref().and_then(|p| renamed.get(p)) {
                    s.screenshot = Some(new.clone());
                }
            }
            Block::Image { image, .. } => {
                if let Some(new) = renamed.get(image.as_str()) {
                    *image = new.clone();
                }
            }
            _ => {}
        }
    }
    out
}

/// An image that passed every path check, waiting to be read and prepared.
struct Job {
    owner: String,
    path: String,
    full: PathBuf,
    redactions: Vec<Rect>,
}

/// The prepared bytes, and for print exports the format they were encoded in.
type PreparedImage = (Option<ShotFormat>, Vec<u8>);

fn prepare_one(job: &Job, output: Output) -> Result<PreparedImage, ExportError> {
    let owner = &job.owner;
    let bytes = std::fs::read(&job.full).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ExportError::MissingImage { owner: owner.clone(), path: job.path.clone() }
        } else {
            ExportError::Io(format!("{owner}: {e}"))
        }
    })?;
    let prepared = match output {
        Output::Markdown => prepare_image(&bytes, &job.redactions, None).map(|b| (None, b)),
        Output::Print => prepare_print_image(&bytes, &job.redactions, Some(PRINT_MAX_WIDTH)).map(|(f, b)| (Some(f), b)),
    };
    prepared.map_err(|e| match e {
        ExportError::Image(m) => ExportError::Image(format!("{owner}: {m}")),
        other => other,
    })
}

/// Decoding, painting and encoding take about 0.1 s per ultrawide screenshot,
/// so images are prepared on a few threads. Capped at 8: each one holds a
/// decoded full-size image (about 30 MB at 5120x1440). Results keep job order,
/// so the first error reported is the first in the document.
fn prepare_parallel(jobs: &[Job], output: Output) -> Vec<Result<PreparedImage, ExportError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8).min(jobs.len());
    if threads <= 1 {
        return jobs.iter().map(|j| prepare_one(j, output)).collect();
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Result<PreparedImage, ExportError>>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = jobs.get(i) else { break };
                let r = prepare_one(job, output);
                results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err(ExportError::Image("image was not prepared".into()))))
        .collect()
}

/// Refuses a folder that holds an earlier export, so stale files can never mix
/// with new ones and nothing of the user's is deleted or overwritten.
fn check_output_clear(out_dir: &Path) -> Result<(), ExportError> {
    for name in ["document.md", "document.html", "images"] {
        if std::fs::symlink_metadata(out_dir.join(name)).is_ok() {
            return Err(ExportError::OutputNotEmpty { name: name.to_string() });
        }
    }
    Ok(())
}

fn write_bundle(out_dir: &Path, doc_name: &str, doc: &str, files: &[(String, Vec<u8>)]) -> Result<(), ExportError> {
    let io = |e: std::io::Error| ExportError::Io(e.to_string());
    std::fs::create_dir_all(out_dir.join("images")).map_err(io)?;
    for (rel, bytes) in files {
        let dest: PathBuf = out_dir.join(rel); // `rel` passed validate_image_path
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&dest, bytes).map_err(io)?;
    }
    std::fs::write(out_dir.join(doc_name), doc).map_err(io)?;
    Ok(())
}

/// Write `document.md` and `images/` into `out_dir`.
pub fn export_markdown(rec: &Recording, source_dir: &Path, out_dir: &Path) -> Result<ExportReport, ExportError> {
    check_output_clear(out_dir)?;
    let p = prepare_all(rec, source_dir, Output::Markdown)?;
    write_bundle(out_dir, "document.md", &to_markdown(rec), &p.files)?;
    Ok(p.report)
}

/// Write `document.html` and `images/` into `out_dir`, ready for a webview to
/// print (PDF) or to keep as an HTML export. Images wider than
/// [`PRINT_MAX_WIDTH`] are scaled down and stored as the smaller of PNG and
/// JPEG under new names (see [`Output`]); Markdown exports keep full size.
pub fn prepare_html_export(rec: &Recording, source_dir: &Path, out_dir: &Path) -> Result<ExportReport, ExportError> {
    check_output_clear(out_dir)?;
    let p = prepare_all(rec, source_dir, Output::Print)?;
    write_bundle(out_dir, "document.html", &to_html(&with_renamed_images(rec, &p.renamed)), &p.files)?;
    Ok(p.report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use crate::testing::{generate_bundle, TempDir};

    fn with_shot(path: &str) -> Recording {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(Block::Step(StepBlock {
            id: "s".into(),
            action: Action::Manual,
            title: "x".into(),
            description: String::new(),
            screenshot: Some(path.into()),
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        }));
        rec
    }

    #[test]
    fn path_validation() {
        for ok in ["images/step-001.png", "images/sub/a b (1).png"] {
            assert!(validate_image_path(ok), "{ok}");
        }
        for bad in [
            "", "/etc/passwd", "C:/x.png", "C:\\x.png", "c:x.png", "../secret.png", "images/../../x.png",
            "images/..", "images\\..\\x.png", "images/a\\b.png", "\\\\server\\share\\x.png", "images//x.png",
            "./images/x.png", "images/./x.png", "x.png", "document.md", "images", "images/", "images/CON.png",
            "images/x.png.", "images/x\0.png", "images/a:b.png",
        ] {
            assert!(!validate_image_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn all_windows_device_names_are_rejected() {
        for name in ["COM3", "com9.png", "LPT3", "lpt9.txt", "CONIN$", "AUX", "Nul.x"] {
            assert!(!validate_image_path(&format!("images/{name}")), "{name}");
            assert!(!validate_image_path(&format!("images/sub/{name}")), "{name}");
        }
        assert!(validate_image_path("images/COM10.png"));
        assert!(validate_image_path("images/com.png"));
    }

    #[test]
    fn case_only_collision_is_an_error_even_with_equal_redactions() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let mut rec = generate_bundle(src.path(), 1, 8, 8);
        let mut second = rec.blocks[0].clone();
        if let Block::Step(s) = &mut second {
            s.screenshot = s.screenshot.as_ref().map(|p| p.to_uppercase().replace("IMAGES/", "images/"));
        }
        // Same redactions, so only the case differs.
        assert_ne!(rec.blocks[0], second);
        rec.blocks.push(second);
        let r = export_markdown(&rec, src.path(), out.path());
        assert!(matches!(r, Err(ExportError::CaseCollision { .. })), "{r:?}");
        assert!(!out.path().join("images").exists());
    }

    #[test]
    fn the_first_broken_image_in_the_document_is_the_one_reported() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let rec = generate_bundle(src.path(), 30, 8, 8);
        let shots: Vec<String> = rec
            .blocks
            .iter()
            .filter_map(|b| if let Block::Step(s) = b { s.screenshot.clone() } else { None })
            .collect();
        // Break steps 7 and 25; images are prepared on several threads.
        for i in [6, 24] {
            std::fs::write(src.path().join(&shots[i]), b"not a png").unwrap();
        }
        for _ in 0..5 {
            match prepare_html_export(&rec, src.path(), out.path()) {
                Err(ExportError::Image(m)) => assert!(m.starts_with("step 7:"), "{m}"),
                other => panic!("expected an image error, got {other:?}"),
            }
        }
        assert!(!out.path().join("images").exists());
    }

    #[test]
    fn wide_images_shrink_for_html_but_not_markdown() {
        let src = TempDir::new("src");
        let rec = generate_bundle(src.path(), 1, PRINT_MAX_WIDTH + 400, 10);
        let Block::Step(s) = &rec.blocks[0] else { panic!("expected a step") };
        let shot = s.screenshot.clone().unwrap();
        let width = |p: PathBuf| image::image_dimensions(p).unwrap().0;
        let (md, html) = (TempDir::new("md"), TempDir::new("html"));
        export_markdown(&rec, src.path(), md.path()).unwrap();
        prepare_html_export(&rec, src.path(), html.path()).unwrap();
        assert_eq!(width(md.path().join(&shot)), PRINT_MAX_WIDTH + 400);
        let doc = std::fs::read_to_string(html.path().join("document.html")).unwrap();
        let printed = ["images/print-0001.png", "images/print-0001.jpg"].into_iter().find(|p| doc.contains(p)).unwrap();
        assert_eq!(width(html.path().join(printed)), PRINT_MAX_WIDTH);
        assert!(!doc.contains(&shot), "the document points at the renamed file");
    }

    #[test]
    fn print_export_renames_shared_images_once_and_skips_withheld() {
        let src = TempDir::new("src");
        let mut rec = generate_bundle(src.path(), 3, 40, 20);
        let shots: Vec<String> = rec.blocks.iter().filter_map(|b| match b {
            Block::Step(s) => s.screenshot.clone(),
            _ => None,
        }).collect();
        // Step 3 reuses step 1's image (same redactions); step 2 is withheld.
        let Block::Step(first) = rec.blocks[0].clone() else { panic!("expected a step") };
        if let Block::Step(s) = &mut rec.blocks[2] {
            s.screenshot = Some(shots[0].clone());
            s.redactions = first.redactions.clone();
        }
        if let Block::Step(s) = &mut rec.blocks[1] {
            s.screenshot_withheld = true;
        }
        let out = TempDir::new("html");
        let report = prepare_html_export(&rec, src.path(), out.path()).unwrap();
        assert_eq!(report.images_written, 1);
        let doc = std::fs::read_to_string(out.path().join("document.html")).unwrap();
        let name = ["images/print-0001.png", "images/print-0001.jpg"].into_iter().find(|p| doc.contains(p)).unwrap();
        assert_eq!(doc.matches(name).count(), 2, "both steps show the one exported file");
        assert!(out.path().join(name).is_file());
        assert_eq!(std::fs::read_dir(out.path().join("images")).unwrap().count(), 1);
    }

    #[test]
    fn identical_paths_with_identical_redactions_are_shared() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let mut rec = generate_bundle(src.path(), 1, 8, 8);
        let again = rec.blocks[0].clone();
        rec.blocks.push(again);
        let r = export_markdown(&rec, src.path(), out.path()).unwrap();
        assert_eq!(r.images_written, 1);
    }

    #[test]
    fn a_folder_with_a_previous_export_is_refused_and_left_alone() {
        let src = TempDir::new("src");
        let rec = generate_bundle(src.path(), 1, 8, 8);
        for stale in ["document.md", "document.html", "images"] {
            for html in [false, true] {
                let out = TempDir::new("out");
                let mark = out.path().join(stale);
                if stale == "images" {
                    std::fs::create_dir(&mark).unwrap();
                    std::fs::write(mark.join("mine.txt"), "keep").unwrap();
                } else {
                    std::fs::write(&mark, "keep").unwrap();
                }
                let r = if html {
                    prepare_html_export(&rec, src.path(), out.path())
                } else {
                    export_markdown(&rec, src.path(), out.path())
                };
                assert!(matches!(r, Err(ExportError::OutputNotEmpty { .. })), "{stale} html={html}: {r:?}");
                let kept = if stale == "images" { mark.join("mine.txt") } else { mark };
                assert_eq!(std::fs::read_to_string(kept).unwrap(), "keep");
                let entries = std::fs::read_dir(out.path()).unwrap().count();
                assert_eq!(entries, 1, "nothing may be added");
            }
        }
    }

    #[test]
    fn exporting_twice_into_one_folder_is_refused_the_second_time() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let rec = generate_bundle(src.path(), 1, 8, 8);
        export_markdown(&rec, src.path(), out.path()).unwrap();
        assert!(matches!(export_markdown(&rec, src.path(), out.path()), Err(ExportError::OutputNotEmpty { .. })));
        assert!(matches!(prepare_html_export(&rec, src.path(), out.path()), Err(ExportError::OutputNotEmpty { .. })));
    }

    #[test]
    fn unsafe_paths_are_errors_and_write_nothing() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        for bad in ["../x.png", "/abs.png", "C:\\x.png", "images\\..\\..\\x.png"] {
            let r = export_markdown(&with_shot(bad), src.path(), &out.path().join("o"));
            assert!(matches!(r, Err(ExportError::UnsafePath { .. })), "{bad}");
            let r = prepare_html_export(&with_shot(bad), src.path(), &out.path().join("o"));
            assert!(matches!(r, Err(ExportError::UnsafePath { .. })), "{bad}");
        }
        assert!(!out.path().join("o").exists());
    }

    #[test]
    fn missing_image_names_the_step() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let err = export_markdown(&with_shot("images/nope.png"), src.path(), out.path()).unwrap_err();
        assert!(matches!(err, ExportError::MissingImage { .. }));
        assert!(err.to_string().contains("step 1"), "{err}");
    }

    #[test]
    fn withheld_step_emits_no_image_even_with_a_value() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        // The file does not exist: a withheld step must not even look for it.
        let mut rec = with_shot("images/secret.png");
        if let Block::Step(s) = &mut rec.blocks[0] {
            s.screenshot_withheld = true;
        }
        let report = export_markdown(&rec, src.path(), out.path()).unwrap();
        assert_eq!(report, ExportReport { steps: 1, images_written: 0, images_with_redactions: 0, withheld: 1 });
        assert!(!out.path().join("images/secret.png").exists());
    }

    #[test]
    fn report_counts() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let mut rec = generate_bundle(src.path(), 3, 16, 16);
        if let Block::Step(s) = &mut rec.blocks[2] {
            s.redactions.clear();
        }
        let r = export_markdown(&rec, src.path(), out.path()).unwrap();
        assert_eq!(r, ExportReport { steps: 3, images_written: 3, images_with_redactions: 2, withheld: 0 });
    }

    #[test]
    fn shared_image_with_different_redactions_is_an_error() {
        let src = TempDir::new("src");
        let out = TempDir::new("out");
        let mut rec = generate_bundle(src.path(), 1, 8, 8);
        let mut second = rec.blocks[0].clone();
        if let Block::Step(s) = &mut second {
            s.redactions.clear();
        }
        rec.blocks.push(second);
        let r = export_markdown(&rec, src.path(), out.path());
        assert!(matches!(r, Err(ExportError::ConflictingRedactions { .. })));
    }
}

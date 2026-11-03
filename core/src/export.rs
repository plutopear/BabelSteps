//! Export to a folder: `document.md` or `document.html`, plus `images/`.
//!
//! Every image is read from the recording folder, passed through
//! [`prepare_image`] (redactions burned in, metadata stripped) and written to
//! the output folder. Originals are never copied. Nothing is written until
//! every image has been validated and prepared, so a failed export leaves no
//! half-finished folder behind (apart from an empty output folder).

use crate::html::to_html;
use crate::image_export::prepare_image;
use crate::markdown::to_markdown;
use crate::model::{Block, Recording, Rect};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ExportError {
    Io(String),
    Image(String),
    /// A screenshot path that could leave the recording folder (or the images folder).
    UnsafePath { owner: String, path: String },
    /// A referenced image that is not in the recording folder.
    MissingImage { owner: String, path: String },
    /// Two blocks use one image path but need different redactions.
    ConflictingRedactions { owner: String, path: String },
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

const RESERVED: [&str; 8] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2"];

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
            && !RESERVED.contains(&c.split('.').next().unwrap_or("").to_ascii_uppercase().as_str())
    })
}

struct Prepared {
    files: Vec<(String, Vec<u8>)>,
    report: ExportReport,
}

fn prepare_all(rec: &Recording, source_dir: &Path) -> Result<Prepared, ExportError> {
    let root = std::fs::canonicalize(source_dir).map_err(|e| ExportError::Io(format!("recording folder: {e}")))?;
    let mut report = ExportReport::default();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut seen: HashMap<String, Vec<Rect>> = HashMap::new();

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
        if let Some(prev) = seen.get(path) {
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
        let bytes = std::fs::read(&full).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound { missing() } else { ExportError::Io(format!("{owner}: {e}")) }
        })?;
        let prepared = prepare_image(&bytes, redactions).map_err(|e| match e {
            ExportError::Image(m) => ExportError::Image(format!("{owner}: {m}")),
            other => other,
        })?;
        report.images_written += 1;
        if !redactions.is_empty() {
            report.images_with_redactions += 1;
        }
        seen.insert(path.to_string(), redactions.to_vec());
        files.push((path.to_string(), prepared));
    }
    Ok(Prepared { files, report })
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
    let p = prepare_all(rec, source_dir)?;
    write_bundle(out_dir, "document.md", &to_markdown(rec), &p.files)?;
    Ok(p.report)
}

/// Write `document.html` and `images/` into `out_dir`, ready for a webview to
/// print (PDF) or to keep as an HTML export.
pub fn prepare_html_export(rec: &Recording, source_dir: &Path, out_dir: &Path) -> Result<ExportReport, ExportError> {
    let p = prepare_all(rec, source_dir)?;
    write_bundle(out_dir, "document.html", &to_html(rec), &p.files)?;
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

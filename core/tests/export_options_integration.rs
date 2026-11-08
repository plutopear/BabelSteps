//! Export options (PSR-2, PSR-3, PSR-5, PSR-6): checked in the files actually
//! written, in both formats, with the options off (the default) and on.

use babelsteps_core::export::{export_markdown_with, prepare_html_export_with};
use babelsteps_core::export_options::ExportOptions;
use babelsteps_core::markdown::{to_markdown, to_markdown_with};
use babelsteps_core::markdown_import::from_markdown;
use babelsteps_core::notice::{apply_typed_text_notice, TYPED_TEXT_NOT_RECORDED};
use babelsteps_core::testing::{generate_bundle, TempDir};
use babelsteps_core::*;
use std::fs;

// 2026-11-08 14:05:09 UTC
const T: u64 = 1_794_146_709_000;
const WINDOW_TITLE: &str = "C:\\Users\\jdoe\\Payroll-Q4.xlsx - Excel";

/// Three click steps on disk: the second withheld, the third with no screenshot,
/// all with a target that carries a window title that must never be exported.
fn fixture(src: &std::path::Path) -> Recording {
    let mut rec = generate_bundle(src, 3, 8, 8);
    // The notice a new recording gets when it starts with keystrokes off.
    let mut fresh = Recording::new("r", "t", 0);
    apply_typed_text_notice(&mut fresh, false);
    rec.blocks.insert(0, fresh.blocks.remove(0));
    for (i, b) in rec.blocks.iter_mut().enumerate() {
        if let Block::Step(s) = b {
            s.captured_unix_ms = T + i as u64 * 61_000;
            s.target = Some(Target {
                name: "Save <now> & \"go\"".into(),
                control_type: "Button".into(),
                window_title: Some(WINDOW_TITLE.into()),
                process_name: "excel.exe".into(),
                is_password: false,
                bounds: None,
            });
            match i {
                2 => s.screenshot_withheld = true,
                3 => s.screenshot = None,
                _ => {}
            }
        }
    }
    rec
}

fn on() -> ExportOptions {
    ExportOptions {
        step_times: true,
        technical_details: true,
        utc_offset_minutes: 60,
        os: "Windows 11 (build 22631)".into(),
        app: "BabelSteps 0.1.0".into(),
    }
}

#[test]
fn defaults_add_no_times_or_details_but_keep_the_notice() {
    let src = TempDir::new("src");
    let rec = fixture(src.path());
    let (md_dir, html_dir) = (TempDir::new("md"), TempDir::new("html"));
    let off = ExportOptions::default();
    export_markdown_with(&rec, src.path(), md_dir.path(), &off).unwrap();
    prepare_html_export_with(&rec, src.path(), html_dir.path(), &off).unwrap();
    let md = fs::read_to_string(md_dir.path().join("document.md")).unwrap();
    let html = fs::read_to_string(html_dir.path().join("document.html")).unwrap();
    assert_eq!(md, to_markdown(&rec), "default options must not change the Markdown");
    for doc in [&md, &html] {
        assert!(doc.contains(TYPED_TEXT_NOT_RECORDED), "the notice is in every export by default");
        for absent in ["Time:", "Recorded on", "Technical details", "excel.exe", "15:05:09", "22631"] {
            assert!(!doc.contains(absent), "{absent:?} appeared with the options off");
        }
    }
}

#[test]
fn options_on_add_times_and_details_in_both_formats() {
    let src = TempDir::new("src");
    let rec = fixture(src.path());
    let (md_dir, html_dir) = (TempDir::new("md"), TempDir::new("html"));
    export_markdown_with(&rec, src.path(), md_dir.path(), &on()).unwrap();
    prepare_html_export_with(&rec, src.path(), html_dir.path(), &on()).unwrap();
    let md = fs::read_to_string(md_dir.path().join("document.md")).unwrap();
    let html = fs::read_to_string(html_dir.path().join("document.html")).unwrap();

    // UTC+1: 14:05:09 UTC is 15:05:09 local; steps are 61 s apart.
    assert!(md.contains("\nRecorded on 8 November 2026.\n"), "{md}");
    assert!(md.contains("## Step 1. Left-click at (1, 1)\n\n*Time: 15:06:10*\n"), "{md}");
    assert!(md.contains("*Time: 15:08:12*"));
    assert!(md.contains("\n## Technical details\n\n- Recording session: 8 November 2026, 15:06:10 to 15:08:12\n"));
    assert!(md.contains("- Recorded steps: 3\n"));
    assert!(md.contains("- Screenshots withheld: 1 (the password check failed or could not finish)\n"));
    assert!(md.contains("- Steps without a screenshot: 1\n"));
    assert!(md.contains("- Operating system: Windows 11 (build 22631)\n"));
    assert!(md.contains("- Step 2: excel.exe, Button \u{201c}Save \\<now\\> & \"go\"\u{201d}\n"), "{md}");

    assert!(html.contains("<p class=\"recorded-on\">Recorded on 8 November 2026.</p>"));
    assert!(html.contains("<p class=\"step-time\">Time: 15:06:10</p>"));
    assert!(html.contains("<h2>Technical details</h2>"));
    assert!(html.contains("<li>Recorded steps: 3</li>"));
    assert!(html.contains("<li>Step 2: excel.exe, Button \u{201c}Save &lt;now&gt; &amp; &quot;go&quot;\u{201d}</li>"), "{html}");
    assert!(!html.contains("<now>"), "element names are escaped");

    for doc in [&md, &html] {
        assert!(doc.contains(TYPED_TEXT_NOT_RECORDED));
        assert!(!doc.contains("jdoe") && !doc.contains("Payroll"), "window titles must never be exported");
        // The details come after every step.
        assert!(doc.find("Technical details").unwrap() > doc.rfind("Step 3. ").unwrap());
    }
}

#[test]
fn markdown_with_times_imports_back_to_the_same_steps() {
    let src = TempDir::new("src");
    let rec = fixture(src.path());
    let times_only = ExportOptions { step_times: true, ..on() };
    let times_only = ExportOptions { technical_details: false, ..times_only };
    let md = to_markdown_with(&rec, &times_only);
    let back = from_markdown(&md);
    // The time lines are dropped, not turned into descriptions. The date line
    // at the top comes back as an ordinary paragraph.
    for b in &back.blocks {
        if let Block::Step(s) = b {
            assert!(s.description.is_empty(), "time line became a description: {:?}", s.description);
        }
    }
    assert_eq!(back.step_count(), 3);
    let first_text = back.blocks.iter().find_map(|b| match b {
        Block::Text { text, .. } => Some(text.as_str()),
        _ => None,
    });
    assert_eq!(first_text, Some("Recorded on 8 November 2026."));
}

#[test]
fn a_user_edited_or_deleted_notice_is_exported_as_left() {
    let src = TempDir::new("src");
    let mut rec = fixture(src.path());
    if let Block::Text { text, .. } = &mut rec.blocks[0] {
        *text = "Only shortcuts were captured.".into();
    }
    let md = to_markdown(&rec);
    assert!(md.contains("Only shortcuts were captured.") && !md.contains(TYPED_TEXT_NOT_RECORDED));
    rec.blocks.remove(0);
    let md = to_markdown(&rec);
    assert!(!md.contains("Typed text") && !md.contains("Only shortcuts"));
}

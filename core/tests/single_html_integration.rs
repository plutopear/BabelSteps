//! Single-file HTML export (B10), checked in the file actually written: the
//! redaction boxes are black in the embedded pictures, nothing in the file
//! points outside it, and the options behave as in the other exports.

use babelsteps_core::export::{export_single_html_with, ExportError};
use babelsteps_core::export_options::ExportOptions;
use babelsteps_core::testing::{png_with_extra_chunks, solid_png, TempDir};
use babelsteps_core::*;
use std::fs;
use std::path::Path;

const FILL: [u8; 4] = [10, 200, 30, 255];

/// Standard Base64 decoder for the test only, written separately from the
/// encoder so a shared mistake cannot hide.
fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not Base64: {:?}", c as char),
    };
    let bytes = s.as_bytes();
    assert_eq!(bytes.len() % 4, 0, "padding");
    let mut out = Vec::new();
    for q in bytes.chunks(4) {
        let pad = q.iter().filter(|&&c| c == b'=').count();
        let n = q[..4 - pad].iter().fold(0u32, |a, &c| (a << 6) | u32::from(val(c))) << (6 * pad);
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

/// Every `src="..."` value in the page, in order.
fn img_srcs(html: &str) -> Vec<&str> {
    html.split("src=\"").skip(1).map(|rest| &rest[..rest.find('"').unwrap()]).collect()
}

fn decode_uri(uri: &str) -> image::RgbaImage {
    let b64 = uri.strip_prefix("data:image/png;base64,").expect("a PNG data URI");
    image::load_from_memory(&b64_decode(b64)).unwrap().to_rgba8()
}

fn step(id: &str, shot: Option<&str>, reds: Vec<Rect>, withheld: bool) -> Block {
    Block::Step(StepBlock {
        id: id.into(),
        action: Action::Manual,
        title: format!("Step {id}"),
        description: "Open the **file** menu".into(),
        screenshot: shot.map(String::from),
        target: None,
        redactions: reds,
        captured_unix_ms: 0,
        screenshot_withheld: withheld,
    })
}

/// Step a has a box, b has none, c is withheld (its file exists and must not appear).
fn fixture(src: &Path) -> Recording {
    fs::create_dir_all(src.join("images")).unwrap();
    fs::write(src.join("images/a.png"), png_with_extra_chunks(&solid_png(40, 30, FILL))).unwrap();
    fs::write(src.join("images/b.png"), solid_png(40, 30, FILL)).unwrap();
    fs::write(src.join("images/c.png"), solid_png(5, 5, [1, 2, 3, 255])).unwrap();
    fs::write(src.join("images/i.png"), solid_png(12, 12, FILL)).unwrap();
    let mut rec = Recording::new("r", "Single file", 0);
    rec.blocks.push(step("a", Some("images/a.png"), vec![Rect { x: 10, y: 5, w: 10, h: 8 }], false));
    rec.blocks.push(step("b", Some("images/b.png"), vec![], false));
    rec.blocks.push(step("c", Some("images/c.png"), vec![], true));
    rec.blocks.push(Block::Image { id: "i".into(), image: "images/i.png".into(), caption: "cap".into() });
    rec
}

fn export(rec: &Recording, src: &Path, out: &Path, opts: &ExportOptions) -> String {
    export_single_html_with(rec, src, out, opts).unwrap();
    fs::read_to_string(out).unwrap()
}

#[test]
fn redaction_boxes_are_black_in_the_embedded_pictures() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    let rec = fixture(src.path());
    let file = out.path().join("guide.html");
    let report = export_single_html_with(&rec, src.path(), &file, &ExportOptions::default()).unwrap();
    assert_eq!((report.steps, report.images_written, report.images_with_redactions, report.withheld), (3, 3, 1, 1));
    let html = fs::read_to_string(&file).unwrap();

    // Step a, step b, then the image block: three pictures (the withheld one is not among them).
    let srcs = img_srcs(&html);
    assert_eq!(srcs.len(), 3);
    assert_eq!(decode_uri(srcs[2]).dimensions(), (12, 12));

    let a = decode_uri(srcs[0]);
    assert_eq!(a.dimensions(), (40, 30));
    for y in 0..30 {
        for x in 0..40 {
            let inside = (10..20).contains(&x) && (5..13).contains(&y);
            let want = if inside { [0, 0, 0, 255] } else { FILL };
            assert_eq!(a.get_pixel(x, y).0, want, "pixel ({x},{y})");
        }
    }
    // The check can fail: step b has no box and keeps the original colour there.
    let b = decode_uri(srcs[1]);
    assert_eq!(b.get_pixel(15, 8).0, FILL);

    // The original's metadata chunks did not come along.
    let raw = b64_decode(srcs[0].strip_prefix("data:image/png;base64,").unwrap());
    assert!(!raw.windows(11).any(|w| w == b"SECRET-META"));

    // The withheld step's picture is nowhere, and no partial file is left.
    assert!(html.contains("withheld"));
    assert!(!html.contains("c.png"));
    let names: Vec<_> = fs::read_dir(out.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("guide.html")]);
}

#[test]
fn the_file_refers_to_nothing_outside_itself() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    let rec = fixture(src.path());
    let opts = ExportOptions {
        step_times: true,
        technical_details: true,
        utc_offset_minutes: 0,
        os: "Windows 11".into(),
        app: "BabelSteps 0.1.0".into(),
    };
    let html = export(&rec, src.path(), &out.path().join("g.html"), &opts);
    let lower = html.to_lowercase();
    let banned_text = [
        "http://", "https://", "//", "<script", "<link", "<iframe", "<object", "<embed", "<a ", "href", "url(", "@import",
        "file:", "srcset", "onerror", "onload",
    ];
    for banned in banned_text {
        // "//" would also catch the harmless "</" paths, so only look outside data URIs.
        if banned == "//" {
            let stripped: String = lower.split("src=\"").enumerate().map(|(i, p)| if i == 0 { p } else { &p[p.find('"').unwrap()..] }).collect();
            assert!(!stripped.contains("//"), "found //");
            continue;
        }
        assert!(!lower.contains(banned), "found {banned:?}");
    }
    // Every `src=` is a data URI, and the policy allows data: pictures only.
    let srcs = img_srcs(&html);
    assert!(!srcs.is_empty());
    for s in &srcs {
        assert!(s.starts_with("data:image/png;base64,"), "{}", &s[..s.len().min(40)]);
    }
    assert_eq!(lower.matches("src=").count(), srcs.len(), "a src= that is not in a quoted attribute");
    assert!(html.contains("content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'\""));
    assert!(!html.contains("img-src 'self'") && !html.contains("file:"));
}

#[test]
fn the_picture_data_is_about_a_third_larger_than_the_pictures() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    let rec = fixture(src.path());
    let html = export(&rec, src.path(), &out.path().join("g.html"), &ExportOptions::default());
    for s in img_srcs(&html) {
        let b64 = s.strip_prefix("data:image/png;base64,").unwrap();
        let bytes = b64_decode(b64).len();
        assert_eq!(b64.len(), bytes.div_ceil(3) * 4);
    }
}

#[test]
fn options_off_add_nothing_and_on_add_times_and_details() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    let mut rec = fixture(src.path());
    for b in &mut rec.blocks {
        if let Block::Step(s) = b {
            s.captured_unix_ms = 1_794_146_709_000; // 2026-11-08 14:05:09 UTC
            s.target = Some(Target {
                name: "Save".into(),
                control_type: "Button".into(),
                window_title: Some("C:\\Users\\jdoe\\Payroll.xlsx - Excel".into()),
                process_name: "excel.exe".into(),
                is_password: false,
                bounds: None,
            });
        }
    }
    let off = export(&rec, src.path(), &out.path().join("off.html"), &ExportOptions::default());
    for absent in ["Time:", "Recorded on", "Technical details", "excel.exe", "Payroll", "jdoe"] {
        assert!(!off.contains(absent), "{absent:?} appeared with the options off");
    }
    let opts = ExportOptions { step_times: true, technical_details: true, utc_offset_minutes: 60, os: "Windows 11 (build 22631)".into(), app: "BabelSteps 0.1.0".into() };
    let on = export(&rec, src.path(), &out.path().join("on.html"), &opts);
    for present in ["Time: 15:05:09", "Recorded on 8 November 2026", "Technical details", "excel.exe", "22631"] {
        assert!(on.contains(present), "{present:?} missing with the options on");
    }
    assert!(!on.contains("Payroll") && !on.contains("jdoe"), "window titles never go into an export");
}

#[test]
fn a_failed_export_writes_nothing() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    let mut rec = fixture(src.path());
    rec.blocks.push(step("x", Some("images/missing.png"), vec![], false));
    let file = out.path().join("g.html");
    let r = export_single_html_with(&rec, src.path(), &file, &ExportOptions::default());
    assert!(matches!(r, Err(ExportError::MissingImage { .. })), "{r:?}");
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);

    let mut bad = fixture(src.path());
    bad.blocks.push(step("y", Some("../x.png"), vec![], false));
    let r = export_single_html_with(&bad, src.path(), &file, &ExportOptions::default());
    assert!(matches!(r, Err(ExportError::UnsafePath { .. })), "{r:?}");
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[test]
fn golden_small_document() {
    let src = TempDir::new("src");
    let out = TempDir::new("out");
    fs::create_dir_all(src.path().join("images")).unwrap();
    fs::write(src.path().join("images/step-001.png"), solid_png(4, 3, [200, 100, 50, 255])).unwrap();
    let mut rec = Recording::new("g", "Golden guide", 0);
    rec.blocks.push(Block::Text { id: "t".into(), text: "Short **intro**.".into() });
    rec.blocks.push(step("1", Some("images/step-001.png"), vec![Rect { x: 1, y: 1, w: 2, h: 1 }], false));
    rec.blocks.push(Block::Callout { id: "c".into(), kind: CalloutKind::Tip, text: "Check the screenshot.".into() });
    let html = export(&rec, src.path(), &out.path().join("g.html"), &ExportOptions::default());

    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/single.html");
    if std::env::var_os("BLESS_GOLDEN").is_some() {
        fs::write(path, &html).unwrap();
    }
    let expected = fs::read_to_string(path).expect("golden file missing; run with BLESS_GOLDEN=1");
    assert_eq!(html.replace("\r\n", "\n"), expected.replace("\r\n", "\n"));
}

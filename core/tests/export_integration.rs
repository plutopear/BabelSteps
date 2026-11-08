//! End-to-end export: build a bundle on disk, export both formats, then check
//! the files that were actually written.

use babelsteps_core::export::{export_markdown, prepare_html_export, ExportReport};
use babelsteps_core::testing::{generate_bundle, png_chunks, png_with_extra_chunks, solid_png, TempDir};
use babelsteps_core::*;
use std::fs;
use std::path::Path;

const SECRET: &str = "SECRET-WINDOW-TITLE-Q4-salaries";
const FILL: [u8; 4] = [10, 200, 30, 255];

fn fixture(src: &Path) -> Recording {
    fs::create_dir_all(src.join("images")).unwrap();
    // Inputs carry metadata that must not survive.
    fs::write(src.join("images/step-001.png"), png_with_extra_chunks(&solid_png(40, 30, FILL))).unwrap();
    fs::write(src.join("images/step-002.png"), png_with_extra_chunks(&solid_png(40, 30, FILL))).unwrap();
    fs::write(src.join("images/extra (1).png"), png_with_extra_chunks(&solid_png(12, 12, FILL))).unwrap();
    // A withheld step's file exists on disk but must never be exported.
    fs::write(src.join("images/withheld.png"), solid_png(5, 5, [1, 2, 3, 255])).unwrap();

    let step = |id: &str, shot: Option<&str>, reds: Vec<Rect>, withheld: bool| {
        Block::Step(StepBlock {
            id: id.into(),
            action: Action::Click { button: MouseButton::Left, x: 1, y: 1 },
            title: format!("Step {id}"),
            description: String::new(),
            screenshot: shot.map(String::from),
            target: Some(Target {
                name: "OK".into(),
                control_type: "Button".into(),
                window_title: Some(SECRET.into()),
                process_name: "notepad.exe".into(),
                is_password: false,
                bounds: Some(Rect { x: 0, y: 0, w: 5, h: 5 }),
            }),
            redactions: reds,
            captured_unix_ms: 0,
            screenshot_withheld: withheld,
        })
    };
    let mut rec = Recording::new("r", "Integration", 0);
    rec.blocks.push(step("a", Some("images/step-001.png"), vec![Rect { x: 10, y: 5, w: 10, h: 8 }], false));
    rec.blocks.push(step("b", Some("images/step-002.png"), vec![], false));
    rec.blocks.push(step("c", Some("images/withheld.png"), vec![], true));
    rec.blocks.push(Block::Image { id: "i".into(), image: "images/extra (1).png".into(), caption: "cap".into() });
    rec
}

fn decode(path: &Path) -> image::RgbaImage {
    image::open(path).unwrap().to_rgba8()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn all_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            v.extend(all_files(&p));
        } else {
            v.push(p);
        }
    }
    v
}

/// `first` and `second` are where steps a and b were written: their own names
/// in Markdown, `images/print-0001.png` and so on in print exports.
fn check_images(out: &Path, first: &str, second: &str) {
    // Redacted pixels are black in the written file; one pixel outside is unchanged.
    let a = decode(&out.join(first));
    for (x, y) in [(10, 5), (19, 12), (15, 8)] {
        assert_eq!(a.get_pixel(x, y).0, [0, 0, 0, 255], "inside ({x},{y})");
    }
    for (x, y) in [(9, 5), (20, 5), (10, 4), (10, 13), (0, 0), (39, 29)] {
        assert_eq!(a.get_pixel(x, y).0, FILL, "outside ({x},{y})");
    }
    // The unredacted step is untouched.
    let b = decode(&out.join(second));
    assert!(b.pixels().all(|p| p.0 == FILL));

    // Metadata is gone from every written image.
    for f in all_files(&out.join("images")) {
        let bytes = fs::read(&f).unwrap();
        for (name, _) in png_chunks(&bytes) {
            assert!(["IHDR", "IDAT", "IEND", "PLTE"].contains(&name.as_str()), "{f:?} has {name}");
        }
        assert!(!bytes.windows(11).any(|w| w == b"SECRET-META"));
    }
    // The withheld step's image was not exported.
    assert!(!out.join("images/withheld.png").exists());
    assert_eq!(all_files(&out.join("images")).len(), 3);
}

fn check_no_leaks(out: &Path, src: &Path) {
    let src_s = src.to_string_lossy().to_string();
    let tmp_s = std::env::temp_dir().to_string_lossy().to_string();
    for f in all_files(out) {
        let bytes = fs::read(&f).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(SECRET), "{f:?} leaks the window title");
        assert!(!text.contains(&src_s), "{f:?} contains the source path");
        assert!(!text.contains(&tmp_s), "{f:?} contains a temp path");
        assert!(!text.contains("withheld.png"), "{f:?} mentions the withheld image");
        if f.extension().is_some_and(|e| e == "md" || e == "html") {
            assert!(!text.contains(":\\") && !text.contains("file://"), "{f:?} has an absolute path");
        }
    }
}

#[test]
fn markdown_export_end_to_end() {
    let src = TempDir::new("it-src");
    let out = TempDir::new("it-out");
    let rec = fixture(src.path());
    let report = export_markdown(&rec, src.path(), out.path()).unwrap();
    assert_eq!(report, ExportReport { steps: 3, images_written: 3, images_with_redactions: 1, withheld: 1 });

    let md = fs::read_to_string(out.path().join("document.md")).unwrap();
    let mut links = 0;
    for part in md.split("](").skip(1) {
        let target = percent_decode(&part[..part.find(')').unwrap()]);
        assert!(!Path::new(&target).is_absolute());
        assert!(out.path().join(&target).is_file(), "broken link {target}");
        links += 1;
    }
    assert_eq!(links, 3);
    assert!(md.contains("Screenshot withheld"));
    check_images(out.path(), "images/step-001.png", "images/step-002.png");
    check_no_leaks(out.path(), src.path());
}

#[test]
fn html_export_end_to_end() {
    let src = TempDir::new("it-src");
    let out = TempDir::new("it-out");
    let rec = fixture(src.path());
    let report = prepare_html_export(&rec, src.path(), out.path()).unwrap();
    assert_eq!(report, ExportReport { steps: 3, images_written: 3, images_with_redactions: 1, withheld: 1 });

    let html = fs::read_to_string(out.path().join("document.html")).unwrap();
    let mut links = 0;
    for part in html.split("src=\"").skip(1) {
        let target = percent_decode(&part[..part.find('"').unwrap()]);
        assert!(out.path().join(&target).is_file(), "broken src {target}");
        links += 1;
    }
    assert_eq!(links, 3);
    assert!(!html.contains("<script"));
    // Print exports rename images in document order. These flat test images
    // are smaller as PNG, so they stay PNG and pixel-exact.
    assert!(html.contains("images/print-0001.png") && html.contains("images/print-0003.png"), "{html}");
    assert!(!html.contains("step-001.png"));
    check_images(out.path(), "images/print-0001.png", "images/print-0002.png");
    check_no_leaks(out.path(), src.path());
}

#[test]
fn originals_are_not_modified_and_not_copied_byte_for_byte() {
    let src = TempDir::new("it-src");
    let out = TempDir::new("it-out");
    let rec = fixture(src.path());
    let before = fs::read(src.path().join("images/step-002.png")).unwrap();
    export_markdown(&rec, src.path(), out.path()).unwrap();
    assert_eq!(before, fs::read(src.path().join("images/step-002.png")).unwrap());
    assert_ne!(before, fs::read(out.path().join("images/step-002.png")).unwrap());
}

#[test]
fn generated_bundle_exports() {
    // Small stand-in for the later 100-step performance test.
    let src = TempDir::new("gen-src");
    let out = TempDir::new("gen-out");
    let rec = generate_bundle(src.path(), 5, 64, 48);
    let r = export_markdown(&rec, src.path(), out.path()).unwrap();
    assert_eq!((r.steps, r.images_written, r.images_with_redactions), (5, 5, 5));
}

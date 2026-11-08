//! Dev-only headless PDF self-test (`--pdf-selftest <dir>`, feature
//! `selftest`). Never compiled into a release build of the app. It creates no
//! visible window, injects no input and takes no screenshots: it generates a
//! recording, prints it through the real WebView2 path in a hidden window, and
//! parses the resulting PDF bytes.

use crate::pdf::{export_pdf_with, PdfError, PdfJob, PdfPrinter};
use babelsteps_core::model::{Block, Recording};
use babelsteps_core::testing::generate_bundle;
use lopdf::{Document, Object};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::AppHandle;

pub fn requested_dir() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let i = args.iter().position(|a| a == "--pdf-selftest")?;
    args.get(i + 1).map(PathBuf::from)
}

const W: u32 = 5120;
const H: u32 = 1440;
const RED: [u8; 3] = [255, 0, 0];
// Planted red squares: one outside the redaction box, one inside it.
const OUT_RED: (u32, u32, u32, u32) = (100, 100, 200, 100);
const IN_RED: (u32, u32, u32, u32) = (1500, 500, 200, 100);

struct Report {
    lines: Vec<String>,
    failed: usize,
}

impl Report {
    fn say(&mut self, s: impl Into<String>) {
        let s = s.into();
        println!("{s}");
        self.lines.push(s);
    }
    fn check(&mut self, name: &str, ok: bool, detail: impl AsRef<str>) {
        if !ok {
            self.failed += 1;
        }
        self.say(format!("{} {name}: {}", if ok { "PASS" } else { "FAIL" }, detail.as_ref()));
    }
}

/// Times only the print step, so it can be separated from image preparation.
struct Timed<'a> {
    inner: &'a dyn PdfPrinter,
    spent: Mutex<Duration>,
}
impl PdfPrinter for Timed<'_> {
    fn print_to_pdf(&self, job: &PdfJob) -> Result<(), PdfError> {
        let t = Instant::now();
        let r = self.inner.print_to_pdf(job);
        *self.spent.lock().unwrap() = t.elapsed();
        r
    }
}

fn plant(png: &[u8], rects: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    use image::{codecs::png::PngEncoder, ImageEncoder};
    let mut img = image::load_from_memory(png).expect("decode").to_rgba8();
    for &(x, y, w, h) in rects {
        for yy in y..y + h {
            for xx in x..x + w {
                img.put_pixel(xx, yy, image::Rgba([255, 0, 0, 255]));
            }
        }
    }
    let mut out = Vec::new();
    PngEncoder::new(&mut out)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
        .unwrap();
    out
}

struct Rgb {
    w: usize,
    h: usize,
    px: Vec<u8>,
}
impl Rgb {
    /// Pixel at source coordinates, scaled if the PDF image has other dimensions.
    fn at(&self, x: u32, y: u32) -> [u8; 3] {
        let xx = (x as usize * self.w / W as usize).min(self.w - 1);
        let yy = (y as usize * self.h / H as usize).min(self.h - 1);
        let o = (yy * self.w + xx) * 3;
        [self.px[o], self.px[o + 1], self.px[o + 2]]
    }
}
fn near(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tol)
}
/// (outside square is red, centre of inside square is black, inside square is red)
fn classify(img: &Rgb) -> (bool, bool, bool) {
    let o = img.at(OUT_RED.0 + OUT_RED.2 / 2, OUT_RED.1 + OUT_RED.3 / 2);
    let i = img.at(IN_RED.0 + IN_RED.2 / 2, IN_RED.1 + IN_RED.3 / 2);
    (near(o, RED, 12), near(i, [0, 0, 0], 12), near(i, RED, 12))
}

fn dict_i64(d: &lopdf::Dictionary, k: &[u8]) -> Option<i64> {
    d.get(k).ok().and_then(|o| o.as_i64().ok())
}

fn decode_pdf_string(b: &[u8]) -> String {
    if b.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let first = needle[0];
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        match hay[i..hay.len() - needle.len() + 1].iter().position(|&b| b == first) {
            None => return false,
            Some(p) => {
                i += p;
                if &hay[i..i + needle.len()] == needle {
                    return true;
                }
                i += 1;
            }
        }
    }
    false
}

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

/// Strings that must never appear in a PDF: forms of the temp path, the user
/// name, and `file:`. All at least 5 bytes so chance matches in binary data
/// are negligible.
fn needles() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    let tmp = std::env::temp_dir().to_string_lossy().into_owned();
    v.push(("temp dir".into(), tmp.clone()));
    v.push(("temp dir (slashes)".into(), tmp.replace('\\', "/")));
    v.push(("babelsteps-pdf".into(), "babelsteps-pdf".into()));
    v.push(("file:".into(), "file:".into()));
    v.push(("file%3A".into(), "file%3A".into()));
    v.push(("document.html".into(), "document.html".into()));
    v.push(("AppData".into(), "AppData".into()));
    if let Ok(u) = std::env::var("USERNAME") {
        if u.len() >= 4 {
            v.push(("user name".into(), u));
        }
    }
    v
}

struct PdfFacts {
    pages: usize,
    media_boxes_ok: bool,
    media_detail: String,
    title: Option<String>,
    producer: Option<String>,
    raw_hits: Vec<String>,
    stream_hits: Vec<String>,
    streams_scanned: usize,
    streams_failed: usize,
    images: usize,
    jpeg_images: usize,
    text: String,
}

fn media_box(doc: &Document, mut id: lopdf::ObjectId) -> Option<[f32; 4]> {
    for _ in 0..10 {
        let d = doc.get_dictionary(id).ok()?;
        if let Ok(Object::Array(a)) = d.get(b"MediaBox").and_then(|o| doc.dereference(o).map(|x| x.1)) {
            let v: Vec<f32> = a.iter().filter_map(|o| o.as_float().ok()).collect();
            if v.len() == 4 {
                return Some([v[0], v[1], v[2], v[3]]);
            }
        }
        id = d.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn inspect(path: &Path) -> Result<(PdfFacts, Document, Vec<u8>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if !bytes.starts_with(b"%PDF-") {
        return Err("not a PDF".into());
    }
    let doc = Document::load_mem(&bytes).map_err(|e| format!("lopdf: {e}"))?;
    let pages = doc.get_pages();
    let mut ok = true;
    let mut detail = String::new();
    for (n, id) in &pages {
        match media_box(&doc, *id) {
            Some(b) if (b[0].abs() < 0.01 && b[1].abs() < 0.01 && (b[2] - 612.0).abs() < 0.01 && (b[3] - 792.0).abs() < 0.01) => {}
            other => {
                ok = false;
                detail = format!("page {n}: {other:?}");
                break;
            }
        }
    }
    let info = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|o| doc.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok());
    let get = |k: &[u8]| {
        info.and_then(|d| d.get(k).ok())
            .and_then(|o| o.as_str().ok())
            .map(decode_pdf_string)
    };
    let title = get(b"Title");
    let producer = get(b"Producer");

    // Raw bytes.
    let mut raw_hits = Vec::new();
    for (name, n) in needles() {
        if contains(&bytes, n.as_bytes()) {
            raw_hits.push(format!("{name} (ascii)"));
        }
        if contains(&bytes, &utf16be(&n)) {
            raw_hits.push(format!("{name} (utf-16)"));
        }
    }
    // Decompressed non-image streams (content streams, fonts, ToUnicode maps, XMP).
    let (mut stream_hits, mut scanned, mut failed, mut images, mut jpeg) = (Vec::new(), 0, 0, 0, 0);
    for obj in doc.objects.values() {
        if let Object::Stream(s) = obj {
            let is_image = s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) == Some(b"Image");
            if is_image {
                images += 1;
                if s.filters().map(|f| f.contains(&&b"DCTDecode"[..])).unwrap_or(false) {
                    jpeg += 1;
                }
                continue;
            }
            match s.decompressed_content() {
                Ok(data) => {
                    scanned += 1;
                    for (name, n) in needles() {
                        if contains(&data, n.as_bytes()) || contains(&data, &utf16be(&n)) {
                            stream_hits.push(name);
                        }
                    }
                }
                Err(_) => failed += 1,
            }
        }
    }
    // Text through ToUnicode maps.
    let nums: Vec<u32> = pages.keys().copied().collect();
    let text = doc.extract_text(&nums).unwrap_or_default();

    Ok((
        PdfFacts {
            pages: pages.len(),
            media_boxes_ok: ok,
            media_detail: detail,
            title,
            producer,
            raw_hits,
            stream_hits,
            streams_scanned: scanned,
            streams_failed: failed,
            images,
            jpeg_images: jpeg,
            text,
        },
        doc,
        bytes,
    ))
}

#[derive(Default, Debug)]
struct ImageScan {
    decoded: usize,
    /// Images whose outside square is still red (only step 1 should be).
    outside_red: usize,
    /// Images whose inside square (centre) is black.
    inside_black: usize,
    /// Images where the inside square is still red: a redaction leak.
    leaks: usize,
    skipped: usize,
    non_rgb: usize,
}

/// Decode every Flate RGB or JPEG image in the PDF and classify the planted squares.
fn scan_images(doc: &Document) -> ImageScan {
    let mut s_ = ImageScan::default();
    for obj in doc.objects.values() {
        let Object::Stream(s) = obj else { continue };
        if s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok()) != Some(b"Image") {
            continue;
        }
        let (Some(w), Some(h)) = (dict_i64(&s.dict, b"Width"), dict_i64(&s.dict, b"Height")) else {
            s_.skipped += 1;
            continue;
        };
        // Print exports may hold JPEG images (PSR-1), which the PDF keeps as DCT streams.
        let is_jpeg = s.filters().map(|f| f.contains(&&b"DCTDecode"[..])).unwrap_or(false);
        let data = if is_jpeg {
            match image::load_from_memory_with_format(&s.content, image::ImageFormat::Jpeg) {
                Ok(img) => img.to_rgb8().into_raw(),
                Err(_) => {
                    s_.skipped += 1;
                    continue;
                }
            }
        } else {
            let Ok(data) = s.decompressed_content() else {
                s_.skipped += 1;
                continue;
            };
            data
        };
        let (w, h) = (w as usize, h as usize);
        if data.len() != w * h * 3 {
            // Alpha masks and gray images land here.
            s_.non_rgb += 1;
            continue;
        }
        s_.decoded += 1;
        let (out_red, in_black, in_red) = classify(&Rgb { w, h, px: data });
        s_.leaks += in_red as usize;
        s_.outside_red += out_red as usize;
        s_.inside_black += in_black as usize;
    }
    s_
}

pub fn run(app: &AppHandle, dir: &Path) -> i32 {
    let mut rep = Report { lines: Vec::new(), failed: 0 };
    let work = dir.join("work");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("create work dir");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(app, &work, &mut rep)));
    if result.is_err() {
        rep.failed += 1;
        rep.say("FAIL self-test panicked");
    }
    let _ = std::fs::remove_dir_all(&work);
    let _ = std::fs::write(dir.join("selftest-results.txt"), rep.lines.join("\n"));
    rep.say(format!("summary: {} failed check(s)", rep.failed));
    if rep.failed == 0 {
        0
    } else {
        1
    }
}

fn body(app: &AppHandle, work: &Path, rep: &mut Report) {
    let printer = crate::pdf::default_printer(app);
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    rep.say(format!(
        "conditions: {} build, {cpus} logical CPUs, owner's daily PC in normal use (other apps running), hidden WebView2 window",
        if cfg!(debug_assertions) { "DEBUG" } else { "release" }
    ));

    // --- 100-step document, 5120x1440 patterned PNGs, one planted red pair in step 1 ---
    let t = Instant::now();
    let src = work.join("src");
    let rec: Recording = generate_bundle(&src, 100, W, H);
    let step1 = src.join("images/step-001.png");
    let original = std::fs::read(&step1).unwrap();
    let planted = plant(&original, &[OUT_RED, IN_RED]);
    std::fs::write(&step1, &planted).unwrap();
    rep.say(format!("setup: generated 100 steps at {W}x{H} in {:.1}s (not part of the export timing)", t.elapsed().as_secs_f32()));

    // Can the redaction check fail? Run it on the unredacted source image.
    let srcimg = image::load_from_memory(&planted).unwrap().to_rgb8();
    let src_rgb = Rgb { w: srcimg.width() as usize, h: srcimg.height() as usize, px: srcimg.into_raw() };
    let (o, b, r) = classify(&src_rgb);
    rep.check(
        "redaction check can fail (control on the unredacted source)",
        o && !b && r,
        format!("source: outside red={o}, inside black={b}, inside red={r}; expected red/not black/red"),
    );

    let mut text_a_title_count = 0usize;
    for (label, header_footer) in [("A header_footer=false", false), ("B header_footer=true", true)] {
        let out = work.join(if header_footer { "b.pdf" } else { "a.pdf" });
        let timed = Timed { inner: printer.as_ref(), spent: Mutex::new(Duration::ZERO) };
        let t = Instant::now();
        let r = export_pdf_with(&timed, &rec, &src, &out, header_footer, Some(String::new()));
        let total = t.elapsed();
        let report = match r {
            Ok(r) => r,
            Err(e) => {
                rep.check(&format!("{label} export"), false, format!("{e:?}"));
                continue;
            }
        };
        let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        let print = *timed.spent.lock().unwrap();
        rep.say(format!(
            "{label}: total {:.2}s (prepare images+HTML {:.2}s, print {:.2}s), PDF {:.1} MB, report {report:?}",
            total.as_secs_f32(),
            (total - print).as_secs_f32(),
            print.as_secs_f32(),
            size as f64 / 1e6
        ));
        rep.check(&format!("{label} D5 under 10 s"), total.as_secs_f32() < 10.0, format!("{:.2}s", total.as_secs_f32()));
        rep.check(&format!("{label} no .part file left"), !work.join(format!("{}.part", out.file_name().unwrap().to_string_lossy())).exists(), "");

        let (facts, doc, _bytes) = match inspect(&out) {
            Ok(x) => x,
            Err(e) => {
                rep.check(&format!("{label} parse PDF"), false, e);
                continue;
            }
        };
        rep.check(&format!("{label} page count > 1"), facts.pages > 1, format!("{} pages", facts.pages));
        rep.check(
            &format!("{label} MediaBox 612x792 on every page"),
            facts.media_boxes_ok,
            format!("{} pages checked {}", facts.pages, facts.media_detail),
        );
        rep.say(format!("{label}: /Title={:?}, /Producer={:?}", facts.title, facts.producer));
        rep.check(
            &format!("{label} /Title equals the recording title"),
            facts.title.as_deref() == Some(rec.title.as_str()),
            format!("expected {:?}", rec.title),
        );
        rep.check(&format!("{label} raw bytes free of path/user/file:"), facts.raw_hits.is_empty(), format!("{:?}", facts.raw_hits));
        rep.check(
            &format!("{label} decompressed streams free of path/user/file:"),
            facts.stream_hits.is_empty(),
            format!("{} non-image streams scanned, {} could not be decompressed, hits {:?}", facts.streams_scanned, facts.streams_failed, facts.stream_hits),
        );
        rep.say(format!(
            "{label}: not inspected: pixel data of {} image streams for path strings ({} JPEG); fonts only as decompressed bytes",
            facts.images, facts.jpeg_images
        ));
        // Text: positive controls first, so an empty extraction cannot pass.
        let first_title = rec.blocks.iter().find_map(|b| if let Block::Step(s) = b { Some(s.title.clone()) } else { None }).unwrap_or_default();
        rep.check(
            &format!("{label} text extraction works (step title found)"),
            facts.text.contains(&first_title),
            format!("{} chars extracted", facts.text.len()),
        );
        let title_count = facts.text.matches(rec.title.as_str()).count();
        if !header_footer {
            text_a_title_count = title_count;
        }
        let t_hits: Vec<String> = needles().into_iter().filter(|(_, n)| facts.text.contains(n.as_str())).map(|(k, _)| k).collect();
        rep.check(&format!("{label} extracted text free of path/user/file:"), t_hits.is_empty(), format!("{t_hits:?}"));
        if header_footer {
            rep.check(
                &format!("{label} header title printed on every page"),
                title_count == text_a_title_count + facts.pages,
                format!("title occurrences: {title_count} with header, {text_a_title_count} without, {} pages", facts.pages),
            );
        }

        // Redaction through real image extraction.
        let t = Instant::now();
        let sc = scan_images(&doc);
        rep.say(format!("{label}: image scan {sc:?} ({:.1}s)", t.elapsed().as_secs_f32()));
        rep.check(
            &format!("{label} redaction in extracted PDF pixels"),
            sc.decoded >= 100 && sc.leaks == 0 && sc.inside_black == sc.decoded && sc.outside_red == 1,
            "all 100 images decoded, the redaction box is black in every one, no red inside it, and exactly one image (step 1) still shows the red square outside it",
        );
    }
    negative_control(printer.as_ref(), work, rep);
    smaller_images_timing(printer.as_ref(), work, rep);
}

/// Informational only: the same 100 steps with 1920x540 images, to estimate
/// what downscaling screenshots before printing would buy. Not a pass/fail check.
fn smaller_images_timing(printer: &dyn PdfPrinter, work: &Path, rep: &mut Report) {
    let src = work.join("src_1920");
    let rec = generate_bundle(&src, 100, 1920, 540);
    let timed = Timed { inner: printer, spent: Mutex::new(Duration::ZERO) };
    let out = work.join("small.pdf");
    let t = Instant::now();
    let r = export_pdf_with(&timed, &rec, &src, &out, false, Some(String::new()));
    let total = t.elapsed();
    let print = *timed.spent.lock().unwrap();
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    rep.say(format!(
        "info (not a check): 100 steps at 1920x540: ok={}, total {:.2}s (prepare {:.2}s, print {:.2}s), PDF {:.1} MB",
        r.is_ok(),
        total.as_secs_f32(),
        (total - print).as_secs_f32(),
        print.as_secs_f32(),
        size as f64 / 1e6
    ));
}

/// Small document: the same footer settings with `FooterUri` unset. If the
/// path check is real, the unset case should expose the file path in the text.
fn negative_control(printer: &dyn PdfPrinter, work: &Path, rep: &mut Report) {
    let src = work.join("src_small");
    let rec = generate_bundle(&src, 2, 600, 300);
    let hits = |footer: Option<String>, name: &str| -> Result<(Vec<String>, String), String> {
        let out = work.join(name);
        export_pdf_with(printer, &rec, &src, &out, true, footer).map_err(|e| format!("{e:?}"))?;
        let (f, _, _) = inspect(&out)?;
        let mut h: Vec<String> = needles().into_iter().filter(|(_, n)| f.text.contains(n.as_str())).map(|(k, _)| k).collect();
        h.extend(f.raw_hits.clone());
        h.extend(f.stream_hits.clone());
        let snippet: String = f.text.lines().filter(|l| l.contains("file:") || l.contains("babelsteps")).take(1).collect();
        Ok((h, snippet))
    };
    match hits(None, "unset.pdf") {
        Ok((h, snip)) => {
            rep.check(
                "negative control: FooterUri unset exposes a path somewhere (so the check can fail)",
                !h.is_empty(),
                format!("hits {h:?}; matching text line length {}", snip.len()),
            );
        }
        Err(e) => rep.check("negative control export", false, e),
    }
    match hits(Some(String::new()), "empty.pdf") {
        Ok((h, _)) => rep.check("FooterUri = empty string: no path/user/file: anywhere", h.is_empty(), format!("{h:?}")),
        Err(e) => rep.check("empty FooterUri export", false, e),
    }
}

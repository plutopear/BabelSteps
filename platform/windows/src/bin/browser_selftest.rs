//! Browser password-field self-test for the real recorder (`babelsteps-windows`).
//!
//! Opens local test pages in a Chromium browser (Edge or Chrome) with fresh,
//! temporary profiles, records clicks and typing in their password and user name
//! fields, and checks the real output files:
//!
//! - the typed password is in no step and no file;
//! - the password field is solid black in every screenshot;
//! - controls prove those checks could fail (user name text is recorded, and
//!   the user name field shows its own colour where we expect it).
//!
//! Two pages, each in a fresh browser:
//! - "plain": the password manager is off. The first action is the password
//!   click, before anything else has asked the browser for its accessibility
//!   tree: the worst case, since Edge's first answer was wrong in M0.
//! - "login": a real sign-in form with the password manager on. It also clicks
//!   Edge's "show password" eye button and types more, then submits, which can
//!   bring up the browser's save-password prompt.
//!
//! Safety, because this runs on a PC someone may be using: the browser window is
//! one we launched, with a throwaway profile; a click is sent only if that window
//! is under the point; keys only while it has focus; the recorder ignores real
//! input (`only_injected`). Full-screen images are deleted after checking. Only a
//! crop of each test page is kept.
//!
//! Usage: `browser-selftest <browser.exe> [output-folder]`. Exit code 0 only if
//! every check passed.

use anyhow::{anyhow, Context, Result};
use babelsteps_core::storage::Bundle;
use babelsteps_core::{Action, Block, Recording, StepBlock};
use babelsteps_windows::testing::{self, Frame};
use babelsteps_windows::testwin::{click_if_root, root_window_at, type_if_foreground};
use babelsteps_windows::{Recorder, RecorderConfig};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode};
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, EnumWindows, GetClassNameW, GetWindowRect, GetWindowTextW, IsWindowVisible, PostMessageW, WM_CLOSE,
};

const SECRET_1: &str = "quartz9vault";
const SECRET_2: &str = "moon4pass";
const SECRET_3: &str = "river8lock";
/// Typed after the "show password" button has made the password readable.
const SECRET_4: &str = "tide2gale";
const VISIBLE: &str = "alice";

/// Test page layout in CSS pixels: (left, top, width, height).
const USER: (i32, i32, i32, i32) = (40, 40, 360, 48);
const BUTTON: (i32, i32, i32, i32) = (440, 40, 160, 48);
const PASS: (i32, i32, i32, i32) = (40, 120, 360, 48);
/// Edge's "show password" button sits at the right end inside the field.
const REVEAL: (i32, i32, i32, i32) = (40 + 360 - 30, 120 + 8, 24, 32);
/// Empty page background.
const BACKGROUND: (i32, i32, i32, i32) = (500, 260, 40, 40);
/// The user name field's background, used to prove the rectangles line up.
const CYAN: [u8; 3] = [0, 255, 255];

/// What one page does: which form, the passwords typed, and what to expect.
struct Scenario {
    name: &'static str,
    login: bool,
    secrets: &'static [&'static str],
    clicks: usize,
    password_runs: usize,
}

const SCENARIOS: [Scenario; 2] = [
    Scenario { name: "plain", login: false, secrets: &[SECRET_1, SECRET_2], clicks: 5, password_runs: 2 },
    Scenario { name: "login", login: true, secrets: &[SECRET_3, SECRET_4], clicks: 5, password_runs: 1 },
];

fn page(title: &str, login: bool) -> String {
    let rect = |(l, t, w, h): (i32, i32, i32, i32)| {
        format!("position:absolute;left:{l}px;top:{t}px;width:{w}px;height:{h}px;box-sizing:border-box;margin:0;font-size:22px")
    };
    let (u, b, p) = (rect(USER), rect(BUTTON), rect(PASS));
    let fields = if login {
        // A sign-in form as sites write it. Submitting hides the form, which
        // browsers take as a successful sign-in.
        format!(
            r#"<form id="f" onsubmit="event.preventDefault();setTimeout(function(){{document.getElementById('f').style.display='none'}},150)">
<input id="user" name="username" type="text" autocomplete="username" aria-label="User name" style="{u};background:rgb(0,255,255);border:2px solid #333">
<button id="go" type="submit" style="{b}">Sign in</button>
<input id="pass" name="password" type="password" autocomplete="current-password" aria-label="Password" style="{p};background:rgb(255,255,0);border:2px solid #333">
</form>"#
        )
    } else {
        format!(
            r#"<input id="user" type="text" autocomplete="off" aria-label="User name" style="{u};background:rgb(0,255,255);border:2px solid #333">
<button id="go" type="button" style="{b}">Continue</button>
<input id="pass" type="password" autocomplete="off" aria-label="Password" style="{p};background:rgb(255,255,0);border:2px solid #333">"#
        )
    };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{title}</title></head>
<body style="margin:0;background:#e8e8e8">
{fields}
</body></html>"#
    )
}

struct Checks {
    lines: Vec<String>,
    passed: usize,
    failed: usize,
}

impl Checks {
    fn check(&mut self, name: &str, pass: bool, detail: impl AsRef<str>) {
        let tag = if pass { "PASS" } else { "FAIL" };
        let line = format!("{tag} {name}: {}", detail.as_ref());
        println!("{line}");
        self.lines.push(line);
        if pass {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
    }
    fn info(&mut self, text: impl AsRef<str>) {
        let line = format!("info {}", text.as_ref());
        println!("{line}");
        self.lines.push(line);
    }
}

fn ms(d: u64) {
    sleep(Duration::from_millis(d));
}

fn wide_to_string(buf: &[u16], n: i32) -> String {
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    wide_to_string(&buf, n)
}

fn title_of(h: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    wide_to_string(&buf, n)
}

struct Search {
    title: String,
    found: isize,
}

unsafe extern "system" fn find_top(h: HWND, lp: LPARAM) -> windows::core::BOOL {
    let s = unsafe { &mut *(lp.0 as *mut Search) };
    // Titles are only compared with our own page's title; nothing is kept.
    if unsafe { IsWindowVisible(h) }.as_bool() && class_of(h) == "Chrome_WidgetWin_1" && title_of(h).starts_with(&s.title) {
        s.found = h.0 as isize;
        return false.into();
    }
    true.into()
}

unsafe extern "system" fn find_render(h: HWND, lp: LPARAM) -> windows::core::BOOL {
    let found = unsafe { &mut *(lp.0 as *mut isize) };
    if class_of(h) == "Chrome_RenderWidgetHostHWND" && unsafe { IsWindowVisible(h) }.as_bool() {
        *found = h.0 as isize;
        return false.into();
    }
    true.into()
}

/// The browser window we launched, and where our page is drawn on screen.
struct Browser {
    child: Child,
    top: isize,
    /// Screen position of the page's top-left corner, and device pixels per CSS pixel.
    origin: (i32, i32),
    scale: f64,
    /// The page area on screen, (x, y, w, h).
    content: (i32, i32, i32, i32),
}

impl Browser {
    fn launch(exe: &Path, profile: &Path, page: &Path, title: &str) -> Result<Self> {
        let url = format!("file:///{}", page.display().to_string().replace('\\', "/"));
        let child = Command::new(exe)
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check", "--disable-sync", "--window-position=120,120", "--window-size=720,420"])
            .arg(format!("--app={url}"))
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;
        let mut b = Self { child, top: 0, origin: (0, 0), scale: 1.0, content: (0, 0, 0, 0) };
        let mut search = Search { title: title.to_string(), found: 0 };
        let deadline = Instant::now() + Duration::from_secs(20);
        while search.found == 0 && Instant::now() < deadline {
            ms(250);
            unsafe {
                let _ = EnumWindows(Some(find_top), LPARAM(&mut search as *mut Search as isize));
            }
        }
        if search.found == 0 {
            b.close();
            return Err(anyhow!("the browser window with our test page did not appear within 20 s"));
        }
        b.top = search.found;
        let top = HWND(search.found as *mut _);
        ms(1500); // Let the page finish drawing.
        let mut render: isize = 0;
        unsafe {
            let _ = EnumChildWindows(Some(top), Some(find_render), LPARAM(&mut render as *mut isize as isize));
        }
        if render == 0 {
            b.close();
            return Err(anyhow!("could not find the page area inside the browser window"));
        }
        let mut r = RECT::default();
        if let Err(e) = unsafe { GetWindowRect(HWND(render as *mut _), &mut r) } {
            b.close();
            return Err(e.into());
        }
        b.origin = (r.left, r.top);
        b.content = (r.left, r.top, r.right - r.left, r.bottom - r.top);
        b.scale = unsafe { GetDpiForWindow(top) } as f64 / 96.0;
        Ok(b)
    }

    /// A page rectangle in screen pixels, (x, y, w, h).
    fn rect(&self, (l, t, w, h): (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
        let s = |v: i32| (v as f64 * self.scale).round() as i32;
        (self.origin.0 + s(l), self.origin.1 + s(t), s(w), s(h))
    }

    fn center(&self, r: (i32, i32, i32, i32)) -> (i32, i32) {
        let (x, y, w, h) = self.rect(r);
        (x + w / 2, y + h / 2)
    }

    fn click(&self, r: (i32, i32, i32, i32)) -> Result<()> {
        let (x, y) = self.center(r);
        click_if_root(self.top, x, y)
    }

    fn type_keys(&self, text: &str) -> Result<()> {
        type_if_foreground(self.top, text)
    }

    fn unobscured(&self) -> bool {
        let (x, y, w, h) = self.content;
        (1..=3).all(|i| (1..=3).all(|j| root_window_at(x + w * i / 4, y + h * j / 4) == self.top))
    }

    fn close(&mut self) {
        if self.top != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(self.top as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            ms(200);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn steps(rec: &Recording) -> Vec<&StepBlock> {
    rec.blocks.iter().filter_map(|b| if let Block::Step(s) = b { Some(s) } else { None }).collect()
}

fn step_text(rec: &Recording) -> String {
    let mut s = String::new();
    for st in steps(rec) {
        s.push_str(&st.title);
        s.push('\n');
        s.push_str(&st.description);
        s.push('\n');
        if let Action::Text { text } = &st.action {
            s.push_str(text);
            s.push('\n');
        }
    }
    s
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// Files in the bundle that contain `needle` as UTF-8 or UTF-16.
fn files_containing(dir: &Path, needle: &str) -> Vec<String> {
    let utf8 = needle.as_bytes().to_vec();
    let utf16: Vec<u8> = needle.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    files_under(dir)
        .into_iter()
        .filter(|p| {
            std::fs::read(p).is_ok_and(|b| b.windows(utf8.len()).any(|w| w == utf8) || b.windows(utf16.len()).any(|w| w == utf16))
        })
        .map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
        .collect()
}

/// Image files on disk that no step refers to, and steps whose image is missing.
fn orphans(bundle: &Path, rec: &Recording) -> (Vec<String>, Vec<String>) {
    let referenced: Vec<String> = steps(rec).iter().filter_map(|s| s.screenshot.clone()).collect();
    let on_disk: Vec<String> = files_under(&bundle.join("images"))
        .iter()
        .filter_map(|p| p.file_name().map(|n| format!("images/{}", n.to_string_lossy())))
        .collect();
    let unreferenced = on_disk.iter().filter(|f| !referenced.contains(f)).cloned().collect();
    let missing = referenced.iter().filter(|f| !on_disk.contains(f)).cloned().collect();
    (unreferenced, missing)
}

const MARKER: [u8; 3] = [255, 0, 0];

/// Pixels of the rectangle, shrunk by `inset` to skip borders and rounding.
fn pixels(f: &Frame, (x, y, w, h): (i32, i32, i32, i32), inset: i32) -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for py in y + inset..y + h - inset {
        for px in x + inset..x + w - inset {
            if let Some(c) = f.sample(px, py) {
                out.push(c);
            }
        }
    }
    out
}

/// Every pixel is black, or the click marker drawn on top of the black box.
fn all_black(f: &Frame, r: (i32, i32, i32, i32)) -> bool {
    let p = pixels(f, r, 3);
    !p.is_empty() && p.iter().all(|c| c.iter().all(|&v| v < 10) || *c == MARKER)
}

/// Share of pixels within a small tolerance of `colour`.
fn share_near(f: &Frame, r: (i32, i32, i32, i32), colour: [u8; 3]) -> f64 {
    let p = pixels(f, r, 4);
    if p.is_empty() {
        return 0.0;
    }
    p.iter().filter(|c| (0..3).all(|i| c[i].abs_diff(colour[i]) <= 12)).count() as f64 / p.len() as f64
}

fn crop(f: &Frame, (x, y, w, h): (i32, i32, i32, i32)) -> Frame {
    let x0 = (x - f.origin_x).clamp(0, f.width as i32) as usize;
    let y0 = (y - f.origin_y).clamp(0, f.height as i32) as usize;
    let x1 = (x - f.origin_x + w).clamp(0, f.width as i32) as usize;
    let y1 = (y - f.origin_y + h).clamp(0, f.height as i32) as usize;
    let mut bgra = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for row in y0..y1 {
        let start = (row * f.width as usize + x0) * 4;
        bgra.extend_from_slice(&f.bgra[start..start + (x1 - x0) * 4]);
    }
    Frame { width: (x1 - x0) as u32, height: (y1 - y0) as u32, origin_x: x, origin_y: y, bgra }
}

fn act(sc: &Scenario, b: &Browser) -> Result<()> {
    if sc.login {
        b.click(USER)?;
        ms(500);
        b.type_keys(VISIBLE)?;
        ms(400);
        b.click(PASS)?;
        ms(500);
        b.type_keys(SECRET_3)?;
        ms(500);
        // Edge's "show password" button: the field now shows the text.
        b.click(REVEAL)?;
        ms(500);
        b.type_keys(SECRET_4)?;
        ms(400);
        b.click(BUTTON)?;
        // The form hides; the browser may offer to save the password.
        ms(2000);
        b.click(BACKGROUND)?;
        ms(800);
    } else {
        // Password first: nothing has asked the browser for its accessibility tree yet.
        b.click(PASS)?;
        ms(500);
        b.type_keys(SECRET_1)?;
        ms(400);
        b.click(USER)?;
        ms(500);
        b.type_keys(VISIBLE)?;
        ms(400);
        b.click(BUTTON)?;
        ms(600);
        b.click(PASS)?;
        ms(500);
        b.type_keys(SECRET_2)?;
        ms(400);
        b.click(BUTTON)?;
        ms(800);
    }
    Ok(())
}

fn record(c: &mut Checks, sc: &Scenario, b: &Browser, work: &Path, crop_out: &Path) -> Result<()> {
    let n = sc.name;
    let bundle = Bundle::create(work, n, n, 0)?;
    let rec = bundle.load()?;
    let config = RecorderConfig { keystrokes_enabled: true, only_injected: true, ignore_own_windows: false, ..RecorderConfig::default() };
    let r = Recorder::start(bundle, rec, config)?;
    ms(800);
    let acted = act(sc, b);
    let (bundle, report) = r.stop()?;
    let bundle = bundle.path().to_path_buf();
    acted?;

    let all = steps(&report.recording);
    c.info(format!(
        "{n}: {} steps; withheld {}, not verified {}, no screenshot {}; slowest analysis {:.0} ms",
        all.len(),
        report.status.withheld,
        report.status.not_verified,
        report.status.no_screenshot,
        report.status.analysis_ms_max
    ));
    c.info(format!("{n}: titles: {:?}", all.iter().map(|s| s.title.as_str()).collect::<Vec<_>>()));
    let text = step_text(&report.recording);
    for secret in sc.secrets {
        let leaks = files_containing(&bundle, secret);
        c.check(&format!("{n}: typed password \"{secret}\" is in no step text"), !text.contains(secret), "titles, descriptions, typed text");
        c.check(&format!("{n}: typed password \"{secret}\" is in no file on disk"), leaks.is_empty(), format!("files containing it: {leaks:?}"));
    }
    c.check(
        &format!("{n} control: ordinary typed text is recorded"),
        text.contains(VISIBLE) && !files_containing(&bundle, VISIBLE).is_empty(),
        "proves the password checks above could fail",
    );
    let runs = all.iter().filter(|s| s.title.starts_with("Enter password")).count();
    c.check(&format!("{n}: password typing is titled \"Enter password\""), runs >= sc.password_runs, format!("{runs} runs, expected {}", sc.password_runs));
    let clicks = all.iter().filter(|s| matches!(s.action, Action::Click { .. })).count();
    c.check(&format!("{n}: all {} clicks were recorded", sc.clicks), clicks >= sc.clicks, format!("{clicks} clicks"));

    let pass = b.rect(PASS);
    let user = b.rect(USER);
    let (cx, cy) = b.center(USER);
    let m = testing::monitors().into_iter().find(|m| m.contains(cx, cy)).ok_or_else(|| anyhow!("no monitor under the test page"))?;
    // In the login page the form hides after "Sign in": later images have no field.
    let with_field = if sc.login { all.iter().position(|s| s.title.contains("\"Sign in\"")).map_or(all.len(), |i| i + 1) } else { all.len() };
    let mut images = 0;
    let mut not_black = Vec::new();
    let mut user_blacked = 0;
    let mut best_cyan: f64 = 0.0;
    let mut last = None;
    for s in &all[..with_field] {
        let Some(rel) = &s.screenshot else { continue };
        let f = Frame::load_png(&bundle.join(rel), m.x, m.y).with_context(|| format!("reading {rel}"))?;
        images += 1;
        if !all_black(&f, pass) {
            not_black.push(format!("{rel} ({})", s.title));
        }
        if all_black(&f, user) {
            user_blacked += 1;
        }
        best_cyan = best_cyan.max(share_near(&f, user, CYAN));
        last = Some(f);
    }
    c.check(&format!("{n}: at least 4 step images to check"), images >= 4, format!("{images} images with the field on the page"));
    c.check(&format!("{n}: the password field is solid black in every image"), not_black.is_empty(), format!("not black in {not_black:?}"));
    c.check(
        &format!("{n} control: the user name field is not blacked out"),
        user_blacked == 0 && images > 0,
        format!("blacked in {user_blacked} of {images}"),
    );
    c.check(
        &format!("{n} control: the field rectangles line up with the images"),
        best_cyan >= 0.6,
        format!("best {:.0}% of the user name field is its cyan background (scale {:.2})", best_cyan * 100.0, b.scale),
    );
    let (unref, missing) = orphans(&bundle, &report.recording);
    c.check(
        &format!("{n}: no image without a step, no step without its image"),
        unref.is_empty() && missing.is_empty(),
        format!("unreferenced {unref:?}, missing {missing:?}"),
    );
    if let Some(f) = last.filter(|_| b.unobscured()) {
        std::fs::write(crop_out, crop(&f, b.content).encode_png()?)?;
        c.info(format!("{n}: crop of the test page from the last image with the field: {}", crop_out.display()));
    }
    Ok(())
}

fn run(exe: &Path, out: &Path) -> Result<Checks> {
    let mut c = Checks { lines: Vec::new(), passed: 0, failed: 0 };
    std::fs::create_dir_all(out)?;
    let tag = std::process::id();
    let work = out.join(format!("bundles-{tag}"));
    std::fs::create_dir_all(&work)?;
    let scratch = std::env::temp_dir().join(format!("babelsteps-browser-test-{tag}"));

    testing::make_thread_dpi_aware();
    c.check("setup: per-monitor DPI aware", testing::per_monitor_dpi_aware(), "needed for black boxes to line up");
    c.info(format!("browser: {}", exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let result = (|| -> Result<()> {
        for sc in &SCENARIOS {
            // A fresh profile per page, so nothing carries over between them.
            let profile = scratch.join(format!("profile-{}", sc.name));
            std::fs::create_dir_all(&profile)?;
            let title = format!("BabelSteps browser test {tag} {}", sc.name);
            let page_path = scratch.join(format!("{}.html", sc.name));
            std::fs::write(&page_path, page(&title, sc.login))?;
            let crop_out = out.join(format!("browser-{}-crop.png", sc.name));
            let mut b = Browser::launch(exe, &profile, &page_path, &title)?;
            let r = record(&mut c, sc, &b, &work, &crop_out);
            b.close();
            r?;
        }
        Ok(())
    })();
    // Full-screen images may show private things: delete them all.
    let _ = std::fs::remove_dir_all(&work);
    c.check("cleanup: test recordings deleted", !work.exists(), "");
    for _ in 0..20 {
        if std::fs::remove_dir_all(&scratch).is_ok() || !scratch.exists() {
            break;
        }
        ms(500); // Browser helper processes may still hold files.
    }
    c.check("cleanup: temporary browser profiles deleted", !scratch.exists(), scratch.display().to_string());
    if let Err(e) = result {
        c.check("run: the test finished", false, format!("{e:#}"));
    }
    Ok(c)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(exe) = args.next().map(PathBuf::from) else {
        println!("usage: browser-selftest <browser.exe> [output-folder]");
        return ExitCode::FAILURE;
    };
    let out = args.next().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/browser-selftest"));
    match run(&exe, &out) {
        Ok(c) => {
            println!("SUMMARY {} passed, {} failed", c.passed, c.failed);
            let _ = std::fs::write(out.join("browser-summary.txt"), c.lines.join("\n"));
            if c.failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            println!("FAIL run: {e:#}");
            ExitCode::FAILURE
        }
    }
}

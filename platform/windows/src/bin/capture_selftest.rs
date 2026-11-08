//! Capture self-test for the real recorder (`babelsteps-windows`).
//!
//! Runs on a PC someone may be using, so: all input is synthetic and lands only on
//! our own test window (`testwin`), the recorder ignores real input
//! (`only_injected`), and full-screen images are deleted after checking. Only a
//! crop of the test window is kept, as `test-window-crop.png`.
//!
//! Every check that can pass vacuously has a control that proves it could fail.
//!
//! Usage: `capture-selftest [output-folder]`. Exit code 0 only if every check passed.

use anyhow::{anyhow, Context, Result};
use babelsteps_core::storage::Bundle;
use babelsteps_core::{Action, Block, Recording, StepBlock};
use babelsteps_windows::testing::{self, Frame, Monitor, SessionEvent};
use babelsteps_windows::testwin::{self, TestWindow, GREEN};
use babelsteps_windows::{Recorder, RecorderConfig, RecorderReport, Scope};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::thread::sleep;
use std::time::{Duration, Instant};

const SECRET: &str = "hunter2";
const VISIBLE: &str = "alice";
const FAIL_CLOSED_TEXT: &str = "bob";
const BEFORE_TRIALS: usize = 20;
const DELAYED_TRIALS: usize = 10;

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

fn config(keys: bool) -> RecorderConfig {
    RecorderConfig { keystrokes_enabled: keys, only_injected: true, ignore_own_windows: false, ..RecorderConfig::default() }
}

fn start(dir: &Path, name: &str, keys: bool) -> Result<Recorder> {
    let bundle = Bundle::create(dir, name, name, 0)?;
    let rec = bundle.load()?;
    let r = Recorder::start(bundle, rec, config(keys))?;
    ms(700); // Let capture deliver its first frames.
    Ok(r)
}

fn start_scoped(dir: &Path, name: &str, scope: Scope) -> Result<Recorder> {
    let bundle = Bundle::create(dir, name, name, 0)?;
    let rec = bundle.load()?;
    let r = Recorder::start(bundle, rec, RecorderConfig { scope, ..config(false) })?;
    ms(700);
    Ok(r)
}

fn stop(r: Recorder) -> Result<(PathBuf, RecorderReport)> {
    let (bundle, report) = r.stop()?;
    Ok((bundle.path().to_path_buf(), report))
}

fn steps(rec: &Recording) -> Vec<&StepBlock> {
    rec.blocks.iter().filter_map(|b| if let Block::Step(s) = b { Some(s) } else { None }).collect()
}

fn clicks(rec: &Recording) -> Vec<&StepBlock> {
    steps(rec).into_iter().filter(|s| matches!(s.action, Action::Click { .. })).collect()
}

fn monitor_of(x: i32, y: i32) -> Result<Monitor> {
    testing::monitors().into_iter().find(|m| m.contains(x, y)).ok_or_else(|| anyhow!("no monitor at ({x}, {y})"))
}

fn load(bundle: &Path, rel: &str, m: &Monitor) -> Result<Frame> {
    Frame::load(&bundle.join(rel), m.x, m.y).with_context(|| format!("reading {rel}"))
}

fn near(a: [u8; 3], b: [u8; 3], tol: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= tol)
}

/// The click marker's exact colour. It is drawn after redaction, on top of the
/// black box, and differs from the test window's red background.
const MARKER: [u8; 3] = [255, 0, 0];

/// The outline colour the recorder draws round the clicked element
/// (`Frame::outline`). Like the marker it is drawn after the black-out, so where
/// it crosses a hidden field it replaces black with green; it never shows what was
/// under the black.
const OUTLINE: [u8; 3] = [0, 200, 0];

/// Every pixel of the rectangle (shrunk by one pixel, to skip edges) is black,
/// or the click marker or the element outline drawn over the black.
fn all_black(f: &Frame, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    (y + 1..y + h - 1)
        .all(|py| (x + 1..x + w - 1).all(|px| f.sample(px, py).is_some_and(|c| c.iter().all(|&v| v < 10) || c == MARKER || c == OUTLINE)))
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

/// Files in the bundle (database, journal, images) that contain `needle` as
/// UTF-8 or UTF-16.
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

/// All text a step carries: titles, descriptions, typed text.
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

fn before_state(c: &mut Checks, win: &TestWindow, dir: &Path, delayed: bool) -> Result<()> {
    let (n, label) = if delayed { (DELAYED_TRIALS, "slow analysis") } else { (BEFORE_TRIALS, "normal") };
    let r = start(dir, if delayed { "before-delayed" } else { "before" }, false)?;
    if delayed {
        // Each event's analysis takes 400 ms longer than clicks arrive apart.
        r.faults().analysis_delay_ms.store(400, Ordering::Relaxed);
    }
    let (bx, by) = win.background_point()?;
    let mut reacted = 0;
    for _ in 0..n {
        win.set_red(false);
        ms(300);
        win.safe_click(bx, by)?;
        ms(60);
        reacted += usize::from(win.is_red());
    }
    win.set_red(false);
    let (bundle, report) = stop(r)?;
    let (px, py) = win.probe_point()?;
    let m = monitor_of(px, py)?;
    let cl = clicks(&report.recording);
    let mut green = 0;
    let mut red_or_other = Vec::new();
    for s in &cl {
        match s.screenshot.as_deref().map(|rel| load(&bundle, rel, &m)) {
            Some(Ok(f)) => match f.sample(px, py) {
                Some(p) if near(p, GREEN, 30) => green += 1,
                other => red_or_other.push(format!("{other:?}")),
            },
            _ => red_or_other.push("no image".into()),
        }
    }
    c.check(&format!("before-state control ({label}): the window reacted to every click"), reacted == n, format!("{reacted}/{n} turned red"));
    c.check(&format!("before-state ({label}): one click step per click"), cl.len() == n, format!("{} steps for {n} clicks", cl.len()));
    c.check(
        &format!("before-state ({label}): every screenshot shows the window before the click"),
        green == n,
        format!("{green}/{n} green; others: {red_or_other:?}"),
    );
    c.check(
        &format!("before-state ({label}): no step marked as possibly after the action"),
        report.status.not_verified == 0,
        format!("{} marked", report.status.not_verified),
    );
    c.info(format!(
        "{label}: {} frames received, capture rebuilds {}, GDI fallbacks {}, over budget {}, steps without image {}, elapsed {:.1}s",
        report.status.capture.wgc_frames,
        report.status.capture.rebuilds,
        report.status.capture.gdi_fallbacks,
        report.status.frames_over_budget,
        report.status.no_screenshot,
        report.elapsed.as_secs_f64()
    ));
    c.info(format!(
        "{label}: analysis per event mean {:.0} ms, max {:.0} ms; last scan searched {} windows, skipped {} hidden",
        report.status.analysis_ms_mean, report.status.analysis_ms_max, report.status.last_scan_windows, report.status.last_scan_hidden
    ));
    Ok(())
}

fn password(c: &mut Checks, win: &TestWindow, dir: &Path, crop_out: &Path) -> Result<usize> {
    let r = start(dir, "password", true)?;
    win.allow_focus(true);
    let typed = (|| -> Result<()> {
        let (x, y) = win.pass_center()?;
        win.safe_click(x, y)?;
        ms(400);
        win.safe_type_keys(SECRET)?;
        ms(300);
        let (x, y) = win.user_center()?;
        win.safe_click(x, y)?;
        ms(400);
        win.safe_type_keys(VISIBLE)?;
        ms(300);
        let (x, y) = win.save_center()?;
        win.safe_click(x, y)?;
        ms(500);
        Ok(())
    })();
    win.allow_focus(false);
    let (bundle, report) = stop(r)?;
    typed?;
    let text = step_text(&report.recording);
    let leaks = files_containing(&bundle, SECRET);
    c.check("password: typed password is in no step text", !text.contains(SECRET), "searched titles, descriptions, typed text");
    c.check("password: typed password is in no file on disk", leaks.is_empty(), format!("files containing it: {leaks:?}"));
    c.check(
        "password control: ordinary typed text is recorded",
        text.contains(VISIBLE) && !files_containing(&bundle, VISIBLE).is_empty(),
        "proves the two checks above could fail",
    );
    c.check(
        "password: the password run is titled \"Enter password\"",
        steps(&report.recording).iter().any(|s| s.title.starts_with("Enter password")),
        format!("titles: {:?}", steps(&report.recording).iter().map(|s| s.title.clone()).collect::<Vec<_>>()),
    );

    let (px, py) = win.probe_point()?;
    let m = monitor_of(px, py)?;
    let (pass, user) = (win.pass_rect()?, win.user_rect()?);
    let mut images = 0;
    let mut not_black = Vec::new();
    let mut user_blacked = 0;
    let mut last: Option<Frame> = None;
    for s in steps(&report.recording) {
        let Some(rel) = &s.screenshot else { continue };
        let f = load(&bundle, rel, &m)?;
        images += 1;
        if !all_black(&f, pass) {
            not_black.push(rel.clone());
        }
        if all_black(&f, user) {
            user_blacked += 1;
        }
        last = Some(f);
    }
    c.check("password: at least 3 step images to check", images >= 3, format!("{images} images, {} withheld", report.status.withheld));
    c.check("password: the password field is solid black in every image", not_black.is_empty(), format!("not black in {not_black:?}"));
    c.check(
        "password control: the user name field is not blacked out",
        user_blacked == 0 && images > 0,
        format!("blacked in {user_blacked} of {images}; proves the black check is not trivially true"),
    );
    if let Some(f) = last.filter(|_| win.unobscured()) {
        let (wx, wy, ww, wh) = win.client_rect()?;
        let crop = crop(&f, wx, wy, ww, wh);
        std::fs::write(crop_out, crop.encode(babelsteps_core::screenshot::ShotFormat::Png, babelsteps_core::screenshot::Effort::Normal)?)?;
        c.info(format!("crop of the test window from the last image: {}", crop_out.display()));
    }
    let (unref, missing) = orphans(&bundle, &report.recording);
    c.check("password: no image without a step, no step without its image", unref.is_empty() && missing.is_empty(), format!("unreferenced {unref:?}, missing {missing:?}"));
    Ok(images)
}

fn crop(f: &Frame, x: i32, y: i32, w: i32, h: i32) -> Frame {
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

/// The click minimizes the password window, so a scan after the click cannot
/// see the field that the before-click frame shows.
fn window_changes_after_click(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "minimize", false)?;
    let (bx, by) = win.background_point()?;
    win.set_red(false);
    ms(300);
    win.minimize_on_next_click();
    win.safe_click(bx, by)?;
    ms(400);
    let minimized = win.is_minimized();
    ms(800);
    win.restore();
    win.set_red(false);
    ms(600);
    let (bundle, report) = stop(r)?;
    let (px, py) = win.probe_point()?;
    let m = monitor_of(px, py)?;
    let pass = win.pass_rect()?;
    let cl = clicks(&report.recording);
    c.check("window change control: the click minimized the window", minimized, "");
    let outcome = match cl.first() {
        None => "no step".to_string(),
        Some(s) if s.screenshot_withheld => "withheld".to_string(),
        Some(s) => match &s.screenshot {
            Some(rel) => {
                let f = load(&bundle, rel, &m)?;
                if all_black(&f, pass) {
                    let (x, y, w, h) = pass;
                    let outline = (y + 1..y + h - 1)
                        .flat_map(|py| (x + 1..x + w - 1).map(move |px| (px, py)))
                        .filter(|&(px, py)| f.sample(px, py) == Some(OUTLINE))
                        .count();
                    c.info(format!("window change: {outline} pixels over the hidden field are the recorder's own element outline"));
                    "image with the field black".to_string()
                } else {
                    // Diagnostics: is it real content, or noise from how the file stores black?
                    let (x, y, w, h) = pass;
                    let mut brightest = 0u8;
                    let mut over = 0usize;
                    let mut total = 0usize;
                    for py in y + 1..y + h - 1 {
                        for px in x + 1..x + w - 1 {
                            if let Some(c) = f.sample(px, py) {
                                total += 1;
                                let v = c.into_iter().max().unwrap_or(0);
                                brightest = brightest.max(v);
                                over += usize::from(v >= 10 && c != MARKER && c != OUTLINE);
                            }
                        }
                    }
                    format!(
                        "IMAGE WITH THE FIELD VISIBLE (file {rel}; {over} of {total} field pixels are not black, brightest channel {brightest})"
                    )
                }
            }
            None => "no image".to_string(),
        },
    };
    c.check(
        "window change: a window minimized by the click never leaves its password field visible",
        outcome == "withheld" || outcome == "image with the field black",
        outcome,
    );
    Ok(())
}

fn fail_closed(c: &mut Checks, win: &TestWindow, dir: &Path, earlier_images: usize) -> Result<()> {
    let r = start(dir, "fail-closed", true)?;
    r.faults().uia.fail.store(true, Ordering::Relaxed);
    win.allow_focus(true);
    let typed = (|| -> Result<()> {
        let (x, y) = win.user_center()?;
        win.safe_click(x, y)?;
        ms(400);
        win.safe_type_keys(FAIL_CLOSED_TEXT)?;
        ms(300);
        let (x, y) = win.save_center()?;
        win.safe_click(x, y)?;
        ms(400);
        Ok(())
    })();
    win.allow_focus(false);
    let (bundle, report) = stop(r)?;
    typed?;
    let all = steps(&report.recording);
    let withheld = all.iter().filter(|s| s.screenshot_withheld && s.screenshot.is_none()).count();
    let images = files_under(&bundle.join("images")).len();
    c.check("fail-closed: steps are still recorded", all.len() >= 2, format!("{} steps", all.len()));
    c.check("fail-closed: every step's screenshot is withheld", withheld == all.len(), format!("{withheld}/{} withheld", all.len()));
    c.check(
        "fail-closed: no image file was written",
        images == 0 && earlier_images > 0,
        format!("{images} files; the password phase wrote {earlier_images}, so writing works"),
    );
    c.check(
        "fail-closed: typed text was dropped",
        !step_text(&report.recording).contains(FAIL_CLOSED_TEXT) && files_containing(&bundle, FAIL_CLOSED_TEXT).is_empty(),
        "not in step text or any file",
    );
    Ok(())
}

fn timeout_and_recovery(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "timeout", false)?;
    let (bx, by) = win.background_point()?;
    r.faults().uia.delay_ms.store(2500, Ordering::Relaxed);
    win.safe_click(bx, by)?;
    ms(4500); // The lookup times out at 1.5 s; the scan worker also waits.
    r.faults().uia.delay_ms.store(0, Ordering::Relaxed);
    ms(3000); // Let the abandoned workers finish.
    win.set_red(false);
    ms(300);
    win.safe_click(bx, by)?;
    ms(500);
    let (_bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    c.check("timeout: a hung UI Automation call withholds that step", cl.first().is_some_and(|s| s.screenshot_withheld), format!("{} click steps", cl.len()));
    c.check(
        "timeout: recording recovers afterwards",
        cl.get(1).is_some_and(|s| s.screenshot.is_some() && !s.screenshot_withheld),
        format!("second step has image: {}", cl.get(1).is_some_and(|s| s.screenshot.is_some())),
    );
    c.check("timeout: the timeout was counted", report.status.uia.timeouts >= 1, format!("{:?}", report.status.uia));
    Ok(())
}

fn scan_failure(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "scan-fail", false)?;
    r.faults().scan_fail.store(true, Ordering::Relaxed);
    let (bx, by) = win.background_point()?;
    win.safe_click(bx, by)?;
    ms(500);
    r.faults().scan_fail.store(false, Ordering::Relaxed);
    win.set_red(false);
    ms(300);
    win.safe_click(bx, by)?;
    ms(500);
    let (_bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    c.check("scan failure: the window scan alone failing withholds the screenshot", cl.first().is_some_and(|s| s.screenshot_withheld), format!("{} click steps", cl.len()));
    c.check("scan failure control: the next click, with the scan working, has its image", cl.get(1).is_some_and(|s| s.screenshot.is_some()), "");
    Ok(())
}

fn gdi_marked(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "gdi", false)?;
    r.faults().force_gdi.store(true, Ordering::Relaxed);
    let (bx, by) = win.background_point()?;
    win.set_red(false);
    ms(300);
    win.safe_click(bx, by)?;
    ms(500);
    let (_bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    c.check(
        "fallback capture: a screenshot that cannot be proven to predate the click is marked",
        cl.first().is_some_and(|s| s.screenshot.is_some() && s.description.contains("after the action")),
        format!("description: {:?}", cl.first().map(|s| s.description.clone())),
    );
    Ok(())
}

/// A6: recording one window or one monitor. Clicks outside are not recorded,
/// and a window's screenshot is cut to the window.
fn scopes(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let (bx, by) = win.background_point()?;
    let m = monitor_of(bx, by)?;
    let device = {
        let n = m.device.iter().position(|&ch| ch == 0).unwrap_or(m.device.len());
        String::from_utf16_lossy(&m.device[..n])
    };
    let ring = |f: &Frame| [(18, 0), (-18, 0), (0, 18), (0, -18)].iter().filter(|(dx, dy)| f.sample(bx + dx, by + dy) == Some(MARKER)).count();

    // One window: the test window itself. A click in the password field,
    // then one on the background.
    let r = start_scoped(dir, "scope-window", Scope::Window(win.hwnd_id()))?;
    win.allow_focus(true);
    let clicked = (|| -> Result<()> {
        let (x, y) = win.pass_center()?;
        win.safe_click(x, y)?;
        ms(400);
        win.safe_click(bx, by)?;
        ms(500);
        Ok(())
    })();
    win.allow_focus(false);
    let (bundle, report) = stop(r)?;
    clicked?;
    let cl = clicks(&report.recording);
    let expect = testing::window_bounds(win.hwnd_id())
        .and_then(|b| b.intersect(&babelsteps_core::Rect { x: m.x, y: m.y, w: m.w, h: m.h }));
    let mut shots = Vec::new();
    if let Some(e) = expect {
        for s in &cl {
            if let Some(rel) = s.screenshot.as_deref() {
                shots.push(load(&bundle, rel, &Monitor { x: e.x, y: e.y, ..m })?);
            }
        }
    }
    // Checked against the client area from testwin, not the product's own bounds call:
    // the cut holds all of it and adds at most a title bar and borders.
    let (cx, cy, cw, ch) = win.client_rect()?;
    let cut = expect.is_some_and(|e| {
        (e.w < m.w || e.h < m.h)
            && shots.iter().all(|f| {
                f.width as i32 == e.w
                    && f.height as i32 == e.h
                    && f.overlaps(&babelsteps_core::Rect { x: cx, y: cy, w: 1, h: 1 })
                    && f.overlaps(&babelsteps_core::Rect { x: cx + cw - 1, y: cy + ch - 1, w: 1, h: 1 })
                    && f.width as i32 <= cw + 40
                    && f.height as i32 <= ch + 80
            })
    });
    c.check(
        "one window: clicks in the window are recorded, each screenshot cut to the window",
        cl.len() == 2 && shots.len() == 2 && cut,
        format!(
            "clicks {}, images {:?}, window {:?}, monitor {}x{}",
            cl.len(),
            shots.iter().map(|f| (f.width, f.height)).collect::<Vec<_>>(),
            expect.map(|e| (e.w, e.h)),
            m.w,
            m.h
        ),
    );
    // Redaction and drawing use screen coordinates, so a cut frame must not shift them.
    let (pass, user) = (win.pass_rect()?, win.user_rect()?);
    let black = shots.iter().filter(|f| all_black(f, pass)).count();
    c.check("one window: the password field is solid black in every cut screenshot", !shots.is_empty() && black == shots.len(), format!("{black} of {}", shots.len()));
    let user_black = shots.iter().filter(|f| all_black(f, user)).count();
    c.check("one window control: the user name field is not blacked out", !shots.is_empty() && user_black == 0, format!("blacked in {user_black}"));
    let points = shots.last().map_or(0, ring);
    c.check("one window: the click marker lands on the click in the cut screenshot", points == 4, format!("{points}/4 ring points red"));

    // Control: scoped to another window, the same click is not recorded.
    let other = unsafe { windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow() }.0 as isize;
    let r = start_scoped(dir, "scope-other", Scope::Window(other))?;
    win.safe_click(bx, by)?;
    ms(500);
    let (_bundle, report) = stop(r)?;
    let n = steps(&report.recording).len();
    c.check("one window: a click on another window is not recorded", n == 0, format!("steps {n}"));

    // One monitor: the test window's monitor, whole-monitor screenshot.
    let r = start_scoped(dir, "scope-monitor", Scope::Monitor(device.clone()))?;
    win.safe_click(bx, by)?;
    ms(500);
    let (bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    let dims = match cl.first().and_then(|s| s.screenshot.as_deref()) {
        Some(rel) => Some(load(&bundle, rel, &m).map(|f| (f.width, f.height))?),
        None => None,
    };
    c.check(
        "one monitor: a click on the chosen monitor is recorded with the whole monitor",
        cl.len() == 1 && dims == Some((m.w as u32, m.h as u32)),
        format!("clicks {}, image {dims:?}, monitor {}x{}", cl.len(), m.w, m.h),
    );

    // Control: scoped to another monitor, the same click is not recorded.
    match testing::monitors().into_iter().find(|o| o.device != m.device) {
        Some(o) => {
            let n = o.device.iter().position(|&ch| ch == 0).unwrap_or(o.device.len());
            let r = start_scoped(dir, "scope-other-monitor", Scope::Monitor(String::from_utf16_lossy(&o.device[..n])))?;
            win.safe_click(bx, by)?;
            ms(500);
            let (_bundle, report) = stop(r)?;
            let n = steps(&report.recording).len();
            c.check("one monitor: a click on another monitor is not recorded", n == 0, format!("steps {n}"));
        }
        None => c.info("one monitor: the click-on-another-monitor check was skipped (one monitor)"),
    }

    // A monitor that is not connected: recording refuses to start.
    let refused = start_scoped(dir, "scope-gone", Scope::Monitor(r"\\.\MISSING-DISPLAY".into()));
    let ok = refused.is_err();
    if let Ok(r) = refused {
        let _ = stop(r);
    }
    c.check("one monitor: a monitor that is not connected is refused", ok, "");
    Ok(())
}

fn pause_and_comment(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "pause", false)?;
    let (bx, by) = win.background_point()?;
    win.safe_click(bx, by)?;
    ms(300);
    r.pause();
    ms(300);
    win.safe_click(bx, by)?;
    ms(200);
    win.safe_click(bx, by)?;
    ms(300);
    r.resume();
    ms(300);
    r.comment("Self-test comment");
    ms(200);
    win.safe_click(bx, by)?;
    ms(400);
    let (bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    c.check("pause: clicks while paused are not recorded", cl.len() == 2, format!("{} click steps for 2 unpaused and 2 paused clicks", cl.len()));
    c.check(
        "comment: a comment becomes a step",
        steps(&report.recording).iter().any(|s| matches!(s.action, Action::Comment) && s.title == "Self-test comment"),
        "",
    );
    let (unref, missing) = orphans(&bundle, &report.recording);
    c.check("pause: no image from a paused click is on disk", unref.is_empty() && missing.is_empty(), format!("unreferenced {unref:?}, missing {missing:?}"));
    Ok(())
}

/// PSR-10. The real lock, sleep and wake messages need the owner (they take over
/// the screen), so this sends the same messages through the recorder's real hidden
/// window. What it proves: the window hears them, nothing is recorded while away,
/// and coming back rebuilds capture. What it cannot prove: that Windows sends them
/// (the live lock test does), or what a real lock screen does to capture.
fn lock_and_sleep(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let r = start(dir, "lock", false)?;
    let (bx, by) = win.background_point()?;
    win.set_red(false);
    win.safe_click(bx, by)?; // 1: recorded.
    ms(300);
    let heard = r.simulate_session(SessionEvent::Locked);
    ms(400);
    let away_now = r.status().away;
    win.safe_click(bx, by)?; // Locked: not recorded.
    ms(200);
    r.simulate_session(SessionEvent::Suspend); // Sleep on top of the lock.
    ms(200);
    r.simulate_session(SessionEvent::Resume); // Wakes onto the lock screen: still away.
    ms(400);
    let still_away = r.status().away;
    win.safe_click(bx, by)?; // Still locked: not recorded.
    ms(200);
    let rebuilds_before = r.status().capture.rebuilds;
    r.simulate_session(SessionEvent::Unlocked);
    ms(1200); // Capture restarts and delivers a fresh frame.
    let back = !r.status().away;
    win.set_red(false);
    ms(300);
    win.safe_click(bx, by)?; // 2: recorded, with a fresh picture.
    ms(400);
    // A plain sleep and wake, with no lock.
    r.simulate_session(SessionEvent::Suspend);
    ms(300);
    win.safe_click(bx, by)?; // Asleep: not recorded.
    ms(200);
    r.simulate_session(SessionEvent::Resume);
    ms(1200);
    win.set_red(false); // The click made while asleep turned the window red.
    ms(300);
    win.safe_click(bx, by)?; // 3: recorded.
    ms(400);
    let (bundle, report) = stop(r)?;
    let cl = clicks(&report.recording);
    c.check("lock: the recorder's window hears the session messages", heard && away_now, format!("heard {heard}, away after lock {away_now}"));
    c.check("lock: waking onto a lock screen is still away", still_away, "");
    c.check("lock: back after the unlock", back, "");
    c.check(
        "lock and sleep: only clicks made while present are recorded",
        cl.len() == 3,
        format!("{} click steps for 3 present and 3 away clicks", cl.len()),
    );
    c.check(
        "lock: capture was rebuilt on the way back",
        report.status.capture.rebuilds > rebuilds_before,
        format!("rebuilds {rebuilds_before} -> {}", report.status.capture.rebuilds),
    );
    c.check("lock: two lock and sleep periods counted", report.status.away_periods == 2, format!("{}", report.status.away_periods));
    let (px, py) = win.probe_point()?;
    let m = monitor_of(px, py)?;
    let mut fresh = 0;
    for s in cl.iter().skip(1) {
        if let Some(Ok(f)) = s.screenshot.as_deref().map(|rel| load(&bundle, rel, &m)) {
            if f.sample(px, py).is_some_and(|p| near(p, GREEN, 30)) {
                fresh += 1;
            }
        }
    }
    c.check("lock: the clicks after coming back have a current picture, not marked as unverified", fresh == 2 && report.status.not_verified == 0, format!("{fresh}/2 current, {} not verified", report.status.not_verified));
    let (unref, missing) = orphans(&bundle, &report.recording);
    c.check("lock: no image from an away click is on disk", unref.is_empty() && missing.is_empty(), format!("unreferenced {unref:?}, missing {missing:?}"));
    Ok(())
}

fn hook_speed(c: &mut Checks, dir: &Path) -> Result<()> {
    let r = start(dir, "hooks", false)?;
    let before = testing::hook_stats();
    let t = Instant::now();
    testwin::f24_burst(1000);
    ms(1000);
    let (_bundle, report) = stop(r)?;
    let after = report.hooks;
    c.check("hooks: no input event was dropped", after.dropped == 0, format!("{} dropped", after.dropped));
    c.check(
        "hooks: the burst reached the hooks",
        after.callbacks >= before.callbacks + 2000,
        format!("{} callbacks during the burst", after.callbacks - before.callbacks),
    );
    c.check("hooks: F24 with keystrokes off makes no step", steps(&report.recording).is_empty(), format!("{} steps", steps(&report.recording).len()));
    c.info(format!(
        "hook callback mean {:.1} us, max {:.1} us over {} callbacks; burst + drain {:.1}s; hooks reinstalled {} times",
        after.mean_us,
        after.max_us,
        after.callbacks,
        t.elapsed().as_secs_f64(),
        after.rehooks
    ));
    Ok(())
}

/// C2: the recording frame must never appear in a screenshot. The control runs
/// the same frame without the capture exclusion and must show red at the screen
/// edge, otherwise this check could not fail.
fn frame_cue(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    use babelsteps_windows::indicator::Indicator;
    const FRAME: [u8; 3] = [220, 30, 30];
    let (bx, by) = win.background_point()?;
    let measure = |excluded: bool, gdi: bool, name: &str| -> Result<usize> {
        let cue = if excluded { Indicator::start()? } else { Indicator::start_unexcluded()? };
        ms(600);
        let r = start(dir, name, false)?;
        if gdi {
            r.faults().force_gdi.store(true, Ordering::Relaxed);
        }
        ms(300);
        for _ in 0..3 {
            win.safe_click(bx, by)?;
            ms(250);
        }
        let (bundle, report) = stop(r)?;
        drop(cue);
        let m = monitor_of(bx, by)?;
        let mut hits = 0;
        for s in clicks(&report.recording) {
            let Some(rel) = s.screenshot.as_deref() else { continue };
            let f = load(&bundle, rel, &m)?;
            for i in 0..3 {
                for x in m.x..m.x + m.w {
                    for y in [m.y + i, m.y + m.h - 1 - i] {
                        hits += usize::from(f.sample(x, y).is_some_and(|p| near(p, FRAME, 12)));
                    }
                }
                for y in m.y..m.y + m.h {
                    for x in [m.x + i, m.x + m.w - 1 - i] {
                        hits += usize::from(f.sample(x, y).is_some_and(|p| near(p, FRAME, 12)));
                    }
                }
            }
        }
        Ok(hits)
    };
    let control = measure(false, false, "cue-control")?;
    let real = measure(true, false, "cue-excluded")?;
    let control_gdi = measure(false, true, "cue-control-gdi")?;
    let real_gdi = measure(true, true, "cue-excluded-gdi")?;
    c.check("frame cue control: the frame without the exclusion IS visible in screenshots", control > 100, format!("{control} frame-coloured edge pixels"));
    c.check("frame cue: with the exclusion, no frame pixels in any screenshot", real == 0, format!("{real} frame-coloured edge pixels"));
    c.check("frame cue control (fallback GDI capture): the frame without the exclusion IS visible", control_gdi > 100, format!("{control_gdi} frame-coloured edge pixels"));
    c.check("frame cue (fallback GDI capture): with the exclusion, no frame pixels in any screenshot", real_gdi == 0, format!("{real_gdi} frame-coloured edge pixels"));
    Ok(())
}

/// C2: with the frame on, clicks on the outermost pixels of the screen still
/// reach the window under it and are recorded for that window. The control
/// uses a frame that is NOT click-through and must be seen to take the pixels.
fn edge_clicks(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    use babelsteps_windows::indicator::Indicator;
    let (bx, by) = win.background_point()?;
    let m = monitor_of(bx, by)?;
    let area = (m.x, m.y, m.w, m.h);
    let result = (|| -> Result<()> {
        for (bottom_right, name) in [(true, "bottom-right"), (false, "top-left")] {
            win.place_client_flush(area, bottom_right)?;
            let pts = win.edge_points(bottom_right)?;
            let all_on_screen = pts.iter().all(|&(x, y)| m.contains(x, y));
            let hits = |win: &TestWindow| pts.iter().filter(|&&(x, y)| babelsteps_windows::testwin::root_window_at(x, y) == win.hwnd_id()).count();
            let blocked = {
                let _cue = Indicator::start_opaque_to_clicks()?;
                ms(500);
                hits(win)
            };
            c.check(
                &format!("edge clicks control ({name}): a frame that is not click-through takes the edge pixels from the window"),
                all_on_screen && blocked == 0,
                format!("{blocked} of {} points still reach the window; all on screen: {all_on_screen}", pts.len()),
            );
            let cue = Indicator::start()?;
            ms(500);
            let reach = hits(win);
            c.check(
                &format!("edge clicks ({name}): with the frame on, every edge pixel (1 to 4 px) still belongs to the window"),
                reach == pts.len(),
                format!("{reach} of {} points", pts.len()),
            );
            // Click every point and report what was recorded. `label` is the run's folder name.
            let click_all = |label: &str| -> Result<(usize, Vec<String>, usize, usize, String)> {
                let r = start(dir, label, false)?;
                ms(300);
                let mut reacted = 0;
                let mut errors = Vec::new();
                for &(x, y) in &pts {
                    win.set_red(false);
                    ms(120);
                    match win.safe_click(x, y) {
                        Ok(()) => {
                            ms(80);
                            reacted += usize::from(win.is_red());
                        }
                        Err(e) => errors.push(format!("{e:#}")),
                    }
                }
                win.set_red(false);
                let (_bundle, report) = stop(r)?;
                let cl = clicks(&report.recording);
                // The test window is a few hundred pixels wide; the frame covers a whole
                // monitor. A step whose target is smaller than the monitor named the window.
                let small = |s: &&&StepBlock| s.target.as_ref().and_then(|t| t.bounds).is_some_and(|b| b.w < m.w / 2 && b.h < m.h / 2);
                let named = cl.iter().filter(small).count();
                let seen = format!(
                    "errors {errors:?}; target sizes {:?}",
                    cl.iter().map(|s| s.target.as_ref().and_then(|t| t.bounds).map(|b| (b.w, b.h))).collect::<Vec<_>>()
                );
                Ok((reacted, errors, cl.len(), named, seen))
            };
            let with_frame = click_all(&format!("edge-{name}"));
            drop(cue);
            let (reacted, errors, steps_n, named, seen) = with_frame?;
            // Baseline: the same clicks with no frame. What the recorder names there is the yardstick.
            win.place_client_flush(area, bottom_right)?;
            let (_, _, base_steps, base_named, base_seen) = click_all(&format!("edge-{name}-noframe"))?;
            c.check(
                &format!("edge clicks ({name}): every click reached the test window"),
                reacted == pts.len() && errors.is_empty(),
                format!("{reacted}/{} turned it red; refused: {errors:?}", pts.len()),
            );
            c.check(
                &format!("edge clicks ({name}): every click was recorded, and named the test window (the no-frame baseline is only a yardstick: the Windows taskbar can cover the bottom screen row, so it may click fewer points)"),
                steps_n == pts.len() && named == pts.len() && base_named == base_steps,
                format!("with frame: {steps_n} steps, {named} naming the test window ({seen}); without: {base_steps} steps, {base_named} naming it ({base_seen})"),
            );
        }
        Ok(())
    })();
    win.reset_position();
    result
}

/// C3 and A8 through the recording path: remapped shortcuts fire and are left
/// out of the steps, and keep-newest-2 on the finished recording removes the
/// right files from disk. Shortcuts are pressed only while the test window has
/// focus. The control run does not ignore the shortcuts and must record them.
fn remapped_hotkeys(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    use babelsteps_core::settings::HotkeySettings;
    use babelsteps_core::storage::Bundle;
    use babelsteps_windows::hotkeys::{self, Hotkey};
    let hk = HotkeySettings {
        note: "Ctrl+Alt+Shift+F11".into(),
        stop: "Ctrl+Alt+Shift+F12".into(),
        ..HotkeySettings::default()
    };
    let specs = hk.parse().map_err(|e| anyhow!(e))?;
    let keys: Vec<Hotkey> = specs.iter().map(Hotkey::from_spec).collect();
    let (bx, by) = win.background_point()?;

    let run = |ignore: bool, name: &str| -> Result<(Vec<bool>, Vec<usize>, PathBuf, RecorderReport)> {
        let (handle, pressed, ok) = hotkeys::register(&keys)?;
        let result = (|| -> Result<(PathBuf, RecorderReport)> {
            let mut cfg = config(true);
            if ignore {
                cfg.ignore_keys = keys.clone();
            }
            let bundle_dir = dir.join(name);
            std::fs::create_dir_all(&bundle_dir)?;
            let bundle = babelsteps_core::storage::Bundle::create(&bundle_dir, name, name, 1)?;
            let rec = Recording::new(name, name, 1);
            let r = Recorder::start(bundle, rec, cfg)?;
            win.allow_focus(true);
            let driven = (|| -> Result<()> {
                for i in 0..5 {
                    win.set_red(false);
                    ms(250);
                    win.safe_click(bx, by)?;
                    ms(250);
                    if i == 0 {
                        for _ in 0..30 {
                            if win.has_focus() {
                                break;
                            }
                            ms(100);
                        }
                    }
                }
                // The remapped "Add a note" and "Stop now" shortcuts.
                win.safe_press(true, true, true, 0x7A)?;
                ms(400);
                win.safe_press(true, true, true, 0x7B)?;
                ms(400);
                Ok(())
            })();
            win.allow_focus(false);
            let out = stop(r);
            driven?;
            out
        })();
        let presses: Vec<usize> = pressed.try_iter().collect();
        drop(handle);
        let (path, report) = result?;
        Ok((ok, presses, path, report))
    };
    let shortcut_steps = |r: &RecorderReport| {
        steps(&r.recording).iter().filter(|s| matches!(s.action, babelsteps_core::model::Action::Shortcut { .. })).count()
    };

    let (ok, presses, path, report) = run(true, "hk-ignored")?;
    c.check("remapped hotkeys: all four shortcuts registered", ok.iter().all(|b| *b), format!("{ok:?} (false = another app holds it)"));
    c.check(
        "remapped hotkeys: the remapped note and stop shortcuts fire",
        presses == vec![2, 3],
        format!("pressed indexes {presses:?}, expected [2, 3]"),
    );
    c.check(
        "remapped hotkeys: with them ignored, no step records either shortcut",
        shortcut_steps(&report) == 0 && clicks(&report.recording).len() == 5,
        format!("{} shortcut steps, {} click steps", shortcut_steps(&report), clicks(&report.recording).len()),
    );

    // Retention on the finished recording, from the saved files.
    let bundle = Bundle::open(&path)?;
    let mut rec = bundle.load()?;
    let before = std::fs::read_dir(bundle.images_dir())?.count();
    let click_shots: Vec<String> = clicks(&rec).iter().filter_map(|s| s.screenshot.clone()).collect();
    let out = bundle.apply_retention(&mut rec, 2)?;
    let after_files: Vec<String> =
        std::fs::read_dir(bundle.images_dir())?.flatten().map(|e| format!("images/{}", e.file_name().to_string_lossy())).collect();
    let with_shot: Vec<String> = steps(&rec).iter().filter_map(|s| s.screenshot.clone()).collect();
    let reloaded = bundle.load()?;
    c.check(
        "retention control: five screenshots on disk before",
        before == 5 && click_shots.len() == 5,
        format!("{before} files, {} click screenshots", click_shots.len()),
    );
    c.check(
        "retention: keep-newest-2 leaves exactly the last two screenshots, the other three files are gone",
        out.removed == 3
            && out.failed == 0
            && click_shots.len() == 5
            && with_shot == click_shots[3..]
            && after_files.len() == 2
            && click_shots[3..].iter().all(|p| after_files.contains(p))
            && click_shots[..3].iter().all(|p| !bundle.path().join(p).exists()),
        format!("removed {}, failed {}, kept {with_shot:?}, files on disk {after_files:?}", out.removed, out.failed),
    );
    c.check("retention: the saved document matches and keeps all five steps", reloaded == rec && reloaded.step_count() == 5, "");

    // Control: the same keys, not ignored, must show up as steps.
    let (_, _, _, control) = run(false, "hk-control")?;
    c.check(
        "remapped hotkeys control: without the ignore list the same shortcuts ARE recorded as steps",
        shortcut_steps(&control) > 0,
        format!("{} shortcut steps", shortcut_steps(&control)),
    );
    Ok(())
}

const TYPED_OFF: &str = "carol";

/// A11, A3 and A2 on our own window, keystrokes off: typing becomes one
/// "Typed in" step with no text; click titles name the control and app but never
/// the window title, right clicks included; the outline and click marker are
/// drawn where they belong.
fn titles_typing_outline(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let window_title = win.title();
    let r = start(dir, "titles", false)?;
    win.allow_focus(true);
    let acted = (|| -> Result<()> {
        let (x, y) = win.user_center()?;
        win.safe_click(x, y)?;
        ms(400);
        win.safe_type_keys(TYPED_OFF)?;
        ms(300);
        let (x, y) = win.save_center()?;
        win.safe_click(x, y)?;
        ms(500);
        let (bx, by) = win.background_point()?;
        win.safe_click(bx, by)?;
        ms(400);
        win.safe_right_click(bx, by)?;
        ms(500);
        Ok(())
    })();
    win.allow_focus(false);
    win.set_red(false);
    let field_text = win.user_text().unwrap_or_default();
    let (bundle, report) = stop(r)?;
    acted?;
    let all = steps(&report.recording);
    let titles: Vec<String> = all.iter().map(|s| s.title.clone()).collect();

    // A11: keystrokes off.
    let typed: Vec<&&StepBlock> = all.iter().filter(|s| s.title.starts_with("Typed in") || s.title == "Typed text").collect();
    c.check("keystrokes off control: the typed text reached the field", field_text.contains(TYPED_OFF), "read back from the control");
    c.check("keystrokes off: typing makes exactly one typing step with no text", typed.len() == 1, format!("titles: {titles:?}"));
    c.check(
        "keystrokes off: the typed text is in no step and no file",
        !step_text(&report.recording).contains(TYPED_OFF) && files_containing(&bundle, TYPED_OFF).is_empty(),
        format!("files containing it: {:?}", files_containing(&bundle, TYPED_OFF)),
    );

    // A3: titles.
    let save = all.iter().find(|s| matches!(s.action, Action::Click { .. }) && s.title.contains("\"Save\""));
    c.check(
        "titles: the Save click names the button and the app",
        save.is_some_and(|s| s.title.starts_with("Left-click \"Save\" button in ") && s.title.contains("capture")),
        format!("titles: {titles:?}"),
    );
    let right: Vec<&&StepBlock> = all.iter().filter(|s| s.title.starts_with("Right-click")).collect();
    c.check("titles: the right click is one \"Right-click\" step", right.len() == 1, format!("titles: {titles:?}"));
    c.check("titles control: the window title was read back", window_title.len() > 5, "proves the next check could fail");
    c.check(
        "titles: the window title is in no step and no file",
        !window_title.is_empty() && !step_text(&report.recording).contains(&window_title) && files_containing(&bundle, &window_title).is_empty(),
        format!("titles: {titles:?}; files containing it: {:?}", files_containing(&bundle, &window_title)),
    );

    // A2: outline and click marker on the Save click's image.
    let (sx, sy, sw, sh) = win.save_rect()?;
    let (cx, cy) = win.save_center()?;
    let m = monitor_of(cx, cy)?;
    let ring = |f: &Frame, x: i32, y: i32| [(18, 0), (-18, 0), (0, 18), (0, -18)].iter().filter(|(dx, dy)| f.sample(x + dx, y + dy) == Some(MARKER)).count();
    // Points on the outline band (1 to 3 px outside the button) on all four
    // sides, and points 8 px outside, where nothing should be drawn. Top and
    // bottom points sit near the left end, clear of the marker ring (radius
    // 16 to 20 px around the centre), which is drawn over the outline.
    let band = [(sx + 8, sy - 2), (sx + 8, sy + sh + 1), (sx - 2, sy + sh / 2), (sx + sw + 1, sy + sh / 2)];
    let outside = [(sx + 8, sy - 8), (sx + 8, sy + sh + 8), (sx - 8, sy + sh / 2), (sx + sw + 8, sy + sh / 2)];
    match save.and_then(|s| s.screenshot.as_deref()).map(|rel| load(&bundle, rel, &m)) {
        Some(Ok(f)) => {
            let on = band.iter().filter(|&&(x, y)| f.sample(x, y).is_some_and(|p| near(p, OUTLINE, 10))).count();
            let off = outside.iter().filter(|&&(x, y)| f.sample(x, y).is_some_and(|p| near(p, OUTLINE, 10))).count();
            c.check("outline: green on all four sides of the clicked button", on == 4, format!("{on}/4 band points green"));
            c.check("outline control: no outline 8 px further out", off == 0, format!("{off}/4 outside points green"));
            c.check("marker: a red ring around the click point", ring(&f, cx, cy) == 4, format!("{}/4 ring points red", ring(&f, cx, cy)));
        }
        other => c.check("outline: the Save click has an image", false, format!("{:?}", other.map(|r| r.map(|_| ()).map_err(|e| e.to_string())))),
    }
    // Marker control: the first click (on the user field) has no ring at the Save button.
    let first = all.iter().find(|s| matches!(s.action, Action::Click { .. }));
    match first.and_then(|s| s.screenshot.as_deref()).map(|rel| load(&bundle, rel, &m)) {
        Some(Ok(f)) => c.check("marker control: no ring at the Save button in another click's image", ring(&f, cx, cy) == 0, ""),
        _ => c.check("marker control: the first click has an image", false, ""),
    }
    let (unref, missing) = orphans(&bundle, &report.recording);
    c.check("titles: no image without a step, no step without its image", unref.is_empty() && missing.is_empty(), format!("unreferenced {unref:?}, missing {missing:?}"));
    Ok(())
}

/// A3 opt-in on: a click on our own window names the window title, and the
/// title is saved with the step. The default-off half is checked in
/// `titles_typing_outline`; this half proves the setting reaches the pipeline.
fn window_title_opt_in(c: &mut Checks, win: &TestWindow, dir: &Path) -> Result<()> {
    let window_title = win.title();
    let bundle = Bundle::create(dir, "wintitle", "wintitle", 0)?;
    let rec = bundle.load()?;
    let r = Recorder::start(bundle, rec, RecorderConfig { show_window_titles: true, ..config(false) })?;
    ms(700);
    let acted = (|| -> Result<()> {
        let (x, y) = win.save_center()?;
        win.safe_click(x, y)?;
        ms(500);
        Ok(())
    })();
    win.set_red(false);
    let (bundle, report) = stop(r)?;
    acted?;
    let titles: Vec<String> = steps(&report.recording).iter().map(|s| s.title.clone()).collect();
    let want = format!("Left-click \"Save\" button in \"{window_title}\"");
    c.check(
        "window titles on: the Save click names the window",
        !window_title.is_empty() && titles.iter().any(|t| t.starts_with(&want)),
        format!("titles: {titles:?}"),
    );
    c.check(
        "window titles on: the title is saved in the recording",
        !files_containing(&bundle, &window_title).is_empty(),
        "control for the default-off check: the same search finds it when it is there",
    );
    Ok(())
}

fn run(out: &Path) -> Result<Checks> {
    let mut c = Checks { lines: Vec::new(), passed: 0, failed: 0 };
    std::fs::create_dir_all(out)?;
    let work = out.join(format!("bundles-{}", std::process::id()));
    std::fs::create_dir_all(&work)?;
    let crop_out = out.join("test-window-crop.png");

    testing::make_thread_dpi_aware();
    let win = TestWindow::spawn()?;
    ms(800);
    c.check("setup: per-monitor DPI aware", testing::per_monitor_dpi_aware(), "needed for black boxes to line up");
    let result = (|| -> Result<()> {
        before_state(&mut c, &win, &work, false)?;
        before_state(&mut c, &win, &work, true)?;
        let images = password(&mut c, &win, &work, &crop_out)?;
        titles_typing_outline(&mut c, &win, &work)?;
        window_title_opt_in(&mut c, &win, &work)?;
        window_changes_after_click(&mut c, &win, &work)?;
        fail_closed(&mut c, &win, &work, images)?;
        timeout_and_recovery(&mut c, &win, &work)?;
        scan_failure(&mut c, &win, &work)?;
        gdi_marked(&mut c, &win, &work)?;
        scopes(&mut c, &win, &work)?;
        pause_and_comment(&mut c, &win, &work)?;
        lock_and_sleep(&mut c, &win, &work)?;
        frame_cue(&mut c, &win, &work)?;
        edge_clicks(&mut c, &win, &work)?;
        remapped_hotkeys(&mut c, &win, &work)?;
        hook_speed(&mut c, &work)?;
        Ok(())
    })();
    drop(win);
    // Full-screen images may show private things: delete them all.
    let _ = std::fs::remove_dir_all(&work);
    c.check("cleanup: test recordings deleted", !work.exists(), "");
    if let Err(e) = result {
        c.check("run: the test finished", false, format!("{e:#}"));
    }
    Ok(c)
}

fn main() -> ExitCode {
    let out = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/capture-selftest"));
    match run(&out) {
        Ok(c) => {
            println!("SUMMARY {} passed, {} failed", c.passed, c.failed);
            let _ = std::fs::write(out.join("selftest-summary.txt"), c.lines.join("\n"));
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

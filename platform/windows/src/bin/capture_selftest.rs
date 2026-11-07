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
use babelsteps_windows::testing::{self, Frame, Monitor};
use babelsteps_windows::testwin::{self, TestWindow, GREEN};
use babelsteps_windows::{Recorder, RecorderConfig, RecorderReport};
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
    Frame::load_png(&bundle.join(rel), m.x, m.y).with_context(|| format!("reading {rel}"))
}

fn near(a: [u8; 3], b: [u8; 3], tol: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= tol)
}

/// The click marker's exact colour. It is drawn after redaction, on top of the
/// black box, and differs from the test window's red background.
const MARKER: [u8; 3] = [255, 0, 0];

/// Every pixel of the rectangle (shrunk by one pixel, to skip edges) is black,
/// or the click marker drawn over the black.
fn all_black(f: &Frame, (x, y, w, h): (i32, i32, i32, i32)) -> bool {
    (y + 1..y + h - 1).all(|py| (x + 1..x + w - 1).all(|px| f.sample(px, py).is_some_and(|c| c.iter().all(|&v| v < 10) || c == MARKER)))
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
        std::fs::write(crop_out, crop.encode_png()?)?;
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
                    "image with the field black".to_string()
                } else {
                    "IMAGE WITH THE FIELD VISIBLE".to_string()
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
        window_changes_after_click(&mut c, &win, &work)?;
        fail_closed(&mut c, &win, &work, images)?;
        timeout_and_recovery(&mut c, &win, &work)?;
        scan_failure(&mut c, &win, &work)?;
        gdi_marked(&mut c, &win, &work)?;
        pause_and_comment(&mut c, &win, &work)?;
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

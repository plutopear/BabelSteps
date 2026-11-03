//! Automated run of spikes 1 to 4. All synthetic input lands on our own test window.

use crate::imgops::Frame;
use crate::recorder::{ClickTrace, Options, Recorder, Summary};
use crate::testwin::{self, TestWindow, GREEN, RED};
use crate::{clock, gdi, monitors, process};
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, sleep};
use std::time::{Duration, Instant};

const BEFORE_TRIALS: usize = 50;
const GDI_TRIALS: usize = 30;
const LOAD_TRIALS: usize = 30;
const DELAY_TRIALS: usize = 20;
const BURST_PRESSES: usize = 5000;

#[derive(Serialize)]
struct Dist {
    n: usize,
    min: f64,
    p50: f64,
    p95: f64,
    max: f64,
}

fn dist(mut v: Vec<f64>) -> Option<Dist> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    Some(Dist { n: v.len(), min: v[0], p50: at(0.5), p95: at(0.95), max: v[v.len() - 1] })
}

fn fmt(d: &Option<Dist>) -> String {
    d.as_ref()
        .map(|d| format!("n={} min={:.2} p50={:.2} p95={:.2} max={:.2}", d.n, d.min, d.p50, d.p95, d.max))
        .unwrap_or_else(|| "none".to_string())
}

#[derive(Serialize)]
struct EncodeBench {
    format: String,
    ms: f64,
    bytes: usize,
}

#[derive(Serialize)]
struct Check {
    name: String,
    pass: bool,
    detail: String,
}

#[derive(Serialize)]
struct BeforeResult {
    trials: usize,
    before: usize,
    after: usize,
    /// Frames that arrived after the click and were still used.
    frames_newer_than_click: usize,
}

#[derive(Serialize)]
struct Report {
    monitors: Vec<monitors::Monitor>,
    logical_cpus: usize,
    gdi_capture_ms: Option<Dist>,
    encode: Vec<EncodeBench>,
    cpu_static_pct_of_one_core: f64,
    cpu_busy_pct_of_one_core: f64,
    memory_start_mb: f64,
    memory_capture_idle_mb: f64,
    memory_end_mb: f64,
    memory_peak_mb: f64,
    border_yellow_pixels_before: usize,
    border_yellow_pixels_during: usize,
    click_to_pixels_ms: Option<Dist>,
    click_to_pixels_under_load_ms: Option<Dist>,
    readback_ms: Option<Dist>,
    uia_ms: Option<Dist>,
    password_scan_ms: Option<Dist>,
    png_ms: Option<Dist>,
    latest_frame: BeforeResult,
    latest_frame_under_load: BeforeResult,
    gdi_on_demand: BeforeResult,
    burst_key_downs: usize,
    burst_seconds: f64,
    summary: Summary,
    checks: Vec<Check>,
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| (*x as i32 - *y as i32).abs() <= 12)
}

fn wait_for_clicks(rec: &Recorder, target: usize) {
    let start = Instant::now();
    while rec.completed_clicks() < target && start.elapsed() < Duration::from_secs(5) {
        sleep(Duration::from_millis(5));
    }
}

fn measure_cpu(duration: Duration) -> f64 {
    let c0 = process::cpu_seconds();
    let t0 = Instant::now();
    sleep(duration);
    (process::cpu_seconds() - c0) / t0.elapsed().as_secs_f64() * 100.0
}

fn bench_gdi(m: &monitors::Monitor) -> (Option<Dist>, Option<Frame>) {
    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..15 {
        let t0 = clock::now();
        if let Ok(f) = gdi::capture_rect(m.x, m.y, m.w, m.h) {
            times.push(clock::to_ms(clock::now() - t0));
            last = Some(f);
        }
    }
    (dist(times), last)
}

fn bench_encoders(frame: &Frame) -> Vec<EncodeBench> {
    let mut out = Vec::new();
    for (name, level) in [
        ("png-fast", png::Compression::Fast),
        ("png-balanced", png::Compression::Balanced),
        ("png-high", png::Compression::High),
    ] {
        let t0 = clock::now();
        if let Ok(bytes) = frame.encode_png(level) {
            out.push(EncodeBench { format: name.into(), ms: clock::to_ms(clock::now() - t0), bytes: bytes.len() });
        }
    }
    let t0 = clock::now();
    if let Ok(bytes) = frame.encode_webp() {
        out.push(EncodeBench { format: "webp-lossless".into(), ms: clock::to_ms(clock::now() - t0), bytes: bytes.len() });
    }
    out
}

/// Pixels along the screen's outer edge that are the capture border's yellow.
fn yellow_edge_pixels(f: &Frame) -> usize {
    let is_yellow = |p: [u8; 3]| p[0] > 200 && p[1] > 170 && p[2] < 110;
    let (w, h) = (f.width as i32, f.height as i32);
    let mut n = 0;
    for x in 0..w {
        for y in [0, h - 1] {
            if f.sample(f.origin_x + x, f.origin_y + y).map(is_yellow).unwrap_or(false) {
                n += 1;
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            if f.sample(f.origin_x + x, f.origin_y + y).map(is_yellow).unwrap_or(false) {
                n += 1;
            }
        }
    }
    n
}

fn before_result(traces: &[ClickTrace], use_gdi: bool) -> BeforeResult {
    let sample = |t: &ClickTrace| if use_gdi { t.probe_gdi } else { t.probe_latest };
    BeforeResult {
        trials: traces.len(),
        before: traces.iter().filter(|t| sample(t).map(|p| near(p, GREEN)).unwrap_or(false)).count(),
        after: traces.iter().filter(|t| sample(t).map(|p| near(p, RED)).unwrap_or(false)).count(),
        frames_newer_than_click: traces.iter().filter(|t| t.frame_age_ms < 0.0).count(),
    }
}

/// Every pixel of a redacted rectangle in the file on disk must be black, or red where
/// the click marker was drawn on top. Returns the number of pixels that are neither.
fn leaked_pixels(path: &std::path::Path, origin: (i32, i32), rects: &[babelsteps_core::Rect]) -> Result<usize> {
    let f = Frame::load_png(path, origin.0, origin.1)?;
    let mut leaked = 0;
    for r in rects {
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                if let Some(p) = f.sample(x, y) {
                    if p != [0, 0, 0] && p != [255, 0, 0] {
                        leaked += 1;
                    }
                }
            }
        }
    }
    Ok(leaked)
}

fn click_and_wait(win: &TestWindow, rec: &Recorder, done: &mut usize, p: (i32, i32)) -> Result<()> {
    win.safe_click(p.0, p.1)?;
    *done += 1;
    wait_for_clicks(rec, *done);
    Ok(())
}

pub fn run(out_dir: PathBuf) -> Result<bool> {
    let (memory_start_mb, _) = process::memory_mb();
    let mons = monitors::enumerate();
    let logical_cpus = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!("Monitors: {}", serde_json::to_string(&mons)?);
    println!("Logical CPUs: {logical_cpus}");
    let primary = mons.iter().find(|m| m.primary).or(mons.first()).copied();
    let (gdi_capture_ms, sample_frame) = match &primary {
        Some(m) => bench_gdi(m),
        None => (None, None),
    };
    let encode = sample_frame.as_ref().map(bench_encoders).unwrap_or_default();
    let border_yellow_pixels_before = sample_frame.as_ref().map(yellow_edge_pixels).unwrap_or(0);
    drop(sample_frame);

    let win = TestWindow::spawn()?;
    sleep(Duration::from_millis(300));
    let probe = win.probe_point()?;
    let bg = win.background_point()?;
    let save = win.save_center()?;
    let user = win.user_center()?;
    let pass = win.pass_center()?;
    let origin = primary.map(|m| (m.x, m.y)).unwrap_or((0, 0));

    let rec = Recorder::start(Options {
        out_dir: out_dir.clone(),
        title: "BabelSteps M0 self-test".to_string(),
        probe: Some(probe),
        only_injected: true,
    })?;
    sleep(Duration::from_millis(700));
    let (memory_capture_idle_mb, _) = process::memory_mb();
    let border_yellow_pixels_during = primary
        .and_then(|m| gdi::capture_rect(m.x, m.y, m.w, m.h).ok())
        .as_ref()
        .map(yellow_edge_pixels)
        .unwrap_or(0);

    // CPU with capture running and an unchanging screen.
    let cpu_static_pct_of_one_core = measure_cpu(Duration::from_secs(4));

    let mut done = 0usize;

    // Phase A: a real button, which reacts on release. Used for the naming check.
    win.set_red(false);
    sleep(Duration::from_millis(200));
    click_and_wait(&win, &rec, &mut done, save)?;
    let a_range = 0..done;

    // Phase B: the hard case. The window reacts on press and repaints immediately.
    let b_start = done;
    for _ in 0..BEFORE_TRIALS {
        win.set_red(false);
        sleep(Duration::from_millis(150));
        click_and_wait(&win, &rec, &mut done, bg)?;
    }
    let b_range = b_start..done;

    // Phase C: same clicks, measuring a simple on-demand capture.
    rec.set_gdi_first(true);
    let c_start = done;
    for _ in 0..GDI_TRIALS {
        win.set_red(false);
        sleep(Duration::from_millis(150));
        click_and_wait(&win, &rec, &mut done, bg)?;
    }
    let c_range = c_start..done;
    rec.set_gdi_first(false);

    // Phase D: same clicks with every CPU core busy.
    let load_stop = Arc::new(AtomicBool::new(false));
    let load_threads: Vec<_> = (0..logical_cpus)
        .map(|_| {
            let stop = load_stop.clone();
            thread::spawn(move || {
                let mut x = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    x = x.wrapping_mul(31).wrapping_add(7);
                    std::hint::black_box(x);
                }
            })
        })
        .collect();
    let d_start = done;
    let mut load_result = Ok(());
    for _ in 0..LOAD_TRIALS {
        win.set_red(false);
        sleep(Duration::from_millis(150));
        load_result = click_and_wait(&win, &rec, &mut done, bg);
        if load_result.is_err() {
            break;
        }
    }
    let d_range = d_start..done;
    load_stop.store(true, Ordering::Relaxed);
    for t in load_threads {
        let _ = t.join();
    }
    load_result?;

    // Phase D2: force the race. The worker waits 40 ms, by which time the window has
    // repainted and a newer frame exists. The history must still pick the older frame.
    rec.set_worker_delay_ms(40);
    let d2_start = done;
    for _ in 0..DELAY_TRIALS {
        win.set_red(false);
        sleep(Duration::from_millis(150));
        click_and_wait(&win, &rec, &mut done, bg)?;
    }
    let d2_range = d2_start..done;
    rec.set_worker_delay_ms(0);

    // Phase E: type into both fields, then click Save with the password field filled.
    // This is the only phase that takes keyboard focus, and it is about a second long.
    win.set_red(false);
    win.allow_focus(true);
    sleep(Duration::from_millis(150));
    click_and_wait(&win, &rec, &mut done, user)?;
    sleep(Duration::from_millis(60));
    let typed_user = win.safe_type("hello");
    click_and_wait(&win, &rec, &mut done, pass)?;
    let pass_step = done - 1;
    sleep(Duration::from_millis(60));
    let typed_pass = win.safe_type("secret");
    sleep(Duration::from_millis(150));
    click_and_wait(&win, &rec, &mut done, save)?;
    let filled_step = done - 1;
    win.allow_focus(false);

    // Phase F: a key burst, to look for dropped events and slow callbacks.
    let keys_before = rec.key_downs();
    let t_burst = Instant::now();
    testwin::f24_burst(BURST_PRESSES);
    while rec.key_downs() < keys_before + BURST_PRESSES && t_burst.elapsed() < Duration::from_secs(10) {
        sleep(Duration::from_millis(5));
    }
    let burst_seconds = t_burst.elapsed().as_secs_f64();
    let burst_key_downs = rec.key_downs() - keys_before;

    // Phase G: CPU with the window repainting about 60 times a second.
    let cpu_busy_pct_of_one_core = {
        let c0 = process::cpu_seconds();
        let t0 = Instant::now();
        let mut red = false;
        while t0.elapsed() < Duration::from_secs(4) {
            red = !red;
            win.set_red(red);
            sleep(Duration::from_millis(16));
        }
        (process::cpu_seconds() - c0) / t0.elapsed().as_secs_f64() * 100.0
    };

    let (memory_end_mb, memory_peak_mb) = process::memory_mb();
    let window_rect = win.rect()?;
    let summary = rec.stop()?;
    drop(win);

    let traces = &summary.traces;
    let latest_frame = before_result(&traces[b_range.clone()], false);
    let c_range_for_timing = c_range.clone();
    let gdi_on_demand = before_result(&traces[c_range], true);
    let latest_frame_under_load = before_result(&traces[d_range.clone()], false);
    let delayed = before_result(&traces[d2_range.clone()], false);
    let delayed_skips = traces[d2_range.clone()].iter().filter(|t| t.skipped_newer).count();

    let doc = out_dir.join("document.md");
    let markdown = std::fs::read_to_string(&doc).unwrap_or_default();
    let image_count = std::fs::read_dir(out_dir.join("images")).map(|d| d.count()).unwrap_or(0);

    let save_trace = traces.get(a_range.start);
    let pass_trace = traces.get(pass_step);
    let filled_trace = traces.get(filled_step);

    let step_png = |i: usize| out_dir.join("images").join(format!("step-{:03}.png", i + 1));
    let pass_leak = pass_trace
        .map(|t| leaked_pixels(&step_png(pass_step), origin, &t.redacted_rects))
        .transpose()?;
    let filled_leak = filled_trace
        .map(|t| leaked_pixels(&step_png(filled_step), origin, &t.redacted_rects))
        .transpose()?;

    // A crop holding only our test window, so the result can be looked at without
    // exposing anything else on the screen.
    if let Ok(f) = Frame::load_png(&step_png(filled_step), origin.0, origin.1) {
        let (x, y, w, h) = window_rect;
        let crop = f.crop(x, y, w, h);
        let _ = std::fs::write(out_dir.join("test-window-crop.png"), crop.encode_png(png::Compression::Fast)?);
    }

    // Full-screen images can show whatever else is on the screen. Everything that
    // needed them has been checked, so only the crop of our own window is kept.
    let _ = std::fs::remove_dir_all(out_dir.join("images"));

    // Phase C is excluded: its timing includes the on-demand comparison capture.
    let click_to_pixels_ms = dist(
        traces
            .iter()
            .enumerate()
            .filter(|(i, _)| !d_range.contains(i) && !c_range_for_timing.contains(i) && !d2_range.contains(i))
            .map(|(_, t)| t.click_to_pixels_ms)
            .collect(),
    );
    let click_to_pixels_under_load_ms = dist(traces[d_range.clone()].iter().map(|t| t.click_to_pixels_ms).collect());
    let d_for_print = d_range;

    let checks = vec![
        Check {
            name: "Scenario: typing stayed in the test window".into(),
            pass: typed_user.is_ok() && typed_pass.is_ok(),
            detail: format!("user: {typed_user:?}, password: {typed_pass:?}"),
        },
        Check {
            name: "Spike 1: every synthetic click reached the mouse hook".into(),
            pass: summary.mouse_downs == done,
            detail: format!("{} of {done}", summary.mouse_downs),
        },
        Check {
            name: "Spike 1: key burst fully received, nothing dropped".into(),
            pass: burst_key_downs == BURST_PRESSES && summary.hooks.dropped == 0,
            detail: format!(
                "{burst_key_downs} of {BURST_PRESSES} in {burst_seconds:.2} s, {} dropped",
                summary.hooks.dropped
            ),
        },
        Check {
            name: "Spike 1: hook callback stays far below the Windows timeout (under 5 ms)".into(),
            pass: summary.hooks.max_us < 5000.0,
            detail: format!(
                "mean {:.2} us, max {:.2} us over {} callbacks",
                summary.hooks.mean_us, summary.hooks.max_us, summary.hooks.callbacks
            ),
        },
        Check {
            name: "Spike 2: every step used Windows.Graphics.Capture".into(),
            pass: summary.wgc_used && traces.iter().all(|t| t.source == "wgc-latest"),
            detail: format!("{} frames, error: {:?}", summary.wgc_frames, summary.wgc_error),
        },
        Check {
            name: "Spike 2: state before the click, window reacting on press".into(),
            pass: latest_frame.before == BEFORE_TRIALS,
            detail: format!(
                "before {} of {BEFORE_TRIALS}, after {}, frames newer than click {}",
                latest_frame.before, latest_frame.after, latest_frame.frames_newer_than_click
            ),
        },
        Check {
            name: "Spike 2: state before the click, with all CPU cores busy".into(),
            pass: latest_frame_under_load.before == LOAD_TRIALS,
            detail: format!(
                "before {} of {LOAD_TRIALS}, after {}, frames newer than click {}",
                latest_frame_under_load.before,
                latest_frame_under_load.after,
                latest_frame_under_load.frames_newer_than_click
            ),
        },
        Check {
            name: "Spike 2: forced race, 40 ms worker delay: history skips newer frames".into(),
            pass: delayed.before == DELAY_TRIALS && delayed_skips > 0,
            detail: format!(
                "before {} of {DELAY_TRIALS}, after {}, newer frame skipped {delayed_skips} times",
                delayed.before, delayed.after
            ),
        },
        Check {
            name: "Spike 3: UI Automation names the Save button".into(),
            pass: save_trace.map(|t| t.target_name == "Save" && t.target_type == "Button").unwrap_or(false),
            detail: save_trace
                .map(|t| format!("name={:?} type={:?} window={:?} process={:?}", t.target_name, t.target_type, t.window_title, t.process_name))
                .unwrap_or_default(),
        },
        Check {
            name: "Spike 3: UI Automation flags the clicked password field".into(),
            pass: pass_trace.map(|t| t.is_password).unwrap_or(false),
            detail: pass_trace
                .map(|t| format!("name={:?} type={:?} is_password={}", t.target_name, t.target_type, t.is_password))
                .unwrap_or_default(),
        },
        Check {
            name: "Privacy: clicked password field is black in the PNG on disk".into(),
            pass: pass_trace.map(|t| !t.redacted_rects.is_empty()).unwrap_or(false) && pass_leak == Some(0),
            detail: format!("leaked pixels: {pass_leak:?}"),
        },
        Check {
            name: "Privacy: filled password field is black when clicking elsewhere (PNG on disk)".into(),
            pass: filled_trace.map(|t| !t.redacted_rects.is_empty()).unwrap_or(false) && filled_leak == Some(0),
            detail: format!(
                "rects {:?}, leaked pixels: {filled_leak:?}",
                filled_trace.map(|t| t.redacted_rects.clone()).unwrap_or_default()
            ),
        },
        Check {
            name: "Spike 4: Markdown and one image per step were written".into(),
            pass: markdown.contains("## Step 1.") && image_count == summary.steps && summary.steps == done,
            detail: format!("{} steps, {image_count} images, {done} clicks", summary.steps),
        },
        Check {
            name: "No worker errors".into(),
            pass: summary.errors.is_empty(),
            detail: format!("{:?}", summary.errors),
        },
        Check {
            name: "Target D6: click to pixels under 150 ms at p95, including under load".into(),
            pass: click_to_pixels_ms.as_ref().map(|d| d.p95 < 150.0).unwrap_or(false)
                && click_to_pixels_under_load_ms.as_ref().map(|d| d.p95 < 150.0).unwrap_or(false),
            detail: format!(
                "normal p95 {:.1} ms, under load p95 {:.1} ms",
                click_to_pixels_ms.as_ref().map(|d| d.p95).unwrap_or(-1.0),
                click_to_pixels_under_load_ms.as_ref().map(|d| d.p95).unwrap_or(-1.0)
            ),
        },
    ];

    let all_pass = checks.iter().all(|c| c.pass);
    let report = Report {
        monitors: mons,
        logical_cpus,
        gdi_capture_ms,
        encode,
        cpu_static_pct_of_one_core,
        cpu_busy_pct_of_one_core,
        memory_start_mb,
        memory_capture_idle_mb,
        memory_end_mb,
        memory_peak_mb,
        border_yellow_pixels_before,
        border_yellow_pixels_during,
        click_to_pixels_ms,
        click_to_pixels_under_load_ms,
        readback_ms: dist(traces.iter().map(|t| t.readback_ms).collect()),
        uia_ms: dist(traces.iter().map(|t| t.uia_ms).collect()),
        password_scan_ms: dist(traces.iter().map(|t| t.password_scan_ms).collect()),
        png_ms: dist(traces.iter().filter_map(|t| t.png_ms).collect()),
        latest_frame,
        latest_frame_under_load,
        gdi_on_demand,
        burst_key_downs,
        burst_seconds,
        summary,
        checks,
    };

    let report_path = out_dir.join("selftest-report.json");
    std::fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;

    println!("\nChecks:");
    for c in &report.checks {
        println!("  [{}] {} ({})", if c.pass { "PASS" } else { "FAIL" }, c.name, c.detail);
    }
    println!("\nTimings (ms):");
    println!("  click to pixels:        {}", fmt(&report.click_to_pixels_ms));
    println!("  click to pixels (load): {}", fmt(&report.click_to_pixels_under_load_ms));
    println!("  readback:               {}", fmt(&report.readback_ms));
    println!("  UI Automation lookup:   {}", fmt(&report.uia_ms));
    println!("  password field scan:    {}", fmt(&report.password_scan_ms));
    println!("  PNG encode and write:   {}", fmt(&report.png_ms));
    println!("  GDI full screen:        {}", fmt(&report.gdi_capture_ms));
    println!(
        "  hook callback: mean {:.2} us, max {:.2} us, {} callbacks, histogram {:?} (limits {:?} us)",
        report.summary.hooks.mean_us,
        report.summary.hooks.max_us,
        report.summary.hooks.callbacks,
        report.summary.hooks.histogram,
        crate::hooks::BUCKETS_US
    );
    println!("\nBefore-state comparison (window reacts on press):");
    println!(
        "  frame history:       {} before, {} after, of {}. Newer frame skipped {} times, none available {} times",
        report.latest_frame.before,
        report.latest_frame.after,
        report.latest_frame.trials,
        report.summary.traces[b_range.clone()].iter().filter(|t| t.skipped_newer).count(),
        report.latest_frame.frames_newer_than_click
    );
    println!(
        "  frame history, load: {} before, {} after, of {}. Newer frame skipped {} times, none available {} times",
        report.latest_frame_under_load.before,
        report.latest_frame_under_load.after,
        report.latest_frame_under_load.trials,
        report.summary.traces[d_for_print.clone()].iter().filter(|t| t.skipped_newer).count(),
        report.latest_frame_under_load.frames_newer_than_click
    );
    println!(
        "  GDI on demand:       {} before, {} after, of {}",
        report.gdi_on_demand.before, report.gdi_on_demand.after, report.gdi_on_demand.trials
    );
    println!("\nResources:");
    println!(
        "  CPU, static screen: {:.2}% of one core. CPU, busy window: {:.2}% of one core ({logical_cpus} logical CPUs)",
        report.cpu_static_pct_of_one_core, report.cpu_busy_pct_of_one_core
    );
    println!(
        "  Memory MB: start {:.1}, capture idle {:.1}, end {:.1}, peak {:.1}",
        report.memory_start_mb, report.memory_capture_idle_mb, report.memory_end_mb, report.memory_peak_mb
    );
    println!(
        "  WGC frames: {}, mean delivery {:.1} ms. Border disabled reported: {}, error: {:?}",
        report.summary.wgc_frames,
        report.summary.wgc_mean_delivery_ms,
        report.summary.border_disabled,
        report.summary.border_error
    );
    println!(
        "  Yellow edge pixels in GDI capture: before capture {}, during capture {}",
        report.border_yellow_pixels_before, report.border_yellow_pixels_during
    );
    println!("\nEncoders on one full-screen frame:");
    for e in &report.encode {
        println!("  {:<14} {:>7.1} ms  {:>9} bytes", e.format, e.ms, e.bytes);
    }
    println!("\nOutput: {}", out_dir.display());
    println!("Report: {}", report_path.display());
    Ok(all_pass)
}

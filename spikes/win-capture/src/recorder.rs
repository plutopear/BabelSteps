//! Spike 4: the end-to-end path. Click → screenshot → UI target → step → Markdown.

use crate::clock;
use crate::gdi;
use crate::hooks::{self, HookHandle, HookStats, InputEvent};
use crate::imgops::Frame;
use crate::monitors::{self, Monitor};
use crate::uia::{self, Uia};
use crate::wgc::WgcCapturer;
use anyhow::{Context, Result};
use babelsteps_core::markdown::to_markdown;
use babelsteps_core::{describe_click, Action, Block, MouseButton, Recording, Rect, StepBlock};
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};

pub struct Options {
    pub out_dir: PathBuf,
    pub title: String,
    /// A screen point sampled from each capture source, to test before/after behavior.
    pub probe: Option<(i32, i32)>,
    /// Ignore real input and act only on synthetic events. The self-test uses this, so
    /// whatever the person at the machine is doing is neither recorded nor counted.
    pub only_injected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClickTrace {
    pub step: usize,
    pub x: i32,
    pub y: i32,
    pub source: String,
    /// Hook timestamp to pixels available in CPU memory.
    pub click_to_pixels_ms: f64,
    pub readback_ms: f64,
    pub frame_age_ms: f64,
    pub uia_ms: f64,
    pub password_scan_ms: f64,
    /// A newer frame existed at readback and was correctly passed over.
    pub skipped_newer: bool,
    pub gdi_ms: Option<f64>,
    pub png_ms: Option<f64>,
    pub png_bytes: Option<usize>,
    pub probe_latest: Option<[u8; 3]>,
    pub probe_gdi: Option<[u8; 3]>,
    pub target_name: String,
    pub target_type: String,
    pub window_title: String,
    pub process_name: String,
    pub is_password: bool,
    /// Screen rectangles blacked out in this step's image.
    pub redacted_rects: Vec<Rect>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub out_dir: String,
    pub steps: usize,
    pub mouse_downs: usize,
    pub key_downs: usize,
    pub wgc_used: bool,
    pub wgc_error: Option<String>,
    pub wgc_frames: u64,
    pub wgc_mean_delivery_ms: f64,
    pub border_disabled: bool,
    pub border_error: Option<String>,
    pub hooks: HookStats,
    pub traces: Vec<ClickTrace>,
    pub errors: Vec<String>,
}

struct EncodeJob {
    step: usize,
    path: PathBuf,
    frame: Frame,
}

struct WorkerOut {
    recording: Recording,
    traces: Vec<ClickTrace>,
    mouse_downs: usize,
    key_downs: usize,
    errors: Vec<String>,
    wgc_frames: u64,
    wgc_mean_delivery_ms: f64,
    border_disabled: bool,
    border_error: Option<String>,
}

pub struct Recorder {
    out_dir: PathBuf,
    hooks: HookHandle,
    stop: Arc<AtomicBool>,
    gdi_first: Arc<AtomicBool>,
    worker_delay_ms: Arc<AtomicU64>,
    worker: JoinHandle<WorkerOut>,
    encoder: JoinHandle<Vec<(usize, f64, usize)>>,
    completed: Arc<AtomicUsize>,
    key_downs: Arc<AtomicUsize>,
    wgc_used: bool,
    wgc_error: Option<String>,
}

impl Recorder {
    pub fn start(opts: Options) -> Result<Self> {
        std::fs::create_dir_all(opts.out_dir.join("images")).context("create output folder")?;
        let monitors = monitors::enumerate();

        let (wgc, wgc_error) = if WgcCapturer::supported() {
            match WgcCapturer::start(&monitors) {
                Ok(w) => (Some(w), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            }
        } else {
            (None, Some("Windows.Graphics.Capture is not supported here".to_string()))
        };
        let wgc_used = wgc.is_some();

        let (enc_tx, enc_rx) = bounded::<EncodeJob>(8);
        let encoder = thread::Builder::new().name("png-encoder".into()).spawn(move || encoder_loop(enc_rx))?;

        let (hooks_handle, events) = hooks::start()?;
        let stop = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicUsize::new(0));
        let key_downs = Arc::new(AtomicUsize::new(0));
        let gdi_first = Arc::new(AtomicBool::new(false));
        let worker_delay_ms = Arc::new(AtomicU64::new(0));

        let worker = {
            let worker_delay_ms = worker_delay_ms.clone();
            let stop = stop.clone();
            let completed = completed.clone();
            let key_downs = key_downs.clone();
            let gdi_first = gdi_first.clone();
            let out_dir = opts.out_dir.clone();
            let title = opts.title.clone();
            let probe = opts.probe;
            let only_injected = opts.only_injected;
            thread::Builder::new().name("step-worker".into()).spawn(move || {
                unsafe {
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
                }
                worker_loop(WorkerArgs {
                    events,
                    worker_delay_ms,
                    only_injected,
                    stop,
                    completed,
                    key_downs,
                    gdi_first,
                    enc_tx,
                    wgc,
                    monitors,
                    out_dir,
                    title,
                    probe,
                })
            })?
        };

        Ok(Self {
            out_dir: opts.out_dir,
            hooks: hooks_handle,
            stop,
            gdi_first,
            worker_delay_ms,
            worker,
            encoder,
            completed,
            key_downs,
            wgc_used,
            wgc_error,
        })
    }

    /// Clicks fully processed so far.
    pub fn completed_clicks(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }

    /// Key presses counted so far.
    pub fn key_downs(&self) -> usize {
        self.key_downs.load(Ordering::Relaxed)
    }

    /// Delay the worker before it captures, forcing frames newer than the click to
    /// exist. This exercises the frame history's "skip newer frames" path.
    pub fn set_worker_delay_ms(&self, ms: u64) {
        self.worker_delay_ms.store(ms, Ordering::Relaxed);
    }

    /// When on, each click runs an on-demand GDI capture first, to measure what
    /// a simple "capture when the click arrives" design would have seen.
    pub fn set_gdi_first(&self, on: bool) {
        self.gdi_first.store(on, Ordering::Relaxed);
    }

    pub fn stop(self) -> Result<Summary> {
        self.hooks.stop();
        self.stop.store(true, Ordering::Relaxed);
        let mut out = self.worker.join().map_err(|_| anyhow::anyhow!("worker panicked"))?;
        let encoded = self.encoder.join().map_err(|_| anyhow::anyhow!("encoder panicked"))?;
        for (step, ms, bytes) in encoded {
            if let Some(t) = out.traces.iter_mut().find(|t| t.step == step) {
                t.png_ms = Some(ms);
                t.png_bytes = Some(bytes);
            }
        }
        write_documents(&self.out_dir, &out.recording)?;
        Ok(Summary {
            out_dir: self.out_dir.display().to_string(),
            steps: out.recording.step_count(),
            mouse_downs: out.mouse_downs,
            key_downs: out.key_downs,
            wgc_used: self.wgc_used,
            wgc_error: self.wgc_error,
            wgc_frames: out.wgc_frames,
            wgc_mean_delivery_ms: out.wgc_mean_delivery_ms,
            border_disabled: out.border_disabled,
            border_error: out.border_error,
            hooks: hooks::stats(),
            traces: out.traces,
            errors: out.errors,
        })
    }
}

fn write_documents(dir: &Path, rec: &Recording) -> Result<()> {
    std::fs::write(dir.join("recording.json"), serde_json::to_vec_pretty(rec)?)?;
    std::fs::write(dir.join("document.md"), to_markdown(rec))?;
    Ok(())
}

fn encoder_loop(rx: Receiver<EncodeJob>) -> Vec<(usize, f64, usize)> {
    let mut results = Vec::new();
    for job in rx {
        let t0 = clock::now();
        match job.frame.encode_png(png::Compression::Fast) {
            Ok(bytes) => {
                let wrote = std::fs::write(&job.path, &bytes).is_ok();
                let ms = clock::to_ms(clock::now() - t0);
                if wrote {
                    results.push((job.step, ms, bytes.len()));
                }
            }
            Err(e) => eprintln!("png encode failed for step {}: {e:#}", job.step),
        }
    }
    results
}

struct WorkerArgs {
    events: Receiver<InputEvent>,
    worker_delay_ms: Arc<AtomicU64>,
    only_injected: bool,
    stop: Arc<AtomicBool>,
    completed: Arc<AtomicUsize>,
    key_downs: Arc<AtomicUsize>,
    gdi_first: Arc<AtomicBool>,
    enc_tx: Sender<EncodeJob>,
    wgc: Option<WgcCapturer>,
    monitors: Vec<Monitor>,
    out_dir: PathBuf,
    title: String,
    probe: Option<(i32, i32)>,
}

fn worker_loop(a: WorkerArgs) -> WorkerOut {
    let uia = Uia::new();
    let mut errors = Vec::new();
    if let Err(e) = &uia {
        errors.push(format!("UI Automation init failed: {e:#}"));
    }

    let mut rec = Recording::new(format!("rec-{}", clock::unix_ms()), a.title.clone(), clock::unix_ms());
    let mut traces = Vec::new();
    let (mut mouse_downs, mut key_downs) = (0usize, 0usize);

    loop {
        match a.events.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(InputEvent::KeyDown { injected, .. }) if injected || !a.only_injected => {
                key_downs += 1;
                a.key_downs.fetch_add(1, Ordering::Relaxed);
            }
            Ok(InputEvent::MouseDown { button, x, y, t, injected }) if injected || !a.only_injected => {
                mouse_downs += 1;
                match process_click(&a, uia.as_ref().ok(), &mut rec, button, x, y, t) {
                    Ok(trace) => traces.push(trace),
                    Err(e) => errors.push(format!("click at ({x}, {y}): {e:#}")),
                }
                a.completed.fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => {
                if a.stop.load(Ordering::Relaxed) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let (wgc_frames, wgc_mean_delivery_ms, border_disabled, border_error) = match &a.wgc {
        Some(w) => (w.frames_received(), w.mean_delivery_ms(), w.border_disabled, w.border_error.clone()),
        None => (0, 0.0, false, None),
    };
    WorkerOut {
        recording: rec,
        traces,
        mouse_downs,
        key_downs,
        errors,
        wgc_frames,
        wgc_mean_delivery_ms,
        border_disabled,
        border_error,
    }
}

fn process_click(
    a: &WorkerArgs,
    uia: Option<&Uia>,
    rec: &mut Recording,
    button: MouseButton,
    x: i32,
    y: i32,
    t: i64,
) -> Result<ClickTrace> {
    let mon_idx = monitors::index_for(&a.monitors, x, y);
    let mon = a.monitors[mon_idx];
    let step_no = rec.step_count() + 1;
    let delay = a.worker_delay_ms.load(Ordering::Relaxed);
    if delay > 0 {
        thread::sleep(std::time::Duration::from_millis(delay));
    }

    // Measurement mode: what would a simple on-demand capture have seen?
    let (probe_gdi, gdi_ms) = if a.gdi_first.load(Ordering::Relaxed) {
        let t0 = clock::now();
        let g = gdi::capture_rect(mon.x, mon.y, mon.w, mon.h)?;
        let ms = clock::to_ms(clock::now() - t0);
        (a.probe.and_then(|(px, py)| g.sample(px, py)), Some(ms))
    } else {
        (None, None)
    };

    // 1. Latest-frame capture, as early as possible.
    let (mut frame, source, readback_ms, frame_age_ms, skipped_newer) =
        match a.wgc.as_ref().map(|w| w.grab(mon_idx, t)) {
            Some(Ok(g)) => (g.frame, "wgc-latest", g.readback_ms, g.frame_age_ms, g.skipped_newer),
            _ => {
                let f = gdi::capture_rect(mon.x, mon.y, mon.w, mon.h)?;
                (f, "gdi-fallback", 0.0, 0.0, false)
            }
        };
    let click_to_pixels_ms = clock::to_ms(clock::now() - t);
    let probe_latest = a.probe.and_then(|(px, py)| frame.sample(px, py));

    // 2. What was clicked.
    let t_uia = clock::now();
    let target = uia.and_then(|u| u.target_at(x, y).ok());
    let uia_ms = clock::to_ms(clock::now() - t_uia);

    // 3. Redact before anything is written, so a password field's pixels never reach
    //    the disk. This covers the clicked field and every other password field in
    //    the clicked window, because a field shows its length even when not clicked.
    let t_scan = clock::now();
    let mut screen_rects: Vec<Rect> = Vec::new();
    if let Some(b) = target.as_ref().filter(|t| t.is_password).and_then(|t| t.bounds) {
        screen_rects.push(b);
    }
    if let (Some(u), Some(root)) = (uia, uia::root_at(x, y)) {
        if let Ok(found) = u.password_rects(root) {
            for r in found {
                if !screen_rects.contains(&r) {
                    screen_rects.push(r);
                }
            }
        }
    }
    let password_scan_ms = clock::to_ms(clock::now() - t_scan);
    // The element at a point is not reliable for this: in a browser it can report
    // false for a real password field. A click inside any found field counts.
    let clicked_password = target.as_ref().map(|t| t.is_password).unwrap_or(false)
        || screen_rects.iter().any(|r| x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h);
    let target = target.map(|mut t| {
        t.is_password = clicked_password;
        t
    });
    let mut redactions: Vec<Rect> = Vec::new();
    for b in &screen_rects {
        frame.black_out(b.x, b.y, b.w, b.h);
        redactions.push(Rect { x: b.x - frame.origin_x, y: b.y - frame.origin_y, w: b.w, h: b.h });
    }

    // 4. Mark the click, then queue the image for encoding off this thread.
    frame.draw_click_marker(x, y);
    let rel = format!("images/step-{step_no:03}.png");
    a.enc_tx
        .send(EncodeJob { step: step_no, path: a.out_dir.join(&rel), frame })
        .context("encoder thread stopped")?;

    let trace = ClickTrace {
        step: step_no,
        x,
        y,
        source: source.to_string(),
        click_to_pixels_ms,
        readback_ms,
        frame_age_ms,
        uia_ms,
        password_scan_ms,
        skipped_newer,
        gdi_ms,
        png_ms: None,
        png_bytes: None,
        probe_latest,
        probe_gdi,
        target_name: target.as_ref().map(|t| t.name.clone()).unwrap_or_default(),
        target_type: target.as_ref().map(|t| t.control_type.clone()).unwrap_or_default(),
        window_title: target.as_ref().and_then(|t| t.window_title.clone()).unwrap_or_default(),
        process_name: target.as_ref().map(|t| t.process_name.clone()).unwrap_or_default(),
        is_password: target.as_ref().map(|t| t.is_password).unwrap_or(false),
        redacted_rects: screen_rects,
    };

    rec.blocks.push(Block::Step(StepBlock {
        id: format!("step-{step_no:03}"),
        action: Action::Click { button, x, y },
        title: describe_click(button, x, y, target.as_ref(), false),
        description: String::new(),
        screenshot: Some(rel),
        target,
        redactions,
        screenshot_withheld: false,
        captured_unix_ms: clock::unix_ms(),
    }));
    Ok(trace)
}

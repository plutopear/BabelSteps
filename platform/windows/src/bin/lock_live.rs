//! Live lock and sleep test (PSR-10), driven by a person.
//!
//! Records the real desktop with the real recorder while the person clicks,
//! locks the PC (Win+L), unlocks, clicks, sleeps, wakes and clicks. The recorder
//! hears the real Windows lock and power messages, which the self-test cannot
//! prove. Nothing from the screen is printed: only times, counts and flags.
//! The recording (full-screen pictures) is deleted when the test ends.
//!
//! Usage: `lock-live <output-folder> <stop-file>`. Stops when the stop file
//! exists (or after 30 minutes). Prints a timeline and a summary.

use anyhow::Result;
use babelsteps_core::storage::Bundle;
use babelsteps_core::{Action, Block};
use babelsteps_windows::testing;
use babelsteps_windows::{Recorder, RecorderConfig};
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "lock-live-out".into()));
    let stop_file = PathBuf::from(args.next().unwrap_or_else(|| "lock-live.stop".into()));
    std::fs::create_dir_all(&out)?;
    testing::make_thread_dpi_aware();

    let bundle = Bundle::create(&out, "lock-live", "lock-live", 0)?;
    let rec = bundle.load()?;
    let r = Recorder::start(bundle, rec, RecorderConfig::default())?;
    let begun = Instant::now();
    let begun_unix_ms = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    println!("recording started; stop file: {}", stop_file.display());

    let (mut last_away, mut last_steps, mut last_rebuilds) = (false, 0usize, 0u64);
    while !stop_file.exists() && begun.elapsed() < Duration::from_secs(30 * 60) {
        sleep(Duration::from_millis(250));
        let st = r.status();
        if st.away != last_away || st.steps != last_steps || st.capture.rebuilds != last_rebuilds {
            println!(
                "[+{:6.1}s] away={} steps={} capture_rebuilds={} away_periods={}",
                begun.elapsed().as_secs_f64(),
                st.away,
                st.steps,
                st.capture.rebuilds,
                st.away_periods
            );
            (last_away, last_steps, last_rebuilds) = (st.away, st.steps, st.capture.rebuilds);
        }
    }

    let (bundle, report) = r.stop()?;
    println!("--- steps (time since start, kind, picture, verified as before-the-click) ---");
    let mut n = 0;
    for b in &report.recording.blocks {
        if let Block::Step(s) = b {
            n += 1;
            let kind = if matches!(s.action, Action::Click { .. }) { "click" } else { "other" };
            let t = s.captured_unix_ms.saturating_sub(begun_unix_ms) as f64 / 1000.0;
            println!("step {n}: +{t:6.1}s {kind} picture={} withheld={}", s.screenshot.is_some(), s.screenshot_withheld);
        }
    }
    println!("--- summary ---");
    println!(
        "steps={} away_periods={} capture_rebuilds={} not_verified={} withheld={} no_screenshot={} missed_inputs={} hook_rehooks={}",
        report.status.steps,
        report.status.away_periods,
        report.status.capture.rebuilds,
        report.status.not_verified,
        report.status.withheld,
        report.status.no_screenshot,
        report.missed_inputs,
        report.hooks.rehooks
    );
    println!("warnings: {:?}", report.status.warnings);
    // Full-screen pictures may show private things: delete them.
    let path = bundle.path().to_path_buf();
    drop(bundle);
    let _ = std::fs::remove_dir_all(&path);
    println!("recording deleted: {}", !path.exists());
    Ok(())
}

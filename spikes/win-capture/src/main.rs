//! M0 spike for Windows capture. Throwaway code: its job is to answer questions.
//!
//!   win-capture selftest [--out DIR]             Automated run on a private test window.
//!   win-capture record [--out DIR] [--seconds N] Records real clicks until Enter or N seconds.
//!   win-capture uia-probe [--out DIR]            UI Automation against real apps (no input sent).
//!   win-capture psr-baseline [--out DIR]         Drives Steps Recorder against the test window.
//!
//! Keystrokes are only counted here. Their text is not stored.

#![cfg(windows)]

mod clock;
mod gdi;
mod hooks;
mod imgops;
mod monitors;
mod probe;
mod process;
mod psr;
mod recorder;
mod selftest;
mod testwin;
mod uia;
mod wgc;

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

fn main() -> Result<()> {
    // All coordinates in this program are physical pixels.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("selftest");
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let stamp = clock::unix_ms();
    let out = value("--out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("recordings/{mode}-{stamp}")));

    match mode {
        "selftest" => {
            if !selftest::run(out)? {
                bail!("one or more self-test checks failed");
            }
        }
        "record" => {
            let seconds: Option<u64> = value("--seconds").and_then(|s| s.parse().ok());
            let rec = recorder::Recorder::start(recorder::Options {
                out_dir: out,
                title: "BabelSteps spike recording".to_string(),
                probe: None,
                only_injected: false,
            })?;
            match seconds {
                Some(n) => {
                    println!("Recording for {n} seconds. Screenshots are saved on each click.");
                    let t0 = Instant::now();
                    while t0.elapsed() < Duration::from_secs(n) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
                None => {
                    println!("Recording. Screenshots are saved on each click. Press Enter here to stop.");
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                }
            }
            let s = rec.stop()?;
            println!("{} steps, {} key presses counted.", s.steps, s.key_downs);
            println!("Output: {}", s.out_dir);
            for e in &s.errors {
                println!("Error: {e}");
            }
        }
        "uia-probe" => probe::run(out, args.iter().any(|a| a == "--edge-only"))?,
        "psr-baseline" => psr::run(out)?,
        // win-capture psr-images DIR  (probe and window rectangle match the test window)
        "psr-images" => {
            let dir = args.get(1).map(PathBuf::from).context("psr-images needs a folder")?;
            psr::inspect_images(dir, (228, 441), (200, 200, 520, 300))?
        }
        other => bail!("unknown mode {other:?}; use selftest, record, uia-probe, or psr-baseline"),
    }
    Ok(())
}

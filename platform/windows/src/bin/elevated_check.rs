//! Live check of password protection against an elevated (administrator) window (PSR-8).
//!
//! A normal process cannot read what is inside an elevated window with UI
//! Automation. The recorder must then withhold the screenshot, or black out the
//! field, never save it unprotected. This tool opens our own test window, which
//! has a password field, either as a normal window (the control) or as an elevated
//! one (the person approves a UAC prompt), records with the real recorder running
//! as a normal user, and checks every click the person makes in that window.
//! The person has to click, because Windows blocks synthetic input into an
//! elevated window from a normal process.
//!
//! Only classifications and counts are printed. The recording is deleted.
//!
//! Usage:
//!   `elevated-check run <work-folder> normal|elevated`   (the driver; run as a normal user)
//!   `elevated-check run <work-folder> both-elevated`     (the driver itself runs as administrator, started
//!                                                         elevated by the caller; its window is elevated too.
//!                                                         This tests the "run BabelSteps as administrator" option, Q31)
//!   `elevated-check window <info-file> <stop-file>`      (started by the driver)
//! The driver stops when `<work-folder>\stop.txt` exists.

use anyhow::{anyhow, bail, Result};
use babelsteps_core::storage::Bundle;
use babelsteps_core::{Action, Block};
use babelsteps_windows::elevation::is_elevated;
use babelsteps_windows::testing::{self, Frame};
use babelsteps_windows::testwin::TestWindow;
use babelsteps_windows::{Recorder, RecorderConfig};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

type Rect = (i32, i32, i32, i32);

/// What the recorder draws over a hidden field after the black-out.
const MARKER: [u8; 3] = [255, 0, 0];
const OUTLINE: [u8; 3] = [0, 200, 0];

fn ms(d: u64) {
    sleep(Duration::from_millis(d));
}

fn rect_text(r: Rect) -> String {
    format!("{} {} {} {}", r.0, r.1, r.2, r.3)
}

fn parse_rect(s: &str) -> Result<Rect> {
    let v: Vec<i32> = s.split_whitespace().map(str::parse).collect::<Result<_, _>>()?;
    match v.as_slice() {
        [a, b, c, d] => Ok((*a, *b, *c, *d)),
        _ => bail!("bad rectangle: {s}"),
    }
}

fn inside((x, y, w, h): Rect, px: i32, py: i32) -> bool {
    px >= x && px < x + w && py >= y && py < y + h
}

/// Every pixel of the rectangle (shrunk by one pixel) is black, or the recorder's own
/// click marker or element outline drawn over the black.
fn field_hidden(f: &Frame, (x, y, w, h): Rect) -> bool {
    (y + 1..y + h - 1)
        .all(|py| (x + 1..x + w - 1).all(|px| f.sample(px, py).is_some_and(|c| c.iter().all(|&v| v < 10) || c == MARKER || c == OUTLINE)))
}

/// The test window, until the stop file appears. Writes where its fields are.
fn run_window(info: &Path, stop: &Path) -> Result<()> {
    testing::make_thread_dpi_aware();
    let win = TestWindow::spawn()?;
    ms(800);
    let text = format!(
        "elevated {}\npass {}\nuser {}\nclient {}\n",
        is_elevated(),
        rect_text(win.pass_rect()?),
        rect_text(win.user_rect()?),
        rect_text(win.client_rect()?)
    );
    std::fs::write(info, text)?;
    let end = Instant::now() + Duration::from_secs(15 * 60);
    while !stop.exists() && Instant::now() < end {
        ms(300);
    }
    drop(win);
    Ok(())
}

struct Info {
    elevated: bool,
    pass: Rect,
    user: Rect,
    client: Rect,
}

fn read_info(path: &Path) -> Result<Info> {
    let text = std::fs::read_to_string(path)?;
    let line = |key: &str| -> Result<&str> {
        text.lines().find_map(|l| l.strip_prefix(key).map(str::trim)).ok_or_else(|| anyhow!("no '{key}' line in the info file"))
    };
    Ok(Info {
        elevated: line("elevated")? == "true",
        pass: parse_rect(line("pass")?)?,
        user: parse_rect(line("user")?)?,
        client: parse_rect(line("client")?)?,
    })
}

fn run_driver(work: &Path, mode: &str) -> Result<bool> {
    let both = mode == "both-elevated";
    if both && !is_elevated() {
        bail!("both-elevated needs a driver started as administrator");
    }
    if !both && is_elevated() {
        bail!("this driver is elevated; run it as a normal user, or the test proves nothing");
    }
    let window_elevated_wanted = mode != "normal";
    std::fs::create_dir_all(work)?;
    let (info_file, stop_file) = (work.join("window-info.txt"), work.join("stop.txt"));
    let _ = std::fs::remove_file(&info_file);
    let _ = std::fs::remove_file(&stop_file);
    testing::make_thread_dpi_aware();

    let bundle = Bundle::create(&work.join("rec"), "elevated-check", "elevated-check", 0)?;
    let rec = bundle.load()?;
    let r = Recorder::start(bundle, rec, RecorderConfig::default())?;
    ms(700);

    let exe = std::env::current_exe()?;
    let args = format!("window \"{}\" \"{}\"", info_file.display(), stop_file.display());
    let mut child = None;
    if mode == "elevated" {
        println!("asking Windows to open the test window as administrator (a UAC prompt should appear)");
        let code = unsafe { ShellExecuteW(None, w!("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(args.as_str()), None, SW_SHOWNORMAL) };
        if (code.0 as isize) <= 32 {
            let _ = r.stop();
            println!("RESULT: not run (the UAC prompt was declined or the window could not start, code {})", code.0 as isize);
            return Ok(false);
        }
    } else {
        child = Some(std::process::Command::new(&exe).arg("window").arg(&info_file).arg(&stop_file).spawn()?);
    }
    let end = Instant::now() + Duration::from_secs(90);
    while !info_file.exists() && Instant::now() < end {
        ms(300);
    }
    if !info_file.exists() {
        let _ = r.stop();
        println!("RESULT: not run (the test window did not appear within 90 s)");
        return Ok(false);
    }
    let info = read_info(&info_file)?;
    println!("test window is up; elevated={} (wanted {})", info.elevated, window_elevated_wanted);
    println!("READY: click in the password box, the user name box and the empty green area, 2 times each; then create the stop file");

    let end = Instant::now() + Duration::from_secs(10 * 60);
    while !stop_file.exists() && Instant::now() < end {
        ms(300);
    }
    ms(500);
    let (bundle, report) = r.stop()?;
    if let Some(mut c) = child {
        let _ = c.wait();
    }

    let (mut in_window, mut protected, mut leaked, mut unprotected_other) = (0, 0, 0, 0);
    let mut lines = Vec::new();
    for b in &report.recording.blocks {
        let Block::Step(s) = b else { continue };
        let Action::Click { x, y, .. } = s.action else { continue };
        if !inside(info.client, x, y) {
            continue;
        }
        in_window += 1;
        let place = if inside(info.pass, x, y) {
            "password box"
        } else if inside(info.user, x, y) {
            "user name box"
        } else {
            "window background"
        };
        let verdict = if s.screenshot_withheld {
            protected += 1;
            "withheld".to_string()
        } else if let Some(rel) = &s.screenshot {
            let mon = testing::monitors().into_iter().find(|m| m.contains(x, y)).ok_or_else(|| anyhow!("no monitor at the click"))?;
            let f = Frame::load(&bundle.path().join(rel), mon.x, mon.y)?;
            if field_hidden(&f, info.pass) {
                protected += 1;
                "picture saved, password box black".to_string()
            } else {
                leaked += 1;
                "PICTURE SAVED WITH THE PASSWORD BOX VISIBLE".to_string()
            }
        } else {
            unprotected_other += 1;
            "no picture".to_string()
        };
        lines.push(format!("click {in_window}: {place}: {verdict}"));
    }
    println!("--- clicks inside the test window ---");
    for l in &lines {
        println!("{l}");
    }
    println!("--- summary ---");
    println!(
        "mode={mode} window_elevated={} clicks_in_window={in_window} protected={protected} leaked={leaked} no_picture={unprotected_other}",
        info.elevated
    );
    println!(
        "recorder: steps={} withheld={} not_verified={} no_screenshot={} hook_rehooks={} warnings={:?}",
        report.status.steps,
        report.status.withheld,
        report.status.not_verified,
        report.status.no_screenshot,
        report.hooks.rehooks,
        report.status.warnings
    );
    let path = bundle.path().to_path_buf();
    drop(bundle);
    let _ = std::fs::remove_dir_all(work.join("rec"));
    println!("recording deleted: {}", !path.exists());
    let ok = info.elevated == window_elevated_wanted && in_window > 0 && leaked == 0;
    if in_window == 0 {
        println!("RESULT: INCONCLUSIVE (no click in the test window was recorded; if the window was elevated, the recorder may not see clicks there)");
    } else if leaked > 0 {
        println!("RESULT: FAIL (a saved picture shows the password box)");
    } else if info.elevated != window_elevated_wanted {
        println!("RESULT: INCONCLUSIVE (the window was not in the integrity level the mode asked for)");
    } else {
        println!("RESULT: PASS for this mode (every recorded click in the window was withheld or had the box black)");
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("window") if args.len() == 3 => run_window(&PathBuf::from(&args[1]), &PathBuf::from(&args[2])).map(|()| true),
        Some("run") if args.len() == 3 && (args[2] == "normal" || args[2] == "elevated" || args[2] == "both-elevated") => run_driver(&PathBuf::from(&args[1]), &args[2]),
        _ => {
            eprintln!("usage: elevated-check run <work-folder> normal|elevated|both-elevated");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

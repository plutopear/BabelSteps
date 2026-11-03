//! Spike 3, real applications: what does UI Automation report, and how fast?
//!
//! No input is sent to these apps: `ElementFromPoint` needs no click. Each app is
//! launched by this program, and only windows that appeared after launch are read or
//! closed, so anything the person already has open is left alone. The browser uses a
//! throwaway profile and a local test page.

use crate::clock;
use crate::testwin;
use crate::uia::{self, Uia};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows::core::BOOL;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Serialize)]
struct Round {
    points_on_window: usize,
    named: usize,
    lookup_p50_ms: f64,
    lookup_max_ms: f64,
    control_types: Vec<String>,
    sample_names: Vec<String>,
}

#[derive(Serialize)]
struct TargetReport {
    app: String,
    skipped: Option<String>,
    rounds: Vec<Round>,
    password_fields: usize,
    password_scan_ms: Vec<f64>,
    password_center_flagged: Option<bool>,
    password_center_sequence: Vec<Option<(String, String, bool)>>,
    /// Element and ancestors at the password field's center: (type, name, is_password).
    password_chain: Vec<(String, String, bool)>,
    /// Focused element after clicking the password field: (type, name, is_password).
    password_focus_after_click: Option<(String, String, bool)>,
    /// Why points were rejected, for diagnosing windows the scan could not read.
    diagnostics: Vec<String>,
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn describe(hwnd: HWND) -> String {
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut r);
    }
    let visible = unsafe { IsWindowVisible(hwnd).as_bool() };
    format!(
        "hwnd {:?} class {:?} title {:?} rect ({}, {}, {}, {}) visible {visible}",
        hwnd.0,
        class_of(hwnd),
        title_of(hwnd),
        r.left,
        r.top,
        r.right,
        r.bottom
    )
}

unsafe extern "system" fn collect_windows(hwnd: HWND, data: LPARAM) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<isize>);
    if IsWindowVisible(hwnd).as_bool() {
        out.push(hwnd.0 as isize);
    }
    BOOL(1)
}

fn title_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn matching_windows(title_part: &str) -> Vec<isize> {
    let mut all: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_windows), LPARAM(&mut all as *mut _ as isize));
    }
    all.into_iter()
        .filter(|h| title_of(HWND(*h as *mut _)).contains(title_part))
        .collect()
}

/// Launch a command and wait for a new window whose title contains `title_part`.
fn launch(cmd: &mut Command, title_part: &str) -> Result<Option<HWND>, String> {
    let before = matching_windows(title_part);
    cmd.spawn().map_err(|e| e.to_string())?;
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(20) {
        if let Some(h) = matching_windows(title_part).into_iter().find(|h| !before.contains(h)) {
            return Ok(Some(HWND(h as *mut _)));
        }
        sleep(Duration::from_millis(100));
    }
    Ok(None)
}

fn scan(u: &Uia, hwnd: HWND, app: &str) -> Result<TargetReport> {
    unsafe {
        // On top without taking keyboard focus, so the scan sees this window.
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    sleep(Duration::from_millis(1500));

    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect)? };
    let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);

    let mut diagnostics = vec![format!("selected: {}", describe(hwnd))];
    let (mid_x, mid_y) = (rect.left + w / 2, rect.top + h / 2);
    let hit = unsafe { WindowFromPoint(POINT { x: mid_x, y: mid_y }) };
    diagnostics.push(format!("hit at center: {}", describe(hit)));
    if let Some(root) = uia::root_at(mid_x, mid_y) {
        diagnostics.push(format!("root at center: {}", describe(root)));
    }

    let mut rounds = Vec::new();
    // Chromium builds its accessibility tree on demand, so later rounds may differ.
    for round in 0..3 {
        if round > 0 {
            sleep(Duration::from_millis(1500));
        }
        let (mut times, mut named, mut points) = (Vec::new(), 0usize, 0usize);
        let (mut types, mut names) = (BTreeSet::new(), BTreeSet::new());
        for gy in 1..=12 {
            for gx in 1..=16 {
                let (x, y) = (rect.left + w * gx / 17, rect.top + h * gy / 13);
                if uia::root_at(x, y).map(|r| r.0) != Some(hwnd.0) {
                    continue;
                }
                points += 1;
                let t0 = clock::now();
                if let Ok(t) = u.target_at(x, y) {
                    times.push(clock::to_ms(clock::now() - t0));
                    if !t.name.trim().is_empty() {
                        named += 1;
                        if names.len() < 10 {
                            names.insert(t.name.chars().take(32).collect::<String>());
                        }
                    }
                    types.insert(t.control_type);
                }
            }
        }
        times.sort_by(|a, b| a.total_cmp(b));
        rounds.push(Round {
            points_on_window: points,
            named,
            lookup_p50_ms: times.get(times.len() / 2).copied().unwrap_or(0.0),
            lookup_max_ms: times.last().copied().unwrap_or(0.0),
            control_types: types.into_iter().collect(),
            sample_names: names.into_iter().collect(),
        });
    }

    let mut password_scan_ms = Vec::new();
    let mut found = Vec::new();
    for _ in 0..5 {
        let t0 = clock::now();
        found = u.password_rects(hwnd).unwrap_or_default();
        password_scan_ms.push(clock::to_ms(clock::now() - t0));
    }
    let center = found.first().map(|r| (r.x + r.w / 2, r.y + r.h / 2));
    let password_center_flagged =
        center.map(|(x, y)| u.target_at(x, y).map(|t| t.is_password).unwrap_or(false));
    // Repeated reads of the same point: is a wrong answer only a first-read effect?
    // Each entry is (type, name, is_password), or None if the lookup itself failed.
    let mut password_center_sequence = Vec::new();
    if let Some((x, y)) = center {
        for _ in 0..8 {
            password_center_sequence.push(
                u.target_at(x, y).ok().map(|t| (t.control_type, t.name, t.is_password)),
            );
            sleep(Duration::from_millis(50));
        }
    }
    let password_chain = center.and_then(|(x, y)| u.chain_at(x, y, 6).ok()).unwrap_or_default();

    // Does keyboard focus report the field correctly? Clicks only on our test page,
    // and only when that window is what is under the point. No text is typed.
    let mut password_focus_after_click = None;
    if let Some((x, y)) = center {
        if uia::root_at(x, y).map(|r| r.0) == Some(hwnd.0) {
            testwin::click(x, y);
            sleep(Duration::from_millis(500));
            password_focus_after_click = u.focused().ok();
        }
    }

    Ok(TargetReport {
        app: app.to_string(),
        skipped: None,
        rounds,
        password_fields: found.len(),
        password_scan_ms,
        password_center_flagged,
        password_center_sequence,
        password_chain,
        password_focus_after_click,
        diagnostics,
    })
}

fn close(hwnd: HWND) {
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

fn test_page(rows: usize) -> String {
    let mut s = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>BabelSteps UIA Test</title></head><body>\
         <h1>Sign in</h1><form><label>User name <input id=\"u\" type=\"text\"></label><br><br>\
         <label>Password <input id=\"p\" type=\"password\"></label><br><br>\
         <button type=\"button\">Save</button> <a href=\"#x\">Help link</a></form><table>",
    );
    for i in 0..rows {
        s.push_str(&format!("<tr><td>Row {i}</td><td><button type=\"button\">Action {i}</button></td></tr>"));
    }
    s.push_str("</table></body></html>");
    s
}

fn edge_path() -> Option<PathBuf> {
    [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}

fn skipped_report(app: &str, why: String) -> TargetReport {
    TargetReport {
        app: app.to_string(),
        skipped: Some(why),
        rounds: vec![],
        password_fields: 0,
        password_scan_ms: vec![],
        password_center_flagged: None,
        password_center_sequence: vec![],
        password_chain: vec![],
        password_focus_after_click: None,
        diagnostics: vec![],
    }
}

fn run_edge(u: &Uia, out_dir: &Path, name: &str, rows: usize) -> Result<TargetReport> {
    let skipped = |why: String| skipped_report(name, why);
    let Some(edge) = edge_path() else {
        return Ok(skipped("Edge not found".to_string()));
    };
    let page = out_dir.join(format!("{name}.html"));
    std::fs::write(&page, test_page(rows))?;
    let abs = std::fs::canonicalize(&page)?;
    let url = format!("file:///{}", abs.display().to_string().trim_start_matches(r"\\?\").replace('\\', "/"));
    let profile = std::env::temp_dir().join(format!("babelsteps-edge-{}", clock::unix_ms()));
    let mut cmd = Command::new(edge);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--disable-sync"])
        .arg(format!("--app={url}"))
        .args(["--window-position=150,150", "--window-size=1000,700"]);
    let report = match launch(&mut cmd, "BabelSteps UIA Test") {
        Ok(Some(hwnd)) => {
            let r = scan(u, hwnd, name);
            close(hwnd);
            r?
        }
        Ok(None) => skipped("window did not appear".to_string()),
        Err(e) => skipped(e),
    };
    sleep(Duration::from_secs(2));
    let _ = std::fs::remove_dir_all(&profile);
    Ok(report)
}

fn run_app(u: &Uia, exe: &str, title: &str, name: &str) -> Result<TargetReport> {
    let skipped = |why: String| skipped_report(name, why);
    if !matching_windows(title).is_empty() {
        return Ok(skipped("already open; left alone".to_string()));
    }
    Ok(match launch(&mut Command::new(exe), title) {
        Ok(Some(hwnd)) => {
            let r = scan(u, hwnd, name);
            close(hwnd);
            r?
        }
        Ok(None) => skipped("window did not appear".to_string()),
        Err(e) => skipped(e),
    })
}

pub fn run(out_dir: PathBuf, edge_only: bool) -> Result<()> {
    std::fs::create_dir_all(&out_dir)?;
    let u = Uia::new()?;
    let mut reports = Vec::new();
    if !edge_only {
        reports.push(run_app(&u, "charmap.exe", "Character Map", "Character Map (classic Win32)")?);
        reports.push(run_app(&u, "calc.exe", "Calculator", "Calculator (modern XAML)")?);
    }
    reports.push(run_edge(&u, &out_dir, "edge-small-page", 0)?);
    reports.push(run_edge(&u, &out_dir, "edge-large-page", 5000)?);
    std::fs::write(out_dir.join("uia-probe.json"), serde_json::to_vec_pretty(&reports)?)?;
    for r in &reports {
        println!("\n{}", r.app);
        if let Some(why) = &r.skipped {
            println!("  skipped: {why}");
            continue;
        }
        for (i, round) in r.rounds.iter().enumerate() {
            println!(
                "  round {}: {} points, {} named, lookup p50 {:.1} ms max {:.1} ms, types {:?}",
                i + 1,
                round.points_on_window,
                round.named,
                round.lookup_p50_ms,
                round.lookup_max_ms,
                round.control_types
            );
            println!("    names: {:?}", round.sample_names);
        }
        println!(
            "  password fields: {}, flagged at center: {:?}, scan ms: {:?}",
            r.password_fields,
            r.password_center_flagged,
            r.password_scan_ms.iter().map(|m| (m * 10.0).round() / 10.0).collect::<Vec<_>>()
        );
        if !r.password_chain.is_empty() {
            println!("  repeated reads at password center: {:?}", r.password_center_sequence);
            println!("  element chain at password center: {:?}", r.password_chain);
            println!("  focused after click: {:?}", r.password_focus_after_click);
        }
        for d in &r.diagnostics {
            println!("  {d}");
        }
    }
    Ok(())
}

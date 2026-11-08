//! Export options (PSR-2, PSR-5, PSR-6; Q25, Q30) and what they add to a
//! document: the time of each step, and a technical-details section with the
//! step counts. Both are off by default. Exporters render what this module
//! works out, so Markdown, HTML and PDF say the same thing.
//!
//! Privacy: the technical details never include user or machine names (Q25).
//! Window titles are left out because they can show file paths, user names and
//! page titles; only the program name and the UI element are listed.

use crate::model::{Action, Block, Recording, StepBlock};

/// What an export includes beyond the document itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// PSR-2: show the local clock time of each step, with the date once at the top.
    pub step_times: bool,
    /// PSR-6: add a technical-details section at the end, with the step counts (PSR-5).
    pub technical_details: bool,
    /// Minutes east of UTC used for clock times. The app passes the offset in
    /// force when the recording was made; the core has no time-zone data.
    pub utc_offset_minutes: i32,
    /// Operating system for the technical details, such as "Windows 11 (build
    /// 22631)". Never a user or machine name. Empty leaves the line out.
    pub os: String,
    /// The program that made the export, such as "BabelSteps 0.1.0". Empty leaves the line out.
    pub app: String,
}

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

/// Local date and time parts: (year, month 1-12, day, hour, minute, second).
fn civil(unix_ms: u64, offset_minutes: i32) -> (i64, usize, u32, u32, u32, u32) {
    let secs = (unix_ms / 1000) as i64 + i64::from(offset_minutes) * 60;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    // Days since 1970-01-01 to a proleptic Gregorian date (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as usize;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day, (sod / 3600) as u32, (sod % 3600 / 60) as u32, (sod % 60) as u32)
}

/// "8 November 2026" in local time.
pub fn local_date(unix_ms: u64, offset_minutes: i32) -> String {
    let (y, m, d, ..) = civil(unix_ms, offset_minutes);
    format!("{d} {} {y}", MONTHS[m - 1])
}

/// "14:05:09" in local time (24-hour clock).
pub fn local_time(unix_ms: u64, offset_minutes: i32) -> String {
    let (.., h, mi, s) = civil(unix_ms, offset_minutes);
    format!("{h:02}:{mi:02}:{s:02}")
}

/// Steps with a real capture time. Imported and older steps carry 0.
fn timed_steps(rec: &Recording) -> impl Iterator<Item = &StepBlock> {
    rec.blocks.iter().filter_map(|b| match b {
        Block::Step(s) if s.captured_unix_ms > 0 => Some(s),
        _ => None,
    })
}

/// The date shown once at the top when step times are on: the first timed step's.
fn first_day(rec: &Recording, opts: &ExportOptions) -> Option<String> {
    timed_steps(rec).next().map(|s| local_date(s.captured_unix_ms, opts.utc_offset_minutes))
}

/// "Recorded on 8 November 2026." when step times are on and any step has a time.
pub fn recorded_on_line(rec: &Recording, opts: &ExportOptions) -> Option<String> {
    if !opts.step_times {
        return None;
    }
    first_day(rec, opts).map(|d| format!("Recorded on {d}."))
}

/// The time shown under a step heading, such as "14:05:09". A step on a later
/// day than the date at the top also carries its date ("9 November 2026, 00:01:02").
pub fn step_time(rec: &Recording, step: &StepBlock, opts: &ExportOptions) -> Option<String> {
    if !opts.step_times || step.captured_unix_ms == 0 {
        return None;
    }
    let time = local_time(step.captured_unix_ms, opts.utc_offset_minutes);
    let day = local_date(step.captured_unix_ms, opts.utc_offset_minutes);
    Some(if first_day(rec, opts).as_deref() == Some(day.as_str()) { time } else { format!("{day}, {time}") })
}

/// Prefix of the step-time line in Markdown exports: `*Time: 14:05:09*`.
pub(crate) const TIME_PREFIX: &str = "Time: ";

/// Is this Markdown line the step-time line written under a step heading?
/// Markdown import drops it so it does not become the step's description.
pub(crate) fn is_step_time_line(line: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^\*Time: (?:\d{1,2} [A-Z][a-z]+ \d{4}, )?\d{2}:\d{2}:\d{2}\*$").expect("valid pattern")
    });
    re.is_match(line)
}

/// Step counts (PSR-5), worked out from the document so they match what is exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StepCounts {
    pub steps: usize,
    /// Screenshot withheld because the password check failed or timed out.
    pub withheld: usize,
    /// Click and keyboard steps with no screenshot that were not withheld: no
    /// frame was available, or the screenshot was removed later. Notes and
    /// steps added by hand are not counted.
    pub no_screenshot: usize,
}

pub fn step_counts(rec: &Recording) -> StepCounts {
    let mut c = StepCounts::default();
    for b in &rec.blocks {
        if let Block::Step(s) = b {
            c.steps += 1;
            if s.screenshot_withheld {
                c.withheld += 1;
            } else if s.screenshot.is_none() && !matches!(s.action, Action::Comment | Action::Manual) {
                c.no_screenshot += 1;
            }
        }
    }
    c
}

/// The technical-details section (PSR-6), ready for an exporter to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalDetails {
    /// Label and value pairs, in display order.
    pub facts: Vec<(&'static str, String)>,
    /// Per step: number, program and UI element. Steps with no element are left
    /// out; a step with a withheld screenshot shows its program only.
    pub steps: Vec<(usize, String)>,
}

pub const DETAILS_HEADING: &str = "Technical details";
pub const DETAILS_STEPS_HEADING: &str = "Programs and UI elements";

/// `None` unless the option is on.
pub fn technical_details(rec: &Recording, opts: &ExportOptions) -> Option<TechnicalDetails> {
    if !opts.technical_details {
        return None;
    }
    let off = opts.utc_offset_minutes;
    let mut facts = Vec::new();
    let mut times = timed_steps(rec).map(|s| s.captured_unix_ms);
    if let Some(first) = times.next() {
        let last = times.last().unwrap_or(first);
        let start = format!("{}, {}", local_date(first, off), local_time(first, off));
        let end = if local_date(first, off) == local_date(last, off) {
            local_time(last, off)
        } else {
            format!("{}, {}", local_date(last, off), local_time(last, off))
        };
        facts.push(("Recording session", format!("{start} to {end}")));
    }
    let c = step_counts(rec);
    facts.push(("Recorded steps", c.steps.to_string()));
    facts.push((
        "Screenshots withheld",
        format!("{} (the password check failed or could not finish)", c.withheld),
    ));
    facts.push(("Steps without a screenshot", c.no_screenshot.to_string()));
    if !opts.os.trim().is_empty() {
        facts.push(("Operating system", opts.os.trim().to_string()));
    }
    if !opts.app.trim().is_empty() {
        facts.push(("Exported with", opts.app.trim().to_string()));
    }

    let mut steps = Vec::new();
    let mut n = 0usize;
    for b in &rec.blocks {
        let Block::Step(s) = b else { continue };
        n += 1;
        let Some(t) = &s.target else { continue };
        let program = t.process_name.trim();
        let kind = t.control_type.trim();
        // A step whose password check failed lists its program only.
        let name = if s.screenshot_withheld { "" } else { t.name.trim() };
        let kind = if s.screenshot_withheld { "" } else { kind };
        let element = match (kind.is_empty(), name.is_empty()) {
            (_, true) => kind.to_string(),
            (true, false) => format!("\u{201c}{name}\u{201d}"),
            (false, false) => format!("{kind} \u{201c}{name}\u{201d}"),
        };
        let line = match (program.is_empty(), element.is_empty()) {
            (true, true) => continue,
            (false, true) => program.to_string(),
            (true, false) => element,
            (false, false) => format!("{program}, {element}"),
        };
        steps.push((n, line));
    }
    Some(TechnicalDetails { facts, steps })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    // 2026-11-08 14:05:09 UTC
    const T: u64 = 1_794_146_709_000;

    fn step(ms: u64, action: Action, shot: Option<&str>, withheld: bool, target: Option<Target>) -> Block {
        Block::Step(StepBlock {
            id: "s".into(),
            action,
            title: "t".into(),
            description: String::new(),
            screenshot: shot.map(String::from),
            target,
            redactions: vec![],
            captured_unix_ms: ms,
            screenshot_withheld: withheld,
        })
    }

    #[test]
    fn dates_and_times() {
        assert_eq!(local_date(0, 0), "1 January 1970");
        assert_eq!(local_time(0, 0), "00:00:00");
        assert_eq!(local_date(T, 0), "8 November 2026");
        assert_eq!(local_time(T, 0), "14:05:09");
        // East and west of UTC, across midnight.
        assert_eq!(local_time(T, 600), "00:05:09");
        assert_eq!(local_date(T, 600), "9 November 2026");
        assert_eq!(local_time(T, -15 * 60), "23:05:09");
        assert_eq!(local_date(T, -15 * 60), "7 November 2026");
        // Leap day.
        assert_eq!(local_date(1_709_164_800_000, 0), "29 February 2024");
        assert_eq!(local_date(951_782_400_000, 0), "29 February 2000");
    }

    #[test]
    fn times_are_off_by_default_and_on_when_asked() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(step(T, Action::Manual, None, false, None));
        rec.blocks.push(step(T + 86_400_000, Action::Manual, None, false, None));
        rec.blocks.push(step(0, Action::Manual, None, false, None));
        let Block::Step(s0) = &rec.blocks[0] else { unreachable!() };
        let Block::Step(s1) = &rec.blocks[1] else { unreachable!() };
        let Block::Step(s2) = &rec.blocks[2] else { unreachable!() };
        let off = ExportOptions::default();
        assert_eq!(recorded_on_line(&rec, &off), None);
        assert_eq!(step_time(&rec, s0, &off), None);
        let on = ExportOptions { step_times: true, ..Default::default() };
        assert_eq!(recorded_on_line(&rec, &on).as_deref(), Some("Recorded on 8 November 2026."));
        assert_eq!(step_time(&rec, s0, &on).as_deref(), Some("14:05:09"));
        assert_eq!(step_time(&rec, s1, &on).as_deref(), Some("9 November 2026, 14:05:09"));
        assert_eq!(step_time(&rec, s2, &on), None, "a step with no capture time gets no time");
    }

    #[test]
    fn time_line_pattern() {
        assert!(is_step_time_line("*Time: 14:05:09*"));
        assert!(is_step_time_line("*Time: 9 November 2026, 00:01:02*"));
        for no in ["Time: 14:05:09", "*Time: 14:05*", "*Time: now*", "*Time: 14:05:09* extra", "*time: 14:05:09*"] {
            assert!(!is_step_time_line(no), "{no}");
        }
    }

    #[test]
    fn counts_and_details() {
        let target = |p: &str, k: &str, n: &str| {
            Some(Target {
                name: n.into(),
                control_type: k.into(),
                window_title: Some("C:\\Users\\someone\\secret.txt - Notepad".into()),
                process_name: p.into(),
                is_password: false,
                bounds: None,
            })
        };
        let mut rec = Recording::new("r", "t", 0);
        let click = || Action::Click { button: MouseButton::Left, x: 0, y: 0 };
        rec.blocks.push(step(T, click(), Some("images/a.png"), false, target("notepad.exe", "Button", "Save")));
        rec.blocks.push(step(T + 1000, click(), None, true, target("msedge.exe", "Edit", "Password")));
        rec.blocks.push(step(T + 2000, click(), None, false, None));
        rec.blocks.push(step(T + 3000, Action::Comment, None, false, None));
        rec.blocks.push(Block::Text { id: "x".into(), text: "hi".into() });
        assert_eq!(step_counts(&rec), StepCounts { steps: 4, withheld: 1, no_screenshot: 1 });

        assert_eq!(technical_details(&rec, &ExportOptions::default()), None);
        let opts = ExportOptions {
            technical_details: true,
            os: "Windows 11 (build 22631)".into(),
            app: "BabelSteps 0.1.0".into(),
            ..Default::default()
        };
        let d = technical_details(&rec, &opts).unwrap();
        assert_eq!(
            d.facts,
            vec![
                ("Recording session", "8 November 2026, 14:05:09 to 14:05:12".to_string()),
                ("Recorded steps", "4".to_string()),
                ("Screenshots withheld", "1 (the password check failed or could not finish)".to_string()),
                ("Steps without a screenshot", "1".to_string()),
                ("Operating system", "Windows 11 (build 22631)".to_string()),
                ("Exported with", "BabelSteps 0.1.0".to_string()),
            ]
        );
        assert_eq!(
            d.steps,
            vec![
                (1, "notepad.exe, Button \u{201c}Save\u{201d}".to_string()),
                (2, "msedge.exe".to_string()),
            ]
        );
        let all = format!("{d:?}");
        assert!(!all.contains("someone") && !all.contains("secret"), "window titles must never appear");
    }

    #[test]
    fn empty_os_and_app_lines_are_left_out() {
        let rec = Recording::new("r", "t", 0);
        let d = technical_details(&rec, &ExportOptions { technical_details: true, ..Default::default() }).unwrap();
        let labels: Vec<&str> = d.facts.iter().map(|(l, _)| *l).collect();
        assert_eq!(labels, ["Recorded steps", "Screenshots withheld", "Steps without a screenshot"]);
        assert!(d.steps.is_empty());
    }
}

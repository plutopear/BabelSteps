//! App settings (A8 screenshot retention, and later hotkeys): plain data, a
//! JSON file, no platform code. A missing or damaged file gives the defaults;
//! the app must start whatever is in it.

use crate::pii::CustomRules;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Q12: unlimited by default, with a warning above this many megabytes.
pub const DEFAULT_SIZE_WARNING_MB: u32 = 500;
pub const MAX_KEEP_SCREENSHOTS: u32 = 100_000;
/// B14: most custom words, and most custom patterns, the user can keep.
pub const MAX_CUSTOM_ENTRIES: usize = 100;
/// B14: longest custom word or pattern, in characters.
pub const MAX_CUSTOM_CHARS: usize = 200;

/// A global shortcut as the user types it, e.g. `Ctrl+Alt+Shift+R`. Keys: A to Z, 0 to 9, F1 to F12.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

impl HotkeySpec {
    pub fn parse(text: &str) -> Result<HotkeySpec, String> {
        let mut spec = HotkeySpec { ctrl: false, alt: false, shift: false, win: false, key: String::new() };
        for part in text.split('+').map(str::trim) {
            let up = part.to_ascii_uppercase();
            let flag = match up.as_str() {
                "CTRL" | "CONTROL" => Some(&mut spec.ctrl),
                "ALT" => Some(&mut spec.alt),
                "SHIFT" => Some(&mut spec.shift),
                "WIN" | "WINDOWS" => Some(&mut spec.win),
                _ => None,
            };
            if let Some(f) = flag {
                if *f {
                    return Err(format!("\"{text}\" names the same modifier twice"));
                }
                *f = true;
                continue;
            }
            let ok_key = (up.len() == 1 && up.chars().all(|c| c.is_ascii_alphanumeric()))
                || (up.len() >= 2 && up.starts_with('F') && up[1..].parse::<u8>().is_ok_and(|n| (1..=12).contains(&n)));
            if !ok_key || !spec.key.is_empty() {
                return Err(format!("\"{text}\" is not a shortcut. Use modifiers and one key, like Ctrl+Alt+Shift+R (keys: A to Z, 0 to 9, F1 to F12)"));
            }
            spec.key = up;
        }
        if spec.key.is_empty() {
            return Err(format!("\"{text}\" has no key. Use modifiers and one key, like Ctrl+Alt+Shift+R"));
        }
        if !(spec.ctrl || spec.alt || spec.win) {
            return Err(format!("\"{text}\" needs Ctrl, Alt or Windows as well, or it would block typing"));
        }
        Ok(spec)
    }

    /// Canonical text: `Ctrl+Alt+Shift+Win+Key`.
    pub fn label(&self) -> String {
        let mut out = String::new();
        for (on, name) in [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift"), (self.win, "Win")] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out + &self.key
    }

    /// Fewer than three modifiers: likely to clash with browsers and other apps (Q14).
    pub fn is_weak(&self) -> bool {
        [self.ctrl, self.alt, self.shift, self.win].iter().filter(|b| **b).count() < 3
    }
}

/// The four global shortcuts (C3), as text. Defaults are Q14's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HotkeySettings {
    pub start_stop: String,
    pub pause: String,
    pub note: String,
    /// The panic stop: always stops and saves.
    pub stop: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        HotkeySettings {
            start_stop: "Ctrl+Alt+Shift+R".into(),
            pause: "Ctrl+Alt+Shift+P".into(),
            note: "Ctrl+Alt+Shift+N".into(),
            stop: "Ctrl+Alt+Shift+X".into(),
        }
    }
}

impl HotkeySettings {
    pub const ACTIONS: [&'static str; 4] = ["Start or stop recording", "Pause or resume", "Add a note", "Stop now"];

    /// Parse all four, in action order. Rejects bad text and two actions on one key.
    pub fn parse(&self) -> Result<[HotkeySpec; 4], String> {
        let texts = [&self.start_stop, &self.pause, &self.note, &self.stop];
        let mut out: Vec<HotkeySpec> = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            let spec = HotkeySpec::parse(t).map_err(|e| format!("{}: {e}", Self::ACTIONS[i]))?;
            if let Some(j) = out.iter().position(|o| *o == spec) {
                return Err(format!("{} and {} both use {}. Give each action its own shortcut", Self::ACTIONS[j], Self::ACTIONS[i], spec.label()));
            }
            out.push(spec);
        }
        Ok(out.try_into().expect("four shortcuts"))
    }

    /// Same shortcuts written canonically (assumes `parse` succeeded).
    pub fn normalized(&self) -> Result<HotkeySettings, String> {
        let [a, b, c, d] = self.parse()?;
        Ok(HotkeySettings { start_stop: a.label(), pause: b.label(), note: c.label(), stop: d.label() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Keep screenshots for the newest N steps when a recording stops.
    /// `None` keeps them all (the default).
    pub keep_screenshots: Option<u32>,
    /// Warn after a recording when its screenshots take more than this many MB
    /// and no limit is set.
    pub size_warning_mb: u32,
    /// C2: a coloured frame around each screen while recording. The tray icon
    /// always shows the state; this only hides the frame.
    pub show_capture_border: bool,
    pub hotkeys: HotkeySettings,
    /// Closing the window keeps BabelSteps running in the tray, so the shortcuts
    /// still work. Off by default: closing quits, as Steps Recorder does.
    pub close_to_tray: bool,
    /// A3: name the full window title in click steps, as Steps Recorder does.
    /// Off by default, because titles often hold file names, page titles and
    /// customer names (privacy doc section 1); steps name the app instead.
    pub show_window_titles: bool,
    /// B14: words hidden from typed text, matched whole and ignoring case.
    /// The user's own text: never log it or put it in an error message.
    pub custom_words: Vec<String>,
    /// B14: regular expressions hidden from typed text (advanced). Same care.
    pub custom_patterns: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            keep_screenshots: None,
            size_warning_mb: DEFAULT_SIZE_WARNING_MB,
            show_capture_border: true,
            hotkeys: HotkeySettings::default(),
            close_to_tray: false,
            show_window_titles: false,
            custom_words: Vec::new(),
            custom_patterns: Vec::new(),
        }
    }
}

/// Why one custom word or pattern is refused, or `None` if it is fine. The
/// entry is the user's own text, so the reason never quotes it.
fn custom_entry_problem(entry: &str, word: bool) -> Option<String> {
    if entry.trim().is_empty() {
        return Some("is empty".into());
    }
    if entry.chars().count() > MAX_CUSTOM_CHARS {
        return Some(format!("is longer than {MAX_CUSTOM_CHARS} characters"));
    }
    let built = if word {
        CustomRules::new([entry], [] as [&str; 0])
    } else {
        CustomRules::new([] as [&str; 0], [entry])
    };
    built.err().map(|_| "is not valid".into())
}

/// Drops the entries `validate` would refuse, and any past the cap.
fn keep_good_entries(list: Vec<String>, word: bool) -> Vec<String> {
    let mut list: Vec<String> = list
        .into_iter()
        .filter(|e| custom_entry_problem(e, word).is_none())
        .collect();
    list.truncate(MAX_CUSTOM_ENTRIES);
    list
}

impl Settings {
    /// Reject values the UI should never send. Returns a plain-language reason.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(n) = self.keep_screenshots {
            if n == 0 || n > MAX_KEEP_SCREENSHOTS {
                return Err(format!("keep between 1 and {MAX_KEEP_SCREENSHOTS} screenshots"));
            }
        }
        if self.size_warning_mb == 0 || self.size_warning_mb > 1_000_000 {
            return Err("the size warning must be between 1 MB and 1,000,000 MB".into());
        }
        self.hotkeys.parse()?;
        for (list, word) in [(&self.custom_words, true), (&self.custom_patterns, false)] {
            let noun = if word { "word" } else { "pattern" };
            if list.len() > MAX_CUSTOM_ENTRIES {
                return Err(format!(
                    "too many custom {noun}s (at most {MAX_CUSTOM_ENTRIES})"
                ));
            }
            for (i, entry) in list.iter().enumerate() {
                if let Some(problem) = custom_entry_problem(entry, word) {
                    return Err(format!("custom {noun} {} {problem}", i + 1));
                }
            }
        }
        Ok(())
    }

    /// B14: the custom words and patterns, ready to check typed text against.
    /// Fails on anything `validate` refuses, because a rule that is skipped
    /// would leave the user believing it protects them.
    pub fn custom_rules(&self) -> Result<CustomRules, String> {
        self.validate()?;
        CustomRules::new(&self.custom_words, &self.custom_patterns).map_err(|e| e.to_string())
    }

    /// Read the file. Missing, unreadable or invalid content gives the defaults,
    /// and values that fail `validate` are replaced field by field.
    pub fn load(path: &Path) -> Settings {
        let mut s: Settings =
            fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if s.validate().is_err() {
            let d = Settings::default();
            if s.keep_screenshots.is_some_and(|n| n == 0 || n > MAX_KEEP_SCREENSHOTS) {
                s.keep_screenshots = d.keep_screenshots;
            }
            if s.size_warning_mb == 0 || s.size_warning_mb > 1_000_000 {
                s.size_warning_mb = d.size_warning_mb;
            }
            if s.hotkeys.parse().is_err() {
                s.hotkeys = d.hotkeys;
            }
            // Keep every custom entry that is still good, so one bad line does
            // not switch off the rest.
            s.custom_words = keep_good_entries(s.custom_words, true);
            s.custom_patterns = keep_good_entries(s.custom_patterns, false);
        }
        s
    }

    /// Write via a temp file and rename, so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.partial");
        let result = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(&serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bs-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn defaults_keep_everything() {
        let s = Settings::default();
        assert_eq!(s.keep_screenshots, None);
        assert!(s.show_capture_border);
        assert!(!s.close_to_tray);
        assert!(!s.show_window_titles);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn round_trips_and_missing_file_gives_defaults() {
        let p = temp("rt");
        assert_eq!(Settings::load(&p), Settings::default());
        let s = Settings {
            keep_screenshots: Some(25),
            size_warning_mb: 200,
            show_capture_border: false,
            hotkeys: HotkeySettings::default(),
            close_to_tray: true,
            show_window_titles: true,
            custom_words: vec!["Zebracorp".into()],
            custom_patterns: vec!["PROJ-[0-9]+".into()],
        };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s);
        assert!(!p.with_extension("json.partial").exists());
    }

    #[test]
    fn damaged_file_and_bad_values_fall_back() {
        let p = temp("bad");
        fs::write(&p, b"{ not json").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
        fs::write(&p, br#"{"keepScreenshots":0,"sizeWarningMb":300}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.keep_screenshots, None);
        assert_eq!(s.size_warning_mb, 300);
        // Unknown fields from a newer version are ignored.
        fs::write(&p, br#"{"keepScreenshots":10,"future":true}"#).unwrap();
        assert_eq!(Settings::load(&p).keep_screenshots, Some(10));
        // Files from before the tray setting existed: closing still quits.
        assert!(!Settings::load(&p).close_to_tray);
        // Files from before the window-title setting existed: titles stay out.
        assert!(!Settings::load(&p).show_window_titles);
    }

    #[test]
    fn validate_rejects_zero_and_huge() {
        let mut s = Settings::default();
        s.keep_screenshots = Some(0);
        assert!(s.validate().is_err());
        s.keep_screenshots = Some(MAX_KEEP_SCREENSHOTS + 1);
        assert!(s.validate().is_err());
        s.keep_screenshots = Some(1);
        s.size_warning_mb = 0;
        assert!(s.validate().is_err());
    }

    #[test]
    fn hotkeys_parse_normalize_and_reject() {
        let d = HotkeySettings::default();
        assert_eq!(d.parse().unwrap()[0].label(), "Ctrl+Alt+Shift+R");
        assert!(!d.parse().unwrap()[0].is_weak());
        let k = HotkeySpec::parse("shift + alt + ctrl + f9").unwrap();
        assert_eq!(k.label(), "Ctrl+Alt+Shift+F9");
        assert!(HotkeySpec::parse("Ctrl+Alt+Q").unwrap().is_weak());
        for bad in ["R", "Shift+R", "Ctrl+Alt", "Ctrl+Alt+F13", "Ctrl+Alt+AB", "Ctrl+Ctrl+R", "Ctrl+Alt+R+T", ""] {
            assert!(HotkeySpec::parse(bad).is_err(), "{bad}");
        }
        let mut h = HotkeySettings::default();
        h.pause = "ctrl+alt+shift+r".into();
        let e = h.parse().unwrap_err();
        assert!(e.contains("both use"), "{e}");
        h.pause = "Ctrl+Alt+Shift+Q".into();
        assert!(h.parse().is_ok());
    }

    // The sentinel stands in for a customer name: no error text may contain it.
    const SENTINEL: &str = "Qzx7Sentinel";

    fn with_custom(words: Vec<String>, patterns: Vec<String>) -> Settings {
        Settings {
            custom_words: words,
            custom_patterns: patterns,
            ..Settings::default()
        }
    }

    #[test]
    fn files_from_before_custom_rules_load_with_none() {
        let p = temp("oldcustom");
        fs::write(&p, br#"{"keepScreenshots":10,"showWindowTitles":true}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.keep_screenshots, Some(10));
        assert!(s.custom_words.is_empty() && s.custom_patterns.is_empty());
        assert!(s.custom_rules().is_ok());
    }

    #[test]
    fn custom_rules_use_camel_case_names_in_the_file() {
        let p = temp("camel");
        with_custom(vec!["acme".into()], vec!["a+".into()])
            .save(&p)
            .unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"customWords\""), "{text}");
        assert!(text.contains("\"customPatterns\""), "{text}");
    }

    #[test]
    fn custom_entries_that_must_be_refused_are_refused_without_quoting_them() {
        let long = "a".repeat(MAX_CUSTOM_CHARS + 1);
        let many: Vec<String> = (0..=MAX_CUSTOM_ENTRIES)
            .map(|i| format!("{SENTINEL}{i}"))
            .collect();
        let cases = [
            with_custom(vec![format!("ok {SENTINEL}"), "   ".into()], vec![]),
            with_custom(vec![], vec![format!("{SENTINEL}(")]),
            with_custom(vec![], vec![format!("{SENTINEL}{{1}}{{2}}[")]),
            with_custom(vec![], vec!["".into()]),
            with_custom(vec![format!("{SENTINEL}{long}")], vec![]),
            with_custom(vec![], vec![format!("{SENTINEL}{long}")]),
            with_custom(vec![], vec![format!("({SENTINEL}\\w{{200}}){{200}}")]),
            with_custom(many.clone(), vec![]),
            with_custom(vec![], many),
        ];
        for (i, s) in cases.iter().enumerate() {
            let e = s.validate().expect_err(&format!("case {i} should be refused"));
            assert!(!e.contains(SENTINEL), "case {i}: {e}");
            assert!(s.custom_rules().is_err(), "case {i}");
        }
        let e = cases[1].validate().unwrap_err();
        assert_eq!(e, "custom pattern 1 is not valid");
        assert_eq!(cases[0].validate().unwrap_err(), "custom word 2 is empty");
    }

    #[test]
    fn custom_entries_at_the_limits_are_accepted() {
        let words: Vec<String> = (0..MAX_CUSTOM_ENTRIES).map(|i| format!("word{i}")).collect();
        let s = with_custom(words, vec!["a".repeat(MAX_CUSTOM_CHARS)]);
        assert!(s.validate().is_ok());
        assert!(s.custom_rules().is_ok());
    }

    #[test]
    fn bad_custom_entries_in_the_file_are_dropped_one_by_one() {
        let p = temp("badcustom");
        fs::write(&p, br#"{"customWords":["acme","  "],"customPatterns":["(","PROJ-[0-9]+"],"keepScreenshots":7}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.custom_words, vec!["acme".to_string()]);
        assert_eq!(s.custom_patterns, vec!["PROJ-[0-9]+".to_string()]);
        assert_eq!(s.keep_screenshots, Some(7));
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rules_built_from_settings_hide_the_word_and_the_pattern() {
        let s = with_custom(vec!["Zebracorp".into()], vec!["PROJ-[0-9]+".into()]);
        let rules = s.custom_rules().unwrap();
        assert_eq!(crate::pii::scrub("ZEBRACORP wants PROJ-42", &rules), "[custom] wants [custom]");
        assert_eq!(crate::pii::scrub("zebracorps", &rules), "zebracorps");
        let shown = format!("{rules:?}");
        assert!(!shown.contains("Zebracorp") && !shown.contains("PROJ"), "{shown}");
    }

    #[test]
    fn bad_hotkeys_in_the_file_fall_back_to_defaults() {
        let p = temp("hk");
        fs::write(&p, br#"{"keepScreenshots":7,"hotkeys":{"startStop":"R","pause":"Ctrl+Alt+P"}}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.hotkeys, HotkeySettings::default());
        assert_eq!(s.keep_screenshots, Some(7));
    }
}

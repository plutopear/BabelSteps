//! C7: settings export and import. The file is plain JSON a person can read.
//!
//! Export covers every setting except machine-specific ones (a folder path can
//! hold a user name), which stay out of the file. Import is strict: unlike
//! `Settings::load`, which quietly falls back to defaults so the app can start,
//! a file the user chose is either accepted whole or rejected with a reason.
//! Nothing is applied on a rejection, and error messages name fields and
//! actions that this code knows, never anything quoted from the file.

use crate::settings::{HotkeySettings, HotkeySpec, Settings};
use serde_json::{json, Map, Value};
use std::fmt;

/// Bump when the file layout changes in a way an older app cannot read.
pub const EXPORT_FORMAT_VERSION: u32 = 1;

/// Why an import was refused. Holds only text written in this file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// Not JSON at all. Position only, never the text around it.
    NotJson {
        line: usize,
        column: usize,
    },
    /// JSON, but not a BabelSteps settings export.
    NotSettingsFile,
    /// Made by a newer BabelSteps than this one.
    TooNew,
    Missing(&'static str),
    WrongType {
        field: &'static str,
        expected: &'static str,
    },
    /// The shortcut for this action is not a valid shortcut.
    BadHotkey(&'static str),
    /// A value outside its limits, or two actions on one shortcut.
    Invalid(String),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingsError::NotJson { line, column } => write!(
                f,
                "This file is not a settings file: it is not valid JSON (problem near line {line}, column {column})."
            ),
            SettingsError::NotSettingsFile => {
                write!(f, "This file is not a BabelSteps settings export.")
            }
            SettingsError::TooNew => write!(
                f,
                "This settings file was made by a newer version of BabelSteps. Update BabelSteps to import it."
            ),
            SettingsError::Missing(field) => {
                write!(f, "The settings file has no value for \"{field}\".")
            }
            SettingsError::WrongType { field, expected } => write!(
                f,
                "The settings file has the wrong kind of value for \"{field}\": expected {expected}."
            ),
            SettingsError::BadHotkey(action) => write!(
                f,
                "The shortcut for \"{action}\" in the settings file is not a valid shortcut."
            ),
            SettingsError::Invalid(reason) => {
                write!(f, "The settings file has a value that cannot be used: {reason}.")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// Shortcut keys in the file, in `HotkeySettings::ACTIONS` order.
const HOTKEY_FIELDS: [&str; 4] = [
    "hotkeys.startStop",
    "hotkeys.pause",
    "hotkeys.note",
    "hotkeys.stop",
];

impl Settings {
    /// The export file text. Every field of `Settings` is named below, so
    /// adding a field stops compiling until it is either exported or marked
    /// machine-specific here.
    pub fn to_export_json(&self) -> String {
        let Settings {
            keep_screenshots,
            size_warning_mb,
            show_capture_border,
            hotkeys,
            close_to_tray,
            show_window_titles,
            custom_words,
            custom_patterns,
        } = self;
        // Machine-specific settings (folder paths and the like) are left out on
        // purpose. There are none today: the recordings folder is not a setting.
        let hk = hotkeys.normalized().unwrap_or_else(|_| hotkeys.clone());
        let doc = json!({
            "format_version": EXPORT_FORMAT_VERSION,
            "keepScreenshots": keep_screenshots,
            "sizeWarningMb": size_warning_mb,
            "showCaptureBorder": show_capture_border,
            "hotkeys": {
                "startStop": hk.start_stop,
                "pause": hk.pause,
                "note": hk.note,
                "stop": hk.stop,
            },
            "closeToTray": close_to_tray,
            "showWindowTitles": show_window_titles,
            // The user's own lists travel with the file; they are not
            // specific to this PC.
            "customWords": custom_words,
            "customPatterns": custom_patterns,
        });
        let mut text = serde_json::to_string_pretty(&doc).expect("a JSON value always serializes");
        text.push('\n');
        text
    }

    /// Parse an export file strictly. Wrong types, missing fields, bad shortcuts
    /// and out-of-range values are errors; unknown extra fields are ignored. The
    /// result has canonical shortcut text and passes `validate`.
    pub fn from_json_strict(text: &str) -> Result<Settings, SettingsError> {
        // Some Windows editors put a byte-order mark at the start of the file.
        let text = text.trim_start_matches('\u{feff}');
        let value: Value = serde_json::from_str(text).map_err(|e| SettingsError::NotJson {
            line: e.line(),
            column: e.column(),
        })?;
        let obj = value.as_object().ok_or(SettingsError::NotSettingsFile)?;

        // The version comes first: a newer file may be laid out differently.
        let version = obj
            .get("format_version")
            .ok_or(SettingsError::NotSettingsFile)?;
        let version = version.as_u64().ok_or(SettingsError::WrongType {
            field: "format_version",
            expected: "a whole number",
        })?;
        if version > u64::from(EXPORT_FORMAT_VERSION) {
            return Err(SettingsError::TooNew);
        }
        if version == 0 {
            return Err(SettingsError::NotSettingsFile);
        }

        let keep_screenshots = match field(obj, "keepScreenshots")? {
            Value::Null => None,
            v => Some(as_u32(v, "keepScreenshots")?),
        };
        let size_warning_mb = as_u32(field(obj, "sizeWarningMb")?, "sizeWarningMb")?;
        let show_capture_border = as_bool(field(obj, "showCaptureBorder")?, "showCaptureBorder")?;
        let close_to_tray = as_bool(field(obj, "closeToTray")?, "closeToTray")?;
        let show_window_titles = as_bool(field(obj, "showWindowTitles")?, "showWindowTitles")?;

        // Files exported before the custom lists existed have neither field.
        let custom_words = string_list(obj, "customWords")?;
        let custom_patterns = string_list(obj, "customPatterns")?;

        let hk = field(obj, "hotkeys")?
            .as_object()
            .ok_or(SettingsError::WrongType {
                field: "hotkeys",
                expected: "an object",
            })?;
        let mut specs: Vec<HotkeySpec> = Vec::new();
        for (i, name) in HOTKEY_FIELDS.iter().enumerate() {
            let key = name.trim_start_matches("hotkeys.");
            let v = hk.get(key).ok_or(SettingsError::Missing(name))?;
            let text = v.as_str().ok_or(SettingsError::WrongType {
                field: name,
                expected: "text",
            })?;
            let spec = HotkeySpec::parse(text)
                .map_err(|_| SettingsError::BadHotkey(HotkeySettings::ACTIONS[i]))?;
            specs.push(spec);
        }
        let hotkeys = HotkeySettings {
            start_stop: specs[0].label(),
            pause: specs[1].label(),
            note: specs[2].label(),
            stop: specs[3].label(),
        };

        let settings = Settings {
            keep_screenshots,
            size_warning_mb,
            show_capture_border,
            hotkeys,
            close_to_tray,
            show_window_titles,
            custom_words,
            custom_patterns,
        };
        // Shortcuts are canonical by now, so what is left to report is a limit
        // or two actions on one shortcut. Those messages hold no file text.
        settings.validate().map_err(SettingsError::Invalid)?;
        Ok(settings)
    }

    /// Put imported settings on top of these. Machine-specific settings keep
    /// the current value, and the custom hide words and patterns are merged
    /// (yours first, then the new ones from the file). Every field is named, so a new one must be classified.
    pub fn merge_import(&self, imported: Settings) -> Settings {
        let Settings {
            keep_screenshots,
            size_warning_mb,
            show_capture_border,
            hotkeys,
            close_to_tray,
            show_window_titles,
            custom_words,
            custom_patterns,
        } = imported;
        let mut out = self.clone();
        out.keep_screenshots = keep_screenshots;
        out.size_warning_mb = size_warning_mb;
        out.show_capture_border = show_capture_border;
        out.hotkeys = hotkeys;
        out.close_to_tray = close_to_tray;
        out.show_window_titles = show_window_titles;
        // The hide lists are added to, never replaced: an import (an older
        // export, or someone else's file) must not switch off protection the
        // user set up themselves.
        out.custom_words = union(&self.custom_words, custom_words, |a, b| a.trim().to_lowercase() == b.trim().to_lowercase());
        out.custom_patterns = union(&self.custom_patterns, custom_patterns, |a, b| a == b);
        out
    }

    /// Plain-language notes for each change from `self` to `after` that
    /// weakens a privacy default, so an import can say so. A settings file
    /// from someone else can switch these, so the user is told, not asked.
    pub fn privacy_notes(&self, after: &Settings) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.show_window_titles && after.show_window_titles {
            out.push("Full window titles are now saved in steps (they can hold file names and customer names).");
        }
        if self.show_capture_border && !after.show_capture_border {
            out.push("The coloured frame around the screen while recording is now off.");
        }
        if !self.close_to_tray && after.close_to_tray {
            out.push("Closing the window now keeps BabelSteps running in the tray.");
        }
        let keeps_more = match (self.keep_screenshots, after.keep_screenshots) {
            (Some(_), None) => true,
            (Some(a), Some(b)) => b > a,
            _ => false,
        };
        if keeps_more {
            out.push("More screenshots are now kept on disk after a recording.");
        }
        out
    }

    /// Parse an export file and merge it onto these settings. Returns the new
    /// settings without changing `self`; the caller saves them through the
    /// normal save path.
    pub fn import_json(&self, text: &str) -> Result<Settings, SettingsError> {
        let merged = self.merge_import(Settings::from_json_strict(text)?);
        // Merging can push a list past its limit; refuse rather than drop entries.
        merged.validate().map_err(SettingsError::Invalid)?;
        Ok(merged)
    }
}

/// `current` followed by the entries of `added` that are not already in it.
fn union(current: &[String], added: Vec<String>, same: impl Fn(&str, &str) -> bool) -> Vec<String> {
    let mut out = current.to_vec();
    for entry in added {
        if !out.iter().any(|e| same(e, &entry)) {
            out.push(entry);
        }
    }
    out
}

fn field<'a>(obj: &'a Map<String, Value>, name: &'static str) -> Result<&'a Value, SettingsError> {
    obj.get(name).ok_or(SettingsError::Missing(name))
}

/// An optional list of text entries; absent means empty. Wrong types are
/// errors that name the field, never the value.
fn string_list(obj: &Map<String, Value>, name: &'static str) -> Result<Vec<String>, SettingsError> {
    let Some(v) = obj.get(name) else {
        return Ok(Vec::new());
    };
    let wrong = || SettingsError::WrongType {
        field: name,
        expected: "a list of text",
    };
    v.as_array()
        .ok_or_else(wrong)?
        .iter()
        .map(|e| e.as_str().map(String::from).ok_or_else(wrong))
        .collect()
}

fn as_bool(v: &Value, name: &'static str) -> Result<bool, SettingsError> {
    v.as_bool().ok_or(SettingsError::WrongType {
        field: name,
        expected: "true or false",
    })
}

/// A whole number. Too big for `u32` becomes `u32::MAX`, which `validate`
/// refuses as out of range.
fn as_u32(v: &Value, name: &'static str) -> Result<u32, SettingsError> {
    let n = v.as_u64().ok_or(SettingsError::WrongType {
        field: name,
        expected: "a whole number, 0 or more",
    })?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::MAX_KEEP_SCREENSHOTS;

    const SENTINEL: &str = "ZQ-SENTINEL-7731";

    fn custom() -> Settings {
        Settings {
            keep_screenshots: Some(25),
            size_warning_mb: 200,
            show_capture_border: false,
            hotkeys: HotkeySettings {
                start_stop: "Ctrl+Alt+Shift+F5".into(),
                pause: "Ctrl+Alt+Shift+F6".into(),
                note: "Ctrl+Alt+Shift+F7".into(),
                stop: "Ctrl+Alt+Shift+F8".into(),
            },
            close_to_tray: true,
            show_window_titles: true,
            custom_words: vec!["Acme".into(), "Project Falcon".into()],
            custom_patterns: vec![r"\bAC-\d{4}\b".into()],
        }
    }

    /// A valid export with one field replaced (a dotted path reaches into hotkeys).
    fn with(path: &str, v: Value) -> String {
        let mut doc: Value = serde_json::from_str(&Settings::default().to_export_json()).unwrap();
        match path.split_once('.') {
            Some((a, b)) => doc[a][b] = v,
            None => doc[path] = v,
        }
        doc.to_string()
    }

    fn without(path: &str) -> String {
        let mut doc: Value = serde_json::from_str(&Settings::default().to_export_json()).unwrap();
        match path.split_once('.') {
            Some((a, b)) => drop(doc[a].as_object_mut().unwrap().remove(b)),
            None => drop(doc.as_object_mut().unwrap().remove(path)),
        }
        doc.to_string()
    }

    /// Refused, nothing changed, and the message holds nothing from the file.
    fn refused(current: &Settings, text: &str) -> SettingsError {
        let before = current.clone();
        let e = current.import_json(text).expect_err("should be refused");
        assert_eq!(*current, before, "a refused import must change nothing");
        for shown in [e.to_string(), format!("{e:?}")] {
            assert!(!shown.contains(SENTINEL), "error repeats the file: {shown}");
        }
        e
    }

    #[test]
    fn export_then_import_gives_the_same_settings() {
        for s in [Settings::default(), custom()] {
            let text = s.to_export_json();
            assert_eq!(Settings::from_json_strict(&text).unwrap(), s);
            assert_eq!(Settings::default().import_json(&text).unwrap(), s);
        }
    }

    #[test]
    fn export_names_its_version_and_every_setting() {
        let doc: Value = serde_json::from_str(&custom().to_export_json()).unwrap();
        assert_eq!(doc["format_version"], EXPORT_FORMAT_VERSION);
        assert_eq!(doc["keepScreenshots"], 25);
        assert_eq!(doc["hotkeys"]["pause"], "Ctrl+Alt+Shift+F6");
        // Every key the app stores is either exported or listed here as machine-specific.
        const MACHINE_SPECIFIC: [&str; 0] = [];
        let stored = serde_json::to_value(Settings::default()).unwrap();
        for key in stored.as_object().unwrap().keys() {
            assert!(
                doc.get(key).is_some() || MACHINE_SPECIFIC.contains(&key.as_str()),
                "setting {key} is neither exported nor marked machine-specific"
            );
        }
        for key in doc.as_object().unwrap().keys() {
            assert!(
                key == "format_version" || stored.get(key).is_some(),
                "{key} is exported but is not a setting"
            );
        }
    }

    #[test]
    fn export_holds_no_paths() {
        // The user's own word lists are theirs to fill, so leave them empty here.
        let mut c = custom();
        c.custom_words.clear();
        c.custom_patterns.clear();
        let text = c.to_export_json().to_ascii_lowercase();
        for word in ["folder", "path", "dir", "user", "\\\\", "c:"] {
            assert!(!text.contains(word), "export mentions {word}");
        }
    }

    #[test]
    fn import_ignores_unknown_and_machine_specific_fields() {
        // A file that carries a folder path, as a future or hand-edited file might.
        let mut doc: Value = serde_json::from_str(&custom().to_export_json()).unwrap();
        doc["recordingsFolder"] = json!(format!("C:\\Users\\{SENTINEL}\\Recordings"));
        doc["futureThing"] = json!({ "x": SENTINEL });
        let text = doc.to_string();
        assert_eq!(Settings::from_json_strict(&text).unwrap(), custom());
        let merged = Settings::default().import_json(&text).unwrap();
        assert_eq!(merged, custom());
        assert!(!format!("{merged:?}").contains(SENTINEL));
    }

    #[test]
    fn merge_replaces_every_exported_setting_except_the_hide_lists() {
        let current = Settings {
            keep_screenshots: Some(3),
            ..Settings::default()
        };
        assert_eq!(current.merge_import(custom()), custom());
        // Importing the defaults switches the other settings back but must not
        // empty the user's own hide lists.
        let mine = custom();
        let merged = mine.merge_import(Settings::default());
        assert_eq!(merged.custom_words, mine.custom_words);
        assert_eq!(merged.custom_patterns, mine.custom_patterns);
        assert_eq!(Settings { custom_words: vec![], custom_patterns: vec![], ..merged }, Settings::default());
        let mut other = Settings::default();
        other.show_capture_border = !mine.show_capture_border;
        let merged = mine.merge_import(other.clone());
        assert_eq!(merged.show_capture_border, other.show_capture_border);
        assert_eq!(merged.custom_words, mine.custom_words);
        assert_eq!(merged.custom_patterns, mine.custom_patterns);
    }

    #[test]
    fn import_adds_hide_entries_without_duplicates_and_keeps_the_users_first() {
        let mut mine = Settings::default();
        mine.custom_words = vec!["Falcon".into(), "Acme".into()];
        mine.custom_patterns = vec![r"\bAC-\d{4}\b".into()];
        let mut file = Settings::default();
        file.custom_words = vec!["acme ".into(), "Orion".into()];
        file.custom_patterns = vec![r"\bAC-\d{4}\b".into(), r"ZX-\d+".into()];
        let merged = mine.import_json(&file.to_export_json()).unwrap();
        assert_eq!(merged.custom_words, ["Falcon", "Acme", "Orion"]);
        assert_eq!(merged.custom_patterns, [r"\bAC-\d{4}\b", r"ZX-\d+"]);
        // An older export with no lists at all leaves the user's lists alone.
        let mut doc: Value = serde_json::from_str(&Settings::default().to_export_json()).unwrap();
        doc.as_object_mut().unwrap().remove("customWords");
        doc.as_object_mut().unwrap().remove("customPatterns");
        assert_eq!(mine.import_json(&doc.to_string()).unwrap().custom_words, mine.custom_words);
    }

    #[test]
    fn import_that_would_overfill_a_hide_list_is_refused_not_trimmed() {
        let mut mine = Settings::default();
        mine.custom_words = (0..crate::settings::MAX_CUSTOM_ENTRIES).map(|i| format!("mine{i}")).collect();
        let mut file = Settings::default();
        file.custom_words = vec!["one-too-many".into()];
        let err = mine.import_json(&file.to_export_json()).expect_err("must be refused");
        assert!(matches!(err, SettingsError::Invalid(_)), "{err:?}");
        assert_eq!(mine.custom_words.len(), crate::settings::MAX_CUSTOM_ENTRIES, "the user's list is untouched");
    }

    #[test]
    fn malformed_json_is_refused() {
        let current = custom();
        for bad in [
            format!("{{ \"keepScreenshots\": \"{SENTINEL}\""),
            format!("not json {SENTINEL}"),
            String::new(),
            format!("{{\"a\": {SENTINEL}}}"),
        ] {
            assert!(
                matches!(refused(&current, &bad), SettingsError::NotJson { .. }),
                "{bad}"
            );
        }
    }

    #[test]
    fn json_that_is_not_a_settings_export_is_refused() {
        let current = custom();
        for bad in [
            "[1,2]",
            "42",
            "null",
            "{}",
            "{\"keepScreenshots\":5}",
            "{\"format_version\":0}",
        ] {
            assert_eq!(
                refused(&current, bad),
                SettingsError::NotSettingsFile,
                "{bad}"
            );
        }
        // A raw settings.json from the app has no format_version either.
        let raw = serde_json::to_string(&Settings::default()).unwrap();
        assert_eq!(refused(&current, &raw), SettingsError::NotSettingsFile);
        let e = refused(&current, &format!("{{\"format_version\":\"{SENTINEL}\"}}"));
        assert!(matches!(
            e,
            SettingsError::WrongType {
                field: "format_version",
                ..
            }
        ));
    }

    #[test]
    fn wrong_types_are_refused() {
        let current = custom();
        let s = json!(SENTINEL);
        for (path, v) in [
            ("keepScreenshots", s.clone()),
            ("keepScreenshots", json!(-3)),
            ("keepScreenshots", json!(2.5)),
            ("keepScreenshots", json!(true)),
            ("sizeWarningMb", s.clone()),
            ("sizeWarningMb", json!(null)),
            ("showCaptureBorder", s.clone()),
            ("showCaptureBorder", json!(1)),
            ("closeToTray", s.clone()),
            ("showWindowTitles", json!(["x", SENTINEL])),
            ("hotkeys", s.clone()),
            ("hotkeys.pause", json!(7)),
            ("hotkeys.note", json!({ "a": SENTINEL })),
        ] {
            let e = refused(&current, &with(path, v.clone()));
            assert!(
                matches!(e, SettingsError::WrongType { .. }),
                "{path} = {v}: {e:?}"
            );
        }
    }

    #[test]
    fn missing_fields_are_refused() {
        let current = custom();
        for path in [
            "keepScreenshots",
            "sizeWarningMb",
            "showCaptureBorder",
            "closeToTray",
            "showWindowTitles",
            "hotkeys",
            "hotkeys.stop",
        ] {
            let e = refused(&current, &without(path));
            assert!(matches!(e, SettingsError::Missing(_)), "{path}: {e:?}");
        }
    }

    #[test]
    fn bad_hotkeys_are_refused_without_repeating_them() {
        let current = custom();
        for (i, key) in ["startStop", "pause", "note", "stop"].iter().enumerate() {
            for bad in [
                format!("Ctrl+Alt+{SENTINEL}"),
                SENTINEL.to_string(),
                "R".to_string(),
                "Shift+R".to_string(),
                "Ctrl+Alt+F13".to_string(),
                String::new(),
            ] {
                let e = refused(&current, &with(&format!("hotkeys.{key}"), json!(bad)));
                assert_eq!(
                    e,
                    SettingsError::BadHotkey(HotkeySettings::ACTIONS[i]),
                    "{key}: {bad}"
                );
            }
        }
    }

    #[test]
    fn two_actions_on_one_shortcut_are_refused() {
        let current = custom();
        let e = refused(&current, &with("hotkeys.pause", json!("ctrl+alt+shift+r")));
        match e {
            SettingsError::Invalid(m) => assert!(m.contains("both use"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn out_of_range_limits_are_refused() {
        let current = custom();
        for (path, v) in [
            ("keepScreenshots", json!(0)),
            ("keepScreenshots", json!(MAX_KEEP_SCREENSHOTS + 1)),
            ("keepScreenshots", json!(u64::MAX)),
            ("sizeWarningMb", json!(0)),
            ("sizeWarningMb", json!(1_000_001)),
            ("sizeWarningMb", json!(u64::MAX)),
        ] {
            let e = refused(&current, &with(path, v.clone()));
            assert!(
                matches!(e, SettingsError::Invalid(_)),
                "{path} = {v}: {e:?}"
            );
        }
        // The edges are accepted.
        assert!(current
            .import_json(&with("keepScreenshots", json!(1)))
            .is_ok());
        assert!(current
            .import_json(&with("keepScreenshots", json!(MAX_KEEP_SCREENSHOTS)))
            .is_ok());
        assert!(current
            .import_json(&with("sizeWarningMb", json!(1_000_000)))
            .is_ok());
    }

    #[test]
    fn a_newer_format_version_is_refused() {
        let current = custom();
        for v in [
            json!(EXPORT_FORMAT_VERSION + 1),
            json!(999),
            json!(u64::MAX),
        ] {
            let mut doc: Value = serde_json::from_str(&custom().to_export_json()).unwrap();
            doc["format_version"] = v;
            // A newer layout may have fields this version cannot read; the version check wins.
            doc["sizeWarningMb"] = json!(SENTINEL);
            doc["somethingNew"] = json!({ "secret": SENTINEL });
            assert_eq!(refused(&current, &doc.to_string()), SettingsError::TooNew);
        }
        let msg = SettingsError::TooNew.to_string();
        assert!(
            msg.contains("newer version") && msg.contains("Update BabelSteps"),
            "{msg}"
        );
    }

    #[test]
    fn a_byte_order_mark_is_tolerated() {
        let text = format!("\u{feff}{}", custom().to_export_json());
        assert_eq!(Settings::from_json_strict(&text).unwrap(), custom());
    }

    #[test]
    fn hotkeys_are_written_canonically_on_import() {
        let text = with("hotkeys.note", json!("shift + alt + ctrl + n"));
        assert_eq!(
            Settings::from_json_strict(&text).unwrap().hotkeys.note,
            "Ctrl+Alt+Shift+N"
        );
    }
    #[test]
    fn privacy_notes_name_only_weakened_defaults() {
        let base = Settings::default();
        assert!(base.privacy_notes(&base).is_empty());
        let mut weaker = base.clone();
        weaker.show_window_titles = true;
        weaker.show_capture_border = false;
        weaker.close_to_tray = true;
        weaker.keep_screenshots = None;
        let mut had_limit = base.clone();
        had_limit.keep_screenshots = Some(10);
        assert_eq!(base.privacy_notes(&weaker).len(), 3);
        assert_eq!(had_limit.privacy_notes(&weaker).len(), 4);
        // Tightening is not reported.
        assert!(weaker.privacy_notes(&base).is_empty());
    }
    #[test]
    fn custom_lists_round_trip_and_bad_ones_are_refused_without_echo() {
        let c = custom();
        let back = Settings::from_json_strict(&c.to_export_json()).unwrap();
        assert_eq!(back.custom_words, c.custom_words);
        assert_eq!(back.custom_patterns, c.custom_patterns);
        // Absent fields (older exports) mean empty lists.
        let mut doc: Value = serde_json::from_str(&Settings::default().to_export_json()).unwrap();
        doc.as_object_mut().unwrap().remove("customWords");
        doc.as_object_mut().unwrap().remove("customPatterns");
        let old = Settings::from_json_strict(&doc.to_string()).unwrap();
        assert!(old.custom_words.is_empty() && old.custom_patterns.is_empty());
        // Wrong type, and an invalid pattern: errors never show the text.
        let wrong =
            Settings::from_json_strict(&with("customWords", json!([SENTINEL, 5]))).unwrap_err();
        assert!(!format!("{wrong} {wrong:?}").contains(SENTINEL));
        let bad = format!("({SENTINEL}");
        let e = Settings::from_json_strict(&with("customPatterns", json!([bad]))).unwrap_err();
        assert!(!format!("{e} {e:?}").contains(SENTINEL));
    }
}

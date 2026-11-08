//! App settings (A8 screenshot retention, and later hotkeys): plain data, a
//! JSON file, no platform code. A missing or damaged file gives the defaults;
//! the app must start whatever is in it.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Q12: unlimited by default, with a warning above this many megabytes.
pub const DEFAULT_SIZE_WARNING_MB: u32 = 500;
pub const MAX_KEEP_SCREENSHOTS: u32 = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Keep screenshots for the newest N steps when a recording stops.
    /// `None` keeps them all (the default).
    pub keep_screenshots: Option<u32>,
    /// Warn after a recording when its screenshots take more than this many MB
    /// and no limit is set.
    pub size_warning_mb: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { keep_screenshots: None, size_warning_mb: DEFAULT_SIZE_WARNING_MB }
    }
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
        Ok(())
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
        assert!(s.validate().is_ok());
    }

    #[test]
    fn round_trips_and_missing_file_gives_defaults() {
        let p = temp("rt");
        assert_eq!(Settings::load(&p), Settings::default());
        let s = Settings { keep_screenshots: Some(25), size_warning_mb: 200 };
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
}

//! Recording storage (requirement C4): a folder bundle `Name.bsrec/` holding
//! `recording.db` (SQLite) and `images/`. Local files only.
//!
//! Privacy notes: `secure_delete=ON` zeroes freed pages so deleted or redacted
//! text does not linger in the db file; `journal_mode=DELETE` keeps a closed
//! bundle as a single db file. Nothing here logs document content, and error
//! messages carry paths and codes, never block text.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OpenFlags};

use crate::model::{Block, Recording, FORMAT_VERSION};

pub const BUNDLE_EXT: &str = "bsrec";
pub const DB_FILE: &str = "recording.db";
pub const IMAGES_DIR: &str = "images";

#[derive(Debug)]
pub enum StorageError {
    Io(io::Error),
    Db(rusqlite::Error),
    /// A stored block that could not be parsed. Position and kind only: the
    /// serde message can quote the stored text.
    Json { line: usize, column: usize, category: &'static str },
    /// The folder is not a BabelSteps bundle.
    NotABundle(PathBuf),
    /// The bundle was written by a newer version of the app.
    TooNew { found: u32, supported: u32 },
    /// A file name or path that is not allowed inside the bundle.
    InvalidName(String),
    /// A destructive request refused on safety grounds.
    Refused(String),
    #[cfg(test)]
    Forced,
}

pub type Result<T> = std::result::Result<T, StorageError>;

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "file error: {e}"),
            StorageError::Db(e) => write!(f, "database error: {e}"),
            StorageError::Json { line, column, category } => {
                write!(f, "stored block could not be read ({category} error at line {line}, column {column})")
            }
            StorageError::NotABundle(p) => {
                write!(f, "{} is not a BabelSteps recording", p.display())
            }
            StorageError::TooNew { found, supported } => write!(
                f,
                "this recording uses format version {found}, but this app only supports up to version {supported}; update BabelSteps to open it"
            ),
            StorageError::InvalidName(n) => write!(f, "not an allowed file name: {n:?}"),
            StorageError::Refused(why) => write!(f, "refused: {why}"),
            #[cfg(test)]
            StorageError::Forced => write!(f, "forced test failure"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<io::Error> for StorageError {
    fn from(e: io::Error) -> Self {
        StorageError::Io(e)
    }
}
impl From<rusqlite::Error> for StorageError {
    fn from(e: rusqlite::Error) -> Self {
        StorageError::Db(e)
    }
}
impl From<serde_json::Error> for StorageError {
    fn from(e: serde_json::Error) -> Self {
        use serde_json::error::Category;
        let category = match e.classify() {
            Category::Io => "io",
            Category::Syntax => "syntax",
            Category::Data => "data",
            Category::Eof => "unexpected end",
        };
        StorageError::Json { line: e.line(), column: e.column(), category }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS blocks (
    position INTEGER PRIMARY KEY NOT NULL,
    block_id TEXT NOT NULL,
    json     TEXT NOT NULL
);";

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA secure_delete=ON;")?;
    conn.execute_batch("PRAGMA journal_mode=DELETE;")?;
    Ok(())
}

/// Windows-safe folder stem for a title (without the `.bsrec` extension).
pub fn safe_folder_stem(title: &str) -> String {
    const ILLEGAL: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    let cleaned: String = title
        .chars()
        .filter(|c| !ILLEGAL.contains(c) && !c.is_control())
        .collect();
    let mut s: String = cleaned.trim().chars().take(100).collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let s = s.trim_start_matches([' ', '.']).to_string();
    if s.is_empty() {
        return "Recording".to_string();
    }
    if is_reserved_device_name(&s) {
        return format!("{s}_");
    }
    s
}

/// CON, PRN, AUX, NUL, COM1-9, LPT1-9, also when followed by an extension.
pub(crate) fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let up = stem.to_ascii_uppercase();
    matches!(up.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((up.starts_with("COM") || up.starts_with("LPT"))
            && up.len() == 4
            && matches!(up.as_bytes()[3], b'1'..=b'9'))
}

/// Check a single file name that will live directly inside `images/`.
fn validate_image_name(name: &str) -> Result<()> {
    let bad = name.is_empty()
        || name == "."
        || name.contains("..")
        || name.contains(['/', '\\', ':', '\0'])
        || name.chars().any(|c| c.is_control())
        || name.starts_with('.')
        || name.ends_with('.')
        || name.ends_with(' ')
        || name.contains(['<', '>', '"', '|', '?', '*'])
        || is_reserved_device_name(name)
        || Path::new(name).is_absolute();
    if bad {
        Err(StorageError::InvalidName(name.to_string()))
    } else {
        Ok(())
    }
}

/// Turn `images/<name>` into `<name>`, validating it.
fn image_name_from_relative(rel: &str) -> Result<&str> {
    let name = rel
        .strip_prefix("images/")
        .ok_or_else(|| StorageError::InvalidName(rel.to_string()))?;
    validate_image_name(name)?;
    Ok(name)
}

#[derive(Debug)]
pub struct Bundle {
    path: PathBuf,
    conn: Connection,
}

impl Bundle {
    /// Create a new empty bundle in `parent_dir`, named after `title`.
    pub fn create(
        parent_dir: &Path,
        title: &str,
        id: &str,
        created_unix_ms: u64,
    ) -> Result<Bundle> {
        fs::create_dir_all(parent_dir)?;
        let stem = safe_folder_stem(title);
        let mut n = 1u32;
        let path = loop {
            let name = if n == 1 {
                format!("{stem}.{BUNDLE_EXT}")
            } else {
                format!("{stem} ({n}).{BUNDLE_EXT}")
            };
            let candidate = parent_dir.join(name);
            // create_dir fails if it exists, so two creators cannot share a folder.
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
                Err(e) => return Err(e.into()),
            }
        };
        let built = (|| -> Result<Bundle> {
            fs::create_dir(path.join(IMAGES_DIR))?;
            let conn = Connection::open(path.join(DB_FILE))?;
            configure(&conn)?;
            conn.execute_batch(SCHEMA)?;
            let b = Bundle { path: path.clone(), conn };
            let tx = b.conn.unchecked_transaction()?;
            b.write_meta(&tx, FORMAT_VERSION, id, title, created_unix_ms, now_ms())?;
            tx.commit()?;
            Ok(b)
        })();
        if built.is_err() {
            let _ = fs::remove_dir_all(&path);
        }
        built
    }

    /// Open an existing bundle. Refuses non-bundles and newer format versions.
    pub fn open(path: &Path) -> Result<Bundle> {
        let db = path.join(DB_FILE);
        if !path.is_dir() || !db.is_file() {
            return Err(StorageError::NotABundle(path.to_path_buf()));
        }
        // No CREATE flag: never turn an arbitrary file into a database.
        let conn = Connection::open_with_flags(
            &db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        configure(&conn)?;
        let has_tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('meta','blocks')",
                [],
                |r| r.get(0),
            )
            .map_err(|_| StorageError::NotABundle(path.to_path_buf()))?;
        if has_tables != 2 {
            return Err(StorageError::NotABundle(path.to_path_buf()));
        }
        let b = Bundle { path: path.to_path_buf(), conn };
        let found: u32 = b
            .meta("format_version")?
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| StorageError::NotABundle(path.to_path_buf()))?;
        if found > FORMAT_VERSION {
            return Err(StorageError::TooNew { found, supported: FORMAT_VERSION });
        }
        Ok(b)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn images_dir(&self) -> PathBuf {
        self.path.join(IMAGES_DIR)
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM meta WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    fn write_meta(
        &self,
        conn: &Connection,
        version: u32,
        id: &str,
        title: &str,
        created: u64,
        updated: u64,
    ) -> Result<()> {
        Self::set_meta(conn, "format_version", &version.to_string())?;
        Self::set_meta(conn, "id", id)?;
        Self::set_meta(conn, "title", title)?;
        Self::set_meta(conn, "created", &created.to_string())?;
        Self::set_meta(conn, "updated", &updated.to_string())?;
        Ok(())
    }

    /// Read the whole document. Older (v1) data loads with defaults filled in.
    pub fn load(&self) -> Result<Recording> {
        let missing = || StorageError::NotABundle(self.path.clone());
        let format_version: u32 =
            self.meta("format_version")?.and_then(|v| v.parse().ok()).ok_or_else(missing)?;
        let id = self.meta("id")?.ok_or_else(missing)?;
        let title = self.meta("title")?.ok_or_else(missing)?;
        let created_unix_ms: u64 =
            self.meta("created")?.and_then(|v| v.parse().ok()).ok_or_else(missing)?;
        let mut stmt = self.conn.prepare("SELECT json FROM blocks ORDER BY position")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut blocks = Vec::new();
        for row in rows {
            blocks.push(serde_json::from_str::<Block>(&row?)?);
        }
        Ok(Recording { format_version, id, title, created_unix_ms, blocks })
    }

    /// Replace the stored document in a single transaction. If anything fails
    /// the previous document stays intact.
    pub fn save(&self, rec: &Recording) -> Result<()> {
        self.save_impl(rec, None)
    }

    fn save_impl(&self, rec: &Recording, fail_after: Option<usize>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        self.write_meta(&tx, FORMAT_VERSION, &rec.id, &rec.title, rec.created_unix_ms, now_ms())?;
        tx.execute("DELETE FROM blocks", [])?;
        {
            let mut ins =
                tx.prepare("INSERT INTO blocks (position, block_id, json) VALUES (?1, ?2, ?3)")?;
            for (i, block) in rec.blocks.iter().enumerate() {
                if fail_after == Some(i) {
                    // Dropping `tx` without commit rolls everything back.
                    #[cfg(test)]
                    return Err(StorageError::Forced);
                }
                let json = serde_json::to_string(block)?;
                ins.execute(params![i as i64, block_id(block), json])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Change the title only. The folder keeps its name.
    pub fn rename(&self, new_title: &str) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        Self::set_meta(&tx, "title", new_title)?;
        Self::set_meta(&tx, "updated", &now_ms().to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// Write an image under `images/`, via a temp file and rename, so a partial
    /// image never appears under its final name. Returns `images/<name>`.
    pub fn write_image(&self, file_name: &str, bytes: &[u8]) -> Result<String> {
        validate_image_name(file_name)?;
        let dir = self.images_dir();
        fs::create_dir_all(&dir)?;
        let tmp = dir.join(format!(".{file_name}.partial"));
        let result = (|| -> io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, dir.join(file_name))
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(format!("{IMAGES_DIR}/{file_name}"))
    }

    /// A8: keep screenshots for the newest `keep` steps only. The trimmed
    /// document is saved first and files are deleted after, so a failure never
    /// leaves a step pointing at a missing picture. `rec` is replaced by the
    /// trimmed document only once it is saved.
    pub fn apply_retention(&self, rec: &mut Recording, keep: usize) -> Result<RetentionOutcome> {
        let mut trimmed = rec.clone();
        let removed = trimmed.apply_retention(keep);
        if removed.is_empty() {
            return Ok(RetentionOutcome { removed: 0, failed: 0 });
        }
        self.save(&trimmed)?;
        *rec = trimmed;
        let failed = removed.iter().filter(|p| self.remove_image(p).is_err()).count();
        Ok(RetentionOutcome { removed: removed.len(), failed })
    }

    /// Delete an image given its relative path (`images/<name>`). A missing
    /// file is not an error.
    pub fn remove_image(&self, relative_path: &str) -> Result<()> {
        let name = image_name_from_relative(relative_path)?;
        match fs::remove_file(self.images_dir().join(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// What `Bundle::apply_retention` did: steps that lost their screenshot, and
/// how many of those picture files could not be deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionOutcome {
    pub removed: usize,
    pub failed: usize,
}

fn block_id(b: &Block) -> &str {
    match b {
        Block::Heading { id, .. }
        | Block::Text { id, .. }
        | Block::List { id, .. }
        | Block::Image { id, .. }
        | Block::Callout { id, .. } => id,
        Block::Step(s) => &s.id,
    }
}

/// Permanently delete a bundle folder. Only a folder whose name ends in
/// `.bsrec` and that contains `recording.db` is removed.
pub fn delete_bundle(path: &Path) -> Result<()> {
    let ext_ok = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(BUNDLE_EXT))
        .unwrap_or(false);
    if !ext_ok {
        return Err(StorageError::Refused("folder name does not end in .bsrec".into()));
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Err(StorageError::Refused("not a plain folder".into()));
    }
    if !path.join(DB_FILE).is_file() {
        return Err(StorageError::Refused("folder has no recording.db".into()));
    }
    fs::remove_dir_all(path)?;
    Ok(())
}

pub mod library {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Summary {
        pub path: PathBuf,
        pub title: String,
        pub created: u64,
        pub updated: u64,
        pub step_count: usize,
    }

    #[derive(Debug)]
    pub struct Listing {
        /// Newest first (by `updated`).
        pub recordings: Vec<Summary>,
        /// Bundles that could not be read, with the reason. Not an overall failure.
        pub skipped: Vec<(PathBuf, String)>,
    }

    fn summarize(path: &Path) -> Result<Summary> {
        let b = Bundle::open(path)?;
        let num = |k: &str| -> Result<u64> {
            b.meta(k)?
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| StorageError::NotABundle(path.to_path_buf()))
        };
        let title = b.meta("title")?.ok_or_else(|| StorageError::NotABundle(path.to_path_buf()))?;
        let step_count: i64 = b.conn.query_row(
            "SELECT count(*) FROM blocks WHERE json_extract(json, '$.type') = 'step'",
            [],
            |r| r.get(0),
        )?;
        Ok(Summary {
            path: path.to_path_buf(),
            title,
            created: num("created")?,
            updated: num("updated")?,
            step_count: step_count as usize,
        })
    }

    /// List the `.bsrec` bundles directly inside `parent_dir`.
    pub fn list(parent_dir: &Path) -> Result<Listing> {
        let mut recordings = Vec::new();
        let mut skipped = Vec::new();
        for entry in fs::read_dir(parent_dir)? {
            let path = match entry {
                Ok(e) => e.path(),
                Err(e) => {
                    skipped.push((parent_dir.to_path_buf(), e.to_string()));
                    continue;
                }
            };
            let is_bundle_name = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(BUNDLE_EXT))
                .unwrap_or(false);
            if !is_bundle_name || !path.is_dir() {
                continue;
            }
            match summarize(&path) {
                Ok(s) => recordings.push(s),
                Err(e) => skipped.push((path, e.to_string())),
            }
        }
        recordings.sort_by(|a, b| b.updated.cmp(&a.updated).then(a.title.cmp(&b.title)));
        Ok(Listing { recordings, skipped })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, StepBlock};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    #[test]
    fn json_errors_do_not_quote_stored_values() {
        // serde's own message here would name the unknown variant.
        let err = serde_json::from_str::<Block>(r#"{"type":"nope-hunter2"}"#).unwrap_err();
        assert!(err.to_string().contains("hunter2"), "control: serde quotes the value");
        let err = StorageError::from(err);
        for shown in [err.to_string(), format!("{err:?}")] {
            assert!(!shown.contains("hunter2"), "{shown}");
        }
        assert!(matches!(err, StorageError::Json { line: 1, .. }));
    }

    /// Temp directory removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let p = std::env::temp_dir()
                .join(format!("babelsteps-storage-test-{}-{n}-{}", std::process::id(), now_ms()));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn step(id: &str, title: &str) -> Block {
        Block::Step(StepBlock {
            id: id.into(),
            action: Action::Comment,
            title: title.into(),
            description: String::new(),
            screenshot: None,
            target: None,
            redactions: vec![],
            captured_unix_ms: 5,
            screenshot_withheld: false,
        })
    }

    fn sample(blocks: Vec<Block>) -> Recording {
        let mut r = Recording::new("rec-1", "My Guide", 1000);
        r.blocks = blocks;
        r
    }

    #[test]
    fn retention_removes_old_files_from_disk_and_keeps_the_document_consistent() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "R", "1", 1).unwrap();
        let mut rec = Recording::new("rec-1", "R", 1);
        for i in 0..5 {
            let rel = b.write_image(&format!("step-{i:03}.png"), b"png").unwrap();
            let Block::Step(mut st) = step(&format!("s{i}"), "t") else { unreachable!() };
            st.screenshot = Some(rel);
            rec.blocks.push(Block::Step(st));
        }
        b.save(&rec).unwrap();
        let count = || fs::read_dir(b.images_dir()).unwrap().count();
        assert_eq!(count(), 5, "control: five files before");
        let out = b.apply_retention(&mut rec, 2).unwrap();
        assert_eq!((out.removed, out.failed), (3, 0));
        assert_eq!(count(), 2);
        let saved = b.load().unwrap();
        assert_eq!(saved, rec);
        let shots: Vec<_> = saved
            .blocks
            .iter()
            .map(|bl| match bl {
                Block::Step(s) => s.screenshot.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(&shots[..3], &[None, None, None]);
        for p in shots[3..].iter().flatten() {
            assert!(b.path().join(p).is_file(), "{p} kept and present");
        }
        assert_eq!(saved.step_count(), 5);
        // Nothing left to remove: a second run changes nothing.
        assert_eq!(b.apply_retention(&mut rec, 2).unwrap().removed, 0);
    }

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(safe_folder_stem("a<b>c:d\"e/f\\g|h?i*j"), "abcdefghij");
        assert_eq!(safe_folder_stem("CON"), "CON_");
        assert_eq!(safe_folder_stem("nul"), "nul_");
        assert_eq!(safe_folder_stem("com1.txt"), "com1.txt_");
        assert_eq!(safe_folder_stem("Lpt9"), "Lpt9_");
        assert_eq!(safe_folder_stem("COM0"), "COM0");
        assert_eq!(safe_folder_stem("Report. . "), "Report");
        assert_eq!(safe_folder_stem("..."), "Recording");
        assert_eq!(safe_folder_stem("  \t "), "Recording");
        assert_eq!(safe_folder_stem("../../evil"), "evil");
    }

    #[test]
    fn create_makes_layout_and_dedups() {
        let t = TempDir::new();
        let a = Bundle::create(t.path(), "Same", "1", 1).unwrap();
        let b = Bundle::create(t.path(), "Same", "2", 2).unwrap();
        let c = Bundle::create(t.path(), "Same", "3", 3).unwrap();
        assert_eq!(a.path().file_name().unwrap(), "Same.bsrec");
        assert_eq!(b.path().file_name().unwrap(), "Same (2).bsrec");
        assert_eq!(c.path().file_name().unwrap(), "Same (3).bsrec");
        assert!(a.path().join(DB_FILE).is_file());
        assert!(a.path().join(IMAGES_DIR).is_dir());
    }

    #[test]
    fn roundtrip_through_fresh_handle() {
        let t = TempDir::new();
        let path = {
            let b = Bundle::create(t.path(), "My Guide", "rec-1", 1000).unwrap();
            let rec = sample(vec![
                Block::Heading { id: "h".into(), level: 1, text: "Intro".into() },
                step("s1", "First"),
                Block::Text { id: "t".into(), text: "hello **there**".into() },
                step("s2", "Second"),
            ]);
            b.save(&rec).unwrap();
            b.path().to_path_buf()
        };
        let b = Bundle::open(&path).unwrap();
        let rec = b.load().unwrap();
        assert_eq!(rec.blocks.len(), 4);
        assert_eq!(rec.step_count(), 2);
        assert_eq!(rec.id, "rec-1");
        assert_eq!(rec.title, "My Guide");
        assert_eq!(rec.created_unix_ms, 1000);
        assert_eq!(rec.blocks[1], step("s1", "First"));
        // Order is preserved.
        assert!(matches!(&rec.blocks[3], Block::Step(s) if s.id == "s2"));
    }

    #[test]
    fn open_refuses_non_bundles() {
        let t = TempDir::new();
        let plain = t.path().join("plain");
        fs::create_dir(&plain).unwrap();
        assert!(matches!(Bundle::open(&plain), Err(StorageError::NotABundle(_))));
        assert!(matches!(Bundle::open(&t.path().join("missing")), Err(StorageError::NotABundle(_))));
        // A recording.db that is not ours must not be adopted or modified.
        let fake = t.path().join("fake.bsrec");
        fs::create_dir(&fake).unwrap();
        fs::write(fake.join(DB_FILE), b"not a database at all, just text").unwrap();
        assert!(Bundle::open(&fake).is_err());
        assert_eq!(fs::read(fake.join(DB_FILE)).unwrap(), b"not a database at all, just text");
        // Valid sqlite but wrong tables.
        let other = t.path().join("other.bsrec");
        fs::create_dir(&other).unwrap();
        Connection::open(other.join(DB_FILE)).unwrap().execute_batch("CREATE TABLE x(a);").unwrap();
        assert!(matches!(Bundle::open(&other), Err(StorageError::NotABundle(_))));
    }

    #[test]
    fn open_refuses_newer_format() {
        let t = TempDir::new();
        let path = Bundle::create(t.path(), "New", "n", 1).unwrap().path().to_path_buf();
        let conn = Connection::open(path.join(DB_FILE)).unwrap();
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'format_version'",
            params![(FORMAT_VERSION + 1).to_string()],
        )
        .unwrap();
        drop(conn);
        match Bundle::open(&path) {
            Err(StorageError::TooNew { found, supported }) => {
                assert_eq!(found, FORMAT_VERSION + 1);
                assert_eq!(supported, FORMAT_VERSION);
                assert!(StorageError::TooNew { found, supported }.to_string().contains("update"));
            }
            other => panic!("expected TooNew, got {other:?}"),
        }
    }

    #[test]
    fn v1_data_loads_with_defaults() {
        let t = TempDir::new();
        let path = Bundle::create(t.path(), "Old", "o", 1).unwrap().path().to_path_buf();
        let conn = Connection::open(path.join(DB_FILE)).unwrap();
        conn.execute("UPDATE meta SET value = '1' WHERE key = 'format_version'", []).unwrap();
        let json = r#"{"type":"step","id":"s1","action":{"kind":"comment"},"title":"t","description":"","screenshot":null,"target":null,"redactions":[],"captured_unix_ms":1}"#;
        conn.execute("INSERT INTO blocks VALUES (0, 's1', ?1)", params![json]).unwrap();
        drop(conn);
        let rec = Bundle::open(&path).unwrap().load().unwrap();
        assert_eq!(rec.format_version, 1);
        match &rec.blocks[0] {
            Block::Step(s) => assert!(!s.screenshot_withheld),
            _ => panic!("expected step"),
        }
    }

    #[test]
    fn failed_save_leaves_old_document() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Atomic", "a", 1).unwrap();
        let old = sample(vec![step("s1", "old one"), step("s2", "old two")]);
        b.save(&old).unwrap();
        let mut new = sample(vec![step("n1", "new"), step("n2", "new"), step("n3", "new")]);
        new.title = "Changed title".into();
        let err = b.save_impl(&new, Some(2)).unwrap_err();
        assert!(matches!(err, StorageError::Forced));
        let path = b.path().to_path_buf();
        drop(b);
        let loaded = Bundle::open(&path).unwrap().load().unwrap();
        assert_eq!(loaded.blocks, old.blocks);
        assert_eq!(loaded.title, "My Guide");
    }

    #[test]
    fn deleted_text_does_not_linger_in_db_file() {
        const MARKER: &str = "ZQXJ-secret-marker-7731";
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Secure", "s", 1).unwrap();
        let padding = "pad ".repeat(200);
        b.save(&sample(vec![
            Block::Text { id: "t".into(), text: format!("{MARKER} {padding}") },
            step("s1", &format!("{MARKER} step")),
        ]))
        .unwrap();
        let path = b.path().to_path_buf();
        // Check the check: the marker really is in the file before overwrite.
        let before = fs::read(path.join(DB_FILE)).unwrap();
        assert!(before.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()));
        // Overwrite with a document that no longer holds it (frees the pages).
        b.save(&sample(vec![])).unwrap();
        drop(b);
        let after = fs::read(path.join(DB_FILE)).unwrap();
        assert!(
            !after.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()),
            "deleted text is still in recording.db"
        );
        // Also after a fresh handle replaces a step with redacted text.
        let b = Bundle::open(&path).unwrap();
        b.save(&sample(vec![step("s1", &format!("{MARKER} again"))])).unwrap();
        b.save(&sample(vec![step("s1", "[redacted]")])).unwrap();
        drop(b);
        let after = fs::read(path.join(DB_FILE)).unwrap();
        assert!(!after.windows(MARKER.len()).any(|w| w == MARKER.as_bytes()));
    }

    #[test]
    fn closed_bundle_is_a_single_db_file() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Single", "s", 1).unwrap();
        b.save(&sample(vec![step("s1", "x")])).unwrap();
        let path = b.path().to_path_buf();
        drop(b);
        let mut names: Vec<String> = fs::read_dir(&path)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec![IMAGES_DIR.to_string(), DB_FILE.to_string()]);
    }

    #[test]
    fn rename_changes_title_only() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Before", "r", 1).unwrap();
        b.save(&sample(vec![step("s1", "x")])).unwrap();
        b.rename("After").unwrap();
        let path = b.path().to_path_buf();
        drop(b);
        assert_eq!(path.file_name().unwrap(), "Before.bsrec");
        let rec = Bundle::open(&path).unwrap().load().unwrap();
        assert_eq!(rec.title, "After");
        assert_eq!(rec.blocks.len(), 1);
    }

    #[test]
    fn image_write_and_remove() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Img", "i", 1).unwrap();
        let rel = b.write_image("step-001.png", b"first").unwrap();
        assert_eq!(rel, "images/step-001.png");
        assert_eq!(fs::read(b.path().join("images/step-001.png")).unwrap(), b"first");
        // Overwrite works and leaves no temp file behind.
        b.write_image("step-001.png", b"second").unwrap();
        assert_eq!(fs::read(b.path().join(&rel)).unwrap(), b"second");
        let files: Vec<_> = fs::read_dir(b.images_dir()).unwrap().collect();
        assert_eq!(files.len(), 1);
        b.remove_image(&rel).unwrap();
        assert!(!b.path().join(&rel).exists());
        b.remove_image(&rel).unwrap(); // already gone: fine
    }

    #[test]
    fn image_names_are_validated() {
        let t = TempDir::new();
        let b = Bundle::create(t.path(), "Img", "i", 1).unwrap();
        let outside = t.path().join("escape.png");
        for bad in [
            "", "..", "../escape.png", "a/b.png", "a\\b.png", "..\\x.png", "C:\\x.png",
            "C:x.png", "/abs.png", "x.png:stream", "NUL", "con.png", ".hidden", "trail.",
            "a\0b.png",
        ] {
            assert!(b.write_image(bad, b"x").is_err(), "accepted {bad:?}");
        }
        assert!(!outside.exists());
        fs::write(&outside, b"keep").unwrap();
        for bad in ["../escape.png", "images/../../escape.png", "escape.png", "images/a/b.png", "/images/x.png", "images/"] {
            assert!(b.remove_image(bad).is_err(), "accepted {bad:?}");
        }
        assert_eq!(fs::read(&outside).unwrap(), b"keep");
        assert_eq!(fs::read_dir(b.images_dir()).unwrap().count(), 0);
    }

    #[test]
    fn library_lists_and_skips_unreadable() {
        let t = TempDir::new();
        let a = Bundle::create(t.path(), "Alpha", "a", 10).unwrap();
        a.save(&sample(vec![step("s1", "x"), Block::Text { id: "t".into(), text: "y".into() }, step("s2", "z")]))
            .unwrap();
        let b = Bundle::create(t.path(), "Beta", "b", 20).unwrap();
        b.save(&sample(vec![])).unwrap();
        drop((a, b));
        let broken = t.path().join("Broken.bsrec");
        fs::create_dir(&broken).unwrap();
        fs::write(broken.join(DB_FILE), b"garbage").unwrap();
        fs::create_dir(t.path().join("not-a-bundle")).unwrap();
        fs::write(t.path().join("stray.txt"), b"x").unwrap();

        let l = library::list(t.path()).unwrap();
        assert_eq!(l.recordings.len(), 2);
        assert_eq!(l.skipped.len(), 1);
        assert_eq!(l.skipped[0].0, broken);
        let alpha = l.recordings.iter().find(|s| s.title == "My Guide" && s.step_count == 2);
        assert!(alpha.is_some());
        assert!(l.recordings.iter().any(|s| s.step_count == 0));
        assert!(l.recordings.iter().all(|s| s.created == 1000 && s.updated > 0));
    }

    #[test]
    fn delete_refuses_ordinary_folders() {
        let t = TempDir::new();
        // Ordinary folder with a db inside but the wrong name.
        let plain = t.path().join("Documents");
        fs::create_dir(&plain).unwrap();
        fs::write(plain.join(DB_FILE), b"x").unwrap();
        fs::write(plain.join("precious.txt"), b"keep me").unwrap();
        assert!(matches!(delete_bundle(&plain), Err(StorageError::Refused(_))));
        assert_eq!(fs::read(plain.join("precious.txt")).unwrap(), b"keep me");
        // Right name, no db.
        let nodb = t.path().join("Looks.bsrec");
        fs::create_dir(&nodb).unwrap();
        fs::write(nodb.join("precious.txt"), b"keep me").unwrap();
        assert!(matches!(delete_bundle(&nodb), Err(StorageError::Refused(_))));
        assert_eq!(fs::read(nodb.join("precious.txt")).unwrap(), b"keep me");
        // A real bundle is deleted.
        let real = Bundle::create(t.path(), "Real", "r", 1).unwrap();
        let rp = real.path().to_path_buf();
        drop(real);
        delete_bundle(&rp).unwrap();
        assert!(!rp.exists());
    }
}

//! Document model. The document is the source of truth: capture produces it,
//! editing changes it, and exporters render it (see docs/03-architecture.md §4).

use serde::{Deserialize, Serialize};
use std::fmt;

pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub format_version: u32,
    pub id: String,
    pub title: String,
    pub created_unix_ms: u64,
    pub blocks: Vec<Block>,
}

// Debug on the document types prints ids, kinds and counts only. Titles, typed
// text, element names and captions are what a user typed or saw, and `{:?}`
// ends up in logs and panic messages.
impl fmt::Debug for Recording {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recording")
            .field("format_version", &self.format_version)
            .field("id", &self.id)
            .field("blocks", &self.blocks.len())
            .finish_non_exhaustive()
    }
}

impl Recording {
    pub fn new(id: impl Into<String>, title: impl Into<String>, created_unix_ms: u64) -> Self {
        Self {
            format_version: FORMAT_VERSION,
            id: id.into(),
            title: title.into(),
            created_unix_ms,
            blocks: Vec::new(),
        }
    }

    /// Number of step blocks (the numbered items in an export).
    pub fn step_count(&self) -> usize {
        self.blocks.iter().filter(|b| matches!(b, Block::Step(_))).count()
    }
}

/// Rich text decision: `Text.text`, `StepBlock.description`, list items, callout
/// text and image captions hold a restricted inline Markdown string: bold `**`,
/// italic `*`, underline `<u>..</u>`, inline code, and line breaks (a newline character).
/// Step titles and heading text are plain strings with no formatting.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// Heading text is plain (no inline formatting).
    Heading { id: String, level: u8, text: String },
    /// `text` is restricted inline Markdown (see the note on [`Block`]).
    Text { id: String, text: String },
    Step(StepBlock),
    /// Each item is restricted inline Markdown on a single line.
    List { id: String, ordered: bool, items: Vec<String> },
    /// `image` is a path relative to the recording folder, forward slashes.
    /// `caption` is restricted inline Markdown.
    Image { id: String, image: String, caption: String },
    /// A highlighted note; `text` is restricted inline Markdown.
    Callout { id: String, kind: CalloutKind, text: String },
}

impl fmt::Debug for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Block::Heading { id, level, .. } => {
                f.debug_struct("Heading").field("id", id).field("level", level).finish_non_exhaustive()
            }
            Block::Text { id, .. } => f.debug_struct("Text").field("id", id).finish_non_exhaustive(),
            Block::Step(s) => fmt::Debug::fmt(s, f),
            Block::List { id, ordered, items } => {
                f.debug_struct("List").field("id", id).field("ordered", ordered).field("items", &items.len()).finish()
            }
            Block::Image { id, .. } => f.debug_struct("Image").field("id", id).finish_non_exhaustive(),
            Block::Callout { id, kind, .. } => {
                f.debug_struct("Callout").field("id", id).field("kind", kind).finish_non_exhaustive()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalloutKind {
    Note,
    Tip,
    Warning,
}

impl CalloutKind {
    pub fn label(self) -> &'static str {
        match self {
            CalloutKind::Note => "Note",
            CalloutKind::Tip => "Tip",
            CalloutKind::Warning => "Warning",
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct StepBlock {
    pub id: String,
    pub action: Action,
    pub title: String,
    pub description: String,
    /// Path relative to the recording folder, forward slashes (e.g. `images/step-001.png`).
    pub screenshot: Option<String>,
    pub target: Option<Target>,
    /// Regions (in screenshot pixels) to be blurred or blacked out on export.
    pub redactions: Vec<Rect>,
    pub captured_unix_ms: u64,
    /// True when the password check failed and the screenshot was withheld.
    #[serde(default)]
    pub screenshot_withheld: bool,
}

impl fmt::Debug for StepBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StepBlock")
            .field("id", &self.id)
            .field("action", &self.action)
            .field("has_screenshot", &self.screenshot.is_some())
            .field("has_target", &self.target.is_some())
            .field("redactions", &self.redactions.len())
            .field("screenshot_withheld", &self.screenshot_withheld)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Click { button: MouseButton, x: i32, y: i32 },
    Text { text: String },
    Shortcut { keys: String },
    Comment,
    Manual,
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Action::Click { .. } => "Action::Click",
            Action::Text { .. } => "Action::Text",
            Action::Shortcut { .. } => "Action::Shortcut",
            Action::Comment => "Action::Comment",
            Action::Manual => "Action::Manual",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn label(self) -> &'static str {
        match self {
            MouseButton::Left => "Left-click",
            MouseButton::Right => "Right-click",
            MouseButton::Middle => "Middle-click",
        }
    }
}

/// The UI element a step acted on, as reported by the platform accessibility API.
#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Target {
    pub name: String,
    pub control_type: String,
    /// Only filled when the user opts in. Never used in step titles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    pub process_name: String,
    pub is_password: bool,
    pub bounds: Option<Rect>,
}

/// Names, titles and process names are not printed; see the note on `Recording`.
impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("is_password", &self.is_password)
            .field("has_bounds", &self.bounds.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Build the human-readable step title from a click and what was under it.
/// Examples: `Left-click "Save" button in Notepad`, `Left-click at (120, 340)`.
pub fn describe_click(button: MouseButton, x: i32, y: i32, target: Option<&Target>) -> String {
    let verb = button.label();
    match target {
        Some(t) if !t.name.trim().is_empty() => {
            let ctl = t.control_type.trim().to_lowercase();
            // Privacy: app name only, never the window title (it can hold
            // document names or other private text).
            let app = app_name(&t.process_name);
            let mut s = format!("{verb} \"{}\"", t.name.trim());
            if !ctl.is_empty() {
                s.push(' ');
                s.push_str(&ctl);
            }
            if !app.is_empty() {
                s.push_str(" in ");
                s.push_str(&app);
            }
            s
        }
        Some(t) if !t.control_type.trim().is_empty() => {
            format!("{verb} {} at ({x}, {y})", t.control_type.trim().to_lowercase())
        }
        _ => format!("{verb} at ({x}, {y})"),
    }
}

/// Process name without a trailing `.exe` (case-insensitive).
fn app_name(process_name: &str) -> String {
    let p = process_name.trim();
    let n = p.len();
    if n >= 4 && p.is_char_boundary(n - 4) && p[n - 4..].eq_ignore_ascii_case(".exe") {
        p[..n - 4].trim_end().to_string()
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_content() {
        const M: &str = "MARKER-hunter2";
        let m = |field: &str| format!("{M}-{field}");
        let target = Target {
            name: m("name"),
            control_type: m("control"),
            window_title: Some(m("window")),
            process_name: m("process"),
            is_password: false,
            bounds: Some(Rect { x: 1, y: 2, w: 3, h: 4 }),
        };
        let step = |action| StepBlock {
            id: "s1".into(),
            action,
            title: m("title"),
            description: m("description"),
            screenshot: Some(format!("images/{}.png", m("shot"))),
            target: Some(target.clone()),
            redactions: vec![Rect { x: 1, y: 2, w: 3, h: 4 }],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        };
        let mut rec = Recording::new("rec1", m("rectitle"), 0);
        rec.blocks = vec![
            Block::Heading { id: "h".into(), level: 1, text: m("heading") },
            Block::Text { id: "t".into(), text: m("text") },
            Block::Step(step(Action::Text { text: m("typed") })),
            Block::Step(step(Action::Shortcut { keys: m("keys") })),
            Block::Step(step(Action::Click { button: MouseButton::Left, x: 1, y: 2 })),
            Block::Step(step(Action::Comment)),
            Block::Step(step(Action::Manual)),
            Block::List { id: "l".into(), ordered: true, items: vec![m("item")] },
            Block::Image { id: "i".into(), image: m("image"), caption: m("caption") },
            Block::Callout { id: "c".into(), kind: CalloutKind::Tip, text: m("callout") },
        ];
        let shown = [
            format!("{rec:?}"),
            format!("{:#?}", rec),
            format!("{:?}", rec.blocks),
            format!("{:?}", target),
            format!("{:?}", Action::Text { text: m("typed") }),
        ];
        for s in &shown {
            assert!(!s.contains("MARKER") && !s.contains("hunter2"), "{s}");
        }
        // The control: the marker really is in the data.
        assert!(serde_json::to_string(&rec).unwrap().contains("MARKER-hunter2-heading"));
        assert!(shown[0].contains("rec1"));
    }
}

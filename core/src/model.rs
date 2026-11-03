//! Document model. The document is the source of truth: capture produces it,
//! editing changes it, and exporters render it (see docs/03-architecture.md §4).

use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub format_version: u32,
    pub id: String,
    pub title: String,
    pub created_unix_ms: u64,
    pub blocks: Vec<Block>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Click { button: MouseButton, x: i32, y: i32 },
    Text { text: String },
    Shortcut { keys: String },
    Comment,
    Manual,
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
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
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

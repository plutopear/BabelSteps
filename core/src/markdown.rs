//! Markdown exporter. Output is `document.md` plus an `images/` folder, using
//! relative image links so it pastes cleanly into ticketing and wiki tools.

use crate::model::{Block, Recording};

/// Escape characters that would be interpreted as Markdown inline syntax.
pub fn escape_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|' | '~' => {
                out.push('\\');
                out.push(ch);
            }
            // Collapse any line breaks: titles must stay on one line.
            '\r' | '\n' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape a block of free text (paragraph). Keeps line breaks, escapes leading
/// block markers such as `#`, `-`, `+`, `>` and `1.`.
fn escape_paragraph(s: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let needs_lead_escape = trimmed.starts_with('#')
                || trimmed.starts_with("- ")
                || trimmed.starts_with("+ ")
                || trimmed.starts_with("> ")
                || trimmed
                    .split_once(". ")
                    .map(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false);
            let esc = escape_inline(line);
            if needs_lead_escape {
                format!("\\{esc}")
            } else {
                esc
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn to_markdown(rec: &Recording) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n", escape_inline(&rec.title)));

    let mut step_no = 0usize;
    for block in &rec.blocks {
        out.push('\n');
        match block {
            Block::Heading { level, text, .. } => {
                let lvl = (*level).clamp(1, 6) as usize;
                out.push_str(&format!("{} {}\n", "#".repeat(lvl), escape_inline(text)));
            }
            Block::Text { text, .. } => {
                out.push_str(&escape_paragraph(text));
                out.push('\n');
            }
            Block::Step(step) => {
                step_no += 1;
                out.push_str(&format!("## Step {step_no}. {}\n", escape_inline(&step.title)));
                if !step.description.trim().is_empty() {
                    out.push('\n');
                    out.push_str(&escape_paragraph(&step.description));
                    out.push('\n');
                }
                if let Some(img) = &step.screenshot {
                    out.push('\n');
                    out.push_str(&format!("![Step {step_no}]({})\n", img.replace(' ', "%20")));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn step(id: &str, title: &str, desc: &str, img: Option<&str>) -> Block {
        Block::Step(StepBlock {
            id: id.into(),
            action: Action::Manual,
            title: title.into(),
            description: desc.into(),
            screenshot: img.map(String::from),
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
        })
    }

    #[test]
    fn golden_export() {
        let mut rec = Recording::new("r1", "Reset a password", 0);
        rec.blocks.push(Block::Text { id: "t1".into(), text: "Use this guide to reset a password.".into() });
        rec.blocks.push(step("s1", "Left-click \"Save\" button in Notepad", "", Some("images/step-001.png")));
        rec.blocks.push(step("s2", "Type text", "Enter the user name.", None));
        let md = to_markdown(&rec);
        let expected = "\
# Reset a password

Use this guide to reset a password.

## Step 1. Left-click \"Save\" button in Notepad

![Step 1](images/step-001.png)

## Step 2. Type text

Enter the user name.
";
        assert_eq!(md, expected);
    }

    #[test]
    fn escapes_inline_syntax_and_newlines() {
        assert_eq!(escape_inline("a*b_c[d]`e`"), "a\\*b\\_c\\[d\\]\\`e\\`");
        assert_eq!(escape_inline("line1\nline2"), "line1 line2");
    }

    #[test]
    fn escapes_leading_block_markers_in_paragraphs() {
        assert_eq!(escape_paragraph("# not a heading"), "\\# not a heading");
        assert_eq!(escape_paragraph("1. not a list"), "\\1. not a list");
        assert_eq!(escape_paragraph("- not a bullet"), "\\- not a bullet");
        assert_eq!(escape_paragraph("plain text"), "plain text");
    }

    #[test]
    fn image_paths_with_spaces_are_encoded() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(step("s", "x", "", Some("images/a b.png")));
        assert!(to_markdown(&rec).contains("(images/a%20b.png)"));
    }

    #[test]
    fn recording_round_trips_through_json() {
        let mut rec = Recording::new("r", "t", 42);
        rec.blocks.push(Block::Step(StepBlock {
            id: "s".into(),
            action: Action::Click { button: MouseButton::Left, x: 10, y: 20 },
            title: "x".into(),
            description: String::new(),
            screenshot: Some("images/step-001.png".into()),
            target: Some(Target { name: "OK".into(), control_type: "Button".into(), ..Default::default() }),
            redactions: vec![Rect { x: 1, y: 2, w: 3, h: 4 }],
            captured_unix_ms: 7,
        }));
        rec.blocks.push(Block::Heading { id: "h".into(), level: 2, text: "H".into() });
        let json = serde_json::to_string(&rec).unwrap();
        let back: Recording = serde_json::from_str(&json).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn click_descriptions() {
        let t = Target { name: "Save".into(), control_type: "Button".into(), window_title: "Untitled - Notepad".into(), ..Default::default() };
        assert_eq!(
            describe_click(MouseButton::Left, 1, 2, Some(&t)),
            "Left-click \"Save\" button in Untitled - Notepad"
        );
        assert_eq!(describe_click(MouseButton::Right, 5, 6, None), "Right-click at (5, 6)");
        let unnamed = Target { control_type: "Pane".into(), ..Default::default() };
        assert_eq!(describe_click(MouseButton::Left, 5, 6, Some(&unnamed)), "Left-click pane at (5, 6)");
    }
}

//! Markdown exporter. Output is `document.md` plus an `images/` folder, using
//! relative image links so it pastes cleanly into ticketing and wiki tools.
//!
//! Plain fields (document title, heading text, step titles) are escaped.
//! Restricted-Markdown fields (text, descriptions, list items, callout text,
//! captions) are emitted as written, except that a line which would change
//! document structure (`# `, `- `, `1. `, `> `, ...) is backslash-guarded.
//! `markdown_import` reverses this for the round trip.

use crate::export_options::{
    recorded_on_line, step_time, technical_details, ExportOptions, DETAILS_HEADING, DETAILS_STEPS_HEADING, TIME_PREFIX,
};
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

/// Sentence shown in place of a screenshot that was withheld.
pub const WITHHELD_SENTENCE: &str = "Screenshot withheld: could not check for passwords.";

/// Alt text of an image block (a step screenshot uses `Step N`).
pub(crate) const IMAGE_ALT: &str = "Image";

/// Marks the end of a step that has no description or screenshot, so that a
/// following text block is not read back as its description.
pub(crate) const STEP_END_MARK: &str = "<!-- -->";

/// Plain field (title, heading): inline-escaped, one line, trimmed.
fn plain(s: &str) -> String {
    escape_inline(s).trim().to_string()
}

/// Heading text must not look like a step heading (`Step 3. ...`) on import.
fn escape_heading_text(s: &str) -> String {
    let e = plain(s);
    if let Some(rest) = e.strip_prefix("Step ") {
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 {
            let after = &rest[digits..];
            if after == "." || after.starts_with(". ") {
                return format!("Step {}\\{}", &rest[..digits], after);
            }
        }
    }
    e
}

/// Would this (trimmed, non-empty) line start a block structure if left bare?
pub(crate) fn needs_guard(line: &str) -> bool {
    let mut chars = line.chars();
    let Some(first) = chars.next() else { return false };
    if first == '#' || first == '>' {
        return true;
    }
    if line.starts_with("![") || line.starts_with("<!--") || line.starts_with("```") || line.starts_with("~~~") {
        return true;
    }
    if matches!(first, '-' | '+' | '*') && matches!(chars.next(), None | Some(' ') | Some('\t')) {
        return true;
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 {
        let rest = &line[digits..];
        if let Some(r) = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')) {
            if r.is_empty() || r.starts_with(' ') {
                return true;
            }
        }
    }
    // Thematic break or setext underline: one of - * _ = repeated 3+ times.
    if matches!(first, '-' | '*' | '_' | '=') {
        let marks: Vec<char> = line.chars().filter(|c| *c != ' ').collect();
        if marks.len() >= 3 && marks.iter().all(|c| *c == first) {
            return true;
        }
    }
    false
}

/// Neutralise a line that would change structure, using backslash escapes that
/// every Markdown tool understands. `unguard_line` reverses this on import.
pub(crate) fn guard_line(line: &str) -> String {
    if !needs_guard(line) {
        return line.to_string();
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        format!("{}\\{}", &line[..digits], &line[digits..])
    } else {
        format!("\\{line}")
    }
}

pub(crate) fn unguard_line(line: &str) -> String {
    if let Some(cand) = line.strip_prefix('\\') {
        if needs_guard(cand) {
            return cand.to_string();
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 {
        if let Some(rest) = line[digits..].strip_prefix('\\') {
            let cand = format!("{}{}", &line[..digits], rest);
            if needs_guard(&cand) {
                return cand;
            }
        }
    }
    line.to_string()
}

/// Non-blank, trimmed lines. Blank lines are dropped so one text value always
/// stays one paragraph.
fn text_lines(s: &str) -> Vec<&str> {
    s.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// Restricted-Markdown paragraph: lines joined with hard breaks, structure guarded.
fn paragraph(s: &str) -> String {
    text_lines(s).into_iter().map(guard_line).collect::<Vec<_>>().join("  \n")
}

/// Single-line restricted Markdown (list items): line breaks become spaces.
fn one_line(s: &str) -> String {
    guard_line(&text_lines(s).join(" "))
}

/// Percent-encode the characters that break a Markdown link destination.
pub(crate) fn encode_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for ch in p.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '%' => out.push_str("%25"),
            _ => out.push(ch),
        }
    }
    out
}

/// Blocks that render to nothing.
fn is_empty_block(b: &Block) -> bool {
    match b {
        Block::Text { text, .. } => text_lines(text).is_empty(),
        Block::List { items, .. } => items.is_empty(),
        _ => false,
    }
}

pub fn to_markdown(rec: &Recording) -> String {
    to_markdown_with(rec, &ExportOptions::default())
}

/// Markdown with the export options applied (step times, technical details).
pub fn to_markdown_with(rec: &Recording, opts: &ExportOptions) -> String {
    let mut out = format!("# {}\n", plain(&rec.title)).replace("# \n", "#\n");
    if let Some(line) = recorded_on_line(rec, opts) {
        out.push('\n');
        out.push_str(&guard_line(&escape_inline(&line)));
        out.push('\n');
    }

    let mut step_no = 0usize;
    for (i, block) in rec.blocks.iter().enumerate() {
        let rendered: Option<String> = match block {
            Block::Heading { level, text, .. } => {
                let lvl = (*level).clamp(1, 6) as usize;
                Some(format!("{} {}", "#".repeat(lvl), escape_heading_text(text)).trim_end().to_string())
            }
            Block::Text { text, .. } => (!is_empty_block(block)).then(|| paragraph(text)),
            Block::Step(step) => {
                step_no += 1;
                let mut parts = vec![format!("## Step {step_no}. {}", plain(&step.title)).trim_end().to_string()];
                if let Some(t) = step_time(rec, step, opts) {
                    // Markdown import recognises and drops this line (`is_step_time_line`).
                    parts.push(format!("*{TIME_PREFIX}{t}*"));
                }
                let desc = paragraph(&step.description);
                let has_desc = !desc.is_empty();
                if has_desc {
                    parts.push(desc);
                }
                let mut has_shot = false;
                if step.screenshot_withheld {
                    // Privacy: never emit the image of a withheld step.
                    parts.push(format!("*{WITHHELD_SENTENCE}*"));
                    has_shot = true;
                } else if let Some(img) = &step.screenshot {
                    parts.push(format!("![Step {step_no}]({})", encode_path(img)));
                    has_shot = true;
                }
                if !has_desc && !has_shot {
                    let next = rec.blocks[i + 1..].iter().find(|b| !is_empty_block(b));
                    if matches!(next, Some(Block::Text { .. })) {
                        parts.push(STEP_END_MARK.to_string());
                    }
                }
                Some(parts.join("\n\n"))
            }
            Block::List { ordered, items, .. } => (!items.is_empty()).then(|| {
                items
                    .iter()
                    .enumerate()
                    .map(|(n, it)| {
                        let marker = if *ordered { format!("{}.", n + 1) } else { "-".to_string() };
                        format!("{marker} {}", one_line(it)).trim_end().to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
            Block::Image { image, caption, .. } => {
                let mut s = format!("![{IMAGE_ALT}]({})", encode_path(image));
                let cap = paragraph(caption);
                if !cap.is_empty() {
                    s.push_str("  \n");
                    s.push_str(&cap);
                }
                Some(s)
            }
            Block::Callout { kind, text, .. } => {
                let lines = text_lines(text);
                let mut v = vec![format!("> **{}:** {}", kind.label(), lines.first().copied().unwrap_or("")).trim_end().to_string()];
                for l in lines.iter().skip(1) {
                    v.push(format!("> {}", guard_line(l)));
                }
                // Hard breaks between callout lines.
                let last = v.len() - 1;
                let joined: Vec<String> = v.into_iter().enumerate().map(|(n, l)| if n < last { format!("{l}  ") } else { l }).collect();
                Some(joined.join("\n"))
            }
        };
        if let Some(r) = rendered {
            out.push('\n');
            out.push_str(&r);
            out.push('\n');
        }
    }
    if let Some(d) = technical_details(rec, opts) {
        out.push_str(&format!("\n## {DETAILS_HEADING}\n\n"));
        for (label, value) in &d.facts {
            out.push_str(&format!("- {label}: {}\n", escape_inline(value).trim()));
        }
        if !d.steps.is_empty() {
            out.push_str(&format!("\n### {DETAILS_STEPS_HEADING}\n\n"));
            for (n, line) in &d.steps {
                out.push_str(&format!("- Step {n}: {}\n", escape_inline(line).trim()));
            }
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
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
            screenshot_withheld: false,
        })
    }

    /// One of every block kind, used by the golden and round-trip tests.
    pub(crate) fn full_fixture() -> Recording {
        let mut rec = Recording::new("r1", "Reset a password", 0);
        rec.blocks.push(Block::Heading { id: "h1".into(), level: 2, text: "Before you start".into() });
        rec.blocks.push(Block::Text { id: "t1".into(), text: "Use this guide to **reset** a password.\nIt takes <u>two</u> minutes.".into() });
        rec.blocks.push(Block::List { id: "l1".into(), ordered: false, items: vec!["Admin rights".into(), "A `token`".into()] });
        rec.blocks.push(Block::List { id: "l2".into(), ordered: true, items: vec!["Open *Settings*".into(), "Choose Reset".into()] });
        rec.blocks.push(step("s1", "Left-click \"Save\" button in Notepad", "", Some("images/step-001.png")));
        rec.blocks.push(step("s2", "Type text", "Enter the user name.", None));
        rec.blocks.push(Block::Step(StepBlock {
            screenshot_withheld: true,
            ..match step("s3", "Type password", "", None) {
                Block::Step(s) => s,
                _ => unreachable!(),
            }
        }));
        rec.blocks.push(Block::Image { id: "i1".into(), image: "images/extra (1).png".into(), caption: "The *final* dialog".into() });
        rec.blocks.push(Block::Callout { id: "c1".into(), kind: CalloutKind::Note, text: "Keep this window open.".into() });
        rec.blocks.push(Block::Callout { id: "c2".into(), kind: CalloutKind::Tip, text: "Use Tab to move.\nShift+Tab goes back.".into() });
        rec.blocks.push(Block::Callout { id: "c3".into(), kind: CalloutKind::Warning, text: "Do not share it.".into() });
        rec
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
    fn golden_export_every_block_kind() {
        let expected = "\
# Reset a password

## Before you start

Use this guide to **reset** a password.<BR>
It takes <u>two</u> minutes.

- Admin rights
- A `token`

1. Open *Settings*
2. Choose Reset

## Step 1. Left-click \"Save\" button in Notepad

![Step 1](images/step-001.png)

## Step 2. Type text

Enter the user name.

## Step 3. Type password

*Screenshot withheld: could not check for passwords.*

![Image](images/extra%20%281%29.png)<BR>
The *final* dialog

> **Note:** Keep this window open.

> **Tip:** Use Tab to move.<BR>
> Shift+Tab goes back.

> **Warning:** Do not share it.
";
        // <BR> stands for the two trailing spaces of a hard break.
        assert_eq!(to_markdown(&full_fixture()), expected.replace("<BR>
", "  
"));
    }

    #[test]
    fn withheld_step_never_links_its_image() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(Block::Step(StepBlock {
            screenshot: Some("images/secret.png".into()),
            screenshot_withheld: true,
            ..match step("s", "x", "", None) {
                Block::Step(s) => s,
                _ => unreachable!(),
            }
        }));
        let md = to_markdown(&rec);
        assert!(!md.contains("secret.png"));
        assert!(md.contains(WITHHELD_SENTENCE));
    }

    #[test]
    fn escapes_inline_syntax_and_newlines() {
        assert_eq!(escape_inline("a*b_c[d]`e`"), "a\\*b\\_c\\[d\\]\\`e\\`");
        assert_eq!(escape_inline("line1\nline2"), "line1 line2");
    }

    #[test]
    fn guards_leading_block_markers_in_paragraphs() {
        assert_eq!(paragraph("# not a heading"), "\\# not a heading");
        assert_eq!(paragraph("1. not a list"), "1\\. not a list");
        assert_eq!(paragraph("- not a bullet"), "\\- not a bullet");
        assert_eq!(paragraph("> quote"), "\\> quote");
        assert_eq!(paragraph("---"), "\\---");
        assert_eq!(paragraph("![x](y)"), "\\![x](y)");
        // Inline formatting and plain text are left alone.
        assert_eq!(paragraph("plain text"), "plain text");
        assert_eq!(paragraph("**bold** and 1.5 and -x"), "**bold** and 1.5 and -x");
        assert_eq!(paragraph("a\n\n  b  "), "a  \nb");
        for l in ["# x", "1. x", "- x", "+ x", "* x", "> x", "---", "```", "![a](b)", "<!-- -->", "7) y", "-"] {
            assert_eq!(unguard_line(&guard_line(l)), l, "{l}");
            assert_ne!(guard_line(l), l, "{l}");
        }
    }

    #[test]
    fn image_paths_are_encoded() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(step("s", "x", "", Some("images/a b (1).png")));
        assert!(to_markdown(&rec).contains("(images/a%20b%20%281%29.png)"));
    }

    #[test]
    fn recording_round_trips_through_json() {
        let mut rec = full_fixture();
        rec.blocks.push(Block::Step(StepBlock {
            id: "s".into(),
            action: Action::Click { button: MouseButton::Left, x: 10, y: 20 },
            title: "x".into(),
            description: String::new(),
            screenshot: Some("images/step-001.png".into()),
            target: Some(Target { name: "OK".into(), control_type: "Button".into(), ..Default::default() }),
            redactions: vec![Rect { x: 1, y: 2, w: 3, h: 4 }],
            captured_unix_ms: 7,
            screenshot_withheld: false,
        }));
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("window_title"), "None window title must not be serialised");
        let back: Recording = serde_json::from_str(&json).unwrap();
        assert_eq!(rec, back);
    }

    #[test]
    fn old_step_json_without_withheld_flag_still_loads() {
        let json = r#"{"type":"step","id":"s","action":{"kind":"manual"},"title":"t","description":"","screenshot":null,"target":null,"redactions":[],"captured_unix_ms":0}"#;
        let b: Block = serde_json::from_str(json).unwrap();
        match b {
            Block::Step(s) => assert!(!s.screenshot_withheld),
            _ => panic!("not a step"),
        }
    }

    #[test]
    fn click_descriptions_use_app_name_never_window_title() {
        let t = Target {
            name: "Save".into(),
            control_type: "Button".into(),
            window_title: Some("Secret plans - Notepad".into()),
            process_name: "notepad.EXE".into(),
            ..Default::default()
        };
        let d = describe_click(MouseButton::Left, 1, 2, Some(&t), false);
        assert_eq!(d, "Left-click \"Save\" button in notepad");
        assert!(!d.contains("Secret"));
        // Title present but no process name: no app part at all.
        let t2 = Target { window_title: Some("Secret".into()), ..Target { name: "Save".into(), ..Default::default() } };
        assert_eq!(describe_click(MouseButton::Left, 1, 2, Some(&t2), false), "Left-click \"Save\"");
        assert_eq!(describe_click(MouseButton::Right, 5, 6, None, false), "Right-click at (5, 6)");
        let unnamed = Target { control_type: "Pane".into(), ..Default::default() };
        assert_eq!(describe_click(MouseButton::Left, 5, 6, Some(&unnamed), false), "Left-click pane at (5, 6)");
    }

    #[test]
    fn click_descriptions_name_the_window_only_with_the_opt_in() {
        let t = Target {
            name: "Save".into(),
            control_type: "Button".into(),
            window_title: Some("Secret plans - Notepad".into()),
            process_name: "notepad.EXE".into(),
            ..Default::default()
        };
        // Off: exactly the default wording, title or not.
        assert_eq!(describe_click(MouseButton::Left, 1, 2, Some(&t), false), "Left-click \"Save\" button in notepad");
        // On: the window, without repeating an app the title already names.
        assert_eq!(describe_click(MouseButton::Left, 1, 2, Some(&t), true), "Left-click \"Save\" button in \"Secret plans - Notepad\"");
        // On, and the title does not name the app: the app follows in brackets.
        let ex = Target { window_title: Some("Downloads".into()), process_name: "explorer.exe".into(), ..t.clone() };
        assert_eq!(describe_click(MouseButton::Right, 1, 2, Some(&ex), true), "Right-click \"Save\" button in \"Downloads\" (explorer)");
        // On, but no title was read (or it was blank): the app name, as before.
        for blank in [None, Some("   ".to_string())] {
            let nt = Target { window_title: blank, ..t.clone() };
            assert_eq!(describe_click(MouseButton::Left, 1, 2, Some(&nt), true), "Left-click \"Save\" button in notepad");
        }
        // On, unnamed element: coordinates plus the window.
        let pane = Target { name: String::new(), control_type: "Title bar".into(), ..t.clone() };
        assert_eq!(describe_click(MouseButton::Left, 5, 6, Some(&pane), true), "Left-click title bar at (5, 6) in \"Secret plans - Notepad\"");
        assert_eq!(describe_click(MouseButton::Left, 5, 6, Some(&pane), false), "Left-click title bar at (5, 6)");
        // No target: nothing to name.
        assert_eq!(describe_click(MouseButton::Middle, 5, 6, None, true), "Middle-click at (5, 6)");
    }
}

//! Markdown importer for the round trip required by B6: read back a document
//! that `to_markdown` produced (possibly edited by hand) into a `Recording`.
//!
//! Hand-written, no parser crate. It understands exactly the shapes the
//! exporter writes: `# Title`, `## Step N. Title` with an optional description
//! paragraph and a screenshot (or the withheld sentence), other headings,
//! paragraphs, ordered and unordered lists, images, and `> **Note:**` callouts.
//!
//! Guarantee: `to_markdown(from_markdown(md)) == md` when `md` came from
//! `to_markdown`. Block kinds and their order survive.
//!
//! Lost on import, because Markdown does not carry it: block ids (replaced by
//! `b1`, `b2`, ...), click coordinates and buttons (every step becomes
//! `Action::Manual`), UI targets, redaction rectangles, timestamps, the
//! recording id, step times (the optional `*Time: ...*` line under a step
//! heading is dropped), and blocks that render to nothing (empty text, empty lists).
//! A withheld step keeps `screenshot_withheld` but has no image path.
//! Adjacent lists of the same kind stay separate here, but other Markdown
//! tools may merge them into one.

use std::collections::VecDeque;
use std::ops::Range;

use crate::export_options::is_step_time_line;
use crate::markdown::{unguard_line, IMAGE_ALT, STEP_END_MARK, WITHHELD_SENTENCE};
use crate::model::{Action, Block, CalloutKind, Recording, StepBlock};

/// Remove backslash escapes added by `escape_inline`.
fn unescape_plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(&n) = it.peek() {
                if n.is_ascii_punctuation() {
                    out.push(n);
                    it.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            // from_str_radix would also accept a leading '+', so check the digits first.
            let hex = s.get(i + 1..i + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit()));
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `# ` to `###### ` heading: level and trimmed text.
fn heading_of(line: &str) -> Option<(usize, &str)> {
    let n = line.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let rest = &line[n..];
    (rest.is_empty() || rest.starts_with(' ')).then(|| (n, rest.trim()))
}

/// `Step 3. Title` (raw, still escaped) to the title part.
fn step_title(heading_text: &str) -> Option<&str> {
    let rest = heading_text.strip_prefix("Step ")?;
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let after = rest[digits..].strip_prefix('.')?;
    (after.is_empty() || after.starts_with(' ')).then(|| after.trim())
}

/// Returns (ordered, item text) for a list line.
fn list_marker(line: &str) -> Option<(bool, &str)> {
    let mut chars = line.chars();
    let first = chars.next()?;
    if matches!(first, '-' | '+' | '*') {
        let rest = &line[1..];
        return (rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t')).then(|| (false, rest.trim()));
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = line[digits..].strip_prefix('.').or_else(|| line[digits..].strip_prefix(')'))?;
    (rest.is_empty() || rest.starts_with(' ')).then(|| (true, rest.trim()))
}

/// `![alt](path)` occupying the whole line: (alt, decoded path).
fn image_of(line: &str) -> Option<(&str, String)> {
    let inner = line.strip_prefix("![")?;
    let close = inner.find("](")?;
    let alt = &inner[..close];
    let path = inner[close + 2..].strip_suffix(')')?;
    Some((alt, percent_decode(path)))
}

fn is_step_alt(alt: &str) -> bool {
    alt.strip_prefix("Step ").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

fn join_unguarded<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    lines.map(unguard_line).collect::<Vec<_>>().join("\n")
}

pub fn from_markdown(md: &str) -> Recording {
    // Split into chunks separated by blank lines; lines are trimmed.
    // All lines live in one vector; a chunk is a range of it, so putting the
    // tail of a chunk back is O(1) rather than a copy.
    let mut lines: Vec<String> = Vec::new();
    let mut chunks: VecDeque<Range<usize>> = VecDeque::new();
    let mut start = 0usize;
    for line in md.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let t = line.trim();
        if t.is_empty() {
            if lines.len() > start {
                chunks.push_back(start..lines.len());
                start = lines.len();
            }
        } else {
            lines.push(t.to_string());
        }
    }
    if lines.len() > start {
        chunks.push_back(start..lines.len());
    }

    let mut title: Option<String> = None;
    let mut blocks: Vec<Block> = Vec::new();
    // Some(has_description) while the last block is a step still taking content.
    let mut open: Option<bool> = None;
    let mut next_id = 0usize;
    let mut new_id = || {
        next_id += 1;
        format!("b{next_id}")
    };

    while let Some(range) = chunks.pop_front() {
        let chunk: &[String] = &lines[range.clone()];
        let first = chunk[0].as_str();

        // Headings are single lines; anything after one is a new chunk.
        if let Some((lvl, rest)) = heading_of(first) {
            requeue(&mut chunks, range.start + 1..range.end);
            open = None;
            if lvl == 1 && title.is_none() {
                title = Some(unescape_plain(rest));
            } else if let (2, Some(t)) = (lvl, step_title(rest)) {
                blocks.push(Block::Step(StepBlock {
                    id: new_id(),
                    action: Action::Manual,
                    title: unescape_plain(t),
                    description: String::new(),
                    screenshot: None,
                    target: None,
                    redactions: vec![],
                    captured_unix_ms: 0,
                    screenshot_withheld: false,
                }));
                open = Some(false);
            } else {
                blocks.push(Block::Heading { id: new_id(), level: lvl as u8, text: unescape_plain(rest) });
            }
            continue;
        }

        // The optional step time (`*Time: 14:05:09*`) right under a step heading.
        // Dropped: without the time zone it cannot be turned back into a timestamp.
        if open == Some(false) && is_step_time_line(first) {
            requeue(&mut chunks, range.start + 1..range.end);
            continue;
        }

        if first == STEP_END_MARK {
            requeue(&mut chunks, range.start + 1..range.end);
            open = None;
            continue;
        }

        if first.starts_with('>') {
            let stripped: Vec<&str> = chunk.iter().map(|l| l.strip_prefix('>').unwrap_or(l).trim()).collect();
            open = None;
            let callout = [CalloutKind::Note, CalloutKind::Tip, CalloutKind::Warning].into_iter().find_map(|k| {
                stripped[0].strip_prefix(&format!("**{}:**", k.label())).map(|rest| (k, rest.trim()))
            });
            match callout {
                Some((kind, rest)) => {
                    let mut lines = vec![rest.to_string()];
                    lines.extend(stripped[1..].iter().map(|l| unguard_line(l)));
                    let text = lines.into_iter().filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
                    blocks.push(Block::Callout { id: new_id(), kind, text });
                }
                None => blocks.push(Block::Text { id: new_id(), text: join_unguarded(stripped.into_iter()) }),
            }
            continue;
        }

        if let Some((ordered, _)) = list_marker(first) {
            open = None;
            let mut items = Vec::new();
            let mut used = 0;
            for l in chunk {
                match list_marker(l) {
                    Some((o, text)) if o == ordered => {
                        items.push(unguard_line(text));
                        used += 1;
                    }
                    _ => break,
                }
            }
            requeue(&mut chunks, range.start + used..range.end);
            blocks.push(Block::List { id: new_id(), ordered, items });
            continue;
        }

        if let Some((alt, path)) = image_of(first) {
            if open.is_some() && is_step_alt(alt) {
                if let Some(Block::Step(s)) = blocks.last_mut() {
                    s.screenshot = Some(path);
                }
                requeue(&mut chunks, range.start + 1..range.end);
                open = None;
            } else {
                open = None;
                let _ = IMAGE_ALT; // alt text is fixed on export; ignored here
                let caption = join_unguarded(chunk[1..].iter().map(String::as_str));
                blocks.push(Block::Image { id: new_id(), image: path, caption });
            }
            continue;
        }

        if open.is_some() && chunk.len() == 1 && first == format!("*{WITHHELD_SENTENCE}*") {
            if let Some(Block::Step(s)) = blocks.last_mut() {
                s.screenshot_withheld = true;
            }
            open = None;
            continue;
        }

        // Plain paragraph.
        let text = join_unguarded(chunk.iter().map(String::as_str));
        match open {
            Some(false) => {
                if let Some(Block::Step(s)) = blocks.last_mut() {
                    s.description = text;
                }
                open = Some(true);
            }
            _ => {
                open = None;
                blocks.push(Block::Text { id: new_id(), text });
            }
        }
    }

    let mut rec = Recording::new("imported", title.unwrap_or_default(), 0);
    rec.blocks = blocks;
    rec
}

fn requeue(q: &mut VecDeque<Range<usize>>, rest: Range<usize>) {
    if !rest.is_empty() {
        q.push_front(rest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::to_markdown;

    fn kinds(rec: &Recording) -> Vec<&'static str> {
        rec.blocks
            .iter()
            .map(|b| match b {
                Block::Heading { .. } => "heading",
                Block::Text { .. } => "text",
                Block::Step(_) => "step",
                Block::List { ordered: true, .. } => "ol",
                Block::List { .. } => "ul",
                Block::Image { .. } => "image",
                Block::Callout { .. } => "callout",
            })
            .collect()
    }

    fn step(title: &str, desc: &str, shot: Option<&str>) -> Block {
        Block::Step(StepBlock {
            id: "s".into(),
            action: Action::Manual,
            title: title.into(),
            description: desc.into(),
            screenshot: shot.map(String::from),
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        })
    }

    #[test]
    fn full_fixture_round_trips() {
        let rec = crate::markdown::tests::full_fixture();
        let md = to_markdown(&rec);
        let back = from_markdown(&md);
        assert_eq!(to_markdown(&back), md);
        assert_eq!(kinds(&back), kinds(&rec));
        assert_eq!(back.title, "Reset a password");
        // Content, not just shape.
        match &back.blocks[6] {
            Block::Step(s) => {
                assert!(s.screenshot_withheld);
                assert_eq!(s.screenshot, None);
            }
            _ => panic!(),
        }
        match &back.blocks[7] {
            Block::Image { image, caption, .. } => {
                assert_eq!(image, "images/extra (1).png");
                assert_eq!(caption, "The *final* dialog");
            }
            _ => panic!(),
        }
        match &back.blocks[3] {
            Block::List { ordered, items, .. } => assert!(*ordered && items == &["Open *Settings*", "Choose Reset"]),
            _ => panic!(),
        }
        match &back.blocks[9] {
            Block::Callout { kind, text, .. } => {
                assert_eq!(*kind, CalloutKind::Tip);
                assert_eq!(text, "Use Tab to move.\nShift+Tab goes back.");
            }
            _ => panic!(),
        }
        let ids: Vec<_> = back.blocks.iter().map(|b| match b { Block::Step(s) => s.id.clone(), Block::Heading { id, .. } | Block::Text { id, .. } | Block::List { id, .. } | Block::Image { id, .. } | Block::Callout { id, .. } => id.clone() }).collect();
        assert_eq!(&ids[..3], &["b1", "b2", "b3"]);
    }

    #[test]
    fn hostile_text_round_trips() {
        let mut rec = Recording::new("r", "T *_[]<>|* ~ `x`\nsecond line", 0);
        rec.blocks.push(Block::Text { id: "t".into(), text: "1. not a list\n# not a heading\n- x\n> q\n---\n![a](b)".into() });
        rec.blocks.push(Block::Text { id: "t".into(), text: "#starts with hash".into() });
        rec.blocks.push(step("Click *_[]<>|* here", "", None)); // empty description, no image
        rec.blocks.push(Block::Text { id: "t".into(), text: "directly after an empty step".into() });
        rec.blocks.push(step("Step 7. fake\nnewline", "1. also not a list", Some("images/a b.png")));
        rec.blocks.push(Block::Heading { id: "h".into(), level: 2, text: "Step 4. looks like a step".into() });
        rec.blocks.push(Block::Heading { id: "h".into(), level: 3, text: "*_[]<>|* # heading".into() });
        rec.blocks.push(Block::List { id: "l".into(), ordered: false, items: vec!["1. nested?".into(), "- dash".into(), "# hash".into(), "".into()] });
        rec.blocks.push(Block::Callout { id: "c".into(), kind: CalloutKind::Warning, text: "# loud\n1. second".into() });
        rec.blocks.push(Block::Image { id: "i".into(), image: "images/x.png".into(), caption: "# cap\n- more".into() });
        let md = to_markdown(&rec);
        let back = from_markdown(&md);
        assert_eq!(to_markdown(&back), md);
        assert_eq!(kinds(&back), kinds(&rec));
        assert_eq!(back.title, "T *_[]<>|* ~ `x` second line");
        match &back.blocks[0] {
            Block::Text { text, .. } => assert_eq!(text, "1. not a list\n# not a heading\n- x\n> q\n---\n![a](b)"),
            _ => panic!(),
        }
        match &back.blocks[2] {
            Block::Step(s) => {
                assert_eq!(s.title, "Click *_[]<>|* here");
                assert_eq!(s.description, "");
            }
            _ => panic!(),
        }
        match &back.blocks[4] {
            Block::Step(s) => {
                assert_eq!(s.title, "Step 7. fake newline");
                assert_eq!(s.description, "1. also not a list");
                assert_eq!(s.screenshot.as_deref(), Some("images/a b.png"));
            }
            _ => panic!(),
        }
        match &back.blocks[5] {
            Block::Heading { level, text, .. } => assert_eq!((*level, text.as_str()), (2, "Step 4. looks like a step")),
            _ => panic!("fake step heading became a step"),
        }
    }

    #[test]
    fn steps_are_renumbered_and_manual() {
        let md = "# Doc\n\n## Step 5. First\n\ndesc\n\n![Step 5](images/a.png)\n\n## Step 9. Second\n";
        let rec = from_markdown(md);
        assert_eq!(rec.step_count(), 2);
        assert_eq!(to_markdown(&rec), "# Doc\n\n## Step 1. First\n\ndesc\n\n![Step 1](images/a.png)\n\n## Step 2. Second\n");
        match &rec.blocks[0] {
            Block::Step(s) => assert_eq!(s.action, Action::Manual),
            _ => panic!(),
        }
    }

    #[test]
    fn empty_text_and_empty_lists_are_dropped_consistently() {
        let mut rec = Recording::new("r", "t", 0);
        rec.blocks.push(Block::Text { id: "t".into(), text: " \n ".into() });
        rec.blocks.push(Block::List { id: "l".into(), ordered: true, items: vec![] });
        rec.blocks.push(step("only", "", None));
        let md = to_markdown(&rec);
        assert_eq!(md, "# t\n\n## Step 1. only\n");
        assert_eq!(to_markdown(&from_markdown(&md)), md);
    }

    #[test]
    fn empty_input_gives_empty_recording() {
        let rec = from_markdown("");
        assert_eq!(rec.title, "");
        assert!(rec.blocks.is_empty());
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b%28c%29%25"), "a b(c)%");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        // from_str_radix accepts a sign; a percent escape must not.
        assert_eq!(percent_decode("%+f"), "%+f");
        assert_eq!(percent_decode("a%+fb"), "a%+fb");
    }

    #[test]
    fn a_long_chunk_of_headings_imports_quickly() {
        let md = "# T\n".to_string() + &"## Step 1. x\n".repeat(20000);
        let rec = from_markdown(&md);
        assert_eq!(rec.blocks.len(), 20000);
    }
}

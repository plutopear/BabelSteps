//! Pre-export scan of a whole document for sensitive text (B13, B14, E5).
//!
//! Typed text is scrubbed when it is captured, but nothing else checks what is
//! in the document by the time it is exported: a title the user typed, an
//! imported Markdown file, a callout, a custom word added after recording. This
//! module runs the detectors in [`crate::pii`] over every text field and reports
//! where each hit is, so the user can look at it and hide it.
//!
//! **Detection misses things and flags harmless text.** An empty result means
//! "nothing was flagged", never "nothing sensitive is here". Screenshots are not
//! read at all (Q27). Names, addresses, and passwords typed into an ordinary
//! field all pass. Nothing here may be used to claim a document is clean.
//!
//! A [`Finding`] carries no matched text. Its `Debug` and `Display` show the
//! place and kind only, and its `preview` shows at most the first and last
//! character of a long match, so findings can be logged or put into an error
//! without leaking what was found.
//!
//! Not scanned: screenshots, the recorded program and screen-element names
//! (`Target`, shown only in the optional technical details of an export), and
//! the raw typed-text record of a step (`Action::Text`), which no exporter
//! writes.

use crate::model::{Block, Recording};
use crate::pii::{self, CustomRules, Kind};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Which text of the document a finding is in. Positions inside a block use
/// zero-based indexes; labels shown to the user count from one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Field {
    /// The recording's title. There is no block, so `block_id` is `None`.
    DocumentTitle,
    /// A step's title.
    Title,
    /// A step's description.
    Description,
    Heading,
    Text,
    ListItem {
        index: usize,
    },
    Callout,
    Caption,
}

/// One flagged span. Offsets are byte offsets into the field's text.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// The block holding the text; `None` for the document title.
    pub block_id: Option<String>,
    pub field: Field,
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    /// A fixed label for the kind, such as "email address".
    pub kind_label: String,
    /// Where it is, for people: "Step 3 description", "Block 5 list item 2".
    pub location: String,
    /// A masked hint: the first and last character of a long match, else "…".
    pub preview: String,
}

/// Place and kind only. The preview and the offsets are left out of the text
/// forms too, so this is safe to log.
impl fmt::Debug for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Finding")
            .field("block_id", &self.block_id)
            .field("field", &self.field)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in {}", self.kind_label, self.location)
    }
}

/// The fixed, plain-language name of a kind. Never contains matched text.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Email => "email address",
        Kind::Phone => "phone number",
        Kind::CardNumber => "card number",
        Kind::Ssn => "ID number",
        Kind::IpAddress => "IP address",
        Kind::Token => "token or key",
        Kind::PrivateKey => "private key",
        Kind::Custom => "custom word or pattern",
    }
}

const ALL_KINDS: [Kind; 8] = [
    Kind::Email,
    Kind::Phone,
    Kind::CardNumber,
    Kind::Ssn,
    Kind::IpAddress,
    Kind::Token,
    Kind::PrivateKey,
    Kind::Custom,
];

/// One text field of the document, with what is needed to name it.
struct Slot<'a> {
    block_id: Option<&'a str>,
    /// One-based position among all blocks; 0 for the document title.
    position: usize,
    /// One-based number among steps, for step fields.
    step_no: Option<usize>,
    field: Field,
    text: &'a str,
}

fn slots(rec: &Recording) -> Vec<Slot<'_>> {
    fn push<'a>(
        out: &mut Vec<Slot<'a>>,
        block_id: Option<&'a str>,
        position: usize,
        step_no: Option<usize>,
        field: Field,
        text: &'a str,
    ) {
        out.push(Slot {
            block_id,
            position,
            step_no,
            field,
            text,
        });
    }
    let mut out = Vec::new();
    push(&mut out, None, 0, None, Field::DocumentTitle, &rec.title);
    let mut step_no = 0;
    for (i, block) in rec.blocks.iter().enumerate() {
        let pos = i + 1;
        match block {
            Block::Heading { id, text, .. } => {
                push(&mut out, Some(id), pos, None, Field::Heading, text)
            }
            Block::Text { id, text } => push(&mut out, Some(id), pos, None, Field::Text, text),
            Block::Step(s) => {
                step_no += 1;
                push(
                    &mut out,
                    Some(&s.id),
                    pos,
                    Some(step_no),
                    Field::Title,
                    &s.title,
                );
                push(
                    &mut out,
                    Some(&s.id),
                    pos,
                    Some(step_no),
                    Field::Description,
                    &s.description,
                );
            }
            Block::List { id, items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    push(
                        &mut out,
                        Some(id),
                        pos,
                        None,
                        Field::ListItem { index },
                        item,
                    );
                }
            }
            Block::Image { id, caption, .. } => {
                push(&mut out, Some(id), pos, None, Field::Caption, caption)
            }
            Block::Callout { id, text, .. } => {
                push(&mut out, Some(id), pos, None, Field::Callout, text)
            }
        }
    }
    out
}

fn location(slot: &Slot<'_>) -> String {
    match (&slot.field, slot.step_no) {
        (Field::DocumentTitle, _) => "document title".to_string(),
        (Field::Title, Some(n)) => format!("Step {n} title"),
        (Field::Description, Some(n)) => format!("Step {n} description"),
        (Field::Heading, _) => format!("Block {} heading", slot.position),
        (Field::Text, _) => format!("Block {} text", slot.position),
        (Field::ListItem { index }, _) => {
            format!("Block {} list item {}", slot.position, index + 1)
        }
        (Field::Callout, _) => format!("Block {} callout", slot.position),
        (Field::Caption, _) => format!("Block {} image caption", slot.position),
        // A step field always has a step number; this arm is for completeness.
        (Field::Title, None) | (Field::Description, None) => format!("Block {}", slot.position),
    }
}

/// Matches in one text, minus any that sit inside a placeholder that this
/// module or capture already wrote (`[email]`), so a hidden item is not flagged
/// again, even when a custom word such as "email" would match the label inside.
fn matches(text: &str, custom: &CustomRules) -> Vec<pii::Match> {
    let mut found = pii::find(text, custom);
    if found.is_empty() || !text.contains('[') {
        return found;
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for kind in ALL_KINDS {
        let p = kind.placeholder();
        spans.extend(text.match_indices(p).map(|(i, _)| (i, i + p.len())));
    }
    found.retain(|m| !spans.iter().any(|&(s, e)| m.start >= s && m.end <= e));
    found
}

/// First and last character of a match of 8 or more characters, else "…".
fn mask(matched: &str) -> String {
    let mut chars = matched.chars();
    match (chars.next(), chars.next_back()) {
        (Some(first), Some(last)) if matched.chars().count() >= 8 => format!("{first}…{last}"),
        _ => "…".to_string(),
    }
}

/// Scans every text of the document and returns what was flagged, in document
/// order. Detection misses things (see the module note): an empty result is
/// not a statement that the document is free of sensitive data.
pub fn scan(rec: &Recording, custom: &CustomRules) -> Vec<Finding> {
    let mut out = Vec::new();
    for slot in slots(rec) {
        if slot.text.is_empty() {
            continue;
        }
        for m in matches(slot.text, custom) {
            out.push(Finding {
                block_id: slot.block_id.map(str::to_string),
                field: slot.field.clone(),
                kind: m.kind,
                start: m.start,
                end: m.end,
                kind_label: kind_label(m.kind).to_string(),
                location: location(&slot),
                preview: mask(&slot.text[m.start..m.end]),
            });
        }
    }
    out
}

fn block_id_of(b: &Block) -> &str {
    match b {
        Block::Heading { id, .. }
        | Block::Text { id, .. }
        | Block::List { id, .. }
        | Block::Image { id, .. }
        | Block::Callout { id, .. } => id,
        Block::Step(s) => &s.id,
    }
}

fn field_mut<'a>(
    rec: &'a mut Recording,
    block_id: Option<&str>,
    field: &Field,
) -> Option<&'a mut String> {
    let Some(id) = block_id else {
        return matches!(field, Field::DocumentTitle).then_some(&mut rec.title);
    };
    let block = rec.blocks.iter_mut().find(|b| block_id_of(b) == id)?;
    match (block, field) {
        (Block::Heading { text, .. }, Field::Heading) => Some(text),
        (Block::Text { text, .. }, Field::Text) => Some(text),
        (Block::Step(s), Field::Title) => Some(&mut s.title),
        (Block::Step(s), Field::Description) => Some(&mut s.description),
        (Block::List { items, .. }, Field::ListItem { index }) => items.get_mut(*index),
        (Block::Image { caption, .. }, Field::Caption) => Some(caption),
        (Block::Callout { text, .. }, Field::Callout) => Some(text),
        _ => None,
    }
}

/// Returns a copy of the document with the finding's span replaced by its
/// placeholder (`[email]`, `[custom]`, ...). The input is not changed.
///
/// Returns `None`, changing nothing, when the finding no longer matches the
/// document: the block or field is gone, the text was edited, or the rules
/// changed since the scan. The caller scans again.
pub fn redact_finding(
    rec: &Recording,
    finding: &Finding,
    custom: &CustomRules,
) -> Option<Recording> {
    let mut out = rec.clone();
    let text = field_mut(&mut out, finding.block_id.as_deref(), &finding.field)?;
    let still_there = matches(text, custom)
        .iter()
        .any(|m| m.start == finding.start && m.end == finding.end && m.kind == finding.kind);
    if !still_there {
        return None;
    }
    text.replace_range(finding.start..finding.end, finding.kind.placeholder());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, CalloutKind, StepBlock};

    // Each sentinel is also a detectable item, so a leak into text is visible.
    const EMAIL: &str = "sentinel.person@example.com";
    const PHONE: &str = "555-123-4567";
    const CARD: &str = "4111 1111 1111 1111";
    const IP: &str = "192.168.10.20";
    const TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    const WORD: &str = "Zebracorp";
    const PATTERN_HIT: &str = "TICKET-98765";

    fn custom() -> CustomRules {
        CustomRules::new([WORD], [r"TICKET-\d{5}"]).unwrap()
    }

    fn step(id: &str, title: &str, description: &str) -> Block {
        Block::Step(StepBlock {
            id: id.into(),
            action: Action::Manual,
            title: title.into(),
            description: description.into(),
            screenshot: None,
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        })
    }

    fn doc(blocks: Vec<Block>) -> Recording {
        let mut r = Recording::new("rec1", "Reset a password", 0);
        r.blocks = blocks;
        r
    }

    fn text_of(rec: &Recording, f: &Finding) -> String {
        let mut c = rec.clone();
        field_mut(&mut c, f.block_id.as_deref(), &f.field)
            .unwrap()
            .clone()
    }

    fn planted() -> Recording {
        doc(vec![
            Block::Heading {
                id: "h1".into(),
                level: 2,
                text: "Intro".into(),
            },
            step(
                "s1",
                "Left-click \"Save\" button in Notepad",
                &format!("Send it to {EMAIL} first."),
            ),
            step(
                "s2",
                "Type in the field",
                &format!("Call {PHONE} if it fails."),
            ),
            Block::Text {
                id: "t1".into(),
                text: format!("The card was {CARD} on file."),
            },
            Block::List {
                id: "l1".into(),
                ordered: false,
                items: vec![
                    "Nothing here".into(),
                    format!("Server {IP} is down"),
                    format!("Use {TOKEN} to log in"),
                ],
            },
            Block::Callout {
                id: "c1".into(),
                kind: CalloutKind::Warning,
                text: format!("Ask {WORD} about it"),
            },
            Block::Image {
                id: "i1".into(),
                image: "images/a.png".into(),
                caption: format!("See {PATTERN_HIT} here"),
            },
        ])
    }

    #[test]
    fn finds_each_kind_in_its_own_block_and_field() {
        let rec = planted();
        let found = scan(&rec, &custom());
        let got: Vec<(Option<&str>, Field, Kind)> = found
            .iter()
            .map(|f| (f.block_id.as_deref(), f.field.clone(), f.kind))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("s1"), Field::Description, Kind::Email),
                (Some("s2"), Field::Description, Kind::Phone),
                (Some("t1"), Field::Text, Kind::CardNumber),
                (Some("l1"), Field::ListItem { index: 1 }, Kind::IpAddress),
                (Some("l1"), Field::ListItem { index: 2 }, Kind::Token),
                (Some("c1"), Field::Callout, Kind::Custom),
                (Some("i1"), Field::Caption, Kind::Custom),
            ]
        );
        // The byte range points at the planted text.
        let expect = [EMAIL, PHONE, CARD, IP, TOKEN, WORD, PATTERN_HIT];
        for (f, want) in found.iter().zip(expect) {
            assert_eq!(&text_of(&rec, f)[f.start..f.end], want);
        }
    }

    #[test]
    fn locations_name_the_step_or_block() {
        let found = scan(&planted(), &custom());
        let places: Vec<&str> = found.iter().map(|f| f.location.as_str()).collect();
        assert_eq!(
            places,
            [
                "Step 1 description",
                "Step 2 description",
                "Block 4 text",
                "Block 5 list item 2",
                "Block 5 list item 3",
                "Block 6 callout",
                "Block 7 image caption",
            ]
        );
        assert_eq!(found[0].kind_label, "email address");
        assert_eq!(found[0].to_string(), "email address in Step 1 description");
    }

    #[test]
    fn finds_titles_headings_and_the_document_title() {
        let mut rec = doc(vec![
            Block::Heading {
                id: "h1".into(),
                level: 1,
                text: format!("About {EMAIL}"),
            },
            step("s1", &format!("Open {WORD}"), ""),
        ]);
        rec.title = format!("Notes for {IP}");
        let found = scan(&rec, &custom());
        let got: Vec<(Option<&str>, Field)> = found
            .iter()
            .map(|f| (f.block_id.as_deref(), f.field.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (None, Field::DocumentTitle),
                (Some("h1"), Field::Heading),
                (Some("s1"), Field::Title)
            ]
        );
        assert_eq!(found[0].location, "document title");
        assert_eq!(found[2].location, "Step 1 title");
    }

    #[test]
    fn clean_text_gives_no_findings() {
        let rec = doc(vec![
            Block::Heading {
                id: "h".into(),
                level: 2,
                text: "Before you start".into(),
            },
            step(
                "s",
                "Left-click \"Save\" button in Notepad",
                "Choose a folder, then press Save.",
            ),
            Block::Text {
                id: "t".into(),
                text: "It takes about 5 minutes. Version 1.2.3.".into(),
            },
            Block::List {
                id: "l".into(),
                ordered: true,
                items: vec!["Open the app".into(), String::new()],
            },
            Block::Callout {
                id: "c".into(),
                kind: CalloutKind::Tip,
                text: "Save often.".into(),
            },
            Block::Image {
                id: "i".into(),
                image: "images/a.png".into(),
                caption: String::new(),
            },
        ]);
        assert!(scan(&rec, &custom()).is_empty());
        assert!(scan(&Recording::new("r", "", 0), &CustomRules::none()).is_empty());
    }

    #[test]
    fn custom_rules_added_later_are_applied() {
        let rec = doc(vec![Block::Text {
            id: "t".into(),
            text: format!("Ask {WORD}."),
        }]);
        assert!(scan(&rec, &CustomRules::none()).is_empty());
        assert_eq!(scan(&rec, &custom()).len(), 1);
    }

    #[test]
    fn redact_replaces_only_the_match() {
        let rec = planted();
        let custom = custom();
        let found = scan(&rec, &custom);
        let after = redact_finding(&rec, &found[0], &custom).unwrap();
        match &after.blocks[1] {
            Block::Step(s) => assert_eq!(s.description, "Send it to [email] first."),
            _ => panic!("not a step"),
        }
        // Nothing else changed: the original is untouched and every other block is equal.
        assert!(matches!(&rec.blocks[1], Block::Step(s) if s.description.contains(EMAIL)));
        for i in (0..rec.blocks.len()).filter(|i| *i != 1) {
            assert_eq!(rec.blocks[i], after.blocks[i], "block {i}");
        }
        assert_eq!(rec.title, after.title);
        // A list item: only that item changes, and the placeholder matches the kind.
        let after = redact_finding(&rec, &found[3], &custom).unwrap();
        match &after.blocks[4] {
            Block::List { items, .. } => {
                assert_eq!(items[0], "Nothing here");
                assert_eq!(items[1], "Server [ip address] is down");
                assert!(items[2].contains(TOKEN));
            }
            _ => panic!("not a list"),
        }
        // Custom matches use the custom placeholder, and the document title works too.
        let after = redact_finding(&rec, &found[5], &custom).unwrap();
        assert!(
            matches!(&after.blocks[5], Block::Callout { text, .. } if text == "Ask [custom] about it")
        );
        let mut titled = rec.clone();
        titled.title = format!("For {EMAIL}!");
        let f = scan(&titled, &custom).remove(0);
        assert_eq!(
            redact_finding(&titled, &f, &custom).unwrap().title,
            "For [email]!"
        );
    }

    #[test]
    fn redacting_multibyte_text_keeps_the_rest() {
        let rec = doc(vec![Block::Text {
            id: "t".into(),
            text: format!("日本語 {EMAIL} ünï"),
        }]);
        let f = scan(&rec, &CustomRules::none()).remove(0);
        let after = redact_finding(&rec, &f, &CustomRules::none()).unwrap();
        assert!(
            matches!(&after.blocks[0], Block::Text { text, .. } if text == "日本語 [email] ünï")
        );
    }

    #[test]
    fn two_findings_in_one_field_can_be_hidden_one_after_the_other() {
        let rec = doc(vec![Block::Text {
            id: "t".into(),
            text: format!("{EMAIL} and {IP}"),
        }]);
        let none = CustomRules::none();
        let mut now = rec;
        for _ in 0..2 {
            let f = scan(&now, &none).remove(0);
            now = redact_finding(&now, &f, &none).unwrap();
        }
        assert!(
            matches!(&now.blocks[0], Block::Text { text, .. } if text == "[email] and [ip address]")
        );
        assert!(scan(&now, &none).is_empty());
    }

    #[test]
    fn a_stale_finding_changes_nothing() {
        let rec = planted();
        let custom = custom();
        let f = scan(&rec, &custom).remove(0);
        // The text was edited after the scan.
        let mut edited = rec.clone();
        if let Block::Step(s) = &mut edited.blocks[1] {
            s.description = "Short.".into();
        }
        assert!(redact_finding(&edited, &f, &custom).is_none());
        // The block is gone.
        let mut gone = rec.clone();
        gone.blocks.remove(1);
        assert!(redact_finding(&gone, &f, &custom).is_none());
        // The rules changed so the word no longer matches.
        let word = scan(&rec, &custom)
            .into_iter()
            .find(|f| f.kind == Kind::Custom)
            .unwrap();
        assert!(redact_finding(&rec, &word, &CustomRules::none()).is_none());
        // A range that is not on a character boundary or is past the end cannot panic.
        let mut bad = f.clone();
        bad.start += 1;
        assert!(redact_finding(&rec, &bad, &custom).is_none());
        bad.end = 10_000;
        assert!(redact_finding(&rec, &bad, &custom).is_none());
    }

    #[test]
    fn a_placeholder_is_not_flagged_again() {
        let text =
            "[email] [phone] [card number] [ssn] [ip address] [token] [private key] [custom]";
        let rec = doc(vec![Block::Text {
            id: "t".into(),
            text: text.into(),
        }]);
        assert!(scan(&rec, &CustomRules::none()).is_empty());
        // Even when a custom word equals a placeholder's label.
        let words = CustomRules::new(["email", "custom", "token"], Vec::<&str>::new()).unwrap();
        assert!(scan(&rec, &words).is_empty());
        // But the same word outside a placeholder is still flagged.
        let rec = doc(vec![Block::Text {
            id: "t".into(),
            text: "[email] and email".into(),
        }]);
        assert_eq!(scan(&rec, &words).len(), 1);
    }

    #[test]
    fn debug_and_display_never_show_the_matched_text() {
        let rec = planted();
        let found = scan(&rec, &custom());
        assert_eq!(found.len(), 7);
        let secrets = [EMAIL, PHONE, CARD, IP, TOKEN, WORD, PATTERN_HIT];
        let shown = [
            format!("{found:?}"),
            format!("{found:#?}"),
            found
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            format!("{:?}", found[0]),
        ];
        for s in &shown {
            for secret in secrets {
                assert!(!s.contains(secret), "{secret} leaked into {s}");
            }
            // Not even a fragment the user would recognize.
            assert!(
                !s.contains("sentinel") && !s.contains("Zebra") && !s.contains("98765"),
                "{s}"
            );
        }
        // The control: the planted text is in the document, and the labels are real.
        assert!(serde_json::to_string(&rec).unwrap().contains(EMAIL));
        assert!(shown[2].contains("email address in Step 1 description"));
        // The preview is masked: at most first and last character, never the middle.
        for f in &found {
            assert!(f.preview.chars().count() <= 3, "{:?}", f.preview);
        }
        assert_eq!(found[0].preview, "s…m");
        let json = serde_json::to_string(&found).unwrap();
        for secret in secrets {
            assert!(!json.contains(secret), "{secret} leaked into the wire form");
        }
    }

    #[test]
    fn findings_round_trip_through_json() {
        let found = scan(&planted(), &custom());
        let json = serde_json::to_string(&found).unwrap();
        assert!(
            json.contains("\"blockId\":\"s1\"") && json.contains("\"kind\":\"list_item\""),
            "{json}"
        );
        let back: Vec<Finding> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, found);
    }

    #[test]
    fn a_large_document_scans_quickly() {
        let mut blocks = Vec::new();
        for i in 0..500 {
            blocks.push(match i % 4 {
                0 => step(
                    &format!("s{i}"),
                    "Left-click \"OK\" button",
                    "Press OK to continue with the next part of the guide.",
                ),
                1 => Block::Text {
                    id: format!("t{i}"),
                    text: "Plain explanatory text with no personal data in it at all.".repeat(4),
                },
                2 => Block::List {
                    id: format!("l{i}"),
                    ordered: false,
                    items: vec!["one".into(), "two".into(), format!("mail {EMAIL}")],
                },
                _ => Block::Callout {
                    id: format!("c{i}"),
                    kind: CalloutKind::Note,
                    text: "Remember to save.".into(),
                },
            });
        }
        let rec = doc(blocks);
        let custom = custom();
        let t = std::time::Instant::now();
        let found = scan(&rec, &custom);
        let took = t.elapsed();
        assert_eq!(found.len(), 125);
        assert!(took < std::time::Duration::from_secs(1), "took {took:?}");
    }
}

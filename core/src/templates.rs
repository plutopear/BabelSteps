//! Built-in starting templates (E3). A template is only a list of ordinary,
//! editable blocks that a new recording starts with. Nothing here reads the
//! system, and the placeholder text names no user, machine, app or window.
//!
//! The document title is the recording's title, so a template does not add a
//! title block. The "Steps" section is just a heading with a fixed id
//! ([`STEPS_HEADING_ID`]). Captured steps go right after the last step that is
//! already below that heading, or straight after the heading when there are
//! none yet ([`steps_insert_index`]). A recording without that heading (a blank
//! one, or one where the user deleted it) appends steps at the end, as before.

use crate::model::{Block, CalloutKind};
use serde::{Deserialize, Serialize};

/// Block id of the "Steps" heading of both templates.
pub const STEPS_HEADING_ID: &str = "template-steps";

/// What a new recording starts with. `Blank` is the default and means no blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Template {
    #[default]
    Blank,
    HelpdeskHowTo,
    TrainingGuide,
}

impl Template {
    /// Every template, in the order a picker lists them.
    pub const ALL: [Template; 3] = [
        Template::Blank,
        Template::HelpdeskHowTo,
        Template::TrainingGuide,
    ];

    /// Name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Template::Blank => "Blank",
            Template::HelpdeskHowTo => "Helpdesk how-to",
            Template::TrainingGuide => "Training guide",
        }
    }

    /// The blocks a new recording starts with. Ids are fixed and unique within
    /// the list; one template is applied once per recording, so they are unique
    /// in the document too (steps and user blocks get their own ids).
    pub fn initial_blocks(self) -> Vec<Block> {
        match self {
            Template::Blank => Vec::new(),
            Template::HelpdeskHowTo => vec![
                heading("template-purpose", "Purpose"),
                text(
                    "template-purpose-text",
                    "Describe in one sentence what this guide helps someone do.",
                ),
                heading("template-before", "Before you start"),
                list(
                    "template-before-list",
                    "Add anything the reader needs first, or delete this section.",
                ),
                heading(STEPS_HEADING_ID, "Steps"),
                heading("template-result", "Result"),
                text(
                    "template-result-text",
                    "Describe what the reader should have when they finish.",
                ),
            ],
            Template::TrainingGuide => vec![
                heading("template-goal", "Goal"),
                text(
                    "template-goal-text",
                    "Describe in one sentence what the learner will be able to do.",
                ),
                Block::Callout {
                    id: "template-tip".into(),
                    kind: CalloutKind::Tip,
                    text: "Take your time; you can repeat these steps as often as you like.".into(),
                },
                heading("template-needs", "What you need"),
                list(
                    "template-needs-list",
                    "Add anything the learner needs first, or delete this section.",
                ),
                heading(STEPS_HEADING_ID, "Steps"),
                heading("template-expect", "What you should see"),
                text(
                    "template-expect-text",
                    "Describe what the learner should see when they finish.",
                ),
            ],
        }
    }
}

fn heading(id: &str, text: &str) -> Block {
    Block::Heading {
        id: id.into(),
        level: 2,
        text: text.into(),
    }
}

fn text(id: &str, text: &str) -> Block {
    Block::Text {
        id: id.into(),
        text: text.into(),
    }
}

fn list(id: &str, item: &str) -> Block {
    Block::List {
        id: id.into(),
        ordered: false,
        items: vec![item.into()],
    }
}

/// Where a captured block goes: after the last step below the template's
/// "Steps" heading, else right after that heading, else at the end.
pub fn steps_insert_index(blocks: &[Block]) -> usize {
    let Some(h) = blocks
        .iter()
        .position(|b| matches!(b, Block::Heading { id, .. } if id == STEPS_HEADING_ID))
    else {
        return blocks.len();
    };
    blocks[h + 1..]
        .iter()
        .rposition(|b| matches!(b, Block::Step(_)))
        .map_or(h + 1, |i| h + 1 + i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::to_markdown;
    use crate::markdown_import::from_markdown;
    use crate::model::{Action, Recording, StepBlock};
    use std::collections::HashSet;

    fn id_of(b: &Block) -> &str {
        match b {
            Block::Step(s) => &s.id,
            Block::Heading { id, .. }
            | Block::Text { id, .. }
            | Block::List { id, .. }
            | Block::Image { id, .. }
            | Block::Callout { id, .. } => id,
        }
    }

    fn headings(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(|b| match b {
                Block::Heading { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn step(id: &str) -> Block {
        Block::Step(StepBlock {
            id: id.into(),
            action: Action::Manual,
            title: "A step".into(),
            description: String::new(),
            screenshot: None,
            target: None,
            redactions: vec![],
            captured_unix_ms: 0,
            screenshot_withheld: false,
        })
    }

    /// The importer renames ids, so compare blocks with ids blanked.
    fn without_ids(blocks: &[Block]) -> Vec<Block> {
        blocks
            .iter()
            .cloned()
            .map(|mut b| {
                match &mut b {
                    Block::Heading { id, .. }
                    | Block::Text { id, .. }
                    | Block::List { id, .. }
                    | Block::Image { id, .. }
                    | Block::Callout { id, .. } => id.clear(),
                    Block::Step(s) => s.id.clear(),
                }
                b
            })
            .collect()
    }

    #[test]
    fn blank_is_the_default_and_has_no_blocks() {
        assert_eq!(Template::default(), Template::Blank);
        assert!(Template::Blank.initial_blocks().is_empty());
    }

    #[test]
    fn ids_are_unique_and_there_is_one_steps_heading() {
        for t in Template::ALL {
            let blocks = t.initial_blocks();
            let ids: HashSet<_> = blocks.iter().map(id_of).collect();
            assert_eq!(ids.len(), blocks.len(), "{t:?}");
            let n = blocks
                .iter()
                .filter(|b| matches!(b, Block::Heading { id, .. } if id == STEPS_HEADING_ID))
                .count();
            assert_eq!(n, usize::from(t != Template::Blank), "{t:?}");
        }
    }

    #[test]
    fn expected_headings() {
        assert_eq!(
            headings(&Template::HelpdeskHowTo.initial_blocks()),
            ["Purpose", "Before you start", "Steps", "Result"]
        );
        assert_eq!(
            headings(&Template::TrainingGuide.initial_blocks()),
            ["Goal", "What you need", "Steps", "What you should see"]
        );
        let has_tip = Template::TrainingGuide.initial_blocks().iter().any(|b| {
            matches!(b, Block::Callout { kind: CalloutKind::Tip, text, .. }
                if text == "Take your time; you can repeat these steps as often as you like.")
        });
        assert!(has_tip);
    }

    #[test]
    fn serde_names_are_snake_case() {
        let json = |t: Template| serde_json::to_string(&t).unwrap();
        assert_eq!(json(Template::Blank), "\"blank\"");
        assert_eq!(json(Template::HelpdeskHowTo), "\"helpdesk_how_to\"");
        assert_eq!(json(Template::TrainingGuide), "\"training_guide\"");
        assert_eq!(
            serde_json::from_str::<Template>("\"training_guide\"").unwrap(),
            Template::TrainingGuide
        );
    }

    #[test]
    fn markdown_round_trip_keeps_the_blocks() {
        for t in Template::ALL {
            let mut rec = Recording::new("r", "A guide", 0);
            rec.blocks = t.initial_blocks();
            let back = from_markdown(&to_markdown(&rec));
            assert_eq!(without_ids(&back.blocks), without_ids(&rec.blocks), "{t:?}");
        }
    }

    #[test]
    fn placeholders_name_nothing_from_the_system() {
        for t in Template::ALL {
            let json = serde_json::to_string(&t.initial_blocks())
                .unwrap()
                .to_lowercase();
            for word in ["\\\\", "c:", ".exe", "users", "desktop-"] {
                assert!(!json.contains(word), "{t:?} contains {word}");
            }
        }
    }

    #[test]
    fn steps_go_after_the_steps_heading_and_before_the_result() {
        for t in [Template::HelpdeskHowTo, Template::TrainingGuide] {
            let mut blocks = t.initial_blocks();
            for n in 1..=3 {
                let at = steps_insert_index(&blocks);
                blocks.insert(at, step(&format!("s{n}")));
            }
            let h = blocks
                .iter()
                .position(|b| id_of(b) == STEPS_HEADING_ID)
                .unwrap();
            let order: Vec<_> = blocks[h..h + 4].iter().map(id_of).collect();
            assert_eq!(order, [STEPS_HEADING_ID, "s1", "s2", "s3"], "{t:?}");
            assert!(matches!(&blocks[h + 4], Block::Heading { .. }), "{t:?}");
        }
    }

    #[test]
    fn without_the_heading_steps_are_appended() {
        let mut blocks = vec![text("a", "x"), step("s1"), text("b", "after")];
        assert_eq!(steps_insert_index(&blocks), 3);
        blocks.insert(steps_insert_index(&blocks), step("s2"));
        assert_eq!(id_of(&blocks[3]), "s2");
        assert_eq!(steps_insert_index(&[]), 0);
    }

    #[test]
    fn a_step_above_the_heading_does_not_move_the_insert_point() {
        let blocks = vec![
            step("s0"),
            heading(STEPS_HEADING_ID, "Steps"),
            text("t", "x"),
        ];
        assert_eq!(steps_insert_index(&blocks), 2);
    }
}

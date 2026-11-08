//! The typed-text notice (PSR-3, Q30): one line at the top of a recording that
//! says what was not captured. It is an ordinary text block, so the user can
//! edit or delete it and every exporter shows it as written. It states only
//! what was or was not recorded, and never claims the file is free of
//! sensitive data (product rule).

use crate::model::{Block, Recording};

pub const TYPED_TEXT_NOT_RECORDED: &str = "Typed text was not recorded. Keyboard shortcuts were.";
pub const TYPED_TEXT_RECORDED: &str = "Typed text was recorded. Review it before sharing.";
/// Block id of the notice, so a later recording session can find it.
pub const NOTICE_ID: &str = "typed-text-notice";

/// Call when a recording session starts, with that session's keystroke setting.
///
/// - A recording with no steps yet gets the notice as its first block.
/// - If the notice says typed text was not recorded, still word for word, and
///   this session records typed text, it is changed to say typed text was recorded.
/// - Otherwise nothing changes: a notice the user edited or deleted stays as they left it.
///
/// Returns true when the document changed.
pub fn apply_typed_text_notice(rec: &mut Recording, keystrokes: bool) -> bool {
    let existing = rec.blocks.iter_mut().find_map(|b| match b {
        Block::Text { id, text } if id == NOTICE_ID => Some(text),
        _ => None,
    });
    if let Some(text) = existing {
        if keystrokes && text.trim() == TYPED_TEXT_NOT_RECORDED {
            *text = TYPED_TEXT_RECORDED.to_string();
            return true;
        }
        return false;
    }
    if rec.step_count() > 0 {
        return false;
    }
    let text = if keystrokes { TYPED_TEXT_RECORDED } else { TYPED_TEXT_NOT_RECORDED };
    rec.blocks.insert(0, Block::Text { id: NOTICE_ID.to_string(), text: text.to_string() });
    true
}

/// The counts shown in the app after every recording (PSR-5), such as
/// "Recorded 12 steps. 1 screenshot was withheld because the password check
/// failed or could not finish." Counts that are zero are left out, apart from
/// the number of steps.
pub fn recording_summary(steps: usize, withheld: usize, no_screenshot: usize, missed: u64) -> String {
    let s = |n: u64| if n == 1 { "" } else { "s" };
    let mut out = match steps {
        0 => "No steps were recorded.".to_string(),
        n => format!("Recorded {n} step{}.", s(n as u64)),
    };
    if withheld > 0 {
        let verb = if withheld == 1 { "was" } else { "were" };
        out.push_str(&format!(
            " {withheld} screenshot{} {verb} withheld because the password check failed or could not finish.",
            s(withheld as u64)
        ));
    }
    if no_screenshot > 0 {
        let verb = if no_screenshot == 1 { "has" } else { "have" };
        out.push_str(&format!(" {no_screenshot} step{} {verb} no screenshot.", s(no_screenshot as u64)));
    }
    if missed > 0 {
        let verb = if missed == 1 { "was" } else { "were" };
        out.push_str(&format!(" {missed} click{} or key press{} {verb} missed.", s(missed), if missed == 1 { "" } else { "es" }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn notice(rec: &Recording) -> Option<&str> {
        rec.blocks.iter().find_map(|b| match b {
            Block::Text { id, text } if id == NOTICE_ID => Some(text.as_str()),
            _ => None,
        })
    }

    fn add_step(rec: &mut Recording) {
        rec.blocks.push(Block::Step(StepBlock {
            id: format!("s{}", rec.blocks.len()),
            action: Action::Manual,
            title: "t".into(),
            description: String::new(),
            screenshot: None,
            target: None,
            redactions: vec![],
            captured_unix_ms: 1,
            screenshot_withheld: false,
        }));
    }

    #[test]
    fn summary_wording() {
        assert_eq!(recording_summary(0, 0, 0, 0), "No steps were recorded.");
        assert_eq!(recording_summary(1, 0, 0, 0), "Recorded 1 step.");
        assert_eq!(
            recording_summary(12, 1, 1, 1),
            "Recorded 12 steps. 1 screenshot was withheld because the password check failed or could not finish. \
             1 step has no screenshot. 1 click or key press was missed."
        );
        assert_eq!(
            recording_summary(5, 2, 3, 4),
            "Recorded 5 steps. 2 screenshots were withheld because the password check failed or could not finish. \
             3 steps have no screenshot. 4 clicks or key presses were missed."
        );
    }

    #[test]
    fn a_new_recording_gets_the_line_for_its_keystroke_setting() {
        for (keys, want) in [(false, TYPED_TEXT_NOT_RECORDED), (true, TYPED_TEXT_RECORDED)] {
            let mut rec = Recording::new("r", "t", 0);
            rec.blocks.push(Block::Heading { id: "h".into(), level: 2, text: "Intro".into() });
            assert!(apply_typed_text_notice(&mut rec, keys));
            assert!(matches!(&rec.blocks[0], Block::Text { id, .. } if id == NOTICE_ID), "first block");
            assert_eq!(notice(&rec), Some(want));
            // A second start adds nothing.
            assert!(!apply_typed_text_notice(&mut rec, keys));
            assert_eq!(rec.blocks.len(), 2);
        }
    }

    #[test]
    fn neither_line_claims_the_file_is_safe() {
        for line in [TYPED_TEXT_NOT_RECORDED, TYPED_TEXT_RECORDED] {
            let l = line.to_lowercase();
            for claim in ["safe", "free of", "no sensitive", "nothing sensitive", "password"] {
                assert!(!l.contains(claim), "{line:?} contains {claim:?}");
            }
        }
    }

    #[test]
    fn a_deleted_notice_stays_deleted() {
        let mut rec = Recording::new("r", "t", 0);
        apply_typed_text_notice(&mut rec, false);
        add_step(&mut rec);
        rec.blocks.retain(|b| !matches!(b, Block::Text { id, .. } if id == NOTICE_ID));
        assert!(!apply_typed_text_notice(&mut rec, false));
        assert!(!apply_typed_text_notice(&mut rec, true));
        assert_eq!(notice(&rec), None);
    }

    #[test]
    fn turning_keystrokes_on_later_corrects_an_unedited_notice() {
        let mut rec = Recording::new("r", "t", 0);
        apply_typed_text_notice(&mut rec, false);
        add_step(&mut rec);
        assert!(apply_typed_text_notice(&mut rec, true));
        assert_eq!(notice(&rec), Some(TYPED_TEXT_RECORDED));
        // Back to off: typed text from the earlier session is still in the document.
        assert!(!apply_typed_text_notice(&mut rec, false));
        assert_eq!(notice(&rec), Some(TYPED_TEXT_RECORDED));
    }

    #[test]
    fn an_edited_notice_is_left_alone() {
        let mut rec = Recording::new("r", "t", 0);
        apply_typed_text_notice(&mut rec, false);
        add_step(&mut rec);
        if let Block::Text { text, .. } = &mut rec.blocks[0] {
            *text = "My own words.".into();
        }
        assert!(!apply_typed_text_notice(&mut rec, true));
        assert_eq!(notice(&rec), Some("My own words."));
    }

    #[test]
    fn an_older_recording_with_steps_gets_no_notice() {
        let mut rec = Recording::new("r", "t", 0);
        add_step(&mut rec);
        assert!(!apply_typed_text_notice(&mut rec, false));
        assert_eq!(notice(&rec), None);
    }
}

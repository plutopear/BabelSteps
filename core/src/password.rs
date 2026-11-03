//! The fail-closed password check (see docs/04-privacy-and-security.md §2).
//!
//! No single signal is trusted, and "could not check" counts as a failure.
//! The signals themselves come from platform code. This module only decides
//! what may be kept once they are known.

/// One answer to "is this a password field?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    No,
    Yes,
    /// The check failed or timed out.
    Unknown,
}

/// The three independent signals for one input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordSignals {
    /// The element under the event reports itself as a password field.
    pub element: Signal,
    /// The point is inside a password field found by the window scan.
    pub window_scan: Signal,
    /// The focused element reports itself as a password field.
    pub focused: Signal,
}

impl PasswordSignals {
    /// True only if all three signals are `No`. Anything else means typed text
    /// must be dropped.
    pub fn text_allowed(&self) -> bool {
        self.element == Signal::No && self.window_scan == Signal::No && self.focused == Signal::No
    }

    /// Whether a screenshot of this event may exist at all.
    ///
    /// Platform code must call this BEFORE writing any image, so that a
    /// withheld image never reaches the disk.
    ///
    /// False if any signal is `Unknown`. Also false if the element or the
    /// focused element says `Yes` while the window scan does not: the scan did
    /// not find that field, so its pixels cannot be assumed blacked out.
    pub fn screenshot_allowed(&self) -> bool {
        if self.any(Signal::Unknown) {
            return false;
        }
        let reported = self.element == Signal::Yes || self.focused == Signal::Yes;
        !reported || self.window_scan == Signal::Yes
    }

    /// True if any signal is `Yes`.
    pub fn any_yes(&self) -> bool {
        self.any(Signal::Yes)
    }

    fn any(&self, value: Signal) -> bool {
        self.element == value || self.window_scan == value || self.focused == value
    }
}

#[cfg(test)]
mod tests {
    use super::Signal::{No as N, Unknown as U, Yes as Y};
    use super::*;

    /// (element, window_scan, focused, text_allowed, screenshot_allowed).
    /// Written out by hand rather than derived, so it can disagree with the code.
    #[rustfmt::skip]
    const TABLE: [(Signal, Signal, Signal, bool, bool); 27] = [
        (N, N, N, true,  true ),
        (N, N, Y, false, false),
        (N, N, U, false, false),
        (N, Y, N, false, true ),
        (N, Y, Y, false, true ),
        (N, Y, U, false, false),
        (N, U, N, false, false),
        (N, U, Y, false, false),
        (N, U, U, false, false),
        (Y, N, N, false, false),
        (Y, N, Y, false, false),
        (Y, N, U, false, false),
        (Y, Y, N, false, true ),
        (Y, Y, Y, false, true ),
        (Y, Y, U, false, false),
        (Y, U, N, false, false),
        (Y, U, Y, false, false),
        (Y, U, U, false, false),
        (U, N, N, false, false),
        (U, N, Y, false, false),
        (U, N, U, false, false),
        (U, Y, N, false, false),
        (U, Y, Y, false, false),
        (U, Y, U, false, false),
        (U, U, N, false, false),
        (U, U, Y, false, false),
        (U, U, U, false, false),
    ];

    #[test]
    fn complete_truth_table() {
        for (element, window_scan, focused, text, shot) in TABLE {
            let s = PasswordSignals { element, window_scan, focused };
            assert_eq!(s.text_allowed(), text, "text_allowed for {s:?}");
            assert_eq!(s.screenshot_allowed(), shot, "screenshot_allowed for {s:?}");
        }
    }

    #[test]
    fn table_covers_all_27_combinations() {
        for element in [N, Y, U] {
            for window_scan in [N, Y, U] {
                for focused in [N, Y, U] {
                    let hits = TABLE
                        .iter()
                        .filter(|r| r.0 == element && r.1 == window_scan && r.2 == focused)
                        .count();
                    assert_eq!(hits, 1, "{element:?} {window_scan:?} {focused:?}");
                }
            }
        }
    }
}

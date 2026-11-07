//! Turns input events into finished steps (B1, B2, B3, B4, A11, B11).
//!
//! A pure state machine: the caller supplies timestamps and every signal, and
//! this code touches no clock, no disk and no platform API.
//!
//! The key rule is fail-closed typed text. The buffer holds characters only
//! while every key of the run has `text_allowed()`. Any other answer erases the
//! whole run, because earlier characters went into the same field and are
//! therefore suspect, and the run stays text-free until it ends.
//!
//! [`Event`] and [`Key`] deliberately do not implement `Debug`, so typed text
//! cannot be logged by accident: `{:?}` on one does not compile.

use crate::model::{Action, Block, MouseButton, StepBlock, Target};
use crate::password::PasswordSignals;
use crate::pii::{self, CustomRules};
use std::fmt;

/// Description of a run whose text was dropped because the check did not finish.
pub const TEXT_NOT_RECORDED: &str = "Text not recorded: could not check for passwords.";

#[derive(Debug, Clone)]
pub struct NormalizerConfig {
    /// A11: when false, characters are never buffered at all.
    pub keystrokes_enabled: bool,
    pub custom: CustomRules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Key {
    /// Text already translated by the platform layer. AltGr arrives here with no mods.
    Text(String),
    Enter,
    Tab,
    Backspace,
    Escape,
    /// A key with no text of its own, by name, such as `F4` or `Down`.
    Named(String),
    /// A key that produces no text, combined with mods.
    Char(char),
}

#[derive(Clone)]
pub enum Event {
    Click {
        t_ms: u64,
        button: MouseButton,
        x: i32,
        y: i32,
        target: Option<Target>,
        signals: PasswordSignals,
        screenshot: Option<String>,
    },
    Key {
        t_ms: u64,
        key: Key,
        mods: Mods,
        target: Option<Target>,
        signals: PasswordSignals,
        screenshot: Option<String>,
    },
    FocusChanged {
        t_ms: u64,
    },
    Pause,
    Resume,
    Comment {
        t_ms: u64,
        text: String,
    },
    ManualStep {
        t_ms: u64,
        title: String,
    },
}

/// The typed text of one field, while it is still being typed.
struct Run {
    started_ms: u64,
    /// Raw typed characters. Empty whenever `erased` is true or keystrokes are off.
    buffer: String,
    target: Option<Target>,
    screenshot: Option<String>,
    withheld: bool,
    /// Some key of this run did not have `text_allowed()`.
    erased: bool,
    saw_yes: bool,
}

impl Run {
    /// Applies one key's signals. Must run before that key's text is used.
    /// Returns the run's screenshot path if this key made it unusable.
    #[must_use]
    fn observe(&mut self, signals: &PasswordSignals) -> Option<String> {
        if !signals.text_allowed() {
            self.erased = true;
            wipe(&mut self.buffer);
        }
        if signals.any_yes() {
            self.saw_yes = true;
        }
        if !signals.screenshot_allowed() {
            self.withheld = true;
            return self.screenshot.take();
        }
        None
    }
}

/// Empties the string and overwrites its bytes. Best effort: earlier copies
/// left behind when the string grew are out of reach.
fn wipe(text: &mut String) {
    let mut bytes = std::mem::take(text).into_bytes();
    bytes.fill(0);
    std::hint::black_box(&bytes);
}

pub struct Normalizer {
    config: NormalizerConfig,
    next_step: u64,
    paused: bool,
    run: Option<Run>,
    /// Screenshot paths passed in that no emitted step uses.
    discarded: Vec<String>,
}

/// Leaves out the buffer, so logged state can never show typed text.
impl fmt::Debug for Normalizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Normalizer")
            .field("config", &self.config)
            .field("next_step", &self.next_step)
            .field("paused", &self.paused)
            .field("in_run", &self.run.is_some())
            .field("discarded", &self.discarded.len())
            .finish()
    }
}

impl Normalizer {
    pub fn new(config: NormalizerConfig) -> Self {
        Self { config, next_step: 1, paused: false, run: None, discarded: Vec::new() }
    }

    /// Screenshot paths that were passed in but ended up on no emitted step:
    /// withheld by a late signal, extra keys of a run, empty runs, events
    /// dropped while paused, and ignored keys. Drains the list.
    ///
    /// The caller must delete these files or, better, not write an image until
    /// its step is emitted, because a late signal can withhold it. Callers
    /// must call `password::PasswordSignals::screenshot_allowed` before writing.
    pub fn take_discarded_screenshots(&mut self) -> Vec<String> {
        std::mem::take(&mut self.discarded)
    }

    fn discard(&mut self, path: Option<String>) {
        self.discarded.extend(path);
    }

    /// Feeds one event and returns any steps it finished, in order.
    pub fn push(&mut self, event: Event) -> Vec<Block> {
        let mut out = Vec::new();
        if self.paused {
            // Nothing typed while paused may reach a step.
            match event {
                Event::Resume => self.paused = false,
                Event::Click { screenshot, .. } | Event::Key { screenshot, .. } => self.discard(screenshot),
                _ => {}
            }
            return out;
        }
        match event {
            Event::Click { t_ms, button, x, y, target, signals, screenshot } => {
                self.flush_run(&mut out);
                let target = self.scrub_target(target);
                let title = crate::model::describe_click(button, x, y, target.as_ref());
                let action = Action::Click { button, x, y };
                self.push_event_step(&mut out, t_ms, action, title, target, &signals, screenshot);
            }
            Event::Key { t_ms, key, mods, target, signals, screenshot } => {
                let target = self.scrub_target(target);
                self.on_key(&mut out, t_ms, key, mods, target, signals, screenshot);
            }
            Event::FocusChanged { .. } => self.flush_run(&mut out),
            Event::Resume => {} // Not paused, so there is nothing to resume.
            Event::Pause => {
                self.flush_run(&mut out);
                self.paused = true;
            }
            Event::Comment { t_ms, text } => {
                self.flush_run(&mut out);
                let title = pii::scrub(&text, &self.config.custom);
                self.push_plain_step(&mut out, t_ms, Action::Comment, title);
            }
            Event::ManualStep { t_ms, title } => {
                self.flush_run(&mut out);
                let title = pii::scrub(&title, &self.config.custom);
                self.push_plain_step(&mut out, t_ms, Action::Manual, title);
            }
        }
        out
    }

    /// Element names and window titles come from the screen and can hold
    /// customer data ("Signed in as a@b.co"), so they are scrubbed like typed text.
    fn scrub_target(&self, target: Option<Target>) -> Option<Target> {
        target.map(|mut t| {
            t.name = pii::scrub(&t.name, &self.config.custom);
            t.window_title = t.window_title.map(|w| pii::scrub(&w, &self.config.custom));
            t
        })
    }

    /// Ends the current run and returns its step, if it produces one.
    pub fn finish(&mut self) -> Vec<Block> {
        let mut out = Vec::new();
        self.flush_run(&mut out);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn on_key(
        &mut self,
        out: &mut Vec<Block>,
        t_ms: u64,
        key: Key,
        mods: Mods,
        target: Option<Target>,
        signals: PasswordSignals,
        screenshot: Option<String>,
    ) {
        // First, always: this key's signals judge the run it arrived in, even
        // if the key is the one that ends the run.
        let dropped = self.run.as_mut().and_then(|run| {
            let dropped = run.observe(&signals);
            if run.target.is_none() {
                run.target = target.clone();
            }
            dropped
        });
        self.discard(dropped);

        if mods.ctrl || mods.alt || mods.meta {
            // Ctrl+Alt is how AltGr arrives on many layouts, so this could be a
            // character of a password. Never name it unless typed text is
            // being recorded and this key's check passed.
            let printable = match &key {
                Key::Char(c) => !c.is_control(),
                Key::Text(t) => !t.is_empty(),
                _ => false,
            };
            let maybe_altgr = mods.ctrl && mods.alt && !mods.meta && printable;
            if maybe_altgr && !(self.config.keystrokes_enabled && signals.text_allowed()) {
                self.discard(screenshot);
                return;
            }
            let Some(combo) = combo_name(&key, mods) else {
                self.discard(screenshot);
                return;
            };
            self.flush_run(out);
            self.push_key_step(out, t_ms, combo, target, &signals, screenshot);
            return;
        }

        match key {
            Key::Text(text) => {
                if text.is_empty() {
                    self.discard(screenshot);
                    return;
                }
                let keep_text = self.config.keystrokes_enabled;
                let mut extra = screenshot;
                if self.run.is_none() {
                    let mut run = Run {
                        started_ms: t_ms,
                        buffer: String::new(),
                        target,
                        screenshot: extra.take(),
                        withheld: false,
                        erased: false,
                        saw_yes: false,
                    };
                    let dropped = run.observe(&signals);
                    self.run = Some(run);
                    self.discard(dropped);
                }
                // Only the first key's screenshot is kept.
                self.discard(extra);
                if let Some(run) = &mut self.run {
                    if keep_text && !run.erased {
                        run.buffer.push_str(&text);
                    }
                }
            }
            Key::Backspace => {
                if let Some(run) = &mut self.run {
                    run.buffer.pop();
                }
                self.discard(screenshot);
            }
            // The platform layer should have sent Text. Do not guess.
            Key::Char(_) => self.discard(screenshot),
            Key::Enter | Key::Tab | Key::Escape | Key::Named(_) => {
                self.flush_run(out);
                match combo_name(&key, mods) {
                    Some(combo) if self.config.keystrokes_enabled => {
                        self.push_key_step(out, t_ms, combo, target, &signals, screenshot);
                    }
                    _ => self.discard(screenshot),
                }
            }
        }
    }

    fn flush_run(&mut self, out: &mut Vec<Block>) {
        let Some(mut run) = self.run.take() else { return };
        let name = field_name(run.target.as_ref()).map(str::to_string);
        let (title, description, text);
        if run.erased || !self.config.keystrokes_enabled {
            text = String::new();
            if run.saw_yes {
                title = match &name {
                    Some(n) => format!("Enter password in \"{n}\""),
                    None => "Enter password".to_string(),
                };
                description = String::new();
            } else {
                title = typed_title(name.as_deref());
                // With keystrokes off the text was never going to be kept, so
                // the failed check is not the reason it is missing.
                description = if self.config.keystrokes_enabled && run.erased {
                    TEXT_NOT_RECORDED.to_string()
                } else {
                    String::new()
                };
            }
        } else {
            if run.buffer.is_empty() {
                // A run left empty produces no step.
                self.discard(run.screenshot.take());
                return;
            }
            text = pii::scrub(&run.buffer, &self.config.custom);
            wipe(&mut run.buffer);
            title = match &name {
                Some(n) => format!("Type \"{text}\" into \"{n}\""),
                None => format!("Type \"{text}\""),
            };
            description = String::new();
        }
        let mut target = run.target.take();
        mark_password(&mut target, run.saw_yes);
        let step = StepBlock {
            id: self.next_id(),
            action: Action::Text { text },
            title,
            description,
            screenshot: run.screenshot.take(),
            target,
            redactions: Vec::new(),
            captured_unix_ms: run.started_ms,
            screenshot_withheld: run.withheld,
        };
        out.push(Block::Step(step));
    }

    fn push_key_step(
        &mut self,
        out: &mut Vec<Block>,
        t_ms: u64,
        keys: String,
        target: Option<Target>,
        signals: &PasswordSignals,
        screenshot: Option<String>,
    ) {
        let title = match shortcut_action(&keys) {
            Some(action) => action.to_string(),
            None => format!("Press {keys}"),
        };
        self.push_event_step(out, t_ms, Action::Shortcut { keys }, title, target, signals, screenshot);
    }

    /// A step that stands for one click or key rather than a run.
    #[allow(clippy::too_many_arguments)]
    fn push_event_step(
        &mut self,
        out: &mut Vec<Block>,
        t_ms: u64,
        action: Action,
        title: String,
        mut target: Option<Target>,
        signals: &PasswordSignals,
        screenshot: Option<String>,
    ) {
        mark_password(&mut target, signals.any_yes());
        let allowed = signals.screenshot_allowed();
        if !allowed {
            self.discard(screenshot.clone());
        }
        let step = StepBlock {
            id: self.next_id(),
            action,
            title,
            description: String::new(),
            screenshot: screenshot.filter(|_| allowed),
            target,
            redactions: Vec::new(),
            captured_unix_ms: t_ms,
            screenshot_withheld: !allowed,
        };
        out.push(Block::Step(step));
    }

    fn push_plain_step(&mut self, out: &mut Vec<Block>, t_ms: u64, action: Action, title: String) {
        let step = StepBlock {
            id: self.next_id(),
            action,
            title,
            description: String::new(),
            screenshot: None,
            target: None,
            redactions: Vec::new(),
            captured_unix_ms: t_ms,
            screenshot_withheld: false,
        };
        out.push(Block::Step(step));
    }

    fn next_id(&mut self) -> String {
        let id = format!("s{}", self.next_step);
        self.next_step += 1;
        id
    }
}

fn mark_password(target: &mut Option<Target>, yes: bool) {
    if let (true, Some(t)) = (yes, target.as_mut()) {
        t.is_password = true;
    }
}

fn field_name(target: Option<&Target>) -> Option<&str> {
    target.map(|t| t.name.trim()).filter(|n| !n.is_empty())
}

fn typed_title(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("Typed in \"{n}\""),
        None => "Typed text".to_string(),
    }
}

/// `Ctrl+Shift+K` style name of a key press, or `None` if the key has no name
/// that is safe to show. Multi-character text is never turned into a name.
fn combo_name(key: &Key, mods: Mods) -> Option<String> {
    let key_name = match key {
        Key::Enter => "Enter".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Backspace => "Backspace".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Named(name) => {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            name.to_string()
        }
        Key::Char(c) => char_name(*c),
        Key::Text(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => char_name(c),
                _ => return None,
            }
        }
    };
    let mut combo = String::new();
    for (held, label) in [(mods.ctrl, "Ctrl+"), (mods.alt, "Alt+"), (mods.shift, "Shift+"), (mods.meta, "Meta+")] {
        if held {
            combo.push_str(label);
        }
    }
    combo.push_str(&key_name);
    Some(combo)
}

fn char_name(c: char) -> String {
    if c == ' ' {
        "Space".to_string()
    } else {
        c.to_uppercase().collect()
    }
}

/// The action name of a well-known shortcut (B3). Exact combinations only:
/// Ctrl+Shift+S is not Save.
fn shortcut_action(combo: &str) -> Option<&'static str> {
    Some(match combo.to_uppercase().as_str() {
        "CTRL+S" => "Save",
        "CTRL+C" => "Copy",
        "CTRL+V" => "Paste",
        "CTRL+X" => "Cut",
        "CTRL+Z" => "Undo",
        "CTRL+Y" => "Redo",
        "CTRL+A" => "Select all",
        "CTRL+F" => "Find",
        "CTRL+P" => "Print",
        "CTRL+N" => "New",
        "CTRL+O" => "Open",
        "CTRL+W" => "Close",
        "CTRL+T" => "New tab",
        "ALT+F4" => "Close window",
        "ALT+TAB" => "Switch window",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::password::Signal::{No, Unknown, Yes};

    const SECRET: &str = "hunter2";
    const CLEAN: PasswordSignals = PasswordSignals { element: No, window_scan: No, focused: No };
    /// A password field the window scan found, so its pixels are blacked out.
    const PW_SCANNED: PasswordSignals = PasswordSignals { element: Yes, window_scan: Yes, focused: Yes };
    /// A password field the window scan missed.
    const PW_UNSCANNED: PasswordSignals = PasswordSignals { element: No, window_scan: No, focused: Yes };
    const UNKNOWN: PasswordSignals = PasswordSignals { element: No, window_scan: Unknown, focused: No };

    fn normalizer(keystrokes_enabled: bool) -> Normalizer {
        Normalizer::new(NormalizerConfig { keystrokes_enabled, custom: CustomRules::none() })
    }

    fn field(name: &str) -> Option<Target> {
        Some(Target { name: name.to_string(), control_type: "Edit".to_string(), process_name: "app.exe".to_string(), ..Default::default() })
    }

    fn key_event(key: Key, mods: Mods, signals: PasswordSignals, shot: Option<&str>) -> Event {
        Event::Key { t_ms: 10, key, mods, target: field("Box"), signals, screenshot: shot.map(str::to_string) }
    }

    fn key(key: Key, signals: PasswordSignals) -> Event {
        key_event(key, Mods::default(), signals, None)
    }

    fn text(s: &str, signals: PasswordSignals) -> Event {
        key(Key::Text(s.to_string()), signals)
    }

    fn ctrl(c: char) -> Event {
        key_event(Key::Char(c), Mods { ctrl: true, ..Mods::default() }, CLEAN, None)
    }

    fn click(signals: PasswordSignals, shot: Option<&str>) -> Event {
        Event::Click {
            t_ms: 20,
            button: MouseButton::Left,
            x: 1,
            y: 2,
            target: field("OK"),
            signals,
            screenshot: shot.map(str::to_string),
        }
    }

    /// Types one character per event, the way a real keyboard delivers them.
    fn type_chars(n: &mut Normalizer, s: &str, signals: PasswordSignals, out: &mut Vec<Block>) {
        for c in s.chars() {
            out.extend(n.push(text(&c.to_string(), signals)));
        }
    }

    fn steps(blocks: &[Block]) -> Vec<&StepBlock> {
        blocks
            .iter()
            .map(|b| match b {
                Block::Step(s) => s,
                other => panic!("expected a step, got {other:?}"),
            })
            .collect()
    }

    fn titles(blocks: &[Block]) -> Vec<&str> {
        steps(blocks).iter().map(|s| s.title.as_str()).collect()
    }

    fn one(blocks: &[Block]) -> &StepBlock {
        let all = steps(blocks);
        assert_eq!(all.len(), 1, "expected one step: {blocks:?}");
        all[0]
    }

    /// Searches the state's Debug output and the blocks' Debug and JSON output,
    /// for the whole secret and for pieces of it.
    fn assert_absent(n: &Normalizer, blocks: &[Block], secret: &str) {
        let haystacks = [format!("{n:?}"), format!("{blocks:?}"), serde_json::to_string(blocks).unwrap()];
        let mut needles = vec![secret.to_string()];
        if secret == SECRET {
            needles.push(secret[..3].to_string());
            needles.push(secret[secret.len() - 3..].to_string());
        }
        for hay in &haystacks {
            for needle in &needles {
                assert!(!hay.contains(needle.as_str()), "found {needle:?} in {hay}");
            }
        }
    }

    // ---- The checker itself must be able to fail ----

    #[test]
    fn an_ordinary_run_does_keep_the_text_so_the_search_can_find_it() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, SECRET, CLEAN, &mut out);
        out.extend(n.finish());
        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains(SECRET), "control failed: {json}");
        let caught = std::panic::catch_unwind(|| assert_absent(&normalizer(true), &out, SECRET));
        assert!(caught.is_err(), "assert_absent did not notice the secret");
    }

    #[test]
    fn debug_of_state_hides_the_buffer_of_an_ordinary_run() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, SECRET, CLEAN, &mut out);
        assert!(out.is_empty());
        assert_absent(&n, &out, SECRET);
    }

    // ---- Every path a secret could take ----

    #[test]
    fn password_run_keeps_no_text() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, SECRET, PW_SCANNED, &mut out);
        assert_absent(&n, &out, SECRET);
        out.extend(n.push(key(Key::Enter, PW_SCANNED)));
        out.extend(n.finish());
        assert_absent(&n, &out, SECRET);

        let all = steps(&out);
        assert_eq!(all[0].title, "Enter password in \"Box\"");
        assert_eq!(all[0].description, "");
        assert_eq!(all[0].action, Action::Text { text: String::new() });
        assert!(all[0].target.as_ref().unwrap().is_password);
        assert_eq!(all[1].title, "Press Enter");
    }

    #[test]
    fn password_run_without_a_name_has_the_short_title() {
        let mut n = normalizer(true);
        n.push(Event::Key {
            t_ms: 1,
            key: Key::Text(SECRET.to_string()),
            mods: Mods::default(),
            target: None,
            signals: PW_SCANNED,
            screenshot: None,
        });
        let out = n.finish();
        assert_eq!(one(&out).title, "Enter password");
        assert_absent(&n, &out, SECRET);
    }

    #[test]
    fn unknown_signal_mid_run_erases_everything() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, "hun", CLEAN, &mut out);
        type_chars(&mut n, "ter2", UNKNOWN, &mut out);
        assert_absent(&n, &out, SECRET);
        out.extend(n.finish());
        assert_absent(&n, &out, SECRET);
        let step = one(&out);
        assert_eq!(step.title, "Typed in \"Box\"");
        assert_eq!(step.description, TEXT_NOT_RECORDED);
        assert_eq!(step.action, Action::Text { text: String::new() });
        assert!(!step.target.as_ref().unwrap().is_password);
    }

    #[test]
    fn typing_abc_then_a_mid_run_yes_leaves_no_abc() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, "abc", CLEAN, &mut out);
        assert_eq!(n.run.as_ref().unwrap().buffer, "abc");
        type_chars(&mut n, "d", PW_SCANNED, &mut out);
        // Erased at once, not only when the step is built.
        assert_eq!(n.run.as_ref().unwrap().buffer, "");
        out.extend(n.finish());
        assert_absent(&n, &out, "abc");
        let step = one(&out);
        assert_eq!(step.title, "Enter password in \"Box\"");
        assert_eq!(step.action, Action::Text { text: String::new() });
        assert!(step.target.as_ref().unwrap().is_password);
    }

    #[test]
    fn the_run_stays_text_free_after_it_is_erased() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, "a", UNKNOWN, &mut out);
        type_chars(&mut n, SECRET, CLEAN, &mut out);
        assert_eq!(n.run.as_ref().unwrap().buffer, "");
        out.extend(n.finish());
        assert_absent(&n, &out, SECRET);
        assert_eq!(one(&out).description, TEXT_NOT_RECORDED);
    }

    #[test]
    fn a_new_run_after_an_erased_one_keeps_text_again() {
        let mut n = normalizer(true);
        let mut out = n.push(text(SECRET, PW_SCANNED));
        out.extend(n.push(Event::FocusChanged { t_ms: 3 }));
        assert_absent(&n, &out, SECRET);
        out.extend(n.push(text("plain", CLEAN)));
        out.extend(n.finish());
        assert_eq!(titles(&out), ["Enter password in \"Box\"", "Type \"plain\" into \"Box\""]);
    }

    #[test]
    fn every_kind_of_later_key_can_erase_the_run() {
        let makers: Vec<(&str, Event)> = vec![
            ("Enter", key(Key::Enter, PW_UNSCANNED)),
            ("Tab", key(Key::Tab, UNKNOWN)),
            ("Backspace", key(Key::Backspace, PW_UNSCANNED)),
            ("Escape", key(Key::Escape, UNKNOWN)),
            ("Named", key(Key::Named("Down".to_string()), PW_UNSCANNED)),
            ("bare Char", key(Key::Char('q'), UNKNOWN)),
            ("shortcut", key_event(Key::Char('v'), Mods { ctrl: true, ..Mods::default() }, PW_UNSCANNED, None)),
        ];
        for (label, event) in makers {
            let mut n = normalizer(true);
            let mut out = n.push(text(SECRET, CLEAN));
            out.extend(n.push(event));
            out.extend(n.finish());
            assert_absent(&n, &out, SECRET);
            let first = steps(&out)[0];
            assert_eq!(first.action, Action::Text { text: String::new() }, "{label}");
            assert!(first.screenshot_withheld, "{label}");
        }
    }

    #[test]
    fn keystrokes_off_never_buffers() {
        let mut n = normalizer(false);
        let mut out = Vec::new();
        type_chars(&mut n, SECRET, CLEAN, &mut out);
        assert_eq!(n.run.as_ref().unwrap().buffer, "");
        assert_absent(&n, &out, SECRET);
        out.extend(n.finish());
        assert_absent(&n, &out, SECRET);
        let step = one(&out);
        assert_eq!(step.title, "Typed in \"Box\"");
        assert_eq!(step.description, "");
        assert_eq!(step.action, Action::Text { text: String::new() });
    }

    #[test]
    fn keystrokes_off_still_names_password_fields_and_withholds() {
        let mut n = normalizer(false);
        n.push(text(SECRET, PW_UNSCANNED));
        let out = n.finish();
        let step = one(&out);
        assert_eq!(step.title, "Enter password in \"Box\"");
        assert!(step.screenshot_withheld);
        assert!(step.target.as_ref().unwrap().is_password);
        assert_absent(&n, &out, SECRET);
    }

    #[test]
    fn keystrokes_off_emits_only_shortcuts_with_ctrl_alt_or_meta() {
        let mut n = normalizer(false);
        let mut out = n.push(key(Key::Enter, CLEAN));
        out.extend(n.push(key(Key::Tab, CLEAN)));
        out.extend(n.push(key(Key::Escape, CLEAN)));
        out.extend(n.push(key(Key::Named("Down".to_string()), CLEAN)));
        assert!(out.is_empty(), "{out:?}");

        out.extend(n.push(text(SECRET, CLEAN)));
        out.extend(n.push(ctrl('s')));
        out.extend(n.push(key(Key::Enter, CLEAN)));
        assert_eq!(titles(&out), ["Typed in \"Box\"", "Save"]);
        assert_absent(&n, &out, SECRET);
    }

    #[test]
    fn nothing_typed_while_paused_reaches_a_step() {
        let mut n = normalizer(true);
        let mut out = n.push(Event::Pause);
        out.extend(n.push(text(SECRET, CLEAN)));
        out.extend(n.push(key(Key::Enter, CLEAN)));
        out.extend(n.push(ctrl('s')));
        out.extend(n.push(click(CLEAN, None)));
        out.extend(n.push(Event::Comment { t_ms: 1, text: SECRET.to_string() }));
        out.extend(n.push(Event::ManualStep { t_ms: 1, title: SECRET.to_string() }));
        out.extend(n.push(Event::FocusChanged { t_ms: 1 }));
        assert!(out.is_empty(), "{out:?}");
        assert_absent(&n, &out, SECRET);
        out.extend(n.push(Event::Resume));
        out.extend(n.finish());
        assert!(out.is_empty(), "{out:?}");

        out.extend(n.push(text("after", CLEAN)));
        out.extend(n.finish());
        assert_eq!(titles(&out), ["Type \"after\" into \"Box\""]);
        assert_eq!(one(&out).id, "s1");
    }

    #[test]
    fn pause_flushes_the_current_run_first() {
        let mut n = normalizer(true);
        n.push(text("before", CLEAN));
        let out = n.push(Event::Pause);
        assert_eq!(titles(&out), ["Type \"before\" into \"Box\""]);
        assert!(n.run.is_none());
    }

    #[test]
    fn element_names_and_manual_titles_are_scrubbed() {
        let mut n = normalizer(true);
        let leaky = Target {
            name: "Signed in as me@example.com".to_string(),
            window_title: Some("Ticket for me@example.com".to_string()),
            process_name: "app.exe".to_string(),
            ..Default::default()
        };
        let mut out = n.push(Event::Click {
            t_ms: 1,
            button: crate::model::MouseButton::Left,
            x: 1,
            y: 2,
            target: Some(leaky),
            signals: CLEAN,
            screenshot: None,
        });
        out.extend(n.push(Event::ManualStep { t_ms: 2, title: "Mail me@example.com".to_string() }));
        assert_eq!(out.len(), 2);
        assert_absent(&n, &out, "me@example.com");
    }

    #[test]
    fn scrubbed_text_is_all_that_is_kept() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, "mail me@example.com", CLEAN, &mut out);
        out.extend(n.finish());
        assert_absent(&n, &out, "me@example.com");
        let step = one(&out);
        assert_eq!(step.title, "Type \"mail [email]\" into \"Box\"");
        assert_eq!(step.action, Action::Text { text: "mail [email]".to_string() });
    }

    #[test]
    fn custom_rules_apply_and_stay_out_of_debug() {
        let custom = CustomRules::new(["acme"], [] as [&str; 0]).unwrap();
        let mut n = Normalizer::new(NormalizerConfig { keystrokes_enabled: true, custom });
        n.push(text("for acme", CLEAN));
        let out = n.finish();
        assert_eq!(one(&out).action, Action::Text { text: "for [custom]".to_string() });
        assert_absent(&n, &out, "acme");
    }

    #[test]
    fn comment_text_is_scrubbed() {
        let mut n = normalizer(true);
        let out = n.push(Event::Comment { t_ms: 5, text: "ask me@example.com".to_string() });
        let step = one(&out);
        assert_eq!(step.title, "ask [email]");
        assert_eq!(step.action, Action::Comment);
        assert_absent(&n, &out, "me@example.com");
    }

    // ---- Runs ----

    #[test]
    fn enter_and_tab_end_the_run_and_get_their_own_step() {
        for (k, press) in [(Key::Enter, "Press Enter"), (Key::Tab, "Press Tab")] {
            let mut n = normalizer(true);
            n.push(text("hi", CLEAN));
            let out = n.push(key(k, CLEAN));
            assert_eq!(titles(&out), ["Type \"hi\" into \"Box\"", press]);
            let all = steps(&out);
            assert_eq!(all[0].action, Action::Text { text: "hi".to_string() });
            assert!(matches!(all[1].action, Action::Shortcut { .. }));
        }
    }

    #[test]
    fn run_ends_on_focus_change_click_shortcut_and_finish() {
        let enders = [Event::FocusChanged { t_ms: 1 }, click(CLEAN, None), ctrl('k')];
        for ender in enders {
            let mut n = normalizer(true);
            n.push(text("hi", CLEAN));
            let out = n.push(ender);
            assert_eq!(steps(&out)[0].title, "Type \"hi\" into \"Box\"");
            assert!(n.run.is_none());
        }
        let mut n = normalizer(true);
        n.push(text("hi", CLEAN));
        assert_eq!(titles(&n.finish()), ["Type \"hi\" into \"Box\""]);
        assert!(n.finish().is_empty());
    }

    #[test]
    fn title_without_a_target_name() {
        for target in [None, field("  ")] {
            let mut n = normalizer(true);
            n.push(Event::Key {
                t_ms: 1,
                key: Key::Text("hi".to_string()),
                mods: Mods::default(),
                target,
                signals: CLEAN,
                screenshot: None,
            });
            assert_eq!(titles(&n.finish()), ["Type \"hi\""]);
        }
    }

    #[test]
    fn backspace_edits_the_buffer() {
        let mut n = normalizer(true);
        n.push(text("abc", CLEAN));
        n.push(key(Key::Backspace, CLEAN));
        n.push(text("x", CLEAN));
        assert_eq!(titles(&n.finish()), ["Type \"abx\" into \"Box\""]);
    }

    #[test]
    fn backspace_on_empty_is_ignored_and_empty_runs_make_no_step() {
        let mut n = normalizer(true);
        assert!(n.push(key(Key::Backspace, CLEAN)).is_empty());
        assert!(n.run.is_none());
        n.push(text("ab", CLEAN));
        n.push(key(Key::Backspace, CLEAN));
        n.push(key(Key::Backspace, CLEAN));
        n.push(key(Key::Backspace, CLEAN));
        assert!(n.finish().is_empty());
        // The next step still takes the first id.
        n.push(text("c", CLEAN));
        assert_eq!(one(&n.finish()).id, "s1");
    }

    #[test]
    fn enter_after_an_emptied_run_is_only_a_press() {
        let mut n = normalizer(true);
        n.push(text("a", CLEAN));
        n.push(key(Key::Backspace, CLEAN));
        assert_eq!(titles(&n.push(key(Key::Enter, CLEAN))), ["Press Enter"]);
    }

    #[test]
    fn empty_text_key_starts_no_run() {
        let mut n = normalizer(true);
        n.push(text("", CLEAN));
        assert!(n.run.is_none());
    }

    // ---- Shortcuts ----

    #[test]
    fn named_shortcuts() {
        let table = [
            ('s', "Save"),
            ('c', "Copy"),
            ('v', "Paste"),
            ('x', "Cut"),
            ('z', "Undo"),
            ('y', "Redo"),
            ('a', "Select all"),
            ('f', "Find"),
            ('p', "Print"),
            ('n', "New"),
            ('o', "Open"),
            ('w', "Close"),
            ('t', "New tab"),
        ];
        for (c, name) in table {
            for c in [c, c.to_ascii_uppercase()] {
                let mut n = normalizer(true);
                let out = n.push(ctrl(c));
                let step = one(&out);
                assert_eq!(step.title, name);
                assert_eq!(step.action, Action::Shortcut { keys: format!("Ctrl+{}", c.to_ascii_uppercase()) });
            }
        }
    }

    #[test]
    fn window_shortcuts_and_general_format() {
        let alt = Mods { alt: true, ..Mods::default() };
        let cases = [
            (Key::Named("F4".to_string()), alt, "Close window", "Alt+F4"),
            (Key::Tab, alt, "Switch window", "Alt+Tab"),
            (Key::Char('k'), Mods { ctrl: true, shift: true, ..Mods::default() }, "Press Ctrl+Shift+K", "Ctrl+Shift+K"),
            // Not Save: exact combinations only.
            (Key::Char('s'), Mods { ctrl: true, shift: true, ..Mods::default() }, "Press Ctrl+Shift+S", "Ctrl+Shift+S"),
            (Key::Char('r'), Mods { meta: true, ..Mods::default() }, "Press Meta+R", "Meta+R"),
            (Key::Char(' '), Mods { ctrl: true, ..Mods::default() }, "Press Ctrl+Space", "Ctrl+Space"),
            (Key::Backspace, Mods { ctrl: true, ..Mods::default() }, "Press Ctrl+Backspace", "Ctrl+Backspace"),
            (Key::Text("g".to_string()), Mods { ctrl: true, ..Mods::default() }, "Press Ctrl+G", "Ctrl+G"),
        ];
        for (k, mods, title, keys) in cases {
            let mut n = normalizer(true);
            let out = n.push(key_event(k, mods, CLEAN, None));
            let step = one(&out);
            assert_eq!(step.title, title);
            assert_eq!(step.action, Action::Shortcut { keys: keys.to_string() });
        }
    }

    #[test]
    fn bare_escape_and_named_keys_are_presses_that_end_the_run() {
        for (k, press) in [(Key::Escape, "Press Esc"), (Key::Named("Down".to_string()), "Press Down")] {
            let mut n = normalizer(true);
            n.push(text("hi", CLEAN));
            let out = n.push(key(k, CLEAN));
            assert_eq!(titles(&out), ["Type \"hi\" into \"Box\"", press]);
        }
    }

    #[test]
    fn keys_with_ctrl_alt_or_meta_never_enter_the_buffer() {
        for mods in [
            Mods { ctrl: true, ..Mods::default() },
            Mods { alt: true, ..Mods::default() },
            Mods { meta: true, ..Mods::default() },
        ] {
            let mut n = normalizer(true);
            let out = n.push(key_event(Key::Text("q".to_string()), mods, CLEAN, None));
            assert!(n.run.is_none());
            assert_eq!(out.len(), 1);
            // Longer text is not a key name, and must not become one.
            let out = n.push(key_event(Key::Text(SECRET.to_string()), mods, CLEAN, None));
            assert!(out.is_empty());
            assert!(n.run.is_none());
            assert_absent(&n, &out, SECRET);
        }
    }

    #[test]
    fn char_without_ctrl_alt_or_meta_is_neither_text_nor_a_step() {
        for mods in [Mods::default(), Mods { shift: true, ..Mods::default() }] {
            let mut n = normalizer(true);
            assert!(n.push(key_event(Key::Char('q'), mods, CLEAN, None)).is_empty());
            assert!(n.run.is_none());
            n.push(text("hi", CLEAN));
            assert!(n.push(key_event(Key::Char('q'), mods, CLEAN, None)).is_empty());
            // It does not end the run or add to it.
            n.push(text("!", CLEAN));
            assert_eq!(titles(&n.finish()), ["Type \"hi!\" into \"Box\""]);
        }
    }

    // ---- Screenshots ----

    #[test]
    fn run_screenshot_comes_from_its_first_event() {
        let mut n = normalizer(true);
        n.push(key_event(Key::Text("a".to_string()), Mods::default(), CLEAN, Some("first.png")));
        n.push(key_event(Key::Text("b".to_string()), Mods::default(), CLEAN, Some("second.png")));
        let out = n.finish();
        let step = one(&out);
        assert_eq!(step.screenshot.as_deref(), Some("first.png"));
        assert!(!step.screenshot_withheld);
    }

    #[test]
    fn withheld_wins_over_an_earlier_screenshot() {
        for bad in [UNKNOWN, PW_UNSCANNED] {
            let mut n = normalizer(true);
            n.push(key_event(Key::Text("a".to_string()), Mods::default(), CLEAN, Some("first.png")));
            n.push(key_event(Key::Text("b".to_string()), Mods::default(), bad, Some("second.png")));
            n.push(key_event(Key::Text("c".to_string()), Mods::default(), CLEAN, Some("third.png")));
            let out = n.finish();
            let step = one(&out);
            assert_eq!(step.screenshot, None);
            assert!(step.screenshot_withheld);
            assert_absent(&n, &out, ".png");
        }
    }

    #[test]
    fn scanned_password_field_keeps_its_blacked_out_screenshot() {
        let mut n = normalizer(true);
        n.push(key_event(Key::Text(SECRET.to_string()), Mods::default(), PW_SCANNED, Some("shot.png")));
        let out = n.finish();
        let step = one(&out);
        assert_eq!(step.screenshot.as_deref(), Some("shot.png"));
        assert!(!step.screenshot_withheld);
    }

    #[test]
    fn first_key_with_a_bad_signal_withholds_and_keeps_no_text() {
        let mut n = normalizer(true);
        n.push(key_event(Key::Text(SECRET.to_string()), Mods::default(), UNKNOWN, Some("shot.png")));
        let out = n.finish();
        let step = one(&out);
        assert_eq!(step.screenshot, None);
        assert!(step.screenshot_withheld);
        assert_absent(&n, &out, SECRET);
    }

    #[test]
    fn click_screenshot_and_password_flag() {
        let mut n = normalizer(true);
        let ok = n.push(click(CLEAN, Some("c.png")));
        assert_eq!(one(&ok).screenshot.as_deref(), Some("c.png"));
        assert!(!one(&ok).screenshot_withheld);
        assert!(!one(&ok).target.as_ref().unwrap().is_password);

        let bad = n.push(click(UNKNOWN, Some("c.png")));
        assert_eq!(one(&bad).screenshot, None);
        assert!(one(&bad).screenshot_withheld);

        let pw = n.push(click(PW_UNSCANNED, Some("c.png")));
        assert_eq!(one(&pw).screenshot, None);
        assert!(one(&pw).screenshot_withheld);
        assert!(one(&pw).target.as_ref().unwrap().is_password);

        let pw_ok = n.push(click(PW_SCANNED, Some("c.png")));
        assert_eq!(one(&pw_ok).screenshot.as_deref(), Some("c.png"));
        assert!(one(&pw_ok).target.as_ref().unwrap().is_password);
    }

    #[test]
    fn a_click_does_not_judge_the_run_before_it() {
        let mut n = normalizer(true);
        n.push(text("hi", CLEAN));
        let out = n.push(click(UNKNOWN, None));
        let all = steps(&out);
        assert_eq!(all[0].title, "Type \"hi\" into \"Box\"");
        assert!(!all[0].screenshot_withheld);
        assert!(all[1].screenshot_withheld);
    }

    #[test]
    fn press_step_uses_its_own_screenshot_and_signals() {
        let mut n = normalizer(true);
        n.push(key_event(Key::Text("hi".to_string()), Mods::default(), CLEAN, Some("run.png")));
        let out = n.push(key_event(Key::Enter, Mods::default(), CLEAN, Some("enter.png")));
        let all = steps(&out);
        assert_eq!(all[0].screenshot.as_deref(), Some("run.png"));
        assert_eq!(all[1].screenshot.as_deref(), Some("enter.png"));
    }

    // ---- Discarded screenshots ----

    fn shot_of(e: &Event) -> Option<String> {
        match e {
            Event::Click { screenshot, .. } | Event::Key { screenshot, .. } => screenshot.clone(),
            _ => None,
        }
    }

    /// Every path passed in must be on exactly one emitted step or in the
    /// discarded list: never both, never neither, never twice.
    fn assert_every_shot_accounted(label: &str, events: Vec<Event>) {
        let passed: Vec<String> = events.iter().filter_map(shot_of).collect();
        let mut n = normalizer(true);
        let mut out = Vec::new();
        for e in events {
            out.extend(n.push(e));
        }
        out.extend(n.finish());
        let used: Vec<String> = steps(&out).iter().filter_map(|s| s.screenshot.clone()).collect();
        let discarded = n.take_discarded_screenshots();
        for p in &passed {
            let in_steps = used.iter().filter(|u| *u == p).count();
            let in_list = discarded.iter().filter(|d| *d == p).count();
            assert_eq!(in_steps + in_list, 1, "{label}: {p} used {in_steps}x, discarded {in_list}x");
        }
        assert_eq!(used.len() + discarded.len(), passed.len(), "{label}: stray paths {used:?} {discarded:?}");
        assert!(n.take_discarded_screenshots().is_empty(), "{label}: list not drained");
    }

    #[test]
    fn every_screenshot_is_on_a_step_or_discarded() {
        let k = |key: Key, mods: Mods, sig: PasswordSignals, p: &str| key_event(key, mods, sig, Some(p));
        let none = Mods::default();
        let ctrl_alt = Mods { ctrl: true, alt: true, ..none };
        let t = |c: &str| Key::Text(c.to_string());
        let cases: Vec<(&str, Vec<Event>)> = vec![
            ("click kept", vec![click(CLEAN, Some("a"))]),
            ("click withheld", vec![click(UNKNOWN, Some("a"))]),
            ("click in unscanned password", vec![click(PW_UNSCANNED, Some("a"))]),
            ("run, later keys dropped", vec![k(t("x"), none, CLEAN, "a"), k(t("y"), none, CLEAN, "b"), k(t("z"), none, CLEAN, "c")]),
            ("late Unknown withholds the held path", vec![k(t("x"), none, CLEAN, "a"), k(t("y"), none, UNKNOWN, "b")]),
            ("late Yes withholds the held path", vec![k(t("x"), none, CLEAN, "a"), k(t("y"), none, PW_UNSCANNED, "b"), k(t("z"), none, CLEAN, "c")]),
            ("first key withheld", vec![k(t("x"), none, UNKNOWN, "a"), k(t("y"), none, CLEAN, "b")]),
            ("late signal on a shortcut ends the run", vec![k(t("x"), none, CLEAN, "a"), k(Key::Char('v'), Mods { ctrl: true, ..none }, PW_UNSCANNED, "b")]),
            ("late signal on Enter", vec![k(t("x"), none, CLEAN, "a"), k(Key::Enter, none, UNKNOWN, "b")]),
            ("empty run", vec![k(t("x"), none, CLEAN, "a"), k(Key::Backspace, none, CLEAN, "b"), Event::FocusChanged { t_ms: 1 }]),
            ("empty text key", vec![k(t(""), none, CLEAN, "a")]),
            ("ignored Char", vec![k(Key::Char('q'), none, CLEAN, "a")]),
            ("backspace with no run", vec![k(Key::Backspace, none, CLEAN, "a")]),
            ("unnameable combo", vec![k(t("long text"), Mods { ctrl: true, ..none }, CLEAN, "a")]),
            ("run then Enter and Tab", vec![k(t("x"), none, CLEAN, "a"), k(Key::Enter, none, CLEAN, "b"), k(Key::Tab, none, CLEAN, "c")]),
            ("AltGr char refused", vec![k(Key::Char('q'), ctrl_alt, UNKNOWN, "a")]),
            ("AltGr char inside a run", vec![k(t("x"), none, CLEAN, "a"), k(Key::Char('q'), ctrl_alt, PW_UNSCANNED, "b")]),
            ("paused", vec![Event::Pause, click(CLEAN, Some("a")), k(t("x"), none, CLEAN, "b"), k(Key::Enter, none, CLEAN, "c"), Event::Resume, click(CLEAN, Some("d"))]),
            ("pause flushes the run, then drops", vec![k(t("x"), none, CLEAN, "a"), Event::Pause, k(t("y"), none, CLEAN, "b")]),
        ];
        for (label, events) in cases {
            assert_every_shot_accounted(label, events);
        }
    }

    #[test]
    fn discard_list_has_the_expected_paths() {
        let mut n = normalizer(true);
        n.push(key_event(Key::Text("a".into()), Mods::default(), CLEAN, Some("first.png")));
        n.push(key_event(Key::Text("b".into()), Mods::default(), CLEAN, Some("second.png")));
        assert_eq!(n.take_discarded_screenshots(), ["second.png"]);
        n.push(key_event(Key::Text("c".into()), Mods::default(), UNKNOWN, Some("third.png")));
        assert_eq!(n.take_discarded_screenshots(), ["first.png", "third.png"]);
        n.finish();
        assert!(n.take_discarded_screenshots().is_empty());
    }

    #[test]
    fn keystrokes_off_discards_unused_press_screenshots() {
        let mut n = normalizer(false);
        n.push(key_event(Key::Enter, Mods::default(), CLEAN, Some("e.png")));
        assert_eq!(n.take_discarded_screenshots(), ["e.png"]);
    }

    // ---- Possible AltGr ----

    const CTRL_ALT: Mods = Mods { ctrl: true, alt: true, shift: false, meta: false };

    #[test]
    fn ctrl_alt_printable_in_a_password_field_is_never_named() {
        for keystrokes in [true, false] {
            for sig in [PW_UNSCANNED, PW_SCANNED, UNKNOWN] {
                let mut n = normalizer(keystrokes);
                let mut out = n.push(key_event(Key::Char('h'), CTRL_ALT, sig, Some("s.png")));
                out.extend(n.finish());
                assert!(out.is_empty(), "keystrokes={keystrokes}: {out:?}");
                assert_absent(&n, &out, "Ctrl+Alt+H");
                assert_eq!(n.take_discarded_screenshots(), ["s.png"]);
            }
        }
    }

    #[test]
    fn ctrl_alt_text_in_a_password_field_is_never_named() {
        // Platform layers send printable keys as `Text`, so AltGr usually arrives this way.
        for keystrokes in [true, false] {
            for sig in [PW_UNSCANNED, PW_SCANNED, UNKNOWN] {
                let mut n = normalizer(keystrokes);
                let mut out = n.push(key_event(Key::Text("h".into()), CTRL_ALT, sig, Some("s.png")));
                out.extend(n.finish());
                assert!(out.is_empty(), "keystrokes={keystrokes}: {out:?}");
                assert_absent(&n, &out, "Ctrl+Alt+H");
                assert_eq!(n.take_discarded_screenshots(), ["s.png"]);
            }
        }
        let mut n = normalizer(false);
        let out = n.push(key_event(Key::Text("h".into()), CTRL_ALT, CLEAN, None));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn ctrl_alt_printable_with_keystrokes_off_is_never_named() {
        let mut n = normalizer(false);
        let out = n.push(key_event(Key::Char('h'), CTRL_ALT, CLEAN, None));
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn ctrl_alt_printable_with_a_clean_check_is_still_named() {
        let mut n = normalizer(true);
        let out = n.push(key_event(Key::Char('h'), CTRL_ALT, CLEAN, None));
        assert_eq!(one(&out).title, "Press Ctrl+Alt+H");
    }

    #[test]
    fn ctrl_alt_with_a_named_key_is_unaffected() {
        let mut n = normalizer(false);
        let out = n.push(key_event(Key::Named("Delete".into()), CTRL_ALT, CLEAN, None));
        assert_eq!(one(&out).title, "Press Ctrl+Alt+Delete");
    }

    #[test]
    fn a_not_allowed_altgr_key_taints_the_run_and_adds_no_step() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        type_chars(&mut n, SECRET, CLEAN, &mut out);
        out.extend(n.push(key_event(Key::Char('q'), CTRL_ALT, UNKNOWN, None)));
        assert!(out.is_empty());
        assert_eq!(n.run.as_ref().unwrap().buffer, "");
        out.extend(n.finish());
        assert_absent(&n, &out, SECRET);
        let step = one(&out);
        assert_eq!(step.description, TEXT_NOT_RECORDED);
        assert!(step.screenshot_withheld);
    }

    #[test]
    fn other_combos_in_a_password_field_are_still_named() {
        let mut n = normalizer(true);
        let out = n.push(key_event(Key::Char('v'), Mods { ctrl: true, ..Mods::default() }, PW_UNSCANNED, None));
        let step = one(&out);
        assert_eq!(step.title, "Paste");
        assert!(step.screenshot_withheld);
        // Alt alone, Ctrl+Alt+Meta and non-printable keys are not the AltGr case.
        let out = n.push(key_event(Key::Char('q'), Mods { alt: true, ..Mods::default() }, PW_UNSCANNED, None));
        assert_eq!(one(&out).title, "Press Alt+Q");
    }

    // ---- Ordering ----

    #[test]
    fn steps_keep_their_order_and_get_sequential_ids() {
        let mut n = normalizer(true);
        let mut out = Vec::new();
        out.extend(n.push(click(CLEAN, None)));
        out.extend(n.push(text("one", CLEAN)));
        out.extend(n.push(ctrl('s')));
        out.extend(n.push(text("two", CLEAN)));
        out.extend(n.push(Event::Comment { t_ms: 30, text: "a note".to_string() }));
        out.extend(n.push(text("three", CLEAN)));
        out.extend(n.push(Event::ManualStep { t_ms: 40, title: "Do this by hand".to_string() }));
        out.extend(n.push(text("four", CLEAN)));
        out.extend(n.push(key(Key::Tab, CLEAN)));
        out.extend(n.push(text("five", CLEAN)));
        out.extend(n.push(click(CLEAN, None)));
        out.extend(n.finish());

        let click_title = "Left-click \"OK\" edit in app";
        assert_eq!(
            titles(&out),
            [
                click_title,
                "Type \"one\" into \"Box\"",
                "Save",
                "Type \"two\" into \"Box\"",
                "a note",
                "Type \"three\" into \"Box\"",
                "Do this by hand",
                "Type \"four\" into \"Box\"",
                "Press Tab",
                "Type \"five\" into \"Box\"",
                click_title,
            ]
        );
        let ids: Vec<&str> = steps(&out).iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"]);
        assert_eq!(steps(&out)[4].action, Action::Comment);
        assert_eq!(steps(&out)[6].action, Action::Manual);
    }

    #[test]
    fn run_time_is_its_first_key() {
        let mut n = normalizer(true);
        n.push(Event::Key {
            t_ms: 100,
            key: Key::Text("a".to_string()),
            mods: Mods::default(),
            target: field("Box"),
            signals: CLEAN,
            screenshot: None,
        });
        n.push(Event::Key {
            t_ms: 200,
            key: Key::Text("b".to_string()),
            mods: Mods::default(),
            target: field("Box"),
            signals: CLEAN,
            screenshot: None,
        });
        assert_eq!(one(&n.finish()).captured_unix_ms, 100);
    }
}

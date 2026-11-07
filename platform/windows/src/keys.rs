//! Virtual-key codes to `core`'s [`Key`] and [`Mods`].
//!
//! Modifier state is tracked from the hook's own key-down and key-up events, not
//! asked of Windows, because `GetKeyState` describes this thread's queue rather
//! than the user's keyboard. Text comes from `ToUnicodeEx` with the foreground
//! window's keyboard layout and flag 0x4, which leaves the system's dead-key state
//! alone, so recording never changes what the user types.
//!
//! AltGr is told apart from Right Alt by the fake left Ctrl (scan code 0x21D)
//! that Windows sends with AltGr on layouts that have it.
//!
//! A key-up can be lost (Win+L, Ctrl+Alt+Del, a hook reinstall), which would
//! leave a modifier stuck and turn typing into shortcuts. So before each key,
//! modifiers we think are held are checked with `GetAsyncKeyState` and dropped
//! if they are up. That check only ever removes a modifier, never adds one.
//!
//! Known limits: a dead key followed by a letter is recorded as the plain letter,
//! and Caps Lock state is read once at start and then tracked.

use babelsteps_core::normalize::{Key, Mods};
use std::collections::HashSet;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Leave the keyboard state untouched (Windows 10 1607 and later).
const TOUNICODE_NO_STATE_CHANGE: u32 = 0x4;
/// The scan code of the left Ctrl that Windows adds to AltGr.
const FAKE_CTRL_SCAN: u32 = 0x21D;

#[derive(Default)]
pub struct KeyTracker {
    down: HashSet<u32>,
    caps: bool,
    fake_ctrl: bool,
    high_surrogate: Option<u16>,
    /// Check held modifiers against the real keyboard. Off in unit tests.
    verify: bool,
}

impl KeyTracker {
    pub fn new() -> Self {
        let caps = unsafe { GetKeyState(i32::from(VK_CAPITAL.0)) } & 1 != 0;
        KeyTracker { caps, verify: true, ..KeyTracker::default() }
    }

    fn held(&self, vk: VIRTUAL_KEY) -> bool {
        self.down.contains(&u32::from(vk.0))
    }

    fn altgr(&self) -> bool {
        self.held(VK_RMENU) && self.fake_ctrl
    }

    pub fn mods(&self) -> Mods {
        let altgr = self.altgr();
        Mods {
            ctrl: self.held(VK_LCONTROL) || self.held(VK_RCONTROL),
            // Right Alt is Alt on layouts without AltGr.
            alt: self.held(VK_LMENU) || (self.held(VK_RMENU) && !altgr),
            shift: self.held(VK_LSHIFT) || self.held(VK_RSHIFT),
            meta: self.held(VK_LWIN) || self.held(VK_RWIN),
        }
    }

    pub fn key_up(&mut self, vk: u32, scan: u32) {
        if vk == u32::from(VK_LCONTROL.0) && scan == FAKE_CTRL_SCAN {
            self.fake_ctrl = false;
            return;
        }
        self.down.remove(&vk);
    }

    /// Drop modifiers whose key-up we missed.
    fn drop_stuck_modifiers(&mut self) {
        if !self.verify {
            return;
        }
        let up = |vk: u32| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 == 0;
        self.down.retain(|&vk| !(is_modifier(VIRTUAL_KEY(vk as u16)) && up(vk)));
        if self.fake_ctrl && up(u32::from(VK_LCONTROL.0)) {
            self.fake_ctrl = false;
        }
    }

    /// A key went down. Returns the key to record, or `None` for modifiers,
    /// auto-repeats of non-text keys, dead keys and keys with no meaning.
    pub fn key_down(&mut self, vk: u32, scan: u32, extended: bool) -> Option<(Key, Mods)> {
        if vk == u32::from(VK_LCONTROL.0) && scan == FAKE_CTRL_SCAN {
            self.fake_ctrl = true;
            return None;
        }
        if vk == u32::from(VK_PACKET.0) {
            // A character sent as Unicode (on-screen keyboards, some input methods).
            return self.packet(scan as u16).map(|t| (Key::Text(t), Mods::default()));
        }
        self.drop_stuck_modifiers();
        let repeat = !self.down.insert(vk);
        let v = VIRTUAL_KEY(vk as u16);
        if is_modifier(v) {
            return None;
        }
        if v == VK_CAPITAL {
            if !repeat {
                self.caps = !self.caps;
            }
            return None;
        }
        let mods = self.mods();
        let altgr = self.altgr();

        let fixed = match v {
            VK_RETURN => Some(Key::Enter),
            VK_TAB => Some(Key::Tab),
            VK_BACK => Some(Key::Backspace),
            VK_ESCAPE => Some(Key::Escape),
            _ => named(v, extended).map(|n| Key::Named(n.to_string())),
        };
        if let Some(key) = fixed {
            // Holding Backspace really deletes many characters; holding an arrow
            // key is one action.
            if repeat && !matches!(key, Key::Backspace) {
                return None;
            }
            return Some((key, mods));
        }

        if altgr {
            // AltGr+key types a character on many layouts.
            if let Some(text) = self.to_text(vk, scan, true) {
                return Some((Key::Text(text), Mods { shift: mods.shift, ..Mods::default() }));
            }
        }
        if mods.ctrl || mods.alt || mods.meta {
            if repeat {
                return None;
            }
            let base = base_char(vk)?;
            return Some((Key::Char(base), mods));
        }
        let text = self.to_text(vk, scan, false)?;
        Some((Key::Text(text), mods))
    }

    /// A UTF-16 unit from a `VK_PACKET` key, pairing surrogates.
    fn packet(&mut self, unit: u16) -> Option<String> {
        match unit {
            0xD800..=0xDBFF => {
                self.high_surrogate = Some(unit);
                None
            }
            0xDC00..=0xDFFF => {
                let high = self.high_surrogate.take()?;
                String::from_utf16(&[high, unit]).ok()
            }
            _ => {
                self.high_surrogate = None;
                let text = String::from_utf16(&[unit]).ok()?;
                (!text.chars().any(char::is_control)).then_some(text)
            }
        }
    }

    fn to_text(&self, vk: u32, scan: u32, altgr: bool) -> Option<String> {
        let mut state = [0u8; 256];
        let shift = self.held(VK_LSHIFT) || self.held(VK_RSHIFT);
        if shift {
            state[usize::from(VK_SHIFT.0)] = 0x80;
        }
        if self.caps {
            state[usize::from(VK_CAPITAL.0)] = 0x01;
        }
        if altgr {
            state[usize::from(VK_CONTROL.0)] = 0x80;
            state[usize::from(VK_MENU.0)] = 0x80;
            state[usize::from(VK_RMENU.0)] = 0x80;
            state[usize::from(VK_LCONTROL.0)] = 0x80;
        }
        let mut buf = [0u16; 8];
        let n = unsafe { ToUnicodeEx(vk, scan, &state, &mut buf, TOUNICODE_NO_STATE_CHANGE, Some(foreground_layout())) };
        if n <= 0 {
            return None; // 0: no character; negative: a dead key.
        }
        let text = String::from_utf16_lossy(&buf[..n as usize]);
        (!text.is_empty() && !text.chars().any(char::is_control)).then_some(text)
    }
}

fn foreground_layout() -> HKL {
    unsafe {
        let tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(tid)
    }
}

fn is_modifier(v: VIRTUAL_KEY) -> bool {
    matches!(
        v,
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT | VK_CONTROL | VK_LCONTROL | VK_RCONTROL | VK_MENU | VK_LMENU | VK_RMENU | VK_LWIN | VK_RWIN
    )
}

/// The unshifted character printed on a key, for naming shortcuts like Ctrl+S.
fn base_char(vk: u32) -> Option<char> {
    let mapped = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0x7FFF_FFFF;
    char::from_u32(mapped).filter(|c| !c.is_control())
}

/// Keys that have a name rather than a character.
fn named(v: VIRTUAL_KEY, extended: bool) -> Option<&'static str> {
    Some(match v {
        VK_F1 => "F1",
        VK_F2 => "F2",
        VK_F3 => "F3",
        VK_F4 => "F4",
        VK_F5 => "F5",
        VK_F6 => "F6",
        VK_F7 => "F7",
        VK_F8 => "F8",
        VK_F9 => "F9",
        VK_F10 => "F10",
        VK_F11 => "F11",
        VK_F12 => "F12",
        VK_F13 => "F13",
        VK_F14 => "F14",
        VK_F15 => "F15",
        VK_F16 => "F16",
        VK_F17 => "F17",
        VK_F18 => "F18",
        VK_F19 => "F19",
        VK_F20 => "F20",
        VK_F21 => "F21",
        VK_F22 => "F22",
        VK_F23 => "F23",
        VK_F24 => "F24",
        VK_LEFT => "Left",
        VK_RIGHT => "Right",
        VK_UP => "Up",
        VK_DOWN => "Down",
        VK_HOME => "Home",
        VK_END => "End",
        VK_PRIOR => "Page Up",
        VK_NEXT => "Page Down",
        VK_INSERT => "Insert",
        VK_DELETE => "Delete",
        VK_SNAPSHOT => "Print Screen",
        VK_PAUSE => "Pause",
        VK_APPS => "Menu",
        VK_NUMLOCK => "Num Lock",
        VK_SCROLL => "Scroll Lock",
        VK_CLEAR if !extended => return None,
        VK_VOLUME_MUTE => "Mute",
        VK_VOLUME_DOWN => "Volume Down",
        VK_VOLUME_UP => "Volume Up",
        VK_MEDIA_PLAY_PAUSE => "Play/Pause",
        VK_MEDIA_NEXT_TRACK => "Next Track",
        VK_MEDIA_PREV_TRACK => "Previous Track",
        VK_MEDIA_STOP => "Stop",
        VK_BROWSER_BACK => "Browser Back",
        VK_BROWSER_FORWARD => "Browser Forward",
        VK_BROWSER_REFRESH => "Browser Refresh",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> KeyTracker {
        KeyTracker::default()
    }

    #[test]
    fn modifiers_produce_nothing_and_are_tracked() {
        let mut k = tracker();
        assert!(k.key_down(u32::from(VK_LCONTROL.0), 0x1D, false).is_none());
        assert!(k.mods().ctrl);
        k.key_up(u32::from(VK_LCONTROL.0), 0x1D);
        assert!(!k.mods().ctrl);
    }

    #[test]
    fn ctrl_s_is_a_char_with_ctrl() {
        let mut k = tracker();
        k.key_down(u32::from(VK_LCONTROL.0), 0x1D, false);
        let (key, mods) = k.key_down(u32::from(b'S'), 0x1F, false).unwrap();
        assert!(matches!(key, Key::Char(c) if c.eq_ignore_ascii_case(&'s')));
        assert!(mods.ctrl && !mods.alt);
    }

    #[test]
    fn altgr_fake_ctrl_is_not_reported_as_ctrl() {
        let mut k = tracker();
        k.key_down(u32::from(VK_LCONTROL.0), 0x21D, false);
        k.key_down(u32::from(VK_RMENU.0), 0x38, true);
        let m = k.mods();
        assert!(!m.ctrl && !m.alt, "AltGr must not look like Ctrl+Alt: {m:?}");
    }

    #[test]
    fn right_alt_without_altgr_is_alt() {
        // US layout: Right Alt sends no fake Ctrl.
        let mut k = tracker();
        k.key_down(u32::from(VK_RMENU.0), 0x38, true);
        let m = k.mods();
        assert!(m.alt && !m.ctrl, "{m:?}");
    }

    #[test]
    fn real_left_ctrl_with_altgr_keeps_ctrl() {
        let mut k = tracker();
        k.key_down(u32::from(VK_LCONTROL.0), 0x1D, false);
        k.key_down(u32::from(VK_LCONTROL.0), 0x21D, false);
        k.key_down(u32::from(VK_RMENU.0), 0x38, true);
        let m = k.mods();
        assert!(m.ctrl && !m.alt, "{m:?}");
    }

    #[test]
    fn unicode_packets_become_text_and_pair_surrogates() {
        let mut k = tracker();
        let vk = u32::from(VK_PACKET.0);
        assert!(matches!(k.key_down(vk, u32::from('é' as u16), false), Some((Key::Text(t), _)) if t == "é"));
        assert!(k.key_down(vk, 0xD83D, false).is_none());
        assert!(matches!(k.key_down(vk, 0xDE00, false), Some((Key::Text(t), _)) if t == "\u{1F600}"));
    }

    #[test]
    fn held_arrow_is_one_event_but_held_backspace_repeats() {
        let mut k = tracker();
        assert!(k.key_down(u32::from(VK_DOWN.0), 0x50, true).is_some());
        assert!(k.key_down(u32::from(VK_DOWN.0), 0x50, true).is_none());
        assert!(k.key_down(u32::from(VK_BACK.0), 0x0E, false).is_some());
        assert!(k.key_down(u32::from(VK_BACK.0), 0x0E, false).is_some());
    }

    #[test]
    fn named_and_fixed_keys() {
        let mut k = tracker();
        assert!(matches!(k.key_down(u32::from(VK_RETURN.0), 0x1C, false), Some((Key::Enter, _))));
        assert!(matches!(k.key_down(u32::from(VK_F4.0), 0x3E, false), Some((Key::Named(n), _)) if n == "F4"));
    }
}

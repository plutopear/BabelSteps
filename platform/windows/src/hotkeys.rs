//! Global hotkeys (C3) with `RegisterHotKey`, on a thread with its own message
//! loop. They work while the app window is hidden or another app has focus.
//!
//! Registration fails when another app already owns the combination; that is
//! reported per hotkey, so the UI can warn about the conflict.

use anyhow::{anyhow, Result};
use crossbeam_channel::{bounded, unbounded, Receiver};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, PostThreadMessageW, MSG, WM_HOTKEY, WM_QUIT};

/// One hotkey: modifiers and a virtual-key code (for letters, the uppercase ASCII code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

impl Hotkey {
    /// Ctrl+Alt+Shift+`letter`, the defaults from Q14.
    pub fn ctrl_alt_shift(letter: char) -> Hotkey {
        Hotkey { ctrl: true, alt: true, shift: true, win: false, vk: letter.to_ascii_uppercase() as u32 }
    }

    /// From the settings text. Letters and digits use their ASCII code, F1 to F12 use 0x70 to 0x7B.
    pub fn from_spec(spec: &babelsteps_core::settings::HotkeySpec) -> Hotkey {
        let vk = match spec.key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
            Some(n) if spec.key.len() >= 2 => 0x70 + n - 1,
            _ => spec.key.chars().next().map(|c| c.to_ascii_uppercase() as u32).unwrap_or(0),
        };
        Hotkey { ctrl: spec.ctrl, alt: spec.alt, shift: spec.shift, win: spec.win, vk }
    }

    fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        for (on, flag) in [(self.ctrl, MOD_CONTROL), (self.alt, MOD_ALT), (self.shift, MOD_SHIFT), (self.win, MOD_WIN)] {
            if on {
                m |= flag;
            }
        }
        m
    }
}

pub struct Hotkeys {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

/// Register each hotkey. Returns the handle, a receiver of pressed hotkey
/// indexes (positions in `keys`), and whether each one registered.
pub fn register(keys: &[Hotkey]) -> Result<(Hotkeys, Receiver<usize>, Vec<bool>)> {
    let keys = keys.to_vec();
    let (pressed_tx, pressed_rx) = unbounded::<usize>();
    let (ready_tx, ready_rx) = bounded::<(u32, Vec<bool>)>(1);
    let join = thread::Builder::new().name("hotkeys".into()).spawn(move || unsafe {
        let ok: Vec<bool> =
            keys.iter().enumerate().map(|(i, k)| RegisterHotKey(None, i as i32 + 1, k.modifiers(), k.vk).is_ok()).collect();
        let _ = ready_tx.send((GetCurrentThreadId(), ok.clone()));
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0;
                if (1..=keys.len()).contains(&id) {
                    let _ = pressed_tx.send(id - 1);
                }
            }
        }
        for (i, registered) in ok.iter().enumerate() {
            if *registered {
                let _ = UnregisterHotKey(None, i as i32 + 1);
            }
        }
    })?;
    let (thread_id, ok) = ready_rx.recv().map_err(|_| anyhow!("the hotkey thread exited early"))?;
    Ok((Hotkeys { thread_id, join: Some(join) }, pressed_rx, ok))
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        if let Some(j) = self.join.take() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use babelsteps_core::settings::HotkeySpec;

    #[test]
    fn settings_text_becomes_virtual_keys() {
        let k = Hotkey::from_spec(&HotkeySpec::parse("Ctrl+Alt+Shift+r").unwrap());
        assert_eq!(k, Hotkey::ctrl_alt_shift('R'));
        assert_eq!(Hotkey::from_spec(&HotkeySpec::parse("Ctrl+Alt+F1").unwrap()).vk, 0x70);
        assert_eq!(Hotkey::from_spec(&HotkeySpec::parse("Ctrl+Alt+F12").unwrap()).vk, 0x7B);
        assert_eq!(Hotkey::from_spec(&HotkeySpec::parse("Ctrl+Win+5").unwrap()).vk, '5' as u32);
    }
}

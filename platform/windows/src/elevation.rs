//! Administrator (elevated) windows (PSR-8, Q31).
//!
//! A recorder running as a normal user is not given input sent to an elevated
//! window (UIPI), so clicks there make no step at all (measured 2026-11-08,
//! `research/m1-results.md`). Nothing from such a window can be saved, but the
//! person should be told, so the panel can say why a gap appears. The
//! token of the window's process says whether it is elevated; a refusal to open
//! it counts too.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// This process runs elevated (as administrator).
pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len)
            .is_ok();
        let _ = CloseHandle(token);
        ok && e.TokenIsElevated != 0
    }
}

/// The window in front belongs to an elevated process, or one whose token this
/// process may not open, while this process is not elevated: almost always an
/// administrator window.
/// False when this process is elevated (it sees everything), when nothing is in
/// front, or when the process can be inspected. A guess, not a proof: a few
/// protected system processes refuse too.
pub fn foreground_is_elevated_for_us() -> bool {
    unsafe {
        if is_elevated() {
            return false;
        }
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return false;
        }
        // Querying limited information works for elevated processes too. A normal
        // process can usually read the token of an elevated one, so ask the token
        // itself (measured 2026-11-08: refusing to open it was NOT the signal, the
        // first version of this check never fired on an administrator window).
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut token = HANDLE::default();
        let verdict = if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            // Refused outright: out of reach for us, so treat it as out of reach.
            true
        } else {
            let mut e = TOKEN_ELEVATION::default();
            let mut len = 0u32;
            let ok = GetTokenInformation(token, TokenElevation, Some(&mut e as *mut _ as *mut _), std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len)
                .is_ok();
            let _ = CloseHandle(token);
            ok && e.TokenIsElevated != 0
        };
        let _ = CloseHandle(process);
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_elevated_is_stable_between_calls() {
        // A process does not gain or lose administrator rights while it runs.
        assert_eq!(is_elevated(), is_elevated());
    }

    #[test]
    fn an_elevated_recorder_never_claims_the_window_in_front_is_out_of_reach() {
        // True on every machine: elevated means "sees everything", so the
        // warning ("clicks in it are not recorded") must never show. CI runners
        // are elevated, so there this checks the real branch; on a normal
        // desktop it only checks that the call is safe with any window in front.
        if is_elevated() {
            assert!(!foreground_is_elevated_for_us());
        } else {
            let _ = foreground_is_elevated_for_us();
        }
    }

    #[test]
    fn the_warning_flag_starts_off() {
        // A fresh status must not warn before the first look at the front window.
        assert!(!crate::pipeline::RecorderStatus::default().elevated_in_front);
    }
}

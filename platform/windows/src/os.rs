//! Operating system version, for the optional technical details in exports
//! (PSR-6). Version numbers only: never the user or machine name (Q25).

use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;

/// Such as "Windows 11 (build 22631)". Uses `RtlGetVersion`, because
/// `GetVersionEx` reports an older version to programs without a matching
/// compatibility manifest. Windows 11 still reports major version 10, so
/// build 22000 and later is named Windows 11.
pub fn os_description() -> String {
    let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // SAFETY: `info` is a valid OSVERSIONINFOW with its size field set.
    let status = unsafe { RtlGetVersion(&mut info) };
    if status.is_err() {
        return "Windows".to_string();
    }
    let name = match (info.dwMajorVersion, info.dwBuildNumber) {
        (10, b) if b >= 22_000 => "Windows 11".to_string(),
        (10, _) => "Windows 10".to_string(),
        (major, _) => format!("Windows {major}.{}", info.dwMinorVersion),
    };
    format!("{name} (build {})", info.dwBuildNumber)
}

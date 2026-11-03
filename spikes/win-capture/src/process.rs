//! Own-process CPU and memory numbers, for the resource targets.

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

fn ft_seconds(ft: FILETIME) -> f64 {
    (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) as f64 / 10_000_000.0
}

/// Total kernel and user CPU time used by this process so far.
pub fn cpu_seconds() -> f64 {
    unsafe {
        let (mut c, mut e, mut k, mut u) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u).is_ok() {
            ft_seconds(k) + ft_seconds(u)
        } else {
            0.0
        }
    }
}

/// Current and peak working set in MB.
pub fn memory_mb() -> (f64, f64) {
    unsafe {
        let mut c = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb).is_ok() {
            (c.WorkingSetSize as f64 / 1_048_576.0, c.PeakWorkingSetSize as f64 / 1_048_576.0)
        } else {
            (0.0, 0.0)
        }
    }
}

//! High-resolution timing helpers built on QueryPerformanceCounter.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

static FREQ: OnceLock<i64> = OnceLock::new();

pub fn freq() -> i64 {
    *FREQ.get_or_init(|| {
        let mut f = 0i64;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut f);
        }
        f.max(1)
    })
}

#[inline]
pub fn now() -> i64 {
    let mut c = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
    }
    c
}

pub fn to_us(ticks: i64) -> f64 {
    ticks as f64 * 1_000_000.0 / freq() as f64
}

pub fn to_ms(ticks: i64) -> f64 {
    to_us(ticks) / 1000.0
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

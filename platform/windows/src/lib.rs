//! BabelSteps Windows capture backend.
//!
//! Turns real clicks and key presses into steps, following the rules M0 proved
//! (`research/m0-spike-results.md`, `docs/03-architecture.md` §3):
//!
//! - Screenshots show the state before the click: every monitor keeps a short
//!   frame history, and a click reads back the newest frame captured before it.
//! - Hook callbacks only timestamp and queue; all input on the machine waits on them.
//! - Analysis (UI Automation, password scans) never delays capture: it runs on its
//!   own thread, and UI Automation calls run on a replaceable worker with time limits.
//! - The password check fails closed (`core::password`). If it fails or times out,
//!   typed text is dropped and the screenshot is withheld.
//! - Password fields are blacked out before anything is written to disk.
//!
//! The only public entry point is [`Recorder`].
#![cfg(windows)]

mod capture;
mod clock;
pub mod elevation;
mod frame;
mod hooks;
pub mod hotkeys;
pub mod indicator;
mod keys;
mod monitors;
pub mod os;
mod pipeline;
mod scan;
pub mod scope;
mod session;
mod uia;

#[cfg(feature = "selftest")]
pub mod testwin;

pub use pipeline::{Faults, Recorder, RecorderConfig, RecorderReport, RecorderStatus};
pub use scope::Scope;

/// Test-only access to internals the self-test checks against real files.
#[cfg(feature = "selftest")]
pub mod testing {
    pub use crate::clock::{now, to_ms};
    pub use crate::frame::Frame;
    pub use crate::hooks::stats as hook_stats;
    pub use crate::monitors::{enumerate as monitors, make_thread_dpi_aware, per_monitor_dpi_aware, Monitor};
    pub use crate::scope::window_bounds;
    pub use crate::session::SessionEvent;
}

//! The operating-system-specific parts: the transparent overlay, sound, the
//! menu, and lock/display detection. Everything else (where the possum runs,
//! when it screams, when it keeps quiet) is shared; see brain.rs.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::run;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{prepare_console, run};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::run;

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
compile_error!("Possum runs on macOS, Windows and Linux.");

/// Nothing to set up before printing on macOS and Linux (see the Windows
/// version).
#[cfg(not(windows))]
pub fn prepare_console() {}

/// A different seed every run, so the screams vary.
pub fn seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0xC0FFEE)
}

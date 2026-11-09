//! The settings window. Each platform draws it natively from `app::view()`
//! and reports user actions back through the functions in `app`. Every call
//! here runs on the main thread.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{close, open, refresh};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::{close, open, refresh};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{close, open, refresh};

pub mod hud;
pub mod panel;

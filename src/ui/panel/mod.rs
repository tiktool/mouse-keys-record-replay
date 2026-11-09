//! The recordings panel: a card at the top center of the display under the
//! cursor that lists saved recordings. It takes focus and closes when it
//! loses it. Calls run on the main thread, and user actions go back through
//! the recording functions in `app`.

/// One saved recording as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub starred: bool,
    pub active: bool,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{close, is_open, show};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::{close, is_open, show};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::{close, is_open, show};

//! The status card shown at the top center of every display while recording,
//! right after a recording is saved, and while a recording loops. It never
//! takes focus and lets every click pass through. Calls run on the main thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    Recording,
    Saved,
    Looping,
}

/// One card. `line` is the small text under the title, split into runs; a
/// run marked `true` is a shortcut and is drawn as a key chip.
#[derive(Clone, Debug, PartialEq)]
pub struct Hud {
    pub kind: HudKind,
    pub title: String,
    pub line: Vec<(String, bool)>,
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::show;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::show;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::show;

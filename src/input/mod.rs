//! Platform input layer: a global listener that can be started and stopped,
//! plus injection that tags every synthetic event so the listener can tell
//! our playback apart from the real user.
//!
//! Every backend exposes the same free functions:
//!   start() -> Result<Hook, String>   listener; calls crate::engine::on_input(ev, injected)
//!   inject(&Ev, buttons_down)         replay one event, tagged as ours
//!   cursor_pos() -> (i32, i32)
//!   any_key_down() -> bool            physical keyboard state
//!   begin_playback() / end_playback() timer resolution etc.
//!   permission_ok(prompt) -> bool     OS input permissions
//!   tray_light_background() -> bool   pick a dark tray glyph (not macOS, which uses a template icon)

use serde::{Deserialize, Serialize};

/// Codes are platform-native: a recording is only replayed on the machine
/// family it was recorded on, so no cross-platform key mapping is needed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k")]
pub enum Ev {
    /// Windows: vk + scan + LLKHF flags. macOS: keycode + CGEventFlags. Linux: X keycode.
    Key { code: u32, scan: u32, flags: u64, down: bool },
    Move { x: i32, y: i32 },
    /// b: 1 left, 2 right, 3 middle, 4/5 side buttons (Windows, macOS).
    /// Linux uses raw X button numbers (4-7 are wheel steps). n = click count.
    Button { b: u8, down: bool, x: i32, y: i32, n: u8 },
    Wheel { dx: i32, dy: i32 },
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use self::linux::*;

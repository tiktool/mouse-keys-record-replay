use super::Ev;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventType, CGMouseButton, CallbackResult, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

const MAGIC: i64 = 0x5245_5052; // "REPR"

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventSourceKeyState(state: CGEventSourceStateID, key: u16) -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
}

pub struct Hook {
    run_loop: CFRunLoop, // CFRunLoop is the crate's Send + Sync wrapper around CFRunLoopRef
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.run_loop.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Generic flag bit plus the device-dependent (left/right) bit for each modifier keycode.
fn modifier_bits(code: u32) -> Option<(u64, u64)> {
    Some(match code {
        0x37 => (0x0010_0000, 0x0008),
        0x36 => (0x0010_0000, 0x0010),
        0x38 => (0x0002_0000, 0x0002),
        0x3C => (0x0002_0000, 0x0004),
        0x3B => (0x0004_0000, 0x0001),
        0x3E => (0x0004_0000, 0x2000),
        0x3A => (0x0008_0000, 0x0020),
        0x3D => (0x0008_0000, 0x0040),
        0x39 => (0x0001_0000, 0),
        0x3F => (0x0080_0000, 0),
        _ => return None,
    })
}

const DEVICE_MODIFIER_MASK: u64 = 0x207F;

fn flags_changed_down(code: u32, flags: u64) -> bool {
    match modifier_bits(code) {
        // With the other side of the same modifier still held, only the device bit tells them apart.
        Some((_, dev)) if dev != 0 && flags & DEVICE_MODIFIER_MASK != 0 => flags & dev != 0,
        Some((generic, _)) => flags & generic != 0,
        None => flags & 0x00FF_0000 != 0,
    }
}

fn convert(ty: CGEventType, e: &CGEvent) -> Option<Ev> {
    let p = e.location();
    let (x, y) = (p.x.round() as i32, p.y.round() as i32);
    let button = |b: u8, down| Ev::Button {
        b,
        down,
        x,
        y,
        n: e.get_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE).clamp(0, 255) as u8,
    };
    let other = || (e.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER) + 1).clamp(3, 5) as u8;
    let code = e.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u32;
    let flags = e.get_flags().bits();
    Some(match ty {
        CGEventType::KeyDown | CGEventType::KeyUp => {
            Ev::Key { code, scan: 0, flags, down: matches!(ty, CGEventType::KeyDown) }
        }
        CGEventType::FlagsChanged => Ev::Key { code, scan: 0, flags, down: flags_changed_down(code, flags) },
        CGEventType::MouseMoved
        | CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => Ev::Move { x, y },
        CGEventType::LeftMouseDown => button(1, true),
        CGEventType::LeftMouseUp => button(1, false),
        CGEventType::RightMouseDown => button(2, true),
        CGEventType::RightMouseUp => button(2, false),
        CGEventType::OtherMouseDown => button(other(), true),
        CGEventType::OtherMouseUp => button(other(), false),
        CGEventType::ScrollWheel => Ev::Wheel {
            dx: e.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2) as i32,
            dy: e.get_integer_value_field(EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1) as i32,
        },
        _ => return None,
    })
}

pub fn start() -> Result<Hook, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let (tx, rx) = mpsc::channel::<Result<CFRunLoop, String>>();
    let thread = std::thread::spawn(move || {
        let port = Arc::new(AtomicPtr::<c_void>::new(std::ptr::null_mut()));
        let port_cb = port.clone();
        use CGEventType::*;
        let mask = vec![
            KeyDown, KeyUp, FlagsChanged, MouseMoved, LeftMouseDown, LeftMouseUp, LeftMouseDragged,
            RightMouseDown, RightMouseUp, RightMouseDragged, OtherMouseDown, OtherMouseUp,
            OtherMouseDragged, ScrollWheel,
        ];
        // Session level: a listen-only tap there needs only Input Monitoring, and events
        // posted at HID still pass through it with their user data intact.
        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            move |_, ty, e| {
                if matches!(ty, TapDisabledByTimeout | TapDisabledByUserInput) {
                    let p = port_cb.load(Ordering::SeqCst);
                    if !p.is_null() {
                        unsafe { CGEventTapEnable(p as CFMachPortRef, true) };
                    }
                } else if let Some(ev) = convert(ty, e) {
                    let injected = e.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == MAGIC;
                    crate::engine::on_input(ev, injected);
                }
                CallbackResult::Keep
            },
        );
        let tap = match tap {
            Ok(t) => t,
            Err(()) => {
                let _ = tx.send(Err("event tap refused: grant Input Monitoring and Accessibility".into()));
                return;
            }
        };
        port.store(tap.mach_port().as_concrete_TypeRef() as *mut c_void, Ordering::SeqCst);
        let Ok(src) = tap.mach_port().create_runloop_source(0) else {
            let _ = tx.send(Err("event tap run loop source failed".into()));
            return;
        };
        let rl = CFRunLoop::get_current();
        rl.add_source(&src, unsafe { kCFRunLoopCommonModes });
        tap.enable();
        let _ = tx.send(Ok(rl));
        // A CFRunLoopStop sent before the loop starts is lost, so re-check the flag on a short timeout.
        while !stop_t.load(Ordering::SeqCst) {
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, Duration::from_millis(500), false);
        }
        drop(tap);
    });
    match rx.recv() {
        Ok(Ok(run_loop)) => Ok(Hook { run_loop, stop, thread: Some(thread) }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => Err("event tap thread exited".into()),
    }
}

pub fn inject(ev: &Ev, buttons_down: u32) {
    // Private state: posted flags come only from what we set, never merged with the
    // physical keyboard, so a user holding Shift does not leak into the replay and vice versa.
    let Ok(src) = CGEventSource::new(CGEventSourceStateID::Private) else { return };
    let mouse = |ty, x: i32, y: i32, btn| {
        CGEvent::new_mouse_event(src.clone(), ty, CGPoint::new(x as f64, y as f64), btn).ok()
    };
    let event = match *ev {
        Ev::Key { code, flags, down, .. } => CGEvent::new_keyboard_event(src.clone(), code as u16, down)
            .ok()
            .inspect(|e| {
                if modifier_bits(code).is_some() {
                    e.set_type(CGEventType::FlagsChanged);
                }
                e.set_flags(CGEventFlags::from_bits_retain(flags));
            }),
        Ev::Move { x, y } => {
            let (ty, btn, num) = if buttons_down & (1 << 1) != 0 {
                (CGEventType::LeftMouseDragged, CGMouseButton::Left, 0)
            } else if buttons_down & (1 << 2) != 0 {
                (CGEventType::RightMouseDragged, CGMouseButton::Right, 1)
            } else if let Some(b) = (3..=5).find(|b| buttons_down & (1 << b) != 0) {
                (CGEventType::OtherMouseDragged, CGMouseButton::Center, b as i64 - 1)
            } else {
                (CGEventType::MouseMoved, CGMouseButton::Left, 0)
            };
            mouse(ty, x, y, btn).inspect(|e| {
                if num >= 2 {
                    e.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, num);
                }
            })
        }
        Ev::Button { b, down, x, y, n } => {
            let (ty, btn) = match (b, down) {
                (1, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
                (1, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
                (2, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
                (2, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
                (_, true) => (CGEventType::OtherMouseDown, CGMouseButton::Center),
                (_, false) => (CGEventType::OtherMouseUp, CGMouseButton::Center),
            };
            mouse(ty, x, y, btn).inspect(|e| {
                if b >= 3 {
                    e.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, b as i64 - 1);
                }
                e.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, n as i64);
            })
        }
        Ev::Wheel { dx, dy } => CGEvent::new_scroll_event(src.clone(), ScrollEventUnit::PIXEL, 2, dy, dx, 0).ok(),
    };
    if let Some(e) = event {
        e.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, MAGIC);
        e.post(CGEventTapLocation::HID);
    }
}

pub fn cursor_pos() -> (i32, i32) {
    CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .and_then(CGEvent::new)
        .map(|e| {
            let p = e.location();
            (p.x.round() as i32, p.y.round() as i32)
        })
        .unwrap_or((0, 0))
}

/// Keycodes 0..128 include the modifier keys, so this covers held modifiers too.
pub fn any_key_down() -> bool {
    (0..128u16).any(|k| unsafe { CGEventSourceKeyState(CGEventSourceStateID::HIDSystemState, k) })
}

pub fn begin_playback() {}

pub fn end_playback() {}

pub fn permission_ok(prompt: bool) -> bool {
    let ax = unsafe {
        if prompt {
            let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
            let opts = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
            AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef())
        } else {
            AXIsProcessTrusted()
        }
    };
    let listen = unsafe { CGPreflightListenEventAccess() || (prompt && CGRequestListenEventAccess()) };
    ax && listen
}

/// The System Settings pane for the first permission still missing, without prompting.
pub fn missing_pane() -> Option<&'static str> {
    if !unsafe { AXIsProcessTrusted() } {
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    } else if !unsafe { CGPreflightListenEventAccess() } {
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
    } else {
        None
    }
}

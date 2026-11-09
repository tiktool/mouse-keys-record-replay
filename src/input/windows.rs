use super::Ev;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::thread::JoinHandle;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// Stamped into dwExtraInfo of everything we inject. The hook reads it back,
// which is how a real keypress is told apart from the same key being replayed.
const MAGIC: usize = 0x5245_5052;

pub struct Hook {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn start() -> Result<Hook, String> {
    let (tx, rx) = mpsc::channel();
    let join = std::thread::spawn(move || unsafe {
        let mut msg: MSG = zeroed();
        // Forces the thread message queue to exist before Drop can post WM_QUIT.
        PeekMessageW(&mut msg, null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
        let module = GetModuleHandleW(null());
        let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb_proc), module, 0);
        let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
        if kb.is_null() || ms.is_null() {
            if !kb.is_null() {
                UnhookWindowsHookEx(kb);
            }
            if !ms.is_null() {
                UnhookWindowsHookEx(ms);
            }
            let _ = tx.send(Err("Could not watch keyboard and mouse".to_string()));
            return;
        }
        let _ = tx.send(Ok(GetCurrentThreadId()));
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {}
        UnhookWindowsHookEx(kb);
        UnhookWindowsHookEx(ms);
    });
    let thread_id = rx.recv().map_err(|e| e.to_string())??;
    Ok(Hook { thread_id, join: Some(join) })
}

unsafe extern "system" fn kb_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(l as *const KBDLLHOOKSTRUCT);
        let down = matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        let ev = Ev::Key { code: k.vkCode, scan: k.scanCode, flags: k.flags as u64, down };
        crate::engine::on_input(ev, k.dwExtraInfo == MAGIC);
    }
    CallNextHookEx(null_mut(), code, w, l)
}

unsafe extern "system" fn mouse_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let m = &*(l as *const MSLLHOOKSTRUCT);
        let (x, y) = (m.pt.x, m.pt.y);
        let hi = (m.mouseData >> 16) as u16;
        let button = |b: u8, down: bool| Some(Ev::Button { b, down, x, y, n: 1 });
        let side = if hi == XBUTTON1 as u16 { 4 } else { 5 };
        let ev = match w as u32 {
            WM_MOUSEMOVE => Some(Ev::Move { x, y }),
            WM_LBUTTONDOWN => button(1, true),
            WM_LBUTTONUP => button(1, false),
            WM_RBUTTONDOWN => button(2, true),
            WM_RBUTTONUP => button(2, false),
            WM_MBUTTONDOWN => button(3, true),
            WM_MBUTTONUP => button(3, false),
            WM_XBUTTONDOWN => button(side, true),
            WM_XBUTTONUP => button(side, false),
            WM_MOUSEWHEEL => Some(Ev::Wheel { dx: 0, dy: hi as i16 as i32 }),
            WM_MOUSEHWHEEL => Some(Ev::Wheel { dx: hi as i16 as i32, dy: 0 }),
            _ => None,
        };
        if let Some(ev) = ev {
            crate::engine::on_input(ev, m.dwExtraInfo == MAGIC);
        }
    }
    CallNextHookEx(null_mut(), code, w, l)
}

fn send(input: INPUT) {
    unsafe { SendInput(1, &input, size_of::<INPUT>() as i32) };
}

fn mouse(x: i32, y: i32, flags: u32, data: i32) -> INPUT {
    // Absolute coordinates are normalised to 0..65535 across the whole virtual desktop.
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN) as i64,
            GetSystemMetrics(SM_YVIRTUALSCREEN) as i64,
            (GetSystemMetrics(SM_CXVIRTUALSCREEN) as i64 - 1).max(1),
            (GetSystemMetrics(SM_CYVIRTUALSCREEN) as i64 - 1).max(1),
        )
    };
    let mut i: INPUT = unsafe { zeroed() };
    i.r#type = INPUT_MOUSE;
    i.Anonymous.mi = MOUSEINPUT {
        dx: (((x as i64 - vx) * 65535 + vw / 2) / vw) as i32,
        dy: (((y as i64 - vy) * 65535 + vh / 2) / vh) as i32,
        mouseData: data as u32,
        dwFlags: flags | MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        time: 0,
        dwExtraInfo: MAGIC,
    };
    i
}

pub fn inject(ev: &Ev, _buttons_down: u32) {
    match *ev {
        Ev::Key { code, scan, flags, down } => {
            let mut i: INPUT = unsafe { zeroed() };
            i.r#type = INPUT_KEYBOARD;
            let mut f = if down { 0 } else { KEYEVENTF_KEYUP };
            if flags & LLKHF_EXTENDED as u64 != 0 {
                f |= KEYEVENTF_EXTENDEDKEY;
            }
            i.Anonymous.ki = KEYBDINPUT {
                wVk: code as u16,
                wScan: scan as u16,
                dwFlags: f,
                time: 0,
                dwExtraInfo: MAGIC,
            };
            send(i);
        }
        Ev::Move { x, y } => send(mouse(x, y, 0, 0)),
        Ev::Button { b, down, x, y, .. } => {
            let (flags, data) = match (b, down) {
                (1, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                (1, false) => (MOUSEEVENTF_LEFTUP, 0),
                (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
                (3, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                (3, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                (b, true) => (MOUSEEVENTF_XDOWN, if b == 4 { XBUTTON1 } else { XBUTTON2 } as i32),
                (b, false) => (MOUSEEVENTF_XUP, if b == 4 { XBUTTON1 } else { XBUTTON2 } as i32),
            };
            send(mouse(x, y, flags, data));
        }
        Ev::Wheel { dx, dy } => {
            let (x, y) = cursor_pos();
            if dy != 0 {
                send(mouse(x, y, MOUSEEVENTF_WHEEL, dy));
            }
            if dx != 0 {
                send(mouse(x, y, MOUSEEVENTF_HWHEEL, dx));
            }
        }
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn any_key_down() -> bool {
    // Starts at 0x08: 0x01..0x06 are mouse buttons.
    (0x08..=0xFE).any(|vk| unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000 != 0)
}

pub fn begin_playback() {
    unsafe { timeBeginPeriod(1) };
}

pub fn end_playback() {
    unsafe { timeEndPeriod(1) };
}

/// Keys of the speed shortcuts: Ctrl, Alt, Shift, Windows and F5 to F10.
pub fn speed_chord_key(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C | 0x74..=0x79)
}

pub fn permission_ok(_prompt: bool) -> bool {
    true
}

pub fn tray_light_background() -> bool {
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut data: u32 = 0;
    let mut size = size_of::<u32>() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    rc == 0 && data == 1
}

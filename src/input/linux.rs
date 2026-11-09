use super::Ev;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as _, Device, XIEventMask};
use x11rb::protocol::xproto::{self, ConnectionExt as _, CreateWindowAux, Window, WindowClass};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

pub struct Hook {
    conn: Arc<RustConnection>,
    wake: Window,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Drop for Hook {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // With an empty event mask the server delivers this to the window's
        // creator, which is the listener thread blocked in wait_for_event.
        let msg = xproto::ClientMessageEvent::new(32, self.wake, 0u32, [0u32; 5]);
        let _ = self.conn.send_event(false, self.wake, xproto::EventMask::NO_EVENT, msg);
        let _ = self.conn.flush();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

fn root(conn: &RustConnection, screen: usize) -> Window {
    conn.setup().roots[screen].root
}

pub fn start() -> Result<Hook, String> {
    let (conn, screen) = RustConnection::connect(None).map_err(|e| e.to_string())?;
    let conn = Arc::new(conn);
    let root = root(&conn, screen);
    let (wake, xtest) = setup(&conn, root).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let (c, s) = (conn.clone(), stop.clone());
    let join = std::thread::spawn(move || listen(&c, root, &xtest, &s));
    Ok(Hook { conn, wake, stop, join: Some(join) })
}

fn setup(conn: &RustConnection, root: Window) -> Res<(Window, Vec<u16>)> {
    let v = conn.xinput_xi_query_version(2, 2)?.reply()?;
    if v.major_version < 2 {
        return Err("XInput 2 is not available".into());
    }
    let xtest = conn
        .xinput_xi_query_device(Device::ALL)?
        .reply()?
        .infos
        .into_iter()
        .filter(|d| String::from_utf8_lossy(&d.name).contains("XTEST"))
        .map(|d| d.deviceid)
        .collect();
    let mask = XIEventMask::RAW_KEY_PRESS
        | XIEventMask::RAW_KEY_RELEASE
        | XIEventMask::RAW_BUTTON_PRESS
        | XIEventMask::RAW_BUTTON_RELEASE
        | XIEventMask::RAW_MOTION;
    let masks = [xinput::EventMask { deviceid: Device::ALL.into(), mask: vec![mask] }];
    conn.xinput_xi_select_events(root, &masks)?.check()?;
    // Never mapped: it only exists so Drop has a target for the wake-up message.
    let wake = conn.generate_id()?;
    let aux = CreateWindowAux::new();
    conn.create_window(0, wake, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &aux)?.check()?;
    Ok((wake, xtest))
}

fn listen(conn: &RustConnection, root: Window, xtest: &[u16], stop: &AtomicBool) {
    let pointer = || match conn.query_pointer(root).map(|c| c.reply()) {
        Ok(Ok(p)) => (p.root_x as i32, p.root_y as i32),
        _ => (0, 0),
    };
    while let Ok(event) = conn.wait_for_event() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let (dev, src) = match &event {
            Event::XinputRawKeyPress(e) | Event::XinputRawKeyRelease(e) => (e.deviceid, e.sourceid),
            Event::XinputRawButtonPress(e)
            | Event::XinputRawButtonRelease(e)
            | Event::XinputRawMotion(e) => (e.deviceid, e.sourceid),
            _ => continue,
        };
        // XIAllDevices delivers every raw event twice: from the slave, then again
        // from its master. Keep the slave copy only.
        if dev != src {
            continue;
        }
        let ev = match event {
            Event::XinputRawKeyPress(e) | Event::XinputRawKeyRelease(e) => {
                let down = e.event_type == xinput::RAW_KEY_PRESS_EVENT;
                Ev::Key { code: e.detail, scan: 0, flags: 0, down }
            }
            // Pointer-emulated wheel buttons are kept: smooth-scroll devices report
            // the wheel only as scroll valuators on RawMotion (which we turn into a
            // plain Move) plus these emulated 4-7 presses, so nothing counts twice.
            Event::XinputRawButtonPress(e) | Event::XinputRawButtonRelease(e) => {
                let down = e.event_type == xinput::RAW_BUTTON_PRESS_EVENT;
                let (x, y) = pointer();
                Ev::Button { b: e.detail as u8, down, x, y, n: 1 }
            }
            _ => {
                let (x, y) = pointer();
                Ev::Move { x, y }
            }
        };
        crate::engine::on_input(ev, xtest.contains(&src));
    }
}

struct Inject {
    conn: RustConnection,
    root: Window,
}

static INJECT: OnceLock<Mutex<Option<Inject>>> = OnceLock::new();

// Lazily connects and drops the connection after any error so the next call reconnects.
fn with<R>(f: impl FnOnce(&Inject) -> Res<R>) -> Option<R> {
    let mut slot = INJECT.get_or_init(|| Mutex::new(None)).lock().ok()?;
    if slot.is_none() {
        let (conn, screen) = RustConnection::connect(None).ok()?;
        let root = root(&conn, screen);
        *slot = Some(Inject { conn, root });
    }
    let r = f(slot.as_ref()?);
    if r.is_err() {
        *slot = None;
    }
    r.ok()
}

pub fn inject(ev: &Ev, _buttons_down: u32) {
    with(|i| {
        let fake = |t: u8, detail: u8, x: i32, y: i32| {
            i.conn.xtest_fake_input(t, detail, 0, i.root, x as i16, y as i16, 0).map(|_| ())
        };
        match *ev {
            Ev::Move { x, y } => fake(xproto::MOTION_NOTIFY_EVENT, 0, x, y)?,
            Ev::Button { b, down, x, y, .. } => {
                fake(xproto::MOTION_NOTIFY_EVENT, 0, x, y)?;
                let t =
                    if down { xproto::BUTTON_PRESS_EVENT } else { xproto::BUTTON_RELEASE_EVENT };
                fake(t, b, 0, 0)?;
            }
            Ev::Key { code, down, .. } => {
                let t = if down { xproto::KEY_PRESS_EVENT } else { xproto::KEY_RELEASE_EVENT };
                fake(t, code as u8, 0, 0)?;
            }
            Ev::Wheel { dx, dy } => {
                // X wheel steps are buttons: 4 up, 5 down, 6 left, 7 right.
                let steps = [(dy > 0, 4), (dy < 0, 5), (dx < 0, 6), (dx > 0, 7)];
                for (_, b) in steps.into_iter().filter(|s| s.0) {
                    fake(xproto::BUTTON_PRESS_EVENT, b, 0, 0)?;
                    fake(xproto::BUTTON_RELEASE_EVENT, b, 0, 0)?;
                }
            }
        }
        i.conn.flush()?;
        Ok(())
    });
}

pub fn cursor_pos() -> (i32, i32) {
    with(|i| {
        let p = i.conn.query_pointer(i.root)?.reply()?;
        Ok((p.root_x as i32, p.root_y as i32))
    })
    .unwrap_or((0, 0))
}

pub fn any_key_down() -> bool {
    with(|i| Ok(i.conn.query_keymap()?.reply()?.keys.iter().any(|&k| k != 0))).unwrap_or(false)
}

pub fn begin_playback() {}

pub fn end_playback() {}

/// Keys of the speed shortcuts on a standard keymap: Ctrl, Alt, Shift, Super
/// and F5 to F10 (X keycodes are evdev codes plus 8).
pub fn speed_chord_key(code: u32) -> bool {
    matches!(code, 37 | 105 | 50 | 62 | 64 | 108 | 133 | 134 | 71..=76)
}

/// X11 needs no grant; a Wayland session without an X display cannot be recorded.
pub fn permission_ok(_prompt: bool) -> bool {
    with(|_| Ok(())).is_some()
}

pub fn tray_light_background() -> bool {
    false
}

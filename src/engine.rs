use crate::input::{self, Ev};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::SeqCst};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub t: u64,
    pub ev: Ev,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Recording,
    Playing,
}

/// Why a recording stopped. Hotkey stops carry the chord length so the
/// shortcut's own key presses can be cut from the end of the recording.
#[derive(Clone, Copy, Debug)]
pub enum StopBy {
    Hotkey(usize),
    Menu,
}

struct Rec {
    start: Instant,
    steps: Vec<Step>,
}

static MODE: AtomicU8 = AtomicU8::new(0);
static ABORT: AtomicBool = AtomicBool::new(false);
static STARTING: AtomicBool = AtomicBool::new(false);
static LOOPING: AtomicBool = AtomicBool::new(false);
static RANDOM: AtomicBool = AtomicBool::new(false);
static REC: Mutex<Option<Rec>> = Mutex::new(None);
static MACRO: Mutex<Vec<Step>> = Mutex::new(Vec::new());
static HOOK: Mutex<Option<input::Hook>> = Mutex::new(None);
static USER_STOP_AT: Mutex<Option<Instant>> = Mutex::new(None);
static NOTIFY: OnceLock<Box<dyn Fn(Option<String>) + Send + Sync>> = OnceLock::new();
static SAVE: OnceLock<Box<dyn Fn(Vec<Step>) + Send + Sync>> = OnceLock::new();

const RELEASE_WAIT: Duration = Duration::from_secs(3);
/// A play shortcut pressed this soon after the user interrupted playback is the
/// same key press that interrupted it, so it must not start playback again.
const REPLAY_GUARD: Duration = Duration::from_millis(1200);
/// Shortest time one pass of a loop takes, so a recording with almost no
/// length cannot spin the processor.
const MIN_PASS: Duration = Duration::from_millis(50);
/// Random timing moves each press this far, in microseconds, earlier or later.
const JITTER_MIN: i64 = 100_000;
const JITTER_MAX: i64 = 500_000;

pub fn init(
    saved: Vec<Step>,
    notify: impl Fn(Option<String>) + Send + Sync + 'static,
    save: impl Fn(Vec<Step>) + Send + Sync + 'static,
) {
    *MACRO.lock().unwrap() = saved;
    let _ = NOTIFY.set(Box::new(notify));
    let _ = SAVE.set(Box::new(save));
}

fn notify(err: Option<String>) {
    if let Some(f) = NOTIFY.get() {
        f(err);
    }
}

pub fn mode() -> Mode {
    match MODE.load(SeqCst) {
        1 => Mode::Recording,
        2 => Mode::Playing,
        _ => Mode::Idle,
    }
}

fn set_mode(m: Mode) {
    MODE.store(
        match m {
            Mode::Idle => 0,
            Mode::Recording => 1,
            Mode::Playing => 2,
        },
        SeqCst,
    );
}

/// Called by the platform listener for every keyboard and mouse event.
pub fn on_input(ev: Ev, injected: bool) {
    if injected {
        return;
    }
    match mode() {
        Mode::Recording => {
            if let Some(r) = REC.lock().unwrap().as_mut() {
                let t = r.start.elapsed().as_micros() as u64;
                r.steps.push(Step { t, ev });
            }
        }
        Mode::Playing => ABORT.store(true, SeqCst),
        Mode::Idle => {}
    }
}

/// (actions, seconds) of the saved recording.
pub fn summary() -> (usize, f32) {
    let m = MACRO.lock().unwrap();
    let actions = m
        .iter()
        .filter(|s| match s.ev {
            Ev::Key { down, .. } | Ev::Button { down, .. } => down,
            Ev::Wheel { .. } => true,
            Ev::Move { .. } => false,
        })
        .count();
    let secs = m.last().map(|s| s.t as f32 / 1e6).unwrap_or(0.0);
    (actions, secs)
}

/// True while playback repeats until the user stops it.
pub fn looping() -> bool {
    mode() == Mode::Playing && LOOPING.load(SeqCst)
}

pub fn has_recording() -> bool {
    MACRO.lock().unwrap().len() > 1
}

/// Read at the start of every pass, so a change applies to the next loop.
pub fn set_random(on: bool) {
    RANDOM.store(on, SeqCst);
}

/// xorshift64*: plenty for timing noise, no extra dependency.
struct Rng(u64);

impl Rng {
    fn seeded() -> Rng {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Rng(nanos | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A signed offset between JITTER_MIN and JITTER_MAX in either direction.
    fn offset(&mut self) -> i64 {
        let r = self.next();
        let size = JITTER_MIN + ((r >> 1) % (JITTER_MAX - JITTER_MIN + 1) as u64) as i64;
        if r & 1 == 0 { size } else { -size }
    }
}

/// Moves every press, together with everything up to the next press, by its
/// own random offset. Positions, keys, hold lengths and order stay as
/// recorded; a block is never pulled in front of the one before it. Key
/// repeats, the second click of a double click and scroll ticks ride with
/// the press before them, so holds, double clicks and scrolls stay intact.
fn jitter(steps: &[Step], rng: &mut Rng) -> Vec<Step> {
    let mut out = Vec::with_capacity(steps.len());
    let mut held: Vec<u32> = Vec::new();
    let mut shift = 0i64;
    let mut last = 0i64;
    for (i, s) in steps.iter().enumerate() {
        let t = s.t as i64;
        let press = match s.ev {
            Ev::Key { code, down: true, .. } => {
                let fresh = !held.contains(&code);
                if fresh {
                    held.push(code);
                }
                fresh
            }
            Ev::Key { code, down: false, .. } => {
                held.retain(|&k| k != code);
                false
            }
            Ev::Button { down, n, .. } => down && n < 2,
            _ => false,
        };
        if i > 0 && press {
            shift = (t + rng.offset()).max(last + 1_000) - t;
        }
        let nt = (t + shift).max(last);
        last = nt;
        out.push(Step { t: nt as u64, ev: s.ev });
    }
    out
}

fn wait_release() {
    let t0 = Instant::now();
    while input::any_key_down() && t0.elapsed() < RELEASE_WAIT {
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_idle() {
    let t0 = Instant::now();
    while mode() != Mode::Idle && t0.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(5));
    }
}

/// Recording starts once the shortcut keys are released, so they never end
/// up at the start of the recording.
pub fn start_recording() {
    if STARTING.swap(true, SeqCst) {
        return;
    }
    thread::spawn(|| {
        wait_idle();
        if mode() != Mode::Idle {
            STARTING.store(false, SeqCst);
            return;
        }
        if !input::permission_ok(true) {
            STARTING.store(false, SeqCst);
            notify(Some(permission_text()));
            return;
        }
        wait_release();
        let (x, y) = input::cursor_pos();
        *REC.lock().unwrap() = Some(Rec {
            start: Instant::now(),
            steps: vec![Step { t: 0, ev: Ev::Move { x, y } }],
        });
        set_mode(Mode::Recording);
        match input::start() {
            Ok(h) => {
                *HOOK.lock().unwrap() = Some(h);
                STARTING.store(false, SeqCst);
                notify(None);
            }
            Err(e) => {
                set_mode(Mode::Idle);
                *REC.lock().unwrap() = None;
                STARTING.store(false, SeqCst);
                notify(Some(e));
            }
        }
    });
}

pub fn stop_recording(by: StopBy, tray_hit: &dyn Fn(i32, i32) -> bool) {
    if mode() != Mode::Recording {
        return;
    }
    set_mode(Mode::Idle);
    drop(HOOK.lock().unwrap().take());
    let Some(rec) = REC.lock().unwrap().take() else {
        notify(None);
        return;
    };
    let mut steps = rec.steps;
    match by {
        StopBy::Hotkey(chord) => trim_chord(&mut steps, chord),
        StopBy::Menu => trim_menu(&mut steps, tray_hit),
    }
    drop_orphans(&mut steps);
    if steps.len() > 1 {
        *MACRO.lock().unwrap() = steps.clone();
        if let Some(save) = SAVE.get() {
            save(steps);
        }
    }
    notify(None);
}

/// Removes the trailing key presses of the stop shortcut: walks back over key
/// downs (ignoring moves and releases) until `chord` distinct keys were seen.
pub fn trim_chord(steps: &mut Vec<Step>, chord: usize) {
    let mut seen: Vec<u32> = Vec::new();
    let mut cut = steps.len();
    for (i, s) in steps.iter().enumerate().rev() {
        match s.ev {
            Ev::Key { code, down: true, .. } => {
                if !seen.contains(&code) {
                    seen.push(code);
                }
                cut = i;
                if seen.len() >= chord {
                    break;
                }
            }
            Ev::Key { .. } | Ev::Move { .. } => {}
            _ => break,
        }
    }
    if seen.len() >= chord {
        steps.truncate(cut.max(1));
    }
}

/// Removes the clicks used to open the tray menu and pick "Stop recording".
pub fn trim_menu(steps: &mut Vec<Step>, tray_hit: &dyn Fn(i32, i32) -> bool) {
    let downs: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s.ev, Ev::Button { down: true, .. }))
        .map(|(i, _)| i)
        .collect();
    let on_tray = downs.iter().rev().copied().find(|&i| match steps[i].ev {
        Ev::Button { x, y, .. } => tray_hit(x, y),
        _ => false,
    });
    let cut = on_tray.or_else(|| downs.len().checked_sub(2).map(|k| downs[k]));
    if let Some(c) = cut {
        steps.truncate(c.max(1));
    }
}

/// Drops releases whose press was not recorded (keys held when recording
/// started) so playback never sends a stray release.
pub fn drop_orphans(steps: &mut Vec<Step>) {
    let mut keys: Vec<u32> = Vec::new();
    let mut buttons = 0u32;
    steps.retain(|s| match s.ev {
        Ev::Key { code, down, .. } => {
            if down {
                if !keys.contains(&code) {
                    keys.push(code);
                }
                true
            } else if let Some(p) = keys.iter().position(|&k| k == code) {
                keys.remove(p);
                true
            } else {
                false
            }
        }
        Ev::Button { b, down, .. } => {
            let bit = 1u32 << b;
            if down {
                buttons |= bit;
                true
            } else if buttons & bit != 0 {
                buttons &= !bit;
                true
            } else {
                false
            }
        }
        _ => true,
    });
}

/// Plays the saved recording once, or again and again until the user takes
/// back control when `repeat` is set.
pub fn play(repeat: bool) {
    if MODE
        .compare_exchange(0, 2, SeqCst, SeqCst)
        .is_err()
    {
        return;
    }
    let steps = MACRO.lock().unwrap().clone();
    if steps.len() <= 1 {
        set_mode(Mode::Idle);
        notify(None);
        return;
    }
    ABORT.store(false, SeqCst);
    LOOPING.store(repeat, SeqCst);
    notify(None);
    thread::spawn(move || {
        if !input::permission_ok(true) {
            set_mode(Mode::Idle);
            notify(Some(permission_text()));
            return;
        }
        wait_release();
        let hook = match input::start() {
            Ok(h) => h,
            Err(e) => {
                set_mode(Mode::Idle);
                notify(Some(e));
                return;
            }
        };
        input::begin_playback();
        let mut rng = Rng::seeded();
        let interrupted = loop {
            let pass = Instant::now();
            let done = if RANDOM.load(SeqCst) { run(&jitter(&steps, &mut rng)) } else { run(&steps) };
            if done {
                break true;
            }
            if !repeat {
                break false;
            }
            while pass.elapsed() < MIN_PASS && !ABORT.load(SeqCst) {
                thread::sleep(Duration::from_millis(2));
            }
        };
        input::end_playback();
        drop(hook);
        if interrupted {
            *USER_STOP_AT.lock().unwrap() = Some(Instant::now());
        }
        set_mode(Mode::Idle);
        notify(None);
    });
}

/// Replays steps on their original timeline. Returns true when the user
/// interrupted. Every key and button still held is released on the way out.
fn run(steps: &[Step]) -> bool {
    let start = Instant::now();
    let mut held: Vec<Ev> = Vec::new();
    let mut buttons = 0u32;
    let mut interrupted = false;
    'outer: for s in steps {
        let due = start + Duration::from_micros(s.t);
        loop {
            if ABORT.load(SeqCst) {
                interrupted = true;
                break 'outer;
            }
            let now = Instant::now();
            if now >= due {
                break;
            }
            thread::sleep((due - now).min(Duration::from_millis(2)));
        }
        match s.ev {
            Ev::Key { code, down, .. } => {
                held.retain(|h| !matches!(h, Ev::Key { code: c, .. } if *c == code));
                if down {
                    held.push(s.ev);
                }
            }
            Ev::Button { b, down, .. } => {
                if down {
                    buttons |= 1 << b;
                } else {
                    buttons &= !(1 << b);
                }
            }
            _ => {}
        }
        input::inject(&s.ev, buttons);
    }
    for h in held.iter().rev() {
        if let Ev::Key { code, scan, flags, .. } = *h {
            input::inject(&Ev::Key { code, scan, flags, down: false }, buttons);
        }
    }
    let (x, y) = input::cursor_pos();
    for b in 1..=5u8 {
        if buttons & (1 << b) != 0 {
            buttons &= !(1 << b);
            input::inject(&Ev::Button { b, down: false, x, y, n: 1 }, buttons);
        }
    }
    interrupted
}

pub fn stop_playing() {
    if mode() == Mode::Playing {
        ABORT.store(true, SeqCst);
    }
}

/// Record shortcut: toggles recording; while playing it stops playback and
/// starts a new recording.
pub fn record_hotkey(chord: usize, tray_hit: &dyn Fn(i32, i32) -> bool) {
    match mode() {
        Mode::Recording => stop_recording(StopBy::Hotkey(chord), tray_hit),
        Mode::Playing => {
            stop_playing();
            start_recording();
        }
        Mode::Idle => start_recording(),
    }
}

/// Play once and loop, from a shortcut or a button: toggle playback; while
/// recording they stop the recording and play it right away.
pub fn play_toggle(repeat: bool, by: StopBy, tray_hit: &dyn Fn(i32, i32) -> bool) {
    match mode() {
        Mode::Playing => stop_playing(),
        Mode::Recording => {
            stop_recording(by, tray_hit);
            play(repeat);
        }
        Mode::Idle => {
            let recent = USER_STOP_AT
                .lock()
                .unwrap()
                .is_some_and(|t| t.elapsed() < REPLAY_GUARD);
            if !recent {
                play(repeat);
            }
        }
    }
}

pub fn shutdown() {
    stop_playing();
    set_mode(Mode::Idle);
    drop(HOOK.lock().unwrap().take());
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(300) && ABORT.load(SeqCst) && mode() == Mode::Playing {
        thread::sleep(Duration::from_millis(5));
    }
}

fn permission_text() -> String {
    "Allow Mouse Keys Record Replay in System Settings, Privacy and Security, under Accessibility and Input Monitoring, then try again.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u32, down: bool) -> Step {
        Step { t: 0, ev: Ev::Key { code, scan: 0, flags: 0, down } }
    }
    fn mv() -> Step {
        Step { t: 0, ev: Ev::Move { x: 1, y: 1 } }
    }
    fn btn(down: bool, x: i32) -> Step {
        Step { t: 0, ev: Ev::Button { b: 1, down, x, y: 0, n: 1 } }
    }

    #[test]
    fn trims_stop_chord_with_autorepeat_and_moves() {
        let mut s = vec![mv(), key(65, true), key(65, false), key(17, true), mv(), key(18, true), key(16, true), key(16, true), key(112, true)];
        trim_chord(&mut s, 4);
        assert_eq!(s, vec![mv(), key(65, true), key(65, false)]);
    }

    #[test]
    fn keeps_recording_when_chord_not_found() {
        let mut s = vec![mv(), btn(true, 5), btn(false, 5), key(112, true)];
        trim_chord(&mut s, 4);
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn drops_orphan_releases() {
        let mut s = vec![mv(), key(17, false), btn(false, 0), key(65, true), key(65, false)];
        drop_orphans(&mut s);
        assert_eq!(s, vec![mv(), key(65, true), key(65, false)]);
    }

    #[test]
    fn menu_trim_cuts_at_tray_click() {
        let mut s = vec![mv(), btn(true, 10), btn(false, 10), btn(true, 900), btn(false, 900), btn(true, 880), btn(false, 880)];
        trim_menu(&mut s, &|x, _| x == 900);
        assert_eq!(s.len(), 3);
    }

    fn at(t: u64, s: Step) -> Step {
        Step { t, ..s }
    }

    #[test]
    fn random_timing_keeps_events_order_and_holds() {
        let src = vec![
            at(0, mv()),
            at(100_000, btn(true, 10)),
            at(160_000, btn(false, 10)),
            at(900_000, mv()),
            at(1_000_000, key(65, true)),
            at(1_080_000, key(65, false)),
            at(1_200_000, key(66, true)),
            at(1_250_000, key(66, false)),
        ];
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..500 {
            let out = jitter(&src, &mut rng);
            assert_eq!(out.len(), src.len());
            for (a, b) in out.iter().zip(&src) {
                assert_eq!(a.ev, b.ev);
            }
            assert!(out.windows(2).all(|w| w[0].t <= w[1].t));
            assert_eq!(out[0].t, 0);
            assert_eq!(out[2].t - out[1].t, 60_000);
            assert_eq!(out[5].t - out[4].t, 80_000);
            assert_eq!(out[7].t - out[6].t, 50_000);
        }
    }

    #[test]
    fn random_timing_offsets_stay_in_range() {
        let mut rng = Rng(12345);
        let mut early = false;
        let mut late = false;
        for _ in 0..10_000 {
            let o = rng.offset();
            assert!((JITTER_MIN..=JITTER_MAX).contains(&o.abs()));
            early |= o < 0;
            late |= o > 0;
        }
        assert!(early && late);
        let src = vec![at(0, mv()), at(5_000_000, key(65, true)), at(5_050_000, key(65, false))];
        for _ in 0..1_000 {
            let out = jitter(&src, &mut rng);
            let d = (out[1].t as i64 - 5_000_000).abs();
            assert!((JITTER_MIN..=JITTER_MAX).contains(&d));
        }
    }

    #[test]
    fn random_timing_keeps_key_repeat_with_its_press() {
        let src = vec![at(0, mv()), at(1_000_000, key(65, true)), at(1_500_000, key(65, true)), at(1_530_000, key(65, true)), at(1_600_000, key(65, false))];
        let mut rng = Rng(4242);
        for _ in 0..500 {
            let out = jitter(&src, &mut rng);
            assert_eq!(out[4].t - out[1].t, 600_000);
        }
    }

    #[test]
    fn random_timing_keeps_double_click_together() {
        let dbl = Step { t: 0, ev: Ev::Button { b: 1, down: true, x: 10, y: 0, n: 2 } };
        let src = vec![at(0, mv()), at(2_000_000, btn(true, 10)), at(2_050_000, btn(false, 10)), at(2_150_000, dbl), at(2_200_000, btn(false, 10))];
        let mut rng = Rng(777);
        for _ in 0..500 {
            let out = jitter(&src, &mut rng);
            assert_eq!(out[3].t - out[1].t, 150_000);
        }
    }

    #[test]
    fn menu_trim_falls_back_to_last_two_clicks() {
        let mut s = vec![mv(), btn(true, 10), btn(false, 10), btn(true, 900), btn(false, 900), btn(true, 880), btn(false, 880)];
        trim_menu(&mut s, &|_, _| false);
        assert_eq!(s.len(), 3);
    }
}

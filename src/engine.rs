use crate::input::{self, Ev};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering::SeqCst};
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
    /// The recording reached `MAX_STEPS`.
    Limit,
}

/// A recording stops and saves itself at this many events, so one left
/// running cannot fill memory and disk. About 50 MB on disk.
pub const MAX_STEPS: usize = 1_000_000;
const LIMIT_TEXT: &str =
    "The recording reached the limit of 1,000,000 events, so it stopped and was saved.";

/// Adds a step unless the recording is full. True when this step filled it.
fn push_capped(steps: &mut Vec<Step>, step: Step, max: usize) -> bool {
    if steps.len() >= max {
        return false;
    }
    steps.push(step);
    steps.len() == max
}

struct Rec {
    start: Instant,
    steps: Vec<Step>,
    /// Pointer movement is left out of this recording.
    skip_moves: bool,
}

static MODE: AtomicU8 = AtomicU8::new(0);
static ABORT: AtomicBool = AtomicBool::new(false);
static STARTING: AtomicBool = AtomicBool::new(false);
static LOOPING: AtomicBool = AtomicBool::new(false);
static RANDOM: AtomicBool = AtomicBool::new(false);
static STOP_ON_INPUT: AtomicBool = AtomicBool::new(true);
static SKIP_MOVES: AtomicBool = AtomicBool::new(false);
/// The saved recording holds no pointer movement: see `free_cursor`.
static FREE: AtomicBool = AtomicBool::new(false);
/// Playback speed as f64 bits; 1.0 by default.
static SPEED: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000);
static CHORD: Mutex<ChordGate> = Mutex::new(ChordGate {
    held: Vec::new(),
    used: false,
});
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
/// Shortest real time between replaying a press and its release. Injection
/// keeps every down and up in order, so apps that read input events see each
/// pair at any speed, but games and apps that poll key state miss a key that
/// goes up in the same instant it went down. 5 ms spans a 200 Hz poll.
const MIN_HOLD: Duration = Duration::from_millis(5);

pub fn init(
    saved: Vec<Step>,
    notify: impl Fn(Option<String>) + Send + Sync + 'static,
    save: impl Fn(Vec<Step>) + Send + Sync + 'static,
) {
    set_macro(saved);
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
            let full = match REC.lock().unwrap().as_mut() {
                Some(r) if !(r.skip_moves && matches!(ev, Ev::Move { .. })) => {
                    let t = r.start.elapsed().as_micros() as u64;
                    push_capped(&mut r.steps, Step { t, ev }, MAX_STEPS)
                }
                _ => false,
            };
            if full {
                // Stopping joins the listener thread, which is this one.
                thread::spawn(|| stop_recording(StopBy::Limit, &|_, _| false));
            }
        }
        Mode::Playing => {
            if !input_stops() {
                return;
            }
            let stop = match ev {
                Ev::Key { code, down, .. } if input::speed_chord_key(code) => {
                    CHORD.lock().unwrap().key(code, down)
                }
                _ => true,
            };
            if stop {
                ABORT.store(true, SeqCst);
            }
        }
        Mode::Idle => {}
    }
}

/// Lets the speed shortcuts change speed during playback without stopping
/// it. Their modifier and F keys are held back while pressed; once all are up
/// playback stops unless a speed shortcut fired in between. Every other key
/// and any mouse input stops playback at once while `input_stops` holds.
struct ChordGate {
    held: Vec<u32>,
    used: bool,
}

impl ChordGate {
    /// Returns true when this key event should stop playback.
    fn key(&mut self, code: u32, down: bool) -> bool {
        if down {
            if !self.held.contains(&code) {
                self.held.push(code);
            }
            return false;
        }
        self.held.retain(|&k| k != code);
        if !self.held.is_empty() {
            return false;
        }
        !std::mem::replace(&mut self.used, false)
    }

    fn reset(&mut self) {
        self.held.clear();
        self.used = false;
    }
}

pub fn set_speed(f: f64) {
    SPEED.store(f.to_bits(), SeqCst);
}

/// A speed shortcut fired: the same chord's key presses must not stop playback.
pub fn speed_hotkey(f: f64) {
    set_speed(f);
    CHORD.lock().unwrap().used = true;
}

fn speed() -> f64 {
    f64::from_bits(SPEED.load(SeqCst))
}

/// (actions, seconds) of the saved recording.
pub fn summary() -> (usize, f32) {
    summarize(&MACRO.lock().unwrap())
}

/// (actions, seconds) of any recording: presses, clicks and wheel turns.
pub fn summarize(m: &[Step]) -> (usize, f32) {
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

/// Swaps in a saved recording. Refused while recording or playing.
pub fn set_recording(steps: Vec<Step>) -> bool {
    if mode() != Mode::Idle {
        return false;
    }
    set_macro(steps);
    true
}

/// A recording with no pointer movement plays at the pointer, wherever the
/// user has it, and the user's own input does not stop it.
fn free_cursor(steps: &[Step]) -> bool {
    !steps.is_empty() && !steps.iter().any(|s| matches!(s.ev, Ev::Move { .. }))
}

fn set_macro(steps: Vec<Step>) {
    FREE.store(free_cursor(&steps), SeqCst);
    *MACRO.lock().unwrap() = steps;
}

/// True when moving the mouse or pressing a key stops playback of the saved
/// recording. False leaves the shortcuts and the tray menu as the only stop.
pub fn input_stops() -> bool {
    stops(
        STOP_ON_INPUT.load(SeqCst),
        SKIP_MOVES.load(SeqCst),
        FREE.load(SeqCst),
    )
}

/// The user's own input stops playback only while playback drives the
/// pointer: not when moves are ignored, not for a recording without moves.
fn stops(stop_on_input: bool, skip_moves: bool, no_moves: bool) -> bool {
    stop_on_input && !skip_moves && !no_moves
}

pub fn set_stop_on_input(on: bool) {
    STOP_ON_INPUT.store(on, SeqCst);
}

/// Read when a recording starts, and live during playback.
pub fn set_skip_moves(on: bool) {
    SKIP_MOVES.store(on, SeqCst);
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
        if r & 1 == 0 {
            size
        } else {
            -size
        }
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
            Ev::Key {
                code, down: true, ..
            } => {
                let fresh = !held.contains(&code);
                if fresh {
                    held.push(code);
                }
                fresh
            }
            Ev::Key {
                code, down: false, ..
            } => {
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
        out.push(Step {
            t: nt as u64,
            ev: s.ev,
        });
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
        let skip_moves = SKIP_MOVES.load(SeqCst);
        let steps = if skip_moves {
            Vec::new()
        } else {
            let (x, y) = input::cursor_pos();
            vec![Step {
                t: 0,
                ev: Ev::Move { x, y },
            }]
        };
        *REC.lock().unwrap() = Some(Rec {
            start: Instant::now(),
            steps,
            skip_moves,
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
        StopBy::Limit => {}
    }
    drop_orphans(&mut steps);
    if steps.len() > 1 {
        set_macro(steps.clone());
        if let Some(save) = SAVE.get() {
            save(steps);
        }
    }
    notify(matches!(by, StopBy::Limit).then(|| LIMIT_TEXT.to_string()));
}

/// Removes the trailing key presses of the stop shortcut: walks back over key
/// downs (ignoring moves and releases) until `chord` distinct keys were seen.
pub fn trim_chord(steps: &mut Vec<Step>, chord: usize) {
    let mut seen: Vec<u32> = Vec::new();
    let mut cut = steps.len();
    for (i, s) in steps.iter().enumerate().rev() {
        match s.ev {
            Ev::Key {
                code, down: true, ..
            } => {
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
    if MODE.compare_exchange(0, 2, SeqCst, SeqCst).is_err() {
        return;
    }
    let steps = MACRO.lock().unwrap().clone();
    if steps.len() <= 1 {
        set_mode(Mode::Idle);
        notify(None);
        return;
    }
    ABORT.store(false, SeqCst);
    CHORD.lock().unwrap().reset();
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
            let done = if RANDOM.load(SeqCst) {
                run(&jitter(&steps, &mut rng))
            } else {
                run(&steps)
            };
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

/// Position on the recording's timeline, in recorded microseconds. It moves
/// at the speed read on every tick, so a change applies to the very next
/// event without restarting.
struct Clock {
    pos: f64,
    last: Instant,
}

impl Clock {
    fn new(now: Instant) -> Clock {
        Clock {
            pos: 0.0,
            last: now,
        }
    }

    fn advance(&mut self, now: Instant, speed: f64) {
        self.pos += now.saturating_duration_since(self.last).as_secs_f64() * 1e6 * speed;
        self.last = now;
    }

    /// Real time until timeline position `t` at `speed`, never negative.
    fn wait(&self, t: u64, speed: f64) -> Duration {
        Duration::from_secs_f64(((t as f64 - self.pos) / speed / 1e6).max(0.0))
    }
}

/// The key or button an event presses or releases, for MIN_HOLD.
fn hold_id(ev: &Ev) -> Option<(u64, bool)> {
    match *ev {
        Ev::Key { code, down, .. } => Some((code as u64, down)),
        Ev::Button { b, down, .. } => Some((1 << 32 | b as u64, down)),
        _ => None,
    }
}

/// Sleeps in short slices until `until`. Returns false when the user interrupted.
fn sleep_until(until: Instant) -> bool {
    loop {
        if ABORT.load(SeqCst) {
            return false;
        }
        let now = Instant::now();
        if now >= until {
            return true;
        }
        thread::sleep((until - now).min(Duration::from_millis(2)));
    }
}

/// Replays steps on their recorded timeline at the current speed. Returns
/// true when the user interrupted. Every key and button still held is
/// released on the way out.
fn run(steps: &[Step]) -> bool {
    let no_moves = free_cursor(steps);
    let mut clock = Clock::new(Instant::now());
    let mut held: Vec<Ev> = Vec::new();
    let mut pressed: Vec<(u64, Instant)> = Vec::new();
    let mut buttons = 0u32;
    let mut interrupted = false;
    'outer: for s in steps {
        loop {
            if ABORT.load(SeqCst) {
                interrupted = true;
                break 'outer;
            }
            let sp = speed();
            clock.advance(Instant::now(), sp);
            if clock.pos >= s.t as f64 {
                break;
            }
            thread::sleep(clock.wait(s.t, sp).min(Duration::from_millis(2)));
        }
        // The clock keeps running while a release waits, so later events catch up.
        if let Some((id, down)) = hold_id(&s.ev) {
            if down {
                pressed.retain(|p| p.0 != id);
                pressed.push((id, Instant::now()));
            } else if let Some(i) = pressed.iter().position(|p| p.0 == id) {
                let (_, at) = pressed.swap_remove(i);
                if !sleep_until(at + MIN_HOLD) {
                    interrupted = true;
                    break 'outer;
                }
            }
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
        let free = no_moves || SKIP_MOVES.load(SeqCst);
        if let Some(ev) = at_pointer(s.ev, free, input::cursor_pos) {
            input::inject(&ev, buttons);
        }
    }
    for h in held.iter().rev() {
        if let Ev::Key {
            code, scan, flags, ..
        } = *h
        {
            input::inject(
                &Ev::Key {
                    code,
                    scan,
                    flags,
                    down: false,
                },
                buttons,
            );
        }
    }
    let (x, y) = input::cursor_pos();
    for b in 1..=5u8 {
        if buttons & (1 << b) != 0 {
            buttons &= !(1 << b);
            input::inject(
                &Ev::Button {
                    b,
                    down: false,
                    x,
                    y,
                    n: 1,
                },
                buttons,
            );
        }
    }
    interrupted
}

/// While the pointer is left to the user, recorded moves are not replayed
/// and a click lands where the pointer is now, not where it was recorded.
fn at_pointer(ev: Ev, free: bool, pos: impl Fn() -> (i32, i32)) -> Option<Ev> {
    match ev {
        Ev::Move { .. } if free => None,
        Ev::Button { b, down, n, .. } if free => {
            let (x, y) = pos();
            Some(Ev::Button { b, down, x, y, n })
        }
        e => Some(e),
    }
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
    while t0.elapsed() < Duration::from_millis(300) && ABORT.load(SeqCst) && mode() == Mode::Playing
    {
        thread::sleep(Duration::from_millis(5));
    }
}

pub fn permission_text() -> String {
    "Mouse Keys Record Replay needs Accessibility and Input Monitoring. Click Open System Settings, turn the app on in the list, then try again.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: u32, down: bool) -> Step {
        Step {
            t: 0,
            ev: Ev::Key {
                code,
                scan: 0,
                flags: 0,
                down,
            },
        }
    }
    fn mv() -> Step {
        Step {
            t: 0,
            ev: Ev::Move { x: 1, y: 1 },
        }
    }
    fn btn(down: bool, x: i32) -> Step {
        Step {
            t: 0,
            ev: Ev::Button {
                b: 1,
                down,
                x,
                y: 0,
                n: 1,
            },
        }
    }

    #[test]
    fn recording_stops_growing_at_the_cap() {
        let mut s = Vec::new();
        assert!(!push_capped(&mut s, mv(), 2));
        assert!(push_capped(&mut s, mv(), 2));
        assert!(!push_capped(&mut s, mv(), 2));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn trims_stop_chord_with_autorepeat_and_moves() {
        let mut s = vec![
            mv(),
            key(65, true),
            key(65, false),
            key(17, true),
            mv(),
            key(18, true),
            key(16, true),
            key(16, true),
            key(112, true),
        ];
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
        let mut s = vec![
            mv(),
            key(17, false),
            btn(false, 0),
            key(65, true),
            key(65, false),
        ];
        drop_orphans(&mut s);
        assert_eq!(s, vec![mv(), key(65, true), key(65, false)]);
    }

    #[test]
    fn menu_trim_cuts_at_tray_click() {
        let mut s = vec![
            mv(),
            btn(true, 10),
            btn(false, 10),
            btn(true, 900),
            btn(false, 900),
            btn(true, 880),
            btn(false, 880),
        ];
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
        let src = vec![
            at(0, mv()),
            at(5_000_000, key(65, true)),
            at(5_050_000, key(65, false)),
        ];
        for _ in 0..1_000 {
            let out = jitter(&src, &mut rng);
            let d = (out[1].t as i64 - 5_000_000).abs();
            assert!((JITTER_MIN..=JITTER_MAX).contains(&d));
        }
    }

    #[test]
    fn random_timing_keeps_key_repeat_with_its_press() {
        let src = vec![
            at(0, mv()),
            at(1_000_000, key(65, true)),
            at(1_500_000, key(65, true)),
            at(1_530_000, key(65, true)),
            at(1_600_000, key(65, false)),
        ];
        let mut rng = Rng(4242);
        for _ in 0..500 {
            let out = jitter(&src, &mut rng);
            assert_eq!(out[4].t - out[1].t, 600_000);
        }
    }

    #[test]
    fn random_timing_keeps_double_click_together() {
        let dbl = Step {
            t: 0,
            ev: Ev::Button {
                b: 1,
                down: true,
                x: 10,
                y: 0,
                n: 2,
            },
        };
        let src = vec![
            at(0, mv()),
            at(2_000_000, btn(true, 10)),
            at(2_050_000, btn(false, 10)),
            at(2_150_000, dbl),
            at(2_200_000, btn(false, 10)),
        ];
        let mut rng = Rng(777);
        for _ in 0..500 {
            let out = jitter(&src, &mut rng);
            assert_eq!(out[3].t - out[1].t, 150_000);
        }
    }

    #[test]
    fn menu_trim_falls_back_to_last_two_clicks() {
        let mut s = vec![
            mv(),
            btn(true, 10),
            btn(false, 10),
            btn(true, 900),
            btn(false, 900),
            btn(true, 880),
            btn(false, 880),
        ];
        trim_menu(&mut s, &|_, _| false);
        assert_eq!(s.len(), 3);
    }

    const STEPS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 100.0];

    #[test]
    fn clock_scales_delays_by_speed() {
        let t0 = Instant::now();
        for sp in STEPS {
            let mut c = Clock::new(t0);
            // One recorded second takes 1/speed real seconds.
            let w = c.wait(1_000_000, sp).as_secs_f64();
            assert!((w - 1.0 / sp).abs() < 1e-9, "{sp}: {w}");
            c.advance(t0 + Duration::from_millis(10), sp);
            assert!((c.pos - 10_000.0 * sp).abs() < 1e-6, "{sp}: {}", c.pos);
        }
    }

    #[test]
    fn clock_follows_a_live_speed_change() {
        let t0 = Instant::now();
        let mut c = Clock::new(t0);
        c.advance(t0 + Duration::from_millis(100), 1.0);
        c.advance(t0 + Duration::from_millis(200), 4.0);
        assert!((c.pos - 500_000.0).abs() < 1e-6);
        assert!((c.wait(1_000_000, 4.0).as_secs_f64() - 0.125).abs() < 1e-9);
        // A position already passed never asks for a negative wait.
        assert_eq!(c.wait(10, 0.25), Duration::ZERO);
    }

    #[test]
    fn random_timing_scales_with_speed_never_negative_or_reordered() {
        // Presses 1.2 s apart, so a half second shift never needs clamping.
        let mut src = vec![at(0, mv())];
        for i in 0..30u64 {
            let code = 65 + (i % 5) as u32;
            src.push(at(1_000_000 + i * 1_200_000, key(code, true)));
            src.push(at(1_000_000 + i * 1_200_000 + 30_000, key(code, false)));
            src.push(at(1_000_000 + i * 1_200_000 + 60_000, mv()));
        }
        let mut rng = Rng(0xDEAD_BEEF_1234_5678);
        for sp in STEPS {
            for _ in 0..50 {
                let out = jitter(&src, &mut rng);
                let c = Clock::new(Instant::now());
                let mut last = Duration::ZERO;
                for (a, b) in out.iter().zip(&src) {
                    assert_eq!(a.ev, b.ev);
                    // Real delay from playback start at this speed is ordered.
                    let w = c.wait(a.t, sp);
                    assert!(w >= last);
                    last = w;
                    let off = (a.t as f64 - b.t as f64).abs() / sp / 1e6;
                    assert!(off <= 0.5 / sp + 1e-9, "{sp}: {off}");
                }
            }
        }
    }

    #[test]
    fn chord_gate_keeps_playing_through_a_speed_shortcut() {
        let mut g = ChordGate {
            held: Vec::new(),
            used: false,
        };
        assert!(!g.key(17, true));
        assert!(!g.key(18, true));
        assert!(!g.key(118, true));
        g.used = true;
        assert!(!g.key(118, false));
        assert!(!g.key(17, false));
        assert!(!g.key(18, false));
        // The same keys with no speed shortcut firing still stop playback.
        assert!(!g.key(17, true));
        assert!(g.key(17, false));
        g.used = true;
        g.reset();
        assert!(g.held.is_empty() && !g.used);
    }

    #[test]
    fn recording_without_moves_plays_at_the_pointer() {
        let free = vec![btn(true, 10), btn(false, 10), key(65, true)];
        assert!(free_cursor(&free));
        assert!(!free_cursor(&[mv(), btn(true, 10)]));
        assert!(!free_cursor(&[]));
        let here = || (700, 300);
        let moved = Ev::Button {
            b: 1,
            down: true,
            x: 700,
            y: 300,
            n: 1,
        };
        assert_eq!(at_pointer(btn(true, 10).ev, true, here), Some(moved));
        assert_eq!(
            at_pointer(btn(true, 10).ev, false, here),
            Some(btn(true, 10).ev)
        );
        assert_eq!(
            at_pointer(key(65, true).ev, true, here),
            Some(key(65, true).ev)
        );
    }

    #[test]
    fn ignoring_moves_applies_to_playback_of_any_recording() {
        // Reported: toggle on, a recording that holds moves, moving the mouse still stopped the loop.
        assert!(!stops(true, true, false));
        assert!(stops(true, false, false));
        assert!(!stops(true, false, true));
        assert!(!stops(false, false, false));
        let here = || (700, 300);
        assert_eq!(at_pointer(mv().ev, true, here), None);
        assert_eq!(at_pointer(mv().ev, false, here), Some(mv().ev));
    }

    #[test]
    fn presses_and_releases_share_a_hold_id() {
        let d = hold_id(&key(65, true).ev).unwrap();
        let u = hold_id(&key(65, false).ev).unwrap();
        assert_eq!((d.0, d.1, u.1), (u.0, true, false));
        assert_ne!(hold_id(&btn(true, 0).ev).unwrap().0, d.0);
        assert!(hold_id(&mv().ev).is_none());
    }
}

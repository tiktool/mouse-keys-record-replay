//! Tray, menu, global shortcuts and the state the settings window shows.
//! Everything here runs on the main thread; other threads reach it through
//! the event loop proxy.

use crate::config::{self, Settings, APP_NAME, VERSION};
use crate::engine::{self, Mode};
use crate::shortcuts::{self, Mods, Which};
use crate::ui::hud::{Hud, HudKind};
use crate::{autostart, history, icons, input, speed, ui, update};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug)]
pub enum UserEvent {
    Engine(Option<String>),
    HotKey(GlobalHotKeyEvent),
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    Show,
    Update(update::Outcome),
    Hud,
    Saved(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Error,
    Success,
}

#[derive(Clone, Debug)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

/// Everything a settings window renders. Built here so every platform shows
/// the same words.
#[derive(Clone, Debug)]
pub struct View {
    pub mode: Mode,
    pub status_title: String,
    pub status_detail: String,
    pub record: String,
    pub play: String,
    pub repeat: String,
    pub history: String,
    pub looping: bool,
    pub autostart: bool,
    pub auto_update: bool,
    pub random_timing: bool,
    pub stop_on_input: bool,
    pub skip_moves: bool,
    /// How playback of the saved recording is stopped, as one sentence.
    #[cfg_attr(windows, allow(dead_code))]
    pub stop_hint: String,
    /// Index into speed::LABELS, the custom factor and its box text.
    pub speed_step: usize,
    pub custom_speed: f64,
    pub speed_error: Option<String>,
    /// Labels of the fixed speed shortcuts, one per step.
    pub speed_keys: Vec<String>,
    pub notice: Option<Notice>,
    pub update_text: String,
    pub version: &'static str,
    pub capturing: Option<Which>,
}

struct Items {
    status: MenuItem,
    record: MenuItem,
    play: MenuItem,
    repeat: MenuItem,
    recordings: MenuItem,
    random: CheckMenuItem,
    stop_on_input: CheckMenuItem,
    skip_moves: CheckMenuItem,
    autostart: CheckMenuItem,
    settings: MenuItem,
    update: MenuItem,
    star: MenuItem,
    quit: MenuItem,
}

struct App {
    settings: Settings,
    manager: GlobalHotKeyManager,
    hk_record: Option<HotKey>,
    hk_play: Option<HotKey>,
    hk_loop: Option<HotKey>,
    hk_speed: Vec<(HotKey, usize)>,
    speed_error: Option<String>,
    hk_history: Option<HotKey>,
    tray: TrayIcon,
    items: Items,
    last_hotkey: Instant,
    notice: Option<Notice>,
    capturing: Option<Which>,
    update_text: String,
    restart_pending: bool,
    shown_mode: Option<Mode>,
    hud_mode: Mode,
    saved_until: Option<Instant>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

static PROXY: OnceLock<Mutex<EventLoopProxy<UserEvent>>> = OnceLock::new();

pub fn send(e: UserEvent) {
    if let Some(p) = PROXY.get() {
        let _ = p.lock().unwrap().send_event(e);
    }
}

fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

impl App {
    fn view(&self) -> View {
        let rec = shortcuts::label(&self.settings.record);
        let play = shortcuts::label(&self.settings.play);
        let repeat = shortcuts::label(&self.settings.repeat);
        let (actions, secs) = engine::summary();
        let mode = engine::mode();
        let looping = engine::looping();
        let input_stops = engine::input_stops();
        let stop_hint = if input_stops {
            "During playback, move the mouse or press any key to take back control.".to_string()
        } else {
            format!("During playback, press {play} or {repeat} to stop. Your own mouse and keyboard input does not stop it.")
        };
        let (title, detail) = match mode {
            Mode::Recording => ("Recording".to_string(), format!("Press {rec} to stop, {play} to play it once or {repeat} to loop it.")),
            Mode::Playing if looping && input_stops => ("Looping".to_string(), "Repeats until you move the mouse or press any key.".to_string()),
            Mode::Playing if looping => ("Looping".to_string(), format!("Repeats until you press {repeat}.")),
            Mode::Playing if input_stops => ("Playing".to_string(), "Move the mouse or press any key to take back control.".to_string()),
            Mode::Playing => ("Playing".to_string(), format!("Press {play} to stop.")),
            Mode::Idle if engine::has_recording() => (
                "Ready".to_string(),
                format!(
                    "Saved recording: {actions} {} in {secs:.1} seconds. Press {play} to play it once or {repeat} to loop it.",
                    if actions == 1 { "action" } else { "actions" }
                ),
            ),
            Mode::Idle => ("Ready".to_string(), format!("Press {rec} to start recording your mouse and keyboard.")),
        };
        View {
            mode,
            status_title: title,
            status_detail: detail,
            record: rec,
            play,
            repeat,
            history: shortcuts::label(&self.settings.history),
            looping,
            autostart: self.settings.autostart,
            auto_update: self.settings.auto_update,
            random_timing: self.settings.random_timing,
            stop_on_input: self.settings.stop_on_input,
            skip_moves: self.settings.skip_moves,
            stop_hint,
            speed_step: self.settings.speed_step.min(speed::CUSTOM),
            custom_speed: self.settings.custom_speed,
            speed_error: self.speed_error.clone(),
            speed_keys: shortcuts::speed_shortcuts()
                .iter()
                .map(|s| shortcuts::label(s))
                .collect(),
            notice: self.notice.clone(),
            update_text: self.update_text.clone(),
            version: VERSION,
            capturing: self.capturing,
        }
    }

    fn register(&mut self, which: Which, combo: &str) -> Result<(), String> {
        let hk = shortcuts::hotkey(combo)?;
        self.manager.register(hk).map_err(|_| {
            format!(
                "{} is already used by another app. Choose a different shortcut.",
                shortcuts::label(combo)
            )
        })?;
        match which {
            Which::Record => self.hk_record = Some(hk),
            Which::Play => self.hk_play = Some(hk),
            Which::Loop => self.hk_loop = Some(hk),
            Which::History => self.hk_history = Some(hk),
        }
        Ok(())
    }

    fn unregister(&mut self, which: Which) {
        let slot = match which {
            Which::Record => &mut self.hk_record,
            Which::Play => &mut self.hk_play,
            Which::Loop => &mut self.hk_loop,
            Which::History => &mut self.hk_history,
        };
        if let Some(hk) = slot.take() {
            let _ = self.manager.unregister(hk);
        }
    }

    fn combo(&self, which: Which) -> &str {
        match which {
            Which::Record => &self.settings.record,
            Which::Play => &self.settings.play,
            Which::Loop => &self.settings.repeat,
            Which::History => &self.settings.history,
        }
    }

    fn registered(&self, which: Which) -> bool {
        match which {
            Which::Record => self.hk_record.is_some(),
            Which::Play => self.hk_play.is_some(),
            Which::Loop => self.hk_loop.is_some(),
            Which::History => self.hk_history.is_some(),
        }
    }

    fn unregister_all(&mut self) {
        for w in ALL {
            self.unregister(w);
        }
    }

    /// Registers every shortcut that is not registered yet.
    fn register_all(&mut self) {
        let mut errors = Vec::new();
        for w in ALL {
            if self.registered(w) {
                continue;
            }
            let c = self.combo(w).to_string();
            if let Err(e) = self.register(w, &c) {
                errors.push(e);
            }
        }
        if let Some(e) = errors.into_iter().next() {
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: e,
            });
        }
    }

    /// The speed shortcuts are fixed and stay registered, also while a new
    /// record or play shortcut is captured.
    fn register_speed(&mut self) {
        for (i, combo) in shortcuts::speed_shortcuts().iter().enumerate() {
            let Ok(hk) = shortcuts::hotkey(combo) else {
                continue;
            };
            if self.manager.register(hk).is_ok() {
                self.hk_speed.push((hk, i));
            } else if self.notice.is_none() {
                let text = format!(
                    "{} is already used by another app, so that speed has no shortcut.",
                    shortcuts::label(combo)
                );
                self.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text,
                });
            }
        }
    }

    fn speed(&self) -> f64 {
        speed::factor(self.settings.speed_step, self.settings.custom_speed)
    }

    fn refresh_tray(&mut self) {
        let v = self.view();
        let mode = v.mode;
        let rec_label = if mode == Mode::Recording {
            "Stop recording"
        } else {
            "Start recording"
        };
        let play_label = if mode == Mode::Playing && !v.looping {
            "Stop playback"
        } else {
            "Play once"
        };
        let loop_label = if v.looping { "Stop loop" } else { "Loop" };
        self.items.status.set_text(match mode {
            Mode::Idle => "Ready",
            Mode::Recording => "Recording...",
            Mode::Playing if v.looping => "Looping...",
            Mode::Playing => "Playing...",
        });
        self.items
            .record
            .set_text(format!("{rec_label}\t{}", v.record));
        self.items
            .play
            .set_text(format!("{play_label}\t{}", v.play));
        self.items
            .repeat
            .set_text(format!("{loop_label}\t{}", v.repeat));
        let can_play = mode != Mode::Idle || engine::has_recording();
        self.items.play.set_enabled(can_play);
        self.items.repeat.set_enabled(can_play);
        self.items.recordings.set_enabled(mode == Mode::Idle);
        self.items.random.set_checked(self.settings.random_timing);
        self.items
            .stop_on_input
            .set_checked(self.settings.stop_on_input);
        self.items.skip_moves.set_checked(self.settings.skip_moves);
        self.items.autostart.set_checked(self.settings.autostart);
        let _ = self
            .tray
            .set_tooltip(Some(format!("{APP_NAME}: {}", v.status_title)));
        if self.shown_mode != Some(mode) {
            self.shown_mode = Some(mode);
            set_icon(&self.tray, mode);
        }
    }

    fn tray_hit(&self) -> impl Fn(i32, i32) -> bool {
        let rect = self.tray.rect();
        move |x, y| match rect {
            Some(r) => {
                let (rx, ry) = (r.position.x as i32, r.position.y as i32);
                let (rw, rh) = (r.size.width as i32, r.size.height as i32);
                let pad = 6;
                x >= rx - pad && x <= rx + rw + pad && y >= ry - pad && y <= ry + rh + pad
            }
            None => false,
        }
    }
}

const ALL: [Which; 4] = [Which::Record, Which::Play, Which::Loop, Which::History];

fn set_icon(tray: &TrayIcon, mode: Mode) {
    #[cfg(target_os = "macos")]
    {
        let colored = mode != Mode::Idle;
        let px = icons::tray(mode, 44, [0, 0, 0], colored);
        if let Ok(icon) = Icon::from_rgba(px, 44, 44) {
            let _ = tray.set_icon_with_as_template(Some(icon), !colored);
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let fg = if input::tray_light_background() {
            [0x1C, 0x1C, 0x1E]
        } else {
            [0xFF, 0xFF, 0xFF]
        };
        let px = icons::tray(mode, 32, fg, true);
        if let Ok(icon) = Icon::from_rgba(px, 32, 32) {
            let _ = tray.set_icon(Some(icon));
        }
    }
}

fn after_change() {
    if engine::mode() != Mode::Idle {
        ui::panel::close();
    }
    with(|a| a.refresh_tray());
    ui::refresh();
    update_hud();
}

const SAVED_CARD: Duration = Duration::from_secs(3);

/// Picks the status card for the current mode. A finished recording gets a
/// short reminder of the play shortcuts; playing once shows nothing.
fn update_hud() {
    let hud = with(|a| {
        let mode = engine::mode();
        if a.hud_mode == Mode::Recording && mode == Mode::Idle && engine::has_recording() {
            a.saved_until = Some(Instant::now() + SAVED_CARD);
            std::thread::spawn(|| {
                std::thread::sleep(SAVED_CARD + Duration::from_millis(50));
                send(UserEvent::Hud);
            });
        } else if mode != Mode::Idle {
            a.saved_until = None;
        }
        a.hud_mode = mode;
        let key = |s: &str| (shortcuts::label(s), true);
        let text = |s: &str| (s.to_string(), false);
        let card = |kind, title: &str, line| Hud {
            kind,
            title: title.to_string(),
            line,
        };
        match mode {
            Mode::Recording => Some(card(
                HudKind::Recording,
                "RECORDING",
                vec![
                    text("Press "),
                    key(&a.settings.record),
                    text(" to stop recording"),
                ],
            )),
            Mode::Playing if engine::looping() => {
                let title = format!(
                    "LOOPING AT {}",
                    speed::step_label(a.settings.speed_step, a.settings.custom_speed)
                );
                let line = if engine::input_stops() {
                    vec![text("Move your mouse or press any key to stop looping")]
                } else {
                    vec![
                        text("Press "),
                        key(&a.settings.repeat),
                        text(" to stop looping"),
                    ]
                };
                Some(card(HudKind::Looping, &title, line))
            }
            Mode::Idle if a.saved_until.is_some_and(|t| Instant::now() < t) => Some(card(
                HudKind::Saved,
                "RECORDING SAVED",
                vec![
                    text("Press "),
                    key(&a.settings.play),
                    text(" to play once or "),
                    key(&a.settings.repeat),
                    text(" to loop"),
                ],
            )),
            _ => None,
        }
    })
    .flatten();
    ui::hud::show(hud);
}

// Functions the settings window calls. All run on the main thread.

pub fn view() -> Option<View> {
    with(|a| a.view())
}

/// Shortcuts are switched off while the window waits for a new one, so
/// pressing the current shortcut can be captured too.
pub fn begin_capture(which: Which) {
    with(|a| {
        a.capturing = Some(which);
        a.notice = None;
        a.unregister_all();
    });
    ui::refresh();
}

pub fn cancel_capture() {
    with(|a| {
        a.capturing = None;
        a.register_all();
    });
    ui::refresh();
}

/// Validates, registers and saves a captured shortcut. The previous one
/// stays active when the new one is refused.
pub fn set_shortcut(which: Which, mods: Mods, code: &str) -> Result<(), String> {
    let combo = shortcuts::compose(mods, code);
    let result = with(|a| {
        a.capturing = None;
        let mut others: Vec<String> = ALL
            .iter()
            .filter(|w| **w != which)
            .map(|w| a.combo(*w).to_string())
            .collect();
        others.extend(shortcuts::speed_shortcuts());
        let others: Vec<&str> = others.iter().map(String::as_str).collect();
        let res = shortcuts::validate(&combo, &others).and_then(|_| a.register(which, &combo));
        match &res {
            Ok(()) => {
                match which {
                    Which::Record => a.settings.record = combo.clone(),
                    Which::Play => a.settings.play = combo.clone(),
                    Which::Loop => a.settings.repeat = combo.clone(),
                    Which::History => a.settings.history = combo.clone(),
                }
                config::save_settings(&a.settings);
                a.register_all();
                a.notice = Some(Notice {
                    kind: NoticeKind::Success,
                    text: format!("Saved. {} is ready to use.", shortcuts::label(&combo)),
                });
            }
            Err(e) => {
                a.register_all();
                a.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: e.clone(),
                });
            }
        }
        res
    })
    .unwrap_or(Err("The app is not ready yet.".into()));
    after_change();
    result
}

pub fn reset_shortcuts() {
    with(|a| {
        a.unregister_all();
        a.settings.record = shortcuts::DEFAULT_RECORD.into();
        a.settings.play = shortcuts::DEFAULT_PLAY.into();
        a.settings.repeat = shortcuts::DEFAULT_LOOP.into();
        a.settings.history = shortcuts::DEFAULT_HISTORY.into();
        config::save_settings(&a.settings);
        a.notice = Some(Notice {
            kind: NoticeKind::Success,
            text: "Shortcuts are back to the defaults.".into(),
        });
        a.register_all();
    });
    after_change();
}

pub fn set_autostart(on: bool) {
    with(|a| {
        match autostart::set(on) {
            Ok(()) => a.settings.autostart = on,
            Err(e) => {
                a.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: e,
                })
            }
        }
        config::save_settings(&a.settings);
    });
    after_change();
}

pub fn set_random_timing(on: bool) {
    engine::set_random(on);
    with(|a| {
        a.settings.random_timing = on;
        config::save_settings(&a.settings);
    });
    after_change();
}

/// Applies at once, also to a playback in progress.
pub fn set_stop_on_input(on: bool) {
    engine::set_stop_on_input(on);
    with(|a| {
        a.settings.stop_on_input = on;
        config::save_settings(&a.settings);
    });
    after_change();
}

/// Applies at once to playback, also of saved recordings, and to the next
/// recording. Saved recordings keep their moves on disk.
pub fn set_skip_moves(on: bool) {
    engine::set_skip_moves(on);
    with(|a| {
        a.settings.skip_moves = on;
        config::save_settings(&a.settings);
    });
    after_change();
}

/// Picks a speed step. It applies at once, also to a playback in progress.
pub fn set_speed_step(step: usize) {
    let step = step.min(speed::CUSTOM);
    with(|a| {
        a.settings.speed_step = step;
        a.speed_error = None;
        engine::set_speed(a.speed());
        config::save_settings(&a.settings);
    });
    after_change();
}

/// Saves the typed custom speed. A refused value keeps the previous one and
/// is reported under the box.
pub fn set_custom_speed(text: &str) -> Result<(), String> {
    let res = speed::parse_custom(text);
    with(|a| match &res {
        Ok(v) => {
            a.settings.custom_speed = *v;
            a.speed_error = None;
            engine::set_speed(a.speed());
            config::save_settings(&a.settings);
        }
        Err(e) => a.speed_error = Some(e.clone()),
    });
    after_change();
    res.map(|_| ())
}

pub fn set_auto_update(on: bool) {
    with(|a| {
        a.settings.auto_update = on;
        config::save_settings(&a.settings);
    });
    after_change();
}

pub fn check_updates() {
    with(|a| a.update_text = "Checking for updates...".into());
    ui::refresh();
    update::check(true, |o| send(UserEvent::Update(o)));
}

pub fn open_repo() {
    open_url(config::REPO_URL);
}

// Only the Windows window links to the site.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn open_site() {
    open_url(config::SITE_URL);
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn toggle_record_from_ui() {
    let hit = with(|a| a.tray_hit());
    match engine::mode() {
        Mode::Recording => {
            if let Some(h) = hit {
                engine::stop_recording(engine::StopBy::Menu, &h);
            }
        }
        _ => engine::start_recording(),
    }
}

pub fn toggle_play_from_ui() {
    play_from_ui(false);
}

pub fn toggle_loop_from_ui() {
    play_from_ui(true);
}

fn play_from_ui(repeat: bool) {
    if let Some(h) = with(|a| a.tray_hit()) {
        engine::play_toggle(repeat, engine::StopBy::Menu, &h);
    }
}

pub fn quit() {
    engine::shutdown();
    ui::close();
    APP.with(|a| a.borrow_mut().take());
    std::process::exit(0);
}

fn build_menu() -> (Menu, Items) {
    let items = Items {
        status: MenuItem::new("Ready", false, None),
        record: MenuItem::new("Start recording", true, None),
        play: MenuItem::new("Play once", true, None),
        repeat: MenuItem::new("Loop", true, None),
        recordings: MenuItem::new("Recordings...", true, None),
        random: CheckMenuItem::new("Random timing", true, false, None),
        stop_on_input: CheckMenuItem::new("Stop on mouse or key input", true, true, None),
        skip_moves: CheckMenuItem::new("Ignore mouse movement", true, false, None),
        settings: MenuItem::new("Settings...", true, None),
        autostart: CheckMenuItem::new("Start at login", true, true, None),
        update: MenuItem::new("Check for updates", true, None),
        star: MenuItem::new("Star on GitHub", true, None),
        quit: MenuItem::new(format!("Quit {APP_NAME}"), true, None),
    };
    let menu = Menu::new();
    let sep = || PredefinedMenuItem::separator();
    let _ = menu.append_items(&[
        &items.status,
        &sep(),
        &items.record,
        &items.play,
        &items.repeat,
        &items.recordings,
        &items.random,
        &items.stop_on_input,
        &items.skip_moves,
        &sep(),
        &items.settings,
        &items.autostart,
        &items.update,
        &items.star,
        &sep(),
        &items.quit,
    ]);
    (menu, items)
}

fn on_menu(e: MenuEvent) {
    let id = e.id;
    let Some(target) = with(|a| {
        let i = &a.items;
        [
            (i.record.id().clone(), 1),
            (i.play.id().clone(), 2),
            (i.repeat.id().clone(), 8),
            (i.recordings.id().clone(), 10),
            (i.random.id().clone(), 9),
            (i.stop_on_input.id().clone(), 11),
            (i.skip_moves.id().clone(), 12),
            (i.settings.id().clone(), 3),
            (i.autostart.id().clone(), 4),
            (i.update.id().clone(), 5),
            (i.star.id().clone(), 6),
            (i.quit.id().clone(), 7),
        ]
        .into_iter()
        .find(|(m, _)| *m == id)
        .map(|(_, n)| n)
    })
    .flatten() else {
        return;
    };
    match target {
        1 => toggle_record_from_ui(),
        2 => toggle_play_from_ui(),
        8 => toggle_loop_from_ui(),
        10 => toggle_panel(),
        9 => {
            let on = with(|a| !a.settings.random_timing).unwrap_or(false);
            set_random_timing(on);
        }
        11 => {
            let on = with(|a| !a.settings.stop_on_input).unwrap_or(true);
            set_stop_on_input(on);
        }
        12 => {
            let on = with(|a| !a.settings.skip_moves).unwrap_or(false);
            set_skip_moves(on);
        }
        3 => ui::open(),
        4 => {
            let on = with(|a| !a.settings.autostart).unwrap_or(true);
            set_autostart(on);
        }
        5 => {
            ui::open();
            check_updates();
        }
        6 => open_repo(),
        _ => quit(),
    }
}

fn on_hotkey(e: GlobalHotKeyEvent) {
    if e.state() != HotKeyState::Pressed {
        return;
    }
    let step = with(|a| {
        a.hk_speed
            .iter()
            .find(|(h, _)| h.id() == e.id())
            .map(|(_, i)| *i)
    })
    .flatten();
    if let Some(step) = step {
        set_speed_step(step);
        // The chord that picked the speed must not count as taking back control.
        engine::speed_hotkey(with(|a| a.speed()).unwrap_or(1.0));
        return;
    }
    let Some((which, chord, hit)) = with(|a| {
        if a.last_hotkey.elapsed() < Duration::from_millis(250) {
            return None;
        }
        a.last_hotkey = Instant::now();
        let which = [
            (a.hk_record, Which::Record),
            (a.hk_play, Which::Play),
            (a.hk_loop, Which::Loop),
            (a.hk_history, Which::History),
        ]
        .into_iter()
        .find(|(h, _)| h.is_some_and(|h| h.id() == e.id()))?
        .1;
        Some((which, shortcuts::chord_len(a.combo(which)), a.tray_hit()))
    })
    .flatten() else {
        return;
    };
    let by = engine::StopBy::Hotkey(chord);
    match which {
        Which::Record => engine::record_hotkey(chord, &hit),
        Which::Play => engine::play_toggle(false, by, &hit),
        Which::Loop => engine::play_toggle(true, by, &hit),
        Which::History => toggle_panel(),
    }
}

/// Opens the recordings panel at startup; used for screenshots.
const PANEL_ARG: &str = "--recordings";

fn on_saved(id: String) {
    with(|a| {
        a.settings.active = id;
        config::save_settings(&a.settings);
    });
    refresh_panel();
}

// The recordings panel. It only opens while nothing records or plays.

fn panel_rows() -> Vec<ui::panel::Row> {
    let active = with(|a| a.settings.active.clone()).unwrap_or_default();
    history::Store::open()
        .map(|s| s.list())
        .unwrap_or_default()
        .into_iter()
        .map(|e| ui::panel::Row {
            detail: format!(
                "{}, {}, {} {}",
                history::local_label(e.created),
                length_text(e.duration_ms),
                e.actions,
                if e.actions == 1 { "action" } else { "actions" }
            ),
            active: e.id == active,
            id: e.id,
            name: e.name,
            starred: e.starred,
        })
        .collect()
}

fn length_text(ms: u64) -> String {
    if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {:02} s", ms / 60_000, ms / 1000 % 60)
    }
}

pub fn toggle_panel() {
    if ui::panel::is_open() {
        ui::panel::close();
    } else if engine::mode() == Mode::Idle {
        ui::panel::show(panel_rows());
    }
}

fn refresh_panel() {
    if ui::panel::is_open() {
        ui::panel::show(panel_rows());
    }
}

/// Makes a saved recording the one the play shortcuts use.
pub fn activate_recording(id: &str) {
    ui::panel::close();
    let Some(steps) = history::Store::open().ok().and_then(|s| s.steps(id)) else {
        return;
    };
    if !engine::set_recording(steps) {
        return;
    }
    with(|a| {
        a.settings.active = id.to_string();
        config::save_settings(&a.settings);
    });
    after_change();
}

pub fn rename_recording(id: &str, name: &str) {
    if let Ok(s) = history::Store::open() {
        s.rename(id, name);
    }
    refresh_panel();
}

pub fn star_recording(id: &str, on: bool) {
    if let Ok(s) = history::Store::open() {
        s.set_starred(id, on);
    }
    refresh_panel();
}

/// Deleting the active recording leaves nothing to play.
pub fn delete_recording(id: &str) {
    if !history::Store::open().is_ok_and(|s| s.delete(id)) {
        return;
    }
    let was_active = with(|a| a.settings.active == id).unwrap_or(false);
    if was_active && engine::set_recording(Vec::new()) {
        with(|a| {
            a.settings.active.clear();
            config::save_settings(&a.settings);
        });
        after_change();
    }
    refresh_panel();
}

fn on_update(o: update::Outcome) {
    let restart = with(|a| {
        a.update_text = match &o {
            update::Outcome::UpToDate => format!("You have the latest version ({VERSION})."),
            update::Outcome::Installed(v) => {
                format!("Version {v} is installed and starts in a moment.")
            }
            update::Outcome::Failed(e) => e.clone(),
            update::Outcome::Silent => String::new(),
        };
        if matches!(o, update::Outcome::Installed(_)) {
            a.restart_pending = true;
        }
        a.restart_pending && engine::mode() == Mode::Idle
    })
    .unwrap_or(false);
    ui::refresh();
    if restart {
        restart_now();
    }
}

fn restart_now() {
    engine::shutdown();
    ui::close();
    APP.with(|a| a.borrow_mut().take());
    update::relaunch();
}

fn startup() {
    let mut settings = config::load_settings();
    let steps = match history::Store::open() {
        Ok(store) => {
            if let Some(id) = config::dir()
                .ok()
                .and_then(|d| history::migrate(&store, &d.join("recording.json")))
            {
                settings.active = id;
                config::save_settings(&settings);
            }
            store.steps(&settings.active).unwrap_or_default()
        }
        Err(err) => {
            eprintln!("recordings: {err}");
            Vec::new()
        }
    };
    engine::init(
        steps,
        |err| send(UserEvent::Engine(err)),
        |steps| {
            std::thread::spawn(move || {
                match history::Store::open().and_then(|s| s.add(&steps, history::now_secs())) {
                    Ok(e) => send(UserEvent::Saved(e.id)),
                    Err(err) => eprintln!("recordings: save failed: {err}"),
                }
            });
        },
    );
    let (menu, items) = build_menu();
    let builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_NAME);
    #[cfg(windows)]
    let builder = builder.with_menu_on_left_click(false);
    let tray = builder.build().expect("tray icon");
    let manager = GlobalHotKeyManager::new().expect("hotkey manager");
    let first_run = !settings.welcomed;
    if first_run {
        settings.welcomed = true;
        config::save_settings(&settings);
    }
    if settings.autostart {
        let _ = autostart::set(true);
    }
    engine::set_random(settings.random_timing);
    engine::set_stop_on_input(settings.stop_on_input);
    engine::set_skip_moves(settings.skip_moves);
    engine::set_speed(speed::factor(settings.speed_step, settings.custom_speed));
    let auto_update = settings.auto_update;
    let mut app = App {
        settings,
        manager,
        hk_record: None,
        hk_play: None,
        hk_loop: None,
        hk_speed: Vec::new(),
        speed_error: None,
        hk_history: None,
        tray,
        items,
        last_hotkey: Instant::now() - Duration::from_secs(1),
        notice: None,
        capturing: None,
        update_text: String::new(),
        restart_pending: false,
        shown_mode: None,
        hud_mode: Mode::Idle,
        saved_until: None,
    };
    app.register_all();
    app.register_speed();
    // Without these permissions record and play cannot work, so the window
    // opens at startup (and after an auto-update) instead of failing on first use.
    #[cfg(target_os = "macos")]
    if input::missing_pane().is_some() {
        app.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: engine::permission_text(),
        });
    }
    let has_error = app.notice.is_some();
    app.refresh_tray();
    APP.with(|a| *a.borrow_mut() = Some(app));
    if first_run || has_error {
        ui::open();
    }
    if std::env::args().any(|a| a == PANEL_ARG) {
        toggle_panel();
    }
    if auto_update {
        update::schedule(|o| send(UserEvent::Update(o)));
    }
}

pub fn run() {
    let relaunched = std::env::args().any(|a| a == update::RELAUNCH_ARG);
    if !instance::claim(relaunched) {
        return;
    }
    update::cleanup();
    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }
    let _ = PROXY.set(Mutex::new(event_loop.create_proxy()));
    GlobalHotKeyEvent::set_event_handler(Some(|e| send(UserEvent::HotKey(e))));
    MenuEvent::set_event_handler(Some(|e| send(UserEvent::Menu(e))));
    TrayIconEvent::set_event_handler(Some(|e| send(UserEvent::Tray(e))));
    instance::listen(|| send(UserEvent::Show));

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::NewEvents(StartCause::Init) => startup(),
            Event::UserEvent(e) => match e {
                UserEvent::Engine(err) => {
                    let restart = with(|a| {
                        if let Some(text) = err.clone() {
                            a.notice = Some(Notice {
                                kind: NoticeKind::Error,
                                text,
                            });
                        }
                        a.restart_pending && engine::mode() == Mode::Idle
                    })
                    .unwrap_or(false);
                    if err.is_some() {
                        ui::open();
                    }
                    after_change();
                    if restart {
                        restart_now();
                    }
                }
                UserEvent::HotKey(e) => on_hotkey(e),
                UserEvent::Menu(e) => on_menu(e),
                UserEvent::Tray(TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }) => {
                    if cfg!(windows) {
                        ui::open();
                    }
                }
                UserEvent::Tray(_) => {}
                UserEvent::Show => ui::open(),
                UserEvent::Update(o) => on_update(o),
                UserEvent::Hud => update_hud(),
                UserEvent::Saved(id) => on_saved(id),
            },
            #[cfg(target_os = "macos")]
            Event::Reopen { .. } => ui::open(),
            _ => {}
        }
    });
}

/// One running copy per user. Starting the app again opens the settings
/// window of the copy that is already running.
mod instance {
    #[cfg(windows)]
    mod imp {
        use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::{
            CreateEventW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE,
        };

        fn name() -> Vec<u16> {
            "Local\\MouseKeysRecordReplay.Show"
                .encode_utf16()
                .chain(Some(0))
                .collect()
        }

        static mut EVENT: usize = 0;

        fn try_claim(signal: bool) -> bool {
            let n = name();
            unsafe {
                let h = CreateEventW(std::ptr::null(), 0, 0, n.as_ptr());
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    if signal {
                        let other = OpenEventW(EVENT_MODIFY_STATE, 0, n.as_ptr());
                        if !other.is_null() {
                            // Lets the running copy bring its window to the front.
                            windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(
                                windows_sys::Win32::UI::WindowsAndMessaging::ASFW_ANY,
                            );
                            SetEvent(other);
                            CloseHandle(other);
                        }
                    }
                    CloseHandle(h);
                    return false;
                }
                EVENT = h as usize;
            }
            true
        }

        pub fn claim(wait: bool) -> bool {
            super::retry(wait, || try_claim(!wait))
        }

        pub fn listen(show: impl Fn() + Send + 'static) {
            let h = unsafe { EVENT };
            std::thread::spawn(move || loop {
                if unsafe { WaitForSingleObject(h as _, INFINITE) } != 0 {
                    return;
                }
                show();
            });
        }
    }

    #[cfg(unix)]
    mod imp {
        use std::os::unix::net::{UnixListener, UnixStream};
        use std::sync::Mutex;

        static LISTENER: Mutex<Option<UnixListener>> = Mutex::new(None);

        pub fn claim(wait: bool) -> bool {
            // No folder for settings means no socket to share: run without the guard.
            let Ok(dir) = crate::config::private_dir() else {
                return true;
            };
            let path = dir.join("instance.sock");
            if !super::retry(wait, || UnixStream::connect(&path).is_err()) {
                return false;
            }
            let _ = std::fs::remove_file(&path);
            match UnixListener::bind(&path) {
                Ok(l) => {
                    *LISTENER.lock().unwrap() = Some(l);
                    true
                }
                Err(_) => true,
            }
        }

        pub fn listen(show: impl Fn() + Send + 'static) {
            let Some(l) = LISTENER.lock().unwrap().take() else {
                return;
            };
            std::thread::spawn(move || {
                for s in l.incoming() {
                    if s.is_ok() {
                        show();
                    }
                }
            });
        }
    }

    /// After an update the new copy starts while the old one is still
    /// closing, so it keeps trying for a few seconds.
    fn retry(wait: bool, mut f: impl FnMut() -> bool) -> bool {
        let tries = if wait { 100 } else { 1 };
        for i in 0..tries {
            if f() {
                return true;
            }
            if i + 1 < tries {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        false
    }

    pub use imp::{claim, listen};
}

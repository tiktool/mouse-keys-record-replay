//! GTK 3 settings window. tao runs GTK on the main thread, so widgets are
//! built and updated directly. Signal handlers never hold a borrow of `UI`
//! because every `app::*` call ends in `refresh()`.

use crate::app::{self, NoticeKind, View};
use crate::config::APP_NAME;
use crate::engine::Mode;
use crate::icons;
use crate::shortcuts::{Mods, Which};
use crate::speed;
use gtk::gdk::{self, ModifierType};
use gtk::gdk_pixbuf::{Colorspace, Pixbuf};
use gtk::glib::{self, Propagation};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};

const CSS: &str = "
.mkrr-title { font-weight: bold; }
.mkrr-heading { font-weight: bold; }
.mkrr-small { font-size: smaller; }
.mkrr-error { color: #FF3B30; }
.mkrr-success { color: #30A46C; }
";

struct Ui {
    window: gtk::Window,
    header: gtk::HeaderBar,
    dot: gtk::DrawingArea,
    title: gtk::Label,
    detail: gtk::Label,
    record: gtk::Button,
    play: gtk::Button,
    repeat: gtk::Button,
    speed: gtk::Scale,
    speed_value: gtk::Label,
    custom: gtk::Entry,
    speed_hint: gtk::Label,
    history: gtk::Button,
    notice: gtk::Label,
    autostart: gtk::Switch,
    auto_update: gtk::Switch,
    random: gtk::Switch,
    stop_on_input: gtk::Switch,
    skip_moves: gtk::Switch,
    tip: gtk::Label,
    update_text: gtk::Label,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    // Set while refresh() writes widget state, so switch handlers stay quiet.
    static UPDATING: Cell<bool> = const { Cell::new(false) };
    static CAPTURING: Cell<Option<Which>> = const { Cell::new(None) };
    static MODE: Cell<Mode> = const { Cell::new(Mode::Idle) };
}

pub fn open() {
    let existing = UI.with(|u| u.borrow().as_ref().map(|ui| ui.window.clone()));
    if let Some(w) = existing {
        w.present();
        refresh();
        return;
    }
    let ui = build();
    let window = ui.window.clone();
    UI.with(|u| *u.borrow_mut() = Some(ui));
    window.show_all();
    refresh();
    window.present();
}

pub fn refresh() {
    let Some(v) = app::view() else { return };
    UI.with(|u| {
        if let Some(ui) = u.borrow().as_ref() {
            UPDATING.with(|f| f.set(true));
            apply(ui, &v);
            UPDATING.with(|f| f.set(false));
        }
    });
}

pub fn close() {
    CAPTURING.with(|c| c.set(None));
    if let Some(ui) = UI.with(|u| u.borrow_mut().take()) {
        ui.window.hide();
        unsafe { ui.window.destroy() };
    }
}

fn apply(ui: &Ui, v: &View) {
    CAPTURING.with(|c| c.set(v.capturing));
    MODE.with(|m| m.set(v.mode));
    ui.header
        .set_subtitle(Some(&format!("Version {}", v.version)));
    ui.dot.queue_draw();
    ui.title.set_text(&v.status_title);
    ui.detail.set_text(&v.status_detail);
    ui.detail.set_visible(!v.status_detail.is_empty());

    let waiting = "Press new shortcut...";
    ui.record.set_label(if v.capturing == Some(Which::Record) {
        waiting
    } else {
        &v.record
    });
    ui.play.set_label(if v.capturing == Some(Which::Play) {
        waiting
    } else {
        &v.play
    });
    ui.repeat.set_label(if v.capturing == Some(Which::Loop) {
        waiting
    } else {
        &v.repeat
    });
    ui.history
        .set_label(if v.capturing == Some(Which::History) {
            waiting
        } else {
            &v.history
        });

    let step = v.speed_step.min(speed::CUSTOM);
    if ui.speed.value().round() as usize != step {
        ui.speed.set_value(step as f64);
    }
    ui.speed_value
        .set_text(&speed::step_label(step, v.custom_speed));
    let custom = step == speed::CUSTOM;
    ui.custom.set_sensitive(custom);
    // Never overwrite what is being typed.
    if !ui.custom.has_focus() {
        ui.custom.set_text(&speed::custom_text(v.custom_speed));
    }
    let hint_ctx = ui.speed_hint.style_context();
    match &v.speed_error {
        Some(e) => {
            hint_ctx.add_class("mkrr-error");
            ui.speed_hint.set_text(e);
        }
        None => {
            hint_ctx.remove_class("mkrr-error");
            let hint = if custom {
                "Type 0.1 to 1000, then press Enter."
            } else {
                "Changes apply at once, also mid-playback."
            };
            ui.speed_hint.set_text(hint);
        }
    }

    let ctx = ui.notice.style_context();
    for c in ["mkrr-error", "mkrr-success", "dim-label"] {
        ctx.remove_class(c);
    }
    match &v.notice {
        Some(n) => {
            ctx.add_class(match n.kind {
                NoticeKind::Error => "mkrr-error",
                NoticeKind::Success => "mkrr-success",
            });
            ui.notice.set_text(&n.text);
            ui.notice.set_visible(true);
        }
        None => {
            ui.notice.set_text("");
            ui.notice.set_visible(false);
        }
    }

    if ui.autostart.is_active() != v.autostart {
        ui.autostart.set_active(v.autostart);
    }
    if ui.auto_update.is_active() != v.auto_update {
        ui.auto_update.set_active(v.auto_update);
    }
    if ui.random.is_active() != v.random_timing {
        ui.random.set_active(v.random_timing);
    }
    if ui.stop_on_input.is_active() != v.stop_on_input {
        ui.stop_on_input.set_active(v.stop_on_input);
    }
    if ui.skip_moves.is_active() != v.skip_moves {
        ui.skip_moves.set_active(v.skip_moves);
    }
    ui.tip.set_text(&v.stop_hint);

    ui.update_text.set_text(&v.update_text);
    ui.update_text.set_visible(!v.update_text.is_empty());
}

fn build() -> Ui {
    let provider = gtk::CssProvider::new();
    let _ = provider.load_from_data(CSS.as_bytes());
    if let Some(screen) = gdk::Screen::default() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title(APP_NAME);
    window.set_default_size(460, -1);
    window.set_resizable(false);
    window.set_position(gtk::WindowPosition::Center);
    window.set_icon(Some(&window_icon()));

    let header = gtk::HeaderBar::new();
    header.set_title(Some(APP_NAME));
    header.set_show_close_button(true);
    window.set_titlebar(Some(&header));

    let root = gtk::Box::new(gtk::Orientation::Vertical, 16);
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);
    root.set_size_request(412, -1);
    window.add(&root);

    let frame = gtk::Frame::new(None);
    let status = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    status.set_margin_top(12);
    status.set_margin_bottom(12);
    status.set_margin_start(12);
    status.set_margin_end(12);
    let dot = gtk::DrawingArea::new();
    dot.set_size_request(12, 12);
    dot.set_valign(gtk::Align::Start);
    dot.set_margin_top(4);
    dot.connect_draw(|da, cr| {
        let (r, g, b) = match MODE.with(|m| m.get()) {
            Mode::Idle => (0.56, 0.56, 0.58),
            Mode::Recording => (1.0, 0x3B as f64 / 255.0, 0x30 as f64 / 255.0),
            Mode::Playing => (0x0A as f64 / 255.0, 0x84 as f64 / 255.0, 1.0),
        };
        let w = da.allocated_width() as f64;
        let h = da.allocated_height() as f64;
        cr.set_source_rgb(r, g, b);
        cr.arc(w / 2.0, h / 2.0, w.min(h) / 2.0, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
        Propagation::Stop
    });
    let text = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let title = label("", &["mkrr-title"]);
    let detail = label("", &["dim-label"]);
    text.pack_start(&title, false, false, 0);
    text.pack_start(&detail, false, false, 0);
    status.pack_start(&dot, false, false, 0);
    status.pack_start(&text, true, true, 0);
    frame.add(&status);
    root.pack_start(&frame, false, false, 0);

    let section = gtk::Box::new(gtk::Orientation::Vertical, 8);
    section.pack_start(&label("Shortcuts", &["mkrr-heading"]), false, false, 0);
    let record = gtk::Button::with_label("");
    let play = gtk::Button::with_label("");
    let repeat = gtk::Button::with_label("");
    let history = gtk::Button::with_label("");
    section.pack_start(&row("Record / stop recording", &record), false, false, 0);
    section.pack_start(&row("Play once / stop playback", &play), false, false, 0);
    section.pack_start(&row("Loop / stop loop", &repeat), false, false, 0);
    section.pack_start(&row("Open recordings", &history), false, false, 0);
    // The speed shortcuts are fixed, so they are listed as text.
    let keys: Vec<String> = app::view().map(|v| v.speed_keys).unwrap_or_default();
    let first = keys.first().cloned().unwrap_or_default();
    let last = keys
        .last()
        .and_then(|k| k.rsplit(' ').next())
        .unwrap_or_default()
        .to_string();
    section.pack_start(
        &row(
            "Change speed",
            &label(&format!("{first} to {last}"), &["dim-label"]),
        ),
        false,
        false,
        0,
    );
    root.pack_start(&section, false, false, 0);

    // Playback speed, right under the record and play shortcuts.
    let speed_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let speed_value = label("", &["mkrr-heading"]);
    speed_box.pack_start(&row("Playback speed", &speed_value), false, false, 0);
    let slider =
        gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, speed::CUSTOM as f64, 1.0);
    slider.set_draw_value(false);
    slider.set_digits(0);
    slider.set_round_digits(0);
    slider.set_value(speed::DEFAULT_STEP as f64);
    for (i, l) in speed::LABELS.iter().enumerate() {
        slider.add_mark(i as f64, gtk::PositionType::Bottom, Some(l));
    }
    let custom = gtk::Entry::new();
    custom.set_width_chars(7);
    custom.set_placeholder_text(Some("100"));
    custom.set_valign(gtk::Align::Start);
    let slider_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    slider_row.pack_start(&slider, true, true, 0);
    slider_row.pack_start(&custom, false, false, 0);
    let custom_x = label("x", &["dim-label"]);
    custom_x.set_valign(gtk::Align::Start);
    custom_x.set_margin_top(6);
    slider_row.pack_start(&custom_x, false, false, 0);
    speed_box.pack_start(&slider_row, false, false, 0);
    let speed_hint = label("", &["dim-label", "mkrr-small"]);
    speed_box.pack_start(&speed_hint, false, false, 0);
    root.pack_start(&speed_box, false, false, 0);
    slider.connect_value_changed(|s| {
        if UPDATING.with(|f| f.get()) {
            return;
        }
        let step = (s.value().round().max(0.0) as usize).min(speed::CUSTOM);
        if app::view().is_some_and(|v| v.speed_step != step) {
            app::set_speed_step(step);
        }
    });
    let commit = |e: &gtk::Entry| {
        if app::view().is_some_and(|v| v.speed_step != speed::CUSTOM) {
            app::set_speed_step(speed::CUSTOM);
        }
        let _ = app::set_custom_speed(&e.text());
    };
    custom.connect_activate(move |e| commit(e));
    custom.connect_focus_out_event(move |e, _| {
        commit(e);
        Propagation::Proceed
    });
    for (button, which) in [
        (&record, Which::Record),
        (&play, Which::Play),
        (&repeat, Which::Loop),
        (&history, Which::History),
    ] {
        button.connect_clicked(move |b| {
            app::begin_capture(which);
            if let Some(w) = b.toplevel().and_then(|t| t.downcast::<gtk::Window>().ok()) {
                w.present();
            }
        });
    }

    let notice = label("", &[]);
    notice.set_no_show_all(true);
    root.pack_start(&notice, false, false, 0);

    let tip = label("", &["dim-label"]);
    root.pack_start(&tip, false, false, 0);

    let toggles = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let autostart = gtk::Switch::new();
    let auto_update = gtk::Switch::new();
    let random = gtk::Switch::new();
    let stop_on_input = gtk::Switch::new();
    let skip_moves = gtk::Switch::new();
    toggles.pack_start(&row("Start when I sign in", &autostart), false, false, 0);
    toggles.pack_start(
        &row("Install updates automatically", &auto_update),
        false,
        false,
        0,
    );
    toggles.pack_start(
        &row(
            "Random timing (each press up to half a second early or late)",
            &random,
        ),
        false,
        false,
        0,
    );
    toggles.pack_start(
        &row("Stop on mouse or key input", &stop_on_input),
        false,
        false,
        0,
    );
    toggles.pack_start(
        &row(
            "Ignore mouse movement (clicks and keys play at the pointer)",
            &skip_moves,
        ),
        false,
        false,
        0,
    );
    root.pack_start(&toggles, false, false, 0);
    autostart.connect_state_set(|_, on| {
        if !UPDATING.with(|f| f.get()) {
            app::set_autostart(on);
        }
        Propagation::Proceed
    });
    auto_update.connect_state_set(|_, on| {
        if !UPDATING.with(|f| f.get()) {
            app::set_auto_update(on);
        }
        Propagation::Proceed
    });
    random.connect_state_set(|_, on| {
        if !UPDATING.with(|f| f.get()) {
            app::set_random_timing(on);
        }
        Propagation::Proceed
    });
    stop_on_input.connect_state_set(|_, on| {
        if !UPDATING.with(|f| f.get()) {
            app::set_stop_on_input(on);
        }
        Propagation::Proceed
    });
    skip_moves.connect_state_set(|_, on| {
        if !UPDATING.with(|f| f.get()) {
            app::set_skip_moves(on);
        }
        Propagation::Proceed
    });

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let reset = gtk::Button::with_label("Reset to defaults");
    let check = gtk::Button::with_label("Check for updates");
    let star = gtk::Button::from_icon_name(Some("starred-symbolic"), gtk::IconSize::Button);
    star.set_label("Star on GitHub");
    star.set_always_show_image(true);
    reset.connect_clicked(|_| app::reset_shortcuts());
    check.connect_clicked(|_| app::check_updates());
    star.connect_clicked(|_| app::open_repo());
    footer.pack_start(&reset, false, false, 0);
    footer.pack_start(&check, false, false, 0);
    footer.pack_end(&star, false, false, 0);
    root.pack_start(&footer, false, false, 0);

    let update_text = label("", &["dim-label", "mkrr-small"]);
    update_text.set_no_show_all(true);
    root.pack_start(&update_text, false, false, 0);

    window.connect_delete_event(|w, _| {
        if CAPTURING.with(|c| c.get()).is_some() {
            app::cancel_capture();
        }
        w.hide();
        Propagation::Stop
    });
    window.connect_key_press_event(|_, ev| on_key(ev));
    window.connect_focus_out_event(|_, _| {
        if CAPTURING.with(|c| c.get()).is_some() {
            app::cancel_capture();
        }
        Propagation::Proceed
    });

    Ui {
        window,
        header,
        dot,
        title,
        detail,
        record,
        play,
        repeat,
        history,
        speed: slider,
        speed_value,
        custom,
        speed_hint,
        notice,
        autostart,
        auto_update,
        random,
        stop_on_input,
        skip_moves,
        tip,
        update_text,
    }
}

fn on_key(ev: &gdk::EventKey) -> Propagation {
    let Some(which) = CAPTURING.with(|c| c.get()) else {
        return Propagation::Proceed;
    };
    let state = ev.state();
    let mods = Mods {
        ctrl: state.contains(ModifierType::CONTROL_MASK),
        alt: state.contains(ModifierType::MOD1_MASK),
        shift: state.contains(ModifierType::SHIFT_MASK),
        sup: state.intersects(ModifierType::SUPER_MASK | ModifierType::MOD4_MASK),
    };
    // X11 and GDK Wayland keycodes are evdev codes plus 8.
    let evdev = u32::from(ev.hardware_keycode()).wrapping_sub(8);
    if evdev == 1 && mods == Mods::default() {
        app::cancel_capture();
        return Propagation::Stop;
    }
    if let Some(code) = evdev_code(evdev) {
        let _ = app::set_shortcut(which, mods, &code);
    }
    Propagation::Stop
}

/// W3C `KeyboardEvent.code` for a Linux evdev key code. Modifiers and keys
/// without a stable name return None.
fn evdev_code(c: u32) -> Option<String> {
    const LETTERS: [(u32, char); 26] = [
        (30, 'A'),
        (48, 'B'),
        (46, 'C'),
        (32, 'D'),
        (18, 'E'),
        (33, 'F'),
        (34, 'G'),
        (35, 'H'),
        (23, 'I'),
        (36, 'J'),
        (37, 'K'),
        (38, 'L'),
        (50, 'M'),
        (49, 'N'),
        (24, 'O'),
        (25, 'P'),
        (16, 'Q'),
        (19, 'R'),
        (31, 'S'),
        (20, 'T'),
        (22, 'U'),
        (47, 'V'),
        (17, 'W'),
        (45, 'X'),
        (21, 'Y'),
        (44, 'Z'),
    ];
    if let Some((_, l)) = LETTERS.iter().find(|(k, _)| *k == c) {
        return Some(format!("Key{l}"));
    }
    let s = match c {
        2..=10 => return Some(format!("Digit{}", c - 1)),
        11 => "Digit0",
        59..=68 => return Some(format!("F{}", c - 58)),
        87 => "F11",
        88 => "F12",
        183..=194 => return Some(format!("F{}", c - 170)),
        71 => "Numpad7",
        72 => "Numpad8",
        73 => "Numpad9",
        75 => "Numpad4",
        76 => "Numpad5",
        77 => "Numpad6",
        79 => "Numpad1",
        80 => "Numpad2",
        81 => "Numpad3",
        82 => "Numpad0",
        1 => "Escape",
        12 => "Minus",
        13 => "Equal",
        14 => "Backspace",
        15 => "Tab",
        26 => "BracketLeft",
        27 => "BracketRight",
        28 => "Enter",
        39 => "Semicolon",
        40 => "Quote",
        41 => "Backquote",
        43 => "Backslash",
        51 => "Comma",
        52 => "Period",
        53 => "Slash",
        57 => "Space",
        102 => "Home",
        103 => "ArrowUp",
        104 => "PageUp",
        105 => "ArrowLeft",
        106 => "ArrowRight",
        107 => "End",
        108 => "ArrowDown",
        109 => "PageDown",
        110 => "Insert",
        111 => "Delete",
        _ => return None,
    };
    Some(s.into())
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_line_wrap(true);
    l.set_max_width_chars(52);
    let ctx = l.style_context();
    for c in classes {
        ctx.add_class(c);
    }
    l
}

fn row(text: &str, control: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let l = label(text, &[]);
    b.pack_start(&l, true, true, 0);
    control.set_valign(gtk::Align::Center);
    b.pack_end(control, false, false, 0);
    b
}

fn window_icon() -> Pixbuf {
    const SIZE: i32 = 64;
    let rgba = icons::tray(Mode::Idle, SIZE as u32, [0x0A, 0x84, 0xFF], false);
    let bytes = glib::Bytes::from_owned(rgba);
    Pixbuf::from_bytes(&bytes, Colorspace::Rgb, true, 8, SIZE, SIZE, SIZE * 4)
}

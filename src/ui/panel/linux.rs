//! Recordings panel for Linux: an undecorated GTK window styled like the
//! status card, rebuilt from the rows on every change. It closes when it
//! loses focus.

use super::Row;
use crate::app;
use gtk::cairo;
use gtk::gdk;
use gtk::glib::Propagation;
use gtk::prelude::*;
use std::cell::RefCell;
use std::f64::consts::PI;

const W: i32 = 420;
const ROW_H: i32 = 54;
const TOP_GAP: i32 = 12;
const MAX_ROWS: usize = 7;
const NAME_MAX: i32 = 80;
const ICON: i32 = 20;

const GOLD: (f64, f64, f64) = (0.961, 0.706, 0.0);

const CSS: &str = "
.mkrr-panel { border-radius: 14px; padding: 4px 8px 8px 8px; }
.mkrr-panel-title { font-weight: bold; font-size: 13pt; }
.mkrr-panel-count, .mkrr-panel-detail { font-size: 9pt; }
.mkrr-panel-name { font-weight: 600; }
.mkrr-panel-pill { font-size: 8pt; font-weight: 600; color: #FFFFFF; background-color: #0A84FF; border-radius: 9px; padding: 1px 8px; }
.mkrr-panel button.flat { padding: 4px; min-height: 0; min-width: 0; }
.mkrr-panel.dark { background-color: #262626; border: 1px solid #3A3A3A; }
.mkrr-panel.dark .mkrr-panel-title, .mkrr-panel.dark .mkrr-panel-name { color: #FFFFFF; }
.mkrr-panel.dark .mkrr-panel-count, .mkrr-panel.dark .mkrr-panel-detail { color: #B0B0B0; }
.mkrr-panel.light { background-color: #FFFFFF; border: 1px solid #DDDDDD; }
.mkrr-panel.light .mkrr-panel-title, .mkrr-panel.light .mkrr-panel-name { color: #1A1A1A; }
.mkrr-panel.light .mkrr-panel-count, .mkrr-panel.light .mkrr-panel-detail { color: #5F5F5F; }
";

struct State {
    window: gtk::Window,
    list: gtk::Box,
    scroll: gtk::ScrolledWindow,
    count: gtk::Label,
    dark: bool,
    rows: Vec<Row>,
    confirm: Option<usize>,
    editing: Option<(usize, Option<gtk::Entry>)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static STYLED: RefCell<bool> = const { RefCell::new(false) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn is_open() -> bool {
    STATE.with(|s| s.try_borrow().map(|b| b.is_some()).unwrap_or(true))
}

pub fn show(rows: Vec<Row>) {
    let mut rows = Some(rows);
    let updated = with(|s| {
        s.rows = rows.take().unwrap_or_default();
        s.confirm = None;
        s.editing = None;
    });
    if updated.is_none() {
        let Some(state) = create(rows.take().unwrap_or_default()) else {
            return;
        };
        STATE.with(|s| *s.borrow_mut() = Some(state));
    }
    rebuild();
    let window = with(|s| s.window.clone());
    if let Some(w) = window {
        if updated.is_none() {
            w.show_all();
            w.present();
        }
    }
}

pub fn close() {
    let state = STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.take()));
    if let Some(s) = state {
        s.window.close();
    }
}

fn style() {
    if STYLED.with(|s| s.replace(true)) {
        return;
    }
    let Some(screen) = gdk::Screen::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    if provider.load_from_data(CSS.as_bytes()).is_ok() {
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Reads the background the active theme paints, as the status card does.
fn dark() -> bool {
    let probe = gtk::Window::new(gtk::WindowType::Popup);
    let bg = probe.style_context().lookup_color("theme_bg_color");
    probe.close();
    if let Some(c) = bg {
        return 0.2126 * c.red() + 0.7152 * c.green() + 0.0722 * c.blue() < 0.5;
    }
    let Some(settings) = gtk::Settings::default() else {
        return false;
    };
    let theme = settings
        .gtk_theme_name()
        .map(|t| t.to_lowercase())
        .unwrap_or_default();
    settings.is_gtk_application_prefer_dark_theme() || theme.contains("dark")
}

/// The work area of the monitor under the pointer.
fn workarea() -> Option<gdk::Rectangle> {
    let display = gdk::Display::default()?;
    let pointer = display.default_seat().and_then(|s| s.pointer());
    let monitor = match pointer {
        Some(p) => {
            let (_, x, y) = p.position();
            display.monitor_at_point(x, y)
        }
        None => display.primary_monitor(),
    }
    .or_else(|| display.monitor(0))?;
    Some(monitor.workarea())
}

fn create(rows: Vec<Row>) -> Option<State> {
    style();
    let dark = dark();
    let area = workarea()?;
    let w = gtk::Window::new(gtk::WindowType::Toplevel);
    w.set_type_hint(gdk::WindowTypeHint::Utility);
    w.set_decorated(false);
    w.set_resizable(false);
    w.set_skip_taskbar_hint(true);
    w.set_skip_pager_hint(true);
    w.set_keep_above(true);
    w.set_title("Recordings");
    w.move_(area.x() + (area.width() - W) / 2, area.y() + TOP_GAP);
    w.connect_key_press_event(|_, e| {
        if e.keyval() == gdk::keys::constants::Escape {
            escape();
            Propagation::Stop
        } else {
            Propagation::Proceed
        }
    });
    w.connect_focus_out_event(|_, _| {
        commit_edit();
        close();
        Propagation::Proceed
    });

    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let ctx = card.style_context();
    ctx.add_class("mkrr-panel");
    ctx.add_class(if dark { "dark" } else { "light" });

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.set_size_request(-1, 44);
    head.set_margin_start(12);
    head.set_margin_end(12);
    let title = gtk::Label::new(Some("Recordings"));
    title.style_context().add_class("mkrr-panel-title");
    head.pack_start(&title, false, false, 0);
    let count = gtk::Label::new(None);
    count.style_context().add_class("mkrr-panel-count");
    head.pack_end(&count, false, false, 0);
    card.pack_start(&head, false, false, 0);

    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scroll.add(&list);
    card.pack_start(&scroll, true, true, 0);

    card.set_size_request(W, -1);
    w.add(&card);
    Some(State {
        window: w,
        list,
        scroll,
        count,
        dark,
        rows,
        confirm: None,
        editing: None,
    })
}

fn rebuild() {
    let focus = with(|s| {
        for child in s.list.children() {
            s.list.remove(&child);
        }
        let n = s.rows.len();
        s.count.set_text(&if n > 0 {
            super::count_text(&s.rows)
        } else {
            String::new()
        });
        if n == 0 {
            let empty = gtk::Label::new(Some("No recordings yet."));
            empty.style_context().add_class("mkrr-panel-detail");
            empty.set_size_request(-1, ROW_H);
            s.list.pack_start(&empty, false, false, 0);
        }
        for i in 0..n {
            let row = row(s, i);
            s.list.pack_start(&row, false, false, 0);
        }
        let h = n.clamp(1, MAX_ROWS) as i32 * ROW_H;
        s.scroll.set_min_content_height(h);
        s.scroll.set_max_content_height(h);
        s.list.show_all();
        s.window.resize(W, 1);
        s.editing.as_ref().and_then(|(_, e)| e.clone())
    })
    .flatten();
    if let Some(entry) = focus {
        entry.grab_focus();
    }
}

fn row(s: &mut State, i: usize) -> gtk::Widget {
    let r = s.rows[i].clone();
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    line.set_size_request(-1, ROW_H);
    line.set_margin_start(4);
    line.set_margin_end(4);

    if s.confirm == Some(i) {
        let ask = gtk::Label::new(Some("Delete this recording?"));
        ask.style_context().add_class("mkrr-panel-name");
        line.pack_start(&ask, false, false, 8);
        let cancel = gtk::Button::with_label("Cancel");
        cancel.set_valign(gtk::Align::Center);
        cancel.connect_clicked(|_| {
            with(|s| s.confirm = None);
            rebuild();
        });
        line.pack_end(&cancel, false, false, 0);
        let delete = gtk::Button::with_label("Delete");
        delete.set_valign(gtk::Align::Center);
        delete.style_context().add_class("destructive-action");
        let id = r.id.clone();
        delete.connect_clicked(move |_| app::delete_recording(&id));
        line.pack_end(&delete, false, false, 0);
        return line.upcast();
    }

    let dark = s.dark;
    let dim = if dark {
        (0.42, 0.42, 0.42)
    } else {
        (0.74, 0.74, 0.74)
    };
    let sub = if dark {
        (0.69, 0.69, 0.69)
    } else {
        (0.37, 0.37, 0.37)
    };
    let starred = r.starred;
    let star = icon_button(move |cr, cx, cy| {
        star_path(cr, cx, cy, 8.0);
        if starred {
            cr.set_source_rgb(GOLD.0, GOLD.1, GOLD.2);
            let _ = cr.fill();
        } else {
            cr.set_source_rgb(dim.0, dim.1, dim.2);
            cr.set_line_width(1.6);
            let _ = cr.stroke();
        }
    });
    let id = r.id.clone();
    star.connect_clicked(move |_| {
        if !commit_edit() {
            app::star_recording(&id, !starred);
        }
    });
    line.pack_start(&star, false, false, 0);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_valign(gtk::Align::Center);
    let editing = s.editing.as_ref().is_some_and(|(e, _)| *e == i);
    if editing {
        let entry = gtk::Entry::new();
        entry.set_text(&r.name);
        entry.set_max_length(NAME_MAX);
        entry.connect_activate(|_| {
            commit_edit();
        });
        text.pack_start(&entry, false, false, 0);
        s.editing = Some((i, Some(entry)));
    } else {
        let name = gtk::Label::new(Some(&r.name));
        name.set_halign(gtk::Align::Start);
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.style_context().add_class("mkrr-panel-name");
        text.pack_start(&name, false, false, 0);
    }
    let detail = gtk::Label::new(Some(&r.detail));
    detail.set_halign(gtk::Align::Start);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    detail.style_context().add_class("mkrr-panel-detail");
    text.pack_start(&detail, false, false, 0);

    let pick = gtk::EventBox::new();
    pick.add(&text);
    if !editing {
        let id = r.id.clone();
        pick.connect_button_release_event(move |_, e| {
            if e.button() == 1 && !commit_edit() {
                app::activate_recording(&id);
            }
            Propagation::Stop
        });
    }
    line.pack_start(&pick, true, true, 0);

    let trash = icon_button(move |cr, cx, cy| {
        cr.set_source_rgb(sub.0, sub.1, sub.2);
        cr.rectangle(cx - 6.5, cy - 5.5, 13.0, 1.6);
        cr.rectangle(cx - 2.5, cy - 7.5, 5.0, 2.0);
        cr.move_to(cx - 5.0, cy - 3.0);
        cr.line_to(cx + 5.0, cy - 3.0);
        cr.line_to(cx + 4.0, cy + 7.0);
        cr.line_to(cx - 4.0, cy + 7.0);
        cr.close_path();
        let _ = cr.fill();
    });
    let delete_at = i;
    trash.connect_clicked(move |_| {
        if !commit_edit() {
            with(|s| s.confirm = Some(delete_at));
            rebuild();
        }
    });
    line.pack_end(&trash, false, false, 0);

    let pencil = icon_button(move |cr, cx, cy| {
        cr.set_source_rgb(sub.0, sub.1, sub.2);
        cr.translate(cx, cy);
        cr.rotate(-PI / 4.0);
        cr.move_to(-7.5, 0.0);
        cr.line_to(-4.5, 2.0);
        cr.line_to(4.0, 2.0);
        cr.line_to(4.0, -2.0);
        cr.line_to(-4.5, -2.0);
        cr.close_path();
        cr.rectangle(5.4, -2.0, 2.1, 4.0);
        let _ = cr.fill();
    });
    let edit_at = i;
    pencil.connect_clicked(move |_| {
        if !commit_edit() {
            with(|s| {
                s.confirm = None;
                s.editing = Some((edit_at, None));
            });
            rebuild();
        }
    });
    line.pack_end(&pencil, false, false, 0);

    if r.active {
        let pill = gtk::Label::new(Some("Active"));
        pill.set_valign(gtk::Align::Center);
        pill.style_context().add_class("mkrr-panel-pill");
        line.pack_end(&pill, false, false, 0);
    }
    line.upcast()
}

/// Ends an open rename. Returns true when one was open, so the click that
/// ended it does nothing else.
fn commit_edit() -> bool {
    let Some(((i, entry), rows)) = with(|s| Some((s.editing.take()?, s.rows.clone()))).flatten()
    else {
        return false;
    };
    let name = entry
        .map(|e| e.text().trim().to_string())
        .unwrap_or_default();
    match rows.get(i) {
        Some(r) if !name.is_empty() && name != r.name => app::rename_recording(&r.id, &name),
        _ => rebuild(),
    }
    true
}

/// Esc leaves an open rename or delete question first, then closes.
fn escape() {
    let backed_out =
        with(|s| s.editing.take().is_some() | s.confirm.take().is_some()).unwrap_or(false);
    if backed_out {
        rebuild();
    } else {
        close();
    }
}

fn icon_button(draw: impl Fn(&cairo::Context, f64, f64) + 'static) -> gtk::Button {
    let area = gtk::DrawingArea::new();
    area.set_size_request(ICON, ICON);
    area.connect_draw(move |a, cr| {
        draw(
            cr,
            a.allocated_width() as f64 / 2.0,
            a.allocated_height() as f64 / 2.0,
        );
        Propagation::Stop
    });
    let b = gtk::Button::new();
    b.set_relief(gtk::ReliefStyle::None);
    b.set_valign(gtk::Align::Center);
    b.add(&area);
    b
}

fn star_path(cr: &cairo::Context, cx: f64, cy: f64, r: f64) {
    for k in 0..10 {
        let radius = if k % 2 == 0 { r } else { r * 0.42 };
        let a = -PI / 2.0 + k as f64 * PI / 5.0;
        let (x, y) = (cx + radius * a.cos(), cy + radius * a.sin());
        if k == 0 {
            cr.move_to(x, y);
        } else {
            cr.line_to(x, y);
        }
    }
    cr.close_path();
}

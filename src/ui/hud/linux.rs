use super::{Hud, HudKind};
use gtk::cairo;
use gtk::gdk;
use gtk::glib::{self, ControlFlow, Propagation};
use gtk::prelude::*;
use std::cell::RefCell;
use std::f64::consts::PI;
use std::time::{Duration, Instant};

const TOP_GAP: i32 = 12;
const SHADOW: i32 = 14;
const ICON: i32 = 32;
const FRAME: Duration = Duration::from_millis(30);
const FADE_IN: f64 = 0.18;
const FADE_OUT: f64 = 0.22;
const PULSE: f64 = 1.6;

const RED: (f64, f64, f64) = (1.0, 0.231, 0.188);
const ACCENT: (f64, f64, f64) = (0.039, 0.518, 1.0);

const CSS: &str = "
.mkrr-hud { border-radius: 14px; padding: 12px 22px 12px 14px; }
.mkrr-hud-title { font-weight: bold; font-size: 10pt; letter-spacing: 1px; }
.mkrr-hud-line { font-size: 9pt; }
.mkrr-hud-key { font-size: 9pt; font-weight: 600; border-radius: 5px; padding: 0 6px; margin: 0 2px; }
.mkrr-hud.dark { background-color: #262626; border: 1px solid #3A3A3A; box-shadow: 0 3px 12px rgba(0,0,0,0.45); }
.mkrr-hud.dark .mkrr-hud-title, .mkrr-hud.dark .mkrr-hud-key { color: #FFFFFF; }
.mkrr-hud.dark .mkrr-hud-line { color: #B0B0B0; }
.mkrr-hud.dark .mkrr-hud-key { background-color: #363636; border: 1px solid #474747; }
.mkrr-hud.light { background-color: #FFFFFF; border: 1px solid #DDDDDD; box-shadow: 0 3px 12px rgba(0,0,0,0.16); }
.mkrr-hud.light .mkrr-hud-title, .mkrr-hud.light .mkrr-hud-key { color: #1A1A1A; }
.mkrr-hud.light .mkrr-hud-line { color: #5F5F5F; }
.mkrr-hud.light .mkrr-hud-key { background-color: #F2F2F2; border: 1px solid #D8D8D8; }
";

struct State {
    hud: Option<Hud>,
    windows: Vec<gtk::Window>,
    visible: bool,
    alpha: f64,
    last: Instant,
    timer: Option<glib::SourceId>,
    styled: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        hud: None,
        windows: Vec::new(),
        visible: false,
        alpha: 0.0,
        last: Instant::now(),
        timer: None,
        styled: false,
    });
}

pub fn show(hud: Option<Hud>) {
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        match hud {
            Some(h) => {
                if s.visible && s.hud.as_ref() == Some(&h) {
                    return;
                }
                if !s.styled {
                    style();
                    s.styled = true;
                }
                for w in s.windows.drain(..) {
                    w.close();
                }
                if !s.visible {
                    s.alpha = 0.0;
                }
                s.windows = create(&h, Instant::now(), s.alpha);
                s.hud = Some(h);
                s.visible = true;
            }
            None => {
                if !s.visible {
                    return;
                }
                s.visible = false;
            }
        }
        s.last = Instant::now();
        if s.timer.is_none() {
            s.timer = Some(glib::timeout_add_local(FRAME, tick));
        }
    });
}

fn tick() -> ControlFlow {
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        let now = Instant::now();
        let dt = now.duration_since(s.last).as_secs_f64();
        s.last = now;
        s.alpha = if s.visible {
            (s.alpha + dt / FADE_IN).min(1.0)
        } else {
            (s.alpha - dt / FADE_OUT).max(0.0)
        };
        let ease = 1.0 - (1.0 - s.alpha).powi(3);
        let pulsing = s.hud.as_ref().is_some_and(|h| h.kind == HudKind::Recording);
        for w in &s.windows {
            w.set_opacity(ease);
            if pulsing {
                w.queue_draw();
            }
        }
        if !s.visible && s.alpha <= 0.0 {
            for w in s.windows.drain(..) {
                w.close();
            }
            s.hud = None;
            s.timer = None;
            return ControlFlow::Break;
        }
        if s.visible && s.alpha >= 1.0 && !pulsing {
            s.timer = None;
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    })
}

fn style() {
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

/// Reads the background the active theme paints, so GTK_THEME overrides and
/// dark theme variants count even when the theme name says nothing.
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

fn create(hud: &Hud, born: Instant, alpha: f64) -> Vec<gtk::Window> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let dark = dark();
    (0..display.n_monitors())
        .filter_map(|i| display.monitor(i))
        .map(|m| window(hud, &m.workarea(), dark, born, alpha))
        .collect()
}

fn window(hud: &Hud, area: &gdk::Rectangle, dark: bool, born: Instant, alpha: f64) -> gtk::Window {
    let w = gtk::Window::new(gtk::WindowType::Popup);
    w.set_type_hint(gdk::WindowTypeHint::Notification);
    w.set_skip_taskbar_hint(true);
    w.set_skip_pager_hint(true);
    w.set_accept_focus(false);
    w.set_focus_on_map(false);
    w.set_keep_above(true);
    w.set_decorated(false);
    w.set_app_paintable(true);
    if let Some(visual) = WidgetExt::screen(&w).and_then(|s| s.rgba_visual()) {
        w.set_visual(Some(&visual));
    }
    w.connect_draw(|_, cr| {
        cr.set_operator(cairo::Operator::Source);
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        cr.set_operator(cairo::Operator::Over);
        Propagation::Proceed
    });

    let card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    card.set_margin(SHADOW);
    let ctx = card.style_context();
    ctx.add_class("mkrr-hud");
    ctx.add_class(if dark { "dark" } else { "light" });

    let kind = hud.kind;
    let card_color = if dark {
        (0.149, 0.149, 0.149)
    } else {
        (1.0, 1.0, 1.0)
    };
    let green = if dark {
        (0.188, 0.820, 0.345)
    } else {
        (0.118, 0.620, 0.243)
    };
    let tint = if dark { 0.22 } else { 0.14 };
    let icon = gtk::DrawingArea::new();
    icon.set_size_request(ICON, ICON);
    icon.set_valign(gtk::Align::Center);
    icon.connect_draw(move |area, cr| {
        let accent = match kind {
            HudKind::Recording => RED,
            HudKind::Looping => ACCENT,
            HudKind::Saved => green,
        };
        let (cx, cy) = (
            area.allocated_width() as f64 / 2.0,
            area.allocated_height() as f64 / 2.0,
        );
        let phase = (born.elapsed().as_secs_f64() % PULSE) / PULSE;
        draw_icon(cr, kind, cx, cy, card_color, accent, tint, phase);
        Propagation::Stop
    });
    card.pack_start(&icon, false, false, 0);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_valign(gtk::Align::Center);
    let title = gtk::Label::new(Some(&hud.title));
    title.set_halign(gtk::Align::Start);
    title.style_context().add_class("mkrr-hud-title");
    text.pack_start(&title, false, false, 0);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    for (run, is_key) in &hud.line {
        let l = gtk::Label::new(Some(run));
        l.style_context().add_class(if *is_key {
            "mkrr-hud-key"
        } else {
            "mkrr-hud-line"
        });
        line.pack_start(&l, false, false, 0);
    }
    text.pack_start(&line, false, false, 0);
    card.pack_start(&text, false, false, 0);
    w.add(&card);

    card.show_all();
    let (_, natural) = card.preferred_size();
    let width = natural.width + 2 * SHADOW;
    let height = natural.height + 2 * SHADOW;
    w.set_default_size(width, height);
    w.move_(
        area.x() + (area.width() - width) / 2,
        area.y() + TOP_GAP - SHADOW,
    );
    w.set_opacity(1.0 - (1.0 - alpha).powi(3));
    w.connect_realize(|w| w.input_shape_combine_region(Some(&cairo::Region::create())));
    w.show_all();
    w
}

#[allow(clippy::too_many_arguments)]
fn draw_icon(
    cr: &cairo::Context,
    kind: HudKind,
    cx: f64,
    cy: f64,
    card: (f64, f64, f64),
    accent: (f64, f64, f64),
    tint: f64,
    phase: f64,
) {
    let mix = |a: f64, b: f64| a + (b - a) * tint;
    cr.set_source_rgb(
        mix(card.0, accent.0),
        mix(card.1, accent.1),
        mix(card.2, accent.2),
    );
    cr.arc(cx, cy, 16.0, 0.0, 2.0 * PI);
    let _ = cr.fill();
    let (r, g, b) = accent;
    match kind {
        HudKind::Recording => {
            cr.set_source_rgba(r, g, b, (1.0 - phase) * 0.45);
            cr.arc(cx, cy, 6.0 + 8.0 * phase, 0.0, 2.0 * PI);
            let _ = cr.fill();
            cr.set_source_rgb(r, g, b);
            cr.arc(cx, cy, 6.0, 0.0, 2.0 * PI);
            let _ = cr.fill();
        }
        HudKind::Looping => {
            cr.set_source_rgb(r, g, b);
            cr.set_line_width(2.0);
            cr.set_line_cap(cairo::LineCap::Round);
            let radius = 6.5;
            for start in [200.0f64, 20.0] {
                let sweep = 125.0;
                cr.new_sub_path();
                cr.arc(
                    cx,
                    cy,
                    radius,
                    start.to_radians(),
                    (start + sweep).to_radians(),
                );
                let _ = cr.stroke();
                let a = (start + sweep).to_radians();
                let (px, py) = (cx + radius * a.cos(), cy + radius * a.sin());
                let (dx, dy) = (-a.sin(), a.cos());
                let (nx, ny) = (a.cos(), a.sin());
                let s = 3.4;
                cr.move_to(px + dx * s, py + dy * s);
                cr.line_to(
                    px + nx * s * 0.95 - dx * s * 0.25,
                    py + ny * s * 0.95 - dy * s * 0.25,
                );
                cr.line_to(
                    px - nx * s * 0.95 - dx * s * 0.25,
                    py - ny * s * 0.95 - dy * s * 0.25,
                );
                cr.close_path();
                let _ = cr.fill();
            }
        }
        HudKind::Saved => {
            cr.set_source_rgb(r, g, b);
            cr.set_line_width(2.3);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.set_line_join(cairo::LineJoin::Round);
            cr.move_to(cx - 5.0, cy + 0.2);
            cr.line_to(cx - 1.5, cy + 3.6);
            cr.line_to(cx + 5.2, cy - 3.4);
            let _ = cr.stroke();
        }
    }
}

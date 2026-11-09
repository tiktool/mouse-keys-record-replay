//! Status card for Windows: one layered, click-through window per display.
//! Shapes are drawn with GDI+ and text with GDI ClearType into one buffer,
//! which the compositor blends with per-pixel alpha.

use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, EnumDisplayMonitors, GdiFlush, GetMonitorInfoW,
    GetTextExtentPoint32W, SelectObject, SetBkMode, SetTextCharacterExtra, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
    HDC, HFONT, HMONITOR, MONITORINFO, TRANSPARENT,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipAddPathArc, GdipClosePathFigure, GdipCreateBitmapFromScan0, GdipCreatePath, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush,
    GdipDeleteGraphics, GdipDeletePath, GdipDeletePen, GdipDisposeImage, GdipDrawArc, GdipDrawLines, GdipDrawPath, GdipFillEllipse,
    GdipFillPath, GdipFillPolygon, GdipGetImageGraphicsContext, GdipSetPenEndCap, GdipSetPenLineJoin, GdipSetPenStartCap,
    GdipSetPixelOffsetMode, GdipSetSmoothingMode, FillModeAlternate, GpBitmap, GpBrush, GpGraphics, GpImage, GpPath, GpPen, GpSolidFill,
    LineCapRound, LineJoinRound, PixelOffsetModeHalf, PointF, SmoothingModeAntiAlias, UnitPixel,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::{Hud, HudKind};
use crate::ui::windows::{apps_use_light_theme, argb, colorref, start_gdiplus, wide, ACCENT, RED};

const PIXEL_FORMAT_32BPP_PARGB: i32 = 0x000E_200B;
const TIMER_ID: usize = 1;
const FRAME_MS: u32 = 30;

/// Layout in logical pixels (96 dpi).
const MARGIN: f32 = 20.0;
const TOP_GAP: f32 = 16.0;
const CARD_H: f32 = 60.0;
const MIN_CARD_W: f32 = 240.0;
const RADIUS: f32 = 14.0;
const TEXT_X: f32 = 60.0;
const ICON_X: f32 = 30.0;

const FADE_IN_S: f32 = 0.18;
const FADE_OUT_S: f32 = 0.22;
const PULSE_S: f32 = 1.6;

struct Theme {
    card: u32,
    border: u32,
    text: u32,
    sub: u32,
    chip: u32,
    chip_border: u32,
    green: u32,
    tint: f32,
    shadow: f32,
}

const DARK: Theme =
    Theme { card: 0x262626, border: 0x3A3A3A, text: 0xFFFFFF, sub: 0xB0B0B0, chip: 0x363636, chip_border: 0x474747, green: 0x30D158, tint: 0.22, shadow: 0.03 };
const LIGHT: Theme =
    Theme { card: 0xFFFFFF, border: 0xDDDDDD, text: 0x1A1A1A, sub: 0x5F5F5F, chip: 0xF2F2F2, chip_border: 0xD8D8D8, green: 0x1E9E3E, tint: 0.14, shadow: 0.012 };

struct Fonts {
    title: HFONT,
    body: HFONT,
    key: HFONT,
}

impl Fonts {
    fn new(dpi: u32) -> Self {
        let face = wide("Segoe UI");
        let make = |px: i32, weight: i32| unsafe {
            CreateFontW(
                -(px * dpi as i32 / 96),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                face.as_ptr(),
            )
        };
        Fonts { title: make(13, 700), body: make(12, 400), key: make(12, 600) }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.title);
            DeleteObject(self.body);
            DeleteObject(self.key);
        }
    }
}

struct Card {
    hwnd: HWND,
    dpi: u32,
    work: RECT,
    fonts: Fonts,
    shown: bool,
}

impl Drop for Card {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

struct State {
    hud: Option<Hud>,
    visible: bool,
    alpha: f32,
    last: Instant,
    born: Instant,
    dark: bool,
    ticking: bool,
    cards: Vec<Card>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CLASS_READY: Cell<bool> = const { Cell::new(false) };
}

/// Shows `hud` on every display, or fades the card out for `None`.
pub fn show(hud: Option<Hud>) {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let s = slot.get_or_insert_with(|| State {
            hud: None,
            visible: false,
            alpha: 0.0,
            last: Instant::now(),
            born: Instant::now(),
            dark: false,
            ticking: false,
            cards: Vec::new(),
        });
        match hud {
            Some(h) => {
                if s.visible && s.hud.as_ref() == Some(&h) {
                    return;
                }
                if s.cards.is_empty() {
                    s.cards = create_cards();
                    s.alpha = 0.0;
                }
                if s.hud.as_ref().map(|o| o.kind) != Some(h.kind) {
                    s.born = Instant::now();
                }
                s.hud = Some(h);
                s.visible = true;
                s.dark = !apps_use_light_theme();            }
            None => {
                if !s.visible {
                    return;
                }
                s.visible = false;
            }
        }
        s.last = Instant::now();
        s.render();
        s.start_timer();
    });
}

impl State {
    fn start_timer(&mut self) {
        if let (false, Some(first)) = (self.ticking, self.cards.first()) {
            unsafe { SetTimer(first.hwnd, TIMER_ID, FRAME_MS, None) };
            self.ticking = true;
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32();
        self.last = now;
        self.alpha = if self.visible { (self.alpha + dt / FADE_IN_S).min(1.0) } else { (self.alpha - dt / FADE_OUT_S).max(0.0) };
        if !self.visible && self.alpha <= 0.0 {
            // Destroying the windows also stops their timer.
            self.cards.clear();
            self.hud = None;
            self.ticking = false;
            return;
        }
        self.render();
        let pulsing = self.visible && self.hud.as_ref().is_some_and(|h| h.kind == HudKind::Recording);
        if !pulsing && self.alpha >= 1.0 {
            if let Some(first) = self.cards.first() {
                unsafe { KillTimer(first.hwnd, TIMER_ID) };
            }
            self.ticking = false;
        }
    }

    fn render(&mut self) {
        let Some(hud) = &self.hud else { return };
        let theme = if self.dark { &DARK } else { &LIGHT };
        let ease = 1.0 - (1.0 - self.alpha).powi(3);
        let phase = (self.born.elapsed().as_secs_f32() / PULSE_S).fract();
        for card in &mut self.cards {
            draw(card, hud, theme, ease, phase);
        }
    }
}

fn class_name() -> Vec<u16> {
    wide("MouseKeysRecordReplay.Hud")
}

fn register_class() {
    if CLASS_READY.with(|c| c.replace(true)) {
        return;
    }
    let class = class_name();
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: null_mut(),
            hCursor: null_mut(),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTTRANSPARENT as LRESULT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_TIMER if wp == TIMER_ID => {
            STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut() {
                    if let Some(s) = slot.as_mut() {
                        s.tick();
                    }
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn collect_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    (*(data as *mut Vec<HMONITOR>)).push(monitor);
    1
}

fn create_cards() -> Vec<Card> {
    start_gdiplus();
    register_class();
    let mut monitors: Vec<HMONITOR> = Vec::new();
    let class = class_name();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(collect_monitor), &mut monitors as *mut _ as LPARAM);
        monitors
            .into_iter()
            .filter_map(|m| {
                let mut info: MONITORINFO = zeroed();
                info.cbSize = size_of::<MONITORINFO>() as u32;
                if GetMonitorInfoW(m, &mut info) == 0 {
                    return None;
                }
                let (mut dx, mut dy) = (96u32, 96u32);
                GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
                let work = info.rcWork;
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                    class.as_ptr(),
                    null(),
                    WS_POPUP,
                    work.left,
                    work.top,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    GetModuleHandleW(null()),
                    null(),
                );
                if hwnd.is_null() {
                    return None;
                }
                let dpi = dx.max(96);
                Some(Card { hwnd, dpi, work, fonts: Fonts::new(dpi), shown: false })
            })
            .collect()
    }
}

/// Mixes `c` toward `to` by `t` (0..1).
fn mix(c: u32, to: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let a = ((c >> s) & 0xFF) as f32;
        let b = ((to >> s) & 0xFF) as f32;
        ((a + (b - a) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

fn with_alpha(c: u32, a: f32) -> u32 {
    (((a.clamp(0.0, 1.0) * 255.0).round() as u32) << 24) | (c & 0xFF_FFFF)
}

fn extent(dc: HDC, font: HFONT, s: &[u16]) -> i32 {
    let mut size = SIZE { cx: 0, cy: 0 };
    unsafe {
        let old = SelectObject(dc, font);
        GetTextExtentPoint32W(dc, s.as_ptr(), s.len() as i32, &mut size);
        SelectObject(dc, old);
    }
    size.cx
}

struct Run {
    text: Vec<u16>,
    key: bool,
    rect: RECT,
}

/// Shapes drawn in device pixels on a GDI+ surface.
struct Shapes {
    g: *mut GpGraphics,
}

impl Shapes {
    unsafe fn round_path(&self, x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
        let d = (r * 2.0).min(h);
        let mut path: *mut GpPath = null_mut();
        GdipCreatePath(FillModeAlternate, &mut path);
        GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(path);
        path
    }

    unsafe fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) {
        let path = self.round_path(x, y, w, h, r);
        let mut brush: *mut GpSolidFill = null_mut();
        GdipCreateSolidFill(color, &mut brush);
        GdipFillPath(self.g, brush as *mut GpBrush, path);
        GdipDeleteBrush(brush as *mut GpBrush);
        GdipDeletePath(path);
    }

    unsafe fn stroke_round(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32, width: f32) {
        let i = width / 2.0;
        let path = self.round_path(x + i, y + i, w - width, h - width, r - i);
        let pen = self.pen(color, width);
        GdipDrawPath(self.g, pen, path);
        GdipDeletePen(pen);
        GdipDeletePath(path);
    }

    unsafe fn circle(&self, cx: f32, cy: f32, r: f32, color: u32) {
        let mut brush: *mut GpSolidFill = null_mut();
        GdipCreateSolidFill(color, &mut brush);
        GdipFillEllipse(self.g, brush as *mut GpBrush, cx - r, cy - r, 2.0 * r, 2.0 * r);
        GdipDeleteBrush(brush as *mut GpBrush);
    }

    unsafe fn pen(&self, color: u32, width: f32) -> *mut GpPen {
        let mut pen: *mut GpPen = null_mut();
        GdipCreatePen1(color, width, UnitPixel, &mut pen);
        GdipSetPenStartCap(pen, LineCapRound);
        GdipSetPenEndCap(pen, LineCapRound);
        GdipSetPenLineJoin(pen, LineJoinRound);
        pen
    }

    unsafe fn polygon(&self, pts: &[PointF], color: u32) {
        let mut brush: *mut GpSolidFill = null_mut();
        GdipCreateSolidFill(color, &mut brush);
        GdipFillPolygon(self.g, brush as *mut GpBrush, pts.as_ptr(), pts.len() as i32, FillModeAlternate);
        GdipDeleteBrush(brush as *mut GpBrush);
    }

    /// Two arcs chasing each other, each ending in an arrow head.
    unsafe fn loop_arrows(&self, cx: f32, cy: f32, k: f32, color: u32) {
        let r = 6.5 * k;
        let pen = self.pen(color, 2.0 * k);
        for start in [200.0f32, 20.0] {
            let sweep = 125.0;
            GdipDrawArc(self.g, pen, cx - r, cy - r, 2.0 * r, 2.0 * r, start, sweep);
            let a = (start + sweep).to_radians();
            let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
            let (dx, dy) = (-a.sin(), a.cos());
            let (nx, ny) = (a.cos(), a.sin());
            let s = 3.4 * k;
            let pts = [
                PointF { X: px + dx * s, Y: py + dy * s },
                PointF { X: px + nx * s * 0.95 - dx * s * 0.25, Y: py + ny * s * 0.95 - dy * s * 0.25 },
                PointF { X: px - nx * s * 0.95 - dx * s * 0.25, Y: py - ny * s * 0.95 - dy * s * 0.25 },
            ];
            self.polygon(&pts, color);
        }
        GdipDeletePen(pen);
    }

    unsafe fn check(&self, cx: f32, cy: f32, k: f32, color: u32) {
        let pts = [
            PointF { X: cx - 5.0 * k, Y: cy + 0.2 * k },
            PointF { X: cx - 1.5 * k, Y: cy + 3.6 * k },
            PointF { X: cx + 5.2 * k, Y: cy - 3.4 * k },
        ];
        let pen = self.pen(color, 2.3 * k);
        GdipDrawLines(self.g, pen, pts.as_ptr(), pts.len() as i32);
        GdipDeletePen(pen);
    }
}

fn draw(card: &mut Card, hud: &Hud, t: &Theme, ease: f32, phase: f32) {
    let k = card.dpi as f32 / 96.0;
    let p = |v: f32| (v * k).round() as i32;
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return;
        }
        let spacing = p(1.4);
        let title = wide(&hud.title);
        let title = &title[..title.len() - 1];
        SetTextCharacterExtra(dc, spacing);
        let title_w = extent(dc, card.fonts.title, title);
        SetTextCharacterExtra(dc, 0);

        let (pad, gap) = (p(6.0), p(2.0));
        let (line_y, line_h) = (p(MARGIN + 31.0), p(18.0));
        let mut x = p(MARGIN + TEXT_X);
        let mut runs: Vec<Run> = Vec::with_capacity(hud.line.len());
        for (s, key) in &hud.line {
            let mut text = wide(s);
            text.pop();
            let w = extent(dc, if *key { card.fonts.key } else { card.fonts.body }, &text);
            let (left, advance) = if *key { (x + gap + pad, w + 2 * (pad + gap)) } else { (x, w) };
            runs.push(Run { text, key: *key, rect: RECT { left, top: line_y, right: left + w, bottom: line_y + line_h } });
            x += advance;
        }
        let line_w = x - p(MARGIN + TEXT_X);

        let card_w = (p(TEXT_X) + title_w.max(line_w) + p(22.0)).max(p(MIN_CARD_W));
        let card_h = p(CARD_H);
        let m = p(MARGIN);
        let (w, h) = (card_w + 2 * m, card_h + 2 * m);

        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut core::ffi::c_void = null_mut();
        let dib = CreateDIBSection(dc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(dc);
            return;
        }
        let old_bmp = SelectObject(dc, dib);

        let mut bmp: *mut GpBitmap = null_mut();
        GdipCreateBitmapFromScan0(w, h, w * 4, PIXEL_FORMAT_32BPP_PARGB, bits as *const u8, &mut bmp);
        let mut g: *mut GpGraphics = null_mut();
        if !bmp.is_null() {
            GdipGetImageGraphicsContext(bmp as *mut GpImage, &mut g);
        }
        if !g.is_null() {
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
            let sh = Shapes { g };
            let (cx, cy, cw, ch, r) = (m as f32, m as f32, card_w as f32, card_h as f32, RADIUS * k);

            for i in 0..10 {
                let e = (i as f32 + 1.0) * 1.3 * k;
                sh.fill_round(cx - e, cy - e + 3.0 * k, cw + 2.0 * e, ch + 2.0 * e, r + e, with_alpha(0, t.shadow));
            }
            sh.fill_round(cx, cy, cw, ch, r, argb(t.card));
            sh.stroke_round(cx, cy, cw, ch, r, argb(t.border), k.max(1.0));

            let accent = match hud.kind {
                HudKind::Recording => RED,
                HudKind::Looping => ACCENT,
                HudKind::Saved => t.green,
            };
            let (ix, iy) = (cx + ICON_X * k, cy + ch / 2.0);
            sh.circle(ix, iy, 16.0 * k, argb(mix(t.card, accent, t.tint)));
            match hud.kind {
                HudKind::Recording => {
                    sh.circle(ix, iy, (6.0 + 8.0 * phase) * k, with_alpha(RED, (1.0 - phase) * 0.45));
                    sh.circle(ix, iy, 6.0 * k, argb(RED));
                }
                HudKind::Looping => sh.loop_arrows(ix, iy, k, argb(accent)),
                HudKind::Saved => sh.check(ix, iy, k, argb(accent)),
            }

            for run in runs.iter().filter(|r| r.key) {
                let (x, y) = ((run.rect.left - pad) as f32, run.rect.top as f32);
                let (rw, rh) = ((run.rect.right - run.rect.left + 2 * pad) as f32, line_h as f32);
                sh.fill_round(x, y, rw, rh, 5.0 * k, argb(t.chip));
                sh.stroke_round(x, y, rw, rh, 5.0 * k, argb(t.chip_border), k.max(1.0));
            }
            GdipDeleteGraphics(g);
        }
        if !bmp.is_null() {
            GdipDisposeImage(bmp as *mut GpImage);
        }

        // GDI writes zero alpha wherever it draws text. Every glyph sits on the
        // opaque card, so the alpha saved here is put back afterwards.
        let px = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        let alpha: Vec<u8> = px.iter().skip(3).step_by(4).copied().collect();
        SetBkMode(dc, TRANSPARENT as i32);
        let flags = DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX;
        let old_font = SelectObject(dc, card.fonts.title);
        SetTextColor(dc, colorref(t.text));
        SetTextCharacterExtra(dc, spacing);
        let mut tr = RECT { left: p(MARGIN + TEXT_X), top: p(MARGIN + 11.0), right: w, bottom: p(MARGIN + 29.0) };
        DrawTextW(dc, title.as_ptr(), title.len() as i32, &mut tr, flags);
        SetTextCharacterExtra(dc, 0);
        for run in &mut runs {
            SelectObject(dc, if run.key { card.fonts.key } else { card.fonts.body });
            SetTextColor(dc, colorref(if run.key { t.text } else { t.sub }));
            run.rect.right += p(4.0);
            DrawTextW(dc, run.text.as_ptr(), run.text.len() as i32, &mut run.rect, flags);
        }
        SelectObject(dc, old_font);
        GdiFlush();
        for (i, a) in alpha.into_iter().enumerate() {
            px[i * 4 + 3] = a;
        }

        let slide = ((1.0 - ease) * -10.0 * k).round() as i32;
        let work = card.work;
        let pos = POINT { x: work.left + ((work.right - work.left) - w) / 2, y: work.top + p(TOP_GAP) - m + slide };
        let size = SIZE { cx: w, cy: h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (ease.clamp(0.0, 1.0) * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(card.hwnd, null_mut(), &pos, &size, dc, &src, 0, &blend, ULW_ALPHA);
        if !card.shown {
            ShowWindow(card.hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(card.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            card.shown = true;
        }

        SelectObject(dc, old_bmp);
        DeleteObject(dib);
        DeleteDC(dc);
    }
}

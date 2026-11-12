//! Settings window for Windows. Shapes are drawn with GDI+ and text with GDI
//! into one off-screen buffer, so the window needs no extra runtime, follows
//! the system theme and stays sharp at every display scale.

use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, DeleteDC,
    DeleteObject, DrawTextW, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint,
    SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DT_CALCRECT,
    DT_CENTER, DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HDC, HFONT,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    FillModeAlternate, GdipAddPathArc, GdipClosePathFigure, GdipCreateBitmapFromScan0,
    GdipCreateFromHDC, GdipCreatePath, GdipCreatePen1, GdipCreateSolidFill, GdipDeleteBrush,
    GdipDeleteGraphics, GdipDeletePath, GdipDeletePen, GdipDisposeImage, GdipDrawImageRectI,
    GdipDrawPath, GdipFillEllipse, GdipFillPath, GdipFillPolygon, GdipFillRectangle,
    GdipSetInterpolationMode, GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdiplusStartup,
    GdiplusStartupInput, GpBitmap, GpBrush, GpGraphics, GpImage, GpPath, GpPen, GpSolidFill,
    InterpolationModeHighQualityBicubic, PixelOffsetModeHalf, PointF, SmoothingModeAntiAlias,
    UnitPixel,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi,
    MDT_EFFECTIVE_DPI,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::app::{self, NoticeKind, View};
use crate::engine::Mode;
use crate::shortcuts::{Mods, Which};
use crate::speed;

/// Client size in logical pixels (96 dpi).
const W: i32 = 460;
const H: i32 = 980;
const PAD: i32 = 20;
const CARD_W: i32 = W - 2 * PAD;
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x0026_200A;

pub(super) const ACCENT: u32 = 0x0A84FF;
pub(super) const RED: u32 = 0xFF3B30;
pub(super) const GOLD: u32 = 0xF5B400;
pub(super) const WHITE: u32 = 0xFFFFFF;

struct Palette {
    bg: u32,
    card: u32,
    border: u32,
    text: u32,
    sub: u32,
    chip: u32,
    chip_hover: u32,
    button: u32,
    button_hover: u32,
    button_border: u32,
    success: u32,
    error: u32,
}

const DARK: Palette = Palette {
    bg: 0x202020,
    card: 0x2B2B2B,
    border: 0x383838,
    text: 0xFFFFFF,
    sub: 0xA8A8A8,
    chip: 0x383838,
    chip_hover: 0x424242,
    button: 0x373737,
    button_hover: 0x404040,
    button_border: 0x454545,
    success: 0x30D158,
    error: 0xFF453A,
};

const LIGHT: Palette = Palette {
    bg: 0xF3F3F3,
    card: 0xFFFFFF,
    border: 0xE5E5E5,
    text: 0x1A1A1A,
    sub: 0x5F5F5F,
    chip: 0xF0F0F0,
    chip_hover: 0xE6E6E6,
    button: 0xFBFBFB,
    button_hover: 0xF2F2F2,
    button_border: 0xD9D9D9,
    success: 0x0F7B0F,
    error: 0xC42B1C,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Record,
    Play,
    Loop,
    Chip(Which),
    Reset,
    Autostart,
    AutoUpdate,
    Random,
    StopOnInput,
    SkipMoves,
    Check,
    Star,
    Site,
    Speed(usize),
    CustomField,
}

impl Hit {
    fn is_link(self) -> bool {
        matches!(self, Hit::Reset | Hit::Site)
    }
}

#[derive(Clone, Copy)]
pub(super) enum Font {
    Title,
    Strong,
    Body,
    Detail,
    Small,
    Caption,
}

pub(super) struct Fonts([HFONT; 6]);

impl Fonts {
    pub(super) fn new(dpi: u32) -> Self {
        let spec = [
            (18, 600),
            (14, 600),
            (14, 400),
            (13, 400),
            (12, 400),
            (12, 600),
        ];
        let face = wide("Segoe UI");
        Fonts(spec.map(|(px, weight)| unsafe {
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
        }))
    }

    pub(super) fn get(&self, f: Font) -> HFONT {
        self.0[f as usize]
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in self.0 {
            unsafe { DeleteObject(f) };
        }
    }
}

struct State {
    dpi: u32,
    dark: bool,
    fonts: Fonts,
    hits: Vec<(RECT, Hit)>,
    hover: Option<Hit>,
    pressed: Option<Hit>,
    tracking: bool,
    icon: (i32, Vec<u8>),
    /// Text of the custom speed box while it is being typed in.
    edit: Option<String>,
}

thread_local! {
    static WINDOW: Cell<HWND> = const { Cell::new(null_mut()) };
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CLASS_READY: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn window() -> HWND {
    WINDOW.with(|w| w.get())
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

pub(super) fn colorref(c: u32) -> COLORREF {
    ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

pub(super) fn argb(c: u32) -> u32 {
    0xFF00_0000 | c
}

/// Mixes `c` toward white by `t` (0..1).
pub(super) fn lighten(c: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let v = ((c >> s) & 0xFF) as f32;
        ((v + (255.0 - v) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub(super) fn apps_use_light_theme() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut data: u32 = 1;
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
    rc != 0 || data != 0
}

fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

fn apply_theme(hwnd: HWND, dark: bool) {
    let p = palette(dark);
    let flag: BOOL = dark as BOOL;
    let caption = colorref(p.bg);
    let text = colorref(p.text);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &flag as *const BOOL as *const _,
            size_of::<BOOL>() as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR as u32,
            &caption as *const u32 as *const _,
            4,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR as u32,
            &text as *const u32 as *const _,
            4,
        );
    }
}

pub(super) fn start_gdiplus() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token = 0usize;
        unsafe { GdiplusStartup(&mut token, &input, null_mut()) };
    });
}

const STYLE: WINDOW_STYLE = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

fn class_name() -> Vec<u16> {
    wide("MouseKeysRecordReplay.Settings")
}

fn register_class() {
    if CLASS_READY.with(|c| c.replace(true)) {
        return;
    }
    let class = class_name();
    unsafe {
        let hinst = GetModuleHandleW(null());
        let icon = |metric| {
            let size = GetSystemMetrics(metric);
            LoadImageW(hinst, 1 as PCWSTR, IMAGE_ICON, size, size, LR_DEFAULTCOLOR)
        };
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon(SM_CXICON),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: icon(SM_CXSMICON),
        };
        RegisterClassExW(&wc);
    }
}

/// Shows the window, or brings it to the front when it is already open.
pub fn open() {
    let existing = window();
    if !existing.is_null() {
        unsafe {
            if IsIconic(existing) != 0 {
                ShowWindow(existing, SW_RESTORE);
            }
            ShowWindow(existing, SW_SHOW);
            SetForegroundWindow(existing);
        }
        return;
    }
    start_gdiplus();
    register_class();
    unsafe {
        let mut cursor: POINT = zeroed();
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let work = info.rcWork;
        let mut r = RECT {
            left: 0,
            top: 0,
            right: W * dx as i32 / 96,
            bottom: H * dx as i32 / 96,
        };
        AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, dx);
        // A short screen at a high scale gets the whole window drawn smaller
        // rather than cut off at the bottom.
        let frame = (r.bottom - r.top) - H * dx as i32 / 96;
        let fit = ((work.bottom - work.top - frame).max(1) * 96 / H) as u32;
        let scale = dx.min(fit).max(66);
        if scale < dx {
            r = RECT {
                left: 0,
                top: 0,
                right: W * scale as i32 / 96,
                bottom: H * scale as i32 / 96,
            };
            AdjustWindowRectExForDpi(&mut r, STYLE, 0, 0, dx);
        }
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let x = work.left + ((work.right - work.left) - ww) / 2;
        let y = (work.top + ((work.bottom - work.top) - wh) / 2).max(work.top);
        let class = class_name();
        let title = wide(crate::config::APP_NAME);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            STYLE,
            x,
            y,
            ww,
            wh,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let dpi = GetDpiForWindow(hwnd).max(96).min(scale.max(66));
        let dark = !apps_use_light_theme();
        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                dpi,
                dark,
                fonts: Fonts::new(dpi),
                hits: Vec::new(),
                hover: None,
                pressed: None,
                tracking: false,
                icon: (0, Vec::new()),
                edit: None,
            })
        });
        WINDOW.with(|w| w.set(hwnd));
        apply_theme(hwnd, dark);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
    }
}

pub fn refresh() {
    let hwnd = window();
    if !hwnd.is_null() {
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

pub fn close() {
    let hwnd = window();
    if !hwnd.is_null() {
        unsafe { DestroyWindow(hwnd) };
    }
}

// Drawing

/// One text run, queued while GDI+ owns the device context and drawn after.
pub(super) struct Text {
    rect: RECT,
    s: Vec<u16>,
    font: Font,
    color: u32,
    flags: u32,
}

/// Draws one frame into `hdc`: shapes through GDI+ between `begin` and
/// `end`, then the queued text through GDI. `H` is what a click area means.
pub(super) struct Canvas<'a, H = Hit> {
    pub(super) g: *mut GpGraphics,
    pub(super) k: f32,
    pub(super) hdc: HDC,
    pub(super) fonts: &'a Fonts,
    pub(super) texts: Vec<Text>,
    pub(super) hits: Vec<(RECT, H)>,
}

impl<H> Canvas<'_, H> {
    pub(super) fn begin(&mut self) {
        unsafe {
            GdipCreateFromHDC(self.hdc, &mut self.g);
            GdipSetSmoothingMode(self.g, SmoothingModeAntiAlias);
            GdipSetPixelOffsetMode(self.g, PixelOffsetModeHalf);
            GdipSetInterpolationMode(self.g, InterpolationModeHighQualityBicubic);
        }
    }

    pub(super) fn end(&mut self) {
        let hdc = self.hdc;
        unsafe {
            GdipDeleteGraphics(self.g);
            SetBkMode(hdc, TRANSPARENT as i32);
        }
        // Two-line runs need explicit centring because DT_VCENTER only applies to single lines.
        for t in &self.texts {
            let mut r = t.rect;
            unsafe {
                let old = SelectObject(hdc, self.fonts.get(t.font));
                if t.flags & DT_WORDBREAK != 0 && t.flags & DT_VCENTER != 0 {
                    let mut m = r;
                    DrawTextW(hdc, t.s.as_ptr(), -1, &mut m, t.flags | DT_CALCRECT);
                    let off = ((r.bottom - r.top) - (m.bottom - m.top)) / 2;
                    r.top += off.max(0);
                }
                SetTextColor(hdc, colorref(t.color));
                DrawTextW(hdc, t.s.as_ptr(), -1, &mut r, t.flags);
                SelectObject(hdc, old);
            }
        }
    }

    pub(super) fn px(&self, v: i32) -> i32 {
        (v as f32 * self.k).round() as i32
    }

    pub(super) fn rect(&self, x: i32, y: i32, w: i32, h: i32) -> RECT {
        RECT {
            left: self.px(x),
            top: self.px(y),
            right: self.px(x + w),
            bottom: self.px(y + h),
        }
    }

    /// Text width in logical pixels. Called before GDI+ takes the DC.
    pub(super) fn measure(&self, font: Font, s: &str) -> i32 {
        let w = wide(s);
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 10_000,
            bottom: 1000,
        };
        unsafe {
            let old = SelectObject(self.hdc, self.fonts.get(font));
            DrawTextW(
                self.hdc,
                w.as_ptr(),
                -1,
                &mut r,
                DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
            );
            SelectObject(self.hdc, old);
        }
        (r.right as f32 / self.k).ceil() as i32
    }

    // The window and panel layout code calls text() and round_rect() about 50 times with flat (x, y, w, h);
    // a rect struct would churn every call site for no behaviour change.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn text(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        s: &str,
        font: Font,
        color: u32,
        flags: u32,
    ) {
        let rect = self.rect(x, y, w, h);
        self.texts.push(Text {
            rect,
            s: wide(s),
            font,
            color,
            flags: flags | DT_NOPREFIX,
        });
    }

    pub(super) fn hit(&mut self, x: i32, y: i32, w: i32, h: i32, hit: H) {
        let r = self.rect(x, y, w, h);
        self.hits.push((r, hit));
    }

    fn round_path(&self, x: f32, y: f32, w: f32, h: f32, r: f32) -> *mut GpPath {
        let (x, y, w, h, r) = (
            x * self.k,
            y * self.k,
            w * self.k,
            h * self.k,
            (r * self.k).min(h * self.k / 2.0),
        );
        let d = r * 2.0;
        let mut path: *mut GpPath = null_mut();
        unsafe {
            GdipCreatePath(FillModeAlternate, &mut path);
            GdipAddPathArc(path, x, y, d, d, 180.0, 90.0);
            GdipAddPathArc(path, x + w - d, y, d, d, 270.0, 90.0);
            GdipAddPathArc(path, x + w - d, y + h - d, d, d, 0.0, 90.0);
            GdipAddPathArc(path, x, y + h - d, d, d, 90.0, 90.0);
            GdipClosePathFigure(path);
        }
        path
    }

    fn fill(&self, path: *mut GpPath, color: u32) {
        unsafe {
            let mut brush: *mut GpSolidFill = null_mut();
            GdipCreateSolidFill(argb(color), &mut brush);
            GdipFillPath(self.g, brush as *mut GpBrush, path);
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    fn stroke(&self, path: *mut GpPath, color: u32, width: f32) {
        unsafe {
            let mut pen: *mut GpPen = null_mut();
            GdipCreatePen1(argb(color), width * self.k, UnitPixel, &mut pen);
            GdipDrawPath(self.g, pen, path);
            GdipDeletePen(pen);
        }
    }

    // Same flat (x, y, w, h) shape as text() and hit(), used the same way by the layout code.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn round_rect(
        &self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        r: f32,
        fill: u32,
        border: Option<(u32, f32)>,
    ) {
        if r <= 0.0 {
            // A zero radius gives empty arcs, which GDI+ fills as nothing.
            let k = self.k;
            unsafe {
                let mut brush: *mut GpSolidFill = null_mut();
                GdipCreateSolidFill(argb(fill), &mut brush);
                GdipFillRectangle(
                    self.g,
                    brush as *mut GpBrush,
                    x as f32 * k,
                    y as f32 * k,
                    w as f32 * k,
                    h as f32 * k,
                );
                GdipDeleteBrush(brush as *mut GpBrush);
            }
            return;
        }
        let path = self.round_path(x as f32, y as f32, w as f32, h as f32, r);
        self.fill(path, fill);
        unsafe { GdipDeletePath(path) };
        if let Some((color, width)) = border {
            let inset = width / 2.0;
            let path = self.round_path(
                x as f32 + inset,
                y as f32 + inset,
                w as f32 - width,
                h as f32 - width,
                r - inset,
            );
            self.stroke(path, color, width);
            unsafe { GdipDeletePath(path) };
        }
    }

    pub(super) fn circle(&self, cx: f32, cy: f32, r: f32, color: u32) {
        unsafe {
            let mut brush: *mut GpSolidFill = null_mut();
            GdipCreateSolidFill(argb(color), &mut brush);
            let k = self.k;
            GdipFillEllipse(
                self.g,
                brush as *mut GpBrush,
                (cx - r) * k,
                (cy - r) * k,
                2.0 * r * k,
                2.0 * r * k,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    pub(super) fn star(&self, cx: f32, cy: f32, r: f32, color: u32) {
        let pts: Vec<PointF> = (0..10)
            .map(|i| {
                let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
                let rr = if i % 2 == 0 { r } else { r * 0.45 };
                PointF {
                    X: (cx + rr * a.cos()) * self.k,
                    Y: (cy + rr * a.sin()) * self.k,
                }
            })
            .collect();
        unsafe {
            let mut brush: *mut GpSolidFill = null_mut();
            GdipCreateSolidFill(argb(color), &mut brush);
            GdipFillPolygon(
                self.g,
                brush as *mut GpBrush,
                pts.as_ptr(),
                pts.len() as i32,
                FillModeAlternate,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }

    pub(super) fn image(&self, x: i32, y: i32, size: i32, bgra: &[u8]) {
        let s = self.px(size);
        unsafe {
            let mut bmp: *mut GpBitmap = null_mut();
            GdipCreateBitmapFromScan0(
                s,
                s,
                s * 4,
                PIXEL_FORMAT_32BPP_ARGB,
                bgra.as_ptr(),
                &mut bmp,
            );
            if !bmp.is_null() {
                GdipDrawImageRectI(self.g, bmp as *mut GpImage, self.px(x), self.px(y), s, s);
                GdipDisposeImage(bmp as *mut GpImage);
            }
        }
    }

    /// Fills the outline through `pts`, given in logical pixels.
    pub(super) fn polygon(&self, pts: &[(f32, f32)], color: u32) {
        let pts: Vec<PointF> = pts
            .iter()
            .map(|&(x, y)| PointF {
                X: x * self.k,
                Y: y * self.k,
            })
            .collect();
        unsafe {
            let mut brush: *mut GpSolidFill = null_mut();
            GdipCreateSolidFill(argb(color), &mut brush);
            GdipFillPolygon(
                self.g,
                brush as *mut GpBrush,
                pts.as_ptr(),
                pts.len() as i32,
                FillModeAlternate,
            );
            GdipDeleteBrush(brush as *mut GpBrush);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    Primary(u32),
    Secondary,
    Disabled,
}

fn paint(hwnd: HWND, hdc: HDC) {
    let Some(view) = app::view() else { return };
    let mut rc: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    let (cw, ch) = (rc.right, rc.bottom);
    unsafe {
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, cw, ch);
        let old = SelectObject(mem, bmp);
        with_state(|s| draw(s, mem, cw, ch, &view));
        BitBlt(hdc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }
}

fn icon_pixels(state: &mut State, size: i32) -> &[u8] {
    if state.icon.0 != size {
        let mut px = crate::appicon::rgba(size as u32);
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        state.icon = (size, px);
    }
    &state.icon.1
}

fn draw(state: &mut State, hdc: HDC, cw: i32, ch: i32, v: &View) {
    let p = palette(state.dark);
    let k = state.dpi as f32 / 96.0;
    let hover = state.hover;
    let icon_size = (36.0 * k).round() as i32;
    let icon = icon_pixels(state, icon_size).to_vec();
    let state_hits;
    {
        let mut c = Canvas {
            g: null_mut(),
            k,
            hdc,
            fonts: &state.fonts,
            texts: Vec::new(),
            hits: Vec::new(),
        };

        let site_label = crate::config::site_label();
        let site_w = c.measure(Font::Body, site_label);
        let reset_w = c.measure(Font::Caption, "Reset to defaults");
        let chip_label = |w: Which| {
            if v.capturing == Some(w) {
                "Press keys...".to_string()
            } else {
                match w {
                    Which::Record => v.record.clone(),
                    Which::Play => v.play.clone(),
                    Which::Loop => v.repeat.clone(),
                    Which::History => v.history.clone(),
                }
            }
        };
        let chip_w = [Which::Record, Which::Play, Which::Loop, Which::History]
            .map(|w| (c.measure(Font::Strong, &chip_label(w)) + 28).max(120));
        let speed_chip = match (v.speed_keys.first(), v.speed_keys.last()) {
            (Some(a), Some(b)) => format!("{a} to {}", b.rsplit(' ').next().unwrap_or(b)),
            _ => String::new(),
        };
        let speed_chip_w = (c.measure(Font::Strong, &speed_chip) + 28).max(120);
        let speed_label_w = speed::LABELS.map(|l| c.measure(Font::Small, l));
        let edit = state.edit.clone();

        c.begin();
        let full = (cw as f32 / k).ceil() as i32 + 1;
        let full_h = (ch as f32 / k).ceil() as i32 + 1;
        c.round_rect(0, 0, full, full_h, 0.0, p.bg, None);

        // Header
        c.image(PAD, 20, 36, &icon);
        c.text(
            PAD + 48,
            18,
            280,
            24,
            crate::config::APP_NAME,
            Font::Title,
            p.text,
            DT_SINGLELINE | DT_VCENTER,
        );
        c.text(
            PAD + 48,
            41,
            280,
            16,
            &format!("Version {}", v.version),
            Font::Small,
            p.sub,
            DT_SINGLELINE | DT_VCENTER,
        );
        let site_color = if hover == Some(Hit::Site) {
            lighten(ACCENT, 0.25)
        } else {
            ACCENT
        };
        c.text(
            W - PAD - site_w,
            28,
            site_w,
            20,
            site_label,
            Font::Body,
            site_color,
            DT_SINGLELINE | DT_VCENTER,
        );
        c.hit(W - PAD - site_w - 4, 26, site_w + 8, 24, Hit::Site);

        // Status
        let y = 72;
        let card_h = 124 + SPEED_ROW;
        card(&c, y, card_h, p);
        let dot = match v.mode {
            Mode::Idle => p.success,
            Mode::Recording => RED,
            Mode::Playing => ACCENT,
        };
        c.circle((PAD + 21) as f32, (y + 25) as f32, 5.0, dot);
        c.text(
            PAD + 34,
            y + 14,
            CARD_W - 50,
            22,
            &v.status_title,
            Font::Strong,
            p.text,
            DT_SINGLELINE | DT_VCENTER,
        );
        c.text(
            PAD + 16,
            y + 38,
            CARD_W - 32,
            36,
            &v.status_detail,
            Font::Detail,
            p.sub,
            DT_WORDBREAK | DT_END_ELLIPSIS,
        );
        let bw = (CARD_W - 32 - 24) / 3;
        let by = y + 124 - 14 - 32;
        let (rec_text, rec_color) = if v.mode == Mode::Recording {
            ("Stop recording", RED)
        } else {
            ("Start recording", ACCENT)
        };
        button(
            &mut c,
            PAD + 16,
            by,
            bw,
            32,
            rec_text,
            Look::Primary(rec_color),
            hover == Some(Hit::Record),
            Hit::Record,
            p,
        );
        let can_play = v.mode != Mode::Idle || crate::engine::has_recording();
        let play_text = if v.mode == Mode::Playing && !v.looping {
            "Stop playback"
        } else {
            "Play once"
        };
        let loop_text = if v.looping { "Stop loop" } else { "Loop" };
        let play_look = if can_play {
            Look::Secondary
        } else {
            Look::Disabled
        };
        button(
            &mut c,
            PAD + 16 + bw + 12,
            by,
            bw,
            32,
            play_text,
            play_look,
            hover == Some(Hit::Play),
            Hit::Play,
            p,
        );
        button(
            &mut c,
            PAD + 16 + (bw + 12) * 2,
            by,
            bw,
            32,
            loop_text,
            play_look,
            hover == Some(Hit::Loop),
            Hit::Loop,
            p,
        );
        speed_row(
            &mut c,
            y + 124,
            v,
            edit.as_deref(),
            &speed_label_w,
            hover,
            p,
        );

        // Shortcuts
        let y = 208 + SPEED_ROW;
        c.text(
            PAD + 4,
            y,
            200,
            16,
            "Shortcuts",
            Font::Caption,
            p.sub,
            DT_SINGLELINE | DT_VCENTER,
        );
        let reset_color = if hover == Some(Hit::Reset) {
            lighten(ACCENT, 0.25)
        } else {
            ACCENT
        };
        c.text(
            W - PAD - 4 - reset_w,
            y,
            reset_w,
            16,
            "Reset to defaults",
            Font::Caption,
            reset_color,
            DT_SINGLELINE | DT_VCENTER,
        );
        c.hit(W - PAD - 8 - reset_w, y - 4, reset_w + 8, 24, Hit::Reset);
        let y = 228 + SPEED_ROW;
        let row = 46;
        card(&c, y, row * 5, p);
        divider(&c, y + row, p);
        divider(&c, y + row * 2, p);
        divider(&c, y + row * 3, p);
        divider(&c, y + row * 4, p);
        // The speed shortcuts are fixed, so their chip is read-only.
        let ry = y + row * 4;
        c.text(
            PAD + 16,
            ry,
            200,
            row,
            "Change speed",
            Font::Body,
            p.text,
            DT_SINGLELINE | DT_VCENTER,
        );
        let cx = W - PAD - 12 - speed_chip_w;
        let cy = ry + (row - 30) / 2;
        c.round_rect(cx, cy, speed_chip_w, 30, 6.0, p.chip, Some((p.border, 1.0)));
        c.text(
            cx,
            cy,
            speed_chip_w,
            30,
            &speed_chip,
            Font::Strong,
            p.text,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );
        let rows = [
            (Which::Record, "Record and stop"),
            (Which::Play, "Play once"),
            (Which::Loop, "Loop until stopped"),
            (Which::History, "Open recordings"),
        ];
        for (i, (w, label)) in rows.into_iter().enumerate() {
            let ry = y + row * i as i32;
            c.text(
                PAD + 16,
                ry,
                200,
                row,
                label,
                Font::Body,
                p.text,
                DT_SINGLELINE | DT_VCENTER,
            );
            let cwid = chip_w[i];
            let cx = W - PAD - 12 - cwid;
            let cy = ry + (row - 30) / 2;
            let capturing = v.capturing == Some(w);
            let hovered = hover == Some(Hit::Chip(w));
            if capturing {
                c.round_rect(cx, cy, cwid, 30, 6.0, p.card, Some((ACCENT, 2.0)));
            } else {
                c.round_rect(
                    cx,
                    cy,
                    cwid,
                    30,
                    6.0,
                    if hovered { p.chip_hover } else { p.chip },
                    Some((p.border, 1.0)),
                );
            }
            let color = if capturing { ACCENT } else { p.text };
            c.text(
                cx,
                cy,
                cwid,
                30,
                &chip_label(w),
                Font::Strong,
                color,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            );
            c.hit(cx, cy, cwid, 30, Hit::Chip(w));
        }

        // Notice
        let y = 370 + SPEED_ROW + 92;
        let (text, color) = match (&v.notice, v.capturing) {
            (_, Some(_)) => (
                "Press the new shortcut, or Esc to cancel.".to_string(),
                ACCENT,
            ),
            (Some(n), None) => (
                n.text.clone(),
                match n.kind {
                    NoticeKind::Error => p.error,
                    NoticeKind::Success => p.success,
                },
            ),
            (None, None) => ("Click a shortcut to change it.".to_string(), p.sub),
        };
        c.text(
            PAD + 4,
            y,
            CARD_W - 8,
            34,
            &text,
            Font::Small,
            color,
            DT_WORDBREAK | DT_END_ELLIPSIS,
        );

        // General
        let y = 408 + SPEED_ROW + 92;
        c.text(
            PAD + 4,
            y,
            200,
            16,
            "General",
            Font::Caption,
            p.sub,
            DT_SINGLELINE | DT_VCENTER,
        );
        let y = 428 + SPEED_ROW + 92;
        let row = 50;
        card(&c, y, row * 6, p);
        for i in 1..6 {
            divider(&c, y + row * i, p);
        }
        let rows = [
            (
                "Launch at startup",
                "Runs quietly in the tray after you sign in.".to_string(),
            ),
            (
                "Update automatically",
                "Installs new versions when the app starts.".to_string(),
            ),
            (
                "Random timing",
                "Each press lands up to half a second early or late.".to_string(),
            ),
            (
                "Stop on mouse or key input",
                "Off: only the shortcuts stop playback.".to_string(),
            ),
            (
                "Ignore mouse movement",
                "Records clicks and keys, played at the pointer.".to_string(),
            ),
            (
                "Updates",
                if v.update_text.is_empty() {
                    format!("You have version {}.", v.version)
                } else {
                    v.update_text.clone()
                },
            ),
        ];
        for (i, (title, sub)) in rows.iter().enumerate() {
            let ry = y + row * i as i32;
            c.text(
                PAD + 16,
                ry + 7,
                260,
                19,
                title,
                Font::Body,
                p.text,
                DT_SINGLELINE | DT_VCENTER,
            );
            c.text(
                PAD + 16,
                ry + 26,
                260,
                16,
                sub,
                Font::Small,
                p.sub,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
        }
        switch(&mut c, y, row, v.autostart, Hit::Autostart, p);
        switch(&mut c, y + row, row, v.auto_update, Hit::AutoUpdate, p);
        switch(&mut c, y + row * 2, row, v.random_timing, Hit::Random, p);
        switch(
            &mut c,
            y + row * 3,
            row,
            v.stop_on_input,
            Hit::StopOnInput,
            p,
        );
        switch(&mut c, y + row * 4, row, v.skip_moves, Hit::SkipMoves, p);
        let bw = 140;
        button(
            &mut c,
            W - PAD - 12 - bw,
            y + row * 5 + (row - 30) / 2,
            bw,
            30,
            "Check now",
            Look::Secondary,
            hover == Some(Hit::Check),
            Hit::Check,
            p,
        );

        // Star
        let y = 740 + SPEED_ROW + 92;
        let sh = 52;
        card(&c, y, sh, p);
        let bw = 150;
        c.text(
            PAD + 16,
            y,
            CARD_W - 32 - bw - 12,
            sh,
            "Enjoying it? A star on GitHub helps others find it.",
            Font::Detail,
            p.text,
            DT_WORDBREAK | DT_VCENTER,
        );
        let bx = W - PAD - 12 - bw;
        let byy = y + (sh - 30) / 2;
        let fill = if hover == Some(Hit::Star) {
            p.button_hover
        } else {
            p.button
        };
        c.round_rect(bx, byy, bw, 30, 6.0, fill, Some((p.button_border, 1.0)));
        c.star((bx + 22) as f32, (byy + 15) as f32, 7.5, GOLD);
        c.text(
            bx + 34,
            byy,
            bw - 42,
            30,
            "Star on GitHub",
            Font::Strong,
            p.text,
            DT_SINGLELINE | DT_VCENTER,
        );
        c.hit(bx, byy, bw, 30, Hit::Star);

        c.end();
        state_hits = std::mem::take(&mut c.hits);
    }
    state.hits = state_hits;
}

/// Height of the speed slider row at the bottom of the status card.
const SPEED_ROW: i32 = 76;
const FIELD_W: i32 = 84;

/// x of each slider stop, left to right.
fn stops() -> [i32; speed::COUNT] {
    let (x0, x1) = (PAD + 34, W - PAD - 16 - FIELD_W - 34);
    std::array::from_fn(|i| x0 + (x1 - x0) * i as i32 / (speed::COUNT as i32 - 1))
}

/// Six-stop slider with the custom box beside it and a hint or error line.
fn speed_row(
    c: &mut Canvas,
    y: i32,
    v: &View,
    edit: Option<&str>,
    label_w: &[i32; speed::COUNT],
    hover: Option<Hit>,
    p: &Palette,
) {
    divider(c, y, p);
    let st = stops();
    let sel = v.speed_step.min(speed::CUSTOM);
    let ty = y + 22;
    c.round_rect(
        st[0],
        ty - 2,
        st[speed::CUSTOM] - st[0],
        4,
        2.0,
        p.border,
        None,
    );
    c.round_rect(st[0], ty - 2, st[sel] - st[0], 4, 2.0, ACCENT, None);
    let half = (st[1] - st[0]) / 2;
    for (i, &x) in st.iter().enumerate() {
        let on = i <= sel;
        c.circle(
            x as f32,
            ty as f32,
            if on { 4.0 } else { 3.5 },
            if on { ACCENT } else { p.sub },
        );
        let lw = label_w[i] + 4;
        let color = if i == sel { p.text } else { p.sub };
        let font = if i == sel { Font::Caption } else { Font::Small };
        c.text(
            x - lw / 2 - 4,
            ty + 10,
            lw + 8,
            16,
            speed::LABELS[i],
            font,
            color,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER,
        );
        c.hit(x - half, y + 4, half * 2, 42, Hit::Speed(i));
    }
    let r = if hover.is_some_and(|h| matches!(h, Hit::Speed(_))) {
        9.0
    } else {
        8.0
    };
    c.circle(st[sel] as f32, ty as f32, r, ACCENT);
    c.circle(st[sel] as f32, ty as f32, r - 3.0, WHITE);

    let custom = sel == speed::CUSTOM;
    let (fx, fy) = (W - PAD - 16 - FIELD_W, y + 8);
    let (fill, border) = match (edit.is_some(), custom) {
        (true, _) => (p.card, (ACCENT, 2.0)),
        (false, true) => (p.chip, (ACCENT, 1.0)),
        (false, false) => (
            if hover == Some(Hit::CustomField) {
                p.chip_hover
            } else {
                p.bg
            },
            (p.border, 1.0),
        ),
    };
    c.round_rect(fx, fy, FIELD_W, 30, 6.0, fill, Some(border));
    let text = match edit {
        Some(t) => format!("{t}|"),
        None => format!("{}x", speed::custom_text(v.custom_speed)),
    };
    c.text(
        fx + 8,
        fy,
        FIELD_W - 16,
        30,
        &text,
        Font::Strong,
        if custom || edit.is_some() {
            p.text
        } else {
            p.sub
        },
        DT_SINGLELINE | DT_VCENTER | DT_CENTER,
    );
    c.hit(fx, fy, FIELD_W, 30, Hit::CustomField);

    let (line, color) = match &v.speed_error {
        Some(e) => (e.clone(), p.error),
        None if edit.is_some() => ("Type 0.1 to 1000, then press Enter.".to_string(), p.sub),
        None => (
            format!(
                "Playing at {}. Changes apply at once, also mid-playback.",
                speed::step_label(sel, v.custom_speed)
            ),
            p.sub,
        ),
    };
    c.text(
        PAD + 16,
        y + 52,
        CARD_W - 32,
        16,
        &line,
        Font::Small,
        color,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
}

fn card(c: &Canvas, y: i32, h: i32, p: &Palette) {
    c.round_rect(PAD, y, CARD_W, h, 8.0, p.card, Some((p.border, 1.0)));
}

fn divider(c: &Canvas, y: i32, p: &Palette) {
    c.round_rect(PAD + 16, y, CARD_W - 32, 1, 0.0, p.border, None);
}

#[allow(clippy::too_many_arguments)]
fn button(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    label: &str,
    look: Look,
    hovered: bool,
    hit: Hit,
    p: &Palette,
) {
    let (fill, border, color) = match look {
        Look::Primary(base) => (
            if hovered { lighten(base, 0.12) } else { base },
            None,
            WHITE,
        ),
        Look::Secondary => (
            if hovered { p.button_hover } else { p.button },
            Some((p.button_border, 1.0)),
            p.text,
        ),
        Look::Disabled => (p.button, Some((p.border, 1.0)), p.sub),
    };
    c.round_rect(x, y, w, h, 6.0, fill, border);
    c.text(
        x,
        y,
        w,
        h,
        label,
        Font::Strong,
        color,
        DT_SINGLELINE | DT_VCENTER | DT_CENTER,
    );
    if look != Look::Disabled {
        c.hit(x, y, w, h, hit);
    }
}

fn switch(c: &mut Canvas, y: i32, row: i32, on: bool, hit: Hit, p: &Palette) {
    let (w, h) = (40, 20);
    let x = W - PAD - 16 - w;
    let sy = y + (row - h) / 2;
    if on {
        c.round_rect(x, sy, w, h, 10.0, ACCENT, None);
        c.circle((x + w - 10) as f32, (sy + 10) as f32, 6.0, WHITE);
    } else {
        c.round_rect(x, sy, w, h, 10.0, p.card, Some((p.sub, 1.0)));
        c.circle((x + 10) as f32, (sy + 10) as f32, 5.0, p.sub);
    }
    c.hit(x - 8, y, w + 16, row, hit);
}

// Input

fn hit_at(lp: LPARAM) -> Option<Hit> {
    let x = (lp & 0xFFFF) as i16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as i16 as i32;
    with_state(|s| {
        s.hits
            .iter()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, h)| *h)
    })
    .flatten()
}

fn act(hwnd: HWND, hit: Hit) {
    let Some(v) = app::view() else { return };
    if v.capturing.is_some() && !matches!(hit, Hit::Chip(_)) {
        app::cancel_capture();
    }
    if hit != Hit::CustomField {
        commit_edit();
    }
    match hit {
        // The window closes first so its own clicks are never recorded or replayed.
        Hit::Record => {
            close();
            app::toggle_record_from_ui();
        }
        Hit::Play => {
            close();
            app::toggle_play_from_ui();
        }
        Hit::Loop => {
            close();
            app::toggle_loop_from_ui();
        }
        Hit::Chip(w) => {
            if v.capturing == Some(w) {
                app::cancel_capture();
            } else {
                app::begin_capture(w);
                unsafe { SetFocus(hwnd) };
            }
        }
        Hit::Reset => app::reset_shortcuts(),
        Hit::Autostart => app::set_autostart(!v.autostart),
        Hit::AutoUpdate => app::set_auto_update(!v.auto_update),
        Hit::Random => app::set_random_timing(!v.random_timing),
        Hit::StopOnInput => app::set_stop_on_input(!v.stop_on_input),
        Hit::SkipMoves => app::set_skip_moves(!v.skip_moves),
        Hit::Check => app::check_updates(),
        Hit::Star => app::open_repo(),
        Hit::Site => app::open_site(),
        Hit::Speed(i) => app::set_speed_step(i),
        Hit::CustomField => {
            if with_state(|s| s.edit.is_none()).unwrap_or(false) {
                app::set_speed_step(speed::CUSTOM);
                with_state(|s| s.edit = Some(speed::custom_text(v.custom_speed)));
                unsafe { SetFocus(hwnd) };
                refresh();
            }
        }
    }
}

/// Applies the typed custom speed. A refused value keeps the old one, and
/// the app shows why under the box.
fn commit_edit() {
    if let Some(text) = with_state(|s| s.edit.take()).flatten() {
        let _ = app::set_custom_speed(&text);
        refresh();
    }
}

fn editing() -> bool {
    with_state(|s| s.edit.is_some()).unwrap_or(false)
}

/// One typed character for the custom speed box.
fn edit_char(ch: u16) {
    match ch {
        0x0D => {
            let text = with_state(|s| s.edit.clone()).flatten().unwrap_or_default();
            if app::set_custom_speed(&text).is_ok() {
                with_state(|s| s.edit = None);
            }
        }
        0x1B => {
            with_state(|s| s.edit = None);
        }
        0x08 => {
            with_state(|s| s.edit.as_mut().map(|t| t.pop()));
        }
        _ => {
            let Some(c) = char::from_u32(ch as u32) else {
                return;
            };
            if c.is_ascii_digit() || matches!(c, '.' | 'x' | 'X' | 'e' | 'E') {
                with_state(|s| {
                    if let Some(t) = s.edit.as_mut().filter(|t| t.len() < 10) {
                        t.push(c);
                    }
                });
            }
        }
    }
    refresh();
}

fn key_code(vk: u16, extended: bool) -> Option<String> {
    let s = match vk {
        0x41..=0x5A => format!("Key{}", vk as u8 as char),
        0x30..=0x39 => format!("Digit{}", vk as u8 as char),
        _ if (VK_F1..=VK_F24).contains(&vk) => format!("F{}", vk - VK_F1 + 1),
        _ if (VK_NUMPAD0..=VK_NUMPAD9).contains(&vk) => format!("Numpad{}", vk - VK_NUMPAD0),
        VK_MULTIPLY => "NumpadMultiply".into(),
        VK_ADD => "NumpadAdd".into(),
        VK_SUBTRACT => "NumpadSubtract".into(),
        VK_DECIMAL => "NumpadDecimal".into(),
        VK_DIVIDE => "NumpadDivide".into(),
        VK_SPACE => "Space".into(),
        VK_RETURN if extended => "NumpadEnter".into(),
        VK_RETURN => "Enter".into(),
        VK_TAB => "Tab".into(),
        VK_BACK => "Backspace".into(),
        VK_DELETE => "Delete".into(),
        VK_INSERT => "Insert".into(),
        VK_HOME => "Home".into(),
        VK_END => "End".into(),
        VK_PRIOR => "PageUp".into(),
        VK_NEXT => "PageDown".into(),
        VK_UP => "ArrowUp".into(),
        VK_DOWN => "ArrowDown".into(),
        VK_LEFT => "ArrowLeft".into(),
        VK_RIGHT => "ArrowRight".into(),
        VK_SNAPSHOT => "PrintScreen".into(),
        VK_SCROLL => "ScrollLock".into(),
        VK_PAUSE => "Pause".into(),
        VK_OEM_1 => "Semicolon".into(),
        VK_OEM_PLUS => "Equal".into(),
        VK_OEM_COMMA => "Comma".into(),
        VK_OEM_MINUS => "Minus".into(),
        VK_OEM_PERIOD => "Period".into(),
        VK_OEM_2 => "Slash".into(),
        VK_OEM_3 => "Backquote".into(),
        VK_OEM_4 => "BracketLeft".into(),
        VK_OEM_5 => "Backslash".into(),
        VK_OEM_6 => "BracketRight".into(),
        VK_OEM_7 => "Quote".into(),
        _ => return None,
    };
    Some(s)
}

fn held(vk: u16) -> bool {
    unsafe { GetKeyState(vk as i32) < 0 }
}

/// Handles a key press while the window waits for a new shortcut.
fn capture_key(which: Which, vk: u16, lp: LPARAM) {
    let mods = Mods {
        ctrl: held(VK_CONTROL),
        alt: held(VK_MENU),
        shift: held(VK_SHIFT),
        sup: held(VK_LWIN) || held(VK_RWIN),
    };
    if vk == VK_ESCAPE && mods == Mods::default() {
        app::cancel_capture();
        return;
    }
    if matches!(
        vk,
        VK_CONTROL
            | VK_MENU
            | VK_SHIFT
            | VK_LWIN
            | VK_RWIN
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
            | VK_LSHIFT
            | VK_RSHIFT
    ) {
        return;
    }
    if let Some(code) = key_code(vk, (lp >> 24) & 1 == 1) {
        let _ = app::set_shortcut(which, mods, &code);
    }
}

fn set_hover(hwnd: HWND, hit: Option<Hit>) {
    let changed = with_state(|s| std::mem::replace(&mut s.hover, hit) != hit).unwrap_or(false);
    if changed {
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            paint(hwnd, hdc);
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            let start = with_state(|s| !std::mem::replace(&mut s.tracking, true)).unwrap_or(false);
            if start {
                let mut t = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut t);
            }
            set_hover(hwnd, hit_at(lp));
            0
        }
        WM_MOUSELEAVE => {
            with_state(|s| s.tracking = false);
            set_hover(hwnd, None);
            0
        }
        WM_SETCURSOR if (lp & 0xFFFF) as u32 == HTCLIENT => {
            let link = with_state(|s| s.hover.is_some_and(Hit::is_link)).unwrap_or(false);
            SetCursor(LoadCursorW(
                null_mut(),
                if link { IDC_HAND } else { IDC_ARROW },
            ));
            1
        }
        WM_LBUTTONDOWN => {
            let hit = hit_at(lp);
            with_state(|s| s.pressed = hit);
            SetCapture(hwnd);
            0
        }
        WM_LBUTTONUP => {
            let pressed = with_state(|s| s.pressed.take()).flatten();
            ReleaseCapture();
            let hit = hit_at(lp);
            match hit {
                Some(h) if Some(h) == pressed => act(hwnd, h),
                None if pressed.is_none() => {
                    if app::view().is_some_and(|v| v.capturing.is_some()) {
                        app::cancel_capture();
                    }
                    commit_edit();
                }
                _ => {}
            }
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let vk = wp as u16;
            match app::view().and_then(|v| v.capturing) {
                Some(which) => {
                    capture_key(which, vk, lp);
                    0
                }
                None if editing() => DefWindowProcW(hwnd, msg, wp, lp),
                None if vk == VK_ESCAPE => {
                    close();
                    0
                }
                None => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
        WM_CHAR | WM_SYSCHAR | WM_SYSKEYUP | WM_KEYUP
            if app::view().is_some_and(|v| v.capturing.is_some()) =>
        {
            0
        }
        WM_CHAR if editing() => {
            edit_char(wp as u16);
            0
        }
        WM_ACTIVATE => {
            if (wp & 0xFFFF) as u32 == WA_INACTIVE
                && app::view().is_some_and(|v| v.capturing.is_some())
            {
                app::cancel_capture();
            }
            if (wp & 0xFFFF) as u32 == WA_INACTIVE {
                commit_edit();
            }
            0
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED => {
            let dark = !apps_use_light_theme();
            let changed =
                with_state(|s| std::mem::replace(&mut s.dark, dark) != dark).unwrap_or(false);
            if changed {
                apply_theme(hwnd, dark);
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = ((wp >> 16) & 0xFFFF) as u32;
            with_state(|s| {
                s.dpi = dpi;
                s.fonts = Fonts::new(dpi);
            });
            let r = &*(lp as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_DESTROY => {
            WINDOW.with(|w| w.set(null_mut()));
            STATE.with(|s| s.borrow_mut().take());
            if app::view().is_some_and(|v| v.capturing.is_some()) {
                app::cancel_capture();
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

#[allow(dead_code)]
fn small_icon_size(dpi: u32) -> i32 {
    unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }
}

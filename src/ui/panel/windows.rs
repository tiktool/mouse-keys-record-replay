//! Recordings panel for Windows: a popup card drawn with the settings
//! window's canvas. Rename uses a child edit control; the panel closes as
//! soon as it loses activation.

use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
    DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, SelectObject,
    SetBkColor, SetTextColor, DT_CENTER, DT_END_ELLIPSIS, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
    HBRUSH, HDC, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{EM_LIMITTEXT, EM_SETSEL, WM_MOUSELEAVE};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::Row;
use crate::app;
use crate::ui::windows::{
    apps_use_light_theme, colorref, lighten, start_gdiplus, wide, Canvas, Font, Fonts, ACCENT,
    GOLD, RED, WHITE,
};

/// Layout in logical pixels (96 dpi).
const W: i32 = 420;
const HEAD_H: i32 = 48;
const ROW_H: i32 = 56;
const PAD: i32 = 8;
const TOP_GAP: i32 = 16;
const STAR_W: i32 = 44;
const ICONS_W: i32 = 76;
const MAX_ROWS: usize = 7;
const NAME_MAX: usize = 80;

struct Theme {
    card: u32,
    border: u32,
    text: u32,
    sub: u32,
    hover: u32,
    chip: u32,
    chip_border: u32,
    dim: u32,
}

const DARK: Theme = Theme {
    card: 0x262626,
    border: 0x3A3A3A,
    text: 0xFFFFFF,
    sub: 0xB0B0B0,
    hover: 0x303030,
    chip: 0x363636,
    chip_border: 0x474747,
    dim: 0x6A6A6A,
};
const LIGHT: Theme = Theme {
    card: 0xFFFFFF,
    border: 0xDDDDDD,
    text: 0x1A1A1A,
    sub: 0x5F5F5F,
    hover: 0xF4F4F4,
    chip: 0xF2F2F2,
    chip_border: 0xD8D8D8,
    dim: 0xBDBDBD,
};

fn theme(dark: bool) -> &'static Theme {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hit {
    Row(usize),
    Star(usize),
    Rename(usize),
    Delete(usize),
    Confirm(usize),
    Cancel,
}

impl Hit {
    fn row(self) -> Option<usize> {
        match self {
            Hit::Row(i) | Hit::Star(i) | Hit::Rename(i) | Hit::Delete(i) | Hit::Confirm(i) => {
                Some(i)
            }
            Hit::Cancel => None,
        }
    }
}

struct State {
    hwnd: HWND,
    dpi: u32,
    dark: bool,
    fonts: Fonts,
    rows: Vec<Row>,
    scroll: usize,
    hits: Vec<(RECT, Hit)>,
    hover: Option<Hit>,
    tracking: bool,
    confirm: Option<usize>,
    edit: Option<(HWND, usize)>,
    brush: HBRUSH,
}

impl Drop for State {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.brush) };
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static CLASS_READY: Cell<bool> = const { Cell::new(false) };
    static EDIT_PROC: Cell<isize> = const { Cell::new(0) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| {
        let mut b = s.try_borrow_mut().ok()?;
        b.as_mut().map(f)
    })
}

fn px(v: i32, k: f32) -> i32 {
    (v as f32 * k).round() as i32
}

fn height(rows: usize) -> i32 {
    HEAD_H + rows.clamp(1, MAX_ROWS) as i32 * ROW_H + PAD
}

fn max_scroll(rows: usize) -> usize {
    rows.saturating_sub(MAX_ROWS)
}

fn invalidate(hwnd: HWND) {
    unsafe { InvalidateRect(hwnd, null(), 0) };
}

pub fn is_open() -> bool {
    STATE.with(|s| s.try_borrow().map(|b| b.is_some()).unwrap_or(true))
}

/// Opens the panel, or shows the new rows when it is already open.
pub fn show(rows: Vec<Row>) {
    let mut rows = Some(rows);
    let updated = with(|s| {
        s.rows = rows.take().unwrap_or_default();
        s.scroll = s.scroll.min(max_scroll(s.rows.len()));
        s.confirm = None;
        s.hover = None;
        let k = s.dpi as f32 / 96.0;
        unsafe {
            SetWindowPos(
                s.hwnd,
                null_mut(),
                0,
                0,
                px(W, k),
                px(height(s.rows.len()), k),
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        s.hwnd
    });
    match (updated, rows) {
        (Some(hwnd), _) => invalidate(hwnd),
        (None, Some(rows)) => create(rows),
        (None, None) => {}
    }
}

pub fn close() {
    let hwnd = STATE.with(|s| s.try_borrow().ok().and_then(|b| b.as_ref().map(|s| s.hwnd)));
    if let Some(hwnd) = hwnd {
        unsafe { DestroyWindow(hwnd) };
    }
}

fn class_name() -> Vec<u16> {
    wide("MouseKeysRecordReplay.Recordings")
}

fn register_class() {
    if CLASS_READY.with(|c| c.replace(true)) {
        return;
    }
    let class = class_name();
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(null()),
            hIcon: null_mut(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: null_mut(),
        };
        RegisterClassExW(&wc);
    }
}

fn create(rows: Vec<Row>) {
    start_gdiplus();
    register_class();
    unsafe {
        let mut cursor: POINT = zeroed();
        GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let dpi = dx.max(96);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let k = dpi as f32 / 96.0;
        let (w, h) = (px(W, k), px(height(rows.len()), k));
        let work = info.rcWork;
        let x = work.left + ((work.right - work.left) - w) / 2;
        let y = work.top + px(TOP_GAP, k);
        let class = class_name();
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            null(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let corner = DWMWCP_ROUND;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const _ as *const _,
            size_of_val(&corner) as u32,
        );
        let dark = !apps_use_light_theme();
        let brush = CreateSolidBrush(colorref(theme(dark).chip));
        let state = State {
            hwnd,
            dpi,
            dark,
            fonts: Fonts::new(dpi),
            rows,
            scroll: 0,
            hits: Vec::new(),
            hover: None,
            tracking: false,
            confirm: None,
            edit: None,
            brush,
        };
        STATE.with(|s| *s.borrow_mut() = Some(state));
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
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
        WM_ACTIVATE => {
            if (wp & 0xFFFF) as u32 == WA_INACTIVE {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            // Dropped after the borrow ends, since dropping frees GDI objects only.
            let state = STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.take()));
            drop(state);
            0
        }
        WM_MOUSEMOVE => {
            set_hover(hwnd, hit_at(lp));
            0
        }
        WM_MOUSELEAVE => {
            with(|s| s.tracking = false);
            set_hover(hwnd, None);
            0
        }
        WM_LBUTTONUP => {
            if let Some(hit) = hit_at(lp) {
                click(hwnd, hit);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp >> 16) & 0xFFFF) as u16 as i16;
            scroll(hwnd, if delta > 0 { -1 } else { 1 });
            0
        }
        WM_KEYDOWN if wp as u16 == VK_ESCAPE => {
            escape();
            0
        }
        WM_COMMAND if ((wp >> 16) & 0xFFFF) as u32 == EN_KILLFOCUS => {
            commit_edit(true);
            0
        }
        WM_CTLCOLOREDIT => {
            let colors = with(|s| (s.brush, theme(s.dark).text, theme(s.dark).chip));
            match colors {
                Some((brush, text, bg)) => {
                    let dc = wp as HDC;
                    SetTextColor(dc, colorref(text));
                    SetBkColor(dc, colorref(bg));
                    brush as LRESULT
                }
                None => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN if wp as u16 == VK_RETURN => {
            commit_edit(true);
            return 0;
        }
        WM_KEYDOWN if wp as u16 == VK_ESCAPE => {
            commit_edit(false);
            return 0;
        }
        // Swallowed so the edit control does not beep on Enter and Esc.
        WM_CHAR if wp == 13 || wp == 27 => return 0,
        _ => {}
    }
    let old: WNDPROC = std::mem::transmute::<isize, WNDPROC>(EDIT_PROC.get());
    CallWindowProcW(old, hwnd, msg, wp, lp)
}

fn hit_at(lp: LPARAM) -> Option<Hit> {
    let (x, y) = (
        (lp & 0xFFFF) as u16 as i16 as i32,
        ((lp >> 16) & 0xFFFF) as u16 as i16 as i32,
    );
    with(|s| {
        s.hits
            .iter()
            .rev()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, h)| *h)
    })
    .flatten()
}

fn set_hover(hwnd: HWND, hit: Option<Hit>) {
    let changed = with(|s| {
        if !s.tracking {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe { TrackMouseEvent(&mut tme) };
            s.tracking = true;
        }
        let changed = s.hover != hit;
        s.hover = hit;
        changed
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

fn scroll(hwnd: HWND, by: isize) {
    commit_edit(true);
    let changed = with(|s| {
        let next = (s.scroll as isize + by).clamp(0, max_scroll(s.rows.len()) as isize) as usize;
        let changed = next != s.scroll;
        s.scroll = next;
        if changed {
            s.confirm = None;
        }
        changed
    });
    if changed == Some(true) {
        invalidate(hwnd);
    }
}

fn escape() {
    let handled = with(|s| s.confirm.take().map(|_| s.hwnd)).flatten();
    match handled {
        Some(hwnd) => invalidate(hwnd),
        None => close(),
    }
}

enum Action {
    Activate(String),
    Star(String, bool),
    Rename(usize),
    Delete(String),
    Redraw,
}

fn click(hwnd: HWND, hit: Hit) {
    let editing = with(|s| s.edit.map(|(_, i)| i)).flatten();
    if editing.is_some() && editing != hit.row() {
        commit_edit(true);
    } else if editing.is_some() {
        return;
    }
    let action = with(|s| match hit {
        Hit::Row(i) => s.rows.get(i).map(|r| Action::Activate(r.id.clone())),
        Hit::Star(i) => s
            .rows
            .get(i)
            .map(|r| Action::Star(r.id.clone(), !r.starred)),
        Hit::Rename(i) => Some(Action::Rename(i)),
        Hit::Delete(i) => {
            s.confirm = Some(i);
            Some(Action::Redraw)
        }
        Hit::Confirm(i) => {
            s.confirm = None;
            s.rows.get(i).map(|r| Action::Delete(r.id.clone()))
        }
        Hit::Cancel => {
            s.confirm = None;
            Some(Action::Redraw)
        }
    })
    .flatten();
    // App calls reopen or close this panel, so no state borrow is held here.
    match action {
        Some(Action::Activate(id)) => app::activate_recording(&id),
        Some(Action::Star(id, on)) => app::star_recording(&id, on),
        Some(Action::Delete(id)) => app::delete_recording(&id),
        Some(Action::Rename(i)) => start_edit(i),
        Some(Action::Redraw) => invalidate(hwnd),
        None => {}
    }
}

/// The name line of row `i`, in logical pixels, while that row is visible.
fn name_rect(scroll: usize, i: usize) -> Option<(i32, i32, i32, i32)> {
    let j = i.checked_sub(scroll).filter(|j| *j < MAX_ROWS)? as i32;
    let y = HEAD_H + j * ROW_H;
    Some((PAD + STAR_W, y + 8, W - 2 * PAD - STAR_W - ICONS_W, 22))
}

fn start_edit(i: usize) {
    let Some((popup, k, font, name, (x, y, w, h))) = with(|s| {
        let rect = name_rect(s.scroll, i)?;
        let name = s.rows.get(i)?.name.clone();
        s.confirm = None;
        Some((
            s.hwnd,
            s.dpi as f32 / 96.0,
            s.fonts.get(Font::Strong),
            name,
            rect,
        ))
    })
    .flatten() else {
        return;
    };
    let class = wide("EDIT");
    let text = wide(&name);
    unsafe {
        let edit = CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
            px(x, k),
            px(y, k),
            px(w, k),
            px(h, k),
            popup,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if edit.is_null() {
            return;
        }
        SendMessageW(edit, WM_SETFONT, font as WPARAM, 0);
        SendMessageW(edit, EM_LIMITTEXT, NAME_MAX, 0);
        SendMessageW(edit, EM_SETSEL, 0, -1);
        let proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT = edit_proc;
        let old = SetWindowLongPtrW(edit, GWLP_WNDPROC, proc as usize as isize);
        EDIT_PROC.set(old);
        with(|s| s.edit = Some((edit, i)));
        SetFocus(edit);
    }
    invalidate(popup);
}

/// Ends an inline rename. The edit is taken out of state first, because
/// moving focus away from it reports a focus loss that lands here again.
fn commit_edit(save: bool) {
    let Some((popup, edit, id, old)) = with(|s| {
        let (edit, i) = s.edit.take()?;
        let row = s.rows.get(i)?;
        Some((s.hwnd, edit, row.id.clone(), row.name.clone()))
    })
    .flatten() else {
        return;
    };
    let name = save.then(|| unsafe {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buf[..n]).trim().to_string()
    });
    unsafe {
        SetFocus(popup);
        DestroyWindow(edit);
    }
    match name {
        Some(name) if !name.is_empty() && name != old => app::rename_recording(&id, &name),
        _ => invalidate(popup),
    }
}

// Drawing

fn paint(hwnd: HWND, hdc: HDC) {
    let mut rc: RECT = unsafe { zeroed() };
    unsafe { GetClientRect(hwnd, &mut rc) };
    let (cw, ch) = (rc.right, rc.bottom);
    unsafe {
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, cw, ch);
        let old = SelectObject(mem, bmp);
        with(|s| draw(s, mem));
        BitBlt(hdc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }
}

fn draw(s: &mut State, hdc: HDC) {
    let t = theme(s.dark);
    let k = s.dpi as f32 / 96.0;
    let hover = s.hover;
    let editing = s.edit.map(|(_, i)| i);
    let n = s.rows.len();
    let total_h = height(n);
    let single = DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS;
    let mut c: Canvas<'_, Hit> = Canvas {
        g: null_mut(),
        k,
        hdc,
        fonts: &s.fonts,
        texts: Vec::new(),
        hits: Vec::new(),
    };
    let pill_w = c.measure(Font::Caption, "Active") + 16;
    c.begin();

    c.round_rect(0, 0, W, total_h, 0.0, t.card, None);
    c.round_rect(0, 0, W, total_h, 8.0, t.card, Some((t.border, 1.0)));
    c.text(
        PAD + 12,
        0,
        200,
        HEAD_H,
        "Recordings",
        Font::Title,
        t.text,
        single,
    );
    if n > 0 {
        let count = format!("{n} saved");
        c.text(
            W - PAD - 12 - 160,
            0,
            160,
            HEAD_H,
            &count,
            Font::Small,
            t.sub,
            single | DT_RIGHT,
        );
    } else {
        c.text(
            0,
            HEAD_H,
            W,
            ROW_H,
            "No recordings yet.",
            Font::Body,
            t.sub,
            single | DT_CENTER,
        );
    }

    let (x0, w0) = (PAD, W - 2 * PAD);
    for (j, (i, row)) in s
        .rows
        .iter()
        .enumerate()
        .skip(s.scroll)
        .take(MAX_ROWS)
        .enumerate()
    {
        let y = HEAD_H + j as i32 * ROW_H;
        let cy = y + ROW_H / 2;
        if s.confirm == Some(i) {
            c.round_rect(x0, y + 2, w0, ROW_H - 4, 8.0, t.hover, None);
            c.text(
                x0 + 16,
                y,
                w0 - 190,
                ROW_H,
                "Delete this recording?",
                Font::Strong,
                t.text,
                single,
            );
            let (bw, bh) = (72, 30);
            let by = cy - bh / 2;
            let cancel_x = x0 + w0 - 12 - bw;
            let delete_x = cancel_x - 8 - bw;
            let red = if hover == Some(Hit::Confirm(i)) {
                lighten(RED, 0.12)
            } else {
                RED
            };
            c.round_rect(delete_x, by, bw, bh, 6.0, red, None);
            c.text(
                delete_x,
                by,
                bw,
                bh,
                "Delete",
                Font::Strong,
                WHITE,
                single | DT_CENTER,
            );
            c.hit(delete_x, by, bw, bh, Hit::Confirm(i));
            let chip = if hover == Some(Hit::Cancel) {
                t.hover
            } else {
                t.chip
            };
            c.round_rect(cancel_x, by, bw, bh, 6.0, chip, Some((t.chip_border, 1.0)));
            c.text(
                cancel_x,
                by,
                bw,
                bh,
                "Cancel",
                Font::Strong,
                t.text,
                single | DT_CENTER,
            );
            c.hit(cancel_x, by, bw, bh, Hit::Cancel);
            continue;
        }

        let row_hover = hover.and_then(Hit::row) == Some(i);
        let bg = if row_hover { t.hover } else { t.card };
        if row_hover {
            c.round_rect(x0, y + 2, w0, ROW_H - 4, 8.0, bg, None);
        }
        c.hit(x0, y, w0, ROW_H, Hit::Row(i));

        let (sx, sy) = ((x0 + STAR_W / 2) as f32, cy as f32);
        if row.starred {
            c.star(sx, sy, 8.5, GOLD);
        } else {
            c.star(
                sx,
                sy,
                8.5,
                if hover == Some(Hit::Star(i)) {
                    t.sub
                } else {
                    t.dim
                },
            );
            c.star(sx, sy, 5.8, bg);
        }
        c.hit(x0, y, STAR_W, ROW_H, Hit::Star(i));

        let (nx, ny, mut nw, nh) = (x0 + STAR_W, y + 8, w0 - STAR_W - ICONS_W, 22);
        if editing == Some(i) {
            c.round_rect(
                nx - 4,
                ny - 3,
                nw + 8,
                nh + 6,
                5.0,
                t.chip,
                Some((ACCENT, 1.0)),
            );
        } else {
            if row.active {
                let px_ = x0 + w0 - ICONS_W - pill_w;
                c.round_rect(px_, ny + 2, pill_w - 6, 18, 9.0, ACCENT, None);
                c.text(
                    px_,
                    ny + 2,
                    pill_w - 6,
                    18,
                    "Active",
                    Font::Caption,
                    WHITE,
                    single | DT_CENTER,
                );
                nw -= pill_w;
            }
            c.text(nx, ny, nw, nh, &row.name, Font::Strong, t.text, single);
        }
        c.text(
            nx,
            y + 30,
            w0 - STAR_W - ICONS_W,
            18,
            &row.detail,
            Font::Small,
            t.sub,
            single,
        );

        let pen_x = x0 + w0 - ICONS_W + 22;
        let bin_x = pen_x + 34;
        for (ix, hit) in [(pen_x, Hit::Rename(i)), (bin_x, Hit::Delete(i))] {
            let on = hover == Some(hit);
            if on {
                c.circle(ix as f32, cy as f32, 15.0, t.chip);
            }
            let color = if on {
                t.text
            } else if row_hover {
                t.sub
            } else {
                t.dim
            };
            match hit {
                Hit::Rename(_) => pencil(&c, ix as f32, cy as f32, color),
                _ => trash(&c, ix as f32, cy as f32, color),
            }
            c.hit(ix - 16, cy - 16, 32, 32, hit);
        }
    }

    if n > MAX_ROWS {
        let track_h = MAX_ROWS as i32 * ROW_H;
        let thumb_h = (track_h * MAX_ROWS as i32 / n as i32).max(24);
        let thumb_y = HEAD_H + (track_h - thumb_h) * s.scroll as i32 / max_scroll(n) as i32;
        c.round_rect(W - 6, thumb_y + 4, 3, thumb_h - 8, 1.5, t.dim, None);
    }

    c.end();
    let hits = c.hits;
    s.hits = hits;
}

fn pencil<H>(c: &Canvas<'_, H>, cx: f32, cy: f32, color: u32) {
    let d = std::f32::consts::FRAC_1_SQRT_2;
    let at = |a: f32, b: f32| (cx + d * a + d * b, cy - d * a + d * b);
    c.polygon(
        &[
            at(-7.5, 0.0),
            at(-4.5, 2.0),
            at(4.0, 2.0),
            at(4.0, -2.0),
            at(-4.5, -2.0),
        ],
        color,
    );
    c.polygon(
        &[at(5.4, 2.0), at(7.5, 2.0), at(7.5, -2.0), at(5.4, -2.0)],
        color,
    );
}

fn trash<H>(c: &Canvas<'_, H>, cx: f32, cy: f32, color: u32) {
    c.polygon(
        &[
            (cx - 6.5, cy - 5.5),
            (cx + 6.5, cy - 5.5),
            (cx + 6.5, cy - 4.0),
            (cx - 6.5, cy - 4.0),
        ],
        color,
    );
    c.polygon(
        &[
            (cx - 2.5, cy - 7.5),
            (cx + 2.5, cy - 7.5),
            (cx + 2.5, cy - 5.5),
            (cx - 2.5, cy - 5.5),
        ],
        color,
    );
    c.polygon(
        &[
            (cx - 5.0, cy - 3.0),
            (cx + 5.0, cy - 3.0),
            (cx + 4.0, cy + 7.0),
            (cx - 4.0, cy + 7.0),
        ],
        color,
    );
}

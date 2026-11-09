use crate::app::{self, NoticeKind, View};
use crate::engine::Mode;
use crate::shortcuts::{Mods, Which};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSControl,
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSEventMask, NSEventModifierFlags,
    NSEventType, NSFont, NSImage, NSLayoutConstraint, NSStackView, NSStackViewGravity, NSSwitch,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

const WIDTH: f64 = 460.0;
const MARGIN: f64 = 24.0;
const INNER: f64 = WIDTH - 2.0 * MARGIN;
const BOX_PAD: f64 = 14.0;
const DOT: f64 = 10.0;

const TAG_RECORD: NSInteger = 1;
const TAG_PLAY: NSInteger = 2;
const TAG_AUTOSTART: NSInteger = 3;
const TAG_AUTO_UPDATE: NSInteger = 4;
const TAG_RESET: NSInteger = 5;
const TAG_CHECK: NSInteger = 6;
const TAG_STAR: NSInteger = 7;
const TAG_LOOP: NSInteger = 8;
const TAG_RANDOM: NSInteger = 9;
const TAG_PERMISSION: NSInteger = 10;

const KVK_ESCAPE: u16 = 53;

struct Ui {
    window: Retained<NSWindow>,
    _target: Retained<Target>,
    version: Retained<NSTextField>,
    dot: Retained<NSBox>,
    status_title: Retained<NSTextField>,
    status_detail: Retained<NSTextField>,
    record: Retained<NSButton>,
    play: Retained<NSButton>,
    repeat: Retained<NSButton>,
    notice: Retained<NSTextField>,
    permission: Retained<NSButton>,
    autostart: Retained<NSSwitch>,
    auto_update: Retained<NSSwitch>,
    random: Retained<NSSwitch>,
    update_text: Retained<NSTextField>,
}

// Kept apart from UI so the key monitor and window delegate can read them
// while app code (which calls back into refresh) is running.
thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static CAPTURE: Cell<Option<Which>> = const { Cell::new(None) };
    static MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Target has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MouseKeysRecordReplaySettingsTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, sender: &NSControl) {
            on_click(sender);
        }
    }

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSWindowDelegate for Target {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            end_capture_on_focus_loss();
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _n: &NSNotification) {
            end_capture_on_focus_loss();
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn on_click(sender: &NSControl) {
    let tag = sender.tag();
    match tag {
        TAG_RECORD => app::begin_capture(Which::Record),
        TAG_PLAY => app::begin_capture(Which::Play),
        TAG_LOOP => app::begin_capture(Which::Loop),
        TAG_AUTOSTART | TAG_AUTO_UPDATE | TAG_RANDOM => {
            let state: NSInteger = unsafe { msg_send![sender, state] };
            let on = state == NSControlStateValueOn;
            match tag {
                TAG_AUTOSTART => app::set_autostart(on),
                TAG_AUTO_UPDATE => app::set_auto_update(on),
                _ => app::set_random_timing(on),
            }
        }
        TAG_RESET => app::reset_shortcuts(),
        TAG_CHECK => app::check_updates(),
        TAG_STAR => app::open_repo(),
        TAG_PERMISSION => {
            if let Some(pane) = crate::input::missing_pane() {
                app::open_url(pane);
            }
        }
        _ => {}
    }
}

fn end_capture_on_focus_loss() {
    if CAPTURE.get().is_some() {
        app::cancel_capture();
        refresh();
    }
}

pub fn open() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let exists = UI.with(|u| u.borrow().is_some());
    if !exists {
        let ui = build(mtm);
        UI.with(|u| *u.borrow_mut() = Some(ui));
    }
    refresh();
    UI.with(|u| {
        if let Some(ui) = u.borrow().as_ref() {
            ui.window.makeKeyAndOrderFront(None);
        }
    });
    let ns_app = NSApplication::sharedApplication(mtm);
    // activate exists from macOS 14; older systems only have the deprecated form.
    if ns_app.respondsToSelector(sel!(activate)) {
        ns_app.activate();
    } else {
        #[allow(deprecated)]
        ns_app.activateIgnoringOtherApps(true);
    }
}

pub fn close() {
    CAPTURE.set(None);
    remove_monitor();
    let ui = UI.with(|u| u.borrow_mut().take());
    if let Some(ui) = ui {
        ui.window.setDelegate(None);
        ui.window.orderOut(None);
    }
}

pub fn refresh() {
    let Some(v) = app::view() else { return };
    let shown = UI.with(|u| {
        let b = u.borrow();
        let Some(ui) = b.as_ref() else { return false };
        apply(ui, &v);
        true
    });
    CAPTURE.set(if shown { v.capturing } else { None });
    if shown && v.capturing.is_some() {
        install_monitor();
    } else {
        remove_monitor();
    }
}

fn apply(ui: &Ui, v: &View) {
    ui.version.setStringValue(&NSString::from_str(&format!("Version {}", v.version)));
    ui.dot.setFillColor(&*match v.mode {
        Mode::Idle => NSColor::systemGrayColor(),
        Mode::Recording => NSColor::systemRedColor(),
        Mode::Playing => NSColor::controlAccentColor(),
    });
    ui.status_title.setStringValue(&NSString::from_str(&v.status_title));
    ui.status_detail.setStringValue(&NSString::from_str(&v.status_detail));
    ui.status_detail.setHidden(v.status_detail.is_empty());

    let shortcut_title = |which: Which, label: &str| {
        if v.capturing == Some(which) {
            NSString::from_str("Press new shortcut...")
        } else {
            NSString::from_str(label)
        }
    };
    ui.record.setTitle(&shortcut_title(Which::Record, &v.record));
    ui.play.setTitle(&shortcut_title(Which::Play, &v.play));
    ui.repeat.setTitle(&shortcut_title(Which::Loop, &v.repeat));

    match &v.notice {
        Some(n) => {
            ui.notice.setStringValue(&NSString::from_str(&n.text));
            ui.notice.setTextColor(Some(&*match n.kind {
                NoticeKind::Error => NSColor::systemRedColor(),
                NoticeKind::Success => NSColor::systemGreenColor(),
            }));
            ui.notice.setHidden(false);
        }
        None => ui.notice.setHidden(true),
    }
    ui.permission.setHidden(crate::input::missing_pane().is_none());

    ui.autostart.setState(if v.autostart { NSControlStateValueOn } else { NSControlStateValueOff });
    ui.auto_update.setState(if v.auto_update { NSControlStateValueOn } else { NSControlStateValueOff });
    ui.random.setState(if v.random_timing { NSControlStateValueOn } else { NSControlStateValueOff });

    ui.update_text.setStringValue(&NSString::from_str(&v.update_text));
    ui.update_text.setHidden(v.update_text.is_empty());
}

fn install_monitor() {
    if MONITOR.with(|m| m.borrow().is_some()) {
        return;
    }
    let block = RcBlock::new(|ev: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { ev.as_ref() };
        if on_key(event) {
            std::ptr::null_mut()
        } else {
            ev.as_ptr()
        }
    });
    let mask = NSEventMask::KeyDown | NSEventMask::FlagsChanged;
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) };
    MONITOR.with(|m| *m.borrow_mut() = monitor);
}

fn remove_monitor() {
    let monitor = MONITOR.with(|m| m.borrow_mut().take());
    if let Some(monitor) = monitor {
        unsafe { NSEvent::removeMonitor(&monitor) };
    }
}

/// Returns true when the event is consumed by shortcut capture.
fn on_key(event: &NSEvent) -> bool {
    let Some(which) = CAPTURE.get() else { return false };
    if event.r#type() != NSEventType::KeyDown {
        return true;
    }
    let flags = event.modifierFlags();
    let mods = Mods {
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
        sup: flags.contains(NSEventModifierFlags::Command),
    };
    let key = event.keyCode();
    if key == KVK_ESCAPE && mods == Mods::default() {
        app::cancel_capture();
        refresh();
    } else if let Some(code) = code_name(key) {
        let _ = app::set_shortcut(which, mods, code);
        refresh();
    }
    true
}

/// macOS virtual key code to the W3C KeyboardEvent.code name.
fn code_name(key: u16) -> Option<&'static str> {
    Some(match key {
        0 => "KeyA",
        1 => "KeyS",
        2 => "KeyD",
        3 => "KeyF",
        4 => "KeyH",
        5 => "KeyG",
        6 => "KeyZ",
        7 => "KeyX",
        8 => "KeyC",
        9 => "KeyV",
        11 => "KeyB",
        12 => "KeyQ",
        13 => "KeyW",
        14 => "KeyE",
        15 => "KeyR",
        16 => "KeyY",
        17 => "KeyT",
        18 => "Digit1",
        19 => "Digit2",
        20 => "Digit3",
        21 => "Digit4",
        22 => "Digit6",
        23 => "Digit5",
        24 => "Equal",
        25 => "Digit9",
        26 => "Digit7",
        27 => "Minus",
        28 => "Digit8",
        29 => "Digit0",
        30 => "BracketRight",
        31 => "KeyO",
        32 => "KeyU",
        33 => "BracketLeft",
        34 => "KeyI",
        35 => "KeyP",
        36 => "Enter",
        37 => "KeyL",
        38 => "KeyJ",
        39 => "Quote",
        40 => "KeyK",
        41 => "Semicolon",
        42 => "Backslash",
        43 => "Comma",
        44 => "Slash",
        45 => "KeyN",
        46 => "KeyM",
        47 => "Period",
        48 => "Tab",
        49 => "Space",
        50 => "Backquote",
        51 => "Backspace",
        53 => "Escape",
        64 => "F17",
        65 => "NumpadDecimal",
        67 => "NumpadMultiply",
        69 => "NumpadAdd",
        75 => "NumpadDivide",
        76 => "NumpadEnter",
        78 => "NumpadSubtract",
        79 => "F18",
        80 => "F19",
        82 => "Numpad0",
        83 => "Numpad1",
        84 => "Numpad2",
        85 => "Numpad3",
        86 => "Numpad4",
        87 => "Numpad5",
        88 => "Numpad6",
        89 => "Numpad7",
        90 => "F20",
        91 => "Numpad8",
        92 => "Numpad9",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        103 => "F11",
        105 => "F13",
        106 => "F16",
        107 => "F14",
        109 => "F10",
        111 => "F12",
        113 => "F15",
        115 => "Home",
        116 => "PageUp",
        117 => "Delete",
        118 => "F4",
        119 => "End",
        120 => "F2",
        121 => "PageDown",
        122 => "F1",
        123 => "ArrowLeft",
        124 => "ArrowRight",
        125 => "ArrowDown",
        126 => "ArrowUp",
        _ => return None,
    })
}

// Layout

fn build(mtm: MainThreadMarker) -> Ui {
    let target = Target::new(mtm);
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, 520.0));
    let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect,
            style,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // The window outlives each close; the red button only hides it.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("Mouse Keys Record Replay"));
    window.setDelegate(Some(ProtocolObject::from_ref(&*target)));

    let content = NSView::initWithFrame(NSView::alloc(mtm), rect);
    let root = vstack(mtm, 14.0);
    content.addSubview(&root);
    pin(&root, &content, MARGIN);
    root.widthAnchor().constraintEqualToConstant(INNER).setActive(true);

    // Header
    let title = label(mtm, "Mouse Keys Record Replay", &NSFont::boldSystemFontOfSize(20.0), None);
    let version = label(mtm, "", &NSFont::systemFontOfSize(NSFont::systemFontSize()), Some(&NSColor::secondaryLabelColor()));
    let header = vstack(mtm, 2.0);
    header.addArrangedSubview(&title);
    header.addArrangedSubview(&version);
    root.addArrangedSubview(&header);

    // Status
    let status_box = card(mtm);
    let dot = NSBox::new(mtm);
    dot.setBoxType(NSBoxType::Custom);
    dot.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    dot.setBorderWidth(0.0);
    dot.setCornerRadius(DOT / 2.0);
    dot.setTranslatesAutoresizingMaskIntoConstraints(false);
    dot.widthAnchor().constraintEqualToConstant(DOT).setActive(true);
    dot.heightAnchor().constraintEqualToConstant(DOT).setActive(true);
    let status_title = label(mtm, "", &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()), None);
    let status_detail = wrapping(mtm, "", INNER - 2.0 * BOX_PAD - DOT - 10.0, NSFont::systemFontSize());
    let title_row = hstack(mtm, 8.0);
    title_row.addArrangedSubview(&dot);
    title_row.addArrangedSubview(&status_title);
    let status_inner = vstack(mtm, 4.0);
    status_inner.addArrangedSubview(&title_row);
    status_inner.addArrangedSubview(&status_detail);
    status_box.addSubview(&status_inner);
    pin(&status_inner, &status_box, BOX_PAD);
    root.addArrangedSubview(&status_box);
    fill_width(&status_box, &root);

    // Shortcuts
    let section = label(mtm, "Shortcuts", &NSFont::boldSystemFontOfSize(NSFont::systemFontSize()), None);
    root.addArrangedSubview(&section);
    root.setCustomSpacing_afterView(8.0, &section);
    let record = button(mtm, &target, "", TAG_RECORD);
    let play = button(mtm, &target, "", TAG_PLAY);
    let repeat = button(mtm, &target, "", TAG_LOOP);
    for b in [&record, &play, &repeat] {
        b.widthAnchor().constraintGreaterThanOrEqualToConstant(180.0).setActive(true);
    }
    let shortcuts = vstack(mtm, 8.0);
    for (text, b) in [("Record / stop recording", &record), ("Play once / stop playback", &play), ("Loop / stop loop", &repeat)] {
        let row = row(mtm, &label(mtm, text, &NSFont::systemFontOfSize(NSFont::systemFontSize()), None), b);
        shortcuts.addArrangedSubview(&row);
        fill_width(&row, &shortcuts);
    }
    root.addArrangedSubview(&shortcuts);
    fill_width(&shortcuts, &root);

    let notice = wrapping(mtm, "", INNER, NSFont::systemFontSize());
    notice.setHidden(true);
    root.addArrangedSubview(&notice);

    let permission = button(mtm, &target, "Open System Settings", TAG_PERMISSION);
    permission.setHidden(true);
    root.addArrangedSubview(&permission);

    let tip = wrapping(mtm, "During playback, move the mouse or press any key to take back control.", INNER, NSFont::smallSystemFontSize());
    root.addArrangedSubview(&tip);

    // Options
    let autostart = switch(mtm, &target, TAG_AUTOSTART);
    let auto_update = switch(mtm, &target, TAG_AUTO_UPDATE);
    let random = switch(mtm, &target, TAG_RANDOM);
    let options = vstack(mtm, 10.0);
    for (text, s) in [("Start when I log in", &autostart), ("Install updates automatically", &auto_update), ("Random timing (each press up to half a second early or late)", &random)] {
        let row = row(mtm, &label(mtm, text, &NSFont::systemFontOfSize(NSFont::systemFontSize()), None), s);
        options.addArrangedSubview(&row);
        fill_width(&row, &options);
    }
    root.addArrangedSubview(&options);
    fill_width(&options, &root);

    // Footer
    let reset = button(mtm, &target, "Reset to defaults", TAG_RESET);
    let check = button(mtm, &target, "Check for updates", TAG_CHECK);
    let star = button(mtm, &target, "Star on GitHub", TAG_STAR);
    if let Some(img) = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("star"), None) {
        star.setImage(Some(&img));
        star.setImagePosition(objc2_app_kit::NSCellImagePosition::ImageLeading);
    }
    let footer = hstack(mtm, 8.0);
    footer.addView_inGravity(&reset, NSStackViewGravity::Leading);
    footer.addView_inGravity(&check, NSStackViewGravity::Leading);
    footer.addView_inGravity(&star, NSStackViewGravity::Trailing);
    root.setCustomSpacing_afterView(22.0, &options);
    root.addArrangedSubview(&footer);
    fill_width(&footer, &root);

    let update_text = wrapping(mtm, "", INNER, NSFont::smallSystemFontSize());
    update_text.setHidden(true);
    root.addArrangedSubview(&update_text);

    window.setContentView(Some(&content));
    window.center();

    Ui {
        window,
        _target: target,
        version,
        dot,
        status_title,
        status_detail,
        record,
        play,
        repeat,
        notice,
        permission,
        autostart,
        auto_update,
        random,
        update_text,
    }
}

fn vstack(mtm: MainThreadMarker, spacing: f64) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    s.setAlignment(objc2_app_kit::NSLayoutAttribute::Leading);
    s.setSpacing(spacing);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

fn hstack(mtm: MainThreadMarker, spacing: f64) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    s.setAlignment(objc2_app_kit::NSLayoutAttribute::CenterY);
    s.setSpacing(spacing);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

/// Label on the leading edge, control on the trailing edge.
fn row(mtm: MainThreadMarker, text: &NSView, control: &NSView) -> Retained<NSStackView> {
    let r = hstack(mtm, 12.0);
    r.addView_inGravity(text, NSStackViewGravity::Leading);
    r.addView_inGravity(control, NSStackViewGravity::Trailing);
    r
}

fn card(mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
    b.setCornerRadius(10.0);
    b.setBorderWidth(1.0);
    b.setBorderColor(&NSColor::separatorColor());
    b.setFillColor(&NSColor::controlBackgroundColor());
    b.setContentViewMargins(NSSize::new(0.0, 0.0));
    b.setTranslatesAutoresizingMaskIntoConstraints(false);
    b
}

fn label(mtm: MainThreadMarker, text: &str, font: &NSFont, color: Option<&NSColor>) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(font));
    if let Some(c) = color {
        l.setTextColor(Some(c));
    }
    l
}

fn wrapping(mtm: MainThreadMarker, text: &str, width: f64, size: f64) -> Retained<NSTextField> {
    let l = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    l.setSelectable(false);
    l.setFont(Some(&NSFont::systemFontOfSize(size)));
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l.setPreferredMaxLayoutWidth(width);
    l
}

fn button(mtm: MainThreadMarker, target: &Target, title: &str, tag: NSInteger) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(title), Some(target), Some(sel!(clicked:)), mtm)
    };
    b.setTag(tag);
    b
}

fn switch(mtm: MainThreadMarker, target: &Target, tag: NSInteger) -> Retained<NSSwitch> {
    let s = NSSwitch::new(mtm);
    unsafe {
        s.setTarget(Some(target));
        s.setAction(Some(sel!(clicked:)));
    }
    s.setTag(tag);
    s
}

fn pin(child: &NSView, parent: &NSView, inset: f64) {
    let constraints = [
        child.topAnchor().constraintEqualToAnchor_constant(&parent.topAnchor(), inset),
        child.leadingAnchor().constraintEqualToAnchor_constant(&parent.leadingAnchor(), inset),
        parent.trailingAnchor().constraintEqualToAnchor_constant(&child.trailingAnchor(), inset),
        parent.bottomAnchor().constraintEqualToAnchor_constant(&child.bottomAnchor(), inset),
    ];
    for c in constraints {
        c.setActive(true);
    }
}

fn fill_width(child: &NSView, parent: &NSView) {
    let c: Retained<NSLayoutConstraint> = child.widthAnchor().constraintEqualToAnchor(&parent.widthAnchor());
    c.setActive(true);
}

//! Recordings panel for macOS: a borderless key panel with a frosted card,
//! rebuilt from the rows on every change. It closes when it stops being key.

use super::Row;
use crate::app;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSButton, NSClickGestureRecognizer,
    NSColor, NSControl, NSEvent, NSEventMask, NSFont, NSFontWeightSemibold, NSImage,
    NSImageSymbolConfiguration, NSLayoutAttribute, NSLayoutConstraintOrientation, NSPanel,
    NSResponder, NSScreen, NSScrollView, NSStackView, NSStackViewDistribution, NSStatusWindowLevel,
    NSTextField, NSTitlePosition, NSUserInterfaceLayoutOrientation, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindow, NSWindowCollectionBehavior, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSNotification, NSObject, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ptr::NonNull;

const W: f64 = 420.0;
const HEAD_H: f64 = 48.0;
const ROW_H: f64 = 54.0;
const PAD: f64 = 8.0;
const TOP_GAP: f64 = 12.0;
const RADIUS: f64 = 14.0;
const MAX_ROWS: usize = 7;
const NAME_MAX: usize = 80;
const KVK_ESCAPE: u16 = 53;

// Button tags carry the row index and the action.
const KIND_STAR: NSInteger = 1;
const KIND_RENAME: NSInteger = 2;
const KIND_DELETE: NSInteger = 3;
const KIND_CONFIRM: NSInteger = 4;
const KIND_CANCEL: NSInteger = 5;
const KIND_EDIT: NSInteger = 6;

fn tag(i: usize, kind: NSInteger) -> NSInteger {
    i as NSInteger * 8 + kind
}

define_class!(
    // SAFETY: a borderless panel only needs to accept key status.
    #[unsafe(super(NSPanel, NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MouseKeysRecordReplayRecordingsPanel"]
    struct KeyPanel;

    impl KeyPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }
    }
);

define_class!(
    // SAFETY: a flipped view only overrides isFlipped; it keeps the list
    // scrolled to its first row instead of the bottom.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MouseKeysRecordReplayRecordingsList"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Target has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MouseKeysRecordReplayRecordingsTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(clicked:))]
        fn clicked(&self, sender: &NSControl) {
            let t = sender.tag();
            on_action((t / 8) as usize, t % 8);
        }

        #[unsafe(method(rowClicked:))]
        fn row_clicked(&self, sender: &NSClickGestureRecognizer) {
            on_row(sender);
        }
    }

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSWindowDelegate for Target {
        #[unsafe(method(windowDidResignKey:))]
        fn window_did_resign_key(&self, _n: &NSNotification) {
            if is_open() {
                commit_edit();
                close();
            }
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

struct State {
    panel: Retained<KeyPanel>,
    target: Retained<Target>,
    open: bool,
    rows: Vec<Row>,
    confirm: Option<usize>,
    editing: Option<(usize, Retained<NSTextField>)>,
    clicks: Vec<(Retained<NSClickGestureRecognizer>, usize)>,
    // The previous content stays alive until the next rebuild, because the
    // button that triggered a rebuild is still running its action.
    previous: Option<Retained<NSView>>,
    monitor: Option<Retained<AnyObject>>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok()?.as_mut().map(f))
}

pub fn is_open() -> bool {
    with(|s| s.open).unwrap_or(false)
}

pub fn show(rows: Vec<Row>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if STATE.with(|s| s.borrow().is_none()) {
        let state = create(mtm);
        STATE.with(|s| *s.borrow_mut() = Some(state));
    }
    let opening = with(|s| {
        s.rows = rows;
        s.confirm = None;
        s.editing = None;
        !s.open
    })
    .unwrap_or(false);
    rebuild(mtm);
    if opening {
        place(mtm);
        install_monitor();
        let panel = with(|s| {
            s.open = true;
            s.panel.clone()
        });
        if let Some(panel) = panel {
            let ns_app = NSApplication::sharedApplication(mtm);
            // activate exists from macOS 14; older systems only have the deprecated form.
            if ns_app.respondsToSelector(sel!(activate)) {
                ns_app.activate();
            } else {
                #[allow(deprecated)]
                ns_app.activateIgnoringOtherApps(true);
            }
            panel.makeKeyAndOrderFront(None);
        }
    }
}

pub fn close() {
    let panel = with(|s| {
        s.open = false;
        s.confirm = None;
        s.editing = None;
        s.panel.clone()
    });
    remove_monitor();
    if let Some(panel) = panel {
        panel.orderOut(None);
    }
}

fn create(mtm: MainThreadMarker) -> State {
    let target = Target::new(mtm);
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(W, 200.0));
    let this = KeyPanel::alloc(mtm).set_ivars(());
    let panel: Retained<KeyPanel> = unsafe {
        msg_send![super(this), initWithContentRect: rect, styleMask: NSWindowStyleMask::Borderless, backing: NSBackingStoreType::Buffered, defer: false]
    };
    unsafe { panel.setReleasedWhenClosed(false) };
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    panel.setHasShadow(true);
    panel.setHidesOnDeactivate(false);
    panel.setLevel(NSStatusWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    State {
        panel,
        target,
        open: false,
        rows: Vec::new(),
        confirm: None,
        editing: None,
        clicks: Vec::new(),
        previous: None,
        monitor: None,
    }
}

/// Moves the panel to the top center of the display under the pointer.
fn place(mtm: MainThreadMarker) {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| {
            let f = s.frame();
            mouse.x >= f.origin.x
                && mouse.x < f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y < f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else { return };
    with(|s| {
        let size = s.panel.frame().size;
        let area = screen.visibleFrame();
        let origin = NSPoint::new(
            (area.origin.x + (area.size.width - size.width) / 2.0).round(),
            (area.origin.y + area.size.height - size.height - TOP_GAP).round(),
        );
        s.panel.setFrameOrigin(origin);
    });
}

fn height(rows: usize) -> f64 {
    HEAD_H + rows.clamp(1, MAX_ROWS) as f64 * ROW_H + PAD
}

fn rebuild(mtm: MainThreadMarker) {
    with(|s| {
        s.clicks.clear();
        let content = content(mtm, s);
        let frame = s.panel.frame();
        let h = height(s.rows.len());
        // Keep the top edge where it is while the height changes.
        let top = frame.origin.y + frame.size.height;
        s.panel.setFrame_display(
            NSRect::new(NSPoint::new(frame.origin.x, top - h), NSSize::new(W, h)),
            true,
        );
        s.previous = s.panel.contentView();
        s.panel.setContentView(Some(&content));
        s.panel.invalidateShadow();
        if let Some((_, field)) = &s.editing {
            s.panel.makeFirstResponder(Some(field));
        }
    });
}

fn content(mtm: MainThreadMarker, s: &mut State) -> Retained<NSView> {
    let card = NSVisualEffectView::new(mtm);
    card.setMaterial(NSVisualEffectMaterial::Popover);
    card.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    card.setState(NSVisualEffectState::Active);
    card.setWantsLayer(true);
    unsafe {
        let layer: *mut AnyObject = msg_send![&*card, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: RADIUS];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }

    let title = label(
        mtm,
        "Recordings",
        &NSFont::boldSystemFontOfSize(17.0),
        &NSColor::labelColor(),
    );
    let head = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.0);
    head.addArrangedSubview(&title);
    let spacer = NSView::new(mtm);
    head.addArrangedSubview(&spacer);
    if !s.rows.is_empty() {
        let count = label(
            mtm,
            &format!("{} saved", s.rows.len()),
            &NSFont::systemFontOfSize(12.0),
            &NSColor::secondaryLabelColor(),
        );
        head.addArrangedSubview(&count);
    }
    card.addSubview(&head);

    let list = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 0.0);
    list.setAlignment(NSLayoutAttribute::Leading);
    if s.rows.is_empty() {
        let empty = label(
            mtm,
            "No recordings yet.",
            &NSFont::systemFontOfSize(13.0),
            &NSColor::secondaryLabelColor(),
        );
        list.addArrangedSubview(&empty);
    }
    for i in 0..s.rows.len() {
        let row = row(mtm, s, i);
        list.addArrangedSubview(&row);
        row.widthAnchor()
            .constraintEqualToConstant(W - 2.0 * PAD)
            .setActive(true);
        row.heightAnchor()
            .constraintEqualToConstant(ROW_H)
            .setActive(true);
    }

    let scroll = NSScrollView::new(mtm);
    scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
    scroll.setDrawsBackground(false);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    let this = FlippedView::alloc(mtm).set_ivars(());
    let doc: Retained<FlippedView> = unsafe { msg_send![super(this), init] };
    doc.setTranslatesAutoresizingMaskIntoConstraints(false);
    doc.addSubview(&list);
    scroll.setDocumentView(Some(&doc));
    card.addSubview(&scroll);

    let clip = scroll.contentView();
    let visible = s.rows.len().clamp(1, MAX_ROWS) as f64 * ROW_H;
    let constraints = [
        head.topAnchor().constraintEqualToAnchor(&card.topAnchor()),
        head.heightAnchor().constraintEqualToConstant(HEAD_H),
        head.leadingAnchor()
            .constraintEqualToAnchor_constant(&card.leadingAnchor(), PAD + 12.0),
        card.trailingAnchor()
            .constraintEqualToAnchor_constant(&head.trailingAnchor(), PAD + 12.0),
        scroll
            .topAnchor()
            .constraintEqualToAnchor_constant(&card.topAnchor(), HEAD_H),
        scroll
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&card.leadingAnchor(), PAD),
        card.trailingAnchor()
            .constraintEqualToAnchor_constant(&scroll.trailingAnchor(), PAD),
        scroll.heightAnchor().constraintEqualToConstant(visible),
        doc.topAnchor().constraintEqualToAnchor(&clip.topAnchor()),
        doc.leadingAnchor()
            .constraintEqualToAnchor(&clip.leadingAnchor()),
        doc.widthAnchor()
            .constraintEqualToAnchor(&clip.widthAnchor()),
        list.topAnchor().constraintEqualToAnchor(&doc.topAnchor()),
        list.bottomAnchor()
            .constraintEqualToAnchor(&doc.bottomAnchor()),
        list.leadingAnchor()
            .constraintEqualToAnchor(&doc.leadingAnchor()),
        list.widthAnchor()
            .constraintEqualToAnchor(&doc.widthAnchor()),
    ];
    for c in constraints {
        c.setActive(true);
    }
    card.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(W, height(s.rows.len())),
    ));
    card.layoutSubtreeIfNeeded();
    doc.scrollPoint(NSPoint::new(0.0, 0.0));
    Retained::into_super(card)
}

fn row(mtm: MainThreadMarker, s: &mut State, i: usize) -> Retained<NSView> {
    let r = s.rows[i].clone();
    let line = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 8.0);
    // Fill lets the text take the free width, so the icons line up in a column.
    line.setDistribution(NSStackViewDistribution::Fill);
    line.setEdgeInsets(objc2_foundation::NSEdgeInsets {
        top: 0.0,
        left: 6.0,
        bottom: 0.0,
        right: 6.0,
    });

    if s.confirm == Some(i) {
        let ask = label(
            mtm,
            "Delete this recording?",
            &semibold(13.0),
            &NSColor::labelColor(),
        );
        line.addArrangedSubview(&ask);
        let gap = NSView::new(mtm);
        gap.setContentHuggingPriority_forOrientation(
            1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        line.addArrangedSubview(&gap);
        let delete = text_button(mtm, &s.target, "Delete", tag(i, KIND_CONFIRM));
        delete.setBezelColor(Some(&NSColor::systemRedColor()));
        line.addArrangedSubview(&delete);
        let cancel = text_button(mtm, &s.target, "Cancel", tag(i, KIND_CANCEL));
        line.addArrangedSubview(&cancel);
        return Retained::into_super(line);
    }

    let star_color = if r.starred {
        NSColor::systemYellowColor()
    } else {
        NSColor::tertiaryLabelColor()
    };
    let star = icon_button(
        mtm,
        &s.target,
        if r.starred { "star.fill" } else { "star" },
        tag(i, KIND_STAR),
        &star_color,
    );
    line.addArrangedSubview(&star);

    let text = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 3.0);
    let editing = s.editing.as_ref().is_some_and(|(e, _)| *e == i);
    if editing {
        let field = NSTextField::textFieldWithString(&NSString::from_str(&r.name), mtm);
        field.setFont(Some(&semibold(13.0)));
        unsafe {
            field.setTarget(Some(&s.target));
            field.setAction(Some(sel!(clicked:)));
        }
        field.setTag(tag(i, KIND_EDIT));
        field
            .widthAnchor()
            .constraintEqualToConstant(260.0)
            .setActive(true);
        text.addArrangedSubview(&field);
        s.editing = Some((i, field));
    } else {
        let name = label(mtm, &r.name, &semibold(13.0), &NSColor::labelColor());
        text.addArrangedSubview(&name);
    }
    let detail = label(
        mtm,
        &r.detail,
        &NSFont::systemFontOfSize(11.5),
        &NSColor::secondaryLabelColor(),
    );
    text.addArrangedSubview(&detail);
    // The text area takes the free width, so a click on it picks the row.
    text.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
    if !editing {
        let click = unsafe {
            NSClickGestureRecognizer::initWithTarget_action(
                NSClickGestureRecognizer::alloc(mtm),
                Some(&s.target),
                Some(sel!(rowClicked:)),
            )
        };
        text.addGestureRecognizer(&click);
        s.clicks.push((click, i));
    }
    line.addArrangedSubview(&text);

    if r.active {
        line.addArrangedSubview(&pill(mtm));
    }
    let gray = NSColor::secondaryLabelColor();
    line.addArrangedSubview(&icon_button(
        mtm,
        &s.target,
        "pencil",
        tag(i, KIND_RENAME),
        &gray,
    ));
    line.addArrangedSubview(&icon_button(
        mtm,
        &s.target,
        "trash",
        tag(i, KIND_DELETE),
        &gray,
    ));
    Retained::into_super(line)
}

enum Action {
    Activate(String),
    Star(String, bool),
    Delete(String),
    Rename(String, String),
    Redraw,
}

fn on_row(sender: &NSClickGestureRecognizer) {
    let i = with(|s| {
        s.clicks
            .iter()
            .find(|(c, _)| std::ptr::eq(&**c, sender))
            .map(|(_, i)| *i)
    })
    .flatten();
    if let Some(i) = i {
        on_action(i, 0);
    }
}

fn on_action(i: usize, kind: NSInteger) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if kind != KIND_EDIT && with(|s| s.editing.is_some()).unwrap_or(false) {
        commit_edit();
        return;
    }
    let action = with(|s| {
        let r = s.rows.get(i)?;
        Some(match kind {
            0 => Action::Activate(r.id.clone()),
            KIND_STAR => Action::Star(r.id.clone(), !r.starred),
            KIND_RENAME => {
                s.confirm = None;
                s.editing = Some((i, NSTextField::new(mtm)));
                Action::Redraw
            }
            KIND_DELETE => {
                s.confirm = Some(i);
                Action::Redraw
            }
            KIND_CONFIRM => {
                s.confirm = None;
                Action::Delete(r.id.clone())
            }
            KIND_CANCEL => {
                s.confirm = None;
                Action::Redraw
            }
            KIND_EDIT => {
                let (_, field) = s.editing.take()?;
                let name: String = field
                    .stringValue()
                    .to_string()
                    .trim()
                    .chars()
                    .take(NAME_MAX)
                    .collect();
                if name.is_empty() || name == r.name {
                    Action::Redraw
                } else {
                    Action::Rename(r.id.clone(), name)
                }
            }
            _ => return None,
        })
    })
    .flatten();
    // App calls reopen or close this panel, so no state borrow is held here.
    match action {
        Some(Action::Activate(id)) => app::activate_recording(&id),
        Some(Action::Star(id, on)) => app::star_recording(&id, on),
        Some(Action::Delete(id)) => app::delete_recording(&id),
        Some(Action::Rename(id, name)) => app::rename_recording(&id, &name),
        Some(Action::Redraw) => rebuild(mtm),
        None => {}
    }
}

fn commit_edit() {
    if let Some(i) = with(|s| s.editing.as_ref().map(|(i, _)| *i)).flatten() {
        on_action(i, KIND_EDIT);
    }
}

/// Esc leaves an open rename or delete question first, then closes.
fn escape() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let backed_out =
        with(|s| s.editing.take().is_some() | s.confirm.take().is_some()).unwrap_or(false);
    if backed_out {
        rebuild(mtm);
    } else {
        close();
    }
}

fn install_monitor() {
    let block = RcBlock::new(|ev: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { ev.as_ref() };
        if is_open() && event.keyCode() == KVK_ESCAPE {
            escape();
            std::ptr::null_mut()
        } else {
            ev.as_ptr()
        }
    });
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
    };
    let old = with(|s| std::mem::replace(&mut s.monitor, monitor)).flatten();
    if let Some(old) = old {
        unsafe { NSEvent::removeMonitor(&old) };
    }
}

fn remove_monitor() {
    if let Some(monitor) = with(|s| s.monitor.take()).flatten() {
        unsafe { NSEvent::removeMonitor(&monitor) };
    }
}

// Views

fn semibold(size: f64) -> Retained<NSFont> {
    NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightSemibold })
}

fn label(
    mtm: MainThreadMarker,
    text: &str,
    font: &NSFont,
    color: &NSColor,
) -> Retained<NSTextField> {
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(font));
    l.setTextColor(Some(color));
    l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
    l.setContentCompressionResistancePriority_forOrientation(
        100.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    l
}

fn icon_button(
    mtm: MainThreadMarker,
    target: &Target,
    symbol: &str,
    tag: NSInteger,
    tint: &NSColor,
) -> Retained<NSButton> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        None,
    )
    .unwrap_or_default();
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(14.0, unsafe {
        NSFontWeightSemibold
    });
    let image = image.imageWithSymbolConfiguration(&config).unwrap_or(image);
    let b = unsafe {
        NSButton::buttonWithImage_target_action(&image, Some(target), Some(sel!(clicked:)), mtm)
    };
    b.setBordered(false);
    b.setContentTintColor(Some(tint));
    b.setTag(tag);
    b.widthAnchor()
        .constraintEqualToConstant(28.0)
        .setActive(true);
    b
}

fn text_button(
    mtm: MainThreadMarker,
    target: &Target,
    title: &str,
    tag: NSInteger,
) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(sel!(clicked:)),
            mtm,
        )
    };
    b.setTag(tag);
    b
}

/// The small accent label on the active recording.
fn pill(mtm: MainThreadMarker) -> Retained<NSView> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setCornerRadius(9.0);
    b.setBorderWidth(0.0);
    b.setFillColor(&NSColor::controlAccentColor());
    b.setContentViewMargins(NSSize::new(8.0, 1.0));
    let l = NSTextField::labelWithString(&NSString::from_str("Active"), mtm);
    l.setFont(Some(&semibold(11.0)));
    l.setTextColor(Some(&NSColor::whiteColor()));
    // A box has no intrinsic size, so pin it to the label or the row stretches it.
    let size = l.intrinsicContentSize();
    b.setContentView(Some(&l));
    b.widthAnchor()
        .constraintEqualToConstant(size.width + 16.0)
        .setActive(true);
    b.heightAnchor()
        .constraintEqualToConstant(size.height + 2.0)
        .setActive(true);
    Retained::into_super(b)
}

fn stack(
    mtm: MainThreadMarker,
    orientation: NSUserInterfaceLayoutOrientation,
    spacing: f64,
) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(orientation);
    s.setAlignment(
        if orientation == NSUserInterfaceLayoutOrientation::Vertical {
            NSLayoutAttribute::Leading
        } else {
            NSLayoutAttribute::CenterY
        },
    );
    s.setSpacing(spacing);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

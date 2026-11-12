use super::{Hud, HudKind};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimationContext, NSBackingStoreType, NSBox, NSBoxType, NSColor, NSFont, NSFontWeightBold,
    NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSImageView, NSPanel, NSScreen,
    NSStackView, NSStatusWindowLevel, NSTextField, NSTitlePosition,
    NSUserInterfaceLayoutOrientation, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;
use std::ptr::NonNull;

const TOP_GAP: f64 = 12.0;
const PAD_X: f64 = 18.0;
const PAD_Y: f64 = 12.0;
const RADIUS: f64 = 14.0;
const ICON: f64 = 22.0;
const FADE_IN: f64 = 0.18;
const FADE_OUT: f64 = 0.22;

struct State {
    hud: Option<Hud>,
    panels: Vec<Retained<NSPanel>>,
}

thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State { hud: None, panels: Vec::new() }) };
}

pub fn show(hud: Option<Hud>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    STATE.with(|cell| {
        let mut s = cell.borrow_mut();
        if s.hud == hud {
            return;
        }
        let old = std::mem::take(&mut s.panels);
        let fresh = s.hud.is_none();
        s.hud = hud.clone();
        if let Some(h) = &hud {
            s.panels = NSScreen::screens(mtm)
                .iter()
                .map(|screen| panel(mtm, &screen, h))
                .collect();
            for p in &s.panels {
                if fresh {
                    p.setAlphaValue(0.0);
                }
                p.orderFrontRegardless();
                if fresh {
                    fade(p, 1.0, FADE_IN, false);
                }
            }
            for p in old {
                p.orderOut(None);
            }
        } else {
            for p in old {
                fade(&p, 0.0, FADE_OUT, true);
            }
        }
    });
}

fn fade(panel: &Retained<NSPanel>, to: f64, duration: f64, then_hide: bool) {
    let target = panel.clone();
    let changes = RcBlock::new(move |ctx: NonNull<NSAnimationContext>| {
        unsafe { ctx.as_ref() }.setDuration(duration);
        let animator: Retained<AnyObject> = unsafe { msg_send![&*target, animator] };
        let _: () = unsafe { msg_send![&*animator, setAlphaValue: to] };
    });
    let keep = panel.clone();
    let done = RcBlock::new(move || {
        if then_hide {
            keep.orderOut(None);
        }
    });
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&done));
}

fn panel(mtm: MainThreadMarker, screen: &NSScreen, hud: &Hud) -> Retained<NSPanel> {
    let content = content(mtm, hud);
    let size = content.fittingSize();
    let size = NSSize::new(size.width.ceil(), size.height.ceil());
    let area = screen.visibleFrame();
    let origin = NSPoint::new(
        (area.origin.x + (area.size.width - size.width) / 2.0).round(),
        (area.origin.y + area.size.height - size.height - TOP_GAP).round(),
    );
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
    let p = NSPanel::initWithContentRect_styleMask_backing_defer(
        NSPanel::alloc(mtm),
        NSRect::new(origin, size),
        style,
        NSBackingStoreType::Buffered,
        false,
    );
    unsafe { p.setReleasedWhenClosed(false) };
    p.setOpaque(false);
    p.setBackgroundColor(Some(&NSColor::clearColor()));
    p.setHasShadow(true);
    p.setIgnoresMouseEvents(true);
    p.setHidesOnDeactivate(false);
    p.setLevel(NSStatusWindowLevel);
    p.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    p.setContentView(Some(&content));
    p.invalidateShadow();
    p
}

/// Frosted card holding the icon, the title and the instruction line.
fn content(mtm: MainThreadMarker, hud: &Hud) -> Retained<NSView> {
    let card = NSVisualEffectView::new(mtm);
    card.setMaterial(NSVisualEffectMaterial::HUDWindow);
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

    let row = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 12.0);
    row.addArrangedSubview(&icon(mtm, hud.kind));

    let text = stack(mtm, NSUserInterfaceLayoutOrientation::Vertical, 3.0);
    let title = NSTextField::labelWithString(&NSString::from_str(&hud.title), mtm);
    title.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe {
        NSFontWeightBold
    })));
    title.setTextColor(Some(&NSColor::labelColor()));
    text.addArrangedSubview(&title);

    let line = stack(mtm, NSUserInterfaceLayoutOrientation::Horizontal, 4.0);
    for (run, is_key) in &hud.line {
        if *is_key {
            line.addArrangedSubview(&chip(mtm, run));
        } else {
            let l = NSTextField::labelWithString(&NSString::from_str(run.trim()), mtm);
            l.setFont(Some(&NSFont::systemFontOfSize(12.0)));
            l.setTextColor(Some(&NSColor::secondaryLabelColor()));
            line.addArrangedSubview(&l);
        }
    }
    text.addArrangedSubview(&line);
    row.addArrangedSubview(&text);

    card.addSubview(&row);
    let constraints = [
        row.topAnchor()
            .constraintEqualToAnchor_constant(&card.topAnchor(), PAD_Y),
        row.leadingAnchor()
            .constraintEqualToAnchor_constant(&card.leadingAnchor(), PAD_X),
        card.trailingAnchor()
            .constraintEqualToAnchor_constant(&row.trailingAnchor(), PAD_X + 4.0),
        card.bottomAnchor()
            .constraintEqualToAnchor_constant(&row.bottomAnchor(), PAD_Y),
    ];
    for c in constraints {
        c.setActive(true);
    }
    card.layoutSubtreeIfNeeded();
    Retained::into_super(card)
}

fn icon(mtm: MainThreadMarker, kind: HudKind) -> Retained<NSView> {
    let (name, color) = match kind {
        HudKind::Recording => ("record.circle.fill", NSColor::systemRedColor()),
        HudKind::Saved => ("checkmark.circle.fill", NSColor::systemGreenColor()),
        HudKind::Looping => ("repeat.circle.fill", NSColor::controlAccentColor()),
    };
    let view = NSImageView::new(mtm);
    if let Some(img) =
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
    {
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(ICON, unsafe {
            NSFontWeightSemibold
        });
        let img = img.imageWithSymbolConfiguration(&config).unwrap_or(img);
        view.setImage(Some(&img));
    }
    view.setContentTintColor(Some(&color));
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.widthAnchor()
        .constraintEqualToConstant(ICON + 4.0)
        .setActive(true);
    view.heightAnchor()
        .constraintEqualToConstant(ICON + 4.0)
        .setActive(true);
    Retained::into_super(Retained::into_super(view))
}

/// A shortcut drawn as a small rounded key.
fn chip(mtm: MainThreadMarker, text: &str) -> Retained<NSView> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setCornerRadius(5.0);
    b.setBorderWidth(1.0);
    b.setBorderColor(&NSColor::separatorColor());
    b.setFillColor(&NSColor::quaternaryLabelColor());
    b.setContentViewMargins(NSSize::new(6.0, 1.0));
    let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    l.setFont(Some(&NSFont::systemFontOfSize_weight(11.5, unsafe {
        NSFontWeightSemibold
    })));
    l.setTextColor(Some(&NSColor::labelColor()));
    b.setContentView(Some(&l));
    b.sizeToFit();
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
            objc2_app_kit::NSLayoutAttribute::Leading
        } else {
            objc2_app_kit::NSLayoutAttribute::CenterY
        },
    );
    s.setSpacing(spacing);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

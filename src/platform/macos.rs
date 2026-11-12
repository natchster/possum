//! macOS: AppKit and Core Animation.
//!
//! * A borderless, transparent, click-through window covers the whole primary
//!   screen at status-bar level (above the menu bar and Dock, on every Space).
//! * The possum is a `CALayer` inside that window whose anchor point is at its
//!   paws. Every tick it moves to where the brain says and rotates to the
//!   direction of travel, so the paws always stay on the screen edge.
//! * Frames are tiny native-resolution `CGImage`s scaled up with
//!   nearest-neighbour filtering, so the pixel art stays razor sharp.
//! * Sounds are played with `NSSound`; lock, screensaver and display sleep
//!   come from system notifications plus a direct check (see quiet.rs).
//! * The menu hangs off a menu-bar icon, and Control-Option-Command-P opens
//!   it too. When the menu bar is too full, macOS silently leaves the icon
//!   out; Possum notices from where macOS put it, then says so in a note in
//!   the top-right corner (and, if it happens at start-up, screams silently).

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
    NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSEvent, NSFont, NSImage, NSMenu, NSMenuItem, NSRunningApplication, NSScreen, NSSound,
    NSStatusBar, NSStatusItem, NSStatusWindowLevel, NSVariableStatusItemLength, NSWindow, NSWindowCollectionBehavior,
    NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{
    CFBoolean, CFData, CFRetained, CFString, CFType, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapInfo, CGColor, CGColorRenderingIntent, CGColorSpace, CGDataProvider,
    CGDisplayIsAsleep, CGImage, CGImageAlphaInfo, CGMainDisplayID, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{
    NSData, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSNotificationSuspensionBehavior, NSObject, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
};
use objc2_quartz_core::{kCAFilterNearest, kCAGravityResize, CALayer, CATextLayer, CATransaction};

use crate::brain::{self, Brain, Host, Scene, Settings, SCREAM_NOW, SILENT};
use crate::scream::{Kind, Rng};
use crate::sprite::{self, Canvas};
use crate::{quiet, screens, Options};

/// What macOS told us since the last tick: lock, screensaver and sleep
/// events (see quiet.rs), and whether the displays changed.
static SYSTEM_EVENTS: Mutex<Vec<quiet::Event>> = Mutex::new(Vec::new());
static SCREENS_CHANGED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The menu, for the keyboard shortcut and for start-up silence (it's
    /// also the menu-bar icon's).
    static MENU: RefCell<Option<Retained<NSMenu>>> = const { RefCell::new(None) };
}

/// Menu item tags for the two scream modes.
const TAG_SCREAM: isize = 0;
const TAG_SCREAM_SILENTLY: isize = 1;

/// The keyboard shortcut that opens the menu, as macOS writes it.
const SHORTCUT: &str = "⌃⌥⌘P";

// ---------------------------------------------------------------------------
// Graphics helpers
// ---------------------------------------------------------------------------

fn cg_image(c: &Canvas, scale: usize) -> CFRetained<CGImage> {
    let rgba = c.rgba_scaled(scale);
    let (w, h) = (c.w as usize * scale, c.h as usize * scale);
    let data = CFData::from_bytes(&rgba);
    let provider = CGDataProvider::with_cf_data(Some(&data)).expect("CGDataProvider");
    // SAFETY: reading an immutable CoreGraphics constant.
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).expect("sRGB");
    // SAFETY: the buffer is exactly w*h*4 bytes of 8-bit RGBA, and alpha is
    // only ever 0 or 255, so straight and premultiplied alpha coincide.
    unsafe {
        CGImage::new(
            w,
            h,
            8,
            32,
            w * 4,
            Some(&space),
            CGBitmapInfo(CGImageAlphaInfo::PremultipliedLast.0),
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .expect("CGImage")
}

fn as_object(image: &CGImage) -> &AnyObject {
    let cf: &CFType = image;
    cf.as_ref()
}

/// Makes a layer render its contents as crisp, scaled-up pixels.
fn pixelated(layer: &CALayer) {
    // SAFETY: reading immutable QuartzCore string constants.
    unsafe {
        layer.setMagnificationFilter(kCAFilterNearest);
        layer.setMinificationFilter(kCAFilterNearest);
        layer.setContentsGravity(kCAGravityResize);
    }
}

fn rotation(angle: f64) -> CGAffineTransform {
    // Snap tiny values so the straight edges get an exact 90° rotation.
    let snap = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
    let (s, c) = (snap(angle.sin()), snap(angle.cos()));
    CGAffineTransform { a: c, b: s, c: -s, d: c, tx: 0.0, ty: 0.0 }
}

/// A rectangle of the given size at the origin.
fn rect(w: f64, h: f64) -> CGRect {
    CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h))
}

fn to_rect(r: CGRect) -> screens::Rect {
    screens::Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
}

fn to_cg(r: screens::Rect) -> CGRect {
    CGRect::new(CGPoint::new(r.x, r.y), CGSize::new(r.w, r.h))
}

/// The frame of the primary screen (the one with the menu bar), if there is
/// a screen at all. There can briefly be none while displays reconfigure.
fn primary_screen_frame(mtm: MainThreadMarker) -> Option<CGRect> {
    NSScreen::screens(mtm).iter().next().map(|s| s.frame())
}

// ---------------------------------------------------------------------------
// Quiet times
// ---------------------------------------------------------------------------

/// Reads the real system state behind quiet.rs: is the screen locked, is
/// the screensaver running, is the primary display asleep, and does this
/// user's session own the screen.
fn probe_system() -> quiet::Probe {
    let mut probe = quiet::Probe::default();
    if let Some(session) = CGSessionCopyCurrentDictionary() {
        let flag = |key: &'static str| -> Option<bool> {
            let key = CFString::from_static_str(key);
            // SAFETY: the session dictionary maps CFString keys to CF
            // objects; the value is only used after checking its type.
            let value = unsafe { session.value((&*key as *const CFString).cast()) };
            if value.is_null() {
                return None;
            }
            let value: &CFType = unsafe { &*value.cast::<CFType>() };
            Some(value.downcast_ref::<CFBoolean>().is_none_or(CFBoolean::as_bool))
        };
        probe.screen_locked = Some(flag("CGSSessionScreenIsLocked").unwrap_or(false));
        probe.on_console = Some(flag("kCGSSessionOnConsoleKey").unwrap_or(true));
    }
    probe.display_asleep = Some(CGDisplayIsAsleep(CGMainDisplayID()));
    let engine = NSString::from_str("com.apple.ScreenSaver.Engine");
    probe.screensaver = Some(NSRunningApplication::runningApplicationsWithBundleIdentifier(&engine).count() > 0);
    probe
}

fn queue_system_event(event: quiet::Event) {
    SYSTEM_EVENTS.lock().unwrap_or_else(|e| e.into_inner()).push(event);
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and we don't
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PossumSystemWatcher"]
    /// Receives the macOS notifications that matter to the possum.
    struct SystemWatcher;

    impl SystemWatcher {
        #[unsafe(method(systemEvent:))]
        fn system_event(&self, note: &NSNotification) {
            if let Some(event) = quiet::Event::from_notification(&note.name().to_string()) {
                queue_system_event(event);
            }
        }

        #[unsafe(method(screensChanged:))]
        fn screens_changed(&self, _note: &NSNotification) {
            SCREENS_CHANGED.store(true, Ordering::Relaxed);
        }
    }
);

impl SystemWatcher {
    /// Subscribes to lock, screensaver, sleep and display-change
    /// notifications. The watcher must stay alive while the app runs.
    fn start(mtm: MainThreadMarker) -> Retained<Self> {
        let this: Retained<Self> = unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), init] };
        // SAFETY: the selectors exist on the watcher with the right
        // signature, and the watcher outlives the subscriptions.
        unsafe {
            // Distributed notifications are held back while an app is
            // inactive, and a menu-bar app is never active; ask for them
            // right away instead.
            let distributed = NSDistributedNotificationCenter::defaultCenter();
            for name in quiet::DISTRIBUTED_NOTIFICATIONS {
                distributed.addObserver_selector_name_object_suspensionBehavior(
                    &this,
                    sel!(systemEvent:),
                    Some(&NSString::from_str(name)),
                    None,
                    NSNotificationSuspensionBehavior::DeliverImmediately,
                );
            }
            let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
            for name in quiet::WORKSPACE_NOTIFICATIONS {
                workspace.addObserver_selector_name_object(&this, sel!(systemEvent:), Some(&NSString::from_str(name)), None);
            }
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &this,
                sel!(screensChanged:),
                Some(NSApplicationDidChangeScreenParametersNotification),
                None,
            );
        }
        this
    }
}

// ---------------------------------------------------------------------------
// The possum on screen
// ---------------------------------------------------------------------------

struct Bubble {
    image: CFRetained<CGImage>,
}

/// Everything the possum looks like, as Core Graphics images.
struct Art {
    run_frames: Vec<CFRetained<CGImage>>,
    scream_frames: Vec<CFRetained<CGImage>>,
    yell_frames: Vec<CFRetained<CGImage>>,
    /// Per kind of scream, per shout, per pointer corner.
    bubbles: [Vec<[Bubble; 4]>; 2],
}

impl Art {
    fn load() -> Self {
        let art = sprite::possum();
        Self {
            run_frames: art.run.iter().map(|c| cg_image(c, 1)).collect(),
            scream_frames: art.scream.iter().map(|c| cg_image(c, 1)).collect(),
            yell_frames: (0..2).map(|f| cg_image(&sprite::yell(f), 1)).collect(),
            bubbles: brain::shouts().map(|texts| {
                texts
                    .iter()
                    .map(|text| {
                        sprite::Pointer::ALL.map(|p| Bubble { image: cg_image(&sprite::bubble(text, p), 1) })
                    })
                    .collect()
            }),
        }
    }
}

/// Plays the pre-synthesised screams with `NSSound`.
struct Sounds([Vec<Retained<NSSound>>; 2]);

impl Host for Sounds {
    fn play(&mut self, kind: Kind, variant: usize) -> bool {
        self.0[brain::kind_index(kind)][variant].play()
    }

    fn hush(&mut self) {
        for sound in self.0.iter().flatten() {
            if sound.isPlaying() {
                sound.stop();
            }
        }
    }

    fn probe(&mut self) -> quiet::Probe {
        probe_system()
    }
}

/// A transparent, borderless, click-through window over the whole screen,
/// above the menu bar and the Dock, on every Space.
fn overlay_window(mtm: MainThreadMarker, frame: CGRect) -> Retained<NSWindow> {
    // SAFETY: standard NSWindow initialiser; we keep the window alive and
    // opt out of "released when closed" semantics below.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setIgnoresMouseEvents(true);
    window.setLevel(NSStatusWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Stationary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    window.setFrame_display(frame, false);
    window
}

/// The possum's layers: the body, its yell lines (a child, so they rotate
/// with it) and the speech bubble (a sibling, so it stays upright).
fn possum_layers(root: &CALayer, scale: f64) -> (Retained<CALayer>, Retained<CALayer>, Retained<CALayer>) {
    let (pw, ph) = (sprite::POSSUM_W as f64, sprite::POSSUM_H as f64);
    let body = CALayer::new();
    body.setBounds(rect(pw * scale, ph * scale));
    // Rotate and position around the paws (layer unit space is y-up).
    body.setAnchorPoint(CGPoint::new(sprite::FEET_X as f64 / pw, 1.0 - sprite::FEET_Y as f64 / ph));
    pixelated(&body);
    root.addSublayer(&body);

    // The bubble is a sibling of the body rather than a child, so it does
    // not inherit the rotation and its text always reads upright.
    let bubble = CALayer::new();
    pixelated(&bubble);
    bubble.setHidden(true);
    bubble.setAnchorPoint(CGPoint::new(0.0, 0.0));
    root.addSublayer(&bubble);

    let yell = CALayer::new();
    pixelated(&yell);
    yell.setHidden(true);
    let ycanvas = sprite::yell(0);
    yell.setBounds(rect(ycanvas.w as f64 * scale, ycanvas.h as f64 * scale));
    yell.setAnchorPoint(CGPoint::new(0.0, 0.5));
    let (mx, my) = sprite::MOUTH_ANCHOR;
    yell.setPosition(CGPoint::new(mx as f64 * scale, (ph - my as f64) * scale));
    body.addSublayer(&yell);

    (body, bubble, yell)
}

/// A note in the top-right corner, below the menu bar, with the possum's
/// head: it says that the menu-bar icon doesn't fit and how to reach the
/// menu anyway. It shows for a few seconds, then fades away. It's part of
/// the click-through overlay, so it never gets in the way.
struct Hint {
    card: Retained<CALayer>,
    head: Retained<CALayer>,
    title: Retained<CATextLayer>,
    text: Retained<CATextLayer>,
    /// When it appeared, while it's up.
    since: Option<f64>,
}

impl Hint {
    /// Fully there this long (fading in included), then it fades out.
    const SHOWN: f64 = 5.0;
    const FADE_IN: f64 = 0.2;
    const FADE_OUT: f64 = 1.0;

    const PADDING: f64 = 12.0;
    const MARGIN: f64 = 10.0;
    const HEAD_SCALE: f64 = 2.0;

    fn new(root: &CALayer) -> Self {
        let card = CALayer::new();
        card.setAnchorPoint(CGPoint::new(0.0, 0.0));
        card.setBackgroundColor(Some(&CGColor::new_generic_gray(0.12, 0.92)));
        card.setBorderColor(Some(&CGColor::new_generic_gray(1.0, 0.18)));
        card.setBorderWidth(1.0);
        card.setCornerRadius(12.0);
        card.setShadowOpacity(0.35);
        card.setShadowRadius(10.0);
        card.setShadowOffset(CGSize::new(0.0, -3.0));
        card.setHidden(true);

        let icon = sprite::status_icon();
        let head = CALayer::new();
        pixelated(&head);
        head.setAnchorPoint(CGPoint::new(0.0, 0.0));
        head.setBounds(rect(icon.w as f64 * Self::HEAD_SCALE, icon.h as f64 * Self::HEAD_SCALE));
        unsafe { head.setContents(Some(as_object(&cg_image(&icon, 1)))) };
        card.addSublayer(&head);

        let label = |font: Retained<NSFont>| {
            let layer = CATextLayer::new();
            layer.setAnchorPoint(CGPoint::new(0.0, 0.0));
            // SAFETY: NSFont is toll-free bridged with CTFont, a CF type
            // that CATextLayer takes as its font.
            unsafe { layer.setFont(Some(&*Retained::as_ptr(&font).cast::<CFType>())) };
            layer.setFontSize(font.pointSize());
            layer.setForegroundColor(Some(&CGColor::new_generic_gray(1.0, 1.0)));
            card.addSublayer(&layer);
            layer
        };
        let title = label(NSFont::boldSystemFontOfSize(13.0));
        let text = label(NSFont::systemFontOfSize(13.0));

        root.addSublayer(&card);
        Self { card, head, title, text, since: None }
    }

    /// Shows the note from `now`, in the top-right corner of `area` (the
    /// part of the screen outside the menu bar and the Dock) at the given
    /// backing scale.
    fn show(&mut self, now: f64, title: &str, text: &str, area: CGRect, backing_scale: f64) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // Sets a line's text and sizes the line to fit it.
        let fit = |layer: &CATextLayer, string: &str| {
            unsafe { layer.setString(Some(&NSString::from_str(string))) };
            layer.setContentsScale(backing_scale);
            let s = layer.preferredFrameSize();
            let size = CGSize::new(s.width.ceil(), s.height.ceil());
            layer.setBounds(rect(size.width, size.height));
            size
        };
        let (title_size, text_size) = (fit(&self.title, title), fit(&self.text, text));
        let head = self.head.bounds().size;
        let lines = title_size.height + 2.0 + text_size.height;
        let (w, h) = (
            Self::PADDING + head.width + 10.0 + title_size.width.max(text_size.width) + Self::PADDING,
            Self::PADDING + head.height.max(lines) + Self::PADDING,
        );
        let left = Self::PADDING + head.width + 10.0;
        let bottom = (h - lines) / 2.0;
        self.head.setPosition(CGPoint::new(Self::PADDING, (h - head.height) / 2.0));
        self.text.setPosition(CGPoint::new(left, bottom));
        self.title.setPosition(CGPoint::new(left, bottom + text_size.height + 2.0));
        self.card.setBounds(rect(w, h));
        self.card.setPosition(CGPoint::new(
            area.origin.x + area.size.width - Self::MARGIN - w,
            area.origin.y + area.size.height - Self::MARGIN - h,
        ));
        self.card.setOpacity(0.0);
        self.card.setHidden(false);
        CATransaction::commit();
        self.since = Some(now);
    }

    /// How visible the note is `t` seconds after it appeared; None once
    /// it's gone.
    fn opacity(t: f64) -> Option<f32> {
        let o = if t < Self::FADE_IN {
            t.max(0.0) / Self::FADE_IN
        } else if t < Self::SHOWN {
            1.0
        } else if t < Self::SHOWN + Self::FADE_OUT {
            1.0 - (t - Self::SHOWN) / Self::FADE_OUT
        } else {
            return None;
        };
        Some(o as f32)
    }

    fn update(&mut self, now: f64) {
        let Some(since) = self.since else {
            return;
        };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        match Self::opacity(now - since) {
            Some(o) => self.card.setOpacity(o),
            None => {
                self.card.setHidden(true);
                self.since = None;
            }
        }
        CATransaction::commit();
    }
}

struct Possum {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    body: Retained<CALayer>,
    bubble: Retained<CALayer>,
    yell: Retained<CALayer>,
    art: Art,
    sounds: Sounds,
    brain: Brain,
    screen: CGRect,
    clock: Instant,
    shown: Option<(bool, usize)>,
    yell_shown: Option<usize>,
    bubble_shown: Option<(Kind, usize, usize)>,
    /// The menu-bar icon, whether it fits, and the note that says when it
    /// doesn't.
    icon: Retained<NSStatusItem>,
    icon_watch: IconWatch,
    next_icon_check: f64,
    hint: Hint,
    last_hint: Option<f64>,
    /// Whether the keyboard shortcut works.
    shortcut: bool,
}

impl Possum {
    /// Look at the menu-bar icon this often, once it's settled.
    const ICON_CHECK_INTERVAL: f64 = 0.25;
    /// After a note, wait this long before another one about the icon being
    /// left out again (switching between apps with short and long menus can
    /// leave it out and bring it back again and again).
    const HINT_AGAIN_AFTER: f64 = 60.0;

    fn new(mtm: MainThreadMarker, opts: &Options, icon: Retained<NSStatusItem>, shortcut: bool) -> Self {
        let seed = super::seed();
        let mut rng = Rng::new(seed);
        let voices = brain::voices(seed, &mut rng);
        let durations = voices.each_ref().map(|v| v.iter().map(|s| s.seconds).collect());
        let sounds = Sounds(voices.map(|v| {
            v.iter()
                .map(|s| {
                    let sound = NSSound::initWithData(NSSound::alloc(), &NSData::with_bytes(&s.wav))
                        .expect("NSSound could not decode the synthesised WAV");
                    sound.setVolume(opts.volume);
                    sound
                })
                .collect()
        }));

        // Points on macOS: one sprite pixel is `scale` points.
        let screen = primary_screen_frame(mtm).unwrap_or_else(|| {
            // macOS reporting no screen at all at launch is very unlikely; if
            // it does, start with a placeholder size and adopt the real
            // screen on the first display check, a second later.
            rect(1440.0, 900.0)
        });
        let settings = Settings { scale: opts.scale, speed: opts.speed, start: opts.start, verbose: opts.verbose };
        let mut brain = Brain::new(&settings, screen.size.width, screen.size.height, durations, rng);

        let window = overlay_window(mtm, screen);
        let view = window.contentView().expect("window has a content view");
        view.setWantsLayer(true);
        let root = view.layer().expect("layer-backed view");
        let (body, bubble, yell) = possum_layers(&root, opts.scale);
        let hint = Hint::new(&root);
        window.orderFrontRegardless();

        let mut sounds = sounds;
        brain.update(sounds.probe());
        brain.log(format_args!(
            "running around the primary display, {}x{}",
            screen.size.width, screen.size.height
        ));
        if shortcut {
            brain.log(format_args!("{SHORTCUT} opens the menu"));
        }
        // The first scream waits until it's clear whether the menu-bar icon
        // fits: if it doesn't, the possum screams silently from the start.
        brain.hold_screams(true);
        let mut possum = Self {
            mtm,
            window,
            body,
            bubble,
            yell,
            art: Art::load(),
            sounds,
            brain,
            screen,
            clock: Instant::now(),
            shown: None,
            yell_shown: None,
            bubble_shown: None,
            icon,
            icon_watch: IconWatch::default(),
            next_icon_check: 0.0,
            hint,
            last_hint: None,
            shortcut,
        };
        possum.tick();
        possum
    }

    /// Reads where macOS put the menu-bar icon, and acts on any change.
    fn check_icon(&mut self, now: f64) {
        let window = self.icon.button(self.mtm).and_then(|button| button.window());
        let spot = window.map_or(IconSpot::Unknown, |w| icon_spot(w.frame(), &screen_tops(self.mtm)));
        match self.icon_watch.update(now, spot) {
            Some(IconNews::LeftOutFromTheStart) => {
                SILENT.store(true, Ordering::Relaxed);
                check_scream_mode();
                self.brain.log(format_args!("the menu-bar icon doesn't fit: screaming silently"));
                self.show_hint(now, "Possum's icon doesn't fit in the menu bar", "So it screams silently.");
            }
            Some(IconNews::LeftOut) => {
                self.brain.log(format_args!("the menu-bar icon doesn't fit any more"));
                if self.last_hint.is_none_or(|t| now - t >= Self::HINT_AGAIN_AFTER) {
                    self.show_hint(now, "Possum's icon no longer fits in the menu bar", "");
                }
            }
            Some(IconNews::Back) => self.brain.log(format_args!("the menu-bar icon is back")),
            Some(IconNews::Fits) => self.brain.log(format_args!("the menu-bar icon fits")),
            None => {}
        }
        if self.icon_watch.settled() {
            self.brain.hold_screams(false);
        }
    }

    /// The note in the top-right corner: what happened (`title`), anything
    /// else to say (`lead`), and how to reach the menu.
    fn show_hint(&mut self, now: f64, title: &str, lead: &str) {
        let how = if self.shortcut {
            format!("Press {SHORTCUT} for its menu.")
        } else {
            "Make room in the menu bar to reach its menu.".to_string()
        };
        let text = if lead.is_empty() { how } else { format!("{lead} {how}") };
        let Some(screen) = NSScreen::screens(self.mtm).iter().next() else {
            return;
        };
        // The window covers the primary screen: in window coordinates.
        let (frame, visible) = (screen.frame(), screen.visibleFrame());
        let area = CGRect::new(
            CGPoint::new(visible.origin.x - frame.origin.x, visible.origin.y - frame.origin.y),
            visible.size,
        );
        self.hint.show(now, title, &text, area, self.window.backingScaleFactor());
        self.last_hint = Some(now);
    }

    /// Follow display changes (resolution, arrangement, plugging monitors);
    /// see screens.rs.
    fn check_screen(&mut self) {
        let primary = primary_screen_frame(self.mtm).map(to_rect);
        let Some(frame) = screens::follow(primary, to_rect(self.screen), to_rect(self.window.frame())) else {
            return;
        };
        self.screen = to_cg(frame);
        self.window.setFrame_display(self.screen, true);
        self.brain.set_screen(frame.w, frame.h);
    }

    fn tick(&mut self) {
        let now = self.clock.elapsed().as_secs_f64();
        // React to macOS right away.
        let events = std::mem::take(&mut *SYSTEM_EVENTS.lock().unwrap_or_else(|e| e.into_inner()));
        for event in events {
            self.brain.apply(event);
        }
        if SCREENS_CHANGED.swap(false, Ordering::Relaxed) {
            self.check_screen();
        }
        // Every tick until it's settled at start-up, then a few times a
        // second.
        if now >= self.next_icon_check {
            self.check_icon(now);
            if self.icon_watch.settled() {
                self.next_icon_check = now + Self::ICON_CHECK_INTERVAL;
            }
        }
        let scene = self.brain.tick(now, &mut self.sounds);
        if scene.check_displays {
            self.check_screen();
        }
        self.draw(&scene);
        self.hint.update(now);
    }

    fn draw(&mut self, scene: &Scene) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.body.setPosition(CGPoint::new(scene.x, scene.y));
        self.body.setAffineTransform(rotation(scene.heading));
        if self.shown != Some((scene.screaming, scene.frame)) {
            self.shown = Some((scene.screaming, scene.frame));
            let frames = if scene.screaming { &self.art.scream_frames } else { &self.art.run_frames };
            unsafe { self.body.setContents(Some(as_object(&frames[scene.frame]))) };
        }
        self.yell.setHidden(scene.yell.is_none());
        if let Some(f) = scene.yell {
            if self.yell_shown != Some(f) {
                self.yell_shown = Some(f);
                unsafe { self.yell.setContents(Some(as_object(&self.art.yell_frames[f]))) };
            }
        }
        self.bubble.setHidden(scene.bubble.is_none());
        if let Some(b) = scene.bubble {
            let key = (b.kind, b.shout, b.quadrant);
            if self.bubble_shown != Some(key) {
                self.bubble_shown = Some(key);
                self.bubble.setBounds(rect(b.w, b.h));
                let image = &self.art.bubbles[brain::kind_index(b.kind)][b.shout][b.quadrant].image;
                unsafe { self.bubble.setContents(Some(as_object(image))) };
            }
            self.bubble.setPosition(CGPoint::new(b.left, b.bottom));
        }
        CATransaction::commit();
    }
}

// ---------------------------------------------------------------------------
// Menu bar item
// ---------------------------------------------------------------------------

define_class!(
    // SAFETY: NSObject has no subclassing requirements and we don't
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PossumMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        /// "Scream" / "Scream silently": a radio group told apart by tag.
        #[unsafe(method(setScreamMode:))]
        fn set_scream_mode(&self, sender: &NSMenuItem) {
            SILENT.store(sender.tag() == TAG_SCREAM_SILENTLY, Ordering::Relaxed);
            check_scream_mode();
        }

        #[unsafe(method(screamNow:))]
        fn scream_now(&self, _sender: &NSMenuItem) {
            SCREAM_NOW.store(true, Ordering::Relaxed);
        }

        /// The keyboard shortcut: the menu, at the mouse pointer.
        #[unsafe(method(showMenu:))]
        fn show_menu(&self, _sender: Option<&AnyObject>) {
            let Some(menu) = MENU.with(|m| m.borrow().clone()) else {
                return;
            };
            // Become the active app, so the menu takes the keyboard and
            // closes on a click elsewhere; then hand the focus back.
            let previous = NSWorkspace::sharedWorkspace().frontmostApplication();
            #[allow(deprecated)] // its replacement, `activate`, needs macOS 14
            NSApplication::sharedApplication(self.mtm()).activateIgnoringOtherApps(true);
            menu.popUpMenuPositioningItem_atLocation_inView(None, NSEvent::mouseLocation(), None);
            if let Some(previous) = previous {
                if previous.processIdentifier() != std::process::id() as i32 {
                    previous.activateWithOptions(NSApplicationActivationOptions::empty());
                }
            }
        }
    }
);

/// Ticks the scream mode that's on, after it changed.
fn check_scream_mode() {
    let silent = SILENT.load(Ordering::Relaxed);
    MENU.with(|menu| {
        let Some(menu) = &*menu.borrow() else {
            return;
        };
        for item in menu.itemArray().iter() {
            if item.action() == Some(sel!(setScreamMode:)) {
                let on = (item.tag() == TAG_SCREAM_SILENTLY) == silent;
                item.setState(if on { NSControlStateValueOn } else { NSControlStateValueOff });
            }
        }
    });
}

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
    target: Option<&AnyObject>,
) -> Retained<NSMenuItem> {
    // SAFETY: the action selectors used below exist on their targets.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    unsafe { item.setTarget(target) };
    item
}

fn install_status_item(mtm: MainThreadMarker, app: &NSApplication, target: &MenuTarget) -> Retained<NSStatusItem> {
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        let icon = sprite::status_icon();
        // Render at 2x so it's crisp on Retina, displayed at 1pt per pixel.
        let image = NSImage::initWithCGImage_size(
            NSImage::alloc(),
            &cg_image(&icon, 2),
            CGSize::new(icon.w as CGFloat, icon.h as CGFloat),
        );
        button.setImage(Some(&image));
        button.setToolTip(Some(&NSString::from_str("Possum is screaming")));
    }

    let menu = NSMenu::new(mtm);
    let header = menu_item(mtm, "Possum is running!", None, "", None);
    header.setEnabled(false);
    menu.addItem(&header);
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    let silent = SILENT.load(Ordering::Relaxed);
    for (title, tag, checked) in [("Scream", TAG_SCREAM, !silent), ("Scream silently", TAG_SCREAM_SILENTLY, silent)] {
        let mode = menu_item(mtm, title, Some(sel!(setScreamMode:)), "", Some(target));
        mode.setTag(tag);
        mode.setState(if checked { NSControlStateValueOn } else { NSControlStateValueOff });
        menu.addItem(&mode);
    }
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(mtm, "Scream Now", Some(sel!(screamNow:)), "s", Some(target)));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&menu_item(mtm, "Quit Possum", Some(sel!(terminate:)), "q", Some(app)));
    item.setMenu(Some(&menu));
    MENU.with(|m| *m.borrow_mut() = Some(menu));
    item
}

// ---------------------------------------------------------------------------
// Keyboard shortcut
// ---------------------------------------------------------------------------

/// The Carbon calls behind a global keyboard shortcut. They need no
/// permission (unlike watching the keyboard), and macOS still supports them.
mod carbon {
    use std::ffi::c_void;

    pub type OSStatus = i32;
    pub type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> OSStatus;

    #[repr(C)]
    pub struct EventTypeSpec {
        pub class: u32,
        pub kind: u32,
    }

    #[repr(C)]
    pub struct HotKeyId {
        pub signature: u32,
        pub id: u32,
    }

    /// kEventClassKeyboard, kEventHotKeyPressed.
    pub const KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
    pub const HOT_KEY_PRESSED: u32 = 5;
    /// kVK_ANSI_P: the P key (where it is on a US keyboard).
    pub const KEY_P: u32 = 0x23;
    /// controlKey | optionKey | cmdKey. Since macOS 15 a shortcut needs
    /// Command or Control: Option and Shift alone are refused.
    pub const CONTROL_OPTION_COMMAND: u32 = 0x1000 | 0x0800 | 0x0100;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        pub fn GetApplicationEventTarget() -> *mut c_void;
        pub fn InstallEventHandler(
            target: *mut c_void,
            handler: Handler,
            count: usize,
            types: *const EventTypeSpec,
            user_data: *mut c_void,
            handler_ref: *mut *mut c_void,
        ) -> OSStatus;
        pub fn RegisterEventHotKey(
            key: u32,
            modifiers: u32,
            id: HotKeyId,
            target: *mut c_void,
            options: u32,
            hot_key_ref: *mut *mut c_void,
        ) -> OSStatus;
    }
}

/// Makes Control-Option-Command-P open the menu. Fails with Carbon's error
/// code, for example when another app already has the shortcut. `target`
/// must live as long as the app.
fn register_shortcut(target: &MenuTarget) -> Result<(), carbon::OSStatus> {
    let pressed = carbon::EventTypeSpec { class: carbon::KEYBOARD, kind: carbon::HOT_KEY_PRESSED };
    let id = carbon::HotKeyId { signature: u32::from_be_bytes(*b"Psum"), id: 1 };
    let (mut handler, mut hot_key) = (null_mut(), null_mut());
    // SAFETY: plain Carbon calls with valid pointers; the handler gets the
    // target, which outlives the app's event loop.
    unsafe {
        let app = carbon::GetApplicationEventTarget();
        let target = (target as *const MenuTarget).cast_mut().cast::<c_void>();
        let status = carbon::InstallEventHandler(app, shortcut_pressed, 1, &pressed, target, &mut handler);
        if status != 0 {
            return Err(status);
        }
        match carbon::RegisterEventHotKey(carbon::KEY_P, carbon::CONTROL_OPTION_COMMAND, id, app, 0, &mut hot_key) {
            0 => Ok(()),
            status => Err(status),
        }
    }
}

/// Carbon calls this on the main thread when the shortcut is pressed.
unsafe extern "C" fn shortcut_pressed(_next: *mut c_void, _event: *mut c_void, target: *mut c_void) -> i32 {
    // SAFETY: `target` is the MenuTarget given to `register_shortcut`.
    let target = unsafe { &*target.cast::<MenuTarget>() };
    // Open the menu once this event is handled: a menu runs an event loop
    // of its own.
    let _: () = unsafe {
        msg_send![target, performSelector: sel!(showMenu:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]
    };
    0
}

// ---------------------------------------------------------------------------
// Does the menu-bar icon fit?
// ---------------------------------------------------------------------------
//
// When the menu bar is too full, macOS silently leaves icons out, and there
// is no API that tells an app. The occlusion state of the icon's window,
// which some apps use, still says "visible" for a left-out icon on macOS 27.
// So Possum looks at where macOS put the icon's window:
//
// * An icon that fits sits in a menu bar, at the top edge of a screen.
// * macOS 27 parks an icon that has no room at (0, -height), just below the
//   bottom-left corner of the main screen: off every screen. Every new icon
//   waits there for a moment (about 10 ms) before macOS places it.
// * Older macOS versions put a left-out icon behind the camera notch, on
//   MacBooks that have one.
//
// One case goes unnoticed: on macOS 27, an icon that was showing and then
// gets crowded out (behind the menu bar's "«" overflow button) keeps its
// old frame, which still looks like a spot in the menu bar.
//
// That's observed behaviour, not a promise. Anything else reads as "can't
// tell", and Possum then carries on as if the icon were showing: if this
// ever stops working, Possum only stops noticing.

/// Where macOS put the menu-bar icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IconSpot {
    Shown,
    LeftOut,
    Unknown,
}

/// A screen, and the left and right edges of its camera notch, if any.
#[derive(Clone, Copy, Debug)]
struct ScreenTop {
    frame: CGRect,
    notch: Option<(f64, f64)>,
}

fn icon_spot(icon: CGRect, screens: &[ScreenTop]) -> IconSpot {
    let (x, y, w, h) = (icon.origin.x, icon.origin.y, icon.size.width, icon.size.height);
    if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
        return IconSpot::Unknown;
    }
    if x.abs() < 0.5 && (y + h).abs() < 0.5 {
        return IconSpot::LeftOut;
    }
    for screen in screens {
        let f = screen.frame;
        let in_menu_bar = (y + h - (f.origin.y + f.size.height)).abs() <= 2.0
            && x >= f.origin.x - 0.5
            && x + w <= f.origin.x + f.size.width + 0.5;
        if in_menu_bar {
            let middle = x + w / 2.0;
            return match screen.notch {
                Some((left, right)) if middle > left && middle < right => IconSpot::LeftOut,
                _ => IconSpot::Shown,
            };
        }
    }
    IconSpot::Unknown
}

fn screen_tops(mtm: MainThreadMarker) -> Vec<ScreenTop> {
    NSScreen::screens(mtm).iter().map(|s| ScreenTop { frame: s.frame(), notch: notch(&s) }).collect()
}

/// The left and right edges of a screen's camera notch: what's left of its
/// top edge between the two usable corners (macOS 12 and later).
fn notch(screen: &NSScreen) -> Option<(f64, f64)> {
    let answers = |selector| screen.respondsToSelector(selector);
    if !answers(sel!(auxiliaryTopLeftArea)) || !answers(sel!(auxiliaryTopRightArea)) {
        return None;
    }
    let (left, right) = (screen.auxiliaryTopLeftArea().size.width, screen.auxiliaryTopRightArea().size.width);
    let f = screen.frame();
    let (start, end) = (f.origin.x + left, f.origin.x + f.size.width - right);
    (left > 0.0 && right > 0.0 && end - start >= 1.0).then_some((start, end))
}

/// What became of the menu-bar icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IconNews {
    /// At start-up: it fits (or Possum can't tell).
    Fits,
    /// At start-up: it was left out.
    LeftOutFromTheStart,
    /// It was showing, and now it's left out.
    LeftOut,
    /// It was left out, and now it's showing again.
    Back,
}

/// Follows the menu-bar icon's spot from reading to reading.
#[derive(Default)]
struct IconWatch {
    /// None until settled at start-up.
    fits: Option<bool>,
    left_out_since: Option<f64>,
}

impl IconWatch {
    /// How long the icon must stay left out to count: longer than the
    /// moment every new icon spends at the left-out spot.
    const SETTLE: f64 = 0.3;
    /// Can't tell this long after start-up: assume it fits.
    const GIVE_UP: f64 = 1.0;

    fn settled(&self) -> bool {
        self.fits.is_some()
    }

    fn update(&mut self, now: f64, spot: IconSpot) -> Option<IconNews> {
        match spot {
            IconSpot::LeftOut => {
                let since = *self.left_out_since.get_or_insert(now);
                if now - since < Self::SETTLE {
                    return None;
                }
                match self.fits.replace(false) {
                    None => Some(IconNews::LeftOutFromTheStart),
                    Some(true) => Some(IconNews::LeftOut),
                    Some(false) => None,
                }
            }
            IconSpot::Shown => {
                self.left_out_since = None;
                match self.fits.replace(true) {
                    None => Some(IconNews::Fits),
                    Some(false) => Some(IconNews::Back),
                    Some(true) => None,
                }
            }
            IconSpot::Unknown => {
                self.left_out_since = None;
                if self.fits.is_none() && now >= Self::GIVE_UP {
                    self.fits = Some(true);
                    return Some(IconNews::Fits);
                }
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------

pub fn run(opts: &Options) {
    let mtm = MainThreadMarker::new().expect("possum must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon: the possum lives on the screen edge and in the menu bar.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let target = MenuTarget::new(mtm);
    let icon = install_status_item(mtm, &app, &target);
    let shortcut = register_shortcut(&target);
    if let Err(status) = shortcut {
        eprintln!("possum: couldn't set up {SHORTCUT} for the menu (Carbon error {status}); another app may use it");
    }
    let possum = RefCell::new(Possum::new(mtm, opts, icon, shortcut.is_ok()));
    let _watcher = SystemWatcher::start(mtm);

    let tick = RcBlock::new(move |_timer: NonNull<NSTimer>| possum.borrow_mut().tick());
    // SAFETY: the block only touches main-thread state and the timer fires
    // on the main run loop. Common modes keep it running while the menu is
    // open.
    unsafe {
        let timer = NSTimer::timerWithTimeInterval_repeats_block(1.0 / 60.0, true, &tick);
        NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes);
    }

    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_notification_names_match_appkit() {
        // quiet.rs spells these out as strings; make sure they are exactly
        // what AppKit posts.
        // SAFETY: reading immutable AppKit string constants.
        let appkit = unsafe {
            [
                objc2_app_kit::NSWorkspaceScreensDidSleepNotification,
                objc2_app_kit::NSWorkspaceScreensDidWakeNotification,
                objc2_app_kit::NSWorkspaceSessionDidResignActiveNotification,
                objc2_app_kit::NSWorkspaceSessionDidBecomeActiveNotification,
                objc2_app_kit::NSWorkspaceWillSleepNotification,
                objc2_app_kit::NSWorkspaceDidWakeNotification,
            ]
        };
        for (ours, theirs) in quiet::WORKSPACE_NOTIFICATIONS.iter().zip(appkit) {
            assert_eq!(*ours, theirs.to_string());
        }
    }

    fn frame(x: f64, y: f64, w: f64, h: f64) -> CGRect {
        CGRect::new(CGPoint::new(x, y), CGSize::new(w, h))
    }

    /// A 3440x1440 main screen, as on the Mac these frames were seen on.
    fn wide_screen() -> ScreenTop {
        ScreenTop { frame: frame(0.0, 0.0, 3440.0, 1440.0), notch: None }
    }

    #[test]
    fn reads_the_icon_spots_seen_on_macos_27() {
        let screens = [wide_screen()];
        // In the menu bar: fits.
        assert_eq!(icon_spot(frame(2701.0, 1410.0, 53.0, 30.0), &screens), IconSpot::Shown);
        // Parked below the main screen's corner: left out.
        assert_eq!(icon_spot(frame(0.0, -30.0, 53.0, 30.0), &screens), IconSpot::LeftOut);
        // Not laid out yet (no height): can't tell.
        assert_eq!(icon_spot(frame(0.0, 0.0, 53.0, 0.0), &screens), IconSpot::Unknown);
        // Anywhere else: can't tell either.
        assert_eq!(icon_spot(frame(500.0, 700.0, 53.0, 30.0), &screens), IconSpot::Unknown);
        assert_eq!(icon_spot(frame(f64::NAN, 1410.0, 53.0, 30.0), &screens), IconSpot::Unknown);
    }

    #[test]
    fn reads_icons_on_a_second_screen_and_behind_a_notch() {
        let laptop = ScreenTop { frame: frame(-1512.0, 0.0, 1512.0, 982.0), notch: Some((-850.0, -662.0)) };
        let screens = [wide_screen(), laptop];
        // In the second screen's menu bar, clear of its notch.
        assert_eq!(icon_spot(frame(-300.0, 950.0, 40.0, 32.0), &screens), IconSpot::Shown);
        // Behind the notch (older macOS): left out.
        assert_eq!(icon_spot(frame(-780.0, 950.0, 40.0, 32.0), &screens), IconSpot::LeftOut);
        // A screen right below the main one ends at y = 0, like the parking
        // spot: the parking spot still counts as left out.
        let below = ScreenTop { frame: frame(-200.0, -1080.0, 1920.0, 1080.0), notch: None };
        assert_eq!(icon_spot(frame(0.0, -30.0, 53.0, 30.0), &[wide_screen(), below]), IconSpot::LeftOut);
    }

    /// Feeds the watch one reading per 60 Hz tick from `from` to `to`.
    fn watch(w: &mut IconWatch, spot: IconSpot, from: f64, to: f64) -> Vec<IconNews> {
        let mut news = Vec::new();
        let mut t = from;
        while t < to {
            news.extend(w.update(t, spot));
            t += 1.0 / 60.0;
        }
        news
    }

    #[test]
    fn a_new_icon_parked_for_a_moment_still_fits() {
        // As seen: not laid out, parked for ~10 ms, then in the menu bar.
        let mut w = IconWatch::default();
        let mut news = watch(&mut w, IconSpot::Unknown, 0.0, 0.07);
        news.extend(watch(&mut w, IconSpot::LeftOut, 0.07, 0.1));
        assert!(!w.settled());
        news.extend(watch(&mut w, IconSpot::Shown, 0.1, 2.0));
        assert_eq!(news, [IconNews::Fits]);
    }

    #[test]
    fn an_icon_left_out_from_the_start_is_noticed_quickly() {
        let mut w = IconWatch::default();
        let news = watch(&mut w, IconSpot::LeftOut, 0.07, 0.6);
        assert_eq!(news, [IconNews::LeftOutFromTheStart]);
        assert!(w.settled());
    }

    #[test]
    fn an_icon_pushed_out_later_and_back_again() {
        let mut w = IconWatch::default();
        let mut news = watch(&mut w, IconSpot::Shown, 0.0, 5.0);
        news.extend(watch(&mut w, IconSpot::LeftOut, 5.0, 6.0));
        news.extend(watch(&mut w, IconSpot::Shown, 6.0, 7.0));
        assert_eq!(news, [IconNews::Fits, IconNews::LeftOut, IconNews::Back]);
    }

    #[test]
    fn when_it_cant_tell_it_carries_on_as_if_the_icon_fits() {
        let mut w = IconWatch::default();
        let news = watch(&mut w, IconSpot::Unknown, 0.0, 5.0);
        assert_eq!(news, [IconNews::Fits]);
        assert!(w.settled(), "the first scream must not wait forever");
    }

    #[test]
    fn the_hint_shows_for_five_seconds_then_fades_away() {
        assert_eq!(Hint::opacity(0.0), Some(0.0));
        assert_eq!(Hint::opacity(0.5), Some(1.0));
        assert_eq!(Hint::opacity(4.99), Some(1.0));
        assert_eq!(Hint::opacity(5.5), Some(0.5));
        assert_eq!(Hint::opacity(6.0), None);
    }

    #[test]
    fn the_probe_reads_a_normal_unlocked_session() {
        // Run locally, tests run in a logged-in, unlocked session with the
        // display on. CI machines have no such session, so skip it there.
        if std::env::var_os("CI").is_some() {
            return;
        }
        let probe = probe_system();
        assert_eq!(probe.screen_locked, Some(false), "{probe:?}");
        assert_eq!(probe.screensaver, Some(false), "{probe:?}");
        assert_eq!(probe.display_asleep, Some(false), "{probe:?}");
        assert_eq!(probe.on_console, Some(true), "{probe:?}");
    }
}

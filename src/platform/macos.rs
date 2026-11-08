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

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDidChangeScreenParametersNotification,
    NSBackingStoreType, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem,
    NSRunningApplication, NSScreen, NSSound, NSStatusBar, NSStatusItem, NSStatusWindowLevel,
    NSVariableStatusItemLength, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{
    CFBoolean, CFData, CFRetained, CFString, CFType, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGDisplayIsAsleep,
    CGImage, CGImageAlphaInfo, CGMainDisplayID, CGSessionCopyCurrentDictionary,
};
use objc2_foundation::{
    NSData, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSNotificationSuspensionBehavior, NSObject, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer,
};
use objc2_quartz_core::{kCAFilterNearest, kCAGravityResize, CALayer, CATransaction};

use crate::brain::{self, Brain, Host, Scene, Settings, SCREAM_NOW, SILENT};
use crate::scream::{Kind, Rng};
use crate::sprite::{self, Canvas};
use crate::{quiet, screens, Options};

/// What macOS told us since the last tick: lock, screensaver and sleep
/// events (see quiet.rs), and whether the displays changed.
static SYSTEM_EVENTS: Mutex<Vec<quiet::Event>> = Mutex::new(Vec::new());
static SCREENS_CHANGED: AtomicBool = AtomicBool::new(false);

/// Menu item tags for the two scream modes.
const TAG_SCREAM: isize = 0;
const TAG_SCREAM_SILENTLY: isize = 1;

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
}

impl Possum {
    fn new(mtm: MainThreadMarker, opts: &Options) -> Self {
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
        window.orderFrontRegardless();

        let mut sounds = sounds;
        brain.update(sounds.probe());
        brain.log(format_args!(
            "running around the primary display, {}x{}",
            screen.size.width, screen.size.height
        ));
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
        };
        possum.tick();
        possum
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
        let scene = self.brain.tick(now, &mut self.sounds);
        if scene.check_displays {
            self.check_screen();
        }
        self.draw(&scene);
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
            let mode = sender.tag();
            SILENT.store(mode == TAG_SCREAM_SILENTLY, Ordering::Relaxed);
            // SAFETY: the item is in the status menu, which outlives it.
            if let Some(menu) = unsafe { sender.menu() } {
                for item in menu.itemArray().iter() {
                    if item.action() == Some(sel!(setScreamMode:)) {
                        item.setState(if item.tag() == mode { NSControlStateValueOn } else { NSControlStateValueOff });
                    }
                }
            }
        }

        #[unsafe(method(screamNow:))]
        fn scream_now(&self, _sender: &NSMenuItem) {
            SCREAM_NOW.store(true, Ordering::Relaxed);
        }
    }
);

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
    item
}

// ---------------------------------------------------------------------------

pub fn run(opts: &Options) {
    let mtm = MainThreadMarker::new().expect("possum must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon: the possum lives on the screen edge and in the menu bar.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let possum = RefCell::new(Possum::new(mtm, opts));
    let target = MenuTarget::new(mtm);
    let _status_item = install_status_item(mtm, &app, &target);
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

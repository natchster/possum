//! Possum: a screaming pixel-art possum that runs counterclockwise around the
//! edge of your screen.
//!
//! How it works:
//! * A borderless, transparent, click-through window covers the whole primary
//!   screen at status-bar level (above the menu bar and Dock, on every Space).
//! * The possum is a `CALayer` inside that window whose anchor point is at its
//!   paws. Every tick we move the layer along the screen border and rotate it
//!   by the direction of travel, so the paws always stay on the screen edge.
//! * Frames are tiny native-resolution `CGImage`s scaled up with
//!   nearest-neighbour filtering, so the pixel art stays razor sharp.
//! * Screams are synthesised at start-up and played with `NSSound`.

mod png;
mod scream;
mod sprite;
mod track;

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSImage, NSMenu, NSMenuItem, NSScreen,
    NSSound, NSStatusBar, NSStatusItem, NSStatusWindowLevel, NSVariableStatusItemLength,
    NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{
    CFData, CFRetained, CFType, CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    kCGColorSpaceSRGB, CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider,
    CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSData, NSObject, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer};
use objc2_quartz_core::{kCAFilterNearest, kCAGravityResize, CALayer, CATransaction};

use scream::{Kind, Rng};
use sprite::Canvas;
use track::Track;

/// Things the menu bar item can ask the possum to do.
///
/// `SILENT` picks the scream mode: `false` is "Scream" (you hear it, no speech
/// bubble), `true` is "Scream silently" (you read it in a bubble, no sound).
static SILENT: AtomicBool = AtomicBool::new(false);
static SCREAM_NOW: AtomicBool = AtomicBool::new(false);

/// Menu item tags for the two scream modes.
const TAG_SCREAM: isize = 0;
const TAG_SCREAM_SILENTLY: isize = 1;

/// What the speech bubble says in "Scream silently" mode, per kind of scream.
const AAAA_SHOUTS: [&str; 6] = ["AAAAAA!", "AAAAAAAAH!", "AAAA!!", "AAARGH!", "AAAAAAAAAAA!", "HAAAA!"];
const HISS_SHOUTS: [&str; 3] = ["HSSSSS!", "HSSSSSSSS!", "SSSSSSS!"];

/// Index of a scream kind into the per-kind sound and bubble tables.
fn kind_index(kind: Kind) -> usize {
    match kind {
        Kind::Aaaa => 0,
        Kind::Hiss => 1,
    }
}

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

struct Options {
    scale: f64,
    speed: f64,
    volume: f32,
    silent: bool,
    start: f64,
    preview: Option<String>,
    iconset: Option<String>,
}

const HELP: &str = "\
possum - a screaming pixel-art possum runs counterclockwise around your screen

USAGE:
    possum [OPTIONS]

OPTIONS:
    --scale <N>          Size of one sprite pixel in points (default 3)
    --speed <N>          Running speed in points per second (default 260)
    --volume <0..1>      Scream volume (default 0.6)
    --silent             Start in \"Scream silently\" mode: a speech bubble
                         instead of sound (switch in the menu bar too)
    --start <0..1>       Where on the border to start, as a fraction of one
                         lap from the bottom-left corner (default 0)
    --preview <FILE>     Write a PNG sprite sheet and exit
    --iconset <DIR>      Write a macOS .iconset folder and exit
    -h, --help           Show this help

Quit from the possum icon in the menu bar, or with Ctrl+C in the terminal.";

fn parse_args() -> Options {
    let mut o = Options {
        scale: 3.0,
        speed: 260.0,
        volume: 0.6,
        silent: false,
        start: 0.0,
        preview: None,
        iconset: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next().unwrap_or_else(|| {
                eprintln!("possum: {name} needs a value\n\n{HELP}");
                std::process::exit(2)
            })
        };
        let number = |name: &str, v: String| -> f64 {
            v.parse().unwrap_or_else(|_| {
                eprintln!("possum: {name} expects a number, got {v:?}");
                std::process::exit(2)
            })
        };
        match arg.as_str() {
            "--scale" => o.scale = number("--scale", value("--scale")).clamp(1.0, 12.0),
            "--speed" => o.speed = number("--speed", value("--speed")).clamp(10.0, 5000.0),
            "--volume" => o.volume = number("--volume", value("--volume")).clamp(0.0, 1.0) as f32,
            "--silent" | "--mute" => o.silent = true,
            "--start" => o.start = number("--start", value("--start")).rem_euclid(1.0),
            "--preview" => o.preview = Some(value("--preview")),
            "--iconset" => o.iconset = Some(value("--iconset")),
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0)
            }
            // Finder passes this when launching an app bundle on old macOS.
            s if s.starts_with("-psn_") => {}
            other => {
                eprintln!("possum: unknown argument {other:?}\n\n{HELP}");
                std::process::exit(2)
            }
        }
    }
    o
}

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

fn size(w: f64, h: f64) -> CGRect {
    CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h))
}

fn primary_screen_frame(mtm: MainThreadMarker) -> CGRect {
    // The first screen is the one with the menu bar.
    NSScreen::screens(mtm)
        .iter()
        .next()
        .or_else(|| NSScreen::mainScreen(mtm))
        .map(|s| s.frame())
        .unwrap_or_else(|| size(1440.0, 900.0))
}

// ---------------------------------------------------------------------------
// The running, screaming possum
// ---------------------------------------------------------------------------

struct Bubble {
    image: CFRetained<CGImage>,
    w: f64,
    h: f64,
    /// Pointer tip in unit (anchor point) coordinates.
    tip: (f64, f64),
}

struct Possum {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    body: Retained<CALayer>,
    bubble: Retained<CALayer>,
    yell: Retained<CALayer>,

    run_frames: Vec<CFRetained<CGImage>>,
    scream_frames: Vec<CFRetained<CGImage>>,
    yell_frames: Vec<CFRetained<CGImage>>,
    /// Per kind of scream: one bubble per shout, in each of the four
    /// pointer orientations.
    bubbles: [Vec<[Bubble; 4]>; 2],
    /// Per kind of scream: a few pre-synthesised variations.
    sounds: [Vec<(Retained<NSSound>, f64)>; 2],

    scale: f64,
    speed: f64,
    screen: CGRect,
    track: Track,
    distance: f64,

    clock: Instant,
    last_tick: f64,
    next_screen_check: f64,
    scream_until: f64,
    next_scream: f64,
    shown: Option<(bool, usize)>,
    /// The possum alternates: "AAAA", hiss, "AAAA", hiss...
    next_kind: Kind,
    kind: Kind,
    shout: usize,
    bubble_shown: Option<(Kind, usize, usize)>,
    rng: Rng,
}

impl Possum {
    fn new(mtm: MainThreadMarker, opts: &Options) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u32)
            .unwrap_or(0xC0FFEE);
        let mut rng = Rng::new(seed);
        let scale = opts.scale;

        // --- art ---------------------------------------------------------
        let art = sprite::possum();
        let run_frames = art.run.iter().map(|c| cg_image(c, 1)).collect();
        let scream_frames = art.scream.iter().map(|c| cg_image(c, 1)).collect();
        let yell_frames = (0..2).map(|f| cg_image(&sprite::yell(f), 1)).collect();
        let bubble_set = |texts: &[&str]| -> Vec<[Bubble; 4]> {
            texts
                .iter()
                .map(|text| {
                    sprite::Pointer::ALL.map(|pointer| {
                        let c = sprite::bubble(text, pointer);
                        Bubble {
                            image: cg_image(&c, 1),
                            w: c.w as f64,
                            h: c.h as f64,
                            tip: pointer.tip(c.w),
                        }
                    })
                })
                .collect()
        };
        let bubbles = [bubble_set(&AAAA_SHOUTS), bubble_set(&HISS_SHOUTS)];

        // --- sound ---------------------------------------------------------
        let load = |s: scream::Scream| {
            let sound = NSSound::initWithData(NSSound::alloc(), &NSData::with_bytes(&s.wav))
                .expect("NSSound could not decode the synthesised WAV");
            sound.setVolume(opts.volume);
            (sound, s.seconds)
        };
        let mut aaaas = Vec::new();
        let mut hisses = Vec::new();
        for i in 0..3u32 {
            // Pitch and length of the "AAAA", as in the first version.
            let pitch = rng.range(560.0, 760.0);
            let seconds = rng.range(1.1, 1.7);
            aaaas.push(load(scream::aaaa(seed.wrapping_add(i * 7919), pitch, seconds)));
            let seconds = rng.range(1.0, 1.6);
            hisses.push(load(scream::hiss(seed.wrapping_add(104_729 + i * 7919), seconds)));
        }
        let sounds = [aaaas, hisses];

        // --- window: transparent, borderless, click-through overlay --------
        let screen = primary_screen_frame(mtm);
        // SAFETY: standard NSWindow initialiser; we keep the window alive and
        // opt out of "released when closed" semantics below.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                screen,
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
        window.setFrame_display(screen, false);

        let view = window.contentView().expect("window has a content view");
        view.setWantsLayer(true);
        let root = view.layer().expect("layer-backed view");

        // --- layers --------------------------------------------------------
        let (pw, ph) = (sprite::POSSUM_W as f64, sprite::POSSUM_H as f64);
        let body = CALayer::new();
        body.setBounds(size(pw * scale, ph * scale));
        // Rotate and position around the paws (layer unit space is y-up).
        body.setAnchorPoint(CGPoint::new(
            sprite::FEET_X as f64 / pw,
            1.0 - sprite::FEET_Y as f64 / ph,
        ));
        pixelated(&body);
        root.addSublayer(&body);

        // The bubble is a sibling of the body rather than a child, so it does
        // not inherit the rotation and its text always reads upright.
        let bubble = CALayer::new();
        pixelated(&bubble);
        bubble.setHidden(true);
        root.addSublayer(&bubble);

        let yell = CALayer::new();
        pixelated(&yell);
        yell.setHidden(true);
        let ycanvas = sprite::yell(0);
        yell.setBounds(size(ycanvas.w as f64 * scale, ycanvas.h as f64 * scale));
        yell.setAnchorPoint(CGPoint::new(0.0, 0.5));
        let (mx, my) = sprite::MOUTH_ANCHOR;
        yell.setPosition(CGPoint::new(mx as f64 * scale, (ph - my as f64) * scale));
        body.addSublayer(&yell);

        window.orderFrontRegardless();

        let track = Self::make_track(screen, scale);
        let mut possum = Self {
            mtm,
            window,
            body,
            bubble,
            yell,
            run_frames,
            scream_frames,
            yell_frames,
            bubbles,
            sounds,
            scale,
            speed: opts.speed,
            screen,
            track,
            distance: opts.start * track.perimeter(),
            clock: Instant::now(),
            last_tick: 0.0,
            next_screen_check: 1.0,
            scream_until: 0.0,
            // It screams right away.
            next_scream: 0.25,
            shown: None,
            // It opens with an "AAAA".
            next_kind: Kind::Aaaa,
            kind: Kind::Aaaa,
            shout: 0,
            bubble_shown: None,
            rng,
        };
        possum.tick();
        possum
    }

    fn make_track(screen: CGRect, scale: f64) -> Track {
        // Turn corners on an arc about half a body length wide.
        Track::new(screen.size.width, screen.size.height, 26.0 * scale)
    }

    /// Follow display changes (resolution, arrangement, plugging monitors).
    fn check_screen(&mut self) {
        let frame = primary_screen_frame(self.mtm);
        if frame != self.screen {
            let progress = self.distance / self.track.perimeter();
            self.screen = frame;
            self.track = Self::make_track(frame, self.scale);
            self.distance = progress * self.track.perimeter();
            self.window.setFrame_display(frame, true);
        }
    }

    fn start_scream(&mut self, now: f64) {
        self.kind = self.next_kind;
        self.next_kind = match self.kind {
            Kind::Aaaa => Kind::Hiss,
            Kind::Hiss => Kind::Aaaa,
        };
        let k = kind_index(self.kind);
        let pick = self.rng.next_u32() as usize % self.sounds[k].len();
        let (sound, seconds) = &self.sounds[k][pick];
        if !SILENT.load(Ordering::Relaxed) {
            sound.stop();
            if !sound.play() {
                eprintln!("possum: could not play the scream (no audio output?)");
            }
        }
        self.scream_until = now + seconds;
        // Catch its breath, then scream again.
        self.next_scream = self.scream_until + self.rng.range(1.0, 3.0) as f64;

        self.shout = self.rng.next_u32() as usize % self.bubbles[k].len();
    }

    /// Converts a point on the sprite (in sprite pixels from its top-left)
    /// into window coordinates, given where the paws are and which way the
    /// possum is heading.
    fn sprite_to_window(&self, paws: (f64, f64), heading: f64, (px, py): (f32, f32)) -> (f64, f64) {
        let dx = (px - sprite::FEET_X) as f64 * self.scale;
        let dy = (sprite::FEET_Y - py) as f64 * self.scale; // y-up
        let (sin, cos) = heading.sin_cos();
        (paws.0 + dx * cos - dy * sin, paws.1 + dx * sin + dy * cos)
    }

    /// Places the upright speech bubble ahead of the head and towards the
    /// middle of the screen, whichever edge the possum is on.
    fn place_bubble(&mut self, paws: (f64, f64), heading: f64) {
        // Quadrant of the heading: 0 = bottom edge (→), 1 = right edge (↑),
        // 2 = top edge (←), 3 = left edge (↓). Same order as Pointer::ALL.
        let quadrant = ((heading / std::f64::consts::FRAC_PI_2 + 0.5).floor() as i64)
            .rem_euclid(4) as usize;
        let b = &self.bubbles[kind_index(self.kind)][self.shout][quadrant];
        let (w, h) = (b.w * self.scale, b.h * self.scale);
        let (tx, ty) = self.sprite_to_window(paws, heading, sprite::BUBBLE_ANCHOR);
        // Keep the whole bubble on screen, even right next to a corner.
        let sw = self.screen.size.width;
        let sh = self.screen.size.height;
        let left = (tx - b.tip.0 * w).clamp(0.0, (sw - w).max(0.0));
        let bottom = (ty - b.tip.1 * h).clamp(0.0, (sh - h).max(0.0));

        if self.bubble_shown != Some((self.kind, self.shout, quadrant)) {
            self.bubble_shown = Some((self.kind, self.shout, quadrant));
            self.bubble.setBounds(size(w, h));
            self.bubble.setAnchorPoint(CGPoint::new(0.0, 0.0));
            unsafe { self.bubble.setContents(Some(as_object(&b.image))) };
        }
        self.bubble.setPosition(CGPoint::new(left.round(), bottom.round()));
    }

    fn tick(&mut self) {
        let now = self.clock.elapsed().as_secs_f64();
        let dt = (now - self.last_tick).clamp(0.0, 0.1);
        self.last_tick = now;

        if now >= self.next_screen_check {
            self.next_screen_check = now + 1.0;
            self.check_screen();
        }

        if SCREAM_NOW.swap(false, Ordering::Relaxed) {
            self.next_scream = now;
        }
        if now >= self.next_scream {
            self.start_scream(now);
        }
        let screaming = now < self.scream_until;
        let silent = SILENT.load(Ordering::Relaxed);
        if silent {
            // Switched to "Scream silently" mid-scream: hush at once.
            for (sound, _) in self.sounds.iter().flatten() {
                if sound.isPlaying() {
                    sound.stop();
                }
            }
        }

        // Panic makes it run a bit faster.
        let speed = self.speed * if screaming { 1.3 } else { 1.0 };
        self.distance += speed * dt;
        let pose = self.track.pose(self.distance);

        // Advance the run cycle by distance so the legs keep up with the
        // speed (one full cycle per ~2 body lengths... of tiny legs).
        let cycle = 32.0 * self.scale;
        let frame = ((self.distance / cycle) * sprite::RUN_FRAMES as f64) as usize
            % sprite::RUN_FRAMES;

        let (mut x, mut y) = (pose.x, pose.y);
        if screaming {
            // Trembling with the sheer force of the scream.
            let j = self.scale * 0.6;
            x += self.rng.range(-j as f32, j as f32) as f64;
            y += self.rng.range(-j as f32, j as f32) as f64;
        }

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.body.setPosition(CGPoint::new(x.round(), y.round()));
        self.body.setAffineTransform(rotation(pose.heading));
        if self.shown != Some((screaming, frame)) {
            self.shown = Some((screaming, frame));
            let frames = if screaming { &self.scream_frames } else { &self.run_frames };
            unsafe { self.body.setContents(Some(as_object(&frames[frame]))) };
        }
        // The bubble is how a silent scream is "heard".
        let show_bubble = screaming && silent;
        self.bubble.setHidden(!show_bubble);
        self.yell.setHidden(!screaming);
        if show_bubble {
            self.place_bubble((x, y), pose.heading);
        }
        if screaming {
            let f = (now * 10.0) as usize % self.yell_frames.len();
            unsafe { self.yell.setContents(Some(as_object(&self.yell_frames[f]))) };
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
                        item.setState(if item.tag() == mode {
                            NSControlStateValueOn
                        } else {
                            NSControlStateValueOff
                        });
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

fn install_status_item(
    mtm: MainThreadMarker,
    app: &NSApplication,
    target: &MenuTarget,
) -> Retained<NSStatusItem> {
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
    for (title, tag, checked) in [
        ("Scream", TAG_SCREAM, !silent),
        ("Scream silently", TAG_SCREAM_SILENTLY, silent),
    ] {
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
// Offline outputs: preview sheet and icon set
// ---------------------------------------------------------------------------

fn write_png(path: &std::path::Path, c: &Canvas, scale: usize, bg: Option<[u8; 3]>) {
    let mut rgba = c.rgba_scaled(scale);
    if let Some([r, g, b]) = bg {
        for px in rgba.chunks_mut(4) {
            if px[3] == 0 {
                px.copy_from_slice(&[r, g, b, 255]);
            }
        }
    }
    let png = png::encode_rgba(c.w as u32 * scale as u32, c.h as u32 * scale as u32, &rgba);
    std::fs::write(path, png).unwrap_or_else(|e| {
        eprintln!("possum: cannot write {}: {e}", path.display());
        std::process::exit(1)
    });
}

/// Sprite sheet: 8 run frames, 8 scream frames, and a composed scream pose.
fn write_preview(path: &str) {
    let art = sprite::possum();
    let (cw, ch) = (sprite::POSSUM_W + 4, sprite::POSSUM_H + 4);
    let bubble = sprite::bubble(AAAA_SHOUTS[0], sprite::Pointer::BottomLeft);
    let composed_h = sprite::POSSUM_H + bubble.h;
    let mut sheet = Canvas::new(cw * 4, ch * 4 + composed_h + 4);
    for (i, frame) in art.run.iter().chain(art.scream.iter()).enumerate() {
        let i = i as i32;
        sheet.blit(frame, (i % 4) * cw + 2, (i / 4) * ch + 2);
    }
    // The full screaming pose, laid out like the layers in the app.
    let (ox, oy) = (2, ch * 4 + bubble.h);
    sheet.blit(&art.scream[0], ox, oy);
    let (bx, by) = sprite::BUBBLE_ANCHOR;
    sheet.blit(&bubble, ox + bx as i32 - 1, oy + by as i32 - bubble.h);
    let yell = sprite::yell(1);
    let (mx, my) = sprite::MOUTH_ANCHOR;
    sheet.blit(&yell, ox + mx as i32, oy + my as i32 - yell.h / 2);
    write_png(path.as_ref(), &sheet, 6, Some([0x7a, 0xa5, 0xd6]));
    println!("wrote {path}");
}

fn write_iconset(dir: &str) {
    let dir = std::path::Path::new(dir);
    std::fs::create_dir_all(dir).expect("create iconset dir");
    let icon = sprite::app_icon(); // 32x32
    // (file name, pixel size); every size is an integer multiple of 32 except
    // the 16px ones, which get a 2:1 nearest-neighbour downscale.
    let sizes = [
        ("icon_16x16.png", 16),
        ("icon_16x16@2x.png", 32),
        ("icon_32x32.png", 32),
        ("icon_32x32@2x.png", 64),
        ("icon_128x128.png", 128),
        ("icon_128x128@2x.png", 256),
        ("icon_256x256.png", 256),
        ("icon_256x256@2x.png", 512),
        ("icon_512x512.png", 512),
        ("icon_512x512@2x.png", 1024),
    ];
    for (name, px) in sizes {
        let path = dir.join(name);
        if px >= 32 {
            write_png(&path, &icon, px / 32, None);
        } else {
            let full = icon.rgba();
            let mut half = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16 {
                for x in 0..16 {
                    let i = ((y * 2) * 32 + x * 2) * 4;
                    half.extend_from_slice(&full[i..i + 4]);
                }
            }
            std::fs::write(&path, png::encode_rgba(16, 16, &half)).expect("write icon");
        }
    }
    println!("wrote {}", dir.display());
}

// ---------------------------------------------------------------------------

fn main() {
    let opts = parse_args();
    if let Some(path) = &opts.preview {
        return write_preview(path);
    }
    if let Some(dir) = &opts.iconset {
        return write_iconset(dir);
    }
    SILENT.store(opts.silent, Ordering::Relaxed);

    let mtm = MainThreadMarker::new().expect("possum must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon: the possum lives on the screen edge and in the menu bar.
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let possum = RefCell::new(Possum::new(mtm, &opts));
    let target = MenuTarget::new(mtm);
    let _status_item = install_status_item(mtm, &app, &target);

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
    fn every_shout_can_be_drawn_in_the_pixel_font() {
        for text in AAAA_SHOUTS.iter().chain(HISS_SHOUTS.iter()) {
            for ch in text.chars() {
                assert!(sprite::has_glyph(ch), "no glyph for {ch:?} in {text:?}");
            }
        }
    }
}

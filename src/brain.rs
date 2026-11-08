//! The possum's brain: where it runs, when it screams, and when it keeps
//! quiet, without any operating-system code. Each platform (see
//! `platform/`) feeds it the time, display changes and system events, gives
//! it a way to play sounds (`Host`), and draws the `Scene` it returns.
//!
//! Coordinates are y-up with the origin at the bottom-left of the screen (as
//! on macOS); platforms with y-down screens flip them when drawing. Lengths
//! are in the platform's screen units: points on macOS, pixels elsewhere.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::quiet::{self, Quiet};
use crate::scream::{self, Kind, Rng};
use crate::sprite;
use crate::track::Track;

/// Requests from the menu (or tray). `SILENT` picks the scream mode: `false`
/// is "Scream" (you hear it, no speech bubble), `true` is "Scream silently"
/// (you read it in a bubble, no sound).
pub static SILENT: AtomicBool = AtomicBool::new(false);
pub static SCREAM_NOW: AtomicBool = AtomicBool::new(false);
/// Set by the Linux tray's "Quit" (macOS and Windows quit through their own
/// event loops).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub static QUIT: AtomicBool = AtomicBool::new(false);

/// What the speech bubble says in "Scream silently" mode, per kind of scream.
pub const AAAA_SHOUTS: [&str; 6] = ["AAAAAA!", "AAAAAAAAH!", "AAAA!!", "AAARGH!", "AAAAAAAAAAA!", "HAAAA!"];
pub const HISS_SHOUTS: [&str; 3] = ["HSSSSS!", "HSSSSSSSS!", "SSSSSSS!"];

/// Index of a scream kind into the per-kind sound and bubble tables.
pub fn kind_index(kind: Kind) -> usize {
    match kind {
        Kind::Aaaa => 0,
        Kind::Hiss => 1,
    }
}

/// The shouts for each kind, in `kind_index` order.
pub fn shouts() -> [&'static [&'static str]; 2] {
    [&AAAA_SHOUTS, &HISS_SHOUTS]
}

/// The sounds, synthesised once at start-up: per kind of scream, a few
/// variations (in `kind_index` order).
pub fn voices(seed: u32, rng: &mut Rng) -> [Vec<scream::Scream>; 2] {
    let mut aaaas = Vec::new();
    let mut hisses = Vec::new();
    for i in 0..3u32 {
        // Pitch and length of the "AAAA", as in the first version.
        let pitch = rng.range(560.0, 760.0);
        let seconds = rng.range(1.1, 1.7);
        aaaas.push(scream::aaaa(seed.wrapping_add(i * 7919), pitch, seconds));
        let seconds = rng.range(1.0, 1.6);
        hisses.push(scream::hiss(seed.wrapping_add(104_729 + i * 7919), seconds));
    }
    [aaaas, hisses]
}

/// What the brain needs from the platform.
pub trait Host {
    /// Starts playing a sound; false if it couldn't.
    fn play(&mut self, kind: Kind, variant: usize) -> bool;
    /// Stops every sound that's playing.
    fn hush(&mut self);
    /// A fresh reading of the lock/screensaver/display state.
    fn probe(&mut self) -> quiet::Probe;
    /// False when there's no way to play sound at all (no audio player).
    fn has_audio(&self) -> bool {
        true
    }
}

/// Settings from the command line, already in screen units.
pub struct Settings {
    /// Screen units per sprite pixel.
    pub scale: f64,
    /// Screen units per second.
    pub speed: f64,
    /// Where on the border to start, as a fraction of one lap.
    pub start: f64,
    pub verbose: bool,
}

/// The upright speech bubble, when one is showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bubble {
    pub kind: Kind,
    pub shout: usize,
    /// Which corner its pointer is on, in `sprite::Pointer::ALL` order.
    pub quadrant: usize,
    pub left: f64,
    pub bottom: f64,
    pub w: f64,
    pub h: f64,
}

/// What to draw this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scene {
    /// Where the paws touch the screen edge (y-up), rounded to whole units.
    pub x: f64,
    pub y: f64,
    /// Direction of travel, counterclockwise from +x.
    pub heading: f64,
    pub screaming: bool,
    /// Run-cycle frame.
    pub frame: usize,
    /// Yell-lines frame, while screaming.
    pub yell: Option<usize>,
    pub bubble: Option<Bubble>,
    /// Once a second: a good moment to check whether displays changed.
    pub check_displays: bool,
}

/// One bubble's size in sprite pixels and its pointer tip in unit space.
#[derive(Clone, Copy)]
struct BubbleSize {
    w: f64,
    h: f64,
    tip: (f64, f64),
}

pub struct Brain {
    scale: f64,
    speed: f64,
    verbose: bool,
    width: f64,
    height: f64,
    track: Track,
    distance: f64,

    last_tick: f64,
    next_check: f64,
    scream_until: f64,
    next_scream: f64,
    was_silent: bool,
    quiet: Quiet,
    was_quiet: Option<quiet::Reason>,
    /// The possum alternates: "AAAA", hiss, "AAAA", hiss...
    next_kind: Kind,
    kind: Kind,
    shout: usize,

    durations: [Vec<f64>; 2],
    bubble_sizes: [Vec<[BubbleSize; 4]>; 2],
    rng: Rng,
}

impl Brain {
    /// `durations` are the lengths of the sounds from `voices`.
    pub fn new(settings: &Settings, width: f64, height: f64, durations: [Vec<f64>; 2], rng: Rng) -> Self {
        let track = make_track(width, height, settings.scale);
        let bubble_sizes = shouts().map(|texts| {
            texts
                .iter()
                .map(|text| {
                    sprite::Pointer::ALL.map(|pointer| {
                        let c = sprite::bubble(text, pointer);
                        BubbleSize { w: c.w as f64, h: c.h as f64, tip: pointer.tip(c.w) }
                    })
                })
                .collect()
        });
        Self {
            scale: settings.scale,
            speed: settings.speed,
            verbose: settings.verbose,
            width,
            height,
            distance: settings.start * track.perimeter(),
            track,
            last_tick: 0.0,
            next_check: 0.0,
            scream_until: 0.0,
            // It screams right away...
            next_scream: 0.25,
            was_silent: SILENT.load(Ordering::Relaxed),
            quiet: Quiet::default(),
            was_quiet: None,
            // ...and opens with an "AAAA".
            next_kind: Kind::Aaaa,
            kind: Kind::Aaaa,
            shout: 0,
            durations,
            bubble_sizes,
            rng,
        }
    }

    pub fn log(&self, message: std::fmt::Arguments) {
        if self.verbose {
            eprintln!("possum [{:8.2}s] {message}", self.last_tick);
        }
    }

    /// Something the system told the platform (lock, screensaver...). Linux
    /// only probes, so it has no events.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    pub fn apply(&mut self, event: quiet::Event) {
        self.log(format_args!("the system says: {event:?}"));
        self.quiet.apply(event);
    }

    /// A direct reading of the system state (see quiet.rs).
    pub fn update(&mut self, probe: quiet::Probe) {
        self.quiet.update(probe);
    }

    #[cfg(test)]
    pub fn screen_size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// The primary display changed size: keep running from the same spot on
    /// the same edge of the new one.
    pub fn set_screen(&mut self, width: f64, height: f64) {
        let track = make_track(width, height, self.scale);
        self.distance = self.track.carry_over(self.distance, &track);
        self.track = track;
        self.width = width;
        self.height = height;
        self.log(format_args!("following the primary display: now {width}x{height}"));
    }

    pub fn tick(&mut self, now: f64, host: &mut dyn Host) -> Scene {
        let dt = (now - self.last_tick).clamp(0.0, 0.1);
        self.last_tick = now;

        // Re-read the real state every second, in case a notification never
        // came (and the platform checks its displays at the same moment).
        let check_displays = now >= self.next_check;
        if check_displays {
            self.next_check = now + 1.0;
            self.quiet.update(host.probe());
        }
        let quiet = self.quiet.reason();
        if quiet != self.was_quiet {
            match quiet {
                Some(reason) => {
                    // Locked, screensaver, display off...: stop mid-scream.
                    host.hush();
                    self.log(format_args!("keeping quiet: {}", reason.describe()));
                }
                None => self.log(format_args!("free to scream again")),
            }
            self.was_quiet = quiet;
        }

        if SCREAM_NOW.swap(false, Ordering::Relaxed) {
            self.next_scream = now;
        }
        if now >= self.next_scream {
            self.start_scream(now, host);
        }
        let screaming = now < self.scream_until;
        let silent = SILENT.load(Ordering::Relaxed);
        if silent && !self.was_silent {
            // Switched to "Scream silently" mid-scream: hush at once.
            host.hush();
        }
        self.was_silent = silent;

        // Panic makes it run a bit faster.
        let speed = self.speed * if screaming { 1.3 } else { 1.0 };
        self.distance += speed * dt;
        let pose = self.track.pose(self.distance);

        // Advance the run cycle by distance so the legs keep up with the
        // speed (one full cycle per ~2 body lengths... of tiny legs).
        let cycle = 32.0 * self.scale;
        let frame = ((self.distance / cycle) * sprite::RUN_FRAMES as f64) as usize % sprite::RUN_FRAMES;

        let (mut x, mut y) = (pose.x, pose.y);
        if screaming {
            // Trembling with the sheer force of the scream.
            let j = self.scale * 0.6;
            x += self.rng.range(-j as f32, j as f32) as f64;
            y += self.rng.range(-j as f32, j as f32) as f64;
        }
        let (x, y) = (x.round(), y.round());

        // The bubble is how a silent scream is "heard".
        let bubble = (screaming && silent).then(|| self.place_bubble((x, y), pose.heading));
        Scene {
            x,
            y,
            heading: pose.heading,
            screaming,
            frame,
            yell: screaming.then_some((now * 10.0) as usize % 2),
            bubble,
            check_displays,
        }
    }

    fn start_scream(&mut self, now: f64, host: &mut dyn Host) {
        self.kind = self.next_kind;
        self.next_kind = match self.kind {
            Kind::Aaaa => Kind::Hiss,
            Kind::Hiss => Kind::Aaaa,
        };
        let k = kind_index(self.kind);
        let variant = self.rng.next_u32() as usize % self.durations[k].len();
        // A new scream cuts off the last one (which is always the other kind).
        host.hush();
        // Look again right before making noise, so a lock or screensaver
        // that started a moment ago is never missed.
        self.quiet.update(host.probe());
        let what = match self.kind {
            Kind::Aaaa => "AAAA",
            Kind::Hiss => "hiss",
        };
        if let Some(reason) = self.quiet.reason() {
            self.log(format_args!("{what}: no sound, because {}", reason.describe()));
        } else if SILENT.load(Ordering::Relaxed) {
            self.log(format_args!("{what}: silently, in a speech bubble"));
        } else if !host.has_audio() {
            self.log(format_args!("{what}: no sound, because there's no audio player"));
        } else if host.play(self.kind, variant) {
            self.log(format_args!("{what}: out loud"));
        } else {
            eprintln!("possum: could not play the scream (no audio output?)");
        }
        self.scream_until = now + self.durations[k][variant];
        // Catch its breath, then scream again.
        self.next_scream = self.scream_until + self.rng.range(1.0, 3.0) as f64;
        self.shout = self.rng.next_u32() as usize % self.bubble_sizes[k].len();
    }

    /// Converts a point on the sprite (in sprite pixels from its top-left)
    /// into screen coordinates, given where the paws are and which way the
    /// possum is heading.
    fn sprite_to_screen(&self, paws: (f64, f64), heading: f64, (px, py): (f32, f32)) -> (f64, f64) {
        let dx = (px - sprite::FEET_X) as f64 * self.scale;
        let dy = (sprite::FEET_Y - py) as f64 * self.scale; // y-up
        let (sin, cos) = heading.sin_cos();
        (paws.0 + dx * cos - dy * sin, paws.1 + dx * sin + dy * cos)
    }

    /// Places the upright speech bubble ahead of the head and towards the
    /// middle of the screen, whichever edge the possum is on.
    fn place_bubble(&self, paws: (f64, f64), heading: f64) -> Bubble {
        // Quadrant of the heading: 0 = bottom edge (→), 1 = right edge (↑),
        // 2 = top edge (←), 3 = left edge (↓). Same order as Pointer::ALL.
        let quadrant = ((heading / std::f64::consts::FRAC_PI_2 + 0.5).floor() as i64).rem_euclid(4) as usize;
        let b = self.bubble_sizes[kind_index(self.kind)][self.shout][quadrant];
        let (w, h) = (b.w * self.scale, b.h * self.scale);
        let (tx, ty) = self.sprite_to_screen(paws, heading, sprite::BUBBLE_ANCHOR);
        // Keep the whole bubble on screen, even right next to a corner.
        let left = (tx - b.tip.0 * w).clamp(0.0, (self.width - w).max(0.0));
        let bottom = (ty - b.tip.1 * h).clamp(0.0, (self.height - h).max(0.0));
        Bubble { kind: self.kind, shout: self.shout, quadrant, left: left.round(), bottom: bottom.round(), w, h }
    }
}

fn make_track(width: f64, height: f64, scale: f64) -> Track {
    // Turn corners on an arc about half a body length wide.
    Track::new(width, height, 26.0 * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiet::Probe;
    use std::sync::Mutex;

    /// The brain's controls are process-wide; tests that touch them take
    /// turns.
    static CONTROLS: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct FakeHost {
        played: Vec<(Kind, usize)>,
        hushes: usize,
        probe: Probe,
        no_audio: bool,
    }

    impl Host for FakeHost {
        fn play(&mut self, kind: Kind, variant: usize) -> bool {
            self.played.push((kind, variant));
            true
        }
        fn hush(&mut self) {
            self.hushes += 1;
        }
        fn probe(&mut self) -> Probe {
            self.probe
        }
        fn has_audio(&self) -> bool {
            !self.no_audio
        }
    }

    fn brain() -> Brain {
        let settings = Settings { scale: 3.0, speed: 260.0, start: 0.0, verbose: false };
        Brain::new(&settings, 1512.0, 982.0, [vec![1.2; 3], vec![1.0; 3]], Rng::new(7))
    }

    /// Runs the brain at 60 Hz from `from` to `to` seconds.
    fn run(b: &mut Brain, host: &mut FakeHost, from: f64, to: f64) -> Vec<Scene> {
        let mut scenes = Vec::new();
        let mut t = from;
        while t < to {
            scenes.push(b.tick(t, host));
            t += 1.0 / 60.0;
        }
        scenes
    }

    fn reset_controls() -> std::sync::MutexGuard<'static, ()> {
        let guard = CONTROLS.lock().unwrap_or_else(|e| e.into_inner());
        SILENT.store(false, Ordering::Relaxed);
        SCREAM_NOW.store(false, Ordering::Relaxed);
        guard
    }

    #[test]
    fn screams_right_away_then_alternates_aaaa_and_hiss() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 30.0);
        assert!(host.played.len() >= 5, "only {} screams in 30 s", host.played.len());
        assert_eq!(host.played[0].0, Kind::Aaaa);
        for pair in host.played.windows(2) {
            assert_ne!(pair[0].0, pair[1].0, "two of the same kind in a row");
        }
    }

    #[test]
    fn runs_counterclockwise_with_paws_on_the_screen_edge() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        let scenes = run(&mut b, &mut host, 0.0, 3.0);
        let first = scenes.first().unwrap();
        let later = scenes.last().unwrap();
        // Starts on the bottom edge, heading right (allowing for the
        // scream's trembling).
        assert!(first.y.abs() <= 2.0 && first.heading == 0.0, "{first:?}");
        assert!(later.x > first.x, "should move right along the bottom");
    }

    #[test]
    fn keeps_quiet_while_locked_and_says_so() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        host.probe = Probe { screen_locked: Some(true), ..Probe::default() };
        run(&mut b, &mut host, 0.0, 20.0);
        assert!(host.played.is_empty(), "played {:?} while locked", host.played);
        // Unlocked: screams again.
        host.probe = Probe { screen_locked: Some(false), ..Probe::default() };
        run(&mut b, &mut host, 20.0, 30.0);
        assert!(!host.played.is_empty());
    }

    #[test]
    fn locking_mid_scream_cuts_it_off() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 0.5); // the first "AAAA" is playing
        assert_eq!(host.played.len(), 1);
        let hushes = host.hushes;
        b.apply(quiet::Event::ScreenLocked);
        b.tick(0.6, &mut host);
        assert!(host.hushes > hushes, "the scream should have been cut off");
    }

    #[test]
    fn scream_silently_shows_a_bubble_and_plays_nothing() {
        let _g = reset_controls();
        SILENT.store(true, Ordering::Relaxed);
        let (mut b, mut host) = (brain(), FakeHost::default());
        let scenes = run(&mut b, &mut host, 0.0, 10.0);
        assert!(host.played.is_empty());
        let with_bubble: Vec<_> = scenes.iter().filter_map(|s| s.bubble).collect();
        assert!(!with_bubble.is_empty());
        // The bubble always stays on screen.
        for bub in with_bubble {
            assert!(bub.left >= 0.0 && bub.bottom >= 0.0, "{bub:?}");
            assert!(bub.left + bub.w <= 1512.0 + 0.5 && bub.bottom + bub.h <= 982.0 + 0.5, "{bub:?}");
        }
        SILENT.store(false, Ordering::Relaxed);
    }

    #[test]
    fn without_an_audio_player_it_screams_without_trying_to_play() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost { no_audio: true, ..Default::default() });
        let scenes = run(&mut b, &mut host, 0.0, 10.0);
        assert!(host.played.is_empty());
        // It still screams visibly.
        assert!(scenes.iter().any(|s| s.screaming));
    }

    #[test]
    fn scream_now_screams_immediately_with_the_next_kind() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 0.5);
        assert_eq!(host.played.len(), 1);
        SCREAM_NOW.store(true, Ordering::Relaxed);
        b.tick(0.6, &mut host);
        assert_eq!(host.played.len(), 2);
        assert_eq!(host.played[1].0, Kind::Hiss);
    }

    #[test]
    fn a_new_screen_keeps_the_possum_on_the_same_edge() {
        let _g = reset_controls();
        let settings = Settings { scale: 3.0, speed: 260.0, start: 0.4, verbose: false };
        let mut b = Brain::new(&settings, 1512.0, 982.0, [vec![1.2; 3], vec![1.0; 3]], Rng::new(1));
        let mut host = FakeHost { probe: Probe { screen_locked: Some(true), ..Probe::default() }, ..Default::default() };
        let before = b.tick(0.0, &mut host);
        b.set_screen(3440.0, 1440.0);
        let after = b.tick(0.01, &mut host);
        assert!((before.heading - after.heading).abs() < 1e-9, "{before:?} -> {after:?}");
        assert_eq!(b.screen_size(), (3440.0, 1440.0));
    }
}

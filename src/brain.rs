//! The possum's brain: where it runs, when it screams, and when it keeps
//! quiet, without any operating-system code. Each platform (see
//! `platform/`) feeds it the time, display changes and system events, gives
//! it a way to play sounds (`Host`), and draws the `Scene` it returns.
//!
//! Coordinates are y-up with the origin at the bottom-left of the screen (as
//! on macOS); platforms with y-down screens flip them when drawing. Lengths
//! are in the platform's screen units: points on macOS, pixels elsewhere.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::quiet::{self, Hush, Quiet};
use crate::scream::{self, Kind, Rng};
use crate::sprite;
use crate::track::Track;

/// Requests from the menu (or tray). `SILENT` picks the scream mode: `false`
/// is "Scream" (you hear it, no speech bubble), `true` is "Scream silently"
/// (you read it in a bubble, no sound).
pub static SILENT: AtomicBool = AtomicBool::new(false);
pub static SCREAM_NOW: AtomicBool = AtomicBool::new(false);
/// "Stop" / "Run": `true` once Stop is picked. The possum then shuts its
/// mouth and slows to a stand within a stride, and stays put (screaming
/// only when asked to, with Scream Now) until Run is picked.
pub static STOPPED: AtomicBool = AtomicBool::new(false);
/// The controls above are process-wide: tests that touch them (here and in
/// the Android library) take turns.
#[cfg(test)]
pub static TEST_CONTROLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Whether the system asks for quiet right now (`Hush::code`), for the menu:
/// then it screams silently whatever `SILENT` says.
static HUSH: AtomicU8 = AtomicU8::new(0);

/// Why the system asks for quiet right now, if it does (see `quiet::Hush`).
pub fn hush() -> Option<Hush> {
    Hush::from_code(HUSH.load(Ordering::Relaxed))
}

/// Whether it screams silently right now, picked or not: what the menu
/// shows as chosen.
#[cfg_attr(all(target_os = "macos", not(test)), expect(dead_code))]
pub fn silent_now() -> bool {
    SILENT.load(Ordering::Relaxed) || hush().is_some()
}
/// Set by the Linux tray's "Quit" (macOS and Windows quit through their own
/// event loops).
#[cfg_attr(not(target_os = "linux"), expect(dead_code))]
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

/// Running, or stopping (see `STOPPED`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    Running,
    /// Slowing down evenly, to stand `left` further on: where its paws are
    /// on the ground between strides. `from` is how far that was at first.
    Stopping {
        left: f64,
        from: f64,
    },
    Standing,
}

/// How long it takes to get back up to full speed.
const SET_OFF_SECONDS: f64 = 0.3;

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
    motion: Motion,
    /// Fraction of full speed: 1 running, less while stopping or setting
    /// off again.
    pace: f64,

    last_tick: f64,
    next_check: f64,
    scream_until: f64,
    next_scream: f64,
    /// No screams for now (see `hold_screams`).
    holding: bool,
    was_silent: bool,
    quiet: Quiet,
    was_quiet: Option<quiet::Reason>,
    was_hush: Option<Hush>,
    /// This scream is out loud although the system asks for quiet: "Scream
    /// Now" was picked.
    heard_anyway: bool,
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
            motion: Motion::Running,
            pace: 1.0,
            last_tick: 0.0,
            next_check: 0.0,
            scream_until: 0.0,
            // It screams right away...
            next_scream: 0.25,
            holding: false,
            was_silent: SILENT.load(Ordering::Relaxed),
            quiet: Quiet::default(),
            was_quiet: None,
            was_hush: None,
            heard_anyway: false,
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
    #[cfg_attr(all(target_os = "linux", not(test)), expect(dead_code))]
    pub fn apply(&mut self, event: quiet::Event) {
        self.log(format_args!("the system says: {event:?}"));
        self.quiet.apply(event);
    }

    /// A direct reading of the system state (see quiet.rs).
    pub fn update(&mut self, probe: quiet::Probe) {
        self.quiet.update(probe);
    }

    /// Holds back screams while the platform settles something first (on
    /// macOS: whether the menu-bar icon fits, which decides whether the very
    /// first scream is out loud). A scream that came due meanwhile starts
    /// as soon as the hold is lifted. The possum keeps running either way.
    #[cfg_attr(all(not(target_os = "macos"), not(test)), expect(dead_code))]
    pub fn hold_screams(&mut self, hold: bool) {
        self.holding = hold;
    }

    #[cfg(test)]
    pub fn screen_size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// Screen units per sprite pixel.
    #[cfg(test)]
    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The primary display changed: keep running from the same spot on the
    /// same edge of the new one, at the size and speed that suit it (see
    /// size.rs).
    pub fn set_screen(&mut self, width: f64, height: f64, scale: f64, speed: f64) {
        if let Motion::Stopping { left, from } = &mut self.motion {
            *left *= scale / self.scale;
            *from *= scale / self.scale;
        }
        let resized = scale != self.scale;
        self.scale = scale;
        self.speed = speed;
        let track = make_track(width, height, scale);
        self.distance = self.track.carry_over(self.distance, &track);
        self.track = track;
        self.width = width;
        self.height = height;
        self.log(format_args!("following the primary display: now {width}x{height}"));
        if resized {
            self.log(format_args!("a sprite pixel is now {scale} units, and it runs {speed:.0} units a second"));
        }
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

        self.follow_stop_and_run(now, host);
        let asked = SCREAM_NOW.swap(false, Ordering::Relaxed);
        if asked {
            self.next_scream = now;
        }
        // Stopped, it only screams when asked.
        let due = now >= self.next_scream && (asked || self.motion == Motion::Running);
        if due && !self.holding {
            self.start_scream(now, host, asked);
        }
        let screaming = now < self.scream_until;
        self.heard_anyway &= screaming;

        // Do Not Disturb and the like: scream silently, as if picked.
        let hush = self.quiet.hush();
        if hush != self.was_hush {
            HUSH.store(Hush::code(hush), Ordering::Relaxed);
            match hush {
                Some(h) => self.log(format_args!("{}: screaming silently", h.describe())),
                None => self.log(format_args!("the system no longer asks for quiet")),
            }
            self.was_hush = hush;
        }
        let silent = SILENT.load(Ordering::Relaxed) || (hush.is_some() && !self.heard_anyway);
        if silent && !self.was_silent {
            // Switched to "Scream silently" (or Do Not Disturb came on)
            // mid-scream: hush at once.
            host.hush();
        }
        self.was_silent = silent;

        // Panic makes it run a bit faster.
        let speed = self.speed * if screaming { 1.3 } else { 1.0 };
        self.distance += self.step(speed, dt);
        let pose = self.track.pose(self.distance);

        // Advance the run cycle by distance so the legs keep up with the
        // speed (one full cycle per ~2 body lengths... of tiny legs).
        let frame = if self.motion == Motion::Standing {
            sprite::STAND
        } else {
            ((self.distance / self.cycle()) * sprite::RUN_FRAMES as f64) as usize % sprite::RUN_FRAMES
        };

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

    /// One run cycle, in screen units (about two body lengths).
    fn cycle(&self) -> f64 {
        32.0 * self.scale
    }

    /// Acts on Stop and Run from the menu.
    fn follow_stop_and_run(&mut self, now: f64, host: &mut dyn Host) {
        let stop = STOPPED.load(Ordering::Relaxed);
        match self.motion {
            Motion::Running if stop => {
                // Mouth shut at once: that's the end of this scream.
                if now < self.scream_until {
                    host.hush();
                    self.scream_until = now;
                }
                let left = self.distance_to_stand();
                self.motion = Motion::Stopping { left, from: left };
                self.log(format_args!("stopping: it stands still {left:.0} units on"));
            }
            Motion::Stopping { .. } | Motion::Standing if !stop => {
                self.motion = Motion::Running;
                // Off again, after a breath (or once a Scream Now is over).
                self.next_scream = self.scream_until.max(now) + self.rng.range(1.0, 3.0) as f64;
                self.log(format_args!("running again"));
            }
            _ => {}
        }
    }

    /// How far it is to the nearest spot ahead where it can stand still:
    /// between strides, where the run cycle starts over (frame 0) or is
    /// halfway (frame 4), so its body doesn't bob. At least a quarter of a
    /// stride on, so it doesn't stop dead.
    fn distance_to_stand(&self) -> f64 {
        let half = self.cycle() / 2.0;
        let left = half - self.distance.rem_euclid(half);
        if left < half / 2.0 { left + half } else { left }
    }

    /// How far it goes this tick at `speed` (screen units a second), slowing
    /// to a stand or picking up speed again as the menu says.
    fn step(&mut self, speed: f64, dt: f64) -> f64 {
        match self.motion {
            Motion::Running => {
                self.pace = (self.pace + dt / SET_OFF_SECONDS).min(1.0);
                speed * self.pace * dt
            }
            Motion::Stopping { left, from } => {
                // Braking evenly: the speed goes with the square root of the
                // distance left. Never quite a crawl, so it gets there.
                self.pace = self.pace.min((left / from).sqrt()).max(0.15);
                let step = (speed * self.pace * dt).min(left);
                if step < left {
                    self.motion = Motion::Stopping { left: left - step, from };
                } else {
                    self.motion = Motion::Standing;
                    self.pace = 0.0;
                    self.log(format_args!("standing still"));
                }
                step
            }
            Motion::Standing => 0.0,
        }
    }

    /// `asked`: "Scream Now" was picked, which is heard even while the
    /// system asks for quiet (except on a call).
    fn start_scream(&mut self, now: f64, host: &mut dyn Host, asked: bool) {
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
        let hush = self.quiet.hush();
        self.heard_anyway = false;
        if let Some(reason) = self.quiet.reason() {
            self.log(format_args!("{what}: no sound, because {}", reason.describe()));
        } else if SILENT.load(Ordering::Relaxed) {
            self.log(format_args!("{what}: silently, in a speech bubble"));
        } else if let Some(h) = hush.filter(|h| !(asked && h.lets_scream_now_through())) {
            self.log(format_args!("{what}: silently, in a speech bubble, because {}", h.describe()));
        } else if !host.has_audio() {
            self.log(format_args!("{what}: no sound, because there's no audio player"));
        } else if host.play(self.kind, variant) {
            self.heard_anyway = hush.is_some();
            match hush {
                Some(h) => self.log(format_args!("{what}: out loud, for Scream Now, although {}", h.describe())),
                None => self.log(format_args!("{what}: out loud")),
            }
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
        let guard = TEST_CONTROLS.lock().unwrap_or_else(|e| e.into_inner());
        SILENT.store(false, Ordering::Relaxed);
        SCREAM_NOW.store(false, Ordering::Relaxed);
        STOPPED.store(false, Ordering::Relaxed);
        HUSH.store(0, Ordering::Relaxed);
        guard
    }

    #[test]
    fn screams_right_away_then_alternates_aaaa_and_hiss() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 30.0);
        assert!(host.played.len() >= 5, "only {} screams in 30 s", host.played.len());
        assert_eq!(host.played[0].0, Kind::Aaaa);
        for [(a, _), (b, _)] in host.played.array_windows() {
            assert_ne!(a, b, "two of the same kind in a row");
        }
    }

    #[test]
    fn a_hold_delays_the_first_scream_until_it_is_lifted() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        b.hold_screams(true);
        let scenes = run(&mut b, &mut host, 0.0, 1.0);
        assert!(host.played.is_empty(), "screamed while held: {:?}", host.played);
        assert!(scenes.last().unwrap().x > scenes[0].x, "the possum should keep running");
        // Lifted: the overdue opening "AAAA" starts right away.
        b.hold_screams(false);
        b.tick(1.0, &mut host);
        assert_eq!(host.played.first().map(|p| p.0), Some(Kind::Aaaa));
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

    fn hushed(h: Hush) -> FakeHost {
        FakeHost { probe: Probe { hush: Some(h), ..Probe::default() }, ..Default::default() }
    }

    #[test]
    fn do_not_disturb_makes_it_scream_silently_and_the_menu_says_so() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), hushed(Hush::DoNotDisturb));
        let scenes = run(&mut b, &mut host, 0.0, 10.0);
        assert!(host.played.is_empty(), "played {:?} in Do Not Disturb", host.played);
        assert!(scenes.iter().any(|s| s.bubble.is_some()), "a silent scream shows a bubble");
        assert_eq!(hush(), Some(Hush::DoNotDisturb));
        assert!(silent_now() && !SILENT.load(Ordering::Relaxed), "the menu shows silent; the choice stays");
        // Off again: out loud, as picked.
        host.probe.hush = None;
        run(&mut b, &mut host, 10.0, 20.0);
        assert!(!host.played.is_empty());
        assert!(hush().is_none() && !silent_now());
    }

    #[test]
    fn do_not_disturb_coming_on_mid_scream_cuts_it_off() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 0.5); // the first "AAAA" is playing
        assert_eq!(host.played.len(), 1);
        let hushes = host.hushes;
        host.probe.hush = Some(Hush::PhoneVibrate);
        // Noticed at the next probe, within a second.
        let scenes = run(&mut b, &mut host, 0.5, 1.2);
        assert!(host.hushes > hushes, "the scream should have been cut off");
        assert!(scenes.last().unwrap().bubble.is_some(), "and the rest of it is a bubble");
    }

    #[test]
    fn scream_now_is_heard_in_do_not_disturb_but_not_on_a_call() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), hushed(Hush::PhoneSilent));
        run(&mut b, &mut host, 0.0, 0.5);
        assert!(host.played.is_empty());
        SCREAM_NOW.store(true, Ordering::Relaxed);
        let scenes = run(&mut b, &mut host, 0.5, 1.0);
        assert_eq!(host.played.len(), 1, "Scream Now is heard");
        assert!(scenes.iter().all(|s| s.bubble.is_none()), "and needs no bubble");
        // The screams after it are silent again.
        run(&mut b, &mut host, 1.0, 12.0);
        assert_eq!(host.played.len(), 1);

        let (mut b, mut host) = (brain(), hushed(Hush::Call));
        run(&mut b, &mut host, 0.0, 0.5);
        SCREAM_NOW.store(true, Ordering::Relaxed);
        let scenes = run(&mut b, &mut host, 0.5, 1.0);
        assert!(host.played.is_empty(), "not during a call");
        assert!(scenes.iter().any(|s| s.bubble.is_some()));
    }

    #[test]
    fn picking_scream_silently_makes_scream_now_a_bubble_too() {
        let _g = reset_controls();
        SILENT.store(true, Ordering::Relaxed);
        let (mut b, mut host) = (brain(), hushed(Hush::DoNotDisturb));
        run(&mut b, &mut host, 0.0, 0.5);
        SCREAM_NOW.store(true, Ordering::Relaxed);
        run(&mut b, &mut host, 0.5, 1.0);
        assert!(host.played.is_empty(), "picked Scream silently: Scream Now is a bubble");
        SILENT.store(false, Ordering::Relaxed);
    }

    #[test]
    fn a_new_screen_keeps_the_possum_on_the_same_edge() {
        let _g = reset_controls();
        let settings = Settings { scale: 3.0, speed: 260.0, start: 0.4, verbose: false };
        let mut b = Brain::new(&settings, 1512.0, 982.0, [vec![1.2; 3], vec![1.0; 3]], Rng::new(1));
        let mut host =
            FakeHost { probe: Probe { screen_locked: Some(true), ..Probe::default() }, ..Default::default() };
        let before = b.tick(0.0, &mut host);
        b.set_screen(3440.0, 1440.0, 3.0, 260.0);
        let after = b.tick(0.01, &mut host);
        assert!((before.heading - after.heading).abs() < 1e-9, "{before:?} -> {after:?}");
        assert_eq!(b.screen_size(), (3440.0, 1440.0));
    }

    #[test]
    fn a_new_screen_can_make_it_bigger_and_faster() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        b.hold_screams(true); // no panic speed-ups
        let small = run(&mut b, &mut host, 0.0, 0.5);
        b.set_screen(1920.0, 1080.0, 4.0, 4.0 / 3.0 * 260.0);
        let big = run(&mut b, &mut host, 0.5, 1.0);
        assert_eq!(b.scale(), 4.0);
        // Its legs keep the same rhythm: the run cycle grows with it.
        let frames = |s: &[Scene]| s.windows(2).filter(|w| w[0].frame != w[1].frame).count();
        assert!(frames(&small).abs_diff(frames(&big)) <= 1, "{} vs {}", frames(&small), frames(&big));
    }

    /// The scenes once it's standing: from the first standing one on.
    fn standing(scenes: &[Scene]) -> &[Scene] {
        let first = scenes.iter().position(|s| s.frame == sprite::STAND).expect("it never stood still");
        &scenes[first..]
    }

    #[test]
    fn stop_shuts_its_mouth_and_it_stands_still_within_a_stride() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        let running = run(&mut b, &mut host, 0.0, 0.5); // the first "AAAA" is playing
        assert!(running.last().unwrap().screaming);
        let hushes = host.hushes;
        STOPPED.store(true, Ordering::Relaxed);
        let scenes = run(&mut b, &mut host, 0.5, 20.0);
        assert!(host.hushes > hushes, "the scream should have been cut off");
        assert!(scenes.iter().all(|s| !s.screaming && s.bubble.is_none()), "its mouth stays shut");
        assert_eq!(host.played.len(), 1, "no more screams while it stands");
        // It slows down to a stand within a stride and well within a second...
        let at = scenes.iter().position(|s| s.frame == sprite::STAND).unwrap();
        assert!(at < 60, "took {at} ticks to stop");
        let still = standing(&scenes);
        let (from, to) = (running.last().unwrap(), &still[0]);
        let travelled = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt();
        assert!(travelled <= 1.5 * 16.0 * 3.0 + 6.0, "ran {travelled} units after Stop");
        // ...at the end of a half stride, where its paws are all down...
        assert!(matches!(scenes[at - 1].frame, 3 | 7), "stood after frame {}", scenes[at - 1].frame);
        // ...and stays put.
        assert!(still.iter().all(|s| (s.x, s.y, s.frame) == (to.x, to.y, sprite::STAND)));
    }

    #[test]
    fn standing_it_screams_only_when_asked_and_runs_on_when_told() {
        let _g = reset_controls();
        let (mut b, mut host) = (brain(), FakeHost::default());
        run(&mut b, &mut host, 0.0, 0.5);
        STOPPED.store(true, Ordering::Relaxed);
        let still = run(&mut b, &mut host, 0.5, 5.0);
        let spot = (standing(&still)[0].x, standing(&still)[0].y);
        let played = host.played.len();
        SCREAM_NOW.store(true, Ordering::Relaxed);
        let scream = run(&mut b, &mut host, 5.0, 6.0);
        assert_eq!(host.played.len(), played + 1, "Scream Now screams");
        for s in &scream {
            assert!(s.screaming && s.frame == sprite::STAND, "{s:?}");
            // Trembling, but in place.
            assert!((s.x - spot.0).abs() <= 2.0 && (s.y - spot.1).abs() <= 2.0, "{s:?} moved from {spot:?}");
        }
        assert!(!run(&mut b, &mut host, 6.0, 7.0).last().unwrap().screaming, "and shuts its mouth again");

        STOPPED.store(false, Ordering::Relaxed);
        let off = run(&mut b, &mut host, 7.0, 12.0);
        assert!(off.iter().all(|s| s.frame != sprite::STAND), "Run sets it off at once");
        assert!(off.last().unwrap().x > spot.0 + 100.0, "{:?}", off.last());
        // Picking up speed, not at full speed from the first tick.
        let first_step = off[1].x - off[0].x;
        assert!(first_step < 260.0 / 60.0, "first step {first_step}");
        assert!(host.played.len() > played + 1, "and screams again");
    }
}

//! Possum for Android, including Samsung DeX and Android's desktop mode on a
//! connected monitor.
//!
//! The app is in two halves. The Java half (`java/`) does what
//! `src/platform/` does on the desktop: the transparent overlay window, the
//! sound output, the menu (a notification), and noticing locks and displays.
//! This library is the other half: the desktop app's shared core, unchanged
//! (the modules below are its very files), which the Java side drives
//! through JNI (`jni_api.rs`):
//!
//! * `Pictures`: every sprite, numbered, for the Java side to turn into
//!   bitmaps once;
//! * `Core`: the brain, ticked every frame. Each tick says what to draw (by
//!   picture number) and which sounds to play or stop.

// The desktop app's shared modules, straight from its source tree. Parts of
// them only some platforms use.
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/brain.rs"]
mod brain;
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/quiet.rs"]
mod quiet;
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/render.rs"]
mod render;
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/scream.rs"]
mod scream;
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/sprite.rs"]
mod sprite;
#[allow(dead_code, reason = "parts of the shared modules are for other platforms")]
#[path = "../../src/track.rs"]
mod track;

#[cfg(target_os = "android")]
mod jni_api;
#[cfg(target_os = "android")]
mod logcat;

use std::sync::atomic::Ordering;

use brain::{Brain, Host, Scene, Settings};
use render::Art;
use scream::{Kind, Rng};
use sprite::Canvas;

// ---------------------------------------------------------------------------
// Pictures
// ---------------------------------------------------------------------------

/// Every picture the app draws, unscaled, in one numbered list: the run
/// frames, the scream frames (each with its two yell-line frames), the
/// speech bubbles (per kind of scream, per shout, per pointer corner), and
/// last the small head for the notification.
pub struct Pictures {
    art: Art,
    status_icon: Canvas,
}

impl Pictures {
    pub fn new() -> Self {
        Self { art: Art::new(), status_icon: sprite::status_icon() }
    }

    fn all(&self) -> impl Iterator<Item = &Canvas> {
        let a = &self.art;
        a.run
            .iter()
            .chain(a.scream.iter().flatten())
            .chain(a.bubbles.iter().flatten().flatten())
            .chain(std::iter::once(&self.status_icon))
    }

    pub fn count(&self) -> usize {
        self.all().count()
    }

    pub fn get(&self, index: usize) -> Option<&Canvas> {
        self.all().nth(index)
    }

    /// The number of the picture `Art::possum` would draw for this scene.
    pub fn possum(&self, scene: &Scene) -> usize {
        match scene.yell {
            Some(yell) if scene.screaming => self.art.run.len() + scene.frame * 2 + yell,
            _ => scene.frame,
        }
    }

    /// The number of the picture `Art::bubble` would draw.
    pub fn bubble(&self, bubble: &brain::Bubble) -> usize {
        let a = &self.art;
        let kind = brain::kind_index(bubble.kind);
        let before: usize = a.bubbles[..kind].iter().map(|shouts| shouts.len() * 4).sum();
        a.run.len() + a.scream.len() * 2 + before + bubble.shout * 4 + bubble.quadrant
    }

    pub fn status_icon(&self) -> usize {
        self.count() - 1
    }

    /// The tallest and widest possum picture, unscaled: the size of the view
    /// the possum is drawn in.
    pub fn possum_size(&self) -> (i32, i32) {
        let a = &self.art;
        let frames = a.run.iter().chain(a.scream.iter().flatten());
        frames.fold((0, 0), |(w, h), c| (w.max(c.w), h.max(c.h)))
    }

    /// The same for the speech bubbles.
    pub fn bubble_size(&self) -> (i32, i32) {
        let bubbles = self.art.bubbles.iter().flatten().flatten();
        bubbles.fold((0, 0), |(w, h), c| (w.max(c.w), h.max(c.h)))
    }
}

impl Default for Pictures {
    fn default() -> Self {
        Self::new()
    }
}

/// A picture as Android colour ints (0xAARRGGBB), row by row from the top.
pub fn argb(canvas: &Canvas) -> Vec<i32> {
    let mut out = Vec::with_capacity((canvas.w * canvas.h) as usize);
    for y in 0..canvas.h {
        for x in 0..canvas.w {
            let [r, g, b, a] = canvas.rgba_at(x, y);
            out.push(i32::from_be_bytes([a, r, g, b]));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------

/// The screams, synthesised once: per kind (in `brain::kind_index` order), a
/// few variations, as 16-bit mono samples.
pub struct Voices {
    pub sample_rate: u32,
    pub samples: [Vec<Vec<i16>>; 2],
    pub seconds: [Vec<f64>; 2],
}

impl Voices {
    pub fn new(seed: u32) -> Self {
        let voices = brain::voices(seed, &mut Rng::new(seed));
        // The sounds come as WAV files: a 44-byte header, then the samples.
        let wav = &voices[0][0].wav;
        let sample_rate = u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]);
        let samples = voices.each_ref().map(|kind| {
            kind.iter().map(|s| s.wav[44..].as_chunks().0.iter().map(|&b| i16::from_le_bytes(b)).collect()).collect()
        });
        let seconds = voices.each_ref().map(|kind| kind.iter().map(|s| s.seconds).collect());
        Self { sample_rate, samples, seconds }
    }
}

// ---------------------------------------------------------------------------
// The brain, ticked from Java
// ---------------------------------------------------------------------------

/// Sound requests in the list a tick returns: stop every sound, or play
/// `PLAY | kind << 8 | variant`.
pub const HUSH: i32 = 1 << 16;
pub const PLAY: i32 = 2 << 16;

/// Bits of the `quiet` argument to `Core::tick`: the screen is locked, the
/// display the possum is on is off.
pub const LOCKED: i32 = 1;
pub const DISPLAY_OFF: i32 = 2;

/// What a tick says to draw, as numbers for Java: the paws (y-up, from the
/// bottom-left of the track) and heading; the possum picture; the bubble
/// picture (-1 for none) and its place; whether to look at the displays
/// again; whether it's screaming.
pub const SCENE_LEN: usize = 11;

pub struct Core {
    brain: Brain,
    size: (f64, f64),
}

/// The brain's `Host`: it can't play anything itself, so it notes what the
/// brain asked for and the Java side does it after the tick.
struct Requests {
    probe: quiet::Probe,
    ops: Vec<i32>,
}

impl Host for Requests {
    fn play(&mut self, kind: Kind, variant: usize) -> bool {
        self.ops.push(PLAY | ((brain::kind_index(kind) as i32) << 8) | variant as i32);
        true
    }

    fn hush(&mut self) {
        self.ops.push(HUSH);
    }

    fn probe(&mut self) -> quiet::Probe {
        self.probe
    }
}

impl Core {
    /// A possum running around a `width` by `height` track (in pixels),
    /// `scale` pixels per sprite pixel, `speed` pixels per second.
    pub fn new(voices: &Voices, scale: f64, speed: f64, width: f64, height: f64, seed: u32) -> Self {
        let settings = Settings { scale, speed, start: 0.0, verbose: true };
        let brain = Brain::new(&settings, width, height, voices.seconds.clone(), Rng::new(seed));
        Self { brain, size: (width, height) }
    }

    /// The track changed size (rotation, resolution, a taskbar): keep
    /// running from the same spot.
    pub fn resize(&mut self, width: f64, height: f64) {
        if (width, height) != self.size {
            self.brain.set_screen(width, height);
            self.size = (width, height);
        }
    }

    /// One frame: what to draw (see `SCENE_LEN`) and the sound requests, in
    /// order. `silent` is "Scream silently" (picked in the menu, or forced
    /// by the phone's own silent mode); `quiet` has the `LOCKED` and
    /// `DISPLAY_OFF` bits.
    pub fn tick(&mut self, pictures: &Pictures, now: f64, silent: bool, quiet: i32) -> ([f32; SCENE_LEN], Vec<i32>) {
        brain::SILENT.store(silent, Ordering::Relaxed);
        let probe = quiet::Probe {
            screen_locked: Some(quiet & LOCKED != 0),
            display_asleep: Some(quiet & DISPLAY_OFF != 0),
            screensaver: None,
            on_console: None,
        };
        let mut requests = Requests { probe, ops: Vec::new() };
        let scene = self.brain.tick(now, &mut requests);
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let (bubble, left, bottom, w, h) = match &scene.bubble {
            Some(b) => (pictures.bubble(b) as f32, b.left as f32, b.bottom as f32, b.w as f32, b.h as f32),
            None => (-1.0, 0.0, 0.0, 0.0, 0.0),
        };
        let numbers = [
            scene.x as f32,
            scene.y as f32,
            scene.heading as f32,
            pictures.possum(&scene) as f32,
            bubble,
            left,
            bottom,
            w,
            h,
            flag(scene.check_displays),
            flag(scene.screaming),
        ];
        (numbers, requests.ops)
    }

    pub fn log(&self, message: std::fmt::Arguments) {
        self.brain.log(message);
    }
}

/// A different seed every run, so the screams vary.
pub fn seed() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u32).unwrap_or(0xC0FFEE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_numbers_match_what_the_desktop_draws() {
        let p = Pictures::new();
        let art = Art::new();
        for frame in 0..sprite::RUN_FRAMES {
            for (screaming, yell) in [(false, None), (true, Some(0)), (true, Some(1))] {
                let scene =
                    Scene { x: 0.0, y: 0.0, heading: 0.0, screaming, frame, yell, bubble: None, check_displays: false };
                let picture = p.get(p.possum(&scene)).unwrap();
                assert_eq!(picture.rgba_scaled(1), art.possum(&scene).rgba_scaled(1));
            }
        }
        for kind in [Kind::Aaaa, Kind::Hiss] {
            for shout in 0..brain::shouts()[brain::kind_index(kind)].len() {
                for quadrant in 0..4 {
                    let b = brain::Bubble { kind, shout, quadrant, left: 0.0, bottom: 0.0, w: 0.0, h: 0.0 };
                    assert_eq!(p.get(p.bubble(&b)).unwrap().rgba_scaled(1), art.bubble(&b).rgba_scaled(1));
                }
            }
        }
        let icon = p.get(p.status_icon()).unwrap();
        assert_eq!(icon.rgba_scaled(1), sprite::status_icon().rgba_scaled(1));
        assert!(p.get(p.count()).is_none());
    }

    #[test]
    fn colours_come_out_as_android_colour_ints() {
        let icon = sprite::status_icon();
        let pixels = argb(&icon);
        assert_eq!(pixels.len(), (icon.w * icon.h) as usize);
        let opaque = pixels.iter().filter(|&&c| c.cast_unsigned() >> 24 == 0xFF).count();
        let clear = pixels.iter().filter(|&&c| c == 0).count();
        assert!(opaque > 0 && opaque + clear == pixels.len());
    }

    #[test]
    fn voices_are_the_desktop_screams() {
        let v = Voices::new(7);
        assert_eq!(v.sample_rate, 44_100);
        for kind in 0..2 {
            assert!(!v.samples[kind].is_empty());
            for (samples, seconds) in v.samples[kind].iter().zip(&v.seconds[kind]) {
                let expected = seconds * v.sample_rate as f64;
                assert!((samples.len() as f64 - expected).abs() <= 1.0, "{} vs {expected}", samples.len());
            }
        }
    }

    #[test]
    fn a_tick_asks_for_sounds_and_says_what_to_draw() {
        let (pictures, voices) = (Pictures::new(), Voices::new(7));
        let mut core = Core::new(&voices, 3.0, 260.0, 1920.0, 1080.0, 7);
        let mut played = Vec::new();
        let mut now = 0.0;
        while now < 1.0 {
            let (scene, ops) = core.tick(&pictures, now, false, 0);
            played.extend(ops);
            assert!(scene[3] >= 0.0 && (scene[3] as usize) < pictures.count());
            assert_eq!(scene[4], -1.0, "no bubble when screaming out loud");
            now += 1.0 / 60.0;
        }
        // The first scream, an "AAAA", comes after a quarter of a second.
        let play = played.iter().find(|&&op| op & PLAY != 0).expect("it screamed");
        assert_eq!((play >> 8) & 0xFF, 0);

        // Silently, there's a bubble and no sound; locked, nothing at all.
        let mut core = Core::new(&voices, 3.0, 260.0, 1920.0, 1080.0, 7);
        let (mut bubble, mut sounds) = (false, 0);
        let mut now = 0.0;
        while now < 1.0 {
            let (scene, ops) = core.tick(&pictures, now, true, 0);
            bubble |= scene[4] >= 0.0;
            sounds += ops.iter().filter(|&&op| op & PLAY != 0).count();
            now += 1.0 / 60.0;
        }
        assert!(bubble && sounds == 0);
        let mut core = Core::new(&voices, 3.0, 260.0, 1920.0, 1080.0, 7);
        let mut now = 0.0;
        while now < 1.0 {
            let (scene, ops) = core.tick(&pictures, now, false, LOCKED);
            assert!(scene[4] < 0.0 && ops.iter().all(|&op| op & PLAY == 0));
            now += 1.0 / 60.0;
        }
    }
}

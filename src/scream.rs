//! Procedurally synthesised possum screams.
//!
//! One scream is four parts, played in order:
//!
//! 1. **Explosive hiss.** Modelled on Virginia opossum hisses from field
//!    recordings: a broadband burst that jumps up ~16 dB within 50 ms, bright
//!    at first (energy centred near 4.5 kHz), then darker as it fades
//!    (centred near 2 kHz, like the opossum's long open-mouthed hisses).
//! 2. **Screech.** A Virginia opossum screech is a shrill, almost pure tone:
//!    it starts around 1.6 kHz, jumps to ~2.3 kHz within ~50 ms, then slides
//!    down to ~1.75 kHz. Here it is slightly longer and higher than life.
//! 3. **"AAAA".** The cartoon scream from the first version of this app,
//!    unchanged: a jittery sawtooth voice through "AAAA" vowel formants.
//! 4. **Breathing hisses.** Threatened opossums keep hissing in short pulses
//!    in time with their breathing; two or three of them trail the scream.
//!
//! The result is wrapped in a 16-bit PCM WAV so AppKit's `NSSound` can play
//! it straight from memory.

use std::f32::consts::{PI, TAU};

const SAMPLE_RATE: u32 = 44_100;

/// Small deterministic PRNG (xorshift32), good enough for audio noise.
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// RBJ cookbook band-pass biquad (constant 0 dB peak gain).
struct BandPass {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BandPass {
    fn new(freq: f32, q: f32) -> Self {
        let w0 = TAU * freq / SAMPLE_RATE as f32;
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * w0.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// RBJ cookbook low-pass biquad.
struct LowPass {
    b0: f32,
    b1: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl LowPass {
    fn new(freq: f32, q: f32) -> Self {
        let w0 = TAU * freq / SAMPLE_RATE as f32;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        // b2 == b0 for a low-pass.
        let y = self.b0 * (x + self.x2) + self.b1 * self.x1 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Colours white noise into a hiss: broad band-passes (low Q, so no
/// whistling resonances) summed, then a low-pass to set how bright it is.
struct NoiseColour {
    bands: Vec<(BandPass, f32)>,
    top: LowPass,
}

impl NoiseColour {
    /// `bands` are `(centre Hz, Q, gain)`; `cutoff` is the low-pass in Hz.
    fn new(bands: &[(f32, f32, f32)], cutoff: f32) -> Self {
        Self {
            bands: bands.iter().map(|&(f, q, g)| (BandPass::new(f, q), g)).collect(),
            top: LowPass::new(cutoff, 0.7),
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let sum: f32 = self.bands.iter_mut().map(|(f, g)| f.process(x) * *g).sum();
        self.top.process(sum)
    }

    /// A fresh, explosive hiss: energy centred near 4.5 kHz.
    fn bright() -> Self {
        Self::new(&[(1_800.0, 0.9, 0.3), (4_300.0, 1.1, 1.0)], 8_000.0)
    }

    /// A long, open-mouthed hiss: energy centred near 2 kHz.
    fn dark() -> Self {
        Self::new(&[(700.0, 0.8, 1.0), (1_800.0, 1.0, 0.8), (3_800.0, 1.3, 0.25)], 4_500.0)
    }
}

/// PolyBLEP correction to remove aliasing from the naive sawtooth step.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Fast attack, exponential decay: the shape of a hiss.
fn burst(t: f32, attack: f32, decay_rate: f32) -> f32 {
    if t < 0.0 {
        0.0
    } else if t < attack {
        t / attack
    } else {
        (-(t - attack) * decay_rate).exp()
    }
}

fn normalize(x: &mut [f32], peak: f32) {
    let max = x.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in x {
        *s *= peak / max;
    }
}

/// Where the parts of a scream sit in time, in seconds from the start.
pub struct Timeline {
    pub screech_start: f32,
    pub screech_len: f32,
    pub aaaa_start: f32,
    pub breaths_start: f32,
    pub breath_gap: f32,
    pub breaths: usize,
    pub total: f32,
}

impl Timeline {
    fn new(rng: &mut Rng, aaaa_len: f32) -> Self {
        // The screech erupts out of the opening hiss...
        let screech_start = 0.12;
        let screech_len = rng.range(0.36, 0.46);
        // ...and slides straight into the "AAAA".
        let aaaa_start = screech_start + screech_len - 0.06;
        let breaths_start = aaaa_start + aaaa_len + 0.05;
        let breath_gap = rng.range(0.22, 0.3);
        let breaths = 2 + (rng.next_u32() % 2) as usize;
        let total = breaths_start + breaths as f32 * breath_gap + 0.2;
        Self { screech_start, screech_len, aaaa_start, breaths_start, breath_gap, breaths, total }
    }
}

/// Each part of a scream on its own, already at its mix level and placed on
/// the shared timeline (all buffers have the same length).
pub struct Parts {
    pub hiss: Vec<f32>,
    pub screech: Vec<f32>,
    pub aaaa: Vec<f32>,
    pub breaths: Vec<f32>,
    pub timeline: Timeline,
}

pub struct Scream {
    pub wav: Vec<u8>,
    pub seconds: f64,
}

/// Synthesise one scream. `pitch` and `seconds` are those of the "AAAA" part
/// (as in the first version); hisses and the screech add about a second.
pub fn synthesize(seed: u32, pitch: f32, seconds: f32) -> Scream {
    let (samples, timeline) = render(seed, pitch, seconds);
    Scream { wav: wav_pcm16(samples.into_iter()), seconds: timeline.total as f64 }
}

/// The finished, mixed scream, normalised to a 0.9 peak.
pub fn render(seed: u32, pitch: f32, seconds: f32) -> (Vec<f32>, Timeline) {
    let p = render_parts(seed, pitch, seconds);
    let mut mix: Vec<f32> = (0..p.aaaa.len())
        .map(|i| p.hiss[i] + p.screech[i] + p.aaaa[i] + p.breaths[i])
        .collect();
    normalize(&mut mix, 0.9);
    (mix, p.timeline)
}

pub fn render_parts(seed: u32, pitch: f32, seconds: f32) -> Parts {
    // A separate random stream, so the "AAAA" stays exactly as it was.
    let mut rng = Rng::new(seed ^ 0x9E37_79B9);
    let sr = SAMPLE_RATE as f32;
    let tl = Timeline::new(&mut rng, seconds);
    let n = (tl.total * sr) as usize;

    // --- 1. explosive hiss, brightening then darkening ----------------------
    let mut bright = NoiseColour::bright();
    let mut dark = NoiseColour::dark();
    let mut hiss: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let white = rng.range(-1.0, 1.0);
            let darken = smoothstep(0.04, 0.3, t);
            let colour = bright.process(white) * (1.0 - darken) + dark.process(white) * darken;
            colour * burst(t, 0.012, 7.0)
        })
        .collect();
    normalize(&mut hiss, 0.85);

    // --- 2. screech: a shrill, sliding near-pure tone ------------------------
    let len = tl.screech_len;
    let start_hz = rng.range(1_550.0, 1_700.0);
    let peak_hz = rng.range(2_300.0, 2_450.0);
    let end_hz = rng.range(1_700.0, 1_800.0);
    let wobble_rate = rng.range(8.0, 11.0);
    let mut phase = 0.0f32;
    let mut screech: Vec<f32> = (0..n)
        .map(|i| {
            let st = i as f32 / sr - tl.screech_start;
            if !(0.0..len + 0.04).contains(&st) {
                return 0.0;
            }
            let u = st / len;
            let hz = start_hz + (peak_hz - start_hz) * smoothstep(0.0, 0.045, st)
                - (peak_hz - end_hz) * smoothstep(0.12, 1.0, u);
            let hz = hz * (1.0 + 0.02 * (TAU * wobble_rate * st).sin());
            phase = (phase + hz / sr).fract();
            let tone = (TAU * phase).sin() + 0.18 * (2.0 * TAU * phase).sin() + 0.06 * (3.0 * TAU * phase).sin();
            let env = smoothstep(0.0, 0.015, st) * (1.0 - smoothstep(len - 0.05, len + 0.03, st));
            tone * env
        })
        .collect();
    normalize(&mut screech, 0.75);

    // --- 3. the original "AAAA", placed after the screech --------------------
    let voice = aaaa_voice(seed, pitch, seconds);
    let offset = (tl.aaaa_start * sr) as usize;
    let mut aaaa = vec![0.0; n];
    for (i, s) in voice.iter().enumerate() {
        if let Some(slot) = aaaa.get_mut(offset + i) {
            *slot = *s;
        }
    }

    // --- 4. breathing hisses ------------------------------------------------
    let mut bright = NoiseColour::bright();
    let mut dark = NoiseColour::dark();
    let mut breaths: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let white = rng.range(-1.0, 1.0);
            let env: f32 = (0..tl.breaths)
                .map(|k| burst(t - tl.breaths_start - k as f32 * tl.breath_gap, 0.012, 14.0))
                .sum();
            (bright.process(white) * 0.5 + dark.process(white) * 0.5) * env
        })
        .collect();
    normalize(&mut breaths, 0.5);

    Parts { hiss, screech, aaaa, breaths, timeline: tl }
}

/// The "AAAA" cartoon scream from the first version of the app, unchanged:
/// a source-filter voice. The source is a band-limited sawtooth whose pitch
/// shoots up, wobbles with vibrato and jitter, then cracks downward; a
/// sub-harmonic amplitude modulation makes it rough. The filter is a set of
/// band-passes at the formants of an open "AAAA" vowel. Peak is 0.9.
pub fn aaaa_voice(seed: u32, pitch: f32, seconds: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let sr = SAMPLE_RATE as f32;
    let n = (sr * seconds) as usize;

    // Formants of an open "AAAA", nudged up a bit for a small animal.
    let mut formants = [
        (BandPass::new(1_000.0, 5.0), 1.0),
        (BandPass::new(1_550.0, 6.0), 0.75),
        (BandPass::new(3_000.0, 7.0), 0.45),
        (BandPass::new(4_300.0, 8.0), 0.2),
    ];

    // Slowly-varying random jitter for an unsteady, panicky voice.
    let mut jitter = 0.0f32;
    let crack_at = rng.range(0.55, 0.8);
    let vib_rate = rng.range(6.5, 9.0);

    let mut phase = 0.0f32;
    let mut sub_phase = 0.0f32;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / sr;
        let u = t / seconds;

        jitter += (rng.range(-1.0, 1.0) - jitter) * 0.002;
        let contour = 1.0 + 0.45 * (1.0 - (-t * 12.0).exp()) // shoot up
            + 0.08 * (PI * u).sin()                          // swell
            - 0.3 * smoothstep(crack_at, 1.0, u); // voice cracks down
        let vibrato = 1.0 + 0.05 * (TAU * vib_rate * t).sin() + 0.02 * (TAU * 13.0 * t).sin();
        let f0 = pitch * contour * vibrato * (1.0 + 0.06 * jitter);

        let dt = f0 / sr;
        phase += dt;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        sub_phase += dt * 0.5;
        if sub_phase >= 1.0 {
            sub_phase -= 1.0;
        }

        let saw = 2.0 * phase - 1.0 - poly_blep(phase, dt);
        let rough = 1.0 - 0.4 * (0.5 + 0.5 * (TAU * sub_phase).sin());
        let breath = rng.range(-1.0, 1.0) * 0.35;
        let source = saw * rough + breath;

        let voiced: f32 = formants.iter_mut().map(|(f, g)| f.process(source) * *g).sum();
        let envelope = smoothstep(0.0, 0.035, t) * (1.0 - smoothstep(seconds - 0.28, seconds, t));
        // Gentle saturation for grit.
        samples.push((voiced * envelope * 3.0).tanh());
    }

    normalize(&mut samples, 0.9);
    samples
}

/// Wraps mono float samples in a 16-bit PCM WAV container.
fn wav_pcm16(samples: impl ExactSizeIterator<Item = f32>) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = SAMPLE_RATE as f32;

    fn window(x: &[f32], from: f32, to: f32) -> &[f32] {
        &x[(from * SR) as usize..((to * SR) as usize).min(x.len())]
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Zero crossings per second: a cheap stand-in for "how bright/hissy",
    /// and for a pure tone, twice its frequency.
    fn zcr(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        crossings as f32 / (x.len() as f32 / SR)
    }

    #[test]
    fn wav_header_is_consistent() {
        let s = synthesize(7, 650.0, 1.2);
        assert_eq!(&s.wav[0..4], b"RIFF");
        assert_eq!(&s.wav[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(s.wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(s.wav.len(), 44 + data_len);
        assert_eq!(data_len, (SR * s.seconds as f32) as usize * 2);
    }

    #[test]
    fn scream_is_loud() {
        let (x, _) = render(42, 650.0, 1.3);
        let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.9).abs() < 1e-3, "peak {peak}");
        assert!(rms(&x) > 0.1, "too quiet, rms {}", rms(&x));
    }

    #[test]
    fn aaaa_voice_is_unchanged_from_the_first_version() {
        // Golden values recorded from the v0.1 implementation (commit
        // 91a751c): sum and checksum of its 16-bit samples.
        let pcm: Vec<i16> = aaaa_voice(7, 600.0, 0.5)
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        assert_eq!(pcm.len(), 22_050);
        let checksum = pcm.iter().fold(0u64, |h, &v| h.wrapping_mul(31).wrapping_add(v as u16 as u64));
        assert_eq!(checksum, GOLDEN_AAAA_CHECKSUM);
    }

    /// See `aaaa_voice_is_unchanged_from_the_first_version`.
    const GOLDEN_AAAA_CHECKSUM: u64 = 15_922_795_222_752_550_495;

    #[test]
    fn the_aaaa_part_of_the_mix_is_the_original_voice() {
        let (mix, tl) = render(3, 650.0, 1.4);
        let voice = aaaa_voice(3, 650.0, 1.4);
        // Mid-"AAAA" nothing else is playing: the mix is the voice, scaled.
        let a = window(&mix, tl.aaaa_start + 0.4, tl.aaaa_start + 0.9);
        let b = window(&voice, 0.4, 0.9);
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let corr = dot / (rms(a) * rms(b) * a.len() as f32);
        assert!(corr > 0.999, "correlation {corr}");
    }

    #[test]
    fn opens_with_a_hiss_that_darkens() {
        for seed in [1, 2, 3, 99] {
            let p = render_parts(seed, 650.0, 1.2);
            let early = zcr(window(&p.hiss, 0.0, 0.04));
            let late = zcr(window(&p.hiss, 0.3, 0.45));
            assert!(early > 5_000.0, "fresh hiss too dull: {early}");
            assert!(early > 1.4 * late, "should darken: {early} -> {late}");
            // Explosive: loud within the first 50 ms.
            assert!(rms(window(&p.hiss, 0.0, 0.05)) > 0.15);
        }
    }

    #[test]
    fn screech_is_a_shrill_sliding_tone() {
        for seed in [4, 5, 6] {
            let p = render_parts(seed, 650.0, 1.2);
            let tl = &p.timeline;
            let hz_at = |from: f32, to: f32| zcr(window(&p.screech, tl.screech_start + from, tl.screech_start + to)) / 2.0;
            let peak = hz_at(0.05, 0.1);
            let end = hz_at(tl.screech_len - 0.1, tl.screech_len - 0.05);
            assert!((2_200.0..2_550.0).contains(&peak), "peak {peak} Hz");
            assert!((1_650.0..1_950.0).contains(&end), "end {end} Hz");
        }
    }

    #[test]
    fn ends_with_breathing_hisses_separated_by_gaps() {
        let (x, tl) = render(5, 650.0, 1.2);
        for k in 0..tl.breaths {
            let start = tl.breaths_start + k as f32 * tl.breath_gap;
            let on = rms(window(&x, start, start + 0.05));
            let gap = rms(window(&x, start + tl.breath_gap - 0.035, start + tl.breath_gap - 0.005));
            assert!(on > 3.0 * gap, "breath {k}: on {on} vs gap {gap}");
        }
    }
}

//! Procedurally synthesised possum screams.
//!
//! There are two kinds, and the possum alternates between them:
//!
//! * **"AAAA"**, the cartoon scream from the first version of this app: a
//!   jittery sawtooth voice through "AAAA" vowel formants.
//! * **Long hiss**, modelled on the long, open-mouthed hisses of Virginia
//!   opossums in field recordings: broadband breath noise centred near
//!   2 kHz, with most of its energy at 1-3 kHz and little above 6 kHz. It
//!   rises in about 60 ms, holds, and fades. Real hisses last ~0.6 s; this one
//!   lasts a bit longer, for the drama.
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

    /// The first push of air in a hiss: energy centred near 4.5 kHz.
    fn bright() -> Self {
        Self::new(&[(1_800.0, 0.9, 0.3), (4_300.0, 1.1, 1.0)], 8_000.0)
    }

    /// The body of a long, open-mouthed hiss: energy centred near 2 kHz.
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

fn normalize(x: &mut [f32], peak: f32) {
    let max = x.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    for s in x {
        *s *= peak / max;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Aaaa,
    Hiss,
}

pub struct Scream {
    pub wav: Vec<u8>,
    pub seconds: f64,
}

impl Scream {
    /// The WAV with its samples scaled to `volume` (0..1), for players that
    /// can't set a volume themselves.
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn wav_at_volume(&self, volume: f32) -> Vec<u8> {
        let mut wav = self.wav.clone();
        for sample in wav[44..].chunks_exact_mut(2) {
            let v = i16::from_le_bytes([sample[0], sample[1]]) as f32 * volume.clamp(0.0, 1.0);
            sample.copy_from_slice(&(v.round() as i16).to_le_bytes());
        }
        wav
    }
}

/// The "AAAA" scream, `pitch` in Hz, `seconds` long.
pub fn aaaa(seed: u32, pitch: f32, seconds: f32) -> Scream {
    let samples = aaaa_voice(seed, pitch, seconds);
    Scream { wav: wav_pcm16(samples.into_iter()), seconds: seconds as f64 }
}

/// A long, open-mouthed hiss, `seconds` long.
pub fn hiss(seed: u32, seconds: f32) -> Scream {
    let samples = long_hiss(seed, seconds);
    Scream { wav: wav_pcm16(samples.into_iter()), seconds: seconds as f64 }
}

/// The long hiss as samples, peak 0.9.
pub fn long_hiss(seed: u32, seconds: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed ^ 0x9E37_79B9);
    let sr = SAMPLE_RATE as f32;
    let n = (sr * seconds) as usize;

    let mut dark = NoiseColour::dark();
    let mut bright = NoiseColour::bright();
    // A breath isn't perfectly steady: its force drifts slowly and flutters.
    let mut drift = 0.0f32;
    let flutter_rate = rng.range(7.0, 10.0);
    let mut samples: Vec<f32> = (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let white = rng.range(-1.0, 1.0);
            drift += (rng.range(-1.0, 1.0) - drift) * 0.0004;
            // The first push of air is a little brighter, then it settles.
            let settle = smoothstep(0.0, 0.35, t);
            let colour = dark.process(white) + bright.process(white) * 0.3 * (1.0 - settle);
            let envelope = smoothstep(0.0, 0.06, t)
                * (1.0 - smoothstep(seconds - 0.3, seconds, t))
                * (1.0 + 0.25 * drift)
                * (1.0 + 0.06 * (TAU * flutter_rate * t).sin());
            colour * envelope
        })
        .collect();
    normalize(&mut samples, 1.0);
    // Gentle saturation: a fuller, more forceful hiss at the same peak.
    for s in &mut samples {
        *s = (*s * 1.6).tanh();
    }
    normalize(&mut samples, 0.9);
    samples
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

    /// Zero crossings per second: a cheap stand-in for "how bright/hissy".
    fn zcr(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| (w[0] >= 0.0) != (w[1] >= 0.0)).count();
        crossings as f32 / (x.len() as f32 / SR)
    }

    #[test]
    fn wav_header_is_consistent() {
        for s in [aaaa(7, 650.0, 1.2), hiss(7, 1.3)] {
            assert_eq!(&s.wav[0..4], b"RIFF");
            assert_eq!(&s.wav[8..16], b"WAVEfmt ");
            let data_len = u32::from_le_bytes(s.wav[40..44].try_into().unwrap()) as usize;
            assert_eq!(s.wav.len(), 44 + data_len);
            assert_eq!(data_len, (SR * s.seconds as f32) as usize * 2);
        }
    }

    #[test]
    fn volume_scales_the_samples_and_keeps_the_header() {
        let s = hiss(3, 0.5);
        let half = s.wav_at_volume(0.5);
        assert_eq!(half.len(), s.wav.len());
        assert_eq!(&half[..44], &s.wav[..44]);
        let peak = |w: &[u8]| {
            w[44..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs()).max().unwrap()
        };
        let (full, scaled) = (peak(&s.wav), peak(&half));
        assert!((scaled as f32 - full as f32 * 0.5).abs() <= 1.0, "{full} -> {scaled}");
        assert_eq!(peak(&s.wav_at_volume(0.0)), 0);
    }

    #[test]
    fn aaaa_voice_is_unchanged_from_the_first_version() {
        // Golden value recorded from the v0.1 implementation (commit
        // 91a751c), run on the same architecture: checksum of its 16-bit
        // samples.
        let pcm: Vec<i16> = aaaa_voice(7, 600.0, 0.5)
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        assert_eq!(pcm.len(), 22_050);
        let checksum = pcm.iter().fold(0u64, |h, &v| h.wrapping_mul(31).wrapping_add(v as u16 as u64));
        assert_eq!(checksum, GOLDEN_AAAA_CHECKSUM);
    }

    /// See `aaaa_voice_is_unchanged_from_the_first_version`. Apple's math
    /// library rounds a few last bits of `sin`/`exp`/`tanh` differently on
    /// Intel, so about 2% of samples differ by one step (1/32767) between
    /// architectures. Each one is pinned to the v0.1 code's output on it.
    #[cfg(target_arch = "aarch64")]
    const GOLDEN_AAAA_CHECKSUM: u64 = 15_922_795_222_752_550_495;
    #[cfg(target_arch = "x86_64")]
    const GOLDEN_AAAA_CHECKSUM: u64 = 5_025_717_520_135_892_727;

    #[test]
    fn hiss_is_long_and_steady_not_a_short_burst() {
        for seed in [1, 2, 3] {
            let x = long_hiss(seed, 1.4);
            let attack = rms(window(&x, 0.06, 0.16));
            let middle = rms(window(&x, 0.5, 0.9));
            // Up to strength fast, and still going strong in the middle.
            assert!(attack > 0.15, "weak start: {attack}");
            assert!(middle > 0.7 * attack, "should hold: {attack} -> {middle}");
            // Fades out at the end.
            assert!(rms(window(&x, 1.36, 1.4)) < 0.3 * middle);
        }
    }

    #[test]
    fn hiss_is_breathy_and_brighter_than_the_voice() {
        let x = long_hiss(5, 1.4);
        // Noise centred near 2 kHz: well above the ~600-1100 Hz voice.
        let rate = zcr(window(&x, 0.5, 1.0));
        assert!((3_000.0..7_000.0).contains(&rate), "zcr {rate}");
        let voice = zcr(window(&aaaa_voice(5, 650.0, 1.4), 0.5, 1.0));
        assert!(rate > 1.5 * voice, "hiss {rate} vs voice {voice}");
    }
}

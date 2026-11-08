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

    fn aaaa_pcm() -> Vec<i16> {
        aaaa_voice(7, 600.0, 0.5).iter().map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect()
    }

    #[test]
    fn aaaa_voice_matches_the_first_version_on_every_platform() {
        // Each OS's math library rounds the last bits of sin/exp/tanh a
        // little differently, so allow a few steps out of 32,767 (rounding
        // noise). Any real change to the voice moves samples by thousands.
        let pcm = aaaa_pcm();
        assert_eq!(pcm.len(), 22_050);
        let worst = pcm.iter().step_by(50).zip(AAAA_REFERENCE).map(|(&a, b)| (a as i32 - b as i32).abs()).max();
        let worst = worst.unwrap_or(0);
        assert!(worst <= 16, "the AAAA voice differs from the original by up to {worst}/32767");
    }

    /// Every 50th sample of the first version's "AAAA" (commit 91a751c) for
    /// seed 7, 600 Hz, 0.5 s, recorded on an Apple Silicon Mac.
    const AAAA_REFERENCE: [i16; 441] = [
        0, 8, 255, -687, -88, 552, -596, -297, 805, 1720, -3757, -2489, 495, 6182,
        -2399, -3021, -697, 1336, 12403, -7673, -1248, 5268, 4308, 15687, -1653, -26018, 1590, 3953,
        -1966, 24106, -3013, -28110, 2651, 12993, 1252, 21513, 9295, -23700, -9831, 2977, 8105, 2112,
        23370, 2746, -24739, -8322, 1360, 2386, -3854, 23169, 6817, -19987, -23289, 1963, 5906, 5565,
        22361, 16636, -13156, -26847, 1515, 6957, 7028, -1947, 20354, 22127, -4774, -26545, -20905, 3299,
        1616, 1078, -1584, 23745, 20201, -5303, -26468, -22510, 2100, 5426, 3134, 6137, 22640, 24601,
        15201, -12741, -27621, -21248, 1163, 2144, 1967, 1647, 16588, 26449, 17076, -8975, -25299, -25039,
        4168, -2568, -1246, 6995, 12521, 25587, 19699, -7270, -24862, -26808, 103, -840, 123, -2193,
        3339, 23415, 24663, 13569, -17088, -26261, -26725, -12618, -684, 289, -3095, 3449, 6020, 13545,
        20661, 28109, 26678, 27017, 22846, 20216, 17790, 10677, -355, -1574, -2689, -5203, -11531, -2034,
        8094, 8438, 19331, 21810, 23638, 26233, 27942, 28796, 24661, 20920, 15236, 15860, 17701, 6105,
        -3652, -23362, -29298, -28142, -27051, -25392, -11292, 11669, 24491, 27321, 28709, 23967, 18153, 13212,
        8802, -2287, -9100, -18502, -28324, -28085, -27402, -26013, -23835, -21687, -15109, -445, 6401, 13850,
        18413, 26554, 26417, 27129, 27968, 27774, 25592, 27069, 26500, 27142, 25930, 25927, 25246, 24846,
        24379, 25369, 24182, 25277, 24015, 22145, 19239, 20483, 18553, 21331, 23383, 23500, 21879, 20414,
        20831, 21779, 17135, 18715, 20560, 21070, 16314, 20628, 20824, 20127, 13537, 17638, 17891, 18205,
        14262, 14930, 17059, 17981, 18910, 21878, 19830, 25420, 21263, 24221, 24068, 26659, 24191, 26298,
        24889, 25344, 26114, 24242, 22854, 12596, 14160, 8039, 3508, 117, -8368, -12342, -11386, -14629,
        -17806, -19911, -18396, -20023, -17032, -21130, -21703, -16602, -20574, -24106, -18898, -17216, -15197, -11451,
        -6302, -2949, 7088, 16718, 19395, 22962, 25032, 26284, 22419, 20455, 13775, 11762, 6201, -191,
        -2843, -23912, -27018, -25935, -22880, -14870, -68, 17219, 23993, 21944, 21099, 11178, 11899, 1008,
        -2321, -12621, -24274, -24219, -21386, -21335, -19294, -9757, -2847, 1762, 9783, 12651, 14673, 18295,
        18192, 19331, 16454, 15403, 16628, 14751, 13284, 9955, 5498, -170, -3807, -10130, -13814, -15274,
        -17883, -10565, -953, -2354, -1811, 792, 269, 6095, 15836, 14335, 3753, -8908, -14058, -12847,
        2084, -597, 632, 1316, 7289, 12656, 4489, -6612, -13998, 2367, 1619, 1956, -653, 7483,
        6783, -2042, -13164, -187, 235, 2407, -1283, 6084, 2108, -8442, -4256, 1522, 3424, 599,
        6788, 192, -8285, -548, 777, 2247, 3203, 3063, -4689, -1319, 567, 1330, 3196, 600,
        -5023, 236, 1209, 978, 2837, -1936, -836, 805, 1898, 2641, 150, -3898, 323, 891,
        -195, 1174, -2413, -790, 527, 244, 1657, -230, -2984, -323, 434, 103, 1508, -135,
        -2561, 17, 558, 458, 1194, 56, -1251, -230, -42, 444, 299, 760, -264, -890,
        25, 170, 131, 392, 172, -342, -176, 52, 37, 16, 132, -48, -167, -3,
        33, 3, 25, -11, -2, 1, 0,
    ];

    #[cfg(target_os = "macos")]
    #[test]
    fn aaaa_voice_is_bit_identical_to_the_first_version_on_macos() {
        // On macOS, where the first version ran, it must match exactly:
        // checksums of the v0.1 code's 16-bit output on each architecture.
        let checksum = aaaa_pcm().iter().fold(0u64, |h, &v| h.wrapping_mul(31).wrapping_add(v as u16 as u64));
        assert_eq!(checksum, GOLDEN_AAAA_CHECKSUM);
    }

    /// Apple's math library rounds a few last bits of `sin`/`exp`/`tanh`
    /// differently on Intel, so about 2% of samples differ by one step
    /// (1/32767) between the two Mac architectures.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    const GOLDEN_AAAA_CHECKSUM: u64 = 15_922_795_222_752_550_495;
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
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

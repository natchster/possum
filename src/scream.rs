//! Procedurally synthesised screams.
//!
//! A scream is a source-filter model of a voice, the same way speech
//! synthesisers work:
//!
//! * **source**: a band-limited sawtooth (the "vocal folds") whose pitch
//!   shoots up, wobbles with vibrato and random jitter, then cracks downward
//!   at the end. A sub-harmonic amplitude modulation adds the rough, growly
//!   texture that makes real screams sound alarming.
//! * **filter**: three parallel band-pass resonators at the formant
//!   frequencies of an open "AAAA" vowel.
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

pub struct Scream {
    pub wav: Vec<u8>,
    pub seconds: f64,
}

/// Synthesise one scream. `pitch` is the base fundamental in Hz.
pub fn synthesize(seed: u32, pitch: f32, seconds: f32) -> Scream {
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

    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-6);
    let gain = 0.9 / peak;
    Scream { wav: wav_pcm16(samples.iter().map(|s| s * gain)), seconds: seconds as f64 }
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

    #[test]
    fn wav_header_is_consistent() {
        let s = synthesize(7, 600.0, 0.5);
        assert_eq!(&s.wav[0..4], b"RIFF");
        assert_eq!(&s.wav[8..16], b"WAVEfmt ");
        let data_len = u32::from_le_bytes(s.wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(s.wav.len(), 44 + data_len);
        assert_eq!(data_len, (SAMPLE_RATE as f32 * 0.5) as usize * 2);
    }

    #[test]
    fn scream_is_loud_and_not_clipped_garbage() {
        let s = synthesize(42, 650.0, 1.0);
        let pcm: Vec<i16> = s.wav[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let peak = pcm.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!(peak > 25_000, "peak {peak}");
        let rms = (pcm.iter().map(|&v| (v as f64).powi(2)).sum::<f64>() / pcm.len() as f64).sqrt();
        assert!(rms > 3_000.0, "too quiet, rms {rms}");
    }
}

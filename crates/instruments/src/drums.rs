//! Drum voices: kick, snare, rim, brush tap and swish, hat, shaker, tom and a
//! PolyBLEP ride, rendered in place into a stereo stem (design section 5.8).
//!
//! Each hit is synthesised into a mono scratch buffer (reused across hits, no
//! per-hit allocation once it has grown to the longest voice), scaled by the
//! hit velocity and added at `sample_at(hit.t)` with equal-power pan gains
//! (`dsp::pan::equal_power`).
//!
//! Building blocks:
//! - Exponential envelopes `exp(-t / tau)` are recursive: `e *= exp(-1 / (tau fs))`
//!   per sample (`Decay`).
//! - Sines are the "magic circle" recursive oscillator (Gordon and Smith
//!   1985, "A sine generation algorithm for VLSI applications", ICMC):
//!   `c -= k s; s += k c` with `k = 2 sin(w / 2)`. Stable for any `k` in
//!   (0, 2), so the frequency can be swept per sample (kick, tom); the sine
//!   output drifts from unit amplitude by at most `k^2 / 8` (7e-4 at 520 Hz).
//! - Noise: `Rng::fill_bipolar` into scratch, filtered with `dsp::biquad`
//!   (RBJ cookbook designs).
//! - Ride squares are band-limited by PolyBLEP (Valimaki and Huovilainen 2007,
//!   "Antialiasing oscillators in subtractive synthesis", IEEE Signal
//!   Processing Magazine 24(2)): a two-sample polynomial residual of the
//!   band-limited step is added at each discontinuity.
//!
//! Voices and the parameters that set their sound (times in seconds):
//! - Kick, 0.45: sine swept `47 + 72 exp(-t/0.034)` Hz (119 to 47), decay
//!   0.17; noise click 4 ms, gain 0.25, decay 0.0012.
//! - Snare, 0.30: 188 Hz sine x 0.45, decay 0.05; noise high-passed at 1.4 kHz
//!   x 0.7, decay 0.11.
//! - Rim, 0.12: noise band-passed 1.8 kHz Q 6 x 3, decay 0.02; 520 Hz ping
//!   x 0.4, decay 0.015.
//! - Brush tap, 0.25: head = 186 Hz sine x 0.35 + noise band-passed 190 Hz
//!   Q 6 x 1.6 + 330 Hz Q 5 x 0.9, decay 0.07, 2 ms attack; snare wires =
//!   noise band-passed 3.8 kHz Q 0.9 x 0.75, decay 0.1, 4 ms attack.
//! - Swish, `dur` (default 0.5): noise band-passed 3.6 kHz Q 0.7, high shelf
//!   -6 dB at 7 kHz, x 0.2, plus noise band-passed 200 Hz Q 4 x 0.25;
//!   envelope `sin(pi x)^1.5 (0.6 + 0.4 |sin(2 pi x)|)` over x in [0, 1)
//!   (the swell, and the stroke modulation of the swirl).
//! - Hat, 0.12: noise high-passed at 7.2 kHz x 0.7, decay 0.032.
//! - Shaker, 0.12: noise band-passed 6.2 kHz Q 1.3 x 1.4, 12 ms attack,
//!   decay 0.045.
//! - Tom `hz` (default 110), 0.5: sine swept `hz (1 + 0.35 exp(-t/0.04))`,
//!   decay 0.28; noise x 0.05, decay 0.02.
//! - Ride, 1.6: six squares at 1.9 x (421, 601, 793, 1033, 1285, 1559) Hz
//!   (800-2962 Hz, an 808-style cluster) with random start phases, averaged,
//!   high-passed at 3.5 kHz, x 0.35, decay 0.7; plus a ping of noise
//!   band-passed 5.2 kHz Q 4 x 0.6, decay 0.04.

use dsp::biquad::{Biquad, BiquadCoeffs};
use dsp::pan::{add_mono, equal_power};
use sfcore::random::{tag, Rng, Tag};
use sfcore::time::sample_at;
use sfcore::SR_F;
use song::events::{DrumHit, DrumKind};
use std::f64::consts::PI;

/// Stream tag for per-hit random draws.
pub const DRUM_HIT: Tag = tag("drums.hit");

/// Longest swish in seconds; longer (or non-finite) requests are clamped.
pub const MAX_SWISH: f64 = 4.0;
/// Shortest swish in seconds.
pub const MIN_SWISH: f64 = 0.02;

/// Ride oscillator frequencies in Hz before the 1.9 scale.
pub const RIDE_BASE_HZ: [f64; 6] = [421.0, 601.0, 793.0, 1033.0, 1285.0, 1559.0];
/// Scale applied to `RIDE_BASE_HZ`.
pub const RIDE_SCALE: f64 = 1.9;

/// Reusable buffers for `render_hit`: the mono voice and up to three noise
/// sources. They grow to the longest voice and are then reused.
#[derive(Default, Debug, Clone)]
pub struct DrumScratch {
    mono: Vec<f32>,
    n0: Vec<f32>,
    n1: Vec<f32>,
    n2: Vec<f32>,
}

impl DrumScratch {
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure(&mut self, n: usize) {
        for v in [&mut self.mono, &mut self.n0, &mut self.n1, &mut self.n2] {
            if v.len() < n {
                v.resize(n, 0.0);
            }
        }
    }
}

/// Recursive exponential decay `exp(-t / tau)`, one multiply per sample.
#[derive(Clone, Copy)]
struct Decay {
    e: f64,
    r: f64,
}

impl Decay {
    fn new(tau: f64) -> Self {
        Decay { e: 1.0, r: (-1.0 / (tau * SR_F)).exp() }
    }

    /// Current value, then advance.
    #[inline(always)]
    fn next(&mut self) -> f64 {
        let v = self.e;
        self.e *= self.r;
        v
    }
}

/// Linear attack `min(1, t / t_att)`.
#[derive(Clone, Copy)]
struct Attack {
    a: f64,
    inc: f64,
}

impl Attack {
    fn new(t_att: f64) -> Self {
        Attack { a: 0.0, inc: 1.0 / (t_att * SR_F) }
    }

    #[inline(always)]
    fn next(&mut self) -> f64 {
        let v = self.a;
        self.a = (self.a + self.inc).min(1.0);
        v
    }
}

/// Magic-circle sine oscillator (Gordon and Smith 1985). `s` starts at 0,
/// so the output is `sin(phase)` with phase advanced before each output.
#[derive(Clone, Copy)]
struct Sine {
    c: f64,
    s: f64,
}

impl Sine {
    fn new() -> Self {
        Sine { c: 1.0, s: 0.0 }
    }

    /// `k = 2 sin(w/2)` for `w = 2 pi f / fs`, by its Taylor series to w^5
    /// (relative error below 1e-9 for w < 0.1).
    #[inline(always)]
    fn k_of(f: f64) -> f64 {
        let w = 2.0 * PI * f / SR_F;
        let w2 = w * w;
        w * (1.0 - w2 / 24.0 + w2 * w2 / 1920.0)
    }

    /// Advance by `k` and return the sine.
    #[inline(always)]
    fn next(&mut self, k: f64) -> f64 {
        self.c -= k * self.s;
        self.s += k * self.c;
        self.s
    }
}

/// PolyBLEP residual (Valimaki and Huovilainen 2007) for an upward step of
/// height 2 (a bipolar -1 to +1 edge) at phase 0, with phase `t` in [0, 1) and increment `dt`.
#[inline(always)]
pub fn poly_blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Band-limited square (+1 for phase < 0.5, -1 after), phase advanced first:
/// the naive square plus a PolyBLEP at the rising edge (phase 0) and minus
/// one at the falling edge (phase 0.5). `ph` in [0, 1), `dt = f / fs`.
///
/// Measured on the ride cluster (six squares, 800-2962 Hz, 44.1 kHz, against
/// an additive band-limited ideal): alias energy below 3.5 kHz 19 dB lower
/// than the naive squares, 3.5-10 kHz 10.6 dB lower. Energy above 15 kHz is
/// 7.6 dB lower on the ride hit; the ideal square cluster itself is only
/// 2.5 dB below the naive one there, since its true odd harmonics lie in
/// 15-22 kHz, so the rest is the gentle roll-off of the two-sample residual.
#[inline(always)]
pub fn blep_square(ph: &mut f64, dt: f64) -> f64 {
    *ph += dt;
    if *ph >= 1.0 {
        *ph -= 1.0;
    }
    let p = *ph;
    let naive = if p < 0.5 { 1.0 } else { -1.0 };
    let mut q = p + 0.5;
    if q >= 1.0 {
        q -= 1.0;
    }
    naive + poly_blep(p, dt) - poly_blep(q, dt)
}

fn secs(t: f64) -> usize {
    (t * SR_F).round() as usize
}

fn filter(buf: &mut [f32], c: BiquadCoeffs) {
    Biquad::new(c).process(buf);
}

fn bp(f: f64, q: f64) -> BiquadCoeffs {
    BiquadCoeffs::bandpass(SR_F, f, q)
}

fn hp(f: f64, q: f64) -> BiquadCoeffs {
    BiquadCoeffs::highpass(SR_F, f, q)
}

/// Length in samples of a voice.
pub fn hit_len(kind: DrumKind) -> usize {
    match kind {
        DrumKind::Kick => secs(0.45),
        DrumKind::Snare => secs(0.3),
        DrumKind::Rim => secs(0.12),
        DrumKind::Tap => secs(0.25),
        DrumKind::Swish { dur } => secs(swish_dur(dur)),
        DrumKind::Hat | DrumKind::Shaker => secs(0.12),
        DrumKind::Tom { .. } => secs(0.5),
        DrumKind::Ride => secs(1.6),
    }
}

fn swish_dur(dur: f32) -> f64 {
    let d = dur as f64;
    if d.is_finite() && d > 0.0 {
        d.clamp(MIN_SWISH, MAX_SWISH)
    } else {
        0.5
    }
}

fn tom_hz(hz: f32) -> f64 {
    let f = hz as f64;
    if f.is_finite() && f > 0.0 {
        f.clamp(20.0, 2000.0)
    } else {
        110.0
    }
}

/// Synthesise one voice at unit velocity into `s.mono[..n]`; returns n.
fn synth(kind: DrumKind, rng: &mut Rng, s: &mut DrumScratch) -> usize {
    let n = hit_len(kind);
    s.ensure(n);
    let DrumScratch { mono, n0, n1, n2 } = s;
    let out = &mut mono[..n];
    match kind {
        DrumKind::Kick => {
            let click_n = secs(0.004).min(n);
            rng.fill_bipolar(&mut n0[..click_n]);
            let (mut osc, mut env, mut sweep, mut click) = (Sine::new(), Decay::new(0.17), Decay::new(0.034), Decay::new(0.0012));
            for (i, v) in out.iter_mut().enumerate() {
                let k = Sine::k_of(47.0 + 72.0 * sweep.next());
                let mut x = osc.next(k) * env.next();
                if i < click_n {
                    x += n0[i] as f64 * 0.25 * click.next();
                }
                *v = x as f32;
            }
        }
        DrumKind::Snare => {
            let nz = &mut n0[..n];
            rng.fill_bipolar(nz);
            filter(nz, hp(1400.0, 0.7));
            let k = Sine::k_of(188.0);
            let (mut osc, mut e1, mut e2) = (Sine::new(), Decay::new(0.05), Decay::new(0.11));
            for (v, z) in out.iter_mut().zip(nz.iter()) {
                *v = (osc.next(k) * 0.45 * e1.next() + *z as f64 * 0.7 * e2.next()) as f32;
            }
        }
        DrumKind::Rim => {
            let nz = &mut n0[..n];
            rng.fill_bipolar(nz);
            filter(nz, bp(1800.0, 6.0));
            let k = Sine::k_of(520.0);
            let (mut osc, mut e1, mut e2) = (Sine::new(), Decay::new(0.02), Decay::new(0.015));
            for (v, z) in out.iter_mut().zip(nz.iter()) {
                *v = (*z as f64 * 3.0 * e1.next() + osc.next(k) * 0.4 * e2.next()) as f32;
            }
        }
        DrumKind::Tap => {
            let (wires, h1, h2) = (&mut n0[..n], &mut n1[..n], &mut n2[..n]);
            rng.fill_bipolar(wires);
            filter(wires, bp(3800.0, 0.9));
            rng.fill_bipolar(h1);
            filter(h1, bp(190.0, 6.0));
            rng.fill_bipolar(h2);
            filter(h2, bp(330.0, 5.0));
            let k = Sine::k_of(186.0);
            let mut osc = Sine::new();
            let (mut eh, mut ah, mut ew, mut aw) = (Decay::new(0.07), Attack::new(0.002), Decay::new(0.1), Attack::new(0.004));
            for i in 0..n {
                let head = (osc.next(k) * 0.35 + h1[i] as f64 * 1.6 + h2[i] as f64 * 0.9) * eh.next() * ah.next();
                let wire = wires[i] as f64 * 0.75 * ew.next() * aw.next();
                out[i] = (head + wire) as f32;
            }
        }
        DrumKind::Swish { .. } => {
            let (sw, hd) = (&mut n0[..n], &mut n1[..n]);
            rng.fill_bipolar(sw);
            filter(sw, bp(3600.0, 0.7));
            filter(sw, BiquadCoeffs::high_shelf(SR_F, 7000.0, 0.7, -6.0));
            rng.fill_bipolar(hd);
            filter(hd, bp(200.0, 4.0));
            // sin(pi x), x = i / n, as a rotating phasor (exact angle
            // increment); sin(2 pi x) = 2 sin(pi x) cos(pi x).
            let w = PI / n as f64;
            let (rc, rs) = (w.cos(), w.sin());
            let (mut c, mut sn) = (1.0f64, 0.0f64);
            for i in 0..n {
                let s1 = sn.max(0.0);
                let e = s1 * s1.sqrt() * (0.6 + 0.4 * (2.0 * sn * c).abs());
                out[i] = ((sw[i] as f64 * 0.2 + hd[i] as f64 * 0.25) * e) as f32;
                let c2 = c * rc - sn * rs;
                sn = sn * rc + c * rs;
                c = c2;
            }
        }
        DrumKind::Hat => {
            rng.fill_bipolar(out);
            filter(out, hp(7200.0, 0.7));
            let mut e = Decay::new(0.032);
            for v in out.iter_mut() {
                *v = (*v as f64 * 0.7 * e.next()) as f32;
            }
        }
        DrumKind::Shaker => {
            rng.fill_bipolar(out);
            filter(out, bp(6200.0, 1.3));
            let (mut a, mut e) = (Attack::new(0.012), Decay::new(0.045));
            for v in out.iter_mut() {
                *v = (*v as f64 * 1.4 * a.next() * e.next()) as f32;
            }
        }
        DrumKind::Tom { hz } => {
            let f = tom_hz(hz);
            let nz = &mut n0[..n];
            rng.fill_bipolar(nz);
            let (mut osc, mut env, mut sweep, mut en) = (Sine::new(), Decay::new(0.28), Decay::new(0.04), Decay::new(0.02));
            for (v, z) in out.iter_mut().zip(nz.iter()) {
                let k = Sine::k_of(f * (1.0 + 0.35 * sweep.next()));
                *v = (osc.next(k) * env.next() + *z as f64 * 0.05 * en.next()) as f32;
            }
        }
        DrumKind::Ride => {
            let mut ph = [0.0f64; 6];
            for p in ph.iter_mut() {
                *p = rng.uniform();
            }
            let dt = RIDE_BASE_HZ.map(|f| f * RIDE_SCALE / SR_F);
            for v in out.iter_mut() {
                let mut acc = 0.0;
                for k in 0..6 {
                    acc += blep_square(&mut ph[k], dt[k]);
                }
                *v = (acc / 6.0) as f32;
            }
            filter(out, hp(3500.0, 0.7));
            let ping = &mut n0[..n];
            rng.fill_bipolar(ping);
            filter(ping, bp(5200.0, 4.0));
            let (mut e1, mut e2) = (Decay::new(0.7), Decay::new(0.04));
            for (v, p) in out.iter_mut().zip(ping.iter()) {
                *v = (*v as f64 * 0.35 * e1.next() + *p as f64 * 0.6 * e2.next()) as f32;
            }
        }
    }
    n
}

/// Render one hit into the stereo stem: synthesise the voice, scale by
/// `hit.vel`, pan with equal-power gains and add at `sample_at(hit.t)`.
/// Samples outside the stem are dropped. A non-finite velocity renders
/// nothing.
pub fn render_hit(hit: &DrumHit, rng: &mut Rng, out_l: &mut [f32], out_r: &mut [f32], scratch: &mut DrumScratch) {
    let vel = hit.vel;
    if !vel.is_finite() || vel == 0.0 || !hit.t.is_finite() {
        return;
    }
    let n = synth(hit.kind, rng, scratch);
    let [gl, gr] = equal_power(hit.pan as f64);
    add_mono(out_l, out_r, sample_at(hit.t), &scratch.mono[..n], [gl * vel, gr * vel]);
}

/// Render all hits into a stereo stem of `len` samples. Hit k draws from
/// `Rng::event(seed, DRUM_HIT, k)`, so each hit's noise is independent of the
/// others.
pub fn render_drums(hits: &[DrumHit], seed: u64, len: usize) -> [Vec<f32>; 2] {
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    let mut scratch = DrumScratch::new();
    for (k, hit) in hits.iter().enumerate() {
        let mut rng = Rng::event(seed, DRUM_HIT, k as u64);
        render_hit(hit, &mut rng, &mut l, &mut r, &mut scratch);
    }
    [l, r]
}

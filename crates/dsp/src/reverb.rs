//! Reverb: an eight-line feedback delay network (Jot and Chaigne 1991,
//! "Digital delay networks for designing artificial reverberators", AES 90th
//! Convention) with frequency-dependent absorption, fed through a predelay,
//! a high-pass and four series Schroeder allpasses (Schroeder 1962).
//!
//! Signal flow per sample, input (L, R):
//! 1. mono sum `(L + R) / 2` -> 220 Hz high-pass (RBJ, Q 0.7) -> 16 ms
//!    predelay -> allpasses 142/379/107/277 samples, g 0.65/0.62/0.60/0.58.
//! 2. Each line k is read at its delay `d_k` and low-passed by its absorption
//!    one-pole: `o_k = b_k v_k + a_k o_k[n-1]`.
//! 3. Feedback: Householder matrix `A = I - (2/N) 1 1^T` (N = 8), lossless
//!    and dense, so line k is written with `o_k - (2/8) sum(o)`, plus the
//!    diffused input times 0.5, plus the side signal `0.15 (L - R)` with a
//!    sign that alternates in pairs of lines (- - + + - - + +).
//! 4. Output taps: `L = o0 - o2 + o4 - o6 + (o1 + o7)/2`,
//!    `R = o1 - o3 + o5 - o7 + (o2 + o6)/2`. Different line sets per channel
//!    decorrelate the two outputs.
//!
//! Absorption design (Jot and Chaigne 1991, section 3). A line of `d` samples
//! must lose 60 dB in T60 seconds, so its gain per pass is
//! `g(T60) = 10^(-3 d / (T60 fs))`. With `gd = g(t60_dc)` and
//! `gn = g(t60_nyq)`, the one-pole
//! `H(z) = gd (1 - a) / (1 - a z^-1)`, `a = (gd - gn) / (gd + gn)`,
//! has `H(1) = gd` at DC and `H(-1) = gd (1 - a) / (1 + a) = gn` at Nyquist,
//! so every line decays at the same rate per second at both ends of the band.
//! Between them T60(f) follows `-3 d / (fs log10 |H(e^jw)|)`.
//!
//! Delay lines: base lengths 1433, 1601, 1867, 2053, 2251, 2399, 2617, 2903
//! (mutually prime, spread over 32-66 ms), each plus a seed offset
//! `(seed (d mod 7)) mod 31` in [0, 30]. All eight live in one `f32` buffer
//! of eight power-of-two segments; one write index is shared and wrap is a
//! mask. Loop filter state is `f64` (design section 4).
//!
//! Parameters that set the sound: the two T60s (2.2 s at DC, 0.8 s at
//! Nyquist in the mix), the delay lengths, the allpass lengths and gains, the
//! 16 ms predelay, the 220 Hz input high-pass and the wet gain (0.55).

use crate::biquad::{Biquad, BiquadCoeffs, DENORMAL_FLOOR};
use crate::delay::{DelayLine, SchroederAllpass};
use sfcore::SR_F;

/// Number of delay lines.
pub const N_LINES: usize = 8;
/// Base delay lengths in samples at 44.1 kHz.
pub const BASE_DELAYS: [usize; N_LINES] = [1433, 1601, 1867, 2053, 2251, 2399, 2617, 2903];
/// Series allpass lengths (samples) and gains.
const AP_LEN: [usize; 4] = [142, 379, 107, 277];
const AP_G: [f32; 4] = [0.65, 0.62, 0.60, 0.58];
/// Predelay in seconds.
const PREDELAY_S: f64 = 0.016;
/// Input high-pass corner (Hz) and Q.
const HP_HZ: f64 = 220.0;
const HP_Q: f64 = 0.7;
/// Gain of the diffused mono input into every line.
const IN_GAIN: f64 = 0.5;
/// Gain of the side signal `L - R` into the lines.
const SIDE_GAIN: f64 = 0.15;
/// Mix T60s: 2.2 s at DC, 0.8 s at Nyquist.
pub const T60_DC: f64 = 2.2;
pub const T60_NYQ: f64 = 0.8;

/// Delay of line `k` for `seed`: base plus `(seed (base mod 7)) mod 31`.
pub fn line_delay(k: usize, seed: u64) -> usize {
    let b = BASE_DELAYS[k % N_LINES];
    b + (seed.wrapping_mul((b % 7) as u64) % 31) as usize
}

/// Absorption one-pole `(b, a)` for a line of `d` samples:
/// `y = b x + a y[n-1]` with DC gain `gd` and Nyquist gain `gn` (module doc).
pub fn absorption(d: usize, fs: f64, t60_dc: f64, t60_nyq: f64) -> (f64, f64) {
    let gd = 10f64.powf(-3.0 * d as f64 / (t60_dc * fs));
    let gn = 10f64.powf(-3.0 * d as f64 / (t60_nyq * fs));
    let a = (gd - gn) / (gd + gn);
    (gd * (1.0 - a), a)
}

/// Eight-line feedback delay network reverb. See the module doc.
#[derive(Clone, Debug)]
pub struct Fdn8 {
    /// Eight segments of `seg` samples each; line k is `buf[k seg .. (k+1) seg]`.
    buf: Vec<f32>,
    seg: usize,
    mask: usize,
    /// Shared write index (masked on use).
    w: usize,
    delay: [usize; N_LINES],
    b: [f64; N_LINES],
    a: [f64; N_LINES],
    /// Absorption filter outputs, one per line.
    z: [f64; N_LINES],
    hp: Biquad,
    pre: DelayLine,
    pre_d: usize,
    ap: [SchroederAllpass; 4],
}

impl Fdn8 {
    /// A reverb at sample rate `fs` with line lengths offset by `seed` and
    /// the given T60s in seconds. Non-finite or non-positive values fall
    /// back to 44.1 kHz and the mix T60s.
    pub fn new(fs: f64, seed: u64, t60_dc: f64, t60_nyq: f64) -> Self {
        let ok = |v: f64, dflt: f64| if v.is_finite() && v > 0.0 { v } else { dflt };
        let fs = ok(fs, SR_F);
        let t60_dc = ok(t60_dc, T60_DC);
        let t60_nyq = ok(t60_nyq, T60_NYQ);
        let mut delay = [0usize; N_LINES];
        let mut b = [0.0; N_LINES];
        let mut a = [0.0; N_LINES];
        for k in 0..N_LINES {
            delay[k] = line_delay(k, seed);
            (b[k], a[k]) = absorption(delay[k], fs, t60_dc, t60_nyq);
        }
        let seg = (delay.iter().copied().max().unwrap_or(1) + 1).next_power_of_two();
        let pre_d = ((PREDELAY_S * fs).round() as usize).max(1);
        Fdn8 {
            buf: vec![0.0; seg * N_LINES],
            seg,
            mask: seg - 1,
            w: 0,
            delay,
            b,
            a,
            z: [0.0; N_LINES],
            hp: Biquad::new(BiquadCoeffs::highpass(fs, HP_HZ, HP_Q)),
            pre: DelayLine::new(pre_d),
            pre_d,
            ap: [0, 1, 2, 3].map(|i| SchroederAllpass::new(AP_LEN[i], AP_G[i])),
        }
    }

    /// Line delays in samples.
    pub fn delays(&self) -> [usize; N_LINES] {
        self.delay
    }

    /// One stereo sample in, the unscaled wet stereo sample out.
    #[inline(always)]
    fn tick(&mut self, l: f64, r: f64) -> [f64; 2] {
        // Input diffusion: high-pass, predelay, series allpasses.
        let x = self.hp.tick((l + r) * 0.5);
        self.pre.push(x as f32);
        let mut xf = self.pre.read_int(self.pre_d);
        for ap in self.ap.iter_mut() {
            xf = ap.tick(xf);
        }
        let x = xf as f64 * IN_GAIN;
        let side = (l - r) * SIDE_GAIN;

        // Read and absorb.
        let w = self.w;
        let mut sum = 0.0;
        for k in 0..N_LINES {
            let v = self.buf[k * self.seg + (w.wrapping_sub(self.delay[k]) & self.mask)] as f64;
            let o = self.b[k] * v + self.a[k] * self.z[k];
            self.z[k] = o;
            sum += o;
        }
        // Householder feedback plus input.
        let h = sum * (2.0 / N_LINES as f64);
        let wi = w & self.mask;
        for k in 0..N_LINES {
            let s = if k & 2 != 0 { side } else { -side };
            self.buf[k * self.seg + wi] = (self.z[k] - h + x + s) as f32;
        }
        self.w = w.wrapping_add(1);

        let o = &self.z;
        [
            o[0] - o[2] + o[4] - o[6] + 0.5 * (o[1] + o[7]),
            o[1] - o[3] + o[5] - o[7] + 0.5 * (o[2] + o[6]),
        ]
    }

    /// One stereo sample in, the unscaled wet stereo sample out.
    pub fn process(&mut self, x: [f32; 2]) -> [f32; 2] {
        let [yl, yr] = self.tick(x[0] as f64, x[1] as f64);
        [yl as f32, yr as f32]
    }

    /// Run the send buses through the reverb and add `wet` times the result
    /// into `out`. Processes the shortest of the four lengths. Flushes
    /// denormal filter state at the end of the block.
    pub fn process_block(&mut self, send: [&[f32]; 2], out: [&mut [f32]; 2], wet: f64) {
        let [sl, sr] = send;
        let [ol, or] = out;
        let n = sl.len().min(sr.len()).min(ol.len()).min(or.len());
        for i in 0..n {
            let [yl, yr] = self.tick(sl[i] as f64, sr[i] as f64);
            ol[i] = (ol[i] as f64 + wet * yl) as f32;
            or[i] = (or[i] as f64 + wet * yr) as f32;
        }
        self.flush_denormals();
    }

    /// Zero loop and input filter state below `DENORMAL_FLOOR`.
    pub fn flush_denormals(&mut self) {
        for z in self.z.iter_mut() {
            if z.abs() < DENORMAL_FLOOR {
                *z = 0.0;
            }
        }
        self.hp.flush_denormals();
    }

    /// Clear all state.
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
        self.z = [0.0; N_LINES];
        self.hp.reset();
        self.pre.clear();
        for ap in self.ap.iter_mut() {
            ap.reset();
        }
    }
}

/// shim: deleted when the mix moves to `Fdn8` directly. Adds `wet` times
/// the reverb of (`in_l`, `in_r`) into (`out_l`, `out_r`) with the mix T60s.
/// A negative `seed` uses its two's-complement bits.
pub fn fdn_reverb(in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32], wet: f64, seed: i64) {
    let mut fdn = Fdn8::new(SR_F, seed as u64, T60_DC, T60_NYQ);
    fdn.process_block([in_l, in_r], [out_l, out_r], wet);
}

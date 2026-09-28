//! Bodies: stochastic modal synthesis of a stereo body impulse response from
//! a measured 1/12-octave magnitude curve (design section 5.6; commuted
//! synthesis lineage: Smith 1993; Karjalainen and Valimaki 1993).
//!
//! Curves (`BODY_CURVES`, dB, 1/12 octave from 80 Hz) were measured from
//! University of Iowa MIS anechoic recordings by tools/extract_curves.py:
//! partial amplitudes of every note divided by the ideal string-force
//! spectrum 1/k. The guitar curve has a steel-string correction; the harp
//! curve is derived from the guitar curve (no harp reference exists).
//! Beyond the measured range the curve falls 12 dB per octave at both ends.
//!
//! Modes run from `f_min` to 13 kHz, spaced `max(3 Hz, 1.1% f)` with a
//! uniform jitter of +-45%. Q ramps from `q_lo` to `q_hi` over 200 Hz to
//! 12.8 kHz (log frequency) with a +-30% jitter. A mode of decay time
//! `tau = Q / (pi f)` has amplitude `sqrt(T df / tau)`, `T` the curve power
//! at f, so the energy per hertz follows the curve. Each channel draws a
//! Gaussian amplitude and a uniform phase per mode (decorrelated stereo).
//! A mode is the recursive oscillator `y = 2 r cos(w) y1 - r^2 y2`, run for
//! 9.5 tau (82 dB) or to the end of the IR, accumulated in `f64`. The last
//! 10% of the IR is faded linearly, then each channel is scaled to unit
//! energy. The IR is not shortened: its 0.32-0.40 s modal tail is part of
//! the body sound.

use dsp::conv::StereoIr;
use sfcore::random::Rng;
use sfcore::SR_F;
use std::f64::consts::{PI, TAU};

/// The instrument bodies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Body {
    Guitar,
    Harp,
    Violin,
}

/// Modal-synthesis parameters for one body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySpec {
    /// IR length, seconds.
    pub secs: f64,
    /// Q at and below 200 Hz.
    pub q_lo: f64,
    /// Q at and above 12.8 kHz.
    pub q_hi: f64,
    /// Lowest mode, Hz.
    pub f_min: f64,
    /// Level trim: `impulse_response` divides the unit-energy IR by this.
    /// Empirical (the original engine's peak-reference divisors), so each
    /// bodied track keeps its shipped level: guitar 5.4, harp 7.3, violin 2.4.
    pub trim: f64,
}

/// First curve point, Hz.
pub const CURVE_LO_HZ: f64 = 80.0;
/// Curve points per octave.
pub const CURVE_PER_OCT: f64 = 12.0;
/// Extrapolation slope beyond the measured range, dB per octave.
pub const CURVE_EDGE_DB_PER_OCT: f64 = 12.0;
/// Highest mode frequency, Hz.
const F_MODE_MAX: f64 = 13000.0;
/// Mode length in decay time constants (82 dB).
const MODE_TAUS: f64 = 9.5;
/// Fraction of the IR faded at the tail.
const TAIL_FADE: f64 = 0.1;

impl Body {
    /// The measured curve, dB, 1/12 octave from `CURVE_LO_HZ`.
    pub fn curve(self) -> &'static [f64; 86] {
        match self {
            Body::Guitar => &BODY_CURVES[0],
            Body::Harp => &BODY_CURVES[1],
            Body::Violin => &BODY_CURVES[2],
        }
    }

    /// Curve level at `f` Hz, dB: linear interpolation on the 1/12-octave
    /// grid; beyond either end, the end value falling
    /// `CURVE_EDGE_DB_PER_OCT` per octave away from the range.
    pub fn level_db(self, f: f64) -> f64 {
        let a = self.curve();
        let last = a.len() - 1;
        let x = (f / CURVE_LO_HZ).log2() * CURVE_PER_OCT;
        if x.is_nan() || x <= 0.0 {
            let oct = if x.is_nan() { 0.0 } else { x / CURVE_PER_OCT };
            return a[0] + CURVE_EDGE_DB_PER_OCT * oct;
        }
        if x >= last as f64 {
            return a[last] - CURVE_EDGE_DB_PER_OCT * (x - last as f64) / CURVE_PER_OCT;
        }
        let i = x as usize;
        let t = x - i as f64;
        a[i] + (a[i + 1] - a[i]) * t
    }

    pub fn spec(self) -> BodySpec {
        match self {
            Body::Guitar => BodySpec { secs: 0.36, q_lo: 26.0, q_hi: 48.0, f_min: 70.0, trim: 5.4 },
            Body::Harp => BodySpec { secs: 0.4, q_lo: 18.0, q_hi: 34.0, f_min: 60.0, trim: 7.3 },
            Body::Violin => BodySpec { secs: 0.32, q_lo: 30.0, q_hi: 40.0, f_min: 180.0, trim: 2.4 },
        }
    }

    /// The stereo modal IR, unit energy per channel, before the trim.
    pub fn taps(self, rng: &mut Rng) -> [Vec<f64>; 2] {
        let s = self.spec();
        let n = (s.secs * SR_F).round() as usize;
        let mut ch = [vec![0.0f64; n], vec![0.0f64; n]];
        let mut f = s.f_min;
        while f < F_MODE_MAX {
            let df = (f * 0.011).max(3.0) * (0.55 + 0.9 * rng.uniform());
            f += df;
            let ramp = ((f / 200.0).log2() / 6.0).clamp(0.0, 1.0);
            let q = s.q_lo + (s.q_hi - s.q_lo) * ramp * (0.7 + 0.6 * rng.uniform());
            let tau = q / (PI * f);
            let power = 10f64.powf(self.level_db(f) / 10.0);
            let a0 = (power * df / tau).sqrt();
            let w = TAU * f / SR_F;
            let r = (-1.0 / (tau * SR_F)).exp();
            let c2 = 2.0 * r * w.cos();
            let r2 = r * r;
            let m = n.min((tau * SR_F * MODE_TAUS).ceil() as usize);
            for d in ch.iter_mut() {
                let a = a0 * rng.gauss();
                let ph = rng.uniform() * TAU;
                // y[i] = a r^i sin(w i + ph): seed y[0] and y[-1].
                let mut y1 = a * ph.sin();
                let mut y2 = a * (ph - w).sin() / r;
                for x in d[..m].iter_mut() {
                    *x += y1;
                    let y = c2 * y1 - r2 * y2;
                    y2 = y1;
                    y1 = y;
                }
            }
        }
        let fl = (n as f64 * TAIL_FADE).round() as usize;
        for d in ch.iter_mut() {
            if fl > 0 {
                for (k, x) in d[n - fl..].iter_mut().enumerate() {
                    *x *= (fl - k) as f64 / fl as f64;
                }
            }
            let e: f64 = d.iter().map(|x| x * x).sum();
            let g = if e > 0.0 { 1.0 / e.sqrt() } else { 1.0 };
            for x in d.iter_mut() {
                *x *= g;
            }
        }
        ch
    }

    /// The body IR divided by `spec().trim`, with its spectra precomputed
    /// for convolution.
    pub fn impulse_response(self, rng: &mut Rng) -> StereoIr {
        let [mut l, mut r] = self.taps(rng);
        let k = 1.0 / self.spec().trim;
        for x in l.iter_mut().chain(r.iter_mut()) {
            *x *= k;
        }
        StereoIr::new(&l, &r)
    }
}

/// Body transfer magnitudes, dB, 1/12 octave from 80 Hz: guitar, harp,
/// violin. Measured (see the module docs); do not edit by hand.
pub static BODY_CURVES: [[f64; 86]; 3] = [
    // Guitar
    [
        -36.8, -36.8, -36.8, -31.3, -25.8, -20.2, -19.1, -18.0, -17.4, -18.5, -19.9, -20.7, -18.9, -12.9, -6.6, -1.4,
        -0.2, 0.0, -1.2, -4.9, -10.3, -15.3, -16.2, -14.6, -13.1, -13.5, -11.7, -12.1, -13.1, -16.9, -18.6, -20.4, -22.0,
        -23.1, -25.1, -24.8, -26.4, -24.1, -24.1, -22.3, -22.4, -22.1, -20.7, -19.4, -18.1, -20.1, -23.6, -25.7, -26.8,
        -25.1, -27.3, -28.1, -30.9, -31.1, -30.9, -30.2, -29.4, -29.0, -28.6, -29.9, -31.9, -35.3, -37.9, -40.4, -41.8,
        -42.6, -42.7, -42.4, -42.6, -42.9, -43.3, -43.2, -42.7, -41.8, -40.8, -39.6, -38.7, -37.9, -37.7, -37.6, -37.6,
        -37.8, -38.5, -39.0, -39.1, -38.8,
    ],
    // Harp
    [
        -35.3, -34.2, -32.0, -28.6, -25.1, -21.3, -18.6, -17.1, -17.0, -17.3, -17.5, -16.6, -14.3, -10.6, -6.5, -2.7,
        -0.3, 0.0, -1.8, -4.8, -8.0, -10.7, -12.4, -13.0, -12.3, -11.4, -11.1, -11.9, -12.9, -14.7, -16.7, -18.7, -20.3,
        -21.6, -22.8, -23.4, -23.8, -23.5, -23.3, -22.8, -22.5, -21.9, -21.4, -21.2, -22.0, -23.5, -25.6, -27.6, -29.6,
        -31.1, -32.7, -34.1, -35.9, -37.0, -37.9, -38.1, -38.2, -38.6, -39.5, -41.3, -43.6, -46.6, -49.5, -52.2, -54.3,
        -55.8, -56.8, -57.6, -58.3, -59.0, -59.6, -60.1, -60.2, -60.1, -59.8, -59.4, -59.1, -58.9, -58.9, -59.0, -59.3,
        -59.9, -60.4, -60.9, -61.4, -61.7,
    ],
    // Violin
    [
        -24.2, -23.5, -22.8, -22.2, -21.5, -20.8, -20.2, -19.5, -18.8, -18.2, -17.5, -16.8, -16.2, -15.5, -14.8, -14.2,
        -13.5, -12.8, -12.2, -11.5, -11.0, -8.2, -5.5, -3.8, -3.2, -3.5, -4.0, -4.5, -4.9, -5.1, -6.1, -7.6, -9.0, -9.4,
        -9.3, -10.0, -12.0, -14.0, -12.6, -10.2, -8.0, -8.3, -9.6, -10.6, -9.8, -7.9, -9.0, -12.0, -14.9, -14.7, -14.4,
        -14.0, -13.6, -10.1, -6.7, -1.9, 0.0, -0.1, -2.0, -5.9, -8.1, -10.1, -9.5, -9.8, -9.2, -9.7, -9.3, -9.8, -10.1,
        -11.3, -14.1, -18.0, -21.3, -22.8, -23.5, -24.9, -27.0, -28.6, -30.6, -32.6, -35.5, -37.6, -39.0, -40.8, -43.6,
        -46.5,
    ],
];

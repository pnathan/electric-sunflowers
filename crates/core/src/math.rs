//! Scalar math: clamp, lerp, smoothstep, mtof (exp2), dB and linear gain conversion, one-pole coefficients from a time constant.
//!
//! All functions are `f64`, total over their inputs (no panics), and use the
//! plain IEEE operations of `std` (`exp2`, `exp`, `log10`, `round`).

use std::f64::consts::{LN_10, TAU};

/// Clamps `x` to `[lo, hi]`. Unlike `f64::clamp` it does not panic when
/// `lo > hi` (the result is then `hi`). A NaN `x` gives `lo`.
#[inline]
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.max(lo).min(hi)
}

/// Linear interpolation `a + (b - a) t`; `t` is not clamped.
#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Hermite smoothstep: `t = clamp((x - e0) / (e1 - e0), 0, 1)`, result
/// `t^2 (3 - 2t)`. Zero slope at both edges. With `e0 == e1` it is a step
/// (0 below `e0`, 1 at and above).
#[inline]
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    if e1 == e0 {
        return if x >= e0 { 1.0 } else { 0.0 };
    }
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// MIDI note number to frequency in Hz, 12-tone equal temperament with
/// A4 = MIDI 69 = 440 Hz: `440 * 2^((m - 69) / 12)`. Fractional `m` gives
/// cents (0.01 per cent).
#[inline]
pub fn mtof(m: f64) -> f64 {
    440.0 * ((m - 69.0) / 12.0).exp2()
}

/// Decibels to linear amplitude gain: `10^(db / 20)`.
#[inline]
pub fn db_to_gain(db: f64) -> f64 {
    (db * (LN_10 / 20.0)).exp()
}

/// Lowest value `gain_to_db` returns: -200 dB (amplitude 1e-10).
pub const DB_FLOOR: f64 = -200.0;

/// Linear amplitude gain to decibels: `20 log10(|g|)`, floored at
/// `DB_FLOOR` (-200 dB). Zero, NaN and tiny gains give the floor.
#[inline]
pub fn gain_to_db(g: f64) -> f64 {
    (20.0 * g.abs().log10()).max(DB_FLOOR)
}

/// One-pole low-pass coefficient for a cutoff `fc` Hz at rate `fs`:
/// `a = 1 - exp(-2 pi fc / fs)`, used as `y += a (x - y)`. This is the
/// impulse-invariant mapping of the analogue pole at `-2 pi fc`, so the
/// -3 dB point is at `fc` for `fc << fs`.
#[inline]
pub fn one_pole_coeff_hz(fc: f64, fs: f64) -> f64 {
    1.0 - (-TAU * fc / fs).exp()
}

/// One-pole smoothing coefficient for a time constant `tau` seconds at rate
/// `fs`: `a = 1 - exp(-1 / (tau fs))`, used as `y += a (x - y)`. The step
/// response reaches `1 - 1/e` (63.2 %) after `tau` seconds. `tau <= 0`
/// gives `a = 1` (no smoothing).
#[inline]
pub fn one_pole_coeff_tau(tau: f64, fs: f64) -> f64 {
    if tau <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (tau * fs)).exp()
}

/// Shifts the MIDI pitch `m` by the fewest whole octaves that bring it into
/// `[lo, hi]`; a pitch already in range is returned unchanged. When the
/// range is narrower than an octave and no transposition lands in it, the
/// one nearest the range centre is taken (outside the range by less than
/// an octave). Non-finite input is returned unchanged.
#[inline]
pub fn fold_octave(m: f64, lo: f64, hi: f64) -> f64 {
    if !(m.is_finite() && lo.is_finite() && hi.is_finite()) || (lo <= m && m <= hi) {
        return m;
    }
    let (near, far) = if m < lo {
        // Lowest transposition at or above lo; the one below it is < lo.
        let up = m + 12.0 * ((lo - m) / 12.0).ceil();
        (up, up - 12.0)
    } else {
        // Highest transposition at or below hi; the one above it is > hi.
        let down = m - 12.0 * ((m - hi) / 12.0).ceil();
        (down, down + 12.0)
    };
    if lo <= near && near <= hi {
        return near;
    }
    let mid = 0.5 * (lo + hi);
    if (far - mid).abs() < (near - mid).abs() {
        far
    } else {
        near
    }
}

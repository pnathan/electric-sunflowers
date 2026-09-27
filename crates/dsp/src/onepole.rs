//! One-pole filters with `f64` state, and the zero-phase smoother for control tracks.
//!
//! - `OnePole`: the exponential smoother z += a (x - z), a = 1 - exp(-1 / (tau fs)),
//!   the impulse-invariant map of an RC low-pass with time constant tau.
//!   A unit step reaches 1 - 1/e (63.2%) after tau seconds. `from_hz` uses
//!   tau = 1 / (2 pi fc). `tick_hp` returns x minus the low-pass output.
//! - `DcBlocker`: y = x - x1 + r y1 (J. O. Smith, "Introduction to Digital
//!   Filters", the DC blocker), a zero at DC and a pole at r; r = exp(-2 pi fc / fs).
//! - `zero_phase_smooth`: forward then backward pass of the same one-pole
//!   (filtfilt, Gustafsson 1996 without the edge padding). Zero phase, squared
//!   magnitude. Each pass starts its state at the first sample it reads, so a
//!   constant track passes unchanged. For precomputed control tracks only.

use std::f64::consts::PI;

/// Coefficient a = 1 - exp(-1 / (tau fs)); tau <= 0 or non-finite gives a = 1 (no smoothing).
pub fn tau_coef(tau: f64, fs: f64) -> f64 {
    let n = tau * fs;
    if n.is_finite() && n > 0.0 {
        1.0 - (-1.0 / n).exp()
    } else {
        1.0
    }
}

/// One-pole low-pass z += a (x - z).
#[derive(Clone, Copy, Debug, Default)]
pub struct OnePole {
    pub a: f64,
    pub z: f64,
}

impl OnePole {
    /// Low-pass with its -3 dB point near fc (exact for fc << fs).
    pub fn from_hz(fc: f64, fs: f64) -> Self {
        let tau = if fc > 0.0 { 1.0 / (2.0 * PI * fc) } else { 0.0 };
        OnePole { a: tau_coef(tau, fs), z: 0.0 }
    }

    /// Low-pass with time constant `tau` seconds.
    pub fn from_tau(tau: f64, fs: f64) -> Self {
        OnePole { a: tau_coef(tau, fs), z: 0.0 }
    }

    /// Low-pass output.
    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        self.z += self.a * (x - self.z);
        self.z
    }

    /// High-pass output: x minus the low-pass output.
    #[inline(always)]
    pub fn tick_hp(&mut self, x: f64) -> f64 {
        x - self.tick(x)
    }

    pub fn reset(&mut self) {
        self.z = 0.0;
    }

    pub fn flush_denormals(&mut self) {
        if self.z.abs() < crate::biquad::DENORMAL_FLOOR {
            self.z = 0.0;
        }
    }
}

/// DC blocker y = x - x1 + r y1.
#[derive(Clone, Copy, Debug, Default)]
pub struct DcBlocker {
    pub r: f64,
    pub x1: f64,
    pub y1: f64,
}

impl DcBlocker {
    /// Pole at `r` (0 <= r < 1; 0.995 at 44.1 kHz puts the corner near 35 Hz).
    pub fn new(r: f64) -> Self {
        DcBlocker { r, x1: 0.0, y1: 0.0 }
    }

    /// Pole at exp(-2 pi fc / fs).
    pub fn from_hz(fc: f64, fs: f64) -> Self {
        Self::new((-2.0 * PI * fc.max(0.0) / fs).exp())
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for v in buf.iter_mut() {
            *v = self.tick(*v as f64) as f32;
        }
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// Filtfilt with the one-pole z += a (x - z): forward pass seeded with the
/// first sample, then backward pass seeded with the last forward output.
pub fn zero_phase_smooth(buf: &mut [f32], a: f64) {
    let (Some(&first), Some(_)) = (buf.first(), buf.last()) else { return };
    let mut y = first as f64;
    for v in buf.iter_mut() {
        y += a * (*v as f64 - y);
        *v = y as f32;
    }
    let mut y = buf[buf.len() - 1] as f64;
    for v in buf.iter_mut().rev() {
        y += a * (*v as f64 - y);
        *v = y as f32;
    }
}

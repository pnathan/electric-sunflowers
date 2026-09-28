//! Two-pole resonator with unity DC gain (Klatt 1980, "Software for a
//! cascade/parallel formant synthesizer", JASA 67(3)).
//!
//! y[n] = a x[n] + b y[n-1] + c y[n-2], with
//!   c = -exp(-2 pi bw / fs), b = 2 exp(-pi bw / fs) cos(2 pi f / fs), a = 1 - b - c.
//! The poles sit at radius exp(-pi bw / fs) and angle 2 pi f / fs, so the
//! resonance is at about f with -3 dB bandwidth about bw; a = 1 - b - c sets
//! H(1) = 1.
//!
//! Ramps. `set_ramped` moves (a, b, c) linearly to a target over n samples.
//! The recursion is stable exactly when (b, c) lies in the open triangle
//! |c| < 1, |b| < 1 - c (the Schur-Cohn conditions for z^2 - b z - c). A
//! triangle is convex, so every point on the segment between two stable
//! designs is stable, and a per-sample linear ramp between them never leaves
//! the stable region. a = 1 - b - c is linear in (b, c), so it ramps with
//! them and the DC gain stays at 1 along the path. (Frozen-time stability
//! only; a slow ramp between stable points is not a source of growth in
//! practice, and the unit test checks boundedness.)

use std::f64::consts::PI;

/// Klatt coefficients (a, b, c) for centre `f` Hz and bandwidth `bw` Hz at `fs`.
pub fn coeffs(f: f64, bw: f64, fs: f64) -> (f64, f64, f64) {
    let bw = if bw.is_finite() { bw.max(0.0) } else { 0.0 };
    let f = if f.is_finite() { f } else { 0.0 };
    let r = (-PI * bw / fs).exp();
    let c = -r * r;
    let b = 2.0 * r * (2.0 * PI * f / fs).cos();
    (1.0 - b - c, b, c)
}

/// Klatt resonator with an optional linear coefficient ramp in progress.
#[derive(Clone, Copy, Debug, Default)]
pub struct Resonator {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub y1: f64,
    pub y2: f64,
    /// Per-sample increments of (a, b, c) while a ramp runs.
    step: (f64, f64, f64),
    /// Ramp target, taken exactly when the ramp ends.
    target: (f64, f64, f64),
    /// Samples left in the ramp.
    left: u32,
}

impl Resonator {
    pub fn new(f: f64, bw: f64, fs: f64) -> Self {
        let mut r = Resonator::default();
        r.set(f, bw, fs);
        r
    }

    /// Design and take the coefficients now; cancels any ramp. State kept.
    pub fn set(&mut self, f: f64, bw: f64, fs: f64) {
        self.set_coeffs(coeffs(f, bw, fs));
    }

    /// Take (a, b, c) now; cancels any ramp. State kept.
    pub fn set_coeffs(&mut self, (a, b, c): (f64, f64, f64)) {
        self.a = a;
        self.b = b;
        self.c = c;
        self.left = 0;
    }

    /// Ramp (a, b, c) linearly to `target` over `n` samples; the n-th tick
    /// after this call uses `target` exactly. n = 0 jumps.
    pub fn set_ramped(&mut self, target: (f64, f64, f64), n: u32) {
        if n == 0 {
            self.set_coeffs(target);
            return;
        }
        let k = 1.0 / n as f64;
        self.step = ((target.0 - self.a) * k, (target.1 - self.b) * k, (target.2 - self.c) * k);
        self.target = target;
        self.left = n;
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                (self.a, self.b, self.c) = self.target;
            } else {
                self.a += self.step.0;
                self.b += self.step.1;
                self.c += self.step.2;
            }
        }
        let y = self.a * x + self.b * self.y1 + self.c * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    pub fn flush_denormals(&mut self) {
        if self.y1.abs() < crate::biquad::DENORMAL_FLOOR {
            self.y1 = 0.0;
        }
        if self.y2.abs() < crate::biquad::DENORMAL_FLOOR {
            self.y2 = 0.0;
        }
    }
}

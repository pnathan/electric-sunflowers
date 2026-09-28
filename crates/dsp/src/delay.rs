//! Delay lines: an `f32` ring buffer with integer, linear and 4-point Lagrange
//! fractional reads; the Schroeder allpass; the first-order Thiran allpass
//! used for fractional loop delay; and the one-pole phase-delay formula used
//! to tune waveguide loops.
//!
//! Read convention for `DelayLine`: push first, then read. After `push(x[n])`,
//! `read_int(d)` returns `x[n - d]`; `read_int(0)` is the sample just pushed.
//! An integer delay beyond the capacity wraps to newer samples (no panic,
//! wrong value); size the line with `new(max_delay)` for the largest delay
//! the caller reads. The fractional reads clamp their delay to
//! `[lo, capacity - 3]` first (NaN reads as `lo`), so every tap they touch is
//! inside the buffer and the float-to-index cast cannot overflow.

/// Ring buffer of `f32` samples with power-of-two capacity, so index wrap is a
/// mask instead of a branch or a modulo.
#[derive(Clone, Debug)]
pub struct DelayLine {
    buf: Vec<f32>,
    mask: usize,
    /// Next write position.
    w: usize,
}

impl DelayLine {
    /// A line that can serve every read in this module for delays up to
    /// `max_delay` samples, including `read_lagrange3(max_delay)`, which
    /// needs taps out to `floor(d) + 2`.
    pub fn new(max_delay: usize) -> Self {
        let len = max_delay.saturating_add(3).next_power_of_two();
        DelayLine { buf: vec![0.0; len], mask: len - 1, w: 0 }
    }

    /// Buffer length (a power of two). The largest meaningful integer delay
    /// is `capacity() - 1`.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Clamp a fractional delay to `[lo, capacity - 3]`; NaN gives `lo`.
    /// `capacity - 3` keeps `floor(d) + 2`, the farthest Lagrange tap, below
    /// the capacity. `max` then `min` is used instead of `f64::clamp` so NaN
    /// maps to `lo` (`f64::max` returns the non-NaN operand).
    #[inline(always)]
    fn clamp_delay(&self, d: f64, lo: f64) -> f64 {
        let hi = (self.buf.len() - 3) as f64;
        d.max(lo).min(hi)
    }

    /// Write one sample and advance.
    #[inline]
    pub fn push(&mut self, x: f32) {
        self.buf[self.w] = x;
        self.w = (self.w + 1) & self.mask;
    }

    /// `x[n - d]`, where `x[n]` is the last sample pushed.
    #[inline]
    pub fn read_int(&self, d: usize) -> f32 {
        self.buf[self.w.wrapping_sub(1).wrapping_sub(d) & self.mask]
    }

    /// Linear interpolation between `x[n - floor(d)]` and `x[n - floor(d) - 1]`.
    /// `d` is clamped to `[0, capacity - 3]`, NaN to 0. Its magnitude response is `|cos(w/2)|` at fraction
    /// 0.5, so it low-passes by an amount that depends on the fraction; do
    /// not use it inside a loop whose delay is modulated.
    #[inline]
    pub fn read_linear(&self, d: f64) -> f64 {
        let d = self.clamp_delay(d, 0.0);
        let i = d as usize; // floor: d is finite and >= 0
        let f = d - i as f64;
        let a = self.read_int(i) as f64;
        let b = self.read_int(i + 1) as f64;
        a + f * (b - a)
    }

    /// Third-order (4-point) Lagrange interpolation (Laakso, Valimaki,
    /// Karjalainen, Laine 1996, "Splitting the unit delay", section 3.3).
    /// Taps at integer delays `i-1, i, i+1, i+2` with `i = floor(d)`, so the
    /// delay relative to the first tap is `D = 1 + frac` in [1, 2), the
    /// centred range with the smallest error. `d` is clamped to
    /// `[1, capacity - 3]` (a smaller `d` would read a future sample; NaN
    /// gives 1). Maximally flat at DC;
    /// at 0.25 fs and fraction 0.5 the gain is 0.884 against 0.707 for linear.
    #[inline]
    pub fn read_lagrange3(&self, d: f64) -> f64 {
        let d = self.clamp_delay(d, 1.0);
        let i = d as usize; // floor: d is finite and >= 1
        let f = d - i as f64;
        // Lagrange basis for D = 1 + f over taps at delays 0, 1, 2, 3.
        let fm1 = f - 1.0;
        let fm2 = f - 2.0;
        let fp1 = f + 1.0;
        let h0 = -f * fm1 * fm2 * (1.0 / 6.0);
        let h1 = fp1 * fm1 * fm2 * 0.5;
        let h2 = -fp1 * f * fm2 * 0.5;
        let h3 = fp1 * f * fm1 * (1.0 / 6.0);
        h0 * self.read_int(i - 1) as f64
            + h1 * self.read_int(i) as f64
            + h2 * self.read_int(i + 1) as f64
            + h3 * self.read_int(i + 2) as f64
    }

    /// Zero the contents and the write position.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
    }
}

/// Schroeder allpass (Schroeder 1962, "Natural sounding artificial
/// reverberation"), single-delay canonical form:
/// `v[n] = x[n] + g v[n-M]`, `y[n] = v[n-M] - g v[n]`,
/// so `H(z) = (z^-M - g) / (1 - g z^-M)`, unit magnitude at all frequencies.
/// `delay` (M, >= 1) sets the echo spacing; `g` in (-1, 1) the decay per echo.
/// Same sign convention as the reverb's series allpasses (g 0.58-0.65).
#[derive(Clone, Debug)]
pub struct SchroederAllpass {
    line: DelayLine,
    delay: usize,
    g: f32,
}

impl SchroederAllpass {
    /// `delay` is clamped to >= 1; `g` to [-0.999, 0.999] so the loop is stable.
    pub fn new(delay: usize, g: f32) -> Self {
        let delay = delay.max(1);
        SchroederAllpass { line: DelayLine::new(delay), delay, g: g.clamp(-0.999, 0.999) }
    }

    #[inline]
    pub fn tick(&mut self, x: f32) -> f32 {
        let vd = self.line.read_int(self.delay - 1); // v[n-M], read before the push
        let v = x + self.g * vd;
        self.line.push(v);
        vd - self.g * v
    }

    pub fn process(&mut self, buf: &mut [f32]) {
        for s in buf.iter_mut() {
            *s = self.tick(*s);
        }
    }

    pub fn reset(&mut self) {
        self.line.clear();
    }
}

/// First-order Thiran allpass for fractional delay (Thiran 1971; Laakso,
/// Valimaki, Karjalainen, Laine 1996, section 3.4):
/// `H(z) = (c + z^-1) / (1 + c z^-1)`, `c = (1 - delta) / (1 + delta)`.
/// Its phase delay is `delta` at DC and falls slowly with frequency. Keep
/// delta in [0.5, 1.5): then |c| <= 1/3, the pole stays well inside the unit
/// circle and the delay error stays small up to about fs/4. A waveguide loop
/// splits its period into an integer line plus this delta.
/// State in `f64`: this filter sits inside recirculating loops.
#[derive(Clone, Debug, Default)]
pub struct Thiran1 {
    c: f64,
    x1: f64,
    y1: f64,
}

impl Thiran1 {
    pub fn new(delta: f64) -> Self {
        let mut t = Thiran1::default();
        t.set_delay(delta);
        t
    }

    /// Set the fractional delay. Clamped to [0.5, 1.5]; NaN maps to 1.
    #[inline]
    pub fn set_delay(&mut self, delta: f64) {
        let d = if delta.is_nan() { 1.0 } else { delta.clamp(0.5, 1.5) };
        self.c = (1.0 - d) / (1.0 + d);
    }

    pub fn coeff(&self) -> f64 {
        self.c
    }

    /// `y[n] = c x[n] + x[n-1] - c y[n-1]`.
    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        let y = self.c * x + self.x1 - self.c * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    /// Phase delay in samples at `w` rad/sample: `-arg H(e^jw) / w`, with
    /// `arg H = atan2(-sin w, c + cos w) - atan2(-c sin w, 1 + c cos w)`.
    /// At w = 0 returns the limit `(1 - c) / (1 + c)`.
    pub fn phase_delay(&self, w: f64) -> f64 {
        let c = self.c;
        if w.abs() < 1e-12 {
            return (1.0 - c) / (1.0 + c);
        }
        let (s, co) = w.sin_cos();
        let arg = (-s).atan2(c + co) - (-c * s).atan2(1.0 + c * co);
        -arg / w
    }
}

/// Phase delay in samples at `w` rad/sample of the one-pole loop filter
/// `H(z) = (1 - a) / (1 - a z^-1)` used in the plucked and bowed strings:
/// `tau(w) = atan2(a sin w, 1 - a cos w) / w`
/// (the formula in the plucked-string loop tuning, where the integer line
/// length is `floor(SR/f0 - tau - 0.5)` and the Thiran delta takes the rest).
/// At w = 0 returns the limit `a / (1 - a)`.
pub fn one_pole_phase_delay(a: f64, w: f64) -> f64 {
    if w.abs() < 1e-12 {
        return a / (1.0 - a);
    }
    let (s, c) = w.sin_cos();
    (a * s).atan2(1.0 - a * c) / w
}

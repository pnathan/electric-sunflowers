//! Biquad: second-order IIR section with `f64` coefficients and state.
//!
//! Designs: Robert Bristow-Johnson, "Cookbook formulae for audio EQ biquad
//! filter coefficients" (the RBJ Audio EQ Cookbook). With w0 = 2 pi f / fs and
//! alpha = sin(w0) / (2 Q):
//! - low-pass, high-pass: the cookbook's H(s) = 1 / (s^2 + s/Q + 1) and its
//!   high-pass mirror; -3.01 dB at f when Q = 1/sqrt(2).
//! - band-pass: the constant 0 dB peak gain form (b0 = alpha, b2 = -alpha).
//! - peaking: A = 10^(dB/40); gain dB at f, bandwidth set by Q.
//! - shelves: the Q form, with 2 sqrt(A) alpha in the numerator and
//!   denominator; gain dB at DC (low shelf) or Nyquist (high shelf), half the
//!   gain in dB at f.
//!
//! All coefficients are normalised by a0. f is clamped to (0, 0.45 fs] and Q
//! to a small positive floor, so no design returns NaN.
//!
//! Topology: transposed direct form II (Jackson, "Digital Filters and Signal
//! Processing"; J. O. Smith, "Introduction to Digital Filters"). Two state
//! words per section; with `f64` state the round-off noise floor is near
//! -300 dBFS, far below the `f32` buffers it runs on (design section 4).

use std::f64::consts::PI;

/// State magnitude below which `flush_denormals` zeroes a state word.
pub const DENORMAL_FLOOR: f64 = 1e-25;

/// Normalised biquad coefficients: H(z) = (b0 + b1 z^-1 + b2 z^-2) / (1 + a1 z^-1 + a2 z^-2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

/// Cookbook intermediates for one design point.
struct Prewarp {
    cos_w: f64,
    alpha: f64,
}

fn prewarp(fs: f64, f: f64, q: f64) -> Prewarp {
    let fs = if fs.is_finite() && fs > 0.0 {
        fs
    } else {
        44_100.0
    };
    let f = if f.is_finite() {
        f.clamp(1e-3, 0.45 * fs)
    } else {
        0.45 * fs
    };
    let q = if q.is_finite() {
        q.max(1e-6)
    } else {
        std::f64::consts::FRAC_1_SQRT_2
    };
    let w = 2.0 * PI * f / fs;
    Prewarp {
        cos_w: w.cos(),
        alpha: w.sin() / (2.0 * q),
    }
}

/// A = 10^(dB/40), the cookbook's amplitude for peaking and shelving designs.
fn shelf_amp(db: f64) -> f64 {
    let db = if db.is_finite() { db } else { 0.0 };
    10f64.powf(db / 40.0)
}

impl BiquadCoeffs {
    /// Pass-through: b0 = 1, everything else 0.
    pub const IDENTITY: BiquadCoeffs = BiquadCoeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn normalise(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        let r = 1.0 / a0;
        BiquadCoeffs {
            b0: b0 * r,
            b1: b1 * r,
            b2: b2 * r,
            a1: a1 * r,
            a2: a2 * r,
        }
    }

    /// Second-order low-pass, corner f, resonance Q.
    pub fn lowpass(fs: f64, f: f64, q: f64) -> Self {
        let Prewarp {
            cos_w: c,
            alpha: al,
        } = prewarp(fs, f, q);
        let b = (1.0 - c) / 2.0;
        Self::normalise(b, 1.0 - c, b, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    /// Second-order high-pass, corner f, resonance Q.
    pub fn highpass(fs: f64, f: f64, q: f64) -> Self {
        let Prewarp {
            cos_w: c,
            alpha: al,
        } = prewarp(fs, f, q);
        let b = (1.0 + c) / 2.0;
        Self::normalise(b, -(1.0 + c), b, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    /// Band-pass with 0 dB gain at the centre f; bandwidth set by Q.
    pub fn bandpass(fs: f64, f: f64, q: f64) -> Self {
        let Prewarp {
            cos_w: c,
            alpha: al,
        } = prewarp(fs, f, q);
        Self::normalise(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al)
    }

    /// Peaking EQ: `db` of gain at f, unity far from f.
    pub fn peaking(fs: f64, f: f64, q: f64, db: f64) -> Self {
        let Prewarp {
            cos_w: c,
            alpha: al,
        } = prewarp(fs, f, q);
        let a = shelf_amp(db);
        Self::normalise(
            1.0 + al * a,
            -2.0 * c,
            1.0 - al * a,
            1.0 + al / a,
            -2.0 * c,
            1.0 - al / a,
        )
    }

    /// Low shelf: `db` of gain at DC, unity at Nyquist, Q form.
    pub fn low_shelf(fs: f64, f: f64, q: f64, db: f64) -> Self {
        Self::shelf(fs, f, q, db, -1.0)
    }

    /// High shelf: `db` of gain at Nyquist, unity at DC, Q form.
    pub fn high_shelf(fs: f64, f: f64, q: f64, db: f64) -> Self {
        Self::shelf(fs, f, q, db, 1.0)
    }

    /// Both cookbook shelves in one form: `sg` = +1 high shelf, -1 low shelf.
    fn shelf(fs: f64, f: f64, q: f64, db: f64, sg: f64) -> Self {
        let Prewarp {
            cos_w: c,
            alpha: al,
        } = prewarp(fs, f, q);
        let a = shelf_amp(db);
        let sq = 2.0 * a.sqrt() * al;
        Self::normalise(
            a * ((a + 1.0) + sg * (a - 1.0) * c + sq),
            -2.0 * sg * a * ((a - 1.0) + sg * (a + 1.0) * c),
            a * ((a + 1.0) + sg * (a - 1.0) * c - sq),
            (a + 1.0) - sg * (a - 1.0) * c + sq,
            2.0 * sg * ((a - 1.0) - sg * (a + 1.0) * c),
            (a + 1.0) - sg * (a - 1.0) * c - sq,
        )
    }

    /// |H(e^{j 2 pi f / fs})| in dB. For tests and plots, not for the audio path.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// The six cookbook responses a channel-strip EQ band may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqKind {
    Lowpass,
    Highpass,
    Bandpass,
    Peaking,
    LowShelf,
    HighShelf,
}

/// One EQ band as data: kind, frequency (Hz), Q, gain (dB; ignored by the
/// low-pass, high-pass and band-pass kinds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EqBand {
    pub kind: EqKind,
    pub f: f64,
    pub q: f64,
    pub db: f64,
}

impl EqBand {
    /// Coefficients of this band at sample rate `fs`.
    pub fn design(&self, fs: f64) -> BiquadCoeffs {
        match self.kind {
            EqKind::Lowpass => BiquadCoeffs::lowpass(fs, self.f, self.q),
            EqKind::Highpass => BiquadCoeffs::highpass(fs, self.f, self.q),
            EqKind::Bandpass => BiquadCoeffs::bandpass(fs, self.f, self.q),
            EqKind::Peaking => BiquadCoeffs::peaking(fs, self.f, self.q, self.db),
            EqKind::LowShelf => BiquadCoeffs::low_shelf(fs, self.f, self.q, self.db),
            EqKind::HighShelf => BiquadCoeffs::high_shelf(fs, self.f, self.q, self.db),
        }
    }
}

/// One biquad section in transposed direct form II:
/// y = b0 x + s1; s1 = b1 x - a1 y + s2; s2 = b2 x - a2 y.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    c: BiquadCoeffs,
    s1: f64,
    s2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Biquad::new(BiquadCoeffs::IDENTITY)
    }
}

impl Biquad {
    /// A section with zero state.
    pub fn new(c: BiquadCoeffs) -> Self {
        Biquad {
            c,
            s1: 0.0,
            s2: 0.0,
        }
    }

    pub fn coeffs(&self) -> BiquadCoeffs {
        self.c
    }

    /// Replace the coefficients and keep the state (TDF-II tolerates slow
    /// coefficient changes better than direct form I).
    pub fn set_coeffs(&mut self, c: BiquadCoeffs) {
        self.c = c;
    }

    /// One sample.
    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        y
    }

    /// Filter a buffer in place, state carried across calls.
    pub fn process(&mut self, buf: &mut [f32]) {
        let BiquadCoeffs { b0, b1, b2, a1, a2 } = self.c;
        let (mut s1, mut s2) = (self.s1, self.s2);
        for v in buf.iter_mut() {
            let x = *v as f64;
            let y = b0 * x + s1;
            s1 = b1 * x - a1 * y + s2;
            s2 = b2 * x - a2 * y;
            *v = y as f32;
        }
        self.s1 = s1;
        self.s2 = s2;
    }

    /// Zero the state.
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// Zero each state word whose magnitude is below `DENORMAL_FLOOR`.
    /// Call at block or phrase boundaries; a guard for targets without FTZ.
    pub fn flush_denormals(&mut self) {
        if self.s1.abs() < DENORMAL_FLOOR {
            self.s1 = 0.0;
        }
        if self.s2.abs() < DENORMAL_FLOOR {
            self.s2 = 0.0;
        }
    }

    /// Magnitude response of the current coefficients in dB.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        self.c.magnitude_db(f, fs)
    }
}

/// The same coefficients on two channels, one state per channel.
#[derive(Clone, Copy, Debug, Default)]
pub struct StereoBiquad {
    pub l: Biquad,
    pub r: Biquad,
}

impl StereoBiquad {
    pub fn new(c: BiquadCoeffs) -> Self {
        StereoBiquad {
            l: Biquad::new(c),
            r: Biquad::new(c),
        }
    }

    pub fn set_coeffs(&mut self, c: BiquadCoeffs) {
        self.l.set_coeffs(c);
        self.r.set_coeffs(c);
    }

    /// Filter both channels in place.
    pub fn process_stereo(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.l.process(l);
        self.r.process(r);
    }

    pub fn reset(&mut self) {
        self.l.reset();
        self.r.reset();
    }

    pub fn flush_denormals(&mut self) {
        self.l.flush_denormals();
        self.r.flush_denormals();
    }
}

/// N sections in series, fixed at compile time so the chain inlines.
#[derive(Clone, Copy, Debug)]
pub struct Cascade<const N: usize> {
    pub stages: [Biquad; N],
}

impl<const N: usize> Cascade<N> {
    pub fn new(c: [BiquadCoeffs; N]) -> Self {
        Cascade {
            stages: c.map(Biquad::new),
        }
    }

    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        self.stages.iter_mut().fold(x, |y, s| s.tick(y))
    }

    /// Filter a buffer in place, one section after another.
    pub fn process(&mut self, buf: &mut [f32]) {
        for s in self.stages.iter_mut() {
            s.process(buf);
        }
    }

    pub fn reset(&mut self) {
        self.stages.iter_mut().for_each(Biquad::reset);
    }

    pub fn flush_denormals(&mut self) {
        self.stages.iter_mut().for_each(Biquad::flush_denormals);
    }

    /// Summed magnitude response of all sections in dB.
    pub fn magnitude_db(&self, f: f64, fs: f64) -> f64 {
        self.stages.iter().map(|s| s.magnitude_db(f, fs)).sum()
    }
}

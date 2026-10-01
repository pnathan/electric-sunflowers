//! Glottal source: the Liljencrants-Fant (LF) flow derivative, read from
//! band-limited single-cycle tables.
//!
//! Model. G. Fant, J. Liljencrants, Q. Lin, "A four-parameter model of
//! glottal flow", STL-QPSR 4/1985; the shape follows from one parameter Rd
//! by G. Fant, "The LF-model revisited", STL-QPSR 2-3/1995:
//!
//! ```text
//! Ra = (-1 + 4.8 Rd) / 100
//! Rk = (22.4 + 11.8 Rd) / 100
//! Rg = Rk / (4 (0.11 Rd / (0.5 + 1.2 Rk) - Ra))
//! tp = 1 / (2 Rg), te = tp (1 + Rk), ta = Ra          (period T0 = 1)
//! open phase    E(t) = E0 exp(alpha t) sin(pi t / tp),            0 <= t <= te
//! return phase  E(t) = -(exp(-eps (t - te)) - exp(-eps (1 - te))) / (eps ta),  te < t <= 1
//! ```
//!
//! E0 sets E(te) = -1. Epsilon solves eps ta = 1 - exp(-eps (1 - te)) by
//! fixed-point iteration; alpha is found by bisection so that the flow
//! returns to zero over the period (zero net flow), using the closed-form
//! integrals of both phases. Rd is Fant's identity
//! Rd = (1 / 0.11) (0.5 + 1.2 Rk) (Rk / (4 Rg) + Ra), inverted above.
//!
//! Tables. For each Rd step (index round(Rd * 40)) a table set is built on
//! first use and kept for the life of the process (`static LF`):
//! - `D`, the flow derivative, RMS-normalised, as a mip set: one 2048-point
//!   single-cycle table per half-octave of f0 from 55 Hz. Level j keeps the
//!   harmonics h <= (SR / 2) / top(j), top(j) = 55 * 2^((j + 1) / 2) Hz, and
//!   has no DC; harmonics are removed with a real FFT (dsp::fft::RealFft).
//!   Playback blends the two levels that bracket f0, linearly by position
//!   within the half-octave, so vibrato across a level edge has no step.
//!   Source: the mip-mapped wavetable method, D. C. Massie, "Wavetable
//!   sampling synthesis", in Kahrs and Brandenburg (eds.), Applications of
//!   Digital Signal Processing to Audio and Acoustics, 1998.
//! - `G`, the flow (running sum of the full-band derivative, scaled to peak
//!   1, floored at 0). One table; it only modulates breath noise.
//!
//! A voice uses two Rd values, lax (Rd + 0.35) and tense (max(0.6, Rd -
//! 0.28)), blended by loudness: w = clamp((av - 0.42) / 0.6, 0, 1). The
//! voice's `SourceTable` interleaves the four values it reads per table
//! index (D lax, D tense, G lax, G tense), so one read is one 16-byte load.
//!
//! `GlottalSource` adds per-period jitter and shimmer (Gaussian, redrawn at
//! each period start) and two cascaded one-pole low-passes for spectral
//! tilt. Parameters that set the sound: Rd, tilt corner (Hz), jitter,
//! shimmer (voice::params), and the loudness blend above.

use std::sync::OnceLock;

use dsp::fft::{RealFft, C32};
use dsp::onepole::OnePole;
use sfcore::random::Rng;
use sfcore::SR_F;

/// Samples per single-cycle table (one period).
pub const TABLE_LEN: usize = 2048;
/// Stored length: one guard sample (equal to sample 0) for interpolation.
const ROW: usize = TABLE_LEN + 1;
/// Rd table steps per unit of Rd: tables exist for Rd = k / 40.
pub const RD_STEPS: f64 = 40.0;
/// Lowest and highest Rd index with a table (Rd 0.3 and 2.7, the range
/// of Fant's regressions). Other Rd values clamp to these.
pub const RD_INDEX_MIN: usize = 12;
pub const RD_INDEX_MAX: usize = 108;
const N_RD: usize = RD_INDEX_MAX + 1;
/// Lower edge of the lowest mip level, Hz.
pub const MIP_BASE_HZ: f64 = 55.0;
/// Mip levels: tops 77.8 Hz .. 2489 Hz; f0 edges 55 .. 1760 Hz.
pub const N_LEVELS: usize = 11;
/// Derivative gain into the tilt filters (sets the voiced level against
/// the noise sources).
const PULSE_GAIN: f64 = 0.176;
/// Gain of the tilted pulse at the source output.
const SOURCE_GAIN: f64 = 1.6;
/// Lax Rd offset and tense Rd offset and floor.
const RD_LAX: f64 = 0.35;
const RD_TENSE: f64 = 0.28;
const RD_TENSE_MIN: f64 = 0.6;

/// LF timing and shape parameters for one Rd, period normalised to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LfParams {
    pub rd: f64,
    pub ra: f64,
    pub rk: f64,
    pub rg: f64,
    /// Time of peak flow.
    pub tp: f64,
    /// Time of the main excitation (derivative minimum).
    pub te: f64,
    /// Return-phase time constant.
    pub ta: f64,
    /// Return-phase decay rate.
    pub eps: f64,
    /// Open-phase growth rate.
    pub alpha: f64,
    /// Open-phase amplitude: E(te) = -1.
    pub e0: f64,
}

impl LfParams {
    /// Parameters from Rd (Fant 1995). Rd is clamped to the table range.
    pub fn from_rd(rd: f64) -> LfParams {
        let lo = RD_INDEX_MIN as f64 / RD_STEPS;
        let hi = RD_INDEX_MAX as f64 / RD_STEPS;
        let rd = if rd.is_finite() {
            rd.clamp(lo, hi)
        } else {
            1.0
        };
        let ra = (-1.0 + 4.8 * rd) / 100.0;
        let rk = (22.4 + 11.8 * rd) / 100.0;
        let rg = rk / (4.0 * (0.11 * rd / (0.5 + 1.2 * rk) - ra));
        let tp = 1.0 / (2.0 * rg);
        let te = tp * (1.0 + rk);
        let ta = ra;
        let mut eps = 1.0 / ta;
        for _ in 0..40 {
            eps = (1.0 - (-eps * (1.0 - te)).exp()) / ta;
        }
        let mut p = LfParams {
            rd,
            ra,
            rk,
            rg,
            tp,
            te,
            ta,
            eps,
            alpha: 0.0,
            e0: 0.0,
        };
        // Bisection on alpha for zero net flow. area() rises through zero
        // on [-10, 80] for every Rd in range; keep the bracket's sign rule
        // so a missing root degrades to the nearer end, not NaN.
        let (mut lo, mut hi) = (-10.0f64, 80.0f64);
        let mut a_lo = p.with_alpha(lo).net_flow();
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            let am = p.with_alpha(mid).net_flow();
            if (am > 0.0) == (a_lo > 0.0) {
                lo = mid;
                a_lo = am;
            } else {
                hi = mid;
            }
        }
        p = p.with_alpha(0.5 * (lo + hi));
        p
    }

    fn with_alpha(mut self, alpha: f64) -> LfParams {
        let wg = std::f64::consts::PI / self.tp;
        self.alpha = alpha;
        self.e0 = -1.0 / ((alpha * self.te).exp() * (wg * self.te).sin());
        self
    }

    /// Fant's identity: Rd from (Ra, Rk, Rg).
    pub fn rd_from_ratios(ra: f64, rk: f64, rg: f64) -> f64 {
        (0.5 + 1.2 * rk) * (rk / (4.0 * rg) + ra) / 0.11
    }

    /// The flow derivative at t in [0, 1].
    pub fn eval(&self, t: f64) -> f64 {
        let wg = std::f64::consts::PI / self.tp;
        if t <= self.te {
            self.e0 * (self.alpha * t).exp() * (wg * t).sin()
        } else {
            -((-self.eps * (t - self.te)).exp() - (-self.eps * (1.0 - self.te)).exp())
                / (self.eps * self.ta)
        }
    }

    /// Integral of the derivative over one period (the flow at t = 1), in
    /// closed form: open phase int e^{a t} sin(w t) dt = e^{a t} (a sin wt -
    /// w cos wt) / (a^2 + w^2); return phase integrated term by term.
    pub fn net_flow(&self) -> f64 {
        let (a, w, te) = (self.alpha, std::f64::consts::PI / self.tp, self.te);
        let open = self.e0 * ((a * te).exp() * (a * (w * te).sin() - w * (w * te).cos()) + w)
            / (a * a + w * w);
        let tr = 1.0 - te;
        let tail = (-self.eps * tr).exp();
        let ret = -((1.0 - tail) / self.eps - tr * tail) / (self.eps * self.ta);
        open + ret
    }
}

/// Upper f0 (Hz) for which mip level `j` is alias-free.
pub fn level_top_hz(j: usize) -> f64 {
    MIP_BASE_HZ * ((j + 1) as f64 * 0.5).exp2()
}

/// Highest harmonic kept in level `j`.
pub fn level_max_harmonic(j: usize) -> usize {
    ((SR_F * 0.5) / level_top_hz(j)).floor() as usize
}

/// Tables for one Rd step.
pub struct LfTable {
    /// `N_LEVELS` rows of `TABLE_LEN + 1` samples: band-limited D per level.
    d: Box<[f32]>,
    /// Flow, `TABLE_LEN + 1` samples.
    g: Box<[f32]>,
}

impl LfTable {
    /// Band-limited derivative of level `j` (`TABLE_LEN + 1` samples; the
    /// last equals the first).
    pub fn level(&self, j: usize) -> &[f32] {
        let j = j.min(N_LEVELS - 1);
        &self.d[j * ROW..(j + 1) * ROW]
    }

    /// Normalised flow (`TABLE_LEN + 1` samples).
    pub fn flow(&self) -> &[f32] {
        &self.g
    }

    fn build(rd: f64) -> LfTable {
        let p = LfParams::from_rd(rd);
        let mut full = vec![0.0f64; TABLE_LEN];
        for (i, v) in full.iter_mut().enumerate() {
            *v = p.eval(i as f64 / TABLE_LEN as f64);
        }
        let rms = (full.iter().map(|x| x * x).sum::<f64>() / TABLE_LEN as f64)
            .sqrt()
            .max(1e-12);

        // Flow: running sum of the full-band derivative, peak 1, floor 0.
        let mut g = vec![0.0f32; ROW];
        let mut s = 0.0f64;
        let mut peak = 1e-9f64;
        let mut acc = vec![0.0f64; ROW];
        for i in 0..ROW {
            s += full[i % TABLE_LEN];
            acc[i] = s;
            peak = peak.max(s);
        }
        for i in 0..ROW {
            g[i] = (acc[i] / peak).max(0.0) as f32;
        }
        g[TABLE_LEN] = g[0];

        // Band-limited derivative per level.
        let fft = RealFft::new(TABLE_LEN);
        let mut scratch = fft.make_scratch();
        let mut spec = vec![C32::default(); fft.spectrum_len()];
        let mut work = vec![0.0f32; TABLE_LEN];
        for (w, x) in work.iter_mut().zip(full.iter()) {
            *w = (x / rms) as f32;
        }
        // Buffers are sized from the plan, so the transforms cannot fail.
        let _ = fft.forward(&mut work, &mut spec, &mut scratch);
        let mut d = vec![0.0f32; N_LEVELS * ROW];
        let mut bins = vec![C32::default(); spec.len()];
        for j in 0..N_LEVELS {
            let hmax = level_max_harmonic(j);
            bins.copy_from_slice(&spec);
            bins[0] = C32::default();
            for b in bins.iter_mut().skip(hmax + 1) {
                *b = C32::default();
            }
            let row = &mut d[j * ROW..(j + 1) * ROW];
            let _ = fft.inverse(&mut bins, &mut row[..TABLE_LEN], &mut scratch);
            row[TABLE_LEN] = row[0];
        }
        LfTable {
            d: d.into_boxed_slice(),
            g: g.into_boxed_slice(),
        }
    }
}

static LF: [OnceLock<LfTable>; N_RD] = [const { OnceLock::new() }; N_RD];

/// Table index of `rd`: round(Rd * 40), clamped to the table range.
pub fn rd_index(rd: f64) -> usize {
    let k = if rd.is_finite() {
        (rd * RD_STEPS).round()
    } else {
        40.0
    };
    (k.max(RD_INDEX_MIN as f64) as usize).min(RD_INDEX_MAX)
}

/// The tables for `rd` (rounded to the nearest 1/40), built on first use.
pub fn lf_table(rd: f64) -> &'static LfTable {
    let k = rd_index(rd);
    LF[k].get_or_init(|| LfTable::build(k as f64 / RD_STEPS))
}

/// Source level trim for `rd`. The stem is normalised to a target loudness,
/// so a laxer source (weaker output) would raise the noise consonants by
/// the same amount; the trim keeps the voiced-to-noise balance the presets
/// near Rd 1.15 have. Measured on the baritone: Rd 1.7 needs x1.45.
fn lax_trim(rd: f64) -> f64 {
    1.0 + (0.45 / 0.55) * (rd - 1.15).max(0.0)
}

/// One mip level of a voice's interleaved table: entry i is
/// [D lax, D tense, G lax, G tense] at table index i.
type Row = [[f32; 4]; ROW];

/// A voice's lax and tense tables, interleaved per index, one `Row` per
/// mip level.
pub struct SourceTable {
    data: Box<[Row]>,
}

impl SourceTable {
    /// Interleaves the lax (Rd + 0.35) and tense (max(0.6, Rd - 0.28))
    /// tables of `rd`.
    pub fn new(rd: f64) -> SourceTable {
        let lax = lf_table(rd + RD_LAX);
        let tense = lf_table((rd - RD_TENSE).max(RD_TENSE_MIN));
        let mut data = vec![[[0.0f32; 4]; ROW]; N_LEVELS];
        for (j, row) in data.iter_mut().enumerate() {
            let (dl, dt) = (lax.level(j), tense.level(j));
            for (i, e) in row.iter_mut().enumerate() {
                *e = [dl[i], dt[i], lax.g[i], tense.g[i]];
            }
        }
        SourceTable {
            data: data.into_boxed_slice(),
        }
    }
}

/// f0 edges of the mip blend: level k and k + 1 are blended for f0 in
/// [edge k, edge k + 1).
fn mip_edges() -> [f64; N_LEVELS] {
    let mut e = [0.0; N_LEVELS];
    for (k, v) in e.iter_mut().enumerate() {
        *v = MIP_BASE_HZ * (k as f64 * 0.5).exp2();
    }
    e
}

/// Per-sample state of a `GlottalSource`, copied into locals for a block.
#[derive(Clone, Copy, Debug)]
struct SourceState {
    /// Phase in periods, [0, 1).
    phase: f64,
    /// Relative period perturbation of the current period.
    jit: f64,
    /// Amplitude factor of the current period.
    shim: f64,
    /// Lower mip level of the current blend, 0..=N_LEVELS - 2.
    level: usize,
    /// Lower edge and inverse width of that level's f0 band.
    band: (f64, f64),
    /// f0 range [lo, hi) over which `level` stays selected: the band, open
    /// below for level 0 and above for the top pair, so an f0 outside the
    /// mip range does not reselect on every sample.
    valid: (f64, f64),
    /// Tilt one-pole states.
    z: [f64; 2],
}

/// LF glottal source with jitter, shimmer and spectral tilt.
pub struct GlottalSource {
    table: SourceTable,
    edges: [f64; N_LEVELS],
    inv_width: [f64; N_LEVELS],
    jitter: f64,
    shimmer: f64,
    /// Tilt one-pole coefficient (both sections).
    tilt: f64,
    /// Level trim for a lax Rd (see `lax_trim`).
    trim: f64,
    st: SourceState,
}

impl GlottalSource {
    /// Source for shape `rd`, tilt corner `tilt_hz` (both one-poles),
    /// jitter and shimmer as relative standard deviations.
    pub fn new(rd: f64, tilt_hz: f64, jitter: f64, shimmer: f64) -> GlottalSource {
        let edges = mip_edges();
        let mut inv_width = [0.0; N_LEVELS];
        for k in 0..N_LEVELS - 1 {
            inv_width[k] = 1.0 / (edges[k + 1] - edges[k]);
        }
        GlottalSource {
            table: SourceTable::new(rd),
            edges,
            inv_width,
            jitter: jitter.max(0.0),
            shimmer: shimmer.max(0.0),
            tilt: OnePole::from_hz(tilt_hz.max(1.0), SR_F).a,
            trim: lax_trim(rd),
            st: SourceState {
                phase: 0.0,
                jit: 0.0,
                shim: 1.0,
                level: 0,
                band: (edges[0], inv_width[0]),
                valid: (f64::NEG_INFINITY, edges[1]),
                z: [0.0; 2],
            },
        }
    }

    /// Phase to 0, no perturbation, tilt state cleared.
    pub fn reset(&mut self) {
        self.st.phase = 0.0;
        self.st.jit = 0.0;
        self.st.shim = 1.0;
        self.st.z = [0.0; 2];
    }

    /// The mip level pair for `f0`, its band and its valid f0 range: f0
    /// in [edge k, edge k + 1) blends levels k and k + 1; below the first
    /// edge level 0 (blend clamps to 0), above the last level N_LEVELS - 1
    /// (blend clamps to 1).
    #[cold]
    #[inline(never)]
    fn select_level(
        edges: &[f64; N_LEVELS],
        inv_width: &[f64; N_LEVELS],
        f0: f64,
    ) -> (usize, (f64, f64), (f64, f64)) {
        let mut k = 0;
        while k < N_LEVELS - 2 && f0 >= edges[k + 1] {
            k += 1;
        }
        let lo = if k == 0 { f64::NEG_INFINITY } else { edges[k] };
        let hi = if k == N_LEVELS - 2 {
            f64::INFINITY
        } else {
            edges[k + 1]
        };
        (k, (edges[k], inv_width[k]), (lo, hi))
    }

    /// Phase wrap at a period start: wrap `phase` into [0, 1) and draw the
    /// next period's jitter and shimmer. A phase below 0 (only a jitter
    /// draw below -100% can cause it) wraps without a new draw. Out of
    /// line: it runs once per period.
    #[cold]
    #[inline(never)]
    fn new_period(
        phase: f64,
        jit: f64,
        shim: f64,
        jitter: f64,
        shimmer: f64,
        rng: &mut Rng,
    ) -> (f64, f64, f64) {
        if phase < 0.0 || !phase.is_finite() {
            let p = if phase.is_finite() {
                phase.rem_euclid(1.0)
            } else {
                0.0
            };
            return (p, jit, shim);
        }
        let p = phase - 1.0;
        let p = if p >= 1.0 { p.fract() } else { p };
        (p, rng.gauss() * jitter, 1.0 + rng.gauss() * shimmer)
    }

    /// One sample on state `st`; see `tick`.
    #[inline(always)]
    fn step(&self, st: &mut SourceState, f0: f64, av: f64, rng: &mut Rng) -> (f64, f64) {
        st.phase += f0 * (1.0 / SR_F) * (1.0 + st.jit);
        if !(0.0..1.0).contains(&st.phase) {
            (st.phase, st.jit, st.shim) =
                Self::new_period(st.phase, st.jit, st.shim, self.jitter, self.shimmer, rng);
        }

        // Mip blend position: linear in f0 across the current level's
        // band; outside the band, reselect the level (rare: vibrato and
        // glides stay in one half-octave most of the time).
        if !(st.valid.0..st.valid.1).contains(&f0) {
            (st.level, st.band, st.valid) = Self::select_level(&self.edges, &self.inv_width, f0);
        }
        let lf = ((f0 - st.band.0) * st.band.1).clamp(0.0, 1.0) as f32;

        let xi = st.phase * TABLE_LEN as f64;
        let i = (xi as usize) & (TABLE_LEN - 1);
        let fr = (xi - i as f64) as f32;
        let k = st.level.min(N_LEVELS - 2);
        let (lo, hi) = (&self.table.data[k], &self.table.data[k + 1]);
        let (a0, a1, b0, b1) = (lo[i], lo[i + 1], hi[i], hi[i + 1]);
        let mut v = [0.0f32; 4];
        for q in 0..4 {
            let l = a0[q] + (a1[q] - a0[q]) * fr;
            let h = b0[q] + (b1[q] - b0[q]) * fr;
            v[q] = l + (h - l) * lf;
        }
        // The flow rows are equal in every level, so blending v[2], v[3]
        // across levels changes nothing; it keeps the four lanes uniform.
        let w = ((av - 0.42) * (1.0 / 0.6)).clamp(0.0, 1.0);
        let d = v[0] as f64 + (v[1] as f64 - v[0] as f64) * w;
        let g = v[2] as f64 + (v[3] as f64 - v[2] as f64) * w;
        st.z[0] += self.tilt * (d * PULSE_GAIN - st.z[0]);
        st.z[1] += self.tilt * (st.z[0] - st.z[1]);
        (st.z[1] * st.shim * SOURCE_GAIN * self.trim, g)
    }

    /// One sample at fundamental `f0` Hz and voicing `av` (which sets the
    /// lax/tense blend). Returns (pulse, flow): the tilted, shimmered flow
    /// derivative (multiply by av for the voiced source) and the
    /// normalised glottal flow in [0, 1]. `rng` draws jitter and shimmer at
    /// each period start.
    pub fn tick(&mut self, f0: f64, av: f64, rng: &mut Rng) -> (f64, f64) {
        let mut st = self.st;
        let r = self.step(&mut st, f0, av, rng);
        self.st = st;
        r
    }

    /// `n` samples with f0 and av ramped linearly: sample j uses
    /// f0.0 + j f0.1 and av.0 + j av.1. Calls `f(j, pulse, flow)` for each.
    /// The state stays in locals for the block.
    #[inline(always)]
    pub fn run(
        &mut self,
        n: usize,
        f0: (f64, f64),
        av: (f64, f64),
        rng: &mut Rng,
        mut f: impl FnMut(usize, f64, f64),
    ) {
        let mut st = self.st;
        let (mut fq, mut a) = (f0.0, av.0);
        for j in 0..n {
            let (pulse, flow) = self.step(&mut st, fq, a, rng);
            f(j, pulse, flow);
            fq += f0.1;
            a += av.1;
        }
        self.st = st;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rd_index_rounds_and_clamps() {
        assert_eq!(rd_index(1.15), 46);
        assert_eq!(rd_index(0.0), RD_INDEX_MIN);
        assert_eq!(rd_index(9.0), RD_INDEX_MAX);
        assert_eq!(rd_index(f64::NAN), 40);
    }

    #[test]
    fn mip_edges_span_55_to_1760() {
        let e = mip_edges();
        assert!((e[0] - 55.0).abs() < 1e-9);
        assert!((e[N_LEVELS - 1] - 1760.0).abs() < 1e-9);
    }
}

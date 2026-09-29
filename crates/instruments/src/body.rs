//! Bodies: modal synthesis of a stereo body impulse response from a
//! measured 1/12-octave magnitude curve (design section 5.6; commuted
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
//! `tau = Q / (pi f)` has curve amplitude `a0 = sqrt(T df / tau)`, `T` the
//! curve power at f, so the energy per hertz follows the curve. A mode is
//! `a r^i sin(w i + ph)`, `a = a0 g`, g standard normal, run by the
//! recursive oscillator `y = 2 r cos(w) y1 - r^2 y2` for 9.5 tau (82 dB) or
//! to the end of the IR, accumulated in `f64`.
//!
//! Fixed low modes. Below `F_CROSS` (300 Hz) the modes, their Q, g and ph
//! come from the body's own stream (`fixed_stream`), the same for every
//! seed: a body has one set of low modes (the guitar's air and top-plate
//! resonances near 100 and 200 Hz are single modes, not an ensemble). The
//! right channel takes the left's amplitudes with ph + pi/2, so it is close
//! to the Hilbert transform of the left: equal magnitude, uncorrelated.
//! Above `F_CROSS` each channel draws its own g and ph from the seed's
//! stream (decorrelated stereo, a different fine structure per seed). Why
//! 300 Hz: the spacing floor of 3 Hz holds up to 273 Hz, so below it a
//! 1/3 octave holds only 6 (at 80 Hz) to 20 modes, too few for their sum to
//! be statistically stable; above it the count is constant at about 21.
//!
//! Stable band levels. Random modes whose bandwidth f/Q is 2-4% of f
//! overlap and interfere, so the band level of a free draw scatters by
//! about 3 dB at every frequency (measured over 32 seeds, 1/3 octave). Two
//! constraints fix what the measured curve fixes and leave the rest random:
//! (1) modes are summed in groups of 1/12 octave (the curve's resolution)
//! from `F_CROSS` down and up, merged upward until a group holds
//! `MIN_GROUP` (3) modes (`groups`), and each group is scaled per channel to
//! its expected energy `sum a0^2 / 2 sum_{i<m} r^2i`; (2) before the sum, each
//! group's complex amplitudes `a e^(j ph)` move by the least weighted
//! change that gives the group no onset step (the IR starts at 0) and no DC
//! (a radiated pressure has none). Without (2) each group's random onset
//! leaks a 1/f skirt above it and a shelf below it, whose level is one
//! random variable per group. The constraints take 2 of a group's 2n real
//! degrees of freedom, so a 1-mode group would be driven to rounding
//! residue and then scaled up by about 1e16; the minimum group size keeps
//! every group well posed (at least 10% of the drawn power kept, gains
//! 1-51, median 2.2-2.8, over 32 seeds). Result, largest 1/3-octave std
//! over 32 seeds from 80 Hz up (was 5.3 dB guitar, 5.9 harp, 8.0 violin):
//! guitar 1.2, harp 1.5, violin 1.4 from 160 Hz (its bands below 160 Hz
//! hold no modes and lie 40-60 dB under its peak). Below 300 Hz: guitar
//! and harp under 0.2 dB. `examples/bodyspread.rs` prints the table and
//! the group diagnostics (`Body::taps_with_groups`).
//!
//! The last 10% of the IR is faded linearly, then each channel is scaled to
//! unit energy. The IR is not shortened: its 0.32-0.40 s modal tail is
//! part of the body sound.

use dsp::conv::StereoIr;
use sfcore::random::{tag, Rng};
use sfcore::SR_F;
use std::f64::consts::{PI, TAU};
use std::ops::Range;

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
/// Energy groups per octave: the curve's own resolution, so each group is
/// held to what the measurement specifies. A group holds about 5 modes
/// above `F_CROSS`. Measured against 1/4, 1/6 and 1/8 octave groups: the
/// smallest worst-case band spread.
const GROUPS_PER_OCT: f64 = 12.0;
/// Fewest modes in an energy group. The step and DC removal takes 2 of a
/// group's 2n real degrees of freedom; at n = 1 it takes all of them.
/// With n >= 3 at least 4 random degrees stay. Over 32 seeds the smallest
/// kept power fraction is 0.105 and the largest gain 51 (a group whose
/// modes cancel in time); tests/body.rs bounds them at 0.05 and 100.
pub const MIN_GROUP: usize = 3;
/// Crossover between the fixed low modes and the per-seed modes, Hz.
pub const F_CROSS: f64 = 300.0;

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

    /// The body's own stream for its low modes: the same for every seed.
    fn fixed_stream(self) -> Rng {
        let k = match self {
            Body::Guitar => 0,
            Body::Harp => 1,
            Body::Violin => 2,
        };
        Rng::stream(k, tag("instruments.body.low"))
    }

    pub fn spec(self) -> BodySpec {
        match self {
            Body::Guitar => BodySpec {
                secs: 0.36,
                q_lo: 26.0,
                q_hi: 48.0,
                f_min: 70.0,
                trim: 5.4,
            },
            Body::Harp => BodySpec {
                secs: 0.4,
                q_lo: 18.0,
                q_hi: 34.0,
                f_min: 60.0,
                trim: 7.3,
            },
            Body::Violin => BodySpec {
                secs: 0.32,
                q_lo: 30.0,
                q_hi: 40.0,
                f_min: 180.0,
                trim: 2.4,
            },
        }
    }

    /// The stereo modal IR, unit energy per channel, before the trim.
    /// Modes below `F_CROSS` draw from `fixed_stream`, modes above from
    /// `rng` (see the module docs).
    pub fn taps(self, rng: &mut Rng) -> [Vec<f64>; 2] {
        self.synth(rng, None)
    }

    /// `taps`, plus one `GroupInfo` per energy group, in frequency order:
    /// the diagnostics behind the module's stability claims.
    pub fn taps_with_groups(self, rng: &mut Rng) -> ([Vec<f64>; 2], Vec<GroupInfo>) {
        let mut info = Vec::new();
        let ch = self.synth(rng, Some(&mut info));
        (ch, info)
    }

    /// The mode table in frequency order; each mode's draws in a fixed
    /// order (spacing, Q, amplitudes, phases) from the stream that owns it.
    fn modes(self, rng: &mut Rng) -> Vec<Mode> {
        let s = self.spec();
        let n = (s.secs * SR_F).round() as usize;
        let mut fixed = self.fixed_stream();
        let mut modes = Vec::with_capacity(512);
        let mut f = s.f_min;
        loop {
            let r: &mut Rng = if f < F_CROSS { &mut fixed } else { rng };
            let df = (f * 0.011).max(3.0) * (0.55 + 0.9 * r.uniform());
            f += df;
            if f >= F_MODE_MAX {
                break;
            }
            let low = f < F_CROSS;
            let r: &mut Rng = if low { &mut fixed } else { rng };
            let ramp = ((f / 200.0).log2() / 6.0).clamp(0.0, 1.0);
            let q = s.q_lo + (s.q_hi - s.q_lo) * ramp * (0.7 + 0.6 * r.uniform());
            let tau = q / (PI * f);
            let power = 10f64.powf(self.level_db(f) / 10.0);
            let a0 = (power * df / tau).sqrt();
            let m = n.min((tau * SR_F * MODE_TAUS).ceil() as usize);
            let (amp, ph) = if low {
                // One amplitude; the right channel in quadrature.
                let a = a0 * r.gauss();
                let p = r.uniform() * TAU;
                ([a, a], [p, p + 0.5 * PI])
            } else {
                let (al, pl) = (a0 * r.gauss(), r.uniform() * TAU);
                let (ar, pr) = (a0 * r.gauss(), r.uniform() * TAU);
                ([al, ar], [pl, pr])
            };
            modes.push(Mode {
                f,
                tau,
                m,
                a0,
                amp,
                ph,
            });
        }
        modes
    }

    fn synth(self, rng: &mut Rng, mut info: Option<&mut Vec<GroupInfo>>) -> [Vec<f64>; 2] {
        let n = (self.spec().secs * SR_F).round() as usize;
        let modes = self.modes(rng);
        let mut ch = [vec![0.0f64; n], vec![0.0f64; n]];
        let mut gb = vec![0.0f64; n];
        for g in groups(&modes) {
            let gi = flush_group(&mut ch, &mut gb, &modes[g]);
            if let Some(v) = info.as_deref_mut() {
                v.push(gi);
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

/// One mode of the IR: frequency, decay time, length in samples, the
/// curve amplitude and each channel's amplitude and phase.
struct Mode {
    f: f64,
    tau: f64,
    m: usize,
    a0: f64,
    amp: [f64; 2],
    ph: [f64; 2],
}

/// The group's complex amplitudes `c_k = a_k e^(j ph_k)` on channel `ch`,
/// moved by the least change (weighted by `a0_k^2`) that satisfies two
/// linear constraints: no onset step, `sum Im c_k = 0` (the IR starts at
/// zero), and no DC, `sum Im(c_k / (1/tau_k - j w_k)) = 0` (the integral of
/// `a e^(-t/tau) sin(w t + ph)` over t >= 0). A radiated pressure has no DC,
/// and without the step the group's energy stays near its band: the random
/// onset of a mode set otherwise leaks a 1/f skirt (and, below the modes, a
/// DC shelf) whose level is a single random variable per group.
///
/// A group of n modes has 2n real unknowns; with n = 1 the two constraints
/// fix both, the solution is c = 0 and only rounding residue remains, which
/// the energy scaling would then raise by about 1e16. `groups` therefore
/// never forms a group under `MIN_GROUP` modes, and a smaller group (only a
/// side of `F_CROSS` holding fewer modes in all) is returned unchanged.
fn no_step_no_dc(group: &[Mode], ch: usize) -> Vec<(f64, f64)> {
    let mut c: Vec<(f64, f64)> = group
        .iter()
        .map(|m| (m.amp[ch] * m.ph[ch].cos(), m.amp[ch] * m.ph[ch].sin()))
        .collect();
    if group.len() < MIN_GROUP {
        return c;
    }
    // Gradients per mode as (d/d re, d/d im): step (0, 1); DC (ki, kr)
    // with k = 1/(1/tau - j w) = (1/tau + j w) / (1/tau^2 + w^2), scaled
    // by the mean w so both rows have like magnitudes.
    let k: Vec<(f64, f64)> = group
        .iter()
        .map(|m| {
            let (s, w) = (1.0 / m.tau, TAU * m.f);
            let d = s * s + w * w;
            (s / d, w / d)
        })
        .collect();
    let wm = group.iter().map(|m| TAU * m.f).sum::<f64>() / group.len().max(1) as f64;
    let g2 = |i: usize| (k[i].1 * wm, k[i].0 * wm);
    let (mut l1, mut l2) = (0.0, 0.0);
    let (mut g11, mut g12, mut g22) = (0.0, 0.0, 0.0);
    for (i, m) in group.iter().enumerate() {
        let wt = m.a0 * m.a0;
        let (gr, gi) = g2(i);
        l1 += c[i].1;
        l2 += c[i].0 * gr + c[i].1 * gi;
        g11 += wt;
        g12 += wt * gi;
        g22 += wt * (gr * gr + gi * gi);
    }
    let det = g11 * g22 - g12 * g12;
    if det.abs() <= 1e-12 * g11 * g22 {
        // One mode (or collinear gradients): remove the step only.
        if g11 > 0.0 {
            let lam = l1 / g11;
            for (ci, m) in c.iter_mut().zip(group) {
                ci.1 -= m.a0 * m.a0 * lam;
            }
        }
        return c;
    }
    let lam1 = (g22 * l1 - g12 * l2) / det;
    let lam2 = (g11 * l2 - g12 * l1) / det;
    for (i, m) in group.iter().enumerate() {
        let wt = m.a0 * m.a0;
        let (gr, gi) = g2(i);
        c[i].0 -= wt * lam2 * gr;
        c[i].1 -= wt * (lam1 + lam2 * gi);
    }
    c
}

/// Energy cell of a mode at `f` Hz: 1/`GROUPS_PER_OCT` octave cells
/// from `F_CROSS`, so no cell holds both fixed and per-seed modes.
fn group_of(f: f64) -> i64 {
    ((f / F_CROSS).log2() * GROUPS_PER_OCT).floor() as i64
}

/// Partitions the frequency-ordered `modes` into energy groups: runs of
/// whole cells (`group_of`), each holding at least `MIN_GROUP` modes, never
/// crossing `F_CROSS`. Cells accumulate upward until the run holds
/// `MIN_GROUP` modes; a remainder at the top of a side joins the group
/// below it. This merges the sparse cells at the bottom (the 3 Hz spacing
/// floor puts 1-2 modes in a 1/12 octave near 80 Hz) and the partial cell
/// under `F_MODE_MAX`.
fn groups(modes: &[Mode]) -> Vec<Range<usize>> {
    let n = modes.len();
    let mut out: Vec<Range<usize>> = Vec::new();
    let (mut start, mut side_first) = (0, 0);
    for i in 1..=n {
        if i < n && group_of(modes[i].f) == group_of(modes[i - 1].f) {
            continue;
        }
        let side_end = i == n || (modes[i].f < F_CROSS) != (modes[i - 1].f < F_CROSS);
        if i - start >= MIN_GROUP || side_end {
            let merge = i - start < MIN_GROUP && out.len() > side_first;
            match out.last_mut() {
                Some(last) if merge => last.end = i,
                _ => out.push(start..i),
            }
            start = i;
        }
        if side_end {
            side_first = out.len();
        }
    }
    out
}

/// Diagnostics of one energy group.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupInfo {
    /// Lowest and highest mode frequency, Hz.
    pub f_lo: f64,
    pub f_hi: f64,
    /// Mode count.
    pub modes: usize,
    /// Per channel: the fraction of the drawn weighted power
    /// `sum |c|^2 / a0^2` left after the step and DC removal. Near
    /// (n - 1) / n for n random modes; near 0 means the constraints consumed
    /// the group.
    pub kept: [f64; 2],
    /// Per channel: the scale applied to reach the group's expected energy.
    pub gain: [f64; 2],
}

/// Synthesises the modes of one group per channel, scales each channel to
/// the group's expected energy and adds it into `ch`. `gb` is scratch of
/// the IR length, zero on entry and on return.
fn flush_group(ch: &mut [Vec<f64>; 2], gb: &mut [f64], group: &[Mode]) -> GroupInfo {
    let len = group.iter().map(|m| m.m).max().unwrap_or(0);
    let mut info = GroupInfo {
        f_lo: group.first().map_or(0.0, |m| m.f),
        f_hi: group.last().map_or(0.0, |m| m.f),
        modes: group.len(),
        kept: [0.0; 2],
        gain: [0.0; 2],
    };
    // Expected energy of a0 g r^i sin(w i + ph), g ~ N(0, 1), ph uniform:
    // a0^2 / 2 sum_{i<m} r^2i.
    let target: f64 = group
        .iter()
        .map(|md| {
            let r2 = (-2.0 / (md.tau * SR_F)).exp();
            md.a0 * md.a0 * 0.5 * (1.0 - r2.powi(md.m as i32)) / (1.0 - r2)
        })
        .sum();
    for (c, d) in ch.iter_mut().enumerate() {
        let coef = no_step_no_dc(group, c);
        let (mut p0, mut p1) = (0.0, 0.0);
        for (md, &(cr, ci)) in group.iter().zip(&coef) {
            let w0 = 1.0 / (md.a0 * md.a0).max(f64::MIN_POSITIVE);
            p0 += md.amp[c] * md.amp[c] * w0;
            p1 += (cr * cr + ci * ci) * w0;
            // a sin(w i + ph) with a e^(j ph) = cr + j ci.
            let a = cr.hypot(ci);
            let ph = ci.atan2(cr);
            let w = TAU * md.f / SR_F;
            let rr = (-1.0 / (md.tau * SR_F)).exp();
            let c2 = 2.0 * rr * w.cos();
            let r2 = rr * rr;
            // y[i] = a r^i sin(w i + ph): seed y[0] and y[-1].
            let mut y1 = a * ph.sin();
            let mut y2 = a * (ph - w).sin() / rr;
            for x in gb[..md.m].iter_mut() {
                *x += y1;
                let y = c2 * y1 - r2 * y2;
                y2 = y1;
                y1 = y;
            }
        }
        let e: f64 = gb[..len].iter().map(|x| x * x).sum();
        let k = if e > 0.0 { (target / e).sqrt() } else { 0.0 };
        info.kept[c] = if p0 > 0.0 { p1 / p0 } else { 0.0 };
        info.gain[c] = k;
        for (x, y) in d[..len].iter_mut().zip(gb[..len].iter_mut()) {
            *x += k * *y;
            *y = 0.0;
        }
    }
    info
}

/// Body transfer magnitudes, dB, 1/12 octave from 80 Hz: guitar, harp,
/// violin. Measured (see the module docs); do not edit by hand.
pub static BODY_CURVES: [[f64; 86]; 3] = [
    // Guitar
    [
        -36.8, -36.8, -36.8, -31.3, -25.8, -20.2, -19.1, -18.0, -17.4, -18.5, -19.9, -20.7, -18.9,
        -12.9, -6.6, -1.4, -0.2, 0.0, -1.2, -4.9, -10.3, -15.3, -16.2, -14.6, -13.1, -13.5, -11.7,
        -12.1, -13.1, -16.9, -18.6, -20.4, -22.0, -23.1, -25.1, -24.8, -26.4, -24.1, -24.1, -22.3,
        -22.4, -22.1, -20.7, -19.4, -18.1, -20.1, -23.6, -25.7, -26.8, -25.1, -27.3, -28.1, -30.9,
        -31.1, -30.9, -30.2, -29.4, -29.0, -28.6, -29.9, -31.9, -35.3, -37.9, -40.4, -41.8, -42.6,
        -42.7, -42.4, -42.6, -42.9, -43.3, -43.2, -42.7, -41.8, -40.8, -39.6, -38.7, -37.9, -37.7,
        -37.6, -37.6, -37.8, -38.5, -39.0, -39.1, -38.8,
    ],
    // Harp
    [
        -35.3, -34.2, -32.0, -28.6, -25.1, -21.3, -18.6, -17.1, -17.0, -17.3, -17.5, -16.6, -14.3,
        -10.6, -6.5, -2.7, -0.3, 0.0, -1.8, -4.8, -8.0, -10.7, -12.4, -13.0, -12.3, -11.4, -11.1,
        -11.9, -12.9, -14.7, -16.7, -18.7, -20.3, -21.6, -22.8, -23.4, -23.8, -23.5, -23.3, -22.8,
        -22.5, -21.9, -21.4, -21.2, -22.0, -23.5, -25.6, -27.6, -29.6, -31.1, -32.7, -34.1, -35.9,
        -37.0, -37.9, -38.1, -38.2, -38.6, -39.5, -41.3, -43.6, -46.6, -49.5, -52.2, -54.3, -55.8,
        -56.8, -57.6, -58.3, -59.0, -59.6, -60.1, -60.2, -60.1, -59.8, -59.4, -59.1, -58.9, -58.9,
        -59.0, -59.3, -59.9, -60.4, -60.9, -61.4, -61.7,
    ],
    // Violin
    [
        -24.2, -23.5, -22.8, -22.2, -21.5, -20.8, -20.2, -19.5, -18.8, -18.2, -17.5, -16.8, -16.2,
        -15.5, -14.8, -14.2, -13.5, -12.8, -12.2, -11.5, -11.0, -8.2, -5.5, -3.8, -3.2, -3.5, -4.0,
        -4.5, -4.9, -5.1, -6.1, -7.6, -9.0, -9.4, -9.3, -10.0, -12.0, -14.0, -12.6, -10.2, -8.0,
        -8.3, -9.6, -10.6, -9.8, -7.9, -9.0, -12.0, -14.9, -14.7, -14.4, -14.0, -13.6, -10.1, -6.7,
        -1.9, 0.0, -0.1, -2.0, -5.9, -8.1, -10.1, -9.5, -9.8, -9.2, -9.7, -9.3, -9.8, -10.1, -11.3,
        -14.1, -18.0, -21.3, -22.8, -23.5, -24.9, -27.0, -28.6, -30.6, -32.6, -35.5, -37.6, -39.0,
        -40.8, -43.6, -46.5,
    ],
];

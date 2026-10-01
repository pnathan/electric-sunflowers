//! Vocal tract: a cascade formant synthesiser (D. H. Klatt, "Software for a
//! cascade/parallel formant synthesizer", JASA 67(3), 1980; D. H. Klatt and
//! L. C. Klatt, JASA 87(2), 1990).
//!
//! Signal path, all `f64`:
//! - Cascade of Klatt unity-DC two-pole resonators (dsp::resonator): F1-F3
//!   follow the articulation (targets per HOP frame, coefficients ramped
//!   linearly per sample, design 3.4); F4, F5 and up to four high
//!   resonances at 5.5, 6.6, 7.7, 8.8 kHz times the voice's formant scale
//!   are fixed per voice and designed once.
//! - RBJ high shelf, +16 dB at 5.2 kHz, Q 0.7, after the cascade.
//! - Frication: white noise through an RBJ constant-peak band-pass at the
//!   consonant's centre and bandwidth (Q = max(0.5, ff / bw)), added after
//!   the tract with gain 2.2. The band-pass designer clamps the centre at
//!   0.45 fs; that is the only clamp.
//! - DC blocker, pole 0.995.
//!
//! Sound-setting values: F4 = (3350 - 350 sf) fs, F5 = (3950 - 500 sf) fs,
//! B4 = 250 - 110 sf, B5 = 300 - 120 sf (sf = singer's-formant level, fs =
//! formant scale); high resonance bandwidths 420 + 0.05 f; F1-F3 bandwidths
//! set by the synth from breath and nasality.
//!
//! A parallel high-frequency branch was tried and rejected: it filled the
//! spectral valleys (/uw/ from -55..-82 dB to -41 dB) and cut vowel
//! distinctness from 13.6 to 10.6 dB.

use dsp::biquad::{Biquad, BiquadCoeffs};
use dsp::onepole::DcBlocker;
use dsp::resonator::{coeffs, Resonator};
use sfcore::{HOP, SR_F};

use crate::params::VoiceParams;
use crate::tuning::{FRICATION_GAIN, SHELF_DB, SHELF_HZ, SHELF_Q};

/// Most high resonances above F5.
pub const MAX_HIGH: usize = 4;
/// Centres of the high resonances at formant scale 1, Hz.
const HIGH_HZ: [f64; MAX_HIGH] = [5500.0, 6600.0, 7700.0, 8800.0];
/// DC blocker pole.
pub const DC_POLE: f64 = 0.995;
/// Highest resonator centre as a fraction of the sample rate.
const MAX_CENTRE: f64 = 0.45;

/// Klatt coefficients with the centre clamped to 0.45 fs.
fn design(f: f64, bw: f64) -> (f64, f64, f64) {
    coeffs(f.min(SR_F * MAX_CENTRE), bw, SR_F)
}

/// Centre and bandwidth (Hz) of the three moving formants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Formants {
    pub f: [f64; 3],
    pub bw: [f64; 3],
}

/// Number of fixed sections: F4, F5 and `MAX_HIGH` high resonances.
/// Unused high resonances are identity sections (a = 1, b = c = 0).
const N_FIXED: usize = 2 + MAX_HIGH;

/// The tract filters and their state.
pub struct Tract {
    /// F1-F3. Coefficients ramp per sample toward `target` by `step`
    /// for `left` more samples (the rule of Resonator::set_ramped, run
    /// here inside the block loop).
    moving: [Resonator; 3],
    step: [[f64; 3]; 3],
    target: [[f64; 3]; 3],
    left: u32,
    fixed: [Resonator; N_FIXED],
    /// Index of the last non-identity fixed section.
    last_fixed: usize,
    shelf: Biquad,
    fric: Biquad,
    /// Last two frication outputs, for the idle test.
    fric_y: [f64; 2],
    dc: DcBlocker,
    /// True after a reset: the next formant update jumps instead of ramping.
    fresh: bool,
}

impl Tract {
    /// The tract of voice `p` with `n_high` (0-4) high resonances.
    pub fn new(p: &VoiceParams, n_high: usize) -> Tract {
        let (sf, fs) = (p.sf, p.fs);
        let n_high = n_high.min(MAX_HIGH);
        let mut fixed = [Resonator::default(); N_FIXED];
        for r in &mut fixed {
            r.set_coeffs((1.0, 0.0, 0.0));
        }
        fixed[0].set_coeffs(design((3350.0 - sf * 350.0) * fs, 250.0 - sf * 110.0));
        fixed[1].set_coeffs(design((3950.0 - sf * 500.0) * fs, 300.0 - sf * 120.0));
        for h in 0..n_high {
            let f = HIGH_HZ[h] * fs;
            fixed[2 + h].set_coeffs(design(f, 420.0 + 0.05 * f));
        }
        Tract {
            moving: [Resonator::default(); 3],
            step: [[0.0; 3]; 3],
            target: [[0.0; 3]; 3],
            left: 0,
            fixed,
            last_fixed: 1 + n_high,
            shelf: Biquad::new(BiquadCoeffs::high_shelf(SR_F, SHELF_HZ, SHELF_Q, SHELF_DB)),
            fric: Biquad::default(),
            fric_y: [0.0; 2],
            dc: DcBlocker::new(DC_POLE),
            fresh: true,
        }
    }

    /// True after a reset, until `jump_formants` or `set_formants`: the
    /// F1-F3 coefficients are stale and must be set, not ramped from.
    pub fn is_fresh(&self) -> bool {
        self.fresh
    }

    /// Take F1-F3 coefficients for `now` at once (after a reset).
    pub fn jump_formants(&mut self, now: &Formants) {
        for q in 0..3 {
            self.moving[q].set_coeffs(design(now.f[q], now.bw[q]));
        }
        self.left = 0;
        self.fresh = false;
    }

    /// Ramp F1-F3 to `next` over the next HOP samples: sample k of the
    /// ramp uses start + k (target - start) / HOP, the HOP-th the target
    /// exactly (as Resonator::set_ramped). The ramp is linear in (a, b,
    /// c); the stable triangle |c| < 1, |b| < 1 - c is convex, so every
    /// intermediate filter is stable, and a = 1 - b - c keeps unity DC gain
    /// along the path (design 3.4).
    pub fn ramp_formants(&mut self, next: &Formants) {
        let k = 1.0 / HOP as f64;
        for q in 0..3 {
            let r = &self.moving[q];
            let (a, b, c) = design(next.f[q], next.bw[q]);
            self.target[q] = [a, b, c];
            self.step[q] = [(a - r.a) * k, (b - r.b) * k, (c - r.c) * k];
        }
        self.left = HOP as u32;
    }

    /// `jump_formants(now)` if fresh, then `ramp_formants(next)`.
    pub fn set_formants(&mut self, now: &Formants, next: &Formants) {
        if self.fresh {
            self.jump_formants(now);
        }
        self.ramp_formants(next);
    }

    /// Frication band for this hop: centre `ff`, bandwidth `bw` Hz.
    pub fn set_frication(&mut self, ff: f64, bw: f64) {
        let q = (ff / bw).max(0.5) * crate::tuning::FRIC_Q_SCALE;
        self.fric.set_coeffs(BiquadCoeffs::bandpass(SR_F, ff, q));
    }

    /// Run `buf` through the cascade and the high shelf in place,
    /// advancing the F1-F3 ramp one step per sample.
    ///
    /// The sections run in groups of three (F1-F3 with their ramp; F4, F5,
    /// H1; H2-H4 and the shelf), one loop per group over the block, on
    /// local copies of coefficients and state. Each group's recursions stay
    /// in registers and the three sections of a group overlap in the
    /// pipeline.
    pub fn cascade_block(&mut self, buf: &mut [f64]) {
        let k = (self.left as usize).min(buf.len());
        if k > 0 {
            let (head, tail) = buf.split_at_mut(k);
            self.run_moving_ramp(head);
            self.left -= k as u32;
            if self.left == 0 {
                for q in 0..3 {
                    let t = self.target[q];
                    (self.moving[q].a, self.moving[q].b, self.moving[q].c) = (t[0], t[1], t[2]);
                }
            }
            run_group(&mut self.moving, tail);
        } else {
            run_group(&mut self.moving, buf);
        }
        let (f_lo, f_hi) = self.fixed.split_at_mut(3);
        run_group(f_lo, buf);
        run_group(f_hi, buf);
        let mut shelf = self.shelf;
        for x in buf.iter_mut() {
            *x = shelf.tick(*x);
        }
        self.shelf = shelf;
    }

    /// F1-F3 over `buf` with the coefficient ramp advancing each sample.
    fn run_moving_ramp(&mut self, buf: &mut [f64]) {
        let mut c = [[0.0f64; 3]; 3];
        let mut y = [[0.0f64; 2]; 3];
        for q in 0..3 {
            let r = &self.moving[q];
            c[q] = [r.a, r.b, r.c];
            y[q] = [r.y1, r.y2];
        }
        let d = self.step;
        for x in buf.iter_mut() {
            let mut v = *x;
            for q in 0..3 {
                c[q][0] += d[q][0];
                c[q][1] += d[q][1];
                c[q][2] += d[q][2];
                let o = c[q][0] * v + c[q][2] * y[q][1] + c[q][1] * y[q][0];
                y[q][1] = y[q][0];
                y[q][0] = o;
                v = o;
            }
            *x = v;
        }
        for q in 0..3 {
            let r = &mut self.moving[q];
            (r.a, r.b, r.c) = (c[q][0], c[q][1], c[q][2]);
            (r.y1, r.y2) = (y[q][0], y[q][1]);
        }
    }

    /// Current (a, b, c) of the three moving formants.
    pub fn moving_coeffs(&self) -> [(f64, f64, f64); 3] {
        [0, 1, 2].map(|q| (self.moving[q].a, self.moving[q].b, self.moving[q].c))
    }

    /// Output stage for one block: `tract` (cascade output) plus
    /// band-passed `fric` (noise times af) times 2.2, DC blocked, into
    /// `out`. `FRIC` selects the frication path at compile time; when off,
    /// `fric` is not read.
    #[inline(always)]
    pub fn finish_block<const FRIC: bool>(&mut self, tract: &[f64], fric: &[f64], out: &mut [f32]) {
        let (mut bp, mut dc) = (self.fric, self.dc);
        let mut fy = self.fric_y;
        for (j, (o, &t)) in out.iter_mut().zip(tract.iter()).enumerate() {
            let mut v = t;
            if FRIC {
                let y = bp.tick(fric[j]);
                fy = [y, fy[0]];
                v += y * FRICATION_GAIN;
            }
            *o = dc.tick(v) as f32;
        }
        (self.fric, self.dc, self.fric_y) = (bp, dc, fy);
    }

    /// True when the frication filter holds energy above 1e-7.
    pub fn frication_ringing(&self) -> bool {
        self.fric_y[0].abs() > 1e-7 || self.fric_y[1].abs() > 1e-7
    }

    /// Clear the frication filter.
    pub fn reset_frication(&mut self) {
        self.fric.reset();
        self.fric_y = [0.0; 2];
    }

    /// True when the cascade output and the frication filter have decayed
    /// below 1e-7.
    pub fn is_quiet(&self) -> bool {
        self.fixed[self.last_fixed].y1.abs() < 1e-7 && !self.frication_ringing()
    }

    /// Clear the resonators and the frication filter (silent frames); the
    /// next formant update jumps to its target.
    pub fn reset_resonators(&mut self) {
        for r in &mut self.moving {
            r.reset();
        }
        for r in &mut self.fixed {
            r.reset();
        }
        self.reset_frication();
        self.left = 0;
        self.fresh = true;
    }

    /// Clear every state word.
    pub fn reset(&mut self) {
        self.reset_resonators();
        self.shelf.reset();
        self.dc.reset();
    }

    /// Zero state words below the denormal floor (phrase boundaries).
    pub fn flush_denormals(&mut self) {
        for r in &mut self.moving {
            r.flush_denormals();
        }
        for r in &mut self.fixed {
            r.flush_denormals();
        }
        self.shelf.flush_denormals();
        self.fric.flush_denormals();
    }

    /// DC gain of the cascade with the current coefficients (1 by
    /// construction of the Klatt resonator and the shelf's unity DC).
    pub fn cascade_dc_gain(&self) -> f64 {
        let mut g = 1.0;
        for r in self.moving.iter().chain(self.fixed.iter()) {
            g *= r.a / (1.0 - r.b - r.c);
        }
        let c = self.shelf.coeffs();
        g * (c.b0 + c.b1 + c.b2) / (1.0 + c.a1 + c.a2)
    }
}

/// Three fixed sections (coefficients constant over the block) over `buf`
/// in one loop, state in locals: y = a x + c y2 + b y1 (Klatt).
#[inline(always)]
fn run_group(r: &mut [Resonator], buf: &mut [f64]) {
    let r: &mut [Resonator; 3] = match r.try_into() {
        Ok(g) => g,
        Err(_) => return,
    };
    let c = [0, 1, 2].map(|q| [r[q].a, r[q].b, r[q].c]);
    let mut y = [0, 1, 2].map(|q| [r[q].y1, r[q].y2]);
    for x in buf.iter_mut() {
        let mut v = *x;
        for q in 0..3 {
            let o = c[q][0] * v + c[q][2] * y[q][1] + c[q][1] * y[q][0];
            y[q][1] = y[q][0];
            y[q][0] = o;
            v = o;
        }
        *x = v;
    }
    for q in 0..3 {
        (r[q].y1, r[q].y2) = (y[q][0], y[q][1]);
    }
}

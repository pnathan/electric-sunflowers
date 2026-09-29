//! Note-wise pitch check with YIN (de Cheveigne and Kawahara 2002,
//! "YIN, a fundamental frequency estimator for speech and music", JASA 111).
//!
//! YIN per frame: difference function d(tau) (step 2), cumulative mean
//! normalised difference d'(tau) (step 3), absolute threshold 0.1 (step 4:
//! the first tau with d' below it, then the local minimum after it; the
//! global minimum when no tau passes), parabolic interpolation (step 5)
//! through the raw d(tau) at the chosen tau and its neighbours: the paper
//! takes the refined period from d, not d', to avoid the bias that the d'
//! normalisation adds.
//! Frames are 40 ms with a 10 ms hop; the integration window is half the
//! frame, so tau spans 0.5 ms to 20 ms (f0 50 Hz to 2 kHz). The cross term
//! of d(tau) comes from an FFT correlation; the energy terms from prefix
//! sums.
//!
//! Per note of 150 ms or longer: YIN over the middle 60% of the note,
//! median f0 over voiced frames (frames that pass the threshold; all frames
//! when none does), error in cents against the note's MIDI pitch.

use crate::fft::Fft;
use serde::{Deserialize, Serialize};

pub const THRESHOLD: f64 = 0.1;
pub const FRAME_S: f64 = 0.040;
pub const HOP_S: f64 = 0.010;
pub const MIN_NOTE_S: f64 = 0.150;
pub const TOL_CENTS: f64 = 50.0;

/// One entry of notes.json.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Note {
    pub t0: f64,
    pub t1: f64,
    pub midi: f64,
}

/// Per-note result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteResult {
    pub t0: f64,
    pub t1: f64,
    pub midi: f64,
    pub f0: Option<f64>,
    pub cents: Option<f64>,
    pub octave_error: bool,
}

/// Whole-file result, written as JSON.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PitchReport {
    pub notes_total: usize,
    pub notes_analysed: usize,
    pub within_50c: usize,
    pub fraction_within_50c: f64,
    pub octave_errors: usize,
    pub median_abs_cents: Option<f64>,
    pub notes: Vec<NoteResult>,
}

/// A reusable YIN estimator for one sample rate.
pub struct Yin {
    sr: f64,
    n: usize,
    w: usize,
    tau_min: usize,
    fft: Fft,
    a_re: Vec<f64>,
    a_im: Vec<f64>,
    b_re: Vec<f64>,
    b_im: Vec<f64>,
    prefix: Vec<f64>,
    /// Raw difference function d(tau), for step 5.
    d: Vec<f64>,
    /// Cumulative mean normalised difference d'(tau), for steps 3 and 4.
    dn: Vec<f64>,
}

impl Yin {
    pub fn new(sample_rate: u32) -> Yin {
        let sr = sample_rate as f64;
        let n = ((FRAME_S * sr).round() as usize).max(8);
        let w = n / 2;
        let tau_min = ((sr / 2000.0).floor() as usize).max(2);
        let m = (n + w).next_power_of_two();
        let fft = Fft::new(m).expect("power of two");
        Yin {
            sr,
            n,
            w,
            tau_min,
            fft,
            a_re: vec![0.0; m],
            a_im: vec![0.0; m],
            b_re: vec![0.0; m],
            b_im: vec![0.0; m],
            prefix: vec![0.0; n + 1],
            d: vec![0.0; w + 1],
            dn: vec![0.0; w + 1],
        }
    }

    pub fn frame_len(&self) -> usize {
        self.n
    }

    /// f0 of `x` (length at least `frame_len()`), and whether the threshold
    /// was passed. `None` for a silent frame.
    pub fn estimate(&mut self, x: &[f64]) -> Option<(f64, bool)> {
        let (n, w) = (self.n, self.w);
        let x = &x[..n];
        self.prefix[0] = 0.0;
        for i in 0..n {
            self.prefix[i + 1] = self.prefix[i] + x[i] * x[i];
        }
        if self.prefix[n] <= 1e-12 * n as f64 {
            return None;
        }
        // cross term c(tau) = sum_{j<w} x[j] x[j+tau]
        let m = self.fft.len();
        for i in 0..m {
            self.a_re[i] = if i < w { x[i] } else { 0.0 };
            self.a_im[i] = 0.0;
            self.b_re[i] = if i < n { x[i] } else { 0.0 };
            self.b_im[i] = 0.0;
        }
        self.fft.run(&mut self.a_re, &mut self.a_im, false);
        self.fft.run(&mut self.b_re, &mut self.b_im, false);
        for i in 0..m {
            let (ar, ai, br, bi) = (self.a_re[i], -self.a_im[i], self.b_re[i], self.b_im[i]);
            self.b_re[i] = ar * br - ai * bi;
            self.b_im[i] = ar * bi + ai * br;
        }
        self.fft.run(&mut self.b_re, &mut self.b_im, true);
        // difference and cumulative mean normalised difference
        let e0 = self.prefix[w];
        self.d[0] = 0.0;
        self.dn[0] = 1.0;
        let mut run = 0.0;
        for tau in 1..=w {
            let d = (e0 + self.prefix[tau + w] - self.prefix[tau] - 2.0 * self.b_re[tau]).max(0.0);
            run += d;
            self.d[tau] = d;
            self.dn[tau] = if run > 0.0 { d * tau as f64 / run } else { 1.0 };
        }
        let hi = w - 1;
        let mut pick = None;
        let mut tau = self.tau_min;
        while tau < hi {
            if self.dn[tau] < THRESHOLD {
                while tau + 1 < hi && self.dn[tau + 1] < self.dn[tau] {
                    tau += 1;
                }
                pick = Some(tau);
                break;
            }
            tau += 1;
        }
        let voiced = pick.is_some();
        let t = pick.unwrap_or_else(|| {
            (self.tau_min..hi)
                .min_by(|&a, &b| self.dn[a].total_cmp(&self.dn[b]))
                .unwrap_or(self.tau_min)
        });
        // step 5: parabolic interpolation on the raw d(tau)
        let (y0, y1, y2) = (self.d[t - 1], self.d[t], self.d[t + 1]);
        let den = y0 - 2.0 * y1 + y2;
        let off = if den.abs() > 1e-15 {
            (0.5 * (y0 - y2) / den).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        Some((self.sr / (t as f64 + off), voiced))
    }
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let k = v.len() / 2;
    Some(if v.len() % 2 == 1 {
        v[k]
    } else {
        0.5 * (v[k - 1] + v[k])
    })
}

/// Cents from the pitch of MIDI note `midi` (A4 = 440 Hz) to `f`.
pub fn cents(f: f64, midi: f64) -> f64 {
    1200.0 * (f / (440.0 * 2f64.powf((midi - 69.0) / 12.0))).log2()
}

/// Median YIN f0 of `x[a..b]`, or `None` when no frame fits or all are silent.
pub fn segment_f0(yin: &mut Yin, x: &[f64], a: usize, b: usize) -> Option<f64> {
    let hop = ((HOP_S * yin.sr).round() as usize).max(1);
    let n = yin.frame_len();
    let (mut voiced, mut all) = (Vec::new(), Vec::new());
    let mut s = a;
    while s + n <= b.min(x.len()) {
        if let Some((f, v)) = yin.estimate(&x[s..s + n]) {
            all.push(f);
            if v {
                voiced.push(f);
            }
        }
        s += hop;
    }
    if !voiced.is_empty() {
        median(&mut voiced)
    } else {
        median(&mut all)
    }
}

/// Runs the note-wise check over a mono signal.
pub fn check(x: &[f64], sample_rate: u32, notes: &[Note]) -> PitchReport {
    let mut yin = Yin::new(sample_rate);
    let sr = sample_rate as f64;
    let mut out = Vec::new();
    let mut abs_c = Vec::new();
    let (mut within, mut octave) = (0usize, 0usize);
    for nt in notes {
        let d = nt.t1 - nt.t0;
        if !(d >= MIN_NOTE_S) || !nt.t0.is_finite() || !nt.midi.is_finite() {
            continue;
        }
        let a = ((nt.t0 + 0.2 * d) * sr).round().max(0.0) as usize;
        let b = ((nt.t1 - 0.2 * d) * sr).round().max(0.0) as usize;
        let f0 = segment_f0(&mut yin, x, a, b);
        let c = f0.map(|f| cents(f, nt.midi));
        let ok = c.is_some_and(|c| c.abs() <= TOL_CENTS);
        let oct = c.is_some_and(|c| (c.abs() - 1200.0).abs() <= TOL_CENTS);
        if ok {
            within += 1;
        }
        if oct {
            octave += 1;
        }
        if let Some(c) = c {
            abs_c.push(c.abs());
        }
        out.push(NoteResult {
            t0: nt.t0,
            t1: nt.t1,
            midi: nt.midi,
            f0,
            cents: c,
            octave_error: oct,
        });
    }
    let analysed = out.len();
    PitchReport {
        notes_total: notes.len(),
        notes_analysed: analysed,
        within_50c: within,
        fraction_within_50c: if analysed > 0 {
            within as f64 / analysed as f64
        } else {
            0.0
        },
        octave_errors: octave,
        median_abs_cents: median(&mut abs_c),
        notes: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sawtooth_220_within_2_cents() {
        let sr = 44100u32;
        let f = 220.0;
        let x: Vec<f64> = (0..sr as usize)
            .map(|i| {
                let ph = (f * i as f64 / sr as f64).fract();
                2.0 * ph - 1.0
            })
            .collect();
        let mut yin = Yin::new(sr);
        let f0 = segment_f0(&mut yin, &x, 4410, 39690).unwrap();
        let c = cents(f0, 57.0);
        assert!(c.abs() < 2.0, "f0 {f0} Hz, {c} cents");

        let r = check(
            &x,
            sr,
            &[
                Note {
                    t0: 0.1,
                    t1: 0.9,
                    midi: 57.0,
                },
                Note {
                    t0: 0.1,
                    t1: 0.2,
                    midi: 57.0,
                },
            ],
        );
        assert_eq!(r.notes_analysed, 1);
        assert_eq!(r.within_50c, 1);
        assert_eq!(r.octave_errors, 0);
    }

    #[test]
    fn octave_error_flagged() {
        let sr = 44100u32;
        let x: Vec<f64> = (0..sr as usize)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sr as f64).sin())
            .collect();
        let r = check(
            &x,
            sr,
            &[Note {
                t0: 0.1,
                t1: 0.9,
                midi: 57.0,
            }],
        );
        assert_eq!(r.within_50c, 0);
        assert_eq!(r.octave_errors, 1);
    }
}

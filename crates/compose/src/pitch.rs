//! Pitch: chooses a phrase's notes by second-order Viterbi.
//!
//! Model: a second-order hidden Markov model over the candidate pitches of
//! each note (scale tones of the local scale within a 20-semitone window).
//! The Viterbi state is the pair (previous, current), so a transition can
//! score three consecutive notes. Emissions score chord-tone fit on strong
//! and long notes, distance from the contour target, agreement with a
//! reference line, the cadence, and continuity with the previous phrase.
//! A tonic cadence over a chord that holds the tonic is a constraint: the
//! last note's candidates are the tonic alone.
//! Pair transitions score interval size (steps best, the tritone worst,
//! leaps by the profile's `leap`); triple transitions score leap recovery,
//! the gap-fill rule that a leap is followed by a step back (L. B. Meyer,
//! "Emotion and Meaning in Music", 1956), and penalise wobbles (a step and
//! straight back) and three repeated notes.
//!
//! Variety: perturb-and-MAP (Papandreou and Yuille, ICCV 2011); uniform
//! noise of width `PitchStyle::noise` on every emission, then the exact
//! MAP path. All arithmetic is relative to the register pitch, so the
//! chosen line transposes exactly with the key.

use sfcore::random::Rng;
use song::{Pc, PcSet};

use crate::contour::Contour;

/// How a phrase ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    /// No cadence rule on the last note beyond chord fit.
    None,
    /// Half cadence: the last note favours the 5th, 2nd, 3rd or 7th degree.
    Open,
    /// Full cadence: the last note favours the tonic.
    Tonic,
}

/// Per-song melodic character (from the melody profile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitchStyle {
    /// Leapiness, 0-1: raises the score of 3rds to octaves, lowers steps.
    pub leap: f64,
    /// Note repetition, 0-1: raises the score of a repeated pitch.
    pub rep: f64,
    /// Width of the uniform perturb-and-MAP noise on each emission.
    pub noise: f64,
}

impl Default for PitchStyle {
    /// The neutral style used for instrumental lines.
    fn default() -> Self {
        PitchStyle { leap: 0.3, rep: 0.3, noise: 0.7 }
    }
}

/// Score weights of the pitch model. Scores are log-domain; larger is better.
#[derive(Clone, Copy, Debug)]
pub struct PitchWeights {
    /// Candidate window around the register pitch, semitones.
    pub below: i32,
    pub above: i32,
    /// A note is strong at metric weight >= this or duration >= `strong_dur` beats.
    pub strong_weight: f64,
    pub strong_dur: f64,
    /// Chord tone (+) or not (-) on a strong note.
    pub strong_chord: f64,
    /// Chord tone on a weak note.
    pub weak_chord: f64,
    /// Non-chord tone lasting at least `long_dur` beats.
    pub long_dur: f64,
    pub long_nonchord: f64,
    /// Per semitone from the contour target.
    pub contour: f64,
    /// Same pitch as the reference line, and within a whole tone of it.
    pub ref_exact: f64,
    pub ref_near: f64,
    /// Tonic cadence: tonic, and non-chord tone.
    pub tonic_end: f64,
    pub tonic_miss: f64,
    /// Open cadence: chord tone on degree 5, 2, 3 or 7; and the tonic.
    pub open_end: f64,
    pub open_tonic: f64,
    /// No cadence: non-chord last note.
    pub plain_end_nonchord: f64,
    /// Per semitone from the previous phrase's last note, on the first note.
    pub prev_end: f64,
    /// First interval equal to the profile's hook interval.
    pub hook: f64,
    /// Leap recovery (Meyer 1956): after a leap of `leap_min` or more, a
    /// step (<= 2) back scores `recover`; going on the same way `continue_leap`.
    pub leap_min: i32,
    pub recover: f64,
    pub continue_leap: f64,
    /// A step and straight back.
    pub wobble: f64,
    /// Two steps the same way.
    pub run: f64,
    /// Three equal notes.
    pub repeat3: f64,
}

/// The pitch weights.
pub const PITCH_WEIGHTS: PitchWeights = PitchWeights {
    below: 5,
    above: 14,
    strong_weight: 0.7,
    strong_dur: 1.5,
    strong_chord: 2.4,
    weak_chord: 0.6,
    long_dur: 1.0,
    long_nonchord: -1.5,
    contour: 0.3,
    ref_exact: 1.3,
    ref_near: 0.3,
    tonic_end: 3.5,
    tonic_miss: -3.0,
    open_end: 1.5,
    open_tonic: -0.4,
    plain_end_nonchord: -1.5,
    prev_end: 0.12,
    hook: 2.2,
    leap_min: 5,
    recover: 0.9,
    continue_leap: -1.0,
    wobble: -0.75,
    run: 0.12,
    repeat3: -0.5,
};

/// Candidates per note: the window size.
const W: usize = (PITCH_WEIGHTS.below + PITCH_WEIGHTS.above + 1) as usize;
/// Largest interval inside the window.
const MAX_IV: usize = W - 1;
/// Side of the triple-transition table, indexed by interval + MAX_IV.
const T: usize = 2 * MAX_IV + 1;

/// One phrase's pitch problem. Slices have one entry per note.
#[derive(Clone, Copy, Debug)]
pub struct PitchProblem<'a> {
    /// Onsets and durations in beats from the phrase start.
    pub onsets: &'a [f64],
    pub durs: &'a [f64],
    /// Metric weight of each onset.
    pub weights: &'a [f64],
    /// Chord tones sounding at each onset.
    pub chord_pcs: &'a [PcSet],
    /// Local scale at each onset; candidates are its tones.
    pub scales: &'a [PcSet],
    /// Register pitch (MIDI): the tonic near middle C.
    pub register: i32,
    pub tonic: Pc,
    /// Contour axis in semitones above `register`.
    pub center: f64,
    pub contour: Contour,
    pub cadence: Cadence,
    /// A line to echo (MIDI), resampled to this phrase's length.
    pub reference: Option<&'a [i32]>,
    /// Last note of the previous phrase (MIDI).
    pub prev_end: Option<i32>,
    /// Phrase length in beats (for the contour position).
    pub line_beats: f64,
    pub style: PitchStyle,
    /// Preferred first interval in semitones; 0 for none.
    pub hook: i32,
}

/// Interval score by size in semitones, for a style.
fn interval_scores(style: &PitchStyle) -> [f64; W] {
    let lp = style.leap;
    let mut s = [0.0; W];
    for (d, x) in s.iter_mut().enumerate() {
        *x = match d {
            0 => -0.4 + 0.8 * style.rep,
            1 | 2 => 0.8 - 0.45 * lp,
            3 | 4 => 0.15 + 0.35 * lp,
            5 => -0.35 + 0.6 * lp,
            6 => -1.6,
            7 => -0.7 + 0.7 * lp,
            8 | 9 => -1.4 + 0.9 * lp,
            12 => -1.6 + 1.0 * lp,
            _ => -2.5 - (d as f64 - 7.0) * 0.4,
        };
    }
    s
}

/// Triple-transition score for intervals l = b - a and m = c - b.
fn triple(l: i32, m: i32, w: &PitchWeights) -> f64 {
    let mut s = 0.0;
    if l.abs() >= w.leap_min {
        if m.signum() == -l.signum() && m.abs() <= 2 {
            s += w.recover;
        } else if m.signum() == l.signum() {
            s += w.continue_leap;
        }
    } else if l != 0 && m == -l && l.abs() <= 2 {
        s += w.wobble;
    } else if l != 0 && m.signum() == l.signum() && l.abs() <= 2 && m.abs() <= 2 {
        s += w.run;
    }
    if l == 0 && m == 0 {
        s += w.repeat3;
    }
    s
}

/// Emission score of pitch `m` for note `i` of `p`, without noise.
fn emission(p: &PitchProblem, i: usize, m: i32, w: &PitchWeights) -> f64 {
    let n = p.onsets.len();
    let pc = Pc::new(m);
    let ct = p.chord_pcs[i].contains(pc);
    let strong = p.weights[i] >= w.strong_weight || p.durs[i] >= w.strong_dur;
    let mut s = if strong {
        if ct {
            w.strong_chord
        } else {
            -w.strong_chord
        }
    } else if ct {
        w.weak_chord
    } else {
        0.0
    };
    if p.durs[i] >= w.long_dur && !ct {
        s += w.long_nonchord;
    }
    let x = if p.line_beats > 0.0 { (p.onsets[i] / p.line_beats).clamp(0.0, 1.0) } else { 0.0 };
    s -= w.contour * ((m - p.register) as f64 - (p.center + p.contour.at(x))).abs();
    if let Some(r) = p.reference.filter(|r| !r.is_empty()) {
        let idx = (i as f64 * (r.len() - 1) as f64 / (n.max(2) - 1) as f64 + 0.5).floor() as usize;
        let rv = r[idx.min(r.len() - 1)];
        if m == rv {
            s += w.ref_exact;
        } else if (m - rv).abs() <= 2 {
            s += w.ref_near;
        }
    }
    if i + 1 == n {
        let deg = (pc.get() as i32 - p.tonic.get() as i32).rem_euclid(12);
        match p.cadence {
            Cadence::Tonic => {
                s += if deg == 0 {
                    w.tonic_end
                } else if ct {
                    0.0
                } else {
                    w.tonic_miss
                };
            }
            Cadence::Open => {
                if ct && matches!(deg, 7 | 2 | 4 | 11) {
                    s += w.open_end;
                }
                if deg == 0 {
                    s += w.open_tonic;
                }
            }
            Cadence::None => {
                if !ct {
                    s += w.plain_end_nonchord;
                }
            }
        }
    }
    if i == 0 {
        if let Some(pe) = p.prev_end {
            s -= w.prev_end * (m - pe).abs() as f64;
        }
    }
    s
}

/// The best-scoring phrase (MIDI per note) for `p`, perturbed by `rng`.
pub fn pitch_line(p: &PitchProblem, rng: &mut Rng) -> Vec<i32> {
    pitch_line_with(p, &PITCH_WEIGHTS, rng)
}

/// `pitch_line` with explicit weights (window size fixed by `PITCH_WEIGHTS`).
pub fn pitch_line_with(p: &PitchProblem, w: &PitchWeights, rng: &mut Rng) -> Vec<i32> {
    let n = p.onsets.len();
    debug_assert!(p.durs.len() == n && p.weights.len() == n && p.chord_pcs.len() == n && p.scales.len() == n);
    if n == 0 {
        return Vec::new();
    }
    let lo = p.register - PITCH_WEIGHTS.below;

    // Candidates and emissions, row-major n x W; `nc[i]` valid per row.
    let mut cand = vec![0i32; n * W];
    let mut em = vec![0.0f64; n * W];
    let mut nc = vec![0usize; n];
    // A tonic cadence over a chord that holds the tonic ends on the tonic.
    let tonic_only = p.cadence == Cadence::Tonic && p.chord_pcs[n - 1].contains(p.tonic);
    for i in 0..n {
        let mut k = 0;
        for m in lo..lo + W as i32 {
            if i + 1 == n && tonic_only && Pc::new(m) != p.tonic {
                continue;
            }
            if p.scales[i].contains(Pc::new(m)) {
                cand[i * W + k] = m;
                em[i * W + k] = emission(p, i, m, w) + (rng.uniform() - 0.5) * p.style.noise;
                k += 1;
            }
        }
        nc[i] = k;
    }
    if nc.contains(&0) {
        // An empty scale cannot come from a mode; keep the register.
        return vec![p.register; n];
    }

    if n == 1 {
        let row = &em[..nc[0]];
        let bi = argmax(row);
        return vec![cand[bi]];
    }

    let iv = interval_scores(&p.style);
    let mut tri = [0.0f64; T * T];
    for (li, l) in (-(MAX_IV as i32)..=MAX_IV as i32).enumerate() {
        for (mi, m) in (-(MAX_IV as i32)..=MAX_IV as i32).enumerate() {
            tri[li * T + mi] = triple(l, m, w);
        }
    }
    let ivs = |a: i32, b: i32| iv[(b - a).unsigned_abs() as usize];

    // dp[a * W + b]: best score of a path ending (cand[i-1][a], cand[i][b]).
    let mut dp = vec![f64::NEG_INFINITY; W * W];
    let mut nd = vec![f64::NEG_INFINITY; W * W];
    for a in 0..nc[0] {
        let ma = cand[a];
        for b in 0..nc[1] {
            let mb = cand[W + b];
            let hook = if p.hook != 0 && mb - ma == p.hook { w.hook } else { 0.0 };
            dp[a * W + b] = em[a] + em[W + b] + ivs(ma, mb) + hook;
        }
    }

    // bp[(i - 2) * W * W + b * W + c] = best a for the pair (b, c) at note i.
    let mut bp = vec![0u8; (n - 2) * W * W];
    for i in 2..n {
        let (ca, cb, cc) = (&cand[(i - 2) * W..], &cand[(i - 1) * W..], &cand[i * W..]);
        let bpi = &mut bp[(i - 2) * W * W..(i - 1) * W * W];
        for b in 0..nc[i - 1] {
            let mb = cb[b];
            for c in 0..nc[i] {
                let mc = cc[c];
                let mrow = (mc - mb + MAX_IV as i32) as usize;
                let mut best = f64::NEG_INFINITY;
                let mut arg = 0usize;
                for a in 0..nc[i - 2] {
                    let l = (mb - ca[a] + MAX_IV as i32) as usize;
                    let v = dp[a * W + b] + tri[l * T + mrow];
                    if v > best {
                        best = v;
                        arg = a;
                    }
                }
                nd[b * W + c] = best + ivs(mb, mc) + em[i * W + c];
                bpi[b * W + c] = arg as u8;
            }
        }
        std::mem::swap(&mut dp, &mut nd);
    }

    let (mut bb, mut bc, mut bv) = (0usize, 0usize, f64::NEG_INFINITY);
    for b in 0..nc[n - 2] {
        for c in 0..nc[n - 1] {
            if dp[b * W + c] > bv {
                bv = dp[b * W + c];
                bb = b;
                bc = c;
            }
        }
    }
    let mut idx = vec![0usize; n];
    idx[n - 1] = bc;
    idx[n - 2] = bb;
    for i in (2..n).rev() {
        idx[i - 2] = bp[(i - 2) * W * W + idx[i - 1] * W + idx[i]] as usize;
    }
    idx.iter().enumerate().map(|(i, &k)| cand[i * W + k]).collect()
}

/// Index of the first maximum.
fn argmax(xs: &[f64]) -> usize {
    let mut bi = 0;
    for (k, &x) in xs.iter().enumerate() {
        if x > xs[bi] {
            bi = k;
        }
    }
    bi
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contour::{Contour, ContourKind};
    use sfcore::random::tag;

    fn problem<'a>(
        onsets: &'a [f64],
        durs: &'a [f64],
        weights: &'a [f64],
        chords: &'a [PcSet],
        scales: &'a [PcSet],
        register: i32,
        cadence: Cadence,
    ) -> PitchProblem<'a> {
        PitchProblem {
            onsets,
            durs,
            weights,
            chord_pcs: chords,
            scales,
            register,
            tonic: Pc::new(register),
            center: 2.0,
            contour: Contour { kind: ContourKind::Arch, amp: 3.0, gain: 1.0, slope: 0.0 },
            cadence,
            reference: None,
            prev_end: None,
            line_beats: onsets.len() as f64,
            style: PitchStyle::default(),
            hook: 0,
        }
    }

    #[test]
    fn single_note() {
        let scales = [song::Mode::Major.scale()];
        let chords = [PcSet::from_intervals(Pc::C, &[0, 4, 7])];
        let p = problem(&[0.0], &[1.0], &[1.0], &chords, &scales, 60, Cadence::Tonic);
        let out = pitch_line(&p, &mut Rng::stream(1, tag("t")));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].rem_euclid(12), 0);
    }

    #[test]
    fn tonic_cadence_ends_on_the_tonic_and_lines_move_by_scale() {
        let scales = [song::Mode::Major.scale(); 6];
        let chords = [PcSet::from_intervals(Pc::C, &[0, 4, 7]); 6];
        let on = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let du = [1.0; 6];
        let wt = [1.0, 0.5, 0.8, 0.5, 1.0, 1.0];
        for k in 0..50 {
            let p = problem(&on, &du, &wt, &chords, &scales, 60, Cadence::Tonic);
            let out = pitch_line(&p, &mut Rng::event(2, tag("t"), k));
            assert_eq!(out.len(), 6);
            assert_eq!(out[5].rem_euclid(12), 0, "{out:?}");
            assert!(out.iter().all(|&m| scales[0].contains(Pc::new(m)) && (55..=74).contains(&m)));
        }
    }

    #[test]
    fn transposes_exactly() {
        for k in 0..20 {
            let on = [0.0, 0.5, 1.0, 2.0, 3.0];
            let du = [0.5, 0.5, 1.0, 1.0, 2.0];
            let wt = [1.0, 0.15, 0.5, 0.8, 0.5];
            let sc0 = [song::Mode::Dorian.scale(); 5];
            let ch0 = [PcSet::from_intervals(Pc::C, &[0, 3, 7]); 5];
            let sc5 = [song::Mode::Dorian.scale().transpose(5); 5];
            let ch5 = [PcSet::from_intervals(Pc::new(5), &[0, 3, 7]); 5];
            let a = pitch_line(&problem(&on, &du, &wt, &ch0, &sc0, 60, Cadence::Open), &mut Rng::event(3, tag("t"), k));
            let b = pitch_line(&problem(&on, &du, &wt, &ch5, &sc5, 53, Cadence::Open), &mut Rng::event(3, tag("t"), k));
            let d: Vec<i32> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
            assert!(d.iter().all(|&x| x == -7), "{a:?} {b:?}");
        }
    }
}

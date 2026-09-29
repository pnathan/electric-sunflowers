//! Helmholtz-motion sweep of the bowed string (design section 9), shared by
//! tests/violin.rs and examples/helmholtz.rs (included by `#[path]`, so the
//! analysis and its rayon use stay out of the library).
//!
//! A note holds Helmholtz motion when, by Goertzel magnitudes (Goertzel
//! 1958) over the analysis window, a(f0) > 0.35 a(2 f0), a(f0/2) < 0.1
//! a(f0) and a(1.5 f0) < 0.1 a(f0): the fundamental is present and there
//! is no period doubling or tripling.

use instruments::violin::render_violin;
use rayon::prelude::*;
use sfcore::math::mtof;
use sfcore::SR_F;
use song::events::BowNote;
use std::f64::consts::TAU;

/// Goertzel magnitude of `x` at `f` Hz.
pub fn goertzel(x: &[f32], f: f64) -> f64 {
    let w = TAU * f / SR_F;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt()
}

/// True when `seg` holds Helmholtz motion at `f0` Hz.
pub fn holds(seg: &[f32], f0: f64) -> bool {
    let a1 = goertzel(seg, f0);
    let a2 = goertzel(seg, 2.0 * f0);
    let ah = goertzel(seg, 0.5 * f0);
    let a15 = goertzel(seg, 1.5 * f0);
    a1 > 0.35 * a2 && ah < 0.1 * a1 && a15 < 0.1 * a1
}

/// Single-note sweep: MIDI 55..=90, velocity 0.4/0.6/0.85, seeds 1 and
/// 2; one note from 0.1 to 1.2 s in 1.6 s, seed `m*13 + sd*101 +
/// round(v*10)`, analysed over 0.45-1.0 s. Returns (stable, total,
/// failures as (midi, velocity)).
pub fn note_sweep() -> (usize, usize, Vec<(u32, f32)>) {
    let mut cases = Vec::new();
    for m in 55..=90u32 {
        for v in [0.4f32, 0.6, 0.85] {
            for sd in [1u64, 2] {
                cases.push((m, v, sd));
            }
        }
    }
    let len = (1.6 * SR_F).round() as usize;
    let (a, b) = (
        (0.45 * SR_F).round() as usize,
        (1.0 * SR_F).round() as usize,
    );
    let res: Vec<bool> = cases
        .par_iter()
        .map(|&(m, v, sd)| {
            let seed = m as u64 * 13 + sd * 101 + (v as f64 * 10.0).round() as u64;
            let note = BowNote {
                t0: 0.1,
                t1: 1.2,
                midi: m as f32,
                vel: v,
                vibrato: true,
            };
            let x = render_violin(&[note], len, seed);
            holds(&x[a..b], mtof(m as f64))
        })
        .collect();
    let ok = res.iter().filter(|&&s| s).count();
    let bad = cases
        .iter()
        .zip(&res)
        .filter(|(_, &s)| !s)
        .map(|(&(m, v, _), _)| (m, v))
        .collect();
    (ok, cases.len(), bad)
}

/// Interval patterns (semitones from the first note) for the phrase sweep.
const PHRASE_SHAPES: [[i32; 4]; 6] = [
    [0, 2, 4, 5],
    [0, -3, 2, 7],
    [0, 5, 3, -2],
    [0, 7, 5, 0],
    [0, -1, -5, 2],
    [0, 4, 9, 7],
];
/// Note length and legato gap in the phrase sweep, seconds.
const PHRASE_NOTE: f64 = 0.5;
const PHRASE_GAP: f64 = 0.02;

/// The phrase-sweep notes for phrase `p` of 30: four legato notes of
/// 0.5 s from t = 0.1 s, the first pitch stepping through MIDI 57-83,
/// shapes from `PHRASE_SHAPES`, pitches kept in 55..=90, velocity
/// cycling 0.4/0.6/0.85.
pub fn phrase_notes(p: usize) -> [BowNote; 4] {
    let start = 57 + (p * 26 / 29) as i32;
    let shape = PHRASE_SHAPES[p % PHRASE_SHAPES.len()];
    let vel = [0.4f32, 0.6, 0.85][p % 3];
    let mut notes = [BowNote {
        t0: 0.0,
        t1: 0.0,
        midi: 0.0,
        vel,
        vibrato: true,
    }; 4];
    for (i, n) in notes.iter_mut().enumerate() {
        let t0 = 0.1 + i as f64 * PHRASE_NOTE;
        n.t0 = t0;
        n.t1 = t0 + PHRASE_NOTE - PHRASE_GAP;
        n.midi = (start + shape[i]).clamp(55, 90) as f32;
    }
    notes
}

/// Phrase sweep: 30 four-note legato phrases (`phrase_notes`), seed
/// 7000 + p. Each note is analysed from 0.15 s after its onset to its
/// end. Returns (stable notes, total notes, failures as (phrase, note,
/// midi)).
pub fn phrase_sweep() -> (usize, usize, Vec<(usize, usize, f32)>) {
    let len = ((0.1 + 4.0 * PHRASE_NOTE + 0.5) * SR_F).round() as usize;
    let res: Vec<Vec<bool>> = (0..30usize)
        .into_par_iter()
        .map(|p| {
            let notes = phrase_notes(p);
            let x = render_violin(&notes, len, 7000 + p as u64);
            notes
                .iter()
                .map(|n| {
                    let a = ((n.t0 + 0.15) * SR_F).round() as usize;
                    let b = (n.t1 * SR_F).round() as usize;
                    holds(&x[a..b], mtof(n.midi as f64))
                })
                .collect()
        })
        .collect();
    let mut ok = 0;
    let mut bad = Vec::new();
    for (p, r) in res.iter().enumerate() {
        for (i, &s) in r.iter().enumerate() {
            if s {
                ok += 1;
            } else {
                bad.push((p, i, phrase_notes(p)[i].midi));
            }
        }
    }
    (ok, 30 * 4, bad)
}

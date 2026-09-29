//! Choir voicing: a four-part chord per timeline segment by minimal voice
//! leading.
//!
//! Exhaustive search over bass, tenor, alto and soprano in `CHOIR_RANGE`:
//! the bass sings the chord's bass pitch class, the upper parts any chord
//! tone, strictly ascending, adjacent upper parts at most 9 semitones apart.
//! Cost (lower is better): motion from the previous voicing (bass weighted
//! 0.6, upper parts 1 per semitone), -2 per distinct pitch class, +5 without
//! the third, +3 for a spread above 26 semitones. The first voicing to beat
//! the best cost strictly wins, in ascending search order. The first
//! voicing leads from C3 G3 C4 G4.

use compose::form::{Form, Sec};
use compose::timeline::Timeline;
use song::{Pc, PcSet};

/// MIDI range searched per part, low to high: bass, tenor, alto, soprano.
pub const CHOIR_RANGE: [(u8, u8); 4] = [(40, 55), (48, 62), (55, 69), (60, 74)];

/// Largest interval between adjacent upper parts, semitones.
const MAX_GAP: i32 = 9;

/// One segment's voicing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoirVoicing {
    /// Index into `Timeline::segs`.
    pub seg: usize,
    /// Bass, tenor, alto, soprano, MIDI, ascending.
    pub notes: [i32; 4],
}

/// Candidate notes of one part, ascending.
struct Cands {
    m: [i32; 16],
    n: usize,
}

impl Cands {
    fn new(lo: u8, hi: u8, pcs: PcSet) -> Cands {
        let mut c = Cands { m: [0; 16], n: 0 };
        for m in pcs.tones_in(lo, hi) {
            if c.n < c.m.len() {
                c.m[c.n] = m as i32;
                c.n += 1;
            }
        }
        c
    }

    fn as_slice(&self) -> &[i32] {
        &self.m[..self.n]
    }
}

/// Voicings for every segment whose section passes `filter`. A segment with
/// no voicing that fits the rules is skipped.
pub fn voicings(form: &Form, tl: &Timeline, filter: impl Fn(&Sec) -> bool) -> Vec<ChoirVoicing> {
    let mut out = Vec::new();
    let mut prev = [48i32, 55, 60, 67];

    for (si, sg) in tl.segs.iter().enumerate() {
        if !filter(&form.sections[sg.sec]) {
            continue;
        }
        let chord = form.chord(sg.chord);
        let bass = Cands::new(
            CHOIR_RANGE[0].0,
            CHOIR_RANGE[0].1,
            PcSet::EMPTY.with(chord.bass),
        );
        let up: [Cands; 3] = std::array::from_fn(|p| {
            Cands::new(CHOIR_RANGE[p + 1].0, CHOIR_RANGE[p + 1].1, chord.tones)
        });

        let mut best: Option<[i32; 4]> = None;
        let mut best_cost = f64::INFINITY;
        for &b in bass.as_slice() {
            for &t in up[0].as_slice().iter().filter(|&&t| t > b) {
                for &a in up[1]
                    .as_slice()
                    .iter()
                    .filter(|&&a| a > t && a - t <= MAX_GAP)
                {
                    for &s in up[2]
                        .as_slice()
                        .iter()
                        .filter(|&&s| s > a && s - a <= MAX_GAP)
                    {
                        let v = [b, t, a, s];
                        let set: PcSet = v.iter().map(|&m| Pc::new(m)).collect();
                        let mut c = (b - prev[0]).abs() as f64 * 0.6
                            + (t - prev[1]).abs() as f64
                            + (a - prev[2]).abs() as f64
                            + (s - prev[3]).abs() as f64;
                        c -= set.len() as f64 * 2.0;
                        if chord.third.is_some_and(|th| !set.contains(th)) {
                            c += 5.0;
                        }
                        if s - b > 26 {
                            c += 3.0;
                        }
                        if c < best_cost {
                            best_cost = c;
                            best = Some(v);
                        }
                    }
                }
            }
        }
        let Some(v) = best else { continue };
        out.push(ChoirVoicing { seg: si, notes: v });
        prev = v;
    }
    out
}

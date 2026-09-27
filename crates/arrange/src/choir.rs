//! Port of the choir-voicing part of engine.js: `CHOIR_R` and
//! `choirVoicings` (engine.js lines ~824-842). This picks the four-part
//! close-position choir chord per timeline segment; the choir singers
//! themselves (per-voice detune/vowel/rendering) are outside this crate's
//! scope.

use compose::form::{Form, Sec};
use compose::timeline::Timeline;

/// `CHOIR_R`: the midi range searched for each of the four parts
/// (bass, tenor, alto, soprano).
pub const CHOIR_R: [(i32, i32); 4] = [(40, 55), (48, 62), (55, 69), (60, 74)];

/// One segment's chosen voicing (`{sg,v}` in JS `choirVoicings`).
#[derive(Clone, Copy, Debug)]
pub struct ChoirVoicing {
    /// index into `tl.segs`.
    pub seg_idx: usize,
    /// bass, tenor, alto, soprano midi notes, low to high.
    pub v: [i32; 4],
}

/// `choirVoicings(form,tl,filter)`: for every segment that passes `filter`,
/// picks the closest-motion four-part voicing (bass on the chord's bass
/// note, the other three any chord tone) subject to a max-9-semitone spread
/// between adjacent upper voices.
pub fn choir_voicings(form: &Form, tl: &Timeline, filter: impl Fn(&Sec) -> bool) -> Vec<ChoirVoicing> {
    let mut out = Vec::new();
    let mut prev = [48i32, 55, 60, 67];

    for (si, sg) in tl.segs.iter().enumerate() {
        let sec = &form.sections[sg.sec];
        if !filter(sec) {
            continue;
        }
        let pcs = &sg.chord.pcs;
        let mut cands: [Vec<i32>; 4] = Default::default();
        for (p, &(lo, hi)) in CHOIR_R.iter().enumerate() {
            let mut a = Vec::new();
            for m in lo..=hi {
                let ok = if p == 0 {
                    m.rem_euclid(12) == sg.chord.bass
                } else {
                    pcs.contains(&m.rem_euclid(12))
                };
                if ok {
                    a.push(m);
                }
            }
            cands[p] = a;
        }

        let mut best: Option<[i32; 4]> = None;
        let mut bs = 1e9f64;
        for &b in &cands[0] {
            for &t in &cands[1] {
                for &a in &cands[2] {
                    for &s in &cands[3] {
                        if !(b < t && t < a && a < s) {
                            continue;
                        }
                        if a - t > 9 || s - a > 9 {
                            continue;
                        }
                        let set: std::collections::HashSet<i32> =
                            [b, t, a, s].iter().map(|x| x.rem_euclid(12)).collect();
                        let mut sc = (b - prev[0]).abs() as f64 * 0.6
                            + (t - prev[1]).abs() as f64
                            + (a - prev[2]).abs() as f64
                            + (s - prev[3]).abs() as f64;
                        sc -= set.len() as f64 * 2.0;
                        if let Some(third) = sg.chord.third {
                            if !set.contains(&third) {
                                sc += 5.0;
                            }
                        }
                        if s - b > 26 {
                            sc += 3.0;
                        }
                        if sc < bs {
                            bs = sc;
                            best = Some([b, t, a, s]);
                        }
                    }
                }
            }
        }
        let best = match best {
            Some(v) => v,
            None => continue,
        };
        out.push(ChoirVoicing { seg_idx: si, v: best });
        prev = best;
    }
    out
}

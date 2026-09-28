//! Choir voicing: a four-part chord per timeline segment by minimal voice
//! leading. The singers themselves are planned and rendered in `engine`.

use compose::form::{Form, Sec};
use compose::timeline::Timeline;
use song::Pc;

/// `CHOIR_R`: the midi range searched for each of the four parts
/// (bass, tenor, alto, soprano).
pub const CHOIR_R: [(i32, i32); 4] = [(40, 55), (48, 62), (55, 69), (60, 74)];

/// One segment's chosen voicing.
#[derive(Clone, Copy, Debug)]
pub struct ChoirVoicing {
    /// index into `tl.segs`.
    pub seg_idx: usize,
    /// bass, tenor, alto, soprano midi notes, low to high.
    pub v: [i32; 4],
}

/// For every segment whose section passes `filter`,
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
        let chord = form.chord(sg.chord);
        let mut cands: [Vec<i32>; 4] = Default::default();
        for (p, &(lo, hi)) in CHOIR_R.iter().enumerate() {
            let mut a = Vec::new();
            for m in lo..=hi {
                let ok = if p == 0 { Pc::new(m) == chord.bass } else { chord.tones.contains(Pc::new(m)) };
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
                        let set: u16 = [b, t, a, s].iter().map(|x| 1u16 << x.rem_euclid(12)).fold(0, |acc, m| acc | m);
                        let mut sc = (b - prev[0]).abs() as f64 * 0.6
                            + (t - prev[1]).abs() as f64
                            + (a - prev[2]).abs() as f64
                            + (s - prev[3]).abs() as f64;
                        sc -= set.count_ones() as f64 * 2.0;
                        if let Some(third) = chord.third {
                            if set & (1 << third.get()) == 0 {
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

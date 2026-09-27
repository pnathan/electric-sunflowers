//! Timeline: maps beats to seconds (with a final ritard) and beats to chords.
//! Ports `buildTimeline` (engine.js).
//!
//! JS parity: `buildTimeline` returns closures `toTime`/`chordAt`/`beatDur`
//! over `T`/`nb`/`base`/`segs`/`end`; here these become methods on `Timeline`.

use crate::form::Form;
use crate::theory::Chord;
use sfcore::js::{clamp, pow};
use sfcore::TAIL;

/// One chord segment on the beat timeline (`segs` in JS).
#[derive(Clone, Debug)]
pub struct Seg {
    pub chord: Chord,
    pub b0: f64,
    pub b1: f64,
    pub sec: usize,
    pub bar: usize,
}

/// buildTimeline's return value.
#[derive(Clone, Debug)]
pub struct Timeline {
    pub t: Vec<f64>,
    pub nb: usize,
    pub base: f64,
    pub segs: Vec<Seg>,
    pub end: f64,
    bpb: i32,
    split: i32,
    nbars: usize,
}

impl Timeline {
    /// buildTimeline(form, tempo)
    pub fn new(form: &Form, tempo: f64) -> Timeline {
        let bpb = form.mi.bpb;
        let nb = form.bars.len() * bpb as usize;
        let base = 60.0 / tempo;
        let mut t = vec![0.0f64; nb + 1];
        t[0] = sfcore::LEAD_IN;
        let rit = nb as i64 - 2 * bpb as i64;
        for b in 0..nb {
            let f = if b as i64 >= rit {
                1.0 + 0.38 * pow((b as i64 - rit + 1) as f64 / (nb as i64 - rit) as f64, 1.4)
            } else {
                1.0
            };
            t[b + 1] = t[b] + base * f;
        }

        let mut segs: Vec<Seg> = Vec::new();
        for (bi, bar) in form.bars.iter().enumerate() {
            for (k, c) in bar.chords.iter().enumerate() {
                let b0 = (bi as i32 * bpb + if k != 0 { form.mi.split } else { 0 }) as f64;
                let b1 = if bar.chords.len() == 1 || k != 0 {
                    (bi as i32 * bpb + bpb) as f64
                } else {
                    (bi as i32 * bpb + form.mi.split) as f64
                };
                let merge = segs
                    .last()
                    .map(|last: &Seg| last.chord.name == c.name && last.b1 == b0 && last.sec == bar.sec)
                    .unwrap_or(false);
                if merge {
                    segs.last_mut().unwrap().b1 = b1;
                } else {
                    segs.push(Seg {
                        chord: c.clone(),
                        b0,
                        b1,
                        sec: bar.sec,
                        bar: bi,
                    });
                }
            }
        }

        let end = t[nb] + TAIL;
        Timeline {
            t,
            nb,
            base,
            segs,
            end,
            bpb,
            split: form.mi.split,
            nbars: form.bars.len(),
        }
    }

    /// toTime(beat)
    pub fn to_time(&self, beat: f64) -> f64 {
        let nb = self.nb as f64;
        if beat >= nb {
            return self.t[self.nb] + (beat - nb) * self.base * 1.38;
        }
        if beat <= 0.0 {
            return self.t[0] + beat * self.base;
        }
        let i = beat.floor() as usize;
        self.t[i] + (beat - i as f64) * (self.t[i + 1] - self.t[i])
    }

    /// chordAt(beat)
    pub fn chord_at<'a>(&self, form: &'a Form, beat: f64) -> &'a Chord {
        let bi = clamp((beat / self.bpb as f64).floor(), 0.0, (self.nbars - 1) as f64) as usize;
        let bar = &form.bars[bi];
        if bar.chords.len() == 1 {
            return &bar.chords[0];
        }
        if (beat - (bi as i32 * self.bpb) as f64) < self.split as f64 - 1e-6 {
            &bar.chords[0]
        } else {
            &bar.chords[1]
        }
    }

    /// beatDur(b)
    pub fn beat_dur(&self, b: f64) -> f64 {
        self.to_time(b + 1.0) - self.to_time(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timeline_basic() {
        let raw = json!({
            "key":"C","mode":"major","meter":"4/4","tempo":120,
            "sections":[
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":"C G"}]},
            ]
        });
        let song = crate::song::normalize_song(&raw).unwrap();
        let form = crate::form::build_form(&song, 0);
        let tl = Timeline::new(&form, song.tempo);
        assert_eq!(tl.t[0], sfcore::LEAD_IN);
        assert!(tl.end > tl.t[tl.nb]);
        assert!(!tl.segs.is_empty());
        let c = tl.chord_at(&form, 0.0);
        assert_eq!(c.root, 0);
    }
}

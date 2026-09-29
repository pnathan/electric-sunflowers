//! Timeline: maps beats to seconds (with a final ritard) and beats to chords.

use crate::form::Form;
use sfcore::TAIL;
use song::{Chord, ChordId};

/// Bars at the end of the song over which the ritard acts.
pub const RIT_BARS: usize = 2;
/// Ritard depth: the last beat of the song is `1 + RIT_DEPTH` times as long
/// as a beat at tempo.
pub const RIT_DEPTH: f64 = 0.38;
/// Ritard curve exponent: beat length is `1 + RIT_DEPTH x^RIT_CURVE` with x
/// rising from 0 to 1 over the ritard, so the slowing starts gently.
pub const RIT_CURVE: f64 = 1.4;

/// One chord segment on the beat timeline: consecutive beats of one chord
/// within one section.
#[derive(Clone, Debug)]
pub struct Seg {
    /// Index into `Form::chords`.
    pub chord: ChordId,
    pub b0: f64,
    pub b1: f64,
    pub sec: usize,
    pub bar: usize,
}

/// Beat times and chord segments of a form.
#[derive(Clone, Debug)]
pub struct Timeline {
    /// Seconds at each beat, `nb + 1` entries; `t[0]` is the lead-in.
    pub t: Vec<f64>,
    /// Beats in the song.
    pub nb: usize,
    /// Seconds per beat at tempo.
    pub base: f64,
    /// Chord segments in beat order; they tile `0..nb` without gaps.
    pub segs: Vec<Seg>,
    /// End of the song in seconds, including the tail.
    pub end: f64,
}

impl Timeline {
    /// Beat times at `tempo` beats per minute, slowed over the last
    /// `RIT_BARS` bars (beat length times `1 + RIT_DEPTH x^RIT_CURVE`), and
    /// the chord segments.
    pub fn new(form: &Form, tempo: f64) -> Timeline {
        let bpb = form.bpb() as usize;
        let split = form.split() as usize;
        let nb = form.bars.len() * bpb;
        let base = 60.0 / tempo;
        let rit_len = RIT_BARS * bpb;
        // First beat of the ritard; negative when the song is shorter.
        let rit = nb as i64 - rit_len as i64;
        let mut t = vec![0.0f64; nb + 1];
        t[0] = sfcore::LEAD_IN;
        for b in 0..nb {
            let k = b as i64 - rit;
            let f = if k >= 0 {
                1.0 + RIT_DEPTH * ((k + 1) as f64 / rit_len as f64).powf(RIT_CURVE)
            } else {
                1.0
            };
            t[b + 1] = t[b] + base * f;
        }

        let mut segs: Vec<Seg> = Vec::new();
        for (bi, bar) in form.bars.iter().enumerate() {
            let chords = bar.chords.as_slice();
            for (k, &c) in chords.iter().enumerate() {
                let bar0 = bi * bpb;
                let b0 = (bar0 + if k != 0 { split } else { 0 }) as f64;
                let b1 = (bar0
                    + if chords.len() == 1 || k != 0 {
                        bpb
                    } else {
                        split
                    }) as f64;
                match segs.last_mut() {
                    Some(last) if last.chord == c && last.b1 == b0 && last.sec == bar.sec => {
                        last.b1 = b1
                    }
                    _ => segs.push(Seg {
                        chord: c,
                        b0,
                        b1,
                        sec: bar.sec,
                        bar: bi,
                    }),
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
        }
    }

    /// Seconds at `beat`; past the end, beats keep the final ritard length.
    pub fn to_time(&self, beat: f64) -> f64 {
        let nb = self.nb as f64;
        if beat >= nb {
            return self.t[self.nb] + (beat - nb) * self.base * (1.0 + RIT_DEPTH);
        }
        if beat <= 0.0 {
            return self.t[0] + beat * self.base;
        }
        let i = beat.floor() as usize;
        self.t[i] + (beat - i as f64) * (self.t[i + 1] - self.t[i])
    }

    /// Beat at `t` seconds, the inverse of `to_time`: binary search on `t`
    /// for the enclosing beat, then linear interpolation inside it. Before
    /// the start and past the end the same linear rule as `to_time` applies
    /// (the final ritard's beat length holds past the end), so `to_beat` is
    /// a true inverse everywhere `to_time` is defined.
    pub fn to_beat(&self, t: f64) -> f64 {
        let nb = self.nb;
        if t >= self.t[nb] {
            return nb as f64 + (t - self.t[nb]) / (self.base * (1.0 + RIT_DEPTH));
        }
        if t <= self.t[0] {
            return (t - self.t[0]) / self.base;
        }
        // self.t is nondecreasing; find i with t[i] <= t <= t[i+1].
        let i = self
            .t
            .partition_point(|&x| x <= t)
            .saturating_sub(1)
            .min(nb - 1);
        let (a, b) = (self.t[i], self.t[i + 1]);
        let frac = if b > a { (t - a) / (b - a) } else { 0.0 };
        i as f64 + frac
    }

    /// Index into `segs` of the segment sounding at `beat`, by binary
    /// search; beats before the song take the first segment, beats after
    /// it the last. A normalised song has at least one bar, so `segs` is
    /// not empty.
    pub fn seg_at(&self, beat: f64) -> usize {
        debug_assert!(!self.segs.is_empty(), "timeline without segments");
        self.segs
            .partition_point(|s| s.b1 <= beat)
            .min(self.segs.len().saturating_sub(1))
    }

    /// The chord sounding at `beat` (clamped to the song). `form` is the
    /// form this timeline was built from; the id indexes `form.chords`.
    pub fn chord_id_at(&self, _form: &Form, beat: f64) -> ChordId {
        self.segs[self.seg_at(beat)].chord
    }

    /// The chord sounding at `beat` (clamped to the song).
    pub fn chord_at<'a>(&self, form: &'a Form, beat: f64) -> &'a Chord {
        form.chord(self.chord_id_at(form, beat))
    }

    /// Length in seconds of the beat starting at `b`.
    pub fn beat_dur(&self, b: f64) -> f64 {
        self.to_time(b + 1.0) - self.to_time(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn form_of(v: serde_json::Value) -> (song::Song, Form) {
        let song = song::normalize_value(&v).unwrap().0;
        let form = crate::form::build_form(&song, 0);
        (song, form)
    }

    #[test]
    fn timeline_basic() {
        let (song, form) = form_of(json!({
            "key":"C","mode":"major","meter":"4/4","tempo":120,
            "sections":[
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
            ]
        }));
        let tl = Timeline::new(&form, song.tempo_bpm);
        assert_eq!(tl.t[0], sfcore::LEAD_IN);
        assert!(tl.end > tl.t[tl.nb]);
        assert!(!tl.segs.is_empty());
        let c = tl.chord_at(&form, 0.0);
        assert_eq!(c.root.get(), 0);
    }

    #[test]
    fn ritard_slows_the_last_bars() {
        let (song, form) = form_of(json!({
            "key":"C","meter":"4/4","tempo":120,
            "sections":[{"type":"verse","lines":[{"syl":"*a b *c d","chords":["C","F","G","C"]}]}]
        }));
        let tl = Timeline::new(&form, song.tempo_bpm);
        let nb = tl.nb;
        assert_eq!(nb, 16);
        // Beats before the ritard are at tempo; the last is 1 + RIT_DEPTH long.
        assert!((tl.beat_dur(0.0) - 0.5).abs() < 1e-12);
        assert!((tl.beat_dur(7.0) - 0.5).abs() < 1e-12);
        assert!(tl.beat_dur(8.0) > 0.5);
        assert!((tl.beat_dur((nb - 1) as f64) - 0.5 * (1.0 + RIT_DEPTH)).abs() < 1e-12);
        for b in 8..nb {
            assert!(tl.beat_dur(b as f64) > tl.beat_dur(b as f64 - 1.0) - 1e-12);
        }
    }

    #[test]
    fn to_beat_is_the_inverse_of_to_time() {
        let (song, form) = form_of(json!({
            "key":"C","meter":"4/4","tempo":120,
            "sections":[{"type":"verse","lines":[{"syl":"*a b *c d","chords":["C","F","G","C"]}]}]
        }));
        let tl = Timeline::new(&form, song.tempo_bpm);
        let nb = tl.nb as f64;
        let mut b = -4.0;
        while b < nb + 4.0 {
            let t = tl.to_time(b);
            let back = tl.to_beat(t);
            assert!((back - b).abs() < 1e-9, "beat {b} -> t {t} -> {back}");
            b += 0.1;
        }
    }

    #[test]
    fn chord_lookup_matches_the_bar_grid() {
        let (song, form) = form_of(json!({
            "key":"C","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"*a b *c d","chords":["C G","Am","F C"]}]}]
        }));
        let tl = Timeline::new(&form, song.tempo_bpm);
        let root = |b: f64| tl.chord_at(&form, b).root.get();
        let bpb = form.bpb() as f64;
        let split = form.split() as f64;
        let bars = form.bars.len() as f64;
        let mut b = -1.0;
        while b < bars * bpb + 2.0 {
            let bi = (b / bpb).floor().clamp(0.0, bars - 1.0) as usize;
            let bar = &form.bars[bi];
            let id = if bar.chords.len() == 1 || b - bi as f64 * bpb < split {
                bar.chords.first()
            } else {
                bar.chords.last()
            };
            assert_eq!(root(b), form.chord(id).root.get(), "beat {b}");
            b += 0.25;
        }
    }
}

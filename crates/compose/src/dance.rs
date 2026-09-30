//! Dance tunes: the instrumental lead of fiddle-dance styles. Where the
//! plain composer sets 4-6 notes per two bars, a dance tune runs on the
//! grid, as a reel (4/4, eighths), a jig (6/8, triplet eighths) or a
//! waltz or mazurka (3/4, eighths) does in a session.
//!
//! Each beat opens on a chord tone near the previous note, in the running
//! direction; the rest of the beat is a scale run or an arpeggio in that
//! direction (a 2-in-3 draw). The direction turns at the edges of the range
//! and with probability 0.3 per beat, so the line moves in waves. A beat
//! other than the last may open with a cut: two sixteenths, the upper scale
//! neighbour and the chord tone (probability 0.2). Two-bar chunks pair as
//! question and answer: the answer repeats the question's first bar and
//! goes its own way in the second. The last beat of a section is one long
//! note: the tonic in an outro, else a chord tone.
//!
//! Randomness: chunk k of section s draws from `Rng::event(seed, DANCE,
//! s << 21 | k)`; bar one of an answer replays its question's stream.

use sfcore::random::{tag, Rng, Tag};
use song::{Meter, Pc, PcSet, SectionKind, Song};

use crate::form::Form;
use crate::melody::InstNote;
use crate::theory::local_scale;
use crate::timeline::Timeline;

const DANCE: Tag = tag("inst.dance");

/// Range of a dance tune, MIDI (D4 to B5: first position and a reach).
const LO: i32 = 62;
const HI: i32 = 83;
/// Probability that a beat turns the running direction.
const TURN_P: f64 = 0.3;
/// Probability that a beat opens with a cut.
const CUT_P: f64 = 0.2;

/// Styles whose breaks are dance tunes.
const DANCE_STYLES: &[&str] = &[
    "irishpub",
    "oldtime",
    "bluegrass",
    "acadian",
    "cajun",
    "zydeco",
    "breton",
];

/// Whether `song`'s instrumental breaks are dance tunes: true for a song
/// with a fiddle-dance style applied.
pub fn is_dance(song: &Song) -> bool {
    song.style
        .as_deref()
        .is_some_and(|s| DANCE_STYLES.contains(&s))
}

/// Grid steps per beat for `meter`.
fn steps_per_beat(meter: Meter) -> i32 {
    match meter {
        Meter::Six8 => 3,
        Meter::Four4 | Meter::Three4 => 2,
    }
}

/// The nearest MIDI note to `from` in `set`, strictly beyond it in
/// direction `dir` when `strict`, within the range; `None` if there is none.
fn step_in(set: PcSet, from: i32, dir: i32, strict: bool) -> Option<i32> {
    let mut m = if strict { from + dir } else { from };
    while (LO..=HI).contains(&m) {
        if set.contains(Pc::new(m)) {
            return Some(m);
        }
        m += dir;
    }
    None
}

/// The chord tone of `set` nearest `from`, ties upward.
fn nearest(set: PcSet, from: i32) -> i32 {
    set.tones_in(LO as u8, HI as u8)
        .map(|m| m as i32)
        .min_by_key(|&m| ((m - from).abs(), -m))
        .unwrap_or(from.clamp(LO, HI))
}

struct Tune<'a> {
    form: &'a Form,
    tl: &'a Timeline,
    tonic: Pc,
    mode: song::Mode,
    lift: bool,
    out: Vec<InstNote>,
    /// Last pitch and running direction.
    p: i32,
    dir: i32,
}

impl Tune<'_> {
    fn harmony(&self, beat: f64) -> (PcSet, PcSet) {
        let c = self.tl.chord_at(self.form, beat + 0.01);
        (c.tones, local_scale(self.tonic, self.mode, c))
    }

    fn note(&mut self, beat: f64, dur: f64, midi: i32) {
        self.out.push(InstNote {
            beat,
            dur,
            midi,
            lift: self.lift,
        });
        self.p = midi;
    }

    /// One beat of running notes starting at `b`, `n` grid steps.
    fn beat(&mut self, rng: &mut Rng, b: f64, n: i32, cut_ok: bool) {
        let (tones, scale) = self.harmony(b);
        if rng.uniform() < TURN_P {
            self.dir = -self.dir;
        }
        if self.p + 4 * self.dir > HI || self.p + 4 * self.dir < LO {
            self.dir = -self.dir;
        }
        let head = step_in(tones, self.p, self.dir, true).unwrap_or_else(|| nearest(tones, self.p));
        let step = 1.0 / n as f64;
        if cut_ok && rng.uniform() < CUT_P {
            let upper = step_in(scale, head, 1, true).unwrap_or(head);
            self.note(b, step / 2.0, upper);
            self.note(b + step / 2.0, step / 2.0, head);
        } else {
            self.note(b, step, head);
        }
        let set = if rng.below(3) < 2 { scale } else { tones };
        for i in 1..n {
            let m = step_in(set, self.p, self.dir, true).unwrap_or_else(|| {
                self.dir = -self.dir;
                step_in(set, self.p, self.dir, true).unwrap_or(self.p)
            });
            self.note(b + i as f64 * step, step, m);
        }
    }
}

/// Dance-tune lead lines for sections without lyrics.
pub fn compose(form: &Form, tl: &Timeline, seed: u64) -> Vec<crate::melody::InstNote> {
    let bpb = form.bpb();
    let n = steps_per_beat(form.meter);
    let cb = 2 * form.stretch.max(1) as usize;
    let mut out = Vec::new();
    for (si, s) in form.sections.iter().enumerate() {
        if s.is_sung() || s.n_bars < cb {
            continue;
        }
        let mut t = Tune {
            form,
            tl,
            tonic: s.key.0,
            mode: s.key.1,
            lift: s.is_lift(),
            out: Vec::new(),
            p: s.key.0.get() as i32 + 72,
            dir: 1,
        };
        while t.p > 76 {
            t.p -= 12;
        }
        let chunks = s.n_bars / cb;
        let beats_per_chunk = cb as i32 * bpb;
        let half = beats_per_chunk / 2;
        let mut question_start = (t.p, t.dir);
        for k in 0..chunks {
            let b0 = ((s.start_bar + cb * k) as i32 * bpb) as f64;
            let q = k - k % 2;
            if k % 2 == 0 {
                question_start = (t.p, t.dir);
            } else {
                (t.p, t.dir) = question_start;
            }
            let mut rq = Rng::event(seed, DANCE, ((si as u64) << 21) | q as u64);
            let mut ra = Rng::event(seed, DANCE, ((si as u64) << 21) | k as u64 | 1 << 20);
            let last_chunk = k + 1 == chunks;
            for bi in 0..beats_per_chunk {
                let b = b0 + bi as f64;
                let rng = if bi < half { &mut rq } else { &mut ra };
                if last_chunk && bi + 1 == beats_per_chunk {
                    let (tones, _) = t.harmony(b);
                    let target = if s.kind == SectionKind::Outro {
                        PcSet::EMPTY.with(t.tonic)
                    } else {
                        tones
                    };
                    let m = nearest(target, t.p);
                    t.note(b, 1.0, m);
                } else {
                    let cut_ok = bi + 1 < beats_per_chunk;
                    t.beat(rng, b, n, cut_ok);
                }
            }
        }
        out.extend(t.out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(meter: &str, style: Option<&str>) -> Song {
        let v = json!({
            "key":"D","mode":"mixolydian","meter":meter,"tempo":120,"title":"t",
            "sections":[
                {"type":"intro","chords":["D","C","D","D"]},
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":["D C"]}]},
                {"type":"outro","chords":["D","A","D","D"]}
            ]
        });
        let mut s = song::normalize_value(&v).unwrap().0;
        s.style = style.map(str::to_string);
        s
    }

    #[test]
    fn only_dance_styles_dance() {
        assert!(is_dance(&song("4/4", Some("irishpub"))));
        assert!(!is_dance(&song("4/4", Some("gospel"))));
        assert!(!is_dance(&song("4/4", None)));
    }

    #[test]
    fn tunes_run_on_the_grid_in_range() {
        for (meter, step) in [("4/4", 0.5), ("6/8", 1.0 / 3.0), ("3/4", 0.5)] {
            let s = song(meter, Some("irishpub"));
            let form = crate::form::build_form(&s, 0);
            let tl = Timeline::new(&form, s.tempo_bpm);
            let a = compose(&form, &tl, 9);
            let b = compose(&form, &tl, 9);
            assert!(
                a.iter()
                    .zip(&b)
                    .all(|(x, y)| x.midi == y.midi && x.beat == y.beat),
                "{meter}"
            );
            let short = a.iter().filter(|n| n.dur <= step + 1e-9).count();
            assert!(short * 10 >= a.len() * 9, "{meter}: {short}/{}", a.len());
            assert!(a.iter().any(|n| n.dur < step - 1e-9), "{meter}: no cuts");
            assert!(a.iter().all(|n| (LO..=HI).contains(&n.midi)), "{meter}");
            let tonic = a.last().unwrap().midi.rem_euclid(12);
            assert_eq!(tonic, 2, "{meter}: outro ends on D");
        }
    }
}

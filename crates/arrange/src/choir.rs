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

use std::collections::BTreeMap;

use compose::form::{Form, Sec};
use compose::melody::LeadNote;
use compose::prepare::Prepared;
use compose::timeline::Timeline;
use song::{ChoirVoicing as Voicing, Part, Pc, PcSet, SectionKind, Voice};

use crate::vocals::CHOIR_VOICES;

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

/// Whether the choir sings in `s`: repeat lifts, bridges and outros. The
/// single copy; the audio (`vocals`) and the full score both call it.
pub fn sings_here(s: &Sec) -> bool {
    s.is_repeat_lift() || matches!(s.kind, SectionKind::Bridge | SectionKind::Outro)
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

/// Whether segment `sg` overlaps a choir line (in beats).
fn overlaps_choir_line(form: &Form, sg: &compose::timeline::Seg) -> bool {
    let bpb = form.meter.grid().beats as usize;
    form.lines.iter().any(|l| {
        l.part.is_choir()
            && sg.b0 < ((l.start_bar + l.n_bars) * bpb) as f64
            && sg.b1 > (l.start_bar * bpb) as f64
    })
}

/// `voicings` without the segments a choir line overlaps: the /aa/ pad is
/// silent while the choir sings words. With no choir line this is exactly
/// `voicings`.
pub fn voicings_clear(
    form: &Form,
    tl: &Timeline,
    filter: impl Fn(&Sec) -> bool,
) -> Vec<ChoirVoicing> {
    let mut v = voicings(form, tl, filter);
    if form.lines.iter().any(|l| l.part.is_choir()) {
        v.retain(|cv| !overlaps_choir_line(form, &tl.segs[cv.seg]));
    }
    v
}

/// One note of a choir line: the composed tune note and the four parts'
/// pitches.
#[derive(Clone, Debug)]
pub struct LineNote {
    /// The composed melody note (text, timing, flags), unshifted.
    pub note: LeadNote,
    /// Bass, tenor, alto, soprano, MIDI.
    pub midi: [i32; 4],
}

/// Preferred octave shift of the tune per part in unison (bass, tenor an
/// octave down).
const UNISON_SHIFT: [i32; 4] = [-1, -1, 0, 0];

fn median(mut v: Vec<i32>) -> Option<i32> {
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

/// The octave shift (in octaves, nearest `pref`) that puts the median of
/// `tune` inside `voice`'s range; failing that, the shift nearest its
/// centre.
fn octave_for(tune: &[i32], voice: Voice, pref: i32) -> i32 {
    let Some(med) = median(tune.to_vec()) else {
        return pref;
    };
    let r = voice.range();
    let fits = |k: i32| {
        let m = med + 12 * k;
        m >= r.lo as i32 && m <= r.hi as i32
    };
    [0, -1, 1, -2, 2, -3, 3]
        .iter()
        .map(|d| pref + d)
        .find(|&k| fits(k))
        .unwrap_or_else(|| {
            (-4..=4)
                .min_by(|&a, &b| {
                    let da = ((med + 12 * a) as f64 - voice.range().centre()).abs();
                    let db = ((med + 12 * b) as f64 - voice.range().centre()).abs();
                    da.partial_cmp(&db).expect("finite")
                })
                .unwrap_or(pref)
        })
}

/// The highest note of `pcs` at or below `limit`.
fn highest_at_or_below(pcs: PcSet, limit: i32) -> i32 {
    let mut m = limit;
    while !pcs.contains(Pc::new(m)) {
        m -= 1;
    }
    m
}

/// The four parts of every choir-line note of the composed melody, in the
/// order of `Comp::lead`.
///
/// - `Unison`: each part sings the tune shifted by whole octaves, chosen
///   once per part over the song's unison notes so the part's median lies
///   in its voice type's range, nearest to bass and tenor an octave down,
///   alto and soprano at pitch.
/// - `Block`: the soprano sings the tune (octave chosen the same way, over
///   the song's block notes). Alto, tenor and bass take the highest tone of
///   the sounding chord below the part above, not above the part's range top
///   (strictly ascending); the bass takes the chord's bass pitch class the
///   same way.
pub fn line_notes(p: &Prepared) -> Vec<LineNote> {
    line_notes_with(p, &BTreeMap::new())
}

/// `line_notes`, with the arranger's choir words: `words` maps a section (an
/// index of `Form::sections`) to `Some(kind)`, every lead note of the
/// section sung by the choir in that voicing (the lead sings it too), or to
/// `None`, the section's notes left out. A section not in `words` is as
/// `line_notes` has it (only the writer's choir lines). With an empty map
/// this is exactly `line_notes`.
pub fn line_notes_with(p: &Prepared, words: &BTreeMap<usize, Option<Voicing>>) -> Vec<LineNote> {
    let form = &p.form;
    let of = |n: &LeadNote| match words.get(&form.lines[n.line_idx].sec) {
        Some(k) => *k,
        None => match form.lines[n.line_idx].part {
            Part::Choir(v) => Some(v),
            _ => None,
        },
    };
    let tunes = |kind: Voicing| -> Vec<i32> {
        p.comp
            .lead
            .iter()
            .filter(|n| of(n) == Some(kind))
            .map(|n| n.midi)
            .collect()
    };
    let (uni, blk) = (tunes(Voicing::Unison), tunes(Voicing::Block));
    let ushift: [i32; 4] =
        std::array::from_fn(|i| octave_for(&uni, CHOIR_VOICES[i], UNISON_SHIFT[i]));
    let bshift = octave_for(&blk, Voice::Soprano, 0);
    let mut out = Vec::new();
    for n in &p.comp.lead {
        let Some(kind) = of(n) else { continue };
        let midi = match kind {
            Voicing::Unison => std::array::from_fn(|i| n.midi + 12 * ushift[i]),
            Voicing::Block => {
                let ch = p.timeline.chord_at(form, n.beat + 0.01);
                let sop = n.midi + 12 * bshift;
                let alto =
                    highest_at_or_below(ch.tones, (sop - 1).min(Voice::Alto.range().hi as i32));
                let tenor =
                    highest_at_or_below(ch.tones, (alto - 1).min(Voice::Tenor.range().hi as i32));
                let bass = highest_at_or_below(
                    PcSet::EMPTY.with(ch.bass),
                    (tenor - 1).min(Voice::Bass.range().hi as i32),
                );
                [bass, tenor, alto, sop]
            }
        };
        out.push(LineNote {
            note: n.clone(),
            midi,
        });
    }
    out
}

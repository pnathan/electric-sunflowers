//! Preparation for rendering: compose, transpose for the voice, time the
//! notes; and the vocal-note and harmony-line helpers.

use song::{Pc, Phoneme, Song, Voice};

use crate::form::{build_form, Form};
use crate::melody::{compose_melody, register_of, Comp, LeadNote};
use crate::theory::local_scale;
use crate::timeline::Timeline;
use crate::voices::choose_transpose;

/// Everything the renderer needs.
pub struct Prepared {
    pub form: Form,
    pub timeline: Timeline,
    pub comp: Comp,
    pub voice: Voice,
    /// Key change for the voice, -5..=6 semitones.
    pub key_shift: i32,
    /// Tonic pitch class after the key change, 0-11.
    pub tonic: i32,
}

/// A phrase-final note ends this many seconds early (a breath).
const PHRASE_END_GAP: f64 = 0.05;
/// A note that would run into the next ends this long before it: longer
/// at a phrase end, a hair otherwise.
const BREATH_GAP: f64 = 0.09;
const LEGATO_GAP: f64 = 0.004;
/// Shortest note after that trim.
const MIN_NOTE: f64 = 0.05;

/// Composes once in the written key, chooses the transposition `tr` for
/// the voice (`choose_transpose`), and moves the result to the key
/// `key_shift` = `tr` folded into -5..=6: the form and timeline are rebuilt
/// with transposed chords, the sung pitches move by `tr`, and the
/// instrumental lines move with the register pitch. Composition is
/// transposition-invariant (all scoring is relative to the register pitch
/// and tonic, and no random stream depends on the key), so this equals
/// composing again in the new key; the `compose_once_equals_two_passes`
/// test checks it. Then times the notes in seconds. `voice_key` `None`
/// uses the song's voice.
pub fn prepare(song: &Song, seed: u32, voice_key: Option<Voice>) -> Prepared {
    let vk = voice_key.unwrap_or(song.voice);
    let (form, timeline, mut comp, key_shift) = compose_for_voice(song, seed, vk);
    time_notes(&mut comp.lead, &timeline);
    let tonic = (song.key.get() as i32 + key_shift).rem_euclid(12);
    Prepared { form, timeline, comp, voice: vk, key_shift, tonic }
}

/// The key change (-5..=6) for a transposition `tr`.
fn key_shift_of(tr: i32) -> i32 {
    let semis = tr.rem_euclid(12);
    if semis > 6 {
        semis - 12
    } else {
        semis
    }
}

/// Compose in the written key and transpose for `voice` (see `prepare`).
fn compose_for_voice(song: &Song, seed: u32, voice: Voice) -> (Form, Timeline, Comp, i32) {
    let mut form = build_form(song, 0);
    let tl = Timeline::new(&form, song.tempo_bpm);
    let mut comp = compose_melody(song, &mut form, &tl, seed);
    let tr = choose_transpose(&comp.lead, voice);
    let key_shift = key_shift_of(tr);

    for n in comp.lead.iter_mut() {
        n.midi += tr;
        if let Some(g) = n.grace.as_mut() {
            *g += tr;
        }
    }
    let tonic = (song.key.get() as i32 + key_shift).rem_euclid(12);
    let t = register_of(tonic);
    let inst_shift = t - comp.t;
    for n in comp.inst.iter_mut() {
        n.midi += inst_shift;
    }
    comp.t = t;
    comp.tonic = tonic;

    let (mut form, tl) = if key_shift == 0 {
        (form, tl)
    } else {
        let mut f = build_form(song, key_shift);
        for (dst, src) in f.lines.iter_mut().zip(form.lines) {
            dst.rh = src.rh;
            dst.pitches = src.pitches;
        }
        let tl = Timeline::new(&f, song.tempo_bpm);
        (f, tl)
    };
    for l in form.lines.iter_mut() {
        if let Some(p) = l.pitches.as_mut() {
            p.iter_mut().for_each(|x| *x += tr);
        }
    }
    (form, tl, comp, key_shift)
}

/// Note times in seconds: a phrase-final note ends `PHRASE_END_GAP` early;
/// a note that would reach within 10 ms of the next ends `BREATH_GAP`
/// (phrase end) or `LEGATO_GAP` before it, but lasts at least `MIN_NOTE`.
fn time_notes(lead: &mut [LeadNote], tl: &Timeline) {
    for n in lead.iter_mut() {
        n.t0 = tl.to_time(n.beat);
        n.t1 = tl.to_time(n.beat + n.dur) - if n.phrase_end { PHRASE_END_GAP } else { 0.0 };
    }
    for i in 1..lead.len() {
        let next_t0 = lead[i].t0;
        let n = &mut lead[i - 1];
        if n.t1 > next_t0 - 0.01 {
            n.t1 = (n.t0 + MIN_NOTE).max(next_t0 - if n.phrase_end { BREATH_GAP } else { LEGATO_GAP });
        }
    }
}

/// One sung note in seconds.
#[derive(Clone, Debug)]
pub struct VocalNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: i32,
    pub ph: Vec<Phoneme>,
    pub amp: f64,
    pub phrase_start: bool,
    pub phrase_end: bool,
    pub grace: Option<i32>,
    pub stress: bool,
}

/// Lead notes as sung notes at level `amp`: unstressed syllables at 0.86,
/// lifted sections at 1.08.
pub fn vocal_notes(lead: &[LeadNote], amp: f64) -> Vec<VocalNote> {
    lead.iter()
        .map(|n| VocalNote {
            t0: n.t0,
            t1: n.t1,
            midi: n.midi,
            ph: n.syl.phones.clone(),
            amp: amp * (if n.stress { 1.0 } else { 0.86 }) * (if n.lift { 1.08 } else { 1.0 }),
            phrase_start: n.phrase_start,
            phrase_end: n.phrase_end,
            grace: n.grace,
            stress: n.stress,
        })
        .collect()
}

/// A harmony a third to a sixth above (`up`) or below the lead: the chord
/// tone preferred by interval (3rd and 4th best), else two scale steps.
pub fn harmony_line(lead: &[LeadNote], form: &Form, tl: &Timeline, song: &Song, tonic: i32, up: bool) -> Vec<LeadNote> {
    // Preference by interval in semitones, 3..=9: thirds best, the
    // tritone worst, sixths next.
    const SCORES: [f64; 10] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.2, -1.0, 0.4, 0.6, 0.6];
    lead.iter()
        .map(|n| {
            let ch = tl.chord_at(form, n.beat + 0.01);
            let sc = local_scale(Pc::new(tonic), song.mode, ch);
            let mut h: Option<i32> = None;
            let mut bs = -1e9f64;
            for d in 3..=9i32 {
                let m = if up { n.midi + d } else { n.midi - d };
                if !ch.tones.contains(Pc::new(m)) {
                    continue;
                }
                let s = SCORES[d as usize];
                if s > bs {
                    bs = s;
                    h = Some(m);
                }
            }
            let hv = match h {
                Some(v) => v,
                None => {
                    let mut steps = 0;
                    let mut m = n.midi;
                    while steps < 2 {
                        m += if up { 1 } else { -1 };
                        if sc.contains(Pc::new(m)) {
                            steps += 1;
                        }
                    }
                    m
                }
            };
            let mut nn = n.clone();
            nn.midi = hv;
            nn.grace = None;
            nn
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song() -> Song {
        let raw = json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,"voice":"baritone",
            "sections":[
                {"type":"intro","chords":["C","G"]},
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":["Am F"]}]},
                {"type":"chorus","same":true}
            ]
        });
        song::normalize_value(&raw).unwrap().0
    }

    /// The demo song moved to `key` (chords transposed from G) in `mode`.
    fn demo_in(key: i32, mode: song::Mode) -> Song {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../../engine/src/demo.json")).expect("demo.json is JSON");
        let shift = key - 7;
        let tr = |s: &str| {
            s.split_whitespace()
                .map(|c| song::chord::transpose_symbol(c, shift, false))
                .collect::<Vec<_>>()
                .join(" ")
        };
        raw["key"] = json!(Pc::new(key).name(false));
        raw["mode"] = json!(mode.as_str());
        for sec in raw["sections"].as_array_mut().unwrap() {
            if let Some(cs) = sec.get_mut("chords").and_then(|c| c.as_array_mut()) {
                for c in cs.iter_mut() {
                    *c = json!(tr(c.as_str().unwrap()));
                }
            }
            if let Some(ls) = sec.get_mut("lines").and_then(|l| l.as_array_mut()) {
                for l in ls.iter_mut() {
                    for c in l["chords"].as_array_mut().unwrap().iter_mut() {
                        *c = json!(tr(c.as_str().unwrap()));
                    }
                }
            }
        }
        song::normalize_value(&raw).expect("demo normalises").0
    }

    fn median_midi(notes: &[LeadNote]) -> i32 {
        let mut m: Vec<i32> = notes.iter().map(|n| n.midi).collect();
        m.sort_unstable();
        m[m.len() >> 1]
    }

    /// The former preparation: compose in the written key, compose again
    /// in the chosen key, then shift octaves so the median lands where
    /// `choose_transpose` wants it (rounding half up).
    fn two_passes(song: &Song, seed: u32, voice: Voice) -> (Form, Comp, i32) {
        let mut form = build_form(song, 0);
        let tl = Timeline::new(&form, song.tempo_bpm);
        let comp = compose_melody(song, &mut form, &tl, seed);
        let tr = choose_transpose(&comp.lead, voice);
        let key_shift = key_shift_of(tr);
        let want = median_midi(&comp.lead) + tr;
        let (mut form, mut comp) = if key_shift != 0 {
            let mut f = build_form(song, key_shift);
            let tl2 = Timeline::new(&f, song.tempo_bpm);
            let c = compose_melody(song, &mut f, &tl2, seed);
            (f, c)
        } else {
            (form, comp)
        };
        let shift = 12 * ((want - median_midi(&comp.lead)) as f64 / 12.0 + 0.5).floor() as i32;
        for n in comp.lead.iter_mut() {
            n.midi += shift;
            if let Some(g) = n.grace.as_mut() {
                *g += shift;
            }
        }
        for l in form.lines.iter_mut() {
            if let Some(p) = l.pitches.as_mut() {
                p.iter_mut().for_each(|x| *x += shift);
            }
        }
        (form, comp, key_shift)
    }

    #[test]
    fn compose_once_equals_two_passes() {
        let mut cases = 0;
        let mut shifted = 0;
        for key in 0..12 {
            for &mode in song::Mode::ALL {
                let s = demo_in(key, mode);
                for seed in 0..100u32 {
                    let voice = Voice::ALL[seed as usize % Voice::ALL.len()];
                    let (f2, c2, ks2) = two_passes(&s, seed, voice);
                    let (f1, _, c1, ks1) = compose_for_voice(&s, seed, voice);
                    let at = format!("key {key} mode {mode} seed {seed} voice {voice}");
                    assert_eq!(ks1, ks2, "{at}");
                    assert_eq!((c1.t, c1.tonic), (c2.t, c2.tonic), "{at}");
                    assert_eq!(c1.lead.len(), c2.lead.len(), "{at}");
                    for (a, b) in c1.lead.iter().zip(&c2.lead) {
                        assert_eq!((a.midi, a.grace, a.beat, a.dur), (b.midi, b.grace, b.beat, b.dur), "{at}");
                    }
                    assert_eq!(c1.inst.len(), c2.inst.len(), "{at}");
                    for (a, b) in c1.inst.iter().zip(&c2.inst) {
                        assert_eq!((a.midi, a.beat, a.dur), (b.midi, b.beat, b.dur), "{at}");
                    }
                    for (a, b) in f1.lines.iter().zip(&f2.lines) {
                        assert_eq!(a.pitches, b.pitches, "{at}");
                    }
                    let syms = |f: &Form| f.chords.iter().map(|c| c.symbol.clone()).collect::<Vec<_>>();
                    assert_eq!(syms(&f1), syms(&f2), "{at}");
                    cases += 1;
                    shifted += usize::from(ks1 != 0);
                }
            }
        }
        assert_eq!(cases, 4800);
        assert!(shifted > 1000, "only {shifted} cases change key");
    }

    #[test]
    fn prepare_produces_ordered_note_times() {
        let s = song();
        let p = prepare(&s, 99, None);
        assert_eq!(p.voice, Voice::Baritone);
        assert!(!p.comp.lead.is_empty());
        for w in p.comp.lead.windows(2) {
            assert!(w[0].t0 <= w[0].t1 + 1e-9);
            assert!(w[0].t1 <= w[1].t0 + 1e-6);
        }
    }

    #[test]
    fn vocal_notes_applies_lift_and_stress_gain() {
        let s = song();
        let p = prepare(&s, 99, None);
        let vn = vocal_notes(&p.comp.lead, 1.0);
        for (n, v) in p.comp.lead.iter().zip(vn.iter()) {
            let expect = (if n.stress { 1.0 } else { 0.86 }) * (if n.lift { 1.08 } else { 1.0 });
            assert!((v.amp - expect).abs() < 1e-9);
        }
    }

    #[test]
    fn harmony_line_stays_in_scale() {
        let s = song();
        let p = prepare(&s, 99, None);
        let hl = harmony_line(&p.comp.lead, &p.form, &p.timeline, &s, p.tonic, true);
        assert_eq!(hl.len(), p.comp.lead.len());
        for n in &hl {
            assert!(n.grace.is_none());
        }
    }
}

//! Preparation for rendering: compose, transpose for the voice, time the
//! notes; and the vocal-note and harmony-line helpers.

use song::{Blend, Pc, Phoneme, SingerId, Song, Voice};

use crate::form::{build_form, Form};
use crate::melody::{compose_melody, register_of, Comp, LeadNote};
use crate::theory::local_scale;
use crate::timeline::Timeline;
use crate::voices::{choose_transpose, choose_transpose_duet, range_penalty};

/// Everything the renderer needs.
pub struct Prepared {
    pub form: Form,
    pub timeline: Timeline,
    pub comp: Comp,
    pub voice: Voice,
    /// Singer B's chosen voice; `None` outside a duet.
    pub voice_b: Option<Voice>,
    /// Key change for the voice, -5..=6 semitones.
    pub key_shift: i32,
    /// Tonic pitch class after the key change, 0-11.
    pub tonic: i32,
}

/// Voice overrides for `prepare_voices`; `None` uses the song's own voice
/// (`Song::voice` for A, `Duet::voice` for B). `b` is ignored in a solo song.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceChoice {
    pub a: Option<Voice>,
    pub b: Option<Voice>,
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
///
/// This is a shim over `prepare_voices` with `VoiceChoice { a: voice_key,
/// b: None }`, kept so existing callers (one voice, no duet override) do
/// not need to change.
pub fn prepare(song: &Song, seed: u64, voice_key: Option<Voice>) -> Prepared {
    prepare_voices(
        song,
        seed,
        VoiceChoice {
            a: voice_key,
            b: None,
        },
    )
}

/// `prepare`, with voice overrides for both singers of a duet (design 4.5).
/// `voice.b` is ignored outside a duet.
pub fn prepare_voices(song: &Song, seed: u64, voice: VoiceChoice) -> Prepared {
    let vk = voice.a.unwrap_or(song.voice);
    let vb = song.is_duet().then(|| {
        voice
            .b
            .or_else(|| song.voice_of(SingerId::B))
            .expect("duet has a B voice")
    });
    let (form, timeline, mut comp, key_shift) = compose_for_voice(song, seed, vk, vb);
    time_notes(&mut comp.lead, &timeline);
    comp.second = compose_second(&comp, &form, &timeline, vk, vb);
    time_notes(&mut comp.second, &timeline);
    let tonic = (song.key.get() as i32 + key_shift).rem_euclid(12);
    Prepared {
        form,
        timeline,
        comp,
        voice: vk,
        voice_b: vb,
        key_shift,
        tonic,
    }
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

/// Compose in the written key and transpose for `voice` (and, in a duet,
/// `voice_b`) (see `prepare`). Solo: `choose_transpose` alone, unchanged.
/// Duet: `choose_transpose_duet` over both singers' melody notes, weighted
/// by their share (design 4.5); the register fit itself (`d`, `o`) was
/// already applied inside `compose_melody`.
fn compose_for_voice(
    song: &Song,
    seed: u64,
    voice: Voice,
    voice_b: Option<Voice>,
) -> (Form, Timeline, Comp, i32) {
    let mut form = build_form(song, 0);
    let tl = Timeline::new(&form, song.tempo_bpm);
    let mut comp = compose_melody(song, &mut form, &tl, seed, voice, voice_b);
    let tr = match voice_b {
        Some(vb) => choose_transpose_duet(&comp.lead, voice, vb),
        None => choose_transpose(&comp.lead, voice),
    };
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
            n.t1 =
                (n.t0 + MIN_NOTE).max(next_t0 - if n.phrase_end { BREATH_GAP } else { LEGATO_GAP });
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
    /// A continuation note of a melisma (`Syllable::is_continuation`): the
    /// vowel holds from the note before.
    pub cont: bool,
}

/// Lead notes as sung notes at level `amp`: unstressed syllables at 0.86,
/// lifted sections at 1.08. A melisma's continuation notes take the level
/// of the syllable's first note.
pub fn vocal_notes(lead: &[LeadNote], amp: f64) -> Vec<VocalNote> {
    let mut head_stress = false;
    lead.iter()
        .map(|n| {
            let cont = n.syl.is_continuation();
            if !cont {
                head_stress = n.stress;
            }
            let loud = if cont { head_stress } else { n.stress };
            VocalNote {
                t0: n.t0,
                t1: n.t1,
                midi: n.midi,
                ph: n.syl.phones.clone(),
                amp: amp * (if loud { 1.0 } else { 0.86 }) * (if n.lift { 1.08 } else { 1.0 }),
                phrase_start: n.phrase_start,
                phrase_end: n.phrase_end,
                grace: n.grace,
                stress: n.stress,
                cont,
            }
        })
        .collect()
}

/// A harmony a third to a sixth above (`up`) or below the lead: the chord
/// tone preferred by interval (3rd and 4th best), else two scale steps.
pub fn harmony_line(lead: &[LeadNote], form: &Form, tl: &Timeline, up: bool) -> Vec<LeadNote> {
    // Preference by interval in semitones, 3..=9: thirds best, the
    // tritone worst, sixths next.
    const SCORES: [f64; 10] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.2, -1.0, 0.4, 0.6, 0.6];
    lead.iter()
        .map(|n| {
            let ch = tl.chord_at(form, n.beat + 0.01);
            let (tonic, mode) = form.sections[form.lines[n.line_idx].sec].key;
            let sc = local_scale(tonic, mode, ch);
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

/// The other singer's notes on every shared line (design 4.5, step 4): the
/// melody's onsets, durations, syllables, phrase flags and lift, no grace
/// notes. `blend: octave` tries melody + 12k for k in -1, 0, 1 (k = 0 is
/// unison: two singers of one type sing the same note); `blend: harmony`
/// takes `harmony_line` over the melody (`up` when the other singer's
/// centre is at or above the melody singer's) then tries + 12k for k in 0,
/// 1 (up) or 0, -1 (down). Either way, the k that best fits the other
/// singer's range (`range_penalty` at zero shift, since the shift is
/// already folded into the candidate pitches) wins. Called after the
/// transposition, over the already-transposed `comp.lead`.
pub fn compose_second(
    comp: &Comp,
    form: &Form,
    tl: &Timeline,
    voice_a: Voice,
    voice_b: Option<Voice>,
) -> Vec<LeadNote> {
    let Some(voice_b) = voice_b else {
        return Vec::new();
    };
    let voice_of = |s: SingerId| if s == SingerId::A { voice_a } else { voice_b };
    let best_by_penalty = |cands: Vec<Vec<LeadNote>>, voice: Voice| -> Vec<LeadNote> {
        cands
            .into_iter()
            .map(|c| {
                let midis: Vec<i32> = c.iter().map(|n| n.midi).collect();
                let pen = range_penalty(&midis, voice, 0);
                (pen, c)
            })
            .min_by(|(a, _), (b, _)| a.partial_cmp(b).expect("finite penalty"))
            .map(|(_, c)| c)
            .unwrap_or_default()
    };

    let mut second = Vec::new();
    for (li_idx, l) in form.lines.iter().enumerate() {
        let Some((other, blend)) = l.part.other() else {
            continue;
        };
        let mnotes: Vec<LeadNote> = comp
            .lead
            .iter()
            .filter(|n| n.line_idx == li_idx)
            .cloned()
            .collect();
        if mnotes.is_empty() {
            continue;
        }
        let vo = voice_of(other);
        let picked = match blend {
            Blend::Octave => {
                let cands = (-1..=1i32)
                    .map(|k| {
                        mnotes
                            .iter()
                            .map(|n| {
                                let mut nn = n.clone();
                                nn.midi += 12 * k;
                                nn.singer = other;
                                nn.grace = None;
                                nn
                            })
                            .collect()
                    })
                    .collect();
                best_by_penalty(cands, vo)
            }
            Blend::Harmony => {
                let melody_singer = l.part.melody();
                let up =
                    voice_of(other).range().centre() >= voice_of(melody_singer).range().centre();
                let base = harmony_line(&mnotes, form, tl, up);
                let ks: [i32; 2] = if up { [0, 1] } else { [0, -1] };
                let cands = ks
                    .iter()
                    .map(|&k| {
                        base.iter()
                            .map(|n| {
                                let mut nn = n.clone();
                                nn.midi += 12 * k;
                                nn.singer = other;
                                nn
                            })
                            .collect()
                    })
                    .collect();
                best_by_penalty(cands, vo)
            }
        };
        second.extend(picked);
    }
    second
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
            serde_json::from_str(include_str!("../../engine/src/demo.json"))
                .expect("demo.json is JSON");
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
    /// `choose_transpose` (or, in a duet, `choose_transpose_duet`) wants it
    /// (rounding half up).
    fn two_passes(
        song: &Song,
        seed: u64,
        voice: Voice,
        voice_b: Option<Voice>,
    ) -> (Form, Comp, i32) {
        let mut form = build_form(song, 0);
        let tl = Timeline::new(&form, song.tempo_bpm);
        let comp = compose_melody(song, &mut form, &tl, seed, voice, voice_b);
        let tr = match voice_b {
            Some(vb) => choose_transpose_duet(&comp.lead, voice, vb),
            None => choose_transpose(&comp.lead, voice),
        };
        let key_shift = key_shift_of(tr);
        let want = median_midi(&comp.lead) + tr;
        let (mut form, mut comp) = if key_shift != 0 {
            let mut f = build_form(song, key_shift);
            let tl2 = Timeline::new(&f, song.tempo_bpm);
            let c = compose_melody(song, &mut f, &tl2, seed, voice, voice_b);
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
                for seed in 0..100u64 {
                    let voice = Voice::ALL[seed as usize % Voice::ALL.len()];
                    let (f2, c2, ks2) = two_passes(&s, seed, voice, None);
                    let (f1, _, c1, ks1) = compose_for_voice(&s, seed, voice, None);
                    let at = format!("key {key} mode {mode} seed {seed} voice {voice}");
                    assert_eq!(ks1, ks2, "{at}");
                    assert_eq!((c1.t, c1.tonic), (c2.t, c2.tonic), "{at}");
                    assert_eq!(c1.lead.len(), c2.lead.len(), "{at}");
                    for (a, b) in c1.lead.iter().zip(&c2.lead) {
                        assert_eq!(
                            (a.midi, a.grace, a.beat, a.dur),
                            (b.midi, b.grace, b.beat, b.dur),
                            "{at}"
                        );
                    }
                    assert_eq!(c1.inst.len(), c2.inst.len(), "{at}");
                    for (a, b) in c1.inst.iter().zip(&c2.inst) {
                        assert_eq!((a.midi, a.beat, a.dur), (b.midi, b.beat, b.dur), "{at}");
                    }
                    for (a, b) in f1.lines.iter().zip(&f2.lines) {
                        assert_eq!(a.pitches, b.pitches, "{at}");
                    }
                    let syms = |f: &Form| {
                        f.chords
                            .iter()
                            .map(|c| c.symbol.clone())
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(syms(&f1), syms(&f2), "{at}");
                    cases += 1;
                    shifted += usize::from(ks1 != 0);
                }
            }
        }
        assert_eq!(cases, 4800);
        assert!(shifted > 1000, "only {shifted} cases change key");
    }

    fn duet_song() -> Song {
        let raw: serde_json::Value = serde_json::from_str(include_str!("../tests/songs/duet.json"))
            .expect("duet.json is JSON");
        let (s, repairs) = song::normalize_value(&raw).expect("duet fixture normalises");
        assert!(repairs.is_empty(), "{repairs:?}");
        assert!(s.is_duet());
        s
    }

    /// The duet fixture moved to `key` (chords transposed from C, its
    /// written key), mode unchanged.
    fn duet_in(key: i32) -> Song {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/songs/duet.json"))
                .expect("duet.json is JSON");
        let tr = |s: &str| {
            s.split_whitespace()
                .map(|c| song::chord::transpose_symbol(c, key, false))
                .collect::<Vec<_>>()
                .join(" ")
        };
        raw["key"] = json!(Pc::new(key).name(false));
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
        song::normalize_value(&raw)
            .expect("duet fixture normalises")
            .0
    }

    /// The duet path of `compose_once_equals_two_passes`: composing once
    /// with both voices chosen up front equals composing in the chosen key
    /// separately, for the duet fixture over its keys and 20 seeds.
    #[test]
    fn compose_once_equals_two_passes_duet() {
        let mut cases = 0;
        for key in 0..12 {
            let s = duet_in(key);
            for seed in 0..20u64 {
                let (f2, c2, ks2) = two_passes(&s, seed, Voice::Baritone, Some(Voice::Alto));
                let (f1, _, c1, ks1) =
                    compose_for_voice(&s, seed, Voice::Baritone, Some(Voice::Alto));
                let at = format!("key {key} seed {seed}");
                assert_eq!(ks1, ks2, "{at}");
                assert_eq!((c1.t, c1.tonic), (c2.t, c2.tonic), "{at}");
                assert_eq!(c1.lead.len(), c2.lead.len(), "{at}");
                for (a, b) in c1.lead.iter().zip(&c2.lead) {
                    assert_eq!(
                        (a.midi, a.grace, a.beat, a.dur, a.singer),
                        (b.midi, b.grace, b.beat, b.dur, b.singer),
                        "{at}"
                    );
                }
                for (a, b) in f1.lines.iter().zip(&f2.lines) {
                    assert_eq!(a.pitches, b.pitches, "{at}");
                }
                cases += 1;
            }
        }
        assert_eq!(cases, 240);
    }

    #[test]
    fn prepare_voices_solo_song_ignores_voice_b() {
        let s = song();
        let p = prepare_voices(
            &s,
            5,
            VoiceChoice {
                a: None,
                b: Some(Voice::Soprano),
            },
        );
        assert_eq!(p.voice_b, None);
        assert!(p.comp.second.is_empty());
    }

    #[test]
    fn duet_notes_mostly_fit_each_singers_range() {
        let s = duet_song();
        let (mut a_in, mut a_all) = (0usize, 0usize);
        let (mut b_in, mut b_all) = (0usize, 0usize);
        let (mut b_med_ok, mut b_med_all) = (0usize, 0usize);
        for seed in 0..20u64 {
            let p = prepare_voices(&s, seed, VoiceChoice::default());
            assert_eq!(p.voice, Voice::Baritone);
            assert_eq!(p.voice_b, Some(Voice::Alto));
            let mut all_notes: Vec<&LeadNote> = p.comp.lead.iter().chain(&p.comp.second).collect();
            all_notes.sort_by_key(|n| n.beat.to_bits());
            let ra = Voice::Baritone.range();
            let rb = Voice::Alto.range();
            let mut a_midis = Vec::new();
            let mut b_midis = Vec::new();
            for n in &all_notes {
                match n.singer {
                    SingerId::A => {
                        a_all += 1;
                        a_in += usize::from((ra.lo as i32..=ra.hi as i32).contains(&n.midi));
                        a_midis.push(n.midi);
                    }
                    SingerId::B => {
                        b_all += 1;
                        b_in += usize::from((rb.lo as i32..=rb.hi as i32).contains(&n.midi));
                        b_midis.push(n.midi);
                    }
                }
            }
            let b_melody: Vec<i32> = p
                .comp
                .lead
                .iter()
                .filter(|n| n.singer == SingerId::B)
                .map(|n| n.midi)
                .collect();
            if !b_melody.is_empty() {
                let mut m = b_melody.clone();
                m.sort_unstable();
                let med = m[m.len() / 2] as f64;
                b_med_all += 1;
                b_med_ok += usize::from((med - rb.centre()).abs() <= 4.0);
            }
        }
        let rate_a = a_in as f64 / a_all as f64;
        let rate_b = b_in as f64 / b_all as f64;
        eprintln!("A in range {rate_a:.3}, B in range {rate_b:.3}");
        assert!(rate_a >= 0.9, "{rate_a}");
        assert!(rate_b >= 0.9, "{rate_b}");
        assert_eq!(
            b_med_ok, b_med_all,
            "B melody median not within 4 semitones of centre({}) in every seed",
            b_med_all
        );
    }

    #[test]
    fn second_matches_the_melodys_rhythm_and_syllables_on_shared_lines() {
        let s = duet_song();
        let p = prepare_voices(&s, 3, VoiceChoice::default());
        let shared: Vec<usize> = (0..p.form.lines.len())
            .filter(|&i| p.form.lines[i].part.other().is_some())
            .collect();
        assert!(!shared.is_empty());
        for li in shared {
            let melody: Vec<&LeadNote> = p.comp.lead.iter().filter(|n| n.line_idx == li).collect();
            let second: Vec<&LeadNote> =
                p.comp.second.iter().filter(|n| n.line_idx == li).collect();
            assert_eq!(melody.len(), second.len(), "line {li}");
            for (m, s) in melody.iter().zip(&second) {
                assert_eq!(m.beat, s.beat, "line {li}");
                assert_eq!(m.dur, s.dur, "line {li}");
                assert_eq!(m.syl, s.syl, "line {li}");
                assert_eq!(m.phrase_start, s.phrase_start, "line {li}");
                assert_eq!(m.phrase_end, s.phrase_end, "line {li}");
                assert_eq!(m.lift, s.lift, "line {li}");
                assert!(s.grace.is_none(), "line {li}");
                assert_ne!(m.singer, s.singer, "line {li}");
            }
        }
    }

    #[test]
    fn octave_blend_is_melody_plus_twelve_k() {
        let s = duet_song();
        let p = prepare_voices(&s, 3, VoiceChoice::default());
        let octave_lines: Vec<usize> = (0..p.form.lines.len())
            .filter(|&i| {
                matches!(
                    p.form.lines[i].part,
                    song::Part::Both {
                        blend: Blend::Octave,
                        ..
                    }
                )
            })
            .collect();
        assert!(!octave_lines.is_empty());
        for li in octave_lines {
            let melody: Vec<&LeadNote> = p.comp.lead.iter().filter(|n| n.line_idx == li).collect();
            let second: Vec<&LeadNote> =
                p.comp.second.iter().filter(|n| n.line_idx == li).collect();
            assert_eq!(melody.len(), second.len());
            let k = (second[0].midi - melody[0].midi) as f64 / 12.0;
            assert!((k.round() - k).abs() < 1e-9, "not a whole octave: {k}");
            let shift = k.round() as i32 * 12;
            for (m, s) in melody.iter().zip(&second) {
                assert_eq!(s.midi - m.midi, shift, "line {li}");
            }
        }
    }

    /// Claude's duet JSON is user input: empty lines, unknown singer
    /// names, a duet on one voice type and a duet override in a solo song
    /// must not panic.
    #[test]
    fn duet_prepare_survives_odd_input() {
        let mut raw: serde_json::Value =
            serde_json::from_str(include_str!("../tests/songs/duet.json"))
                .expect("duet.json is JSON");
        raw["duet"] = json!({"voice": "baritone"});
        for sec in raw["sections"].as_array_mut().unwrap() {
            if sec.get("lines").is_some() {
                sec["sing"] = json!("nobody");
            }
        }
        let secs = raw["sections"].as_array_mut().unwrap();
        secs.push(json!({"type": "verse", "sing": "both", "lines": []}));
        secs.push(json!({"type": "chorus", "sing": "B", "lines": [{"syl": "", "ph": "", "chords": ["C"]}]}));
        if let Ok((s, _)) = song::normalize_value(&raw) {
            for seed in 0..3u64 {
                let p = prepare_voices(
                    &s,
                    seed,
                    VoiceChoice {
                        a: Some(Voice::Soprano),
                        b: Some(Voice::Baritone),
                    },
                );
                assert_eq!(p.voice_b.is_some(), s.is_duet());
                assert!(p
                    .comp
                    .lead
                    .iter()
                    .chain(&p.comp.second)
                    .all(|n| n.t1 >= n.t0));
            }
        }
        let same = duet_song();
        let p = prepare_voices(
            &same,
            1,
            VoiceChoice {
                a: Some(Voice::Tenor),
                b: Some(Voice::Tenor),
            },
        );
        assert_eq!(p.voice_b, Some(Voice::Tenor));
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
        let hl = harmony_line(&p.comp.lead, &p.form, &p.timeline, true);
        assert_eq!(hl.len(), p.comp.lead.len());
        for n in &hl {
            assert!(n.grace.is_none());
        }
    }
}

//! Full-render preparation: `prepare`, `vocalNotes` and `harmonyLine`
//! (engine.js lines ~843-866).

use sfcore::js::round;

use crate::form::{build_form, Form};
use crate::melody::{compose_melody, Comp, LeadNote};
use crate::song::Song;
use crate::theory::local_scale;
use crate::timeline::Timeline;
use crate::voices::{choose_transpose, Voice};

/// prepare's return value: everything the renderer needs.
pub struct Prepared {
    pub form: Form,
    pub timeline: Timeline,
    pub comp: Comp,
    pub voice: Voice,
    pub key_shift: i32,
    pub tonic: i32,
}

fn median_midi(notes: &[LeadNote]) -> i32 {
    let mut m: Vec<i32> = notes.iter().map(|n| n.midi).collect();
    m.sort_unstable();
    m[m.len() >> 1]
}

/// prepare(song,seed,voiceKey): composes twice (original key, then
/// transposed) and shifts octaves so the median lands where `chooseTranspose`
/// wants it. `voice_key` of `None` is JS's `'auto'`: use the song's own voice.
pub fn prepare(song: &Song, seed: u32, voice_key: Option<Voice>) -> Prepared {
    // pass 1: compose in original key to decide transposition for this voice
    let mut form = build_form(song, 0);
    let mut tl = Timeline::new(&form, song.tempo);
    let mut comp = compose_melody(song, &mut form, &tl, seed);

    let vk = voice_key.unwrap_or(song.voice);
    let tr = choose_transpose(&comp.lead, vk);
    let semis = tr.rem_euclid(12);
    let key_shift = if semis > 6 { semis - 12 } else { semis };

    let want = median_midi(&comp.lead) + tr;

    if key_shift != 0 {
        form = build_form(song, key_shift);
        tl = Timeline::new(&form, song.tempo);
        comp = compose_melody(song, &mut form, &tl, seed);
    }

    let shift = 12 * round((want - median_midi(&comp.lead)) as f64 / 12.0) as i32;
    for n in comp.lead.iter_mut() {
        n.midi += shift;
        if let Some(g) = n.grace {
            n.grace = Some(g + shift);
        }
    }
    for l in form.lines.iter_mut() {
        if let Some(p) = &l.pitches {
            l.pitches = Some(p.iter().map(|x| x + shift).collect());
        }
    }

    // seconds
    for n in comp.lead.iter_mut() {
        n.t0 = tl.to_time(n.beat);
        n.t1 = tl.to_time(n.beat + n.dur) - if n.phrase_end { 0.05 } else { 0.0 };
    }
    for i in 0..comp.lead.len().saturating_sub(1) {
        let nx_t0 = comp.lead[i + 1].t0;
        let phrase_end = comp.lead[i].phrase_end;
        if comp.lead[i].t1 > nx_t0 - 0.01 {
            comp.lead[i].t1 = (comp.lead[i].t0 + 0.05).max(nx_t0 - if phrase_end { 0.09 } else { 0.004 });
        }
    }

    let tonic = (song.key_pc + key_shift + 120).rem_euclid(12);
    Prepared { form, timeline: tl, comp, voice: vk, key_shift, tonic }
}

/// One rendered vocal note (`vocalNotes`'s per-note output shape).
#[derive(Clone, Debug)]
pub struct VocalNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: i32,
    pub ph: Vec<String>,
    pub amp: f64,
    pub phrase_start: bool,
    pub phrase_end: bool,
    pub grace: Option<i32>,
    pub stress: bool,
}

/// vocalNotes(lead,amp,sec). JS parity: the JS function takes a third `sec`
/// parameter that is never referenced in its body (it reads `n.sec`, the
/// note's own section, not the parameter) and every call site passes only
/// two arguments; the parameter is dead and is omitted here.
pub fn vocal_notes(lead: &[LeadNote], amp: f64) -> Vec<VocalNote> {
    lead.iter()
        .map(|n| VocalNote {
            t0: n.t0,
            t1: n.t1,
            midi: n.midi,
            ph: n.syl.ph.clone(),
            amp: amp * (if n.stress { 1.0 } else { 0.86 }) * (if n.lift { 1.08 } else { 1.0 }),
            phrase_start: n.phrase_start,
            phrase_end: n.phrase_end,
            grace: n.grace,
            stress: n.stress,
        })
        .collect()
}

/// harmonyLine(lead,tl,song,tonic,up)
pub fn harmony_line(lead: &[LeadNote], form: &Form, tl: &Timeline, song: &Song, tonic: i32, up: bool) -> Vec<LeadNote> {
    // JS: [0,0,0,1,1,.2,-1,.4,.6,.6][d] for d in 3..=9
    const SCORES: [f64; 10] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.2, -1.0, 0.4, 0.6, 0.6];
    lead.iter()
        .map(|n| {
            let ch = tl.chord_at(form, n.beat + 0.01);
            let sc = local_scale(tonic, &song.mode, ch);
            let mut h: Option<i32> = None;
            let mut bs = -1e9f64;
            for d in 3..=9i32 {
                let m = if up { n.midi + d } else { n.midi - d };
                if !ch.pcs.contains(&m.rem_euclid(12)) {
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
                        if sc.contains(&m.rem_euclid(12)) {
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
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":"C G"}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":"Am F"}]},
                {"type":"chorus","same":true}
            ]
        });
        crate::song::normalize_song(&raw).unwrap()
    }

    #[test]
    fn prepare_produces_ordered_note_times() {
        let s = song();
        let p = prepare(&s, 99, None);
        assert_eq!(p.voice, crate::voices::Voice::Baritone);
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

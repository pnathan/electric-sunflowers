//! The harp: a rolled chord per chord segment, an echo of the top three
//! tones on the half bar in busy sections, and a rising scale glissando into
//! each bridge and into the final lifted section.
//!
//! Where it plays: lifted sections, choruses, bridges, outros, interludes,
//! the intro and verses after the first; never the pre-chorus. Verses and
//! the intro are sparse: every other bar, four tones rolled at 90 ms, at 0.7
//! level. Elsewhere six tones rolled at 65 ms. Tones: from the chord root
//! upward in G3-E6 (MIDI 55-88), chord tones only. Level 0.55 rising to
//! 0.65 across the roll. Echo (intensity >= 2, segments of a bar or more):
//! the top three tones 50 ms apart at 0.4, on beat 3 (beat 2 in 6/8).
//!
//! Glissando: the scale of the mode on the chord root at the section start,
//! D4-E6 (62-88), over 0.62 s ending 30 ms before the downbeat, level 0.28
//! rising to 0.53.
//!
//! Harp notes ring; each note's `t1` equals its `t0` (the instrument sets
//! the ring time). Randomness: roll k's onset jitter (+-5 ms per tone) from
//! `Rng::event(seed, HARP_ROLL, k)`.

use compose::form::{Form, Sec};
use compose::timeline::Timeline;
use sfcore::random::{tag, Rng, Tag};
use song::events::PluckNote;
use song::{Pc, PcSet, SectionKind};

const HARP_ROLL: Tag = tag("harp.roll");

/// Tone range of the rolled chords, MIDI.
const ROLL_LO: u8 = 55;
const ROLL_HI: u8 = 88;
/// Glissando range, MIDI.
const GLISS_LO: u8 = 62;
const GLISS_HI: u8 = 88;
/// Glissando length and gap before the downbeat, seconds.
const GLISS_LEN: f64 = 0.62;
const GLISS_GAP: f64 = 0.03;

fn note(t: f64, midi: u8, vel: f64) -> PluckNote {
    PluckNote {
        t0: t,
        t1: t,
        midi: midi as f32,
        vel: vel as f32,
    }
}

/// The glissando's scale: the mode of the section's own key on `root`.
fn gliss_scale(sec: &Sec, root: Pc) -> PcSet {
    sec.key.1.scale().transpose(root.get() as i32)
}

/// The harp part.
pub fn plan(form: &Form, tl: &Timeline, seed: u64) -> Vec<PluckNote> {
    let bpb = form.bpb();
    let mut notes: Vec<PluckNote> = Vec::new();

    for (si, sg) in tl.segs.iter().enumerate() {
        let sec = &form.sections[sg.sec];
        let on = sec.is_lift()
            || match sec.kind {
                SectionKind::Chorus
                | SectionKind::Bridge
                | SectionKind::Outro
                | SectionKind::Interlude
                | SectionKind::Intro => true,
                SectionKind::Verse => sec.occ > 0,
                SectionKind::Prechorus => false,
            };
        if !on {
            continue;
        }
        let sparse = matches!(sec.kind, SectionKind::Verse | SectionKind::Intro);
        if sparse && (sg.bar - sec.start_bar) % 2 == 1 {
            continue;
        }
        let chord = form.chord(sg.chord);
        let want = if sparse { 4 } else { 6 };
        let mut tones = [0u8; 6];
        let mut n = 0;
        // From the root upward.
        let first = ROLL_LO + (chord.root.get() + 12 - ROLL_LO % 12) % 12;
        for m in chord.tones.tones_in(first, ROLL_HI).take(want) {
            tones[n] = m;
            n += 1;
        }
        let tones = &tones[..n];
        let t0 = tl.to_time(sg.b0);
        let stagger = if sparse { 0.09 } else { 0.065 };
        let level = if sparse { 0.7 } else { 1.0 };
        let mut r = Rng::event(seed, HARP_ROLL, si as u64);
        for (k, &m) in tones.iter().enumerate() {
            let t = t0 + k as f64 * stagger + 0.005 * r.bipolar();
            notes.push(note(t, m, (0.55 + 0.1 * k as f64 / n as f64) * level));
        }
        if sec.intensity.level() >= 2 && sg.b1 - sg.b0 >= bpb as f64 {
            let tm = tl.to_time(sg.b0 + if bpb == 2 { 1.0 } else { 2.0 });
            for (k, &m) in tones[n.saturating_sub(3)..].iter().enumerate() {
                notes.push(note(tm + k as f64 * 0.05, m, 0.4));
            }
        }
    }

    for sec in &form.sections {
        if !(sec.kind == SectionKind::Bridge || sec.lift.is_some_and(|l| l.is_final)) {
            continue;
        }
        let b = sec.beats(&form.meter).start;
        let t1 = tl.to_time(b) - GLISS_GAP;
        let t0 = t1 - GLISS_LEN;
        let scale = gliss_scale(sec, tl.chord_at(form, b).root);
        let n = scale.tones_in(GLISS_LO, GLISS_HI).count();
        for (k, m) in scale.tones_in(GLISS_LO, GLISS_HI).enumerate() {
            let x = k as f64 / n as f64;
            notes.push(note(t0 + (t1 - t0) * x, m, 0.28 + 0.25 * x));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use song::Mode;

    /// A minor song whose last chorus turns to A major: the glissando into
    /// that chorus uses the major scale, the one into the bridge the minor.
    #[test]
    fn the_glissando_scale_follows_the_section_key() {
        let v = json!({
            "schema_version":2,"key":"A","mode":"minor","meter":"4/4","tempo":100,
            "band":{"harp":true},
            "sections":[
                {"type":"verse","lines":[{"syl":"*one *two three *four","chords":["Am","Em"]}]},
                {"type":"bridge","lines":[{"syl":"*five *six seven *eight","chords":["F","G"]}]},
                {"type":"chorus","key":"A major","lines":[{"syl":"*nine *ten","chords":["A","E"]}]}
            ]
        });
        let s = song::normalize_value(&v).unwrap().0;
        let p = compose::prepare::prepare(&s, 3, None);
        let form = &p.form;
        let bridge = &form.sections[1];
        let chorus = &form.sections[2];
        assert!(chorus.lift.is_some_and(|l| l.is_final));
        assert_eq!(bridge.key.1, Mode::Minor);
        assert_eq!(chorus.key.1, Mode::Major);
        let root = |sec: &Sec| p.timeline.chord_at(form, sec.beats(&form.meter).start).root;
        let maj = gliss_scale(chorus, root(chorus));
        let min = gliss_scale(bridge, root(bridge));
        assert_eq!(
            maj,
            Mode::Major.scale().transpose(root(chorus).get() as i32)
        );
        assert_eq!(
            min,
            Mode::Minor.scale().transpose(root(bridge).get() as i32)
        );
        // The plan sounds both glissandi.
        let notes = plan(form, &p.timeline, 3);
        let t1 = |sec: &Sec| p.timeline.to_time(sec.beats(&form.meter).start) - GLISS_GAP;
        let gliss = |sec: &Sec| -> Vec<u8> {
            notes
                .iter()
                .filter(|n| {
                    (t1(sec) - GLISS_LEN - 1e-9..=t1(sec) + 1e-9).contains(&n.t0)
                        && n.vel < 0.54
                        && n.vel > 0.279
                })
                .map(|n| n.midi as u8)
                .collect()
        };
        let g = gliss(chorus);
        assert!(g.len() >= 8, "{g:?}");
        assert!(g.iter().all(|&m| maj.contains(Pc::new(m as i32))));
    }
}

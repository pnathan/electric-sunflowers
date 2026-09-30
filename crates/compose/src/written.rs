//! Written break tunes (schema 3): the lead line of an instrumental section
//! as the writer set it. The engine plays the notes as written: no note is
//! added, dropped or moved except by whole octaves, for the whole tune.
//!
//! Time. A tune's lengths are note values. A beat is a quarter note in 4/4
//! and 3/4 and a dotted quarter in 6/8 (`Meter::grid`), so a quarter lasts
//! one beat in 4/4 and 3/4 and 2/3 of a beat in 6/8 (`song::beat_ticks`).
//! In a stretched song (`Form::stretch` 2) each written bar spans two form
//! bars and the tune's beats double with it, so the tune keeps pace with its
//! chords. A tune shorter than its section repeats from its start; the last
//! note is cut at the section's end.
//!
//! Pitch. `d` is the section's tonic after the song's transposition. `place`
//! (run by `prepare` once the transposition is known) moves the whole tune
//! by whole octaves: most notes inside the violin's lead range, then the
//! plain `d` nearest MIDI 67-74.

use song::{BreakNote, Repair};

use crate::form::Form;
use crate::melody::InstNote;

/// The violin's lead range, MIDI (`arrange::violin`).
pub const LEAD_LO: i32 = 64;
pub const LEAD_HI: i32 = 86;
/// Preferred span of the plain `d`, MIDI.
const D_LO: i32 = 67;
const D_HI: i32 = 74;

/// The notes of the tune in section `si`, repeated to fill it. `midi` holds
/// `60 +` the semitones above the tonic, for `place` to settle.
pub fn compose(form: &Form, si: usize, tune: &[BreakNote]) -> Vec<InstNote> {
    let s = &form.sections[si];
    let bpb = form.bpb() as f64;
    let start = s.start_bar as f64 * bpb;
    let len = s.n_bars as f64 * bpb;
    let per_tick = form.stretch.max(1) as f64 / song::beat_ticks(form.meter) as f64;
    let mut out = Vec::new();
    if tune.is_empty() {
        return out;
    }
    let mut at = 0.0;
    'fill: loop {
        for n in tune {
            if at >= len - 1e-9 {
                break 'fill;
            }
            let dur = n.ticks as f64 * per_tick;
            if let Some(p) = n.pitch {
                out.push(InstNote {
                    beat: start + at,
                    dur: dur.min(len - at),
                    midi: 60 + p.semis as i32 + 12 * p.octave as i32,
                    lift: s.is_lift(),
                    written: true,
                });
            }
            at += dur;
        }
    }
    out
}

/// Places each written tune: sets `midi` of its notes from the section's
/// tonic (`form`, transposed) by whole octaves. Records
/// `Repair::BreakTuneRange` for a section with notes still outside the
/// lead range.
pub fn place(inst: &mut [InstNote], form: &Form, repairs: &mut Vec<Repair>) {
    let bpb = form.bpb() as f64;
    for (si, s) in form.sections.iter().enumerate() {
        if s.break_tune.is_none() {
            continue;
        }
        let (b0, b1) = (
            s.start_bar as f64 * bpb,
            (s.start_bar + s.n_bars) as f64 * bpb,
        );
        let mine: Vec<usize> = (0..inst.len())
            .filter(|&i| inst[i].written && inst[i].beat >= b0 - 1e-9 && inst[i].beat < b1 - 1e-9)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let pc = s.key.0.get() as i32;
        let inside = |m: i32| (LEAD_LO..=LEAD_HI).contains(&m);
        let mut best: Option<(usize, i32, f64, i32)> = None;
        for d in (pc + 24..=pc + 96).step_by(12) {
            let n_in = mine
                .iter()
                .filter(|&&i| inside(d + inst[i].midi - 60))
                .count();
            let off = (D_LO - d).max(d - D_HI).max(0);
            let mid = (d as f64 - (D_LO + D_HI) as f64 / 2.0).abs();
            let better = match best {
                None => true,
                Some((bn, boff, bmid, _)) => {
                    n_in > bn || (n_in == bn && (off < boff || (off == boff && mid < bmid)))
                }
            };
            if better {
                best = Some((n_in, off, mid, d));
            }
        }
        let Some((n_in, _, _, d)) = best else {
            continue;
        };
        for &i in &mine {
            inst[i].midi += d - 60;
        }
        if n_in < mine.len() {
            repairs.push(Repair::BreakTuneRange {
                section: si,
                notes: mine.len() - n_in,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepare::prepare;
    use serde_json::json;

    /// An intro with `tune`, then a verse; `meter` and `chords` for the intro.
    fn song(version: u32, meter: &str, chords: &[&str], tune: Option<&str>) -> song::Song {
        let mut intro = json!({"type": "intro", "chords": chords});
        if let Some(t) = tune {
            intro["tune"] = json!(t);
        }
        let v = json!({
            "schema_version": version, "key": "C", "mode": "major", "meter": meter,
            "tempo": if meter == "6/8" { 50 } else { 90 }, "title": "t", "voice": "baritone",
            "sections": [
                intro,
                {"type": "verse", "lines": [
                    {"syl": "one *two three *four", "chords": ["C G"]}]}
            ]
        });
        song::normalize_value(&v).unwrap().0
    }

    fn written(p: &crate::prepare::Prepared) -> Vec<(f64, f64, i32)> {
        p.comp
            .inst
            .iter()
            .filter(|n| n.written)
            .map(|n| (n.beat, n.dur, n.midi))
            .collect()
    }

    #[test]
    fn a_written_break_in_four_four_comes_out_exactly() {
        let s = song(3, "4/4", &["C", "G"], Some("d8 r8 m8 f8 s4 m4"));
        let p = prepare(&s, 7, None);
        let w = written(&p);
        // One bar written, repeated to fill two.
        let beats: Vec<f64> = w.iter().map(|n| n.0).collect();
        assert_eq!(
            beats,
            [0.0, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 4.5, 5.0, 5.5, 6.0, 7.0]
        );
        let durs: Vec<f64> = w.iter().map(|n| n.1).collect();
        assert_eq!(durs[..6], [0.5, 0.5, 0.5, 0.5, 1.0, 1.0]);
        let d = w[0].2;
        let steps: Vec<i32> = w[..6].iter().map(|n| n.2 - d).collect();
        assert_eq!(steps, [0, 2, 4, 5, 7, 4]);
        let first: Vec<i32> = w[..6].iter().map(|n| n.2).collect();
        let second: Vec<i32> = w[6..].iter().map(|n| n.2).collect();
        assert_eq!(first, second);
        assert_eq!(d.rem_euclid(12), p.tonic);
        assert!(w.iter().all(|n| (LEAD_LO..=LEAD_HI).contains(&n.2)));
        assert!(p.comp.repairs.is_empty(), "{:?}", p.comp.repairs);
    }

    #[test]
    fn a_written_break_in_six_eight_counts_dotted_quarter_beats() {
        // A bar of 6/8 is two beats; a quarter is 2/3 of a beat.
        let s = song(3, "6/8", &["C", "G"], Some("d8 r8 m8 s8 m8 d8 | s4 m8 d4."));
        let p = prepare(&s, 7, None);
        let w = written(&p);
        let third = 1.0 / 3.0;
        let want = [
            0.0,
            third,
            2.0 * third,
            1.0,
            4.0 * third,
            5.0 * third,
            2.0,
            2.0 + 2.0 * third,
            2.0 + 3.0 * third,
        ];
        assert_eq!(w.len(), want.len());
        for (n, b) in w.iter().zip(want) {
            assert!((n.0 - b).abs() < 1e-9, "{} vs {b}", n.0);
        }
        assert!((w[6].1 - 2.0 * third).abs() < 1e-9);
        assert!((w[8].1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rests_advance_time_and_the_last_note_is_cut() {
        let s = song(3, "4/4", &["C", "G"], Some("d4 z4 d2"));
        let p = prepare(&s, 7, None);
        let beats: Vec<f64> = written(&p).iter().map(|n| n.0).collect();
        assert_eq!(beats, [0.0, 2.0, 4.0, 6.0]);
        // A tune of six beats in an eight-beat section: the repeat is cut.
        let s = song(3, "4/4", &["C", "G"], Some("d2. m2."));
        let w = written(&prepare(&s, 7, None));
        let cut: Vec<(f64, f64)> = w.iter().map(|n| (n.0, n.1)).collect();
        assert_eq!(cut, [(0.0, 3.0), (3.0, 3.0), (6.0, 2.0)]);
    }

    #[test]
    fn a_tune_out_of_range_keeps_its_notes_and_records_a_repair() {
        let s = song(3, "4/4", &["C", "G"], Some("d8 d'8 d''8 z8 z2"));
        let p = prepare(&s, 7, None);
        let w = written(&p);
        assert_eq!(w.len(), 6);
        assert_eq!(w[1].2 - w[0].2, 12);
        assert_eq!(w[2].2 - w[1].2, 12);
        assert_eq!(
            p.comp.repairs,
            vec![song::Repair::BreakTuneRange {
                section: 0,
                notes: 2
            }]
        );
    }

    #[test]
    fn the_violin_range_decides_the_octave() {
        // Low tune: the tonic goes up until the tune sits in range.
        let s = song(3, "4/4", &["C", "G"], Some("s,4 d4 m4 s4"));
        let w = written(&prepare(&s, 7, None));
        assert!(
            w.iter().all(|n| (LEAD_LO..=LEAD_HI).contains(&n.2)),
            "{w:?}"
        );
        // s, is 5 below d and s is 7 above: only d at 77 holds all four.
        assert_eq!(w[1].2, 77);
    }

    #[test]
    fn an_unwritten_break_is_composed_as_before() {
        let v2 = song(2, "4/4", &["C", "G", "Am", "F"], None);
        let v3 = song(3, "4/4", &["C", "G", "Am", "F"], None);
        let key = |s: &song::Song| {
            prepare(s, 7, None)
                .comp
                .inst
                .iter()
                .map(|n| (n.beat.to_bits(), n.dur.to_bits(), n.midi, n.written))
                .collect::<Vec<_>>()
        };
        let (a, b) = (key(&v2), key(&v3));
        assert!(!a.is_empty());
        assert_eq!(a, b);
        assert!(a.iter().all(|n| !n.3));
    }
}

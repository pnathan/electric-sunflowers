//! The bass line: the chord's bass note on every chord change; in busier
//! sections (intensity >= 2, not the bridge, not the last chord) a root-fifth
//! pattern every two beats (three in 3/4, one in 6/8) with a chromatic or
//! whole-step approach note into the next chord in 4/4. The intro is tacet
//! and the first verse enters halfway.
//!
//! Register: each root is the nearest note of its pitch class from 6
//! semitones below the previous root, clamped to MIDI 34-46; a fifth above
//! 48 drops an octave.
//!
//! Randomness: the approach direction of segment k draws from
//! `Rng::event(seed, BASS_APPROACH, k)`; note k's onset jitter (+-4 ms) from
//! `Rng::event(seed, BASS_TIMING, k)`.
//!
//! The sine sub layer and every synthesis choice belong to the instrument.

use compose::form::Form;
use compose::timeline::Timeline;
use sfcore::random::{tag, Rng, Tag};
use song::events::PluckNote;
use song::{Pc, SectionKind};

const BASS_APPROACH: Tag = tag("bass.approach");
const BASS_TIMING: Tag = tag("bass.timing");

/// Lowest and highest reference for the next root, MIDI.
const ROOT_LO: f64 = 34.0;
const ROOT_HI: f64 = 46.0;
/// A fifth above this drops an octave.
const FIFTH_MAX: i32 = 48;
/// Onset jitter, seconds (+-).
const JITTER: f64 = 0.004;

/// The lowest note from `round(c) - 6` up whose pitch class is `pc`.
pub fn nearest(pc: Pc, c: f64) -> i32 {
    let m0 = c.round() as i32 - 6;
    m0 + (pc.get() as i32 - m0).rem_euclid(12)
}

/// A note on the beat grid before timing.
struct Note {
    beat: f64,
    dur: f64,
    midi: i32,
    vel: f32,
}

/// The bass line as plucked notes.
pub fn plan(form: &Form, tl: &Timeline, seed: u64) -> Vec<PluckNote> {
    let bpb = form.bpb();
    let segs = &tl.segs;
    let mut notes: Vec<Note> = Vec::new();
    let mut prev = 40i32;

    for (si, sg) in segs.iter().enumerate() {
        let sec = &form.sections[sg.sec];
        if sec.kind == SectionKind::Intro {
            continue;
        }
        let half = sec.beats(&form.meter).start + (sec.n_bars as f64 / 2.0) * bpb as f64;
        if sec.kind == SectionKind::Verse && sec.occ == 0 && sg.b0 < half {
            continue;
        }
        let intensity = sec.intensity.level();
        let chord = form.chord(sg.chord);
        let root = nearest(chord.bass, (prev as f64).clamp(ROOT_LO, ROOT_HI));
        prev = root;
        let next = segs.get(si + 1);
        if next.is_none() || intensity <= 1 || sec.kind == SectionKind::Bridge {
            notes.push(Note { beat: sg.b0, dur: sg.b1 - sg.b0 - 0.1, midi: root, vel: 0.85 });
            continue;
        }
        let step = match bpb {
            2 => 1.0,
            3 => 3.0,
            _ => 2.0,
        };
        let held = if bpb == 3 { 3.0 } else { 2.0 };
        let mut b = sg.b0;
        while b < sg.b1 - 1e-6 {
            let rem = sg.b1 - b;
            let first = b == sg.b0;
            let mut m = match chord.fifth {
                Some(fifth) if !first => nearest(fifth, root as f64 + 2.0),
                _ => root,
            };
            if m > FIFTH_MAX {
                m -= 12;
            }
            if !first && rem <= 2.0 && intensity >= 2 && bpb == 4 {
                // Fifth, then an approach note into the next chord's bass.
                notes.push(Note { beat: b, dur: 0.9, midi: m, vel: 0.75 });
                let target = next.map_or(root, |nx| nearest(form.chord(nx.chord).bass, root as f64));
                let mut r = Rng::event(seed, BASS_APPROACH, si as u64);
                let ap = target
                    + if r.uniform() < 0.5 {
                        -1
                    } else if target > root {
                        -2
                    } else {
                        2
                    };
                notes.push(Note { beat: b + 1.0, dur: 0.9, midi: ap, vel: 0.7 });
            } else {
                notes.push(Note { beat: b, dur: rem.min(held) - 0.08, midi: m, vel: if first { 0.9 } else { 0.75 } });
            }
            b += step;
        }
    }

    notes
        .iter()
        .enumerate()
        .map(|(k, n)| {
            let mut r = Rng::event(seed, BASS_TIMING, k as u64);
            PluckNote {
                t0: tl.to_time(n.beat) + JITTER * r.bipolar(),
                t1: tl.to_time(n.beat + n.dur),
                midi: n.midi as f32,
                vel: n.vel,
            }
        })
        .collect()
}

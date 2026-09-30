//! The violin part: the instrumental lead (intro, interludes, outro) folded
//! into E4-D6 (MIDI 64-86) at 0.65 unless the break lead is the guitar, a
//! counter-line in G4-D6 over the lifted sections, long tones in D4-G5 over
//! the bridge, and fills in A4-E6 after the lines of verses after the first.
//! Every note has vibrato; the instrument plans the bowing. A written break
//! tune (schema 3) keeps its pitches, plays at 0.7 on the beat and 0.6 off
//! it, and has vibrato only on notes of a beat or more.

use compose::prepare::Prepared;
use sfcore::math::fold_octave;
use sfcore::random::{tag, Tag};
use song::events::BowNote;
use song::{BreakLead, SectionKind, Song};

use crate::lines::{counter_line, fills, LineNote};

const VIOLIN_COUNTER: Tag = tag("violin.counter");
const VIOLIN_BRIDGE: Tag = tag("violin.bridge");
const VIOLIN_FILL: Tag = tag("violin.fill");

/// Lead range, MIDI.
const LEAD_LO: f64 = 64.0;
const LEAD_HI: f64 = 86.0;
/// Lead level.
const LEAD_VEL: f32 = 0.65;
/// Written break notes: the level of a note that starts on a beat, and of
/// the others. Vibrato only on a note of at least one beat.
const WRITTEN_ON: f32 = 0.7;
const WRITTEN_OFF: f32 = 0.6;
/// The lead lifts the bow this long before the written end, seconds.
const LEAD_LIFT: f64 = 0.02;

fn bow(n: &LineNote) -> BowNote {
    BowNote {
        t0: n.t0,
        t1: n.t1,
        midi: n.midi as f32,
        vel: n.vel,
        vibrato: true,
    }
}

/// The violin's notes.
pub fn plan(song: &Song, prepared: &Prepared, seed: u64) -> Vec<BowNote> {
    let form = &prepared.form;
    let tl = &prepared.timeline;
    let lead = &prepared.comp.lead;
    let mut notes: Vec<BowNote> = Vec::new();
    if song.break_lead != BreakLead::Guitar {
        notes.extend(prepared.comp.inst.iter().map(|n| {
            let (vel, vibrato) = if n.written {
                let on_beat = (n.beat - n.beat.round()).abs() < 1e-6;
                (
                    if on_beat { WRITTEN_ON } else { WRITTEN_OFF },
                    n.dur >= 1.0 - 1e-9,
                )
            } else {
                (LEAD_VEL, true)
            };
            BowNote {
                t0: tl.to_time(n.beat),
                t1: tl.to_time(n.beat + n.dur) - LEAD_LIFT,
                midi: fold_octave(n.midi as f64, LEAD_LO, LEAD_HI) as f32,
                vel,
                vibrato,
            }
        }));
    }
    let ctr = counter_line(
        form,
        tl,
        lead,
        67,
        86,
        |s| s.is_lift(),
        seed,
        VIOLIN_COUNTER,
        false,
    );
    let br = counter_line(
        form,
        tl,
        lead,
        62,
        79,
        |s| s.kind == SectionKind::Bridge,
        seed,
        VIOLIN_BRIDGE,
        true,
    );
    let fl = fills(
        form,
        tl,
        69,
        88,
        |s| s.kind == SectionKind::Verse && s.occ > 0,
        seed,
        VIOLIN_FILL,
    );
    notes.extend(ctr.iter().chain(&br).chain(&fl).map(bow));
    notes
}

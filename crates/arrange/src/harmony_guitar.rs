//! The harmony guitar: the instrumental lead folded into G3-E5 (MIDI 55-76)
//! at 0.55 unless the break lead is the violin, fills in B3-G5 after the
//! lines of every verse, and in repeated lifted sections an arpeggio of the
//! lowest three chord tones from E4 (up, pattern 1-2-3-2) on every grid step.
//!
//! Two note lists because the instrument plays them with two presets: `lead`
//! (lead and fills, held to the note end) and `arp` (short fixed-length
//! arpeggio notes at velocity 1; the preset sets their level).
//!
//! Randomness: arpeggio segment k's onset jitter (+-4 ms) from
//! `Rng::event(seed, HG_ARP, k)`; fills from `Rng::event(seed, HG_FILL, line)`.

use compose::prepare::Prepared;
use sfcore::math::fold_octave;
use sfcore::random::{tag, Rng, Tag};
use song::events::PluckNote;
use song::{BreakLead, SectionKind, Song};

use crate::lines::fills;

const HG_FILL: Tag = tag("hg.fill");
const HG_ARP: Tag = tag("hg.arp");

/// Lead range, MIDI.
const LEAD_LO: f64 = 55.0;
const LEAD_HI: f64 = 76.0;
const LEAD_VEL: f32 = 0.55;
/// Arpeggio tones: the lowest three chord tones in this range, MIDI.
const ARP_LO: u8 = 64;
const ARP_HI: u8 = 83;
/// Arpeggio order over the three tones.
const ARP_ORDER: [usize; 4] = [0, 1, 2, 1];
/// Arpeggio onset jitter, seconds (+-).
const ARP_JITTER: f64 = 0.004;

/// The harmony guitar's two note lists.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HarmonyGuitar {
    /// Lead and fill notes.
    pub lead: Vec<PluckNote>,
    /// Arpeggio notes (fixed length; `t1` equals `t0`).
    pub arp: Vec<PluckNote>,
}

/// The harmony guitar part.
pub fn plan(song: &Song, prepared: &Prepared, seed: u64) -> HarmonyGuitar {
    let form = &prepared.form;
    let tl = &prepared.timeline;
    let mut lead: Vec<PluckNote> = Vec::new();
    if song.break_lead != BreakLead::Violin {
        lead.extend(prepared.comp.inst.iter().map(|n| PluckNote {
            t0: tl.to_time(n.beat),
            t1: tl.to_time(n.beat + n.dur),
            midi: fold_octave(n.midi as f64, LEAD_LO, LEAD_HI) as f32,
            vel: LEAD_VEL,
        }));
    }
    let fl = fills(
        form,
        tl,
        59,
        79,
        |s| s.kind == SectionKind::Verse,
        seed,
        HG_FILL,
    );
    lead.extend(fl.iter().map(|n| PluckNote {
        t0: n.t0,
        t1: n.t1,
        midi: n.midi as f32,
        vel: n.vel,
    }));

    let mut arp: Vec<PluckNote> = Vec::new();
    let step = 1.0 / form.sub() as f64;
    for (si, sg) in tl.segs.iter().enumerate() {
        if !form.sections[sg.sec].is_repeat_lift() {
            continue;
        }
        let mut tones = [0u8; 3];
        let mut n = 0;
        for m in form.chord(sg.chord).tones.tones_in(ARP_LO, ARP_HI).take(3) {
            tones[n] = m;
            n += 1;
        }
        if n == 0 {
            continue;
        }
        let mut r = Rng::event(seed, HG_ARP, si as u64);
        let mut b = sg.b0;
        let mut k = 0usize;
        while b < sg.b1 - 1e-6 {
            let t = tl.to_time(b) + ARP_JITTER * r.bipolar();
            let m = tones[ARP_ORDER[k % 4] % n];
            arp.push(PluckNote {
                t0: t,
                t1: t,
                midi: m as f32,
                vel: 1.0,
            });
            b += step;
            k += 1;
        }
    }
    HarmonyGuitar { lead, arp }
}

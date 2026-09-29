//! Singer phrasing: `song::Phrasing` (a writer's delivery and ending choice)
//! turned into the numbers the articulation and control-track code read
//! (design 5.2). Two of the table's columns are not fields here because
//! they land on existing knobs: `vibrato` and `glide` are multiplied into
//! `synth::VoiceSettings.vibrato_scale` and `.glide` when a `SingStyle`
//! resolves to settings (`VoiceSettings::from`).
//!
//! Algorithm: `phrase_notes` is time-scale shaping of note spans (sustain,
//! phrase-final length) applied once, before articulation plans anything;
//! the rest are parameters of the existing synthesis-by-rule plan (Holmes,
//! Mattingly, Shearme 1964; Klatt 1987) and the phrase-end fade.

use std::borrow::Cow;

use song::events::VocalNote;
use song::{Delivery, Endings, Phrasing};

/// Delivery and ending numbers for one singer (design 5.2 table). Flowing
/// delivery plus Released endings equals today's constants exactly, so
/// `PhrasingParams::default()` cannot change a rendered sample.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhrasingParams {
    /// Share of the gap to the next note a non-final note sustains into
    /// (`phrase_notes`); 1.0 leaves the note's written end alone.
    pub sustain: f64,
    /// Share of the inter-onset interval the onset consonants may take
    /// (replaces `articulation::ONSET_SHARE`).
    pub onset_share: f64,
    /// Share of the onset consonants' scaled span placed before the
    /// note's written onset; the rest delays the vowel start
    /// (`articulation::syllables`). 1.0 is today: the vowel lands on the
    /// beat and every consonant precedes it.
    pub lead_in: f64,
    /// Scale folded into `synth::VoiceSettings.vibrato_scale`.
    pub vibrato: f64,
    /// Scale folded into `synth::VoiceSettings.glide`.
    pub glide: f64,
    /// Per-note swell depth (`controls::shape_dynamics`; 0.16 today).
    pub swell: f64,
    /// Scale on the pre-phrase breath noise level (`controls::BREATH_AH`).
    pub breath: f64,
    /// Share of a phrase-final note's written length it keeps
    /// (`phrase_notes`); 1.0 leaves it alone.
    pub end_len: f64,
    /// Depth of the phrase-end fade (`controls::shape_dynamics`; 0.4
    /// today).
    pub fade_depth: f64,
    /// Where the phrase-end fade starts, as a share of the note (0.55
    /// today).
    pub fade_from: f64,
}

/// (sustain, onset_share, lead_in, vibrato, glide, swell, breath) per
/// delivery, design 5.2.
type DeliveryRow = (f64, f64, f64, f64, f64, f64, f64);
/// (end_len, fade_depth, fade_from) per ending, design 5.2.
type EndingRow = (f64, f64, f64);

const LEGATO: DeliveryRow = (1.00, 0.35, 1.0, 1.0, 1.4, 0.10, 1.0);
const FLOWING: DeliveryRow = (1.00, 0.45, 1.0, 1.0, 1.0, 0.16, 1.0);
const PARLANDO: DeliveryRow = (0.85, 0.55, 0.6, 0.5, 0.6, 0.00, 0.7);
const DETACHED: DeliveryRow = (0.65, 0.45, 1.0, 0.6, 0.5, 0.05, 1.0);

const HELD: EndingRow = (1.00, 0.25, 0.70);
const RELEASED: EndingRow = (1.00, 0.40, 0.55);
const CLIPPED: EndingRow = (0.60, 0.15, 0.60);

impl PhrasingParams {
    /// The parameters of one writer's phrasing choice (design 5.2 table).
    pub fn of(p: Phrasing) -> PhrasingParams {
        let (sustain, onset_share, lead_in, vibrato, glide, swell, breath) = match p.delivery {
            Delivery::Legato => LEGATO,
            Delivery::Flowing => FLOWING,
            Delivery::Parlando => PARLANDO,
            Delivery::Detached => DETACHED,
        };
        let (end_len, fade_depth, fade_from) = match p.endings {
            Endings::Held => HELD,
            Endings::Released => RELEASED,
            Endings::Clipped => CLIPPED,
        };
        PhrasingParams {
            sustain,
            onset_share,
            lead_in,
            vibrato,
            glide,
            swell,
            breath,
            end_len,
            fade_depth,
            fade_from,
        }
    }
}

impl Default for PhrasingParams {
    /// Flowing + Released: today's articulation exactly.
    fn default() -> PhrasingParams {
        PhrasingParams::of(Phrasing::default())
    }
}

/// Shortest sustained or phrase-final length `phrase_notes` produces, s.
const MIN_SHAPED: f64 = 0.05;

/// `notes` with `p.sustain` and `p.end_len` applied to each note's `t1`,
/// before articulation plans anything from them:
///
/// - A note that is not `phrase_end` and has a following note ends at
///   `min(t1, t0 + max(MIN_SHAPED, sustain * (next.t0 - t0)))`.
/// - A `phrase_end` note keeps `end_len` of its written length (`t1 - t0`),
///   at least `MIN_SHAPED`, and never more than its written length.
///
/// Borrows `notes` unchanged when `sustain == 1.0` and `end_len == 1.0`
/// (`PhrasingParams::default()`), so the default phrasing cannot change a
/// rendered sample.
pub fn phrase_notes<'a>(notes: &'a [VocalNote], p: &PhrasingParams) -> Cow<'a, [VocalNote]> {
    if p.sustain == 1.0 && p.end_len == 1.0 {
        return Cow::Borrowed(notes);
    }
    let mut out = notes.to_vec();
    for k in 0..notes.len() {
        let n = &notes[k];
        if n.phrase_end {
            if p.end_len != 1.0 {
                let len = n.t1 - n.t0;
                // Never lengthen: a note written shorter than MIN_SHAPED stays.
                out[k].t1 = n.t1.min(n.t0 + (p.end_len * len).max(MIN_SHAPED));
            }
        } else if p.sustain != 1.0 {
            if let Some(next) = notes.get(k + 1) {
                let cap = n.t0 + (p.sustain * (next.t0 - n.t0)).max(MIN_SHAPED);
                out[k].t1 = n.t1.min(cap);
            }
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(t0: f64, t1: f64, phrase_end: bool) -> VocalNote {
        VocalNote {
            t0,
            t1,
            midi: 60.0,
            phones: Vec::new(),
            amp: 1.0,
            stress: false,
            phrase_start: false,
            phrase_end,
            grace: None,
        }
    }

    #[test]
    fn of_default_equals_the_constants() {
        let p = PhrasingParams::of(Phrasing::default());
        assert_eq!(p.sustain, 1.0);
        assert_eq!(p.onset_share, 0.45);
        assert_eq!(p.lead_in, 1.0);
        assert_eq!(p.vibrato, 1.0);
        assert_eq!(p.glide, 1.0);
        assert_eq!(p.swell, 0.16);
        assert_eq!(p.breath, 1.0);
        assert_eq!(p.end_len, 1.0);
        assert_eq!(p.fade_depth, 0.4);
        assert_eq!(p.fade_from, 0.55);
        assert_eq!(p, PhrasingParams::default());
    }

    #[test]
    fn phrase_notes_borrows_on_the_default() {
        let notes = [note(0.0, 1.0, false), note(1.0, 2.0, true)];
        match phrase_notes(&notes, &PhrasingParams::default()) {
            Cow::Borrowed(s) => assert_eq!(s.as_ptr(), notes.as_ptr()),
            Cow::Owned(_) => panic!("default phrasing must borrow"),
        }
    }

    #[test]
    fn phrase_notes_caps_a_non_final_note_by_sustain() {
        let notes = [note(0.0, 1.0, false), note(2.0, 2.5, true)];
        let p = PhrasingParams {
            sustain: 0.5,
            ..PhrasingParams::default()
        };
        let out = phrase_notes(&notes, &p);
        assert!((out[0].t1 - 1.0).abs() < 1e-12); // 0.5 * (2.0 - 0.0) = 1.0 < original 1.0
        let p2 = PhrasingParams {
            sustain: 0.2,
            ..PhrasingParams::default()
        };
        let out2 = phrase_notes(&notes, &p2);
        assert!((out2[0].t1 - 0.4).abs() < 1e-12); // 0.2 * 2.0 = 0.4
        assert_eq!(out2[1].t1, notes[1].t1); // phrase_end untouched by sustain
    }

    #[test]
    fn phrase_notes_shortens_a_phrase_final_note_by_end_len() {
        let notes = [note(0.0, 1.0, false), note(1.0, 3.0, true)];
        let p = PhrasingParams {
            end_len: 0.6,
            ..PhrasingParams::default()
        };
        let out = phrase_notes(&notes, &p);
        assert_eq!(out[0].t1, notes[0].t1); // non-final untouched by end_len
        assert!((out[1].t1 - (1.0 + 0.6 * 2.0)).abs() < 1e-12);
    }

    #[test]
    fn phrase_notes_floors_at_min_shaped() {
        let notes = [note(0.0, 1.0, false), note(1.001, 1.002, true)];
        let p = PhrasingParams {
            sustain: 0.01,
            end_len: 0.01,
            ..PhrasingParams::default()
        };
        let out = phrase_notes(&notes, &p);
        assert!((out[0].t1 - 0.0 - MIN_SHAPED).abs() < 1e-12);
        assert_eq!(out[1].t1, 1.002); // shorter than the floor: left alone, never lengthened
    }
}

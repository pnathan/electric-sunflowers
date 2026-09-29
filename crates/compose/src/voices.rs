//! Transposition of a melody into a voice's comfortable range, and the
//! duet register fit (design, features-2 4.5) that picks singer B's
//! relative register offset and folds the octave term into one shared key.

use crate::melody::LeadNote;
use song::{SingerId, Voice};

/// Range penalty of `notes_midi` shifted by `t` semitones into `voice`'s
/// range: 3 per semitone of the 5th and 95th percentiles outside the range,
/// plus 0.8 per semitone of the median from the range centre. Empty input
/// scores 0 (no notes, no penalty).
pub fn range_penalty(notes_midi: &[i32], voice: Voice, t: i32) -> f64 {
    if notes_midi.is_empty() {
        return 0.0;
    }
    let range = voice.range();
    let (lo_r, hi_r) = (range.lo as i32, range.hi as i32);
    let mut ms: Vec<i32> = notes_midi.to_vec();
    ms.sort_unstable();
    let lo = ms[(ms.len() as f64 * 0.05).floor() as usize];
    let hi = ms[(ms.len() as f64 * 0.95).floor() as usize];
    let med = ms[ms.len() / 2];
    let c = (lo_r + hi_r) as f64 / 2.0;
    let l = lo + t;
    let h = hi + t;
    let m = med + t;
    (0.0f64).max((lo_r - l) as f64) * 3.0
        + (0.0f64).max((h - hi_r) as f64) * 3.0
        + ((m as f64) - c).abs() * 0.8
}

/// Octave-distance term of a shift `t`: 0.55 per semitone from a whole
/// octave (best at t a multiple of 12).
fn octave_term(t: i32) -> f64 {
    let r = t.rem_euclid(12);
    let off = r.min(12 - r);
    off as f64 * 0.55
}

/// Semitone shift (-30..=30) that best fits `melody` into `voice`'s range
/// (`range_penalty`) plus the octave term. Kept bit-identical to the
/// pre-duet formula: `choose_transpose_duet` with one singer at share 1
/// computes the same sum, but this is the direct path solo songs use.
pub fn choose_transpose(melody: &[LeadNote], voice: Voice) -> i32 {
    if melody.is_empty() {
        return 0;
    }
    let ms: Vec<i32> = melody.iter().map(|n| n.midi).collect();
    let mut best = 0i32;
    let mut bs = 1e9f64;
    for t in -30..=30 {
        let s = range_penalty(&ms, voice, t) + octave_term(t);
        if s < bs {
            bs = s;
            best = t;
        }
    }
    best
}

/// Singer B's register relative to A (design 4.5): `delta` is the
/// difference of the voices' range centres in semitones; `o` is `delta / 12`
/// rounded and clamped to -1..=1 (the octave B's melody notes are shifted
/// after composition); `d` is the residual clamped to -5..=5 (added to
/// singer B's line centre while composing). Baritone/alto: delta 10, o 1,
/// d -2.
pub fn duet_register(voice_a: Voice, voice_b: Voice) -> (i32, f64) {
    let delta = voice_b.range().centre() - voice_a.range().centre();
    let o = (delta / 12.0).round().clamp(-1.0, 1.0) as i32;
    let d = (delta - 12.0 * o as f64).clamp(-5.0, 5.0);
    (o, d)
}

/// One key for both singers (design 4.5, `choose_transpose_duet`): the sum
/// over each singer of their share of melody notes times `range_penalty`
/// for their own voice, plus the octave term. With one singer at share 1
/// this is `choose_transpose`'s sum in a different order (floating-point
/// identical only when the other singer contributes no notes); the solo
/// path always calls `choose_transpose`, never this function.
pub fn choose_transpose_duet(lead: &[LeadNote], voice_a: Voice, voice_b: Voice) -> i32 {
    if lead.is_empty() {
        return 0;
    }
    let a_notes: Vec<i32> = lead
        .iter()
        .filter(|n| n.singer == SingerId::A)
        .map(|n| n.midi)
        .collect();
    let b_notes: Vec<i32> = lead
        .iter()
        .filter(|n| n.singer == SingerId::B)
        .map(|n| n.midi)
        .collect();
    let total = lead.len() as f64;
    let share_a = a_notes.len() as f64 / total;
    let share_b = b_notes.len() as f64 / total;

    let mut best = 0i32;
    let mut bs = 1e9f64;
    for t in -30..=30 {
        let s = share_a * range_penalty(&a_notes, voice_a, t)
            + share_b * range_penalty(&b_notes, voice_b, t)
            + octave_term(t);
        if s < bs {
            bs = s;
            best = t;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_transpose_pulls_toward_range() {
        // All notes far above the baritone range: expect a downward shift.
        let syl = song::Syllable {
            text: String::new(),
            word: 0,
            stress: false,
            word_start: false,
            word_end: false,
            phones: vec![],
        };
        let melody: Vec<LeadNote> = (0..20)
            .map(|i| LeadNote {
                beat: 0.0,
                dur: 1.0,
                midi: 90 + (i % 3),
                syl: syl.clone(),
                line_idx: 0,
                i: 0,
                stress: false,
                phrase_start: false,
                phrase_end: false,
                grace: None,
                lift: false,
                singer: SingerId::A,
                t0: 0.0,
                t1: 0.0,
            })
            .collect();
        let tr = choose_transpose(&melody, Voice::Baritone);
        assert!(tr < 0, "expected a downward transpose, got {tr}");
        assert_eq!(choose_transpose(&[], Voice::Alto), 0);
    }

    #[test]
    fn duet_register_baritone_alto() {
        let (o, d) = duet_register(Voice::Baritone, Voice::Alto);
        assert_eq!(o, 1);
        assert!((d - -2.0).abs() < 1e-9, "d = {d}");
    }

    #[test]
    fn duet_register_same_voice_is_unison() {
        let (o, d) = duet_register(Voice::Baritone, Voice::Baritone);
        assert_eq!(o, 0);
        assert!((d - 0.0).abs() < 1e-9);
    }

    #[test]
    fn choose_transpose_duet_reduces_to_solo_with_one_singer() {
        let syl = song::Syllable {
            text: String::new(),
            word: 0,
            stress: false,
            word_start: false,
            word_end: false,
            phones: vec![],
        };
        let melody: Vec<LeadNote> = (0..20)
            .map(|i| LeadNote {
                beat: 0.0,
                dur: 1.0,
                midi: 90 + (i % 3),
                syl: syl.clone(),
                line_idx: 0,
                i: 0,
                stress: false,
                phrase_start: false,
                phrase_end: false,
                grace: None,
                lift: false,
                singer: SingerId::A,
                t0: 0.0,
                t1: 0.0,
            })
            .collect();
        let solo = choose_transpose(&melody, Voice::Baritone);
        let duet = choose_transpose_duet(&melody, Voice::Baritone, Voice::Alto);
        assert_eq!(solo, duet);
    }
}

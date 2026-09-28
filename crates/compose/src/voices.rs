//! Transposition of a melody into a voice's comfortable range.

use crate::melody::LeadNote;
use song::Voice;

/// Semitone shift (-30..=30) that best fits `melody` into `voice`'s range:
/// penalties of 3 per semitone of the 5th and 95th percentiles outside the
/// range, 0.8 per semitone of the median from the range centre, and 0.55
/// per semitone of the shift's distance from a whole octave.
pub fn choose_transpose(melody: &[LeadNote], voice: Voice) -> i32 {
    if melody.is_empty() {
        return 0;
    }
    let range = voice.range();
    let (lo_r, hi_r) = (range.lo as i32, range.hi as i32);
    let mut ms: Vec<i32> = melody.iter().map(|n| n.midi).collect();
    ms.sort_unstable();
    let lo = ms[(ms.len() as f64 * 0.05).floor() as usize];
    let hi = ms[(ms.len() as f64 * 0.95).floor() as usize];
    let med = ms[ms.len() / 2];
    let c = (lo_r + hi_r) as f64 / 2.0;

    let mut best = 0i32;
    let mut bs = 1e9f64;
    for t in -30..=30 {
        let l = lo + t;
        let h = hi + t;
        let m = med + t;
        let mut s = (0.0f64).max((lo_r - l) as f64) * 3.0 + (0.0f64).max((h - hi_r) as f64) * 3.0
            + ((m as f64) - c).abs() * 0.8;
        let r = t.rem_euclid(12);
        let off = r.min(12 - r);
        s += off as f64 * 0.55;
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
                t0: 0.0,
                t1: 0.0,
            })
            .collect();
        let tr = choose_transpose(&melody, Voice::Baritone);
        assert!(tr < 0, "expected a downward transpose, got {tr}");
        assert_eq!(choose_transpose(&[], Voice::Alto), 0);
    }
}

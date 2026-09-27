//! Voices: the `VOICES` preset table and `chooseTranspose`.
//! Ports engine.js lines ~843-860 (right above `/* voices */` marker) plus
//! the `VOICES` table declared just before `chooseTranspose`.

use crate::melody::LeadNote;

/// One `VOICES` entry. Every field the JS object has is kept, since the
/// voice-synthesis crate reuses this data wholesale.
#[derive(Clone, Copy, Debug)]
pub struct VoiceParams {
    pub cons_scale: f64,
    pub rd: f64,
    pub label: &'static str,
    pub lo: i32,
    pub hi: i32,
    pub fs: f64,
    pub f1s: f64,
    pub breath: f64,
    pub vib_rate: f64,
    pub vib_depth: f64,
    pub oq: f64,
    pub tilt: f64,
    pub jitter: f64,
    pub shimmer: f64,
    pub sf: f64,
}

/// VOICES(voiceKey): looks up one preset by name; panics on an unknown key,
/// as the JS `VOICES[voice]` access would yield `undefined` and then fail
/// downstream on first field read.
pub fn voice_params(name: &str) -> VoiceParams {
    match name {
        "baritone" => VoiceParams {
            cons_scale: 1.2, rd: 1.15, label: "Baritone", lo: 45, hi: 65,
            fs: 1.0, f1s: 1.0, breath: 0.10, vib_rate: 5.1, vib_depth: 0.30,
            oq: 0.62, tilt: 3800.0, jitter: 0.004, shimmer: 0.05, sf: 0.45,
        },
        "tenor" => VoiceParams {
            cons_scale: 1.25, rd: 1.0, label: "Tenor", lo: 50, hi: 70,
            fs: 1.04, f1s: 1.02, breath: 0.07, vib_rate: 5.6, vib_depth: 0.36,
            oq: 0.56, tilt: 5000.0, jitter: 0.003, shimmer: 0.035, sf: 0.6,
        },
        "alto" => VoiceParams {
            cons_scale: 1.35, rd: 1.25, label: "Alto", lo: 55, hi: 75,
            fs: 1.16, f1s: 1.08, breath: 0.16, vib_rate: 5.3, vib_depth: 0.32,
            oq: 0.66, tilt: 4300.0, jitter: 0.003, shimmer: 0.04, sf: 0.2,
        },
        "soprano" => VoiceParams {
            cons_scale: 1.4, rd: 1.05, label: "Soprano", lo: 60, hi: 81,
            fs: 1.22, f1s: 1.12, breath: 0.09, vib_rate: 5.8, vib_depth: 0.42,
            oq: 0.6, tilt: 5600.0, jitter: 0.0025, shimmer: 0.03, sf: 0.25,
        },
        "bass" => VoiceParams {
            cons_scale: 1.15, rd: 1.2, label: "Bass", lo: 38, hi: 58,
            fs: 0.94, f1s: 0.96, breath: 0.1, vib_rate: 4.9, vib_depth: 0.22,
            oq: 0.64, tilt: 3200.0, jitter: 0.004, shimmer: 0.04, sf: 0.35,
        },
        other => panic!("VOICES: unknown voice key {other:?}"),
    }
}

/// chooseTranspose(melody,voice)
pub fn choose_transpose(melody: &[LeadNote], voice: &str) -> i32 {
    let p = voice_params(voice);
    let mut ms: Vec<i32> = melody.iter().map(|n| n.midi).collect();
    ms.sort_unstable();
    let lo = ms[(ms.len() as f64 * 0.05).floor() as usize];
    let hi = ms[(ms.len() as f64 * 0.95).floor() as usize];
    let med = ms[ms.len() / 2];
    let c = (p.lo + p.hi) as f64 / 2.0;

    let mut best = 0i32;
    let mut bs = 1e9f64;
    for t in -30..=30 {
        let l = lo + t;
        let h = hi + t;
        let m = med + t;
        let mut s = (0.0f64).max((p.lo - l) as f64) * 3.0 + (0.0f64).max((h - p.hi) as f64) * 3.0
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
    fn voice_params_match_js_table() {
        let b = voice_params("bass");
        assert_eq!(b.lo, 38);
        assert_eq!(b.hi, 58);
        assert_eq!(b.label, "Bass");
    }

    #[test]
    #[should_panic]
    fn unknown_voice_panics() {
        voice_params("nope");
    }

    #[test]
    fn choose_transpose_pulls_toward_range() {
        // all notes far above baritone's comfortable range: expect a negative shift
        let melody: Vec<LeadNote> = (0..20)
            .map(|i| LeadNote {
                beat: 0.0,
                dur: 1.0,
                midi: 90 + (i % 3),
                syl: crate::song::Syllable {
                    text: String::new(), stress: false, word_idx: 0, first: false,
                    last: false, word: String::new(), ph: vec![],
                },
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
        let tr = choose_transpose(&melody, "baritone");
        assert!(tr < 0, "expected a downward transpose, got {tr}");
    }
}

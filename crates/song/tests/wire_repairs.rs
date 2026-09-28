//! `normalize` repairs for lyric parts that are not syllables, and tempo
//! rounding with `f64::round` (design sections 4 and 7).

use serde_json::{json, Value};
use song::wire::{normalize_value, Repair};

fn song_with(syl: &str, ph: Option<&str>, tempo: Value) -> Value {
    let mut line = json!({"syl": syl, "chords": ["C"]});
    if let Some(p) = ph {
        line["ph"] = json!(p);
    }
    json!({
        "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": tempo,
        "guitar": "strum", "voice": "tenor",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                 "choir": false, "harmonies": false, "doubles": false},
        "sections": [
            {"type": "verse", "lines": [{"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}, line]}
        ]
    })
}

fn dropped(r: &[Repair]) -> Vec<String> {
    r.iter()
        .filter_map(|x| match x {
            Repair::DroppedSyllable { section: 0, line: 1, text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn part_without_letter_or_digit_is_repaired() {
    let (s, r) = normalize_value(&song_with("*hold ... on-", Some("hh ow l d|aa n"), json!(90))).unwrap();
    assert_eq!(dropped(&r), vec!["...".to_string()], "{r:?}");
    let syls = &s.sections[0].lines()[1].syllables;
    assert_eq!(syls.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(), ["hold", "on"]);
}

#[test]
fn stars_and_lone_hyphens_are_repaired() {
    let (_, r) = normalize_value(&song_with("go - * gone --", None, json!(90))).unwrap();
    assert_eq!(dropped(&r), ["-", "*", "--"], "{r:?}");
}

#[test]
fn hyphen_splits_record_nothing() {
    let (s, r) = normalize_value(&song_with("wa-ter-fall", None, json!(90))).unwrap();
    assert!(dropped(&r).is_empty(), "{r:?}");
    assert_eq!(s.sections[0].lines()[1].syllables.len(), 3);
}

#[test]
fn line_of_only_punctuation_records_both_repairs() {
    let (s, r) = normalize_value(&song_with("... !", None, json!(90))).unwrap();
    assert_eq!(dropped(&r), ["...", "!"], "{r:?}");
    assert!(r.contains(&Repair::DroppedLine { section: 0, line: 1 }), "{r:?}");
    assert_eq!(s.sections[0].lines().len(), 1);
}

#[test]
fn tempo_rounds_half_away_from_zero() {
    for (t, want) in [(96.5, 97.0), (99.5, 100.0), (100.49, 100.0), (120.0, 120.0), (52.5, 53.0)] {
        let (s, r) = normalize_value(&song_with("one", None, json!(t))).unwrap();
        assert_eq!(s.tempo_bpm, want, "{t}");
        assert!(r.iter().all(|x| !matches!(x, Repair::ClampedTempo { .. })), "{t}: {r:?}");
    }
    // Just below one half: floor(x + 0.5) gives 1 (x + 0.5 rounds up to 1.0), f64::round gives 0;
    // both are clamped to the range floor and reported.
    let x = 0.5 - f64::EPSILON / 4.0;
    assert_eq!(x.round(), 0.0);
    let (s, r) = normalize_value(&song_with("one", None, json!(x))).unwrap();
    assert_eq!(s.tempo_bpm, 52.0);
    assert!(r.contains(&Repair::ClampedTempo { from: x, to: 52.0 }), "{r:?}");
}

//! Schema 3: solfege tunes on lines and sections, named tunes, and their
//! repairs and version gates.

use serde_json::{json, Value};
use song::tune::TunePitch;
use song::wire::{normalize_value, to_wire, Repair};
use song::Song;

fn song(v3: bool, sections: Value, extra: Value) -> Value {
    let mut v = json!({
        "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": 90,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                 "choir": false, "harmonies": false, "doubles": false},
        "sections": sections
    });
    if v3 {
        v["schema_version"] = json!(3);
    }
    for (k, x) in extra.as_object().into_iter().flatten() {
        v[k] = x.clone();
    }
    v
}

fn read(v: &Value) -> (Song, Vec<Repair>) {
    let (s, r) = normalize_value(v).expect("song reads");
    let r = r
        .into_iter()
        .filter(|x| !matches!(x, Repair::PhonemeFallback { .. }))
        .collect();
    (s, r)
}

fn verse(lines: Value) -> Value {
    json!({"type": "verse", "lines": lines})
}

#[test]
fn a_line_tune_is_read() {
    let v = song(
        true,
        json!([verse(json!([
            {"syl": "*one *two three", "chords": ["C"], "tune": "d m, s' - "}
        ]))]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    let t = s.sections[0].lines()[0].tune.clone().unwrap();
    assert_eq!(t.len(), 3);
    assert_eq!(
        t[0].pitch,
        Some(TunePitch {
            semis: 0,
            octave: 0
        })
    );
    assert_eq!(
        t[1].pitch,
        Some(TunePitch {
            semis: 4,
            octave: -1
        })
    );
    assert_eq!(
        t[2].pitch,
        Some(TunePitch {
            semis: 7,
            octave: 1
        })
    );
    assert_eq!(t[2].hold, 1);
}

#[test]
fn melisma_notes_count() {
    let v = song(
        true,
        json!([verse(json!([
            {"syl": "*glo~3-ry", "chords": ["C"], "tune": "s m r d"}
        ]))]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    assert!(s.sections[0].lines()[0].tune.is_some());
}

#[test]
fn a_count_mismatch_drops_the_tune() {
    let v = song(
        true,
        json!([verse(json!([
            {"syl": "*one *two three", "chords": ["C"], "tune": "d m"}
        ]))]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert_eq!(
        r,
        vec![Repair::TuneLength {
            section: 0,
            line: 0,
            tune: 2,
            notes: 3
        }]
    );
    assert!(s.sections[0].lines()[0].tune.is_none());
}

#[test]
fn a_bad_token_drops_the_tune() {
    let v = song(
        true,
        json!([verse(json!([
            {"syl": "*one *two", "chords": ["C"], "tune": "d q"}
        ]))]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert_eq!(
        r,
        vec![Repair::TuneToken {
            section: 0,
            line: 0,
            token: "q".into()
        }]
    );
    assert!(s.sections[0].lines()[0].tune.is_none());
}

#[test]
fn named_tunes_wrap_over_lines_and_a_line_tune_wins() {
    let v = song(
        true,
        json!([
            {"type": "verse", "tune": "A", "lines": [
                {"syl": "*one *two", "chords": ["C"]},
                {"syl": "*three *four", "chords": ["C"]},
                {"syl": "*five *six", "chords": ["C"], "tune": "s s"}
            ]},
            {"type": "chorus", "tune": "A", "lines": [
                {"syl": "*seven *eight", "chords": ["C"]}
            ]}
        ]),
        json!({"tunes": {"A": ["d m", "s m"]}}),
    );
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    let tunes: Vec<Vec<u8>> = s
        .sections
        .iter()
        .flat_map(|x| x.lines().iter())
        .map(|l| {
            l.tune
                .as_ref()
                .unwrap()
                .iter()
                .map(|n| n.pitch.unwrap().semis)
                .collect()
        })
        .collect();
    assert_eq!(tunes, [vec![0, 4], vec![7, 4], vec![7, 7], vec![0, 4]]);
}

#[test]
fn the_list_form_of_tunes_reads_the_same() {
    let v = song(
        true,
        json!([{"type": "verse", "tune": "A", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]),
        json!({"tunes": [{"name": "A", "lines": ["d m"]}]}),
    );
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    assert!(s.sections[0].lines()[0].tune.is_some());
}

#[test]
fn an_unknown_tune_name_is_ignored() {
    let v = song(
        true,
        json!([{"type": "verse", "tune": "Z", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]),
        json!({"tunes": {"A": ["d m"]}}),
    );
    let (s, r) = read(&v);
    assert_eq!(
        r,
        vec![Repair::UnknownTune {
            section: 0,
            name: "Z".into()
        }]
    );
    assert!(s.sections[0].lines()[0].tune.is_none());
}

#[test]
fn tune_fields_need_version_3() {
    let body = json!([{"type": "verse", "tune": "A", "lines": [
        {"syl": "*one *two", "chords": ["C"], "tune": "d m"}]}]);
    for declared in [1, 2] {
        let mut v = song(false, body.clone(), json!({"tunes": {"A": ["d m"]}}));
        v["schema_version"] = json!(declared);
        let (s, r) = read(&v);
        assert_eq!(s.schema_version, declared);
        assert_eq!(r.len(), 3, "{r:?}");
        assert!(r
            .iter()
            .all(|x| matches!(x, Repair::FieldNeedsSchema { needs: 3, .. })));
        assert!(s.sections[0].lines()[0].tune.is_none());
    }
}

#[test]
fn tune_fields_without_a_version_infer_three() {
    let v = song(
        false,
        json!([verse(json!([
            {"syl": "*one *two", "chords": ["C"], "tune": "d m"}
        ]))]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert_eq!(s.schema_version, 3);
    assert_eq!(r, vec![Repair::SchemaVersionInferred(3)]);
}

#[test]
fn a_v2_song_reads_as_before() {
    let v = song(
        false,
        json!([verse(json!([{"syl": "*one *two", "chords": ["C"]}]))]),
        json!({"schema_version": 2}),
    );
    let (s, r) = read(&v);
    assert_eq!(s.schema_version, 2);
    assert!(r.is_empty());
    assert!(s.sections[0].lines().iter().all(|l| l.tune.is_none()));
    let w = to_wire(&s);
    assert!(!w.to_string().contains("\"tune"));
    assert_eq!(read(&w).0, s);
}

#[test]
fn tunes_round_trip_through_the_wire() {
    let v = song(
        true,
        json!([{"type": "verse", "tune": "A", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]),
        json!({"tunes": {"A": ["d, m' -"]}}),
    );
    let (s, _) = read(&v);
    let (s2, r2) = read(&to_wire(&s));
    assert!(r2.is_empty(), "{r2:?}");
    assert_eq!(s2, s);
}

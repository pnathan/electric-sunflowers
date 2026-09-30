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

// ---------------------------------------------------------------- break tunes

fn with_break(meter: &str, tune: Value, bars: usize, extra: Value) -> Value {
    let mut v = song(
        true,
        json!([
            {"type": "intro", "chords": vec!["C"; bars], "tune": tune},
            verse(json!([{"syl": "*one *two", "chords": ["C"]}]))
        ]),
        extra,
    );
    v["meter"] = json!(meter);
    v
}

fn break_ticks_of(s: &Song) -> Vec<u16> {
    s.sections[0]
        .break_tune
        .as_ref()
        .expect("break tune")
        .iter()
        .map(|n| n.ticks)
        .collect()
}

#[test]
fn a_break_tune_is_read_with_lengths_dots_and_rests() {
    let (s, r) = read(&with_break(
        "4/4",
        json!("d8 r16 m16 s4 z4 l2. "),
        2,
        json!({}),
    ));
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(break_ticks_of(&s), [12, 6, 6, 24, 24, 72]);
    let t = s.sections[0].break_tune.as_ref().unwrap();
    assert_eq!(t[4].pitch, None);
    assert!(s.sections[1].break_tune.is_none());
}

#[test]
fn break_bar_lines_are_checked() {
    let (_, r) = read(&with_break(
        "4/4",
        json!("d4 d4 d4 d4 | s4 s4 s4 | d1 |"),
        3,
        json!({}),
    ));
    assert_eq!(
        r,
        vec![Repair::BreakTuneBar {
            section: 0,
            bar: 2,
            ticks: 72,
            expected: 96
        }]
    );
    // 6/8: a bar is six eighths.
    let (_, r) = read(&with_break(
        "6/8",
        json!("d8 d8 d8 s8 s8 s8 | s4 m4 d4. |"),
        3,
        json!({}),
    ));
    let bars: Vec<&Repair> = r
        .iter()
        .filter(|x| matches!(x, Repair::BreakTuneBar { .. }))
        .collect();
    assert_eq!(
        bars,
        [&Repair::BreakTuneBar {
            section: 0,
            bar: 2,
            ticks: 84,
            expected: 72
        }]
    );
}

#[test]
fn a_break_tune_longer_than_its_section_is_cut() {
    let (s, r) = read(&with_break("4/4", json!("d1 m1 s1"), 2, json!({})));
    assert_eq!(
        r,
        vec![Repair::BreakTuneCut {
            section: 0,
            tune: 288,
            room: 192
        }]
    );
    assert_eq!(break_ticks_of(&s), [96, 96]);
    // The cut can fall inside a note.
    let (s, _) = read(&with_break("4/4", json!("d2. m2."), 1, json!({})));
    assert_eq!(break_ticks_of(&s), [72, 24]);
}

#[test]
fn a_bad_break_token_drops_the_tune() {
    let (s, r) = read(&with_break("4/4", json!("d8 x8 m8"), 2, json!({})));
    assert!(s.sections[0].break_tune.is_none());
    assert_eq!(
        r,
        vec![Repair::BreakTuneToken {
            section: 0,
            token: "x8".into()
        }]
    );
    let (s, r) = read(&with_break("4/4", json!("d m s"), 2, json!({})));
    assert!(s.sections[0].break_tune.is_none());
    assert!(matches!(r[0], Repair::BreakTuneToken { .. }), "{r:?}");
}

#[test]
fn a_break_tune_may_name_an_entry_of_tunes() {
    let (s, r) = read(&with_break(
        "4/4",
        json!("REEL"),
        2,
        json!({"tunes": {"REEL": ["d4 d4 d4 d4 |", "s4 s4 s2"]}}),
    ));
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(break_ticks_of(&s), [24, 24, 24, 24, 24, 24, 48]);
    // The list form of the JSON schema reads too.
    let (s, _) = read(&with_break(
        "4/4",
        json!("REEL"),
        2,
        json!({"tunes": [{"name": "REEL", "lines": ["d1", "s1"]}]}),
    ));
    assert_eq!(break_ticks_of(&s), [96, 96]);
    // A name that is not there.
    let (s, r) = read(&with_break("4/4", json!("NOPE"), 2, json!({})));
    assert!(s.sections[0].break_tune.is_none());
    assert_eq!(
        r,
        vec![Repair::UnknownTune {
            section: 0,
            name: "NOPE".into()
        }]
    );
}

#[test]
fn a_same_break_copies_its_source_tune() {
    let v = song(
        true,
        json!([
            {"type": "intro", "chords": ["C", "G"], "tune": "d1 s1"},
            verse(json!([{"syl": "*one *two", "chords": ["C"]}])),
            {"type": "intro", "same": true}
        ]),
        json!({}),
    );
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(s.sections[2].repeat_of, Some(0));
    assert_eq!(s.sections[2].break_tune, s.sections[0].break_tune);
    assert!(s.sections[2].break_tune.is_some());
}

#[test]
fn break_tunes_round_trip() {
    let (s, _) = read(&with_break(
        "6/8",
        json!("d4. z8 s,8 m16 r16 d'4"),
        2,
        json!({}),
    ));
    let (back, r) = read(&to_wire(&s));
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(back, s);
}

#[test]
fn a_break_tune_needs_schema_three() {
    let mut v = with_break("4/4", json!("d1"), 1, json!({}));
    v["schema_version"] = json!(2);
    let (s, r) = read(&v);
    assert!(s.sections[0].break_tune.is_none());
    assert!(r.iter().any(|x| matches!(
        x,
        Repair::FieldNeedsSchema {
            field: "tune",
            needs: 3,
            ..
        }
    )));
}

//! Duet parts: wire `sing`/`lead`/`blend`/`duet`, the typed `Part`/`Duet`
//! model, and every repair of design section 4.4.

use serde_json::{json, Value};
use song::wire::{normalize_value, to_wire, Repair};
use song::{Blend, Part, SectionBody, SingerId, Song, Voice};

const DEMO: &str = include_str!("../../engine/src/demo.json");

/// A minimal song, `sections` substituted in whole, `extra` merged over the
/// top level (a key set to `null` removes it).
fn song_with(sections: Value, extra: Value) -> Value {
    let mut v = json!({
        "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": 90,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                 "choir": false, "harmonies": false, "doubles": false},
        "sections": sections
    });
    for (k, x) in extra.as_object().into_iter().flatten() {
        if x.is_null() {
            v.as_object_mut().map(|o| o.remove(k));
        } else {
            v[k] = x.clone();
        }
    }
    v
}

fn line(syl: &str, ph: &str, extra: Value) -> Value {
    let mut l = json!({"syl": syl, "ph": ph, "chords": ["C"]});
    for (k, x) in extra.as_object().into_iter().flatten() {
        l[k] = x.clone();
    }
    l
}

fn parts(s: &Song) -> Vec<Part> {
    s.sections
        .iter()
        .flat_map(|a| match &a.body {
            SectionBody::Sung(lines) => lines.iter().map(|l| l.part).collect::<Vec<_>>(),
            SectionBody::Instrumental(_) => vec![],
        })
        .collect()
}

const A: Part = Part::Solo(SingerId::A);
const B: Part = Part::Solo(SingerId::B);

fn both(melody: SingerId, blend: Blend) -> Part {
    Part::Both { melody, blend }
}

fn duet_sections() -> Value {
    json!([{"type": "verse", "sing": "A", "lines": [
        line("one", "w ah n", json!({})),
        line("two", "t uw", json!({}))]},
        {"type": "verse", "sing": "B", "lines": [
            line("three", "th r iy", json!({})),
            line("four", "f ao r", json!({"sing": "A"}))]},
        {"type": "chorus", "sing": "both", "lead": "B", "blend": "harmony", "lines": [
            line("five", "f ay v", json!({})),
            line("six", "s ih k s", json!({"lead": "A", "blend": "octave"}))]}])
}

fn duet_song() -> Value {
    song_with(duet_sections(), json!({"duet": {"voice": "alto"}}))
}

// ---------------------------------------------------------------- header

#[test]
fn duet_not_object_is_dropped() {
    for bad in [json!("alto"), json!(5), json!([1]), json!(true)] {
        let v = song_with(duet_sections(), json!({"duet": bad}));
        let (s, r) = normalize_value(&v).unwrap();
        assert!(!s.is_duet());
        assert!(
            r.iter().any(|x| matches!(x, Repair::DuetDropped { .. })),
            "{r:?}"
        );
    }
}

#[test]
fn duet_voice_absent_or_unknown_is_dropped() {
    for bad in [json!({}), json!({"voice": "kazoo"}), json!({"voice": null})] {
        let v = song_with(duet_sections(), json!({"duet": bad}));
        let (s, r) = normalize_value(&v).unwrap();
        assert!(!s.is_duet());
        assert!(
            r.iter().any(|x| matches!(x, Repair::DuetDropped { .. })),
            "{r:?}"
        );
    }
}

#[test]
fn duet_absent_is_silent() {
    // No duet field at all, and no B/both parts: the demo's own case, but
    // spelled out here for the invariant the plan calls out explicitly.
    let v = song_with(
        json!([{"type": "verse", "lines": [line("one *two", "w ah n|t uw", json!({}))]}]),
        json!({}),
    );
    let (s, r) = normalize_value(&v).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert!(!s.is_duet());
    assert_eq!(s.phrasing, None);
    assert_eq!(s.duet, None);
    assert!(parts(&s).iter().all(|&p| p == A));
}

#[test]
fn valid_duet_has_no_repair_and_carries_voice() {
    let (s, r) = normalize_value(&duet_song()).unwrap();
    assert!(r.is_empty(), "{r:?}");
    assert!(s.is_duet());
    assert_eq!(s.voice_of(SingerId::A), Some(Voice::Baritone));
    assert_eq!(s.voice_of(SingerId::B), Some(Voice::Alto));
}

// ---------------------------------------------------------------- unused duet

#[test]
fn duet_where_b_never_sings_is_dropped() {
    let v = song_with(
        json!([{"type": "verse", "lines": [line("one *two", "w ah n|t uw", json!({}))]}]),
        json!({"duet": {"voice": "alto"}}),
    );
    let (s, r) = normalize_value(&v).unwrap();
    assert!(!s.is_duet());
    assert_eq!(r, vec![Repair::UnusedDuet]);
    assert!(parts(&s).iter().all(|&p| p == A));
}

// ---------------------------------------------------------------- sing text

#[test]
fn unknown_sing_text_on_a_line_is_a() {
    let v = duet_song();
    let mut v = v;
    v["sections"][0]["lines"][0]["sing"] = json!("xyz");
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(parts(&s)[0], A);
    assert!(
        r.iter().any(|x| matches!(x, Repair::UnknownPart { section: 0, line: Some(0), text } if text == "xyz")),
        "{r:?}"
    );
}

#[test]
fn unknown_sing_text_on_a_section_default_is_one_repair() {
    let mut v = duet_song();
    v["sections"][0]["sing"] = json!("xyz");
    let (s, r) = normalize_value(&v).unwrap();
    // Both lines of the section inherit the bad default, but the section
    // itself is reported once, with no line index.
    assert_eq!(parts(&s)[0], A);
    assert_eq!(parts(&s)[1], A);
    let hits: Vec<_> = r
        .iter()
        .filter(|x| {
            matches!(
                x,
                Repair::UnknownPart {
                    section: 0,
                    line: None,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(hits.len(), 1, "{r:?}");
}

// ---------------------------------------------------------------- solo song, B/both requested

#[test]
fn b_or_both_in_a_solo_song_is_a() {
    for sing in ["B", "both"] {
        let v = song_with(
            json!([{"type": "verse", "lines": [line("one *two", "w ah n|t uw", json!({"sing": sing}))]}]),
            json!({}),
        );
        let (s, r) = normalize_value(&v).unwrap();
        assert!(!s.is_duet());
        assert_eq!(parts(&s)[0], A);
        assert!(
            r.contains(&Repair::PartWithoutDuet {
                section: 0,
                line: Some(0)
            }),
            "{sing}: {r:?}"
        );
    }
}

#[test]
fn section_default_b_in_a_solo_song_is_one_repair_per_section() {
    let v = song_with(
        json!([{"type": "verse", "sing": "both", "lines": [
            line("one", "w ah n", json!({})),
            line("two", "t uw", json!({}))]}]),
        json!({}),
    );
    let (s, r) = normalize_value(&v).unwrap();
    assert!(parts(&s).iter().all(|&p| p == A));
    let hits: Vec<_> = r
        .iter()
        .filter(|x| {
            matches!(
                x,
                Repair::PartWithoutDuet {
                    section: 0,
                    line: None
                }
            )
        })
        .collect();
    assert_eq!(hits.len(), 1, "{r:?}");
}

// ---------------------------------------------------------------- lead/blend

#[test]
fn lead_or_blend_on_an_unshared_line_is_ignored() {
    let mut v = duet_song();
    v["sections"][0]["lines"][0]["lead"] = json!("B");
    v["sections"][0]["lines"][0]["blend"] = json!("octave");
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(parts(&s)[0], A);
    assert!(
        r.contains(&Repair::IgnoredPartField {
            section: 0,
            line: 0,
            field: "lead"
        }),
        "{r:?}"
    );
    assert!(
        r.contains(&Repair::IgnoredPartField {
            section: 0,
            line: 0,
            field: "blend"
        }),
        "{r:?}"
    );
}

#[test]
fn unknown_lead_or_blend_on_a_shared_line_defaults() {
    let mut v = duet_song();
    v["sections"][2]["lines"][0]["lead"] = json!("nobody");
    v["sections"][2]["lines"][0]["blend"] = json!("unison");
    let (s, r) = normalize_value(&v).unwrap();
    assert_eq!(parts(&s)[4], both(SingerId::A, Blend::Harmony));
    assert!(
        r.contains(&Repair::DefaultedField {
            field: "lines.lead"
        }),
        "{r:?}"
    );
    assert!(
        r.contains(&Repair::DefaultedField {
            field: "lines.blend"
        }),
        "{r:?}"
    );
}

// ---------------------------------------------------------------- defaults and overrides

#[test]
fn section_defaults_and_line_overrides() {
    let (s, r) = normalize_value(&duet_song()).unwrap();
    assert!(r.is_empty(), "{r:?}");
    let p = parts(&s);
    assert_eq!(p[0], A); // section default A
    assert_eq!(p[1], A); // section default A
    assert_eq!(p[2], B); // section default B
    assert_eq!(p[3], A); // line overrides to A
    assert_eq!(p[4], both(SingerId::B, Blend::Harmony)); // section default both/B/harmony
    assert_eq!(p[5], both(SingerId::A, Blend::Octave)); // line overrides lead and blend
}

#[test]
fn same_copies_lines_with_their_parts() {
    let mut sections = duet_sections();
    sections
        .as_array_mut()
        .unwrap()
        .push(json!({"type": "chorus", "same": true}));
    let v = song_with(sections, json!({"duet": {"voice": "alto"}}));
    let (s, _) = normalize_value(&v).unwrap();
    assert_eq!(s.sections[3].repeat_of, Some(2));
    assert_eq!(s.sections[3].body, s.sections[2].body);
}

// ---------------------------------------------------------------- round trip

#[test]
fn to_wire_round_trips_a_duet_song() {
    let (s, _) = normalize_value(&duet_song()).unwrap();
    let w = to_wire(&s);
    let (s2, r2) = normalize_value(&w).unwrap();
    assert!(r2.is_empty(), "{r2:?}");
    assert_eq!(s2, s);
}

#[test]
fn demo_has_no_new_repairs_and_every_part_solo_a() {
    let v: Value = serde_json::from_str(DEMO).expect("demo.json is JSON");
    let (s, r) = normalize_value(&v).expect("demo normalises");
    assert!(r.is_empty(), "{r:?}");
    assert!(!s.is_duet());
    assert_eq!(s.phrasing, None);
    assert_eq!(s.duet, None);
    assert!(parts(&s).iter().all(|&p| p == A));
}

// ---------------------------------------------------------------- accessors

#[test]
fn voice_of_and_phrasing_of() {
    let (s, _) = normalize_value(&duet_song()).unwrap();
    assert_eq!(s.voice_of(SingerId::A), Some(Voice::Baritone));
    assert_eq!(s.voice_of(SingerId::B), Some(Voice::Alto));
    assert_eq!(s.phrasing_of(SingerId::A), song::Phrasing::default());
    assert_eq!(s.phrasing_of(SingerId::B), song::Phrasing::default());

    let solo = song_with(
        json!([{"type": "verse", "lines": [line("one *two", "w ah n|t uw", json!({}))]}]),
        json!({}),
    );
    let (s, _) = normalize_value(&solo).unwrap();
    assert_eq!(s.voice_of(SingerId::A), Some(s.voice));
    assert_eq!(s.voice_of(SingerId::B), None);
}

//! Schema versioning: `schema_version` 1 and 2, the version-2 fields (choir
//! lines, melismas, section key and rubato), and their repairs.

use serde_json::{json, Value};
use song::wire::{normalize_value, to_wire, Repair, SongError};
use song::{ChoirVoicing, Mode, Part, Pc, Rubato, SectionBody, Song};

/// A minimal song with one verse; `extra` merged over the top level (a key
/// set to `null` removes it).
fn base(extra: Value) -> Value {
    let mut v = json!({
        "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": 90,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                 "choir": false, "harmonies": false, "doubles": false},
        "sections": [{"type": "verse", "lines": [{"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
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

fn line(syl: &str, chords: Value, extra: Value) -> Value {
    let mut l = json!({"syl": syl, "chords": chords});
    for (k, x) in extra.as_object().into_iter().flatten() {
        l[k] = x.clone();
    }
    l
}

/// Reads `v`; the tests give no `ph`, so G2P fallbacks are not reported.
fn read(v: &Value) -> (Song, Vec<Repair>) {
    let (s, r) = normalize_value(v).expect("song reads");
    let r = r
        .into_iter()
        .filter(|x| !matches!(x, Repair::PhonemeFallback { .. }))
        .collect();
    (s, r)
}

fn lines(s: &Song) -> Vec<&song::Line> {
    s.sections.iter().flat_map(|a| a.lines()).collect()
}

// ------------------------------------------------------------ version

#[test]
fn absent_version_is_one_and_silent() {
    let (s, r) = read(&base(json!({})));
    assert_eq!(s.schema_version, 1);
    assert!(r.is_empty(), "{r:?}");
}

#[test]
fn versions_one_and_two_are_read() {
    for v in [1, 2] {
        let (s, r) = read(&base(json!({"schema_version": v})));
        assert_eq!(s.schema_version, v as u32);
        assert!(r.is_empty(), "{r:?}");
    }
}

#[test]
fn a_newer_version_is_refused() {
    let e = normalize_value(&base(json!({"schema_version": 3}))).unwrap_err();
    assert_eq!(e, SongError::UnsupportedSchema(3));
    assert!(e.to_string().contains("newer"));
}

#[test]
fn an_unusable_version_is_defaulted() {
    let (s, r) = read(&base(json!({"schema_version": 0})));
    assert_eq!(s.schema_version, 1);
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "schema_version"
        }]
    );
}

#[test]
fn a_string_version_reads_as_a_number() {
    let (s, _) = read(&base(json!({"schema_version": "2"})));
    assert_eq!(s.schema_version, 2);
}

#[test]
fn v2_fields_without_a_version_infer_two() {
    let (s, r) = read(&base(json!({"rubato": "light"})));
    assert_eq!(s.schema_version, 2);
    assert_eq!(s.rubato, Rubato::Light);
    assert_eq!(r, vec![Repair::SchemaVersionInferred]);
}

#[test]
fn v2_fields_in_a_version_1_document_are_ignored() {
    let mut v = base(json!({"schema_version": 1, "rubato": "free"}));
    v["sections"] = json!([
        {"type": "verse", "key": "D", "rubato": "light", "voicing": "block", "sing": "choir",
         "lines": [line("*lo~3 me", json!(["C"]), json!({"sing": "choir", "voicing": "block"}))]}
    ]);
    let (s, r) = read(&v);
    assert_eq!(s.schema_version, 1);
    assert_eq!(s.rubato, Rubato::Steady);
    assert!(s.sections[0].key_change.is_none());
    assert!(s.sections[0].rubato.is_none());
    assert!(!lines(&s)[0].part.is_choir());
    let needs: Vec<&str> = r
        .iter()
        .filter_map(|x| match x {
            Repair::FieldNeedsSchema { field, needs, .. } => {
                assert_eq!(*needs, 2);
                Some(*field)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        needs,
        vec![
            "rubato",
            "key",
            "rubato",
            "voicing",
            "sing: choir",
            "voicing",
            "sing: choir"
        ]
    );
    // A tilde stays in a version-1 syllable's text, as it always did.
    assert_eq!(lines(&s)[0].syllables[0].notes, 1);
    assert!(r.iter().all(|x| !x.to_string().is_empty()));
}

// ------------------------------------------------------------ choir lines

fn choir_song(sing: Value, band_choir: bool) -> Value {
    let mut v = base(json!({"schema_version": 2}));
    v["band"]["choir"] = json!(band_choir);
    v["sections"] = json!([
        {"type": "verse", "lines": [
            line("*one", json!(["C"]), json!({})),
            line("*two", json!(["G"]), sing)]}
    ]);
    v
}

#[test]
fn a_choir_line_is_read() {
    let (s, r) = read(&choir_song(json!({"sing": "choir"}), true));
    assert!(r.is_empty(), "{r:?}");
    let l = lines(&s);
    assert_eq!(l[0].part, Part::default());
    assert_eq!(l[1].part, Part::Choir(ChoirVoicing::Unison));
    assert!(l[1].part.is_choir());
    assert!(l[1].part.other().is_none());
}

#[test]
fn a_choir_line_switches_the_choir_on() {
    let (s, r) = read(&choir_song(json!({"sing": "Choir"}), false));
    assert!(s.band.choir);
    assert_eq!(r, vec![Repair::ChoirEnabled]);
}

#[test]
fn choir_voicing_line_and_section_default() {
    let (s, r) = read(&choir_song(
        json!({"sing": "choir", "voicing": "block"}),
        true,
    ));
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(lines(&s)[1].part, Part::Choir(ChoirVoicing::Block));

    let mut v = choir_song(json!({"sing": "choir"}), true);
    v["sections"][0]["voicing"] = json!("block");
    let (s, _) = read(&v);
    assert_eq!(lines(&s)[1].part, Part::Choir(ChoirVoicing::Block));

    v["sections"][0]["lines"][1]["voicing"] = json!("unison");
    let (s, _) = read(&v);
    assert_eq!(lines(&s)[1].part, Part::Choir(ChoirVoicing::Unison));
}

#[test]
fn an_unknown_voicing_defaults_and_a_stray_one_is_ignored() {
    let (s, r) = read(&choir_song(
        json!({"sing": "choir", "voicing": "wide"}),
        true,
    ));
    assert_eq!(lines(&s)[1].part, Part::Choir(ChoirVoicing::Unison));
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "lines.voicing"
        }]
    );
    let (_, r) = read(&choir_song(json!({"voicing": "block"}), true));
    assert_eq!(
        r,
        vec![Repair::IgnoredPartField {
            section: 0,
            line: 1,
            field: "voicing"
        }]
    );
}

#[test]
fn a_choir_section_default_reaches_its_lines() {
    let mut v = choir_song(json!({}), true);
    v["sections"][0]["sing"] = json!("choir");
    let (s, _) = read(&v);
    assert!(lines(&s).iter().all(|l| l.part.is_choir()));
}

// ------------------------------------------------------------ melismas

fn melisma(syl: &str) -> (Song, Vec<Repair>) {
    let mut v = base(json!({"schema_version": 2}));
    v["sections"] = json!([{"type": "verse", "lines": [line(syl, json!(["C"]), json!({}))]}]);
    read(&v)
}

#[test]
fn melisma_marks() {
    let (s, r) = melisma("*a~3-men");
    assert!(r.is_empty(), "{r:?}");
    let syl = &lines(&s)[0].syllables;
    assert_eq!(syl.len(), 2);
    assert_eq!((syl[0].text.as_str(), syl[0].notes), ("a", 3));
    assert_eq!((syl[1].text.as_str(), syl[1].notes), ("men", 1));

    let (s, _) = melisma("glo~ ry");
    assert_eq!(lines(&s)[0].syllables[0].notes, 2);
    assert_eq!(lines(&s)[0].syllables[0].text, "glo");
}

#[test]
fn melisma_length_is_clamped() {
    let (s, r) = melisma("oh~9");
    assert_eq!(lines(&s)[0].syllables[0].notes, song::MELISMA_MAX_NOTES);
    assert!(matches!(r[..], [Repair::ClampedMelisma { to: 4, .. }]));
    let (s, r) = melisma("oh~1");
    assert_eq!(lines(&s)[0].syllables[0].notes, 2);
    assert!(matches!(r[..], [Repair::ClampedMelisma { to: 2, .. }]));
}

#[test]
fn a_tilde_that_is_not_a_mark_stays_text() {
    let (s, _) = melisma("a~b c");
    assert_eq!(lines(&s)[0].syllables[0].notes, 1);
}

// ------------------------------------------------------------ key change

fn modulating() -> Value {
    let mut v = base(json!({"schema_version": 2}));
    v["sections"] = json!([
        {"type": "verse", "lines": [line("*one", json!(["C", "F"]), json!({}))]},
        {"type": "chorus", "lines": [line("*two", json!(["G", "C"]), json!({}))]},
        {"type": "chorus", "same": true, "key": "D"},
        {"type": "outro", "chords": ["D", "A"]}
    ]);
    v
}

fn symbols(s: &Song, i: usize) -> Vec<String> {
    s.sections[i]
        .bars()
        .flat_map(|b| b.as_slice().iter())
        .map(|&id| s.chord(id).symbol.clone())
        .collect()
}

#[test]
fn a_repeat_in_a_new_key_moves_its_chords() {
    let (s, r) = read(&modulating());
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(symbols(&s, 1), ["G", "C"]);
    assert_eq!(symbols(&s, 2), ["A", "D"]);
    let k = s.sections[2].key_change.expect("key change");
    assert_eq!((k.tonic, k.mode), (Pc::new(2), Mode::Major));
    assert!(s.sections[1].key_change.is_none());
    // The outro stays in the running key.
    assert!(s.sections[3].key_change.is_none());
    assert_eq!(s.key_at(1), (Pc::new(0), Mode::Major));
    assert_eq!(s.key_at(3), (Pc::new(2), Mode::Major));
    assert!(s.modulates());
    assert_eq!(s.sections[2].repeat_of, Some(1));
}

#[test]
fn a_repeat_after_a_change_follows_the_running_key() {
    let mut v = modulating();
    v["sections"] = json!([
        {"type": "verse", "lines": [line("*one", json!(["C"]), json!({}))]},
        {"type": "chorus", "lines": [line("*two", json!(["G"]), json!({}))]},
        {"type": "bridge", "key": "E minor", "lines": [line("*three", json!(["Em"]), json!({}))]},
        {"type": "chorus", "same": true}
    ]);
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    let k = s.sections[2].key_change.expect("bridge key");
    assert_eq!((k.tonic, k.mode), (Pc::new(4), Mode::Minor));
    // G in C is the fifth; a fourth of the way to E gives B.
    assert_eq!(symbols(&s, 3), ["B"]);
    assert!(s.sections[3].key_change.is_none());
}

#[test]
fn a_key_equal_to_the_running_key_is_no_change_and_bad_text_is_repaired() {
    let mut v = modulating();
    v["sections"][1]["key"] = json!("C major");
    let (s, r) = read(&v);
    assert!(s.sections[1].key_change.is_none());
    assert!(r.is_empty(), "{r:?}");

    v["sections"][1]["key"] = json!("xyz");
    let (s, r) = read(&v);
    assert!(s.sections[1].key_change.is_none());
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "sections.key"
        }]
    );

    // A tonic with an unknown mode word keeps the running mode.
    v["sections"][1]["key"] = json!("E lydian");
    let (s, r) = read(&v);
    let k = s.sections[1].key_change.expect("key change");
    assert_eq!((k.tonic, k.mode), (Pc::new(4), Mode::Major));
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "sections.key"
        }]
    );
}

// ------------------------------------------------------------ rubato

#[test]
fn rubato_song_and_section() {
    let mut v = base(json!({"schema_version": 2, "rubato": "light"}));
    v["sections"] = json!([
        {"type": "verse", "lines": [line("*one", json!(["C"]), json!({}))]},
        {"type": "chorus", "rubato": "free", "lines": [line("*two", json!(["G"]), json!({}))]},
        {"type": "chorus", "same": true}
    ]);
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(s.rubato, Rubato::Light);
    assert_eq!(s.rubato_at(0), Rubato::Light);
    assert_eq!(s.rubato_at(1), Rubato::Free);
    assert_eq!(s.rubato_at(2), Rubato::Free);

    v["sections"][1]["rubato"] = json!("wild");
    let (s, r) = read(&v);
    assert_eq!(s.rubato_at(1), Rubato::Light);
    assert_eq!(
        r,
        vec![Repair::DefaultedField {
            field: "sections.rubato"
        }]
    );
}

// ------------------------------------------------------------ round trip

#[test]
fn a_version_2_song_round_trips() {
    let mut v = modulating();
    v["rubato"] = json!("light");
    v["band"]["choir"] = json!(true);
    v["sections"][1]["lines"][0] = line(
        "*glo~3-ry ho~ san~4-na",
        json!(["G", "C"]),
        json!({"sing": "choir", "voicing": "block"}),
    );
    v["sections"][3]["rubato"] = json!("free");
    let (s, r) = read(&v);
    assert!(r.is_empty(), "{r:?}");
    let w = to_wire(&s);
    assert_eq!(w["schema_version"], 2);
    let (again, r2) = read(&w);
    assert!(r2.is_empty(), "{r2:?}");
    assert_eq!(again, s);
    let chorus = match &s.sections[1].body {
        SectionBody::Sung(l) => l,
        _ => panic!("sung"),
    };
    assert_eq!(chorus[0].syllables[0].notes, 3);
}

#[test]
fn a_version_1_song_round_trips_without_a_version() {
    let (s, _) = read(&base(json!({})));
    let w = to_wire(&s);
    assert!(w.get("schema_version").is_none());
    assert_eq!(read(&w).0, s);
}

// ------------------------------------------------------------ schemas

#[test]
fn schemas_per_version() {
    use song::schema::{json_schema, json_schema_for, json_schema_v1, json_schema_v2};
    assert_eq!(json_schema(), json_schema_v2());
    assert_eq!(json_schema_for(1), Some(json_schema_v1()));
    assert_eq!(json_schema_for(2), Some(json_schema_v2()));
    assert_eq!(json_schema_for(0), None);
    assert_eq!(json_schema_for(3), None);

    let v1 = json_schema_v1();
    let v2 = json_schema_v2();
    // Version 2 requires the version; version 1 accepts it and does not.
    assert_eq!(v2["properties"]["schema_version"]["enum"], json!([2]));
    assert_eq!(v1["properties"]["schema_version"]["enum"], json!([1]));
    assert!(v2["required"]
        .as_array()
        .unwrap()
        .contains(&json!("schema_version")));
    assert!(!v1["required"]
        .as_array()
        .unwrap()
        .contains(&json!("schema_version")));

    // Version-2 additions exist only in version 2.
    assert!(v1["properties"].get("rubato").is_none());
    assert_eq!(v2["properties"]["rubato"]["enum"], json!(Rubato::NAMES));
    let sec1 = &v1["properties"]["sections"]["items"]["properties"];
    let sec2 = &v2["properties"]["sections"]["items"]["properties"];
    for f in ["key", "rubato", "voicing"] {
        assert!(sec1.get(f).is_none(), "{f} in v1");
        assert!(sec2.get(f).is_some(), "{f} missing in v2");
    }
    assert!(!sec1["sing"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("choir")));
    assert!(sec2["sing"]["enum"]
        .as_array()
        .unwrap()
        .contains(&json!("choir")));
    let line2 = &sec2["lines"]["items"]["properties"];
    assert_eq!(line2["voicing"]["enum"], json!(ChoirVoicing::NAMES));
    assert!(line2["syl"]["description"].as_str().unwrap().contains('~'));
    assert!(
        v1["properties"]["sections"]["items"]["properties"]["lines"]["items"]["properties"]
            .get("voicing")
            .is_none()
    );
}

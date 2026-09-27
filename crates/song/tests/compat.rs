//! The song crate against the parser it replaces (compose::song, compose::theory,
//! compose::phonetics) on the demo song and the chord and G2P tables, and a
//! round trip through the reply format and the schema.

use serde_json::Value;
use song::chord::{parse_detail, Chord};
use song::g2p::g2p;
use song::pitch::{Pc, FLATS, SHARPS};
use song::wire::{normalize_value, to_wire};
use song::{schema, SectionBody, SectionKind};

const DEMO: &str = include_str!("../../engine/src/demo.json");

fn demo() -> Value {
    serde_json::from_str(DEMO).expect("demo.json is JSON")
}

fn pcs_old(c: &compose::theory::Chord) -> Vec<u8> {
    let mut v: Vec<u8> = c.pcs.iter().map(|&p| p as u8).collect();
    v.sort_unstable();
    v
}

fn set_old(v: &[i32]) -> Vec<u8> {
    let mut v: Vec<u8> = v.iter().map(|&p| p as u8).collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn assert_same_chord(sym: &str) {
    let old = compose::theory::parse_chord(sym);
    let new = Chord::parse(sym).unwrap_or_else(|e| panic!("{sym}: {e}"));
    let new_pcs: Vec<u8> = new.tones.iter().map(Pc::get).collect();
    assert_eq!(new_pcs, pcs_old(&old), "{sym}: tones");
    assert_eq!(new.root.get() as i32, old.root, "{sym}: root");
    assert_eq!(new.bass.get() as i32, old.bass, "{sym}: bass");
    assert_eq!(new.third.map(|p| p.get() as i32), old.third, "{sym}: third");
    assert_eq!(new.fifth.map(|p| p.get() as i32), old.fifth, "{sym}: fifth");
    assert_eq!(new.seventh.map(|p| p.get() as i32), old.seventh, "{sym}: seventh");
    let new_ess: Vec<u8> = new.essential.iter().map(Pc::get).collect();
    assert_eq!(new_ess, set_old(&old.ess), "{sym}: essential");
}

#[test]
fn demo_chords_match_compose() {
    let v = demo();
    let mut n = 0;
    for s in v["sections"].as_array().into_iter().flatten() {
        let lists = std::iter::once(&s["chords"]).chain(s["lines"].as_array().into_iter().flatten().map(|l| &l["chords"]));
        for list in lists {
            for entry in list.as_array().into_iter().flatten().filter_map(Value::as_str) {
                for tok in entry.split_whitespace() {
                    assert_same_chord(tok);
                    n += 1;
                }
            }
        }
    }
    assert!(n > 20, "only {n} chords checked");
}

#[test]
fn every_quality_on_every_root_matches_compose() {
    for (q, _) in compose::theory::qual_table() {
        for names in [&SHARPS, &FLATS] {
            for root in names.iter() {
                let sym = format!("{root}{q}");
                assert_same_chord(&sym);
                let (_, ignored) = parse_detail(&sym).expect("parses");
                assert_eq!(ignored, None, "{sym}");
                assert_same_chord(&format!("{sym}/E"));
                assert_same_chord(&format!("{sym}/Bb"));
            }
        }
    }
    // Unknown suffixes fall back to the longest known prefix, as before.
    for sym in ["C7#9", "Dm(add9)", "Gmaj7#11", "Asus4add9", "E7b9b13"] {
        assert_same_chord(sym);
    }
}

#[test]
fn demo_normalises_like_compose_with_no_repairs() {
    let v = demo();
    let (s, repairs) = normalize_value(&v).expect("demo normalises");
    assert!(repairs.is_empty(), "repairs: {repairs:?}");
    let old = compose::song::normalize_song(&v).expect("compose normalises demo");

    assert_eq!(s.title, old.title);
    assert_eq!(s.key.get() as i32, old.key_pc);
    assert_eq!(s.mode.as_str(), old.mode);
    assert_eq!(s.meter.as_str(), old.meter_name);
    assert_eq!(s.tempo_bpm, old.tempo);
    assert_eq!(s.guitar.as_str(), old.guitar);
    assert_eq!(s.voice.as_str(), old.voice.as_str());
    assert_eq!(s.band.drums.as_str(), old.band.drums);
    assert_eq!(
        [s.band.bass, s.band.harmony_guitar, s.band.harp, s.band.violin, s.band.choir, s.band.harmonies, s.band.doubles],
        [old.band.bass, old.band.harmony_guitar, old.band.harp, old.band.violin, old.band.choir, old.band.harmonies,
         old.band.doubles]
    );
    assert_eq!(s.sections.len(), old.sections.len());

    let (mut n_lines, mut n_syl, mut n_chords) = (0, 0, 0);
    for (a, b) in s.sections.iter().zip(&old.sections) {
        assert_eq!(a.kind.as_str(), b.type_);
        assert_eq!(a.repeat_of.is_some(), b.repeat);
        let bar_syms = |bars: &[song::BarChords]| -> Vec<Vec<String>> {
            bars.iter().map(|x| x.as_slice().iter().map(|&id| s.chord(id).symbol.clone()).collect()).collect()
        };
        match (&a.body, &b.lines) {
            (SectionBody::Sung(lines), Some(old_lines)) => {
                assert_eq!(lines.len(), old_lines.len());
                for (l, ol) in lines.iter().zip(old_lines) {
                    n_lines += 1;
                    assert_eq!(l.syllables.len(), ol.syls.len());
                    assert_eq!(bar_syms(&l.bars), ol.bars);
                    n_chords += l.bars.iter().map(|x| x.len()).sum::<usize>();
                    assert_eq!(l.text(), ol.text);
                    for (x, ox) in l.syllables.iter().zip(&ol.syls) {
                        n_syl += 1;
                        assert_eq!(x.text, ox.text);
                        assert_eq!(x.stress, ox.stress, "{}", x.text);
                        assert_eq!(x.word as usize, ox.word_idx);
                        assert_eq!((x.word_start, x.word_end), (ox.first, ox.last));
                        let ph: Vec<&str> = x.phones.iter().map(|p| p.symbol()).collect();
                        assert_eq!(ph, ox.ph, "{}", x.text);
                    }
                }
            }
            (SectionBody::Instrumental(bars), None) => {
                assert_eq!(bar_syms(bars), b.bars);
                n_chords += bars.iter().map(|x| x.len()).sum::<usize>();
            }
            _ => panic!("section {:?}: sung/instrumental mismatch", a.kind),
        }
    }
    assert!(n_lines >= 20 && n_syl >= 150 && n_chords >= 40, "{n_lines} lines, {n_syl} syllables, {n_chords} chords");
    assert!(s.sections.iter().any(|x| x.kind == SectionKind::Bridge));
}

#[test]
fn g2p_matches_compose() {
    let mut words: Vec<String> = [
        "love", "tion", "night", "though", "eight", "cat", "the", "sing", "church", "quick", "know", "gnome",
        "city", "yes", "rhythm", "cycle", "xylophone", "whisper", "phone", "laughter", "nation", "vision",
        "judge", "Queen", "wrote", "boat", "rain", "coin", "saw", "few", "true", "bird", "car", "for", "ice",
        "", "--", "O'Brien",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let v = demo();
    for s in v["sections"].as_array().into_iter().flatten() {
        for l in s["lines"].as_array().into_iter().flatten() {
            let syl = l["syl"].as_str().unwrap_or("");
            words.extend(syl.split(|c: char| c.is_whitespace() || c == '-').map(str::to_string));
        }
    }
    for w in &words {
        let new: Vec<&str> = g2p(w).iter().map(|p| p.symbol()).collect();
        assert_eq!(new, compose::phonetics::g2p(w), "{w:?}");
    }
}

/// Minimal JSON Schema check: type, enum, properties, required,
/// additionalProperties false, items.
fn validate(schema: &Value, v: &Value, path: &str) -> Result<(), String> {
    let ty = schema["type"].as_str().unwrap_or("");
    let ok = match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        _ => true,
    };
    if !ok {
        return Err(format!("{path}: not {ty}"));
    }
    if let Some(e) = schema["enum"].as_array() {
        if !e.contains(v) {
            return Err(format!("{path}: {v} not in enum"));
        }
    }
    if let Some(o) = v.as_object() {
        let props = schema["properties"].as_object().ok_or(format!("{path}: no properties"))?;
        for r in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if !o.contains_key(r) {
                return Err(format!("{path}: missing {r}"));
            }
        }
        for (k, x) in o {
            match props.get(k) {
                Some(ps) => validate(ps, x, &format!("{path}.{k}"))?,
                None if schema["additionalProperties"] == false => return Err(format!("{path}: extra {k}")),
                None => {}
            }
        }
    }
    if let Some(a) = v.as_array() {
        for (i, x) in a.iter().enumerate() {
            validate(&schema["items"], x, &format!("{path}[{i}]"))?;
        }
    }
    Ok(())
}

#[test]
fn schema_round_trip() {
    let sch = schema::json_schema();
    let v = demo();
    validate(&sch, &v, "demo").unwrap();

    let (s, _) = normalize_value(&v).unwrap();
    let w = to_wire(&s);
    validate(&sch, &w, "to_wire").unwrap();
    let (s2, r2) = normalize_value(&w).unwrap();
    assert!(r2.is_empty(), "{r2:?}");
    assert_eq!(s2, s);

    // Every enum spelling the schema offers is accepted by the parser with no repair.
    let enums = [
        ("mode", &sch["properties"]["mode"]),
        ("meter", &sch["properties"]["meter"]),
        ("guitar", &sch["properties"]["guitar"]),
        ("voice", &sch["properties"]["voice"]),
    ];
    for (field, p) in enums {
        for name in p["enum"].as_array().into_iter().flatten() {
            let mut x = v.clone();
            x[field] = name.clone();
            if field == "meter" {
                x["tempo"] = Value::from(60);
            }
            let (_, r) = normalize_value(&x).unwrap();
            assert!(r.is_empty(), "{field}={name}: {r:?}");
        }
    }
    for name in sch["properties"]["band"]["properties"]["drums"]["enum"].as_array().into_iter().flatten() {
        let mut x = v.clone();
        x["band"]["drums"] = name.clone();
        assert!(normalize_value(&x).unwrap().1.is_empty());
    }
    for name in sch["properties"]["sections"]["items"]["properties"]["type"]["enum"].as_array().into_iter().flatten() {
        let mut x = v.clone();
        x["sections"][1]["type"] = name.clone();
        let (s, r) = normalize_value(&x).unwrap();
        assert!(r.is_empty(), "type {name}: {r:?}");
        assert_eq!(Some(s.sections[1].kind.as_str()), name.as_str());
    }
    // The song itself serialises (for --dump-json).
    let dumped = serde_json::to_value(&s).unwrap();
    assert_eq!(dumped["meter"], "3/4");
    assert_eq!(dumped["band"]["harmonyGuitar"], true);
}

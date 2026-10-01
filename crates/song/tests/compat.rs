//! The demo song through the song crate's normaliser, and a round trip
//! through the reply format and the schema.

use serde_json::Value;
use song::wire::{normalize_value, to_wire};
use song::{schema, SectionBody, SectionKind};

const DEMO: &str = include_str!("../../engine/src/demo.json");

fn demo() -> Value {
    serde_json::from_str(DEMO).expect("demo.json is JSON")
}

#[test]
fn demo_normalises_with_no_repairs() {
    let v = demo();
    let (s, repairs) = normalize_value(&v).expect("demo normalises");
    assert!(repairs.is_empty(), "repairs: {repairs:?}");
    let (mut n_lines, mut n_syl, mut n_chords) = (0, 0, 0);
    for a in &s.sections {
        match &a.body {
            SectionBody::Sung(lines) => {
                for l in lines {
                    n_lines += 1;
                    n_syl += l.syllables.len();
                    n_chords += l.bars.iter().map(|x| x.len()).sum::<usize>();
                    assert!(
                        l.syllables.iter().all(|x| !x.phones.is_empty()),
                        "{}",
                        l.text()
                    );
                }
            }
            SectionBody::Instrumental(bars) => {
                n_chords += bars.iter().map(|x| x.len()).sum::<usize>()
            }
        }
    }
    assert!(
        n_lines >= 20 && n_syl >= 150 && n_chords >= 40,
        "{n_lines} lines, {n_syl} syllables, {n_chords} chords"
    );
    assert!(s.sections.iter().any(|x| x.kind == SectionKind::Bridge));
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
        let props = schema["properties"]
            .as_object()
            .ok_or(format!("{path}: no properties"))?;
        for r in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !o.contains_key(r) {
                return Err(format!("{path}: missing {r}"));
            }
        }
        for (k, x) in o {
            match props.get(k) {
                Some(ps) => validate(ps, x, &format!("{path}.{k}"))?,
                None if schema["additionalProperties"] == false => {
                    return Err(format!("{path}: extra {k}"))
                }
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

/// The demo (written before schema versions: version 1) against the
/// version-1 schema.
#[test]
fn schema_round_trip_v1() {
    schema_round_trip(schema::json_schema_v1(), demo(), 1);
}

/// The same song declared version 2 against the version-2 schema.
#[test]
fn schema_round_trip_v2() {
    let mut v = demo();
    v["schema_version"] = Value::from(2);
    schema_round_trip(schema::json_schema_v2(), v, 2);
}

/// The same song declared version 3 against the version-3 schema (the
/// latest, `json_schema()`).
#[test]
fn schema_round_trip_v3() {
    let mut v = demo();
    v["schema_version"] = Value::from(3);
    assert_eq!(schema::json_schema(), schema::json_schema_v3());
    schema_round_trip(schema::json_schema(), v, 3);
}

fn schema_round_trip(sch: Value, v: Value, version: u32) {
    validate(&sch, &v, "demo").unwrap();

    let (s, _) = normalize_value(&v).unwrap();
    assert_eq!(s.schema_version, version);
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
    for name in sch["properties"]["band"]["properties"]["drums"]["enum"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let mut x = v.clone();
        x["band"]["drums"] = name.clone();
        assert!(normalize_value(&x).unwrap().1.is_empty());
    }
    for name in sch["properties"]["sections"]["items"]["properties"]["type"]["enum"]
        .as_array()
        .into_iter()
        .flatten()
    {
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

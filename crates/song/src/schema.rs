//! JSON Schema of the model's song reply, for `--json-schema` on the CLI and
//! `output_config.format.schema` on the API. Every object is closed
//! (`additionalProperties: false`) and lists `required`. Enum lists come
//! from the model enums (`NAMES`), so schema and parser cannot disagree.

use crate::model::{
    Blend, Delivery, DrumKit, Endings, GuitarPattern, Meter, Mode, SectionKind, SingerId, Voice,
};
use serde_json::{json, Value};

/// `SingerId::NAMES` ("A", "B") plus "both", the enum list of `sing`.
fn sing_names() -> Vec<&'static str> {
    let mut v = SingerId::NAMES.to_vec();
    v.push("both");
    v
}

fn phrasing_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "delivery": {"type": "string", "enum": Delivery::NAMES},
            "endings": {"type": "string", "enum": Endings::NAMES}
        },
        "required": ["delivery", "endings"],
        "additionalProperties": false
    })
}

fn duet_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "voice": {"type": "string", "enum": Voice::NAMES},
            "phrasing": phrasing_schema()
        },
        "required": ["voice"],
        "additionalProperties": false
    })
}

fn line_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "syl": {"type": "string"},
            "ph": {"type": "string"},
            "chords": {"type": "array", "items": {"type": "string"}},
            "sing": {"type": "string", "enum": sing_names()},
            "lead": {"type": "string", "enum": SingerId::NAMES},
            "blend": {"type": "string", "enum": Blend::NAMES}
        },
        "required": ["syl", "ph", "chords"],
        "additionalProperties": false
    })
}

fn section_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {"type": "string", "enum": SectionKind::NAMES},
            "same": {"type": "boolean"},
            "chords": {"type": "array", "items": {"type": "string"}},
            "lines": {"type": "array", "items": line_schema()},
            "sing": {"type": "string", "enum": sing_names()},
            "lead": {"type": "string", "enum": SingerId::NAMES},
            "blend": {"type": "string", "enum": Blend::NAMES}
        },
        "required": ["type"],
        "additionalProperties": false
    })
}

fn band_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "drums": {"type": "string", "enum": DrumKit::NAMES},
            "bass": {"type": "boolean"},
            "harmonyGuitar": {"type": "boolean"},
            "harp": {"type": "boolean"},
            "violin": {"type": "boolean"},
            "choir": {"type": "boolean"},
            "harmonies": {"type": "boolean"},
            "doubles": {"type": "boolean"}
        },
        "required": ["drums", "bass", "harmonyGuitar", "harp", "violin", "choir", "harmonies", "doubles"],
        "additionalProperties": false
    })
}

/// The song reply schema.
pub fn json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "note": {"type": "string"},
            "key": {"type": "string"},
            "mode": {"type": "string", "enum": Mode::NAMES},
            "meter": {"type": "string", "enum": Meter::NAMES},
            "tempo": {"type": "number"},
            "guitar": {"type": "string", "enum": GuitarPattern::NAMES},
            "voice": {"type": "string", "enum": Voice::NAMES},
            "band": band_schema(),
            "phrasing": phrasing_schema(),
            "duet": duet_schema(),
            "sections": {"type": "array", "items": section_schema()}
        },
        "required": ["title", "note", "key", "mode", "meter", "tempo", "guitar", "voice", "band", "sections"],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &Value) -> Vec<&str> {
        v["enum"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    }

    #[test]
    fn enum_lists_match_the_enums() {
        let s = json_schema();
        let p = &s["properties"];
        assert_eq!(names(&p["mode"]), Mode::NAMES);
        assert_eq!(names(&p["meter"]), Meter::NAMES);
        assert_eq!(names(&p["guitar"]), GuitarPattern::NAMES);
        assert_eq!(names(&p["voice"]), Voice::NAMES);
        assert!(names(&p["voice"]).contains(&"bass"));
        assert_eq!(names(&p["band"]["properties"]["drums"]), DrumKit::NAMES);
        assert_eq!(
            names(&p["sections"]["items"]["properties"]["type"]),
            SectionKind::NAMES
        );
        for n in names(&p["voice"]) {
            assert!(n.parse::<Voice>().is_ok());
        }

        assert_eq!(
            names(&p["phrasing"]["properties"]["delivery"]),
            Delivery::NAMES
        );
        assert_eq!(
            names(&p["phrasing"]["properties"]["endings"]),
            Endings::NAMES
        );
        assert_eq!(names(&p["duet"]["properties"]["voice"]), Voice::NAMES);
        assert_eq!(
            names(&p["duet"]["properties"]["phrasing"]["properties"]["delivery"]),
            Delivery::NAMES
        );

        let line = &p["sections"]["items"]["properties"]["lines"]["items"]["properties"];
        assert_eq!(names(&line["sing"]), sing_names());
        assert_eq!(names(&line["lead"]), SingerId::NAMES);
        assert_eq!(names(&line["blend"]), Blend::NAMES);
        let sec = &p["sections"]["items"]["properties"];
        assert_eq!(names(&sec["sing"]), sing_names());
        assert_eq!(names(&sec["lead"]), SingerId::NAMES);
        assert_eq!(names(&sec["blend"]), Blend::NAMES);
        assert!(sing_names().contains(&"both"));
        for n in SingerId::NAMES {
            assert!(n.parse::<SingerId>().is_ok());
        }
    }

    #[test]
    fn objects_are_closed() {
        let s = json_schema();
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["band"]["additionalProperties"], false);
        assert_eq!(
            s["properties"]["sections"]["items"]["additionalProperties"],
            false
        );
        assert_eq!(
            s["properties"]["sections"]["items"]["properties"]["lines"]["items"]
                ["additionalProperties"],
            false
        );
        assert_eq!(s["properties"]["phrasing"]["additionalProperties"], false);
        assert_eq!(s["properties"]["duet"]["additionalProperties"], false);
        assert_eq!(
            s["properties"]["duet"]["properties"]["phrasing"]["additionalProperties"],
            false
        );
        assert_eq!(s["properties"]["duet"]["required"], json!(["voice"]));
        assert_eq!(
            s["properties"]["phrasing"]["required"],
            json!(["delivery", "endings"])
        );
    }
}

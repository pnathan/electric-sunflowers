//! JSON Schema of the model's song reply, for `--json-schema` on the CLI and
//! `output_config.format.schema` on the API. Every object is closed
//! (`additionalProperties: false`) and lists `required`. Enum lists come
//! from the model enums (`NAMES`), so schema and parser cannot disagree.

use crate::model::{DrumKit, GuitarPattern, Meter, Mode, SectionKind, Voice};
use serde_json::{json, Value};

fn line_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "syl": {"type": "string"},
            "ph": {"type": "string"},
            "chords": {"type": "array", "items": {"type": "string"}}
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
            "lines": {"type": "array", "items": line_schema()}
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
        v["enum"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
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
        assert_eq!(names(&p["sections"]["items"]["properties"]["type"]), SectionKind::NAMES);
        for n in names(&p["voice"]) {
            assert!(n.parse::<Voice>().is_ok());
        }
    }

    #[test]
    fn objects_are_closed() {
        let s = json_schema();
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["band"]["additionalProperties"], false);
        assert_eq!(s["properties"]["sections"]["items"]["additionalProperties"], false);
        assert_eq!(s["properties"]["sections"]["items"]["properties"]["lines"]["items"]["additionalProperties"], false);
    }
}

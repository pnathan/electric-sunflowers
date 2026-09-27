//! JSON Schema for the song reply, matching the shape `songPrompt` (prompt.rs) asks
//! for, usable both as `--json-schema` on the CLI and as `output_config.format.schema`
//! on the API. Every object declares `additionalProperties: false` and `required`.

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
            "type": {"type": "string", "enum": [
                "intro", "verse", "chorus", "prechorus", "bridge", "interlude", "outro"
            ]},
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
            "drums": {"type": "string", "enum": ["none", "brushes", "soft", "full"]},
            "bass": {"type": "boolean"},
            "harmonyGuitar": {"type": "boolean"},
            "harp": {"type": "boolean"},
            "violin": {"type": "boolean"},
            "choir": {"type": "boolean"},
            "harmonies": {"type": "boolean"},
            "doubles": {"type": "boolean"}
        },
        "required": [
            "drums", "bass", "harmonyGuitar", "harp", "violin", "choir", "harmonies", "doubles"
        ],
        "additionalProperties": false
    })
}

/// The full song object schema, matching `songPrompt`'s reply-format section.
pub fn song_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {"type": "string"},
            "note": {"type": "string"},
            "key": {"type": "string"},
            "mode": {"type": "string", "enum": ["major", "minor", "dorian", "mixolydian"]},
            "meter": {"type": "string", "enum": ["4/4", "3/4", "6/8"]},
            "tempo": {"type": "number"},
            "guitar": {"type": "string", "enum": ["strum", "fingerpick", "travis", "arpeggio"]},
            "voice": {"type": "string", "enum": ["baritone", "tenor", "alto", "soprano"]},
            "band": band_schema(),
            "sections": {"type": "array", "items": section_schema()}
        },
        "required": [
            "title", "note", "key", "mode", "meter", "tempo", "guitar", "voice", "band", "sections"
        ],
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_well_formed_and_closed() {
        let s = song_schema();
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["band"]["additionalProperties"], false);
        assert_eq!(s["properties"]["sections"]["items"]["additionalProperties"], false);
        assert!(s["required"].as_array().unwrap().iter().any(|v| v == "sections"));
    }
}

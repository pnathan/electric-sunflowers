//! JSON Schemas of the model's song reply, one per schema version, for
//! `--json-schema` on the CLI and `output_config.format.schema` on the API.
//! Every object is closed (`additionalProperties: false`) and lists
//! `required`. Enum lists come from the model enums (`NAMES`), so schema and
//! parser cannot disagree.
//!
//! - Version 1 (`json_schema_v1`): the original format. `schema_version` is
//!   optional and, when present, is 1.
//! - Version 2 (`json_schema_v2`): adds a required `schema_version: 2`,
//!   `rubato` (song and section), a section `key`, choir lines (`sing:
//!   "choir"`, `voicing`) and melismas (`~N` in `syl`, described in the
//!   `syl` description). Every version-1 song is a valid version-2 song
//!   once `schema_version` is set to 2.
//! - Version 3 (`json_schema_v3`): adds solfege tunes: `tune` on lines and
//!   sections and a song `tunes` list (see `song::tune`). Every version-2
//!   song is a valid version-3 song once `schema_version` is set to 3.
//! - `json_schema()` is the latest, what `songwriter` asks Claude for.
//!
//! `wire::normalize` reads every version listed here; a newer
//! `schema_version` is `SongError::UnsupportedSchema`.

use crate::model::{
    Blend, ChoirVoicing, Delivery, DrumKit, Endings, Energy, GuitarPattern, Meter, Mode, Rubato,
    SectionKind, SingerId, Voice, SCHEMA_V1, SCHEMA_V2, SCHEMA_V3,
};
use serde_json::{json, Value};

/// `SingerId::NAMES` ("A", "B") plus "both", the enum list of `sing` in
/// version 1; version 2 adds "choir".
fn sing_names(version: u32) -> Vec<&'static str> {
    let mut v = SingerId::NAMES.to_vec();
    v.push("both");
    if version >= SCHEMA_V2 {
        v.push("choir");
    }
    v
}

const SYL_V1: &str =
    "Lyric text: words separated by spaces, syllables by hyphens, * before a stressed syllable.";
const SYL_V2: &str = "Lyric text: words separated by spaces, syllables by hyphens, * before a stressed syllable. A syllable followed by ~ or ~N (2 to 4) is sung over that many notes (a melisma), e.g. *glo~3-ry.";

const SECTION_TUNE: &str = "Sung section: the name of an entry of the song's tunes; line i of the section sings tune line i (wrapping), and a line's own tune wins. Instrumental section (intro, interlude, outro, break): the tune the lead instrument plays, or the name of an entry of the song's tunes (its lines are joined). Solfege tokens as for a line, each followed by a length: 8 eighth, 16 sixteenth, 4 quarter, 2 half, 1 whole, a trailing . dots it (d4.); z8 is an eighth rest; | is an optional bar line, and every bar must fill the meter. A quarter is one beat in 4/4 and 3/4, and 2/3 of a dotted-quarter beat in 6/8. A tune shorter than the section repeats from its start to fill it; a longer one is cut.";

const TUNE_LINE: &str = "The line's melody in movable-do solfege, one token per sung note (melisma notes count), separated by spaces. Tokens: d r m f s l t, raised di ri fi si li, lowered ra me se le te; s, is an octave down, d' an octave up; . leaves the note free; - holds the note before it one more beat. do is the tonic of the section's key.";

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

fn line_schema(version: u32) -> Value {
    let mut v = json!({
        "type": "object",
        "properties": {
            "syl": {"type": "string", "description": if version >= SCHEMA_V2 { SYL_V2 } else { SYL_V1 }},
            "ph": {"type": "string"},
            "chords": {"type": "array", "items": {"type": "string"}},
            "sing": {"type": "string", "enum": sing_names(version)},
            "lead": {"type": "string", "enum": SingerId::NAMES},
            "blend": {"type": "string", "enum": Blend::NAMES}
        },
        "required": ["syl", "ph", "chords"],
        "additionalProperties": false
    });
    if version >= SCHEMA_V2 {
        v["properties"]["voicing"] = json!({"type": "string", "enum": ChoirVoicing::NAMES});
    }
    if version >= SCHEMA_V3 {
        v["properties"]["tune"] = json!({"type": "string", "description": TUNE_LINE});
    }
    v
}

fn section_schema(version: u32) -> Value {
    let mut v = json!({
        "type": "object",
        "properties": {
            "type": {"type": "string", "enum": SectionKind::NAMES},
            "same": {"type": "boolean"},
            "chords": {"type": "array", "items": {"type": "string"}},
            "lines": {"type": "array", "items": line_schema(version)},
            "sing": {"type": "string", "enum": sing_names(version)},
            "lead": {"type": "string", "enum": SingerId::NAMES},
            "blend": {"type": "string", "enum": Blend::NAMES}
        },
        "required": ["type"],
        "additionalProperties": false
    });
    if version >= SCHEMA_V2 {
        let p = &mut v["properties"];
        p["voicing"] = json!({"type": "string", "enum": ChoirVoicing::NAMES});
        p["key"] = json!({"type": "string", "description": "The key from this section on, e.g. \"E\" or \"A minor\"; the chords of the section are written in it. With same: true the copied chords move to it."});
        p["rubato"] = json!({"type": "string", "enum": Rubato::NAMES});
    }
    if version >= SCHEMA_V3 {
        v["properties"]["tune"] = json!({"type": "string", "description": SECTION_TUNE});
        v["properties"]["energy"] = json!({"type": "string", "enum": Energy::NAMES, "description": "How hard the band plays this section. Absent: the engine builds up like a ballad (quiet intro, low first verse, choruses lifted)."});
    }
    v
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

/// The schema of one version, `None` for a version this build does not know.
pub fn json_schema_for(version: u32) -> Option<Value> {
    match version {
        SCHEMA_V1 => Some(json_schema_v1()),
        SCHEMA_V2 => Some(json_schema_v2()),
        SCHEMA_V3 => Some(json_schema_v3()),
        _ => None,
    }
}

/// The newest song reply schema (`SCHEMA_LATEST`).
pub fn json_schema() -> Value {
    json_schema_v3()
}

/// Version 3: version 2 plus `tunes` (a list of `{name, lines}`; the parser
/// also reads an object of name to lines) and the `tune` fields of sections
/// and lines.
pub fn json_schema_v3() -> Value {
    let mut v = build(SCHEMA_V3);
    v["properties"]["schema_version"] = json!({"type": "integer", "enum": [SCHEMA_V3]});
    v["properties"]["rubato"] = json!({"type": "string", "enum": Rubato::NAMES});
    v["properties"]["tunes"] = json!({
        "type": "array",
        "description": "Named tunes: for verses that share a melody, each has tune lines in the notation of a line's tune; for instrumental sections, a break tune (solfege with lengths; the lines are joined). A section names one with its tune field.",
        "items": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "lines": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["name", "lines"],
            "additionalProperties": false
        }
    });
    let req = v["required"].as_array_mut().expect("required is an array");
    req.insert(0, json!("schema_version"));
    v
}

/// Version 2: version 1 plus `schema_version` (required), `rubato`, and
/// the section and line additions.
pub fn json_schema_v2() -> Value {
    let mut v = build(SCHEMA_V2);
    v["properties"]["schema_version"] = json!({"type": "integer", "enum": [SCHEMA_V2]});
    v["properties"]["rubato"] = json!({"type": "string", "enum": Rubato::NAMES});
    let req = v["required"].as_array_mut().expect("required is an array");
    req.insert(0, json!("schema_version"));
    v
}

/// Version 1: the original format; `schema_version` optional, 1 when given.
pub fn json_schema_v1() -> Value {
    let mut v = build(SCHEMA_V1);
    v["properties"]["schema_version"] = json!({"type": "integer", "enum": [SCHEMA_V1]});
    v
}

fn build(version: u32) -> Value {
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
            "sections": {"type": "array", "items": section_schema(version)}
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
        assert_eq!(names(&line["sing"]), sing_names(SCHEMA_V2));
        assert_eq!(names(&line["lead"]), SingerId::NAMES);
        assert_eq!(names(&line["blend"]), Blend::NAMES);
        let sec = &p["sections"]["items"]["properties"];
        assert_eq!(names(&sec["sing"]), sing_names(SCHEMA_V2));
        assert_eq!(names(&sec["lead"]), SingerId::NAMES);
        assert_eq!(names(&sec["blend"]), Blend::NAMES);
        assert!(sing_names(SCHEMA_V2).contains(&"both"));
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

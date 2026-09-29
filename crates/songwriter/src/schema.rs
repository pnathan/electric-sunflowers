//! JSON Schema of the song reply, usable as `--json-schema` on the CLI and
//! as `output_config.format.schema` on the API. It is `song::schema`'s, so
//! the schema and the parser share their enum lists.

use serde_json::Value;

/// The full song object schema.
pub fn song_schema() -> Value {
    song::schema::json_schema()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_well_formed_and_closed() {
        let s = song_schema();
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["band"]["additionalProperties"], false);
        assert_eq!(
            s["properties"]["sections"]["items"]["additionalProperties"],
            false
        );
        assert!(s["required"]
            .as_array()
            .is_some_and(|r| r.iter().any(|v| v == "sections")));
    }
}

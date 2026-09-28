//! Songwriter: styles and forms, the songwriter prompt, the reply schema,
//! and access to Claude.
//!
//! `write_song` picks a style direction and a register, renders the
//! prompt, calls Claude with the reply schema, and extracts the song JSON.
//! Validation is the caller's (`song::normalize_value`, then
//! `Style::apply`).

pub mod claude;
pub mod prompt;
pub mod schema;
pub mod styles;

use sfcore::random::{tag, Tag};
use song::Voice;

pub use sfcore::random::Rng;

use claude::{Claude, ClaudeError, Effort, Request};
use styles::{Direction, StyleId};

/// Stream tag for the songwriter's creative draws; callers seed
/// `Rng::stream(seed, WRITE_TAG)`.
pub const WRITE_TAG: Tag = tag("songwriter.write");

/// What to write.
#[derive(Clone, Debug)]
pub struct WriteRequest<'a> {
    /// The user's mood or prompt, quoted into the prompt verbatim.
    pub mood: &'a str,
    /// The singer, when the user chose one.
    pub voice: Option<Voice>,
    /// The style; `None` picks one at random (songwriter's choice).
    pub style: Option<StyleId>,
    /// The current year; sets the persona's age.
    pub year: i32,
    /// Model id; `None` is `claude::DEFAULT_MODEL`.
    pub model: Option<String>,
    pub effort: Effort,
}

impl<'a> WriteRequest<'a> {
    pub fn new(mood: &'a str, year: i32) -> Self {
        WriteRequest { mood, voice: None, style: None, year, model: None, effort: Effort::default() }
    }
}

/// A written song, not yet validated.
#[derive(Clone, Debug)]
pub struct Written {
    /// The song JSON as the model wrote it.
    pub raw: serde_json::Value,
    /// The direction the prompt gave.
    pub direction: Direction,
    /// The register the prompt gave for an open feeling.
    pub register: &'static str,
    /// The model that answered, when the transport reports it.
    pub model: Option<String>,
}

/// Writes one song. Draws from `rng`: the style direction (see
/// `styles::style_direction`), then the register. Extracts the first JSON
/// object from the reply, tolerating a code fence or prose around it.
pub fn write_song(claude: &dyn Claude, req: &WriteRequest, rng: &mut Rng) -> Result<Written, WriteSongError> {
    let direction = styles::style_direction(req.style, rng);
    let register = prompt::pick_register(rng);
    let text = prompt::song_prompt(req.mood, req.voice, &direction, register, req.year);

    let mut creq = Request::new(text);
    if let Some(m) = &req.model {
        creq.model = m.clone();
    }
    creq.effort = req.effort;
    creq.json_schema = Some(schema::song_schema());

    let reply = claude.complete(&creq).map_err(WriteSongError::Claude)?;
    let json = extract_json_object(&reply.text).ok_or_else(|| WriteSongError::NoJsonFound(reply.text.clone()))?;
    let raw = serde_json::from_str(json).map_err(|e| WriteSongError::InvalidJson(e.to_string()))?;
    Ok(Written { raw, direction, register, model: reply.model })
}

#[derive(Debug)]
pub enum WriteSongError {
    Claude(ClaudeError),
    NoJsonFound(String),
    InvalidJson(String),
}

impl std::fmt::Display for WriteSongError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteSongError::Claude(e) => write!(f, "claude call failed: {e}"),
            WriteSongError::NoJsonFound(t) => write!(f, "no JSON object found in reply: {t}"),
            WriteSongError::InvalidJson(e) => write!(f, "invalid JSON: {e}"),
        }
    }
}

impl std::error::Error for WriteSongError {}

/// The first balanced `{...}` object in `text`. Braces inside string
/// literals (with backslash escapes) are skipped, so a `}` in a lyric or
/// title cannot end the scan.
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0u32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_object() {
        let t = r#"{"a":1}"#;
        assert_eq!(extract_json_object(t), Some(t));
    }

    #[test]
    fn extracts_from_code_fence_and_prose() {
        let t = "Sure:\n```json\n{\"a\": {\"b\": 1}, \"c\": \"}\\\"\"}\n```\nEnjoy!";
        let v: serde_json::Value = serde_json::from_str(extract_json_object(t).unwrap()).unwrap();
        assert_eq!(v["a"]["b"], 1);
        assert_eq!(v["c"], "}\"");
    }

    #[test]
    fn returns_none_when_no_object() {
        assert!(extract_json_object("no json here").is_none());
        assert!(extract_json_object("{\"a\": {").is_none());
    }
}

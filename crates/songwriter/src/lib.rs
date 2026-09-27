//! Songwriter: styles, forms, the songwriter prompt, and access to Claude.

pub mod claude;
pub mod prompt;
pub mod schema;
pub mod styles;

use claude::{Claude, ClaudeError, Request};
use styles::Direction;

/// Model/effort options for `write_song`. `model` defaults to
/// `claude::DEFAULT_MODEL` when `None`; `effort` is passed through to the
/// request only when given.
#[derive(Debug, Default, Clone)]
pub struct WriteSongOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// Builds the songwriter prompt, calls `claude`, and extracts the song JSON object
/// from the reply text (tolerating a code fence or stray prose around it), returning
/// it alongside the `Direction` (style/form/mode/meter/tempo choice) used to build the
/// prompt so the caller can drive `applyStyle`-equivalent normalization afterward.
///
/// `style` names a STYLES key to fix the direction instead of a random pick; `None`
/// lets `style_direction` choose (the songwriter's "choice picks a style at random").
///
/// `opts` overrides the request's model and/or effort; leave default for the
/// crate's usual `claude::DEFAULT_MODEL` and no effort setting.
pub fn write_song(
    claude: &dyn Claude,
    mood: &str,
    voice_pref: Option<&str>,
    style: Option<&str>,
    year: i32,
    rand: &mut dyn FnMut() -> f64,
    opts: WriteSongOptions,
) -> Result<(serde_json::Value, Direction), WriteSongError> {
    let dir = styles::style_direction(style, rand);
    let prompt_text = prompt::song_prompt(mood, voice_pref, Some(dir.clone()), year, rand);

    let mut req = Request::new(prompt_text);
    req.model = opts.model.unwrap_or_else(|| claude::DEFAULT_MODEL.to_string());
    req.effort = opts.effort;
    req.json_schema = Some(schema::song_schema());

    let reply = claude.complete(&req).map_err(WriteSongError::Claude)?;
    let json = extract_json_object(&reply.text)
        .ok_or_else(|| WriteSongError::NoJsonFound(reply.text.clone()))?;
    let value: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| WriteSongError::InvalidJson(e.to_string()))?;

    Ok((value, dir))
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

/// Extracts the first balanced `{...}` object from `text`, tolerating a markdown code
/// fence (```json ... ```) or stray prose before/after it. Balances braces while
/// ignoring braces inside string literals, so a chord string like `"F#m"` or a title
/// containing `}` in prose can't break the scan.
fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
        } else {
            match c {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(text[start..=i].to_string());
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_plain_object() {
        let t = r#"{"a":1}"#;
        assert_eq!(extract_json_object(t).unwrap(), t);
    }

    #[test]
    fn extracts_from_code_fence_and_prose() {
        let t = "Sure, here you go:\n```json\n{\"a\": {\"b\": 1}, \"c\": \"}\"}\n```\nEnjoy!";
        let extracted = extract_json_object(t).unwrap();
        let v: serde_json::Value = serde_json::from_str(&extracted).unwrap();
        assert_eq!(v["a"]["b"], 1);
        assert_eq!(v["c"], "}");
    }

    #[test]
    fn returns_none_when_no_object() {
        assert!(extract_json_object("no json here").is_none());
    }
}

//! Access to Claude: a trait plus a CLI-shelling implementation and an HTTP implementation
//! against the Anthropic Messages API.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Default model id. The owner's CLAUDE.md names `claude-opus-5-5` as the deploy default;
/// `claude-fable-5-1` is the top Mythos tier, for harder writing tasks.
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const TOP_MODEL: &str = "claude-fable-5-1";

/// One completion request. Fields are optional except `prompt` and `model`.
#[derive(Clone, Debug, Default)]
pub struct Request {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub json_schema: Option<serde_json::Value>,
    /// "low" | "medium" | "high" | "xhigh" | "max"
    pub effort: Option<String>,
}

impl Request {
    pub fn new(prompt: impl Into<String>) -> Self {
        Request { prompt: prompt.into(), model: DEFAULT_MODEL.to_string(), ..Default::default() }
    }
}

/// A completion result.
#[derive(Clone, Debug)]
pub struct Reply {
    pub text: String,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
}

/// Failure modes, kept separate so callers can react differently (retry a transport
/// failure, surface a refusal to the user, fail hard on a parse error).
#[derive(Debug)]
pub enum ClaudeError {
    /// Could not spawn the CLI process, or a transport-level failure talking to it.
    Spawn(std::io::Error),
    /// The CLI exited non-zero, or the HTTP call returned a non-2xx status.
    /// `body` is the raw stdout/stderr or HTTP response body, for diagnosis.
    Status { code: i32, body: String },
    /// The model declined to answer (`stop_reason: "refusal"`).
    Refusal { category: Option<String>, explanation: Option<String> },
    /// The reply could not be parsed as expected (bad JSON envelope, missing field, ...).
    Parse(String),
}

impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeError::Spawn(e) => write!(f, "spawn/transport failure: {e}"),
            ClaudeError::Status { code, body } => write!(f, "non-zero status {code}: {body}"),
            ClaudeError::Refusal { category, explanation } => write!(
                f,
                "refusal (category={:?}): {}",
                category,
                explanation.as_deref().unwrap_or("")
            ),
            ClaudeError::Parse(s) => write!(f, "parse failure: {s}"),
        }
    }
}

impl std::error::Error for ClaudeError {}

pub trait Claude {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError>;
}

/// Shells out to the `claude` CLI in print mode. Runs with its working directory set to a
/// fresh empty temp directory so no project CLAUDE.md is picked up; the user's own
/// `~/.claude/CLAUDE.md` still reaches the model in this mode (verified live: `--setting-sources
/// project,local` and `--safe-mode` do not stop it -- see the crate's parity notes / task report).
/// Never pass `--bare`: it disables the logged-in OAuth account, which is the reason to use
/// this path over the API implementation.
pub struct ClaudeCli {
    /// Path to the `claude` binary; default "claude" (resolved via PATH).
    pub bin: String,
}

impl Default for ClaudeCli {
    fn default() -> Self {
        ClaudeCli { bin: "claude".to_string() }
    }
}

impl ClaudeCli {
    pub fn new(bin: impl Into<String>) -> Self {
        ClaudeCli { bin: bin.into() }
    }
}

impl Claude for ClaudeCli {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
        let tmp = tempdir().map_err(ClaudeError::Spawn)?;

        // Keep the songwriter's context clean: no CLAUDE.md files, no auto-memory, no MCP
        // server instructions. Measured 2026-09-26 (CLI 2.1.283): with these, the context
        // holds only the SDK preamble, environment, model, account email and date.
        let mut cmd = Command::new(&self.bin);
        cmd.current_dir(&tmp)
            .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1")
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
            .arg("--strict-mcp-config")
            .arg("-p")
            .arg("--output-format")
            .arg("json")
            .arg("--model")
            .arg(&req.model)
            .arg("--tools")
            .arg("")
            .arg("--no-session-persistence");
        if let Some(sp) = &req.system_prompt {
            cmd.arg("--system-prompt").arg(sp);
        }
        if let Some(schema) = &req.json_schema {
            cmd.arg("--json-schema").arg(schema.to_string());
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(ClaudeError::Spawn)?;
        {
            let mut stdin = child.stdin.take().expect("piped stdin");
            stdin.write_all(req.prompt.as_bytes()).map_err(ClaudeError::Spawn)?;
        }
        let output = child.wait_with_output().map_err(ClaudeError::Spawn)?;
        let _ = std::fs::remove_dir_all(&tmp);

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ClaudeError::Status {
                code: output.status.code().unwrap_or(-1),
                body: if stdout.is_empty() { stderr } else { stdout },
            });
        }

        parse_cli_envelope(&stdout)
    }
}

/// Parses the CLI's `-p --output-format json` envelope: `{"is_error":bool,"result":"...",
/// "subtype":"...","stop_reason":..., ...}` (fields observed live; only `is_error` and
/// `result` are relied on here beyond diagnostics).
fn parse_cli_envelope(stdout: &str) -> Result<Reply, ClaudeError> {
    let v: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| ClaudeError::Parse(format!("{e}: {stdout}")))?;

    let is_error = v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(false);
    let subtype = v.get("subtype").and_then(|x| x.as_str()).map(|s| s.to_string());

    if is_error {
        // The CLI surfaces a refusal as an error result with a descriptive subtype/result
        // text rather than a structured stop_reason; treat "refusal"-flavored subtypes as
        // ClaudeError::Refusal, everything else as a status failure.
        let result_text = v.get("result").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if subtype.as_deref() == Some("refusal") {
            return Err(ClaudeError::Refusal { category: None, explanation: Some(result_text) });
        }
        return Err(ClaudeError::Status { code: -1, body: stdout.to_string() });
    }

    let text = v
        .get("result")
        .and_then(|x| x.as_str())
        .ok_or_else(|| ClaudeError::Parse(format!("no 'result' field: {stdout}")))?
        .to_string();

    Ok(Reply { text, model: None, stop_reason: subtype })
}

fn tempdir() -> std::io::Result<std::path::PathBuf> {
    let mut p = std::env::temp_dir();
    let unique = format!(
        "sfsongwriter-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    p.push(unique);
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

/// Calls the Anthropic Messages API over blocking HTTP (`ureq`).
pub struct ClaudeApi {
    pub api_key: String,
    pub base_url: String,
}

impl ClaudeApi {
    pub fn new(api_key: impl Into<String>) -> Self {
        ClaudeApi { api_key: api_key.into(), base_url: "https://api.anthropic.com".to_string() }
    }

    /// Reads the key from `ANTHROPIC_API_KEY`.
    pub fn from_env() -> Result<Self, ClaudeError> {
        let key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| ClaudeError::Parse("ANTHROPIC_API_KEY not set".to_string()))?;
        Ok(Self::new(key))
    }
}

impl Claude for ClaudeApi {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(16000),
            "messages": [{"role": "user", "content": req.prompt}],
        });
        if let Some(sp) = &req.system_prompt {
            body["system"] = serde_json::Value::String(sp.clone());
        }
        let mut output_config = serde_json::Map::new();
        if let Some(schema) = &req.json_schema {
            output_config.insert(
                "format".to_string(),
                serde_json::json!({"type": "json_schema", "schema": schema}),
            );
        }
        if let Some(effort) = &req.effort {
            output_config.insert("effort".to_string(), serde_json::Value::String(effort.clone()));
        }
        if !output_config.is_empty() {
            body["output_config"] = serde_json::Value::Object(output_config);
        }

        let url = format!("{}/v1/messages", self.base_url);
        let result = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .timeout(Duration::from_secs(600))
            .send_json(body);

        match result {
            Ok(resp) => {
                let v: serde_json::Value = resp
                    .into_json()
                    .map_err(|e| ClaudeError::Parse(format!("bad JSON response: {e}")))?;
                parse_api_reply(&v)
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                Err(ClaudeError::Status { code: code as i32, body })
            }
            Err(ureq::Error::Transport(t)) => Err(ClaudeError::Spawn(std::io::Error::new(
                std::io::ErrorKind::Other,
                t.to_string(),
            ))),
        }
    }
}

/// Parses a Messages API response body: concatenates `text` blocks, skips `thinking`
/// blocks, and maps `stop_reason: "refusal"` (with `stop_details`) to `ClaudeError::Refusal`.
fn parse_api_reply(v: &serde_json::Value) -> Result<Reply, ClaudeError> {
    let stop_reason = v.get("stop_reason").and_then(|x| x.as_str()).map(|s| s.to_string());

    if stop_reason.as_deref() == Some("refusal") {
        let details = v.get("stop_details");
        let category = details
            .and_then(|d| d.get("category"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        let explanation = details
            .and_then(|d| d.get("explanation"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        return Err(ClaudeError::Refusal { category, explanation });
    }

    let content = v
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| ClaudeError::Parse(format!("no 'content' array: {v}")))?;

    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                text.push_str(t);
            }
        }
    }

    let model = v.get("model").and_then(|m| m.as_str()).map(|s| s.to_string());
    Ok(Reply { text, model, stop_reason })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_reply_and_skips_thinking() {
        let v = serde_json::json!({
            "content": [
                {"type": "thinking", "thinking": "internal reasoning"},
                {"type": "text", "text": "hello "},
                {"type": "text", "text": "world"}
            ],
            "stop_reason": "end_turn",
            "model": "claude-opus-5-5",
        });
        let reply = parse_api_reply(&v).unwrap();
        assert_eq!(reply.text, "hello world");
        assert_eq!(reply.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(reply.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn parses_refusal() {
        let v = serde_json::json!({
            "content": [],
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "cyber", "explanation": "no"},
        });
        match parse_api_reply(&v) {
            Err(ClaudeError::Refusal { category, explanation }) => {
                assert_eq!(category.as_deref(), Some("cyber"));
                assert_eq!(explanation.as_deref(), Some("no"));
            }
            other => panic!("expected Refusal, got {other:?}"),
        }
    }

    #[test]
    fn request_construction_carries_schema_and_effort() {
        let mut req = Request::new("write a song");
        req.json_schema = Some(serde_json::json!({"type": "object"}));
        req.effort = Some("low".to_string());
        req.max_tokens = Some(1234);

        // Build the body the same way ClaudeApi::complete does, without a live call.
        let mut body = serde_json::json!({
            "model": req.model,
            "max_tokens": req.max_tokens.unwrap_or(16000),
            "messages": [{"role": "user", "content": req.prompt}],
        });
        let mut output_config = serde_json::Map::new();
        if let Some(schema) = &req.json_schema {
            output_config
                .insert("format".to_string(), serde_json::json!({"type": "json_schema", "schema": schema}));
        }
        if let Some(effort) = &req.effort {
            output_config.insert("effort".to_string(), serde_json::Value::String(effort.clone()));
        }
        body["output_config"] = serde_json::Value::Object(output_config);

        assert_eq!(body["max_tokens"], 1234);
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }

    #[test]
    fn parses_cli_envelope_success() {
        let stdout = r#"{"is_error":false,"result":"PONG","subtype":"success"}"#;
        let reply = parse_cli_envelope(stdout).unwrap();
        assert_eq!(reply.text, "PONG");
    }

    #[test]
    fn parses_cli_envelope_error() {
        let stdout = r#"{"is_error":true,"result":"boom","subtype":"error_during_execution"}"#;
        match parse_cli_envelope(stdout) {
            Err(ClaudeError::Status { .. }) => {}
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[ignore]
    #[test]
    fn live_api_call() {
        let api = ClaudeApi::from_env().expect("ANTHROPIC_API_KEY not set");
        let mut req = Request::new("Say the word PONG and nothing else.");
        req.model = "claude-haiku-4-5".to_string();
        req.max_tokens = Some(64);
        let reply = api.complete(&req).unwrap();
        assert!(reply.text.contains("PONG"));
    }
}

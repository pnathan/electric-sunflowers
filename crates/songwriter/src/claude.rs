//! Access to Claude: the `Claude` trait, a client that shells out to the
//! `claude` CLI in print mode, and a client for the Anthropic Messages API
//! over blocking HTTP (`ureq`).
//!
//! The API client builds its body with `request_body`, retries 429, 5xx
//! and transport failures with exponential backoff (`Backoff`), and maps
//! `stop_reason` "refusal" and "max_tokens" to their own errors.

use std::fmt;
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::usage::{ModelUsage, Usage};

/// How a `Reply` was obtained: the `claude` CLI in print mode, or the
/// Messages API directly. Recorded in `usage::Generation` and in
/// `settings::ClaudeSettings`; serialises as the lower-case name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    #[default]
    Cli,
    Api,
}

impl Transport {
    pub const ALL: &'static [Transport] = &[Transport::Cli, Transport::Api];

    pub const fn as_str(self) -> &'static str {
        match self {
            Transport::Cli => "cli",
            Transport::Api => "api",
        }
    }
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Transport {
    type Err = ClaudeError;
    fn from_str(s: &str) -> Result<Self, ClaudeError> {
        let t = s.trim();
        Transport::ALL
            .iter()
            .copied()
            .find(|v| t.eq_ignore_ascii_case(v.as_str()))
            .ok_or_else(|| ClaudeError::Config(format!("unknown transport {s:?} (want cli, api)")))
    }
}

/// Default model (CLAUDE.md: name the model explicitly).
pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
/// The top tier, for harder writing tasks.
pub const TOP_MODEL: &str = "claude-fable-5-1";
/// Default output cap. It covers thinking and the song JSON (a few
/// thousand tokens); a cut-off reply is `ClaudeError::MaxTokens`.
pub const DEFAULT_MAX_TOKENS: u32 = 32_000;
/// HTTP timeout for one API call. A non-streaming call returns only when
/// the whole reply is done, so this must cover `DEFAULT_MAX_TOKENS` of output.
pub const API_TIMEOUT: Duration = Duration::from_secs(900);
/// Messages API version header.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Thinking depth and token spend (`output_config.effort`, CLI `--effort`).
/// Always sent: the default differs by model (Opus 5.5 defaults to
/// medium), so the request states it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Low,
    Medium,
    /// Default: song quality is the priority.
    #[default]
    High,
    XHigh,
    Max,
}

impl Effort {
    pub const ALL: &'static [Effort] = &[Effort::Low, Effort::Medium, Effort::High, Effort::XHigh, Effort::Max];

    pub const fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

impl fmt::Display for Effort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Effort {
    type Err = ClaudeError;
    fn from_str(s: &str) -> Result<Self, ClaudeError> {
        let t = s.trim();
        Effort::ALL
            .iter()
            .copied()
            .find(|e| t.eq_ignore_ascii_case(e.as_str()))
            .ok_or_else(|| ClaudeError::Config(format!("unknown effort {s:?} (want low, medium, high, xhigh, max)")))
    }
}

/// One completion request.
#[derive(Clone, Debug)]
pub struct Request {
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model: String,
    pub max_tokens: u32,
    /// Reply schema (`output_config.format`, CLI `--json-schema`).
    pub json_schema: Option<Value>,
    pub effort: Effort,
}

impl Request {
    pub fn new(prompt: impl Into<String>) -> Self {
        Request {
            prompt: prompt.into(),
            system_prompt: None,
            model: DEFAULT_MODEL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
            json_schema: None,
            effort: Effort::default(),
        }
    }
}

/// A completion result.
#[derive(Clone, Debug)]
pub struct Reply {
    pub text: String,
    /// The model that answered, when the transport reports it.
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    /// Total token usage for the call.
    pub usage: Usage,
    /// CLI: every `modelUsage` entry, sorted by model id. API: one entry
    /// for `model`, when known.
    pub per_model: Vec<ModelUsage>,
    /// CLI `total_cost_usd`. The API reports no cost.
    pub cost_usd: Option<f64>,
    /// CLI `duration_ms`.
    pub duration_ms: Option<u64>,
}

impl Reply {
    /// A reply with only `text` set; everything else absent. For tests
    /// and mocks that do not model usage.
    pub fn text_only(text: impl Into<String>) -> Reply {
        Reply {
            text: text.into(),
            model: None,
            stop_reason: None,
            usage: Usage::default(),
            per_model: Vec::new(),
            cost_usd: None,
            duration_ms: None,
        }
    }
}

/// Failure modes, kept apart so callers can react differently.
#[derive(Debug)]
pub enum ClaudeError {
    /// Bad configuration: a missing API key, an unknown effort name.
    Config(String),
    /// Could not run the CLI process or talk to it.
    Spawn(std::io::Error),
    /// HTTP transport failure (DNS, connect, TLS, timeout). Retried.
    Transport(String),
    /// Non-2xx HTTP status, or a failed CLI run. `body` is the raw
    /// response or output; `retry_after` is the server's hint, if any.
    /// 429 and 5xx are retried.
    Status {
        code: i32,
        body: String,
        retry_after: Option<Duration>,
    },
    /// The model declined (`stop_reason: "refusal"`).
    Refusal {
        category: Option<String>,
        explanation: Option<String>,
    },
    /// The reply hit the output cap (`stop_reason: "max_tokens"`); the
    /// text so far is kept for diagnosis.
    MaxTokens { partial: String },
    /// The reply envelope could not be parsed.
    Parse(String),
}

impl fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaudeError::Config(s) => write!(f, "configuration: {s}"),
            ClaudeError::Spawn(e) => write!(f, "could not run the claude CLI: {e}"),
            ClaudeError::Transport(s) => write!(f, "transport failure: {s}"),
            ClaudeError::Status { code, body, .. } => write!(f, "status {code}: {body}"),
            ClaudeError::Refusal { category, explanation } => write!(
                f,
                "refusal (category {}): {}",
                category.as_deref().unwrap_or("none"),
                explanation.as_deref().unwrap_or("")
            ),
            ClaudeError::MaxTokens { partial } => {
                write!(
                    f,
                    "reply cut off at the output cap after {} characters",
                    partial.chars().count()
                )
            }
            ClaudeError::Parse(s) => write!(f, "parse failure: {s}"),
        }
    }
}

impl std::error::Error for ClaudeError {}

impl ClaudeError {
    /// Whether a retry can help: 429, 5xx, or a transport failure.
    pub fn is_retryable(&self) -> bool {
        match self {
            ClaudeError::Status { code, .. } => *code == 429 || (500..600).contains(code),
            ClaudeError::Transport(_) => true,
            _ => false,
        }
    }
}

pub trait Claude {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError>;

    /// Transport used for `usage::Generation` records. Defaults to `Cli`
    /// so mock implementations elsewhere (studio, tests) still compile
    /// without naming it.
    fn transport(&self) -> Transport {
        Transport::Cli
    }
}

/// Retry schedule: after failed attempt k (0-based) wait `first * 2^k`,
/// or the server's `retry-after` when longer (capped at `max_hint`), for
/// at most `retries` retries. Default: 3 retries after 1 s, 2 s, 4 s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub retries: u32,
    pub first: Duration,
    pub max_hint: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff {
            retries: 3,
            first: Duration::from_secs(1),
            max_hint: Duration::from_secs(60),
        }
    }
}

impl Backoff {
    /// Wait before retry `k` (0-based) after `err`.
    pub fn delay(&self, k: u32, err: &ClaudeError) -> Duration {
        let base = self.first.saturating_mul(1u32 << k.min(16));
        let hint = match err {
            ClaudeError::Status {
                retry_after: Some(d), ..
            } => (*d).min(self.max_hint),
            _ => Duration::ZERO,
        };
        base.max(hint)
    }

    /// Runs `call`, retrying retryable errors on this schedule; `sleep`
    /// performs each wait (a parameter so tests need not wait).
    pub fn run<T>(
        &self,
        sleep: &mut dyn FnMut(Duration),
        mut call: impl FnMut() -> Result<T, ClaudeError>,
    ) -> Result<T, ClaudeError> {
        let mut k = 0;
        loop {
            match call() {
                Err(e) if e.is_retryable() && k < self.retries => {
                    sleep(self.delay(k, &e));
                    k += 1;
                }
                r => return r,
            }
        }
    }
}

/// Shells out to the `claude` CLI in print mode, in a fresh empty temp
/// directory with CLAUDE.md files, auto-memory and MCP servers off, so the
/// songwriter's context holds only the CLI preamble and the prompt. The
/// user's own `~/.claude/CLAUDE.md` still reaches the model in this mode.
/// Never pass `--bare`: it disables the logged-in account, which is the
/// reason to use this client. The CLI does its own retries; this client
/// does not retry.
pub struct ClaudeCli {
    /// Path to the `claude` binary; default "claude" (resolved via PATH).
    pub bin: String,
}

impl Default for ClaudeCli {
    fn default() -> Self {
        ClaudeCli {
            bin: "claude".to_string(),
        }
    }
}

impl ClaudeCli {
    pub fn new(bin: impl Into<String>) -> Self {
        ClaudeCli { bin: bin.into() }
    }

    /// The CLI arguments for `req` (the prompt goes to stdin).
    fn args(req: &Request) -> Vec<String> {
        let mut a: Vec<String> = [
            "--strict-mcp-config",
            "-p",
            "--output-format",
            "json",
            "--model",
            &req.model,
            "--effort",
            req.effort.as_str(),
            "--tools",
            "",
            "--no-session-persistence",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        if let Some(sp) = &req.system_prompt {
            a.push("--system-prompt".into());
            a.push(sp.clone());
        }
        if let Some(schema) = &req.json_schema {
            a.push("--json-schema".into());
            a.push(schema.to_string());
        }
        a
    }
}

impl Claude for ClaudeCli {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
        let tmp = tempfile::Builder::new()
            .prefix("sfsongwriter-")
            .tempdir()
            .map_err(ClaudeError::Spawn)?;
        let mut child = Command::new(&self.bin)
            .current_dir(tmp.path())
            .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1")
            .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
            .args(Self::args(req))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ClaudeError::Spawn)?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| ClaudeError::Spawn(std::io::Error::other("no stdin pipe to the CLI")))?;
            stdin.write_all(req.prompt.as_bytes()).map_err(ClaudeError::Spawn)?;
        }
        let output = child.wait_with_output().map_err(ClaudeError::Spawn)?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(ClaudeError::Status {
                code: output.status.code().unwrap_or(-1),
                body: if stdout.is_empty() { stderr } else { stdout },
                retry_after: None,
            });
        }
        parse_cli_envelope(&stdout, &req.model)
    }

    fn transport(&self) -> Transport {
        Transport::Cli
    }
}

/// A JSON object's `u64` field, or `None` when absent or not a number.
/// Never fails: a malformed or missing usage field is silently absent.
fn get_u64(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

/// The `modelUsage` key with the most `outputTokens` (treating an absent
/// count as 0); ties go to `requested`, when it is one of the tied
/// entries, else the first tied entry by model id (`per_model` is already
/// sorted by id).
fn pick_answering_model(per_model: &[ModelUsage], requested: &str) -> Option<String> {
    let max = per_model.iter().map(|m| m.usage.output_tokens.unwrap_or(0)).max()?;
    let mut tied = per_model.iter().filter(|m| m.usage.output_tokens.unwrap_or(0) == max);
    let first = tied.next()?;
    if let Some(m) = std::iter::once(first).chain(tied.clone()).find(|m| m.model == requested) {
        return Some(m.model.clone());
    }
    Some(first.model.clone())
}

/// Parses the CLI's `-p --output-format json` envelope (`{"is_error",
/// "result", "subtype", "stop_reason", "usage", "modelUsage",
/// "total_cost_usd", "duration_ms", ...}`). `requested_model` breaks a
/// tie for which model answered (see `pick_answering_model`). Usage
/// fields are all optional: a missing or non-numeric one never fails the
/// parse, it just reads as `None`.
fn parse_cli_envelope(stdout: &str, requested_model: &str) -> Result<Reply, ClaudeError> {
    let v: Value = serde_json::from_str(stdout).map_err(|e| ClaudeError::Parse(format!("{e}: {stdout}")))?;
    let text = v.get("result").and_then(Value::as_str).unwrap_or("").to_string();
    let subtype = v.get("subtype").and_then(Value::as_str);
    let stop_reason = v.get("stop_reason").and_then(Value::as_str);
    if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
        if subtype == Some("refusal") || stop_reason == Some("refusal") {
            return Err(ClaudeError::Refusal {
                category: None,
                explanation: Some(text),
            });
        }
        return Err(ClaudeError::Status {
            code: -1,
            body: stdout.to_string(),
            retry_after: None,
        });
    }
    if stop_reason == Some("max_tokens") {
        return Err(ClaudeError::MaxTokens { partial: text });
    }
    if v.get("result").and_then(Value::as_str).is_none() {
        return Err(ClaudeError::Parse(format!("no 'result' field: {stdout}")));
    }

    let usage_obj = v.get("usage");
    let usage = Usage {
        input_tokens: usage_obj.and_then(|u| get_u64(u, "input_tokens")),
        output_tokens: usage_obj.and_then(|u| get_u64(u, "output_tokens")),
        cache_read_input_tokens: usage_obj.and_then(|u| get_u64(u, "cache_read_input_tokens")),
        cache_creation_input_tokens: usage_obj.and_then(|u| get_u64(u, "cache_creation_input_tokens")),
        thinking_tokens: usage_obj
            .and_then(|u| u.get("output_tokens_details"))
            .and_then(|d| get_u64(d, "thinking_tokens")),
    };

    let mut per_model: Vec<ModelUsage> = Vec::new();
    if let Some(obj) = v.get("modelUsage").and_then(Value::as_object) {
        for (model, mv) in obj {
            let mu = Usage {
                input_tokens: get_u64(mv, "inputTokens"),
                output_tokens: get_u64(mv, "outputTokens"),
                cache_read_input_tokens: get_u64(mv, "cacheReadInputTokens"),
                cache_creation_input_tokens: get_u64(mv, "cacheCreationInputTokens"),
                thinking_tokens: get_u64(mv, "thinkingTokens"),
            };
            per_model.push(ModelUsage {
                model: model.clone(),
                usage: mu,
                cost_usd: mv.get("costUSD").and_then(Value::as_f64),
            });
        }
    }
    per_model.sort_by(|a, b| a.model.cmp(&b.model));
    let model = pick_answering_model(&per_model, requested_model);

    Ok(Reply {
        text,
        model,
        stop_reason: stop_reason.or(subtype).map(str::to_string),
        usage,
        per_model,
        cost_usd: v.get("total_cost_usd").and_then(Value::as_f64),
        duration_ms: get_u64(&v, "duration_ms"),
    })
}

/// Calls the Anthropic Messages API.
pub struct ClaudeApi {
    pub api_key: String,
    pub base_url: String,
    pub backoff: Backoff,
}

impl ClaudeApi {
    pub fn new(api_key: impl Into<String>) -> Self {
        ClaudeApi {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com".to_string(),
            backoff: Backoff::default(),
        }
    }

    /// Reads the key from `ANTHROPIC_API_KEY`; `Config` when unset or empty.
    pub fn from_env() -> Result<Self, ClaudeError> {
        match std::env::var("ANTHROPIC_API_KEY") {
            Ok(k) if !k.trim().is_empty() => Ok(Self::new(k.trim())),
            _ => Err(ClaudeError::Config("ANTHROPIC_API_KEY is not set".to_string())),
        }
    }

    /// One HTTP attempt.
    fn post(&self, body: &Value) -> Result<Reply, ClaudeError> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let result = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .set("content-type", "application/json")
            .timeout(API_TIMEOUT)
            .send_json(body);
        match result {
            Ok(resp) => {
                let v: Value = resp
                    .into_json()
                    .map_err(|e| ClaudeError::Transport(format!("reading the response: {e}")))?;
                parse_api_reply(&v)
            }
            Err(ureq::Error::Status(code, resp)) => {
                let retry_after = resp.header("retry-after").and_then(parse_retry_after);
                let body = resp.into_string().unwrap_or_default();
                Err(ClaudeError::Status {
                    code: i32::from(code),
                    body,
                    retry_after,
                })
            }
            Err(ureq::Error::Transport(t)) => Err(ClaudeError::Transport(t.to_string())),
        }
    }
}

impl Claude for ClaudeApi {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
        let body = request_body(req);
        self.backoff.run(&mut std::thread::sleep, || self.post(&body))
    }

    fn transport(&self) -> Transport {
        Transport::Api
    }
}

/// `retry-after` in whole seconds (the HTTP-date form is ignored).
fn parse_retry_after(h: &str) -> Option<Duration> {
    h.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// The Messages API request body for `req`: model, max_tokens, one user
/// message, the optional system prompt, and `output_config` with the
/// effort always set and the reply schema as `format` when given.
pub fn request_body(req: &Request) -> Value {
    let mut output_config = json!({ "effort": req.effort.as_str() });
    if let Some(schema) = &req.json_schema {
        output_config["format"] = json!({ "type": "json_schema", "schema": schema });
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "messages": [{ "role": "user", "content": req.prompt }],
        "output_config": output_config,
    });
    if let Some(sp) = &req.system_prompt {
        body["system"] = Value::String(sp.clone());
    }
    body
}

/// Parses a Messages API response: concatenates `text` blocks, skips
/// `thinking` blocks, and maps `stop_reason` "refusal" (with
/// `stop_details`) and "max_tokens" to their errors. `usage` carries
/// input, output and cache token counts; the API does not break out
/// thinking tokens or report cost, so those stay `None`.
fn parse_api_reply(v: &Value) -> Result<Reply, ClaudeError> {
    let stop_reason = v.get("stop_reason").and_then(Value::as_str).map(str::to_string);
    if stop_reason.as_deref() == Some("refusal") {
        let field = |k: &str| {
            v.get("stop_details")
                .and_then(|d| d.get(k))
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        return Err(ClaudeError::Refusal {
            category: field("category"),
            explanation: field("explanation"),
        });
    }
    let content = v
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ClaudeError::Parse(format!("no 'content' array: {v}")))?;
    let mut text = String::new();
    for block in content {
        if block.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                text.push_str(t);
            }
        }
    }
    if stop_reason.as_deref() == Some("max_tokens") {
        return Err(ClaudeError::MaxTokens { partial: text });
    }
    let model = v.get("model").and_then(Value::as_str).map(str::to_string);
    let usage_obj = v.get("usage");
    let usage = Usage {
        input_tokens: usage_obj.and_then(|u| get_u64(u, "input_tokens")),
        output_tokens: usage_obj.and_then(|u| get_u64(u, "output_tokens")),
        cache_read_input_tokens: usage_obj.and_then(|u| get_u64(u, "cache_read_input_tokens")),
        cache_creation_input_tokens: usage_obj.and_then(|u| get_u64(u, "cache_creation_input_tokens")),
        thinking_tokens: None,
    };
    let per_model = match &model {
        Some(m) => vec![ModelUsage {
            model: m.clone(),
            usage: usage.clone(),
            cost_usd: None,
        }],
        None => Vec::new(),
    };
    Ok(Reply {
        text,
        model,
        stop_reason,
        usage,
        per_model,
        cost_usd: None,
        duration_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn body_carries_schema_effort_and_system() {
        let mut req = Request::new("write a song");
        req.json_schema = Some(json!({"type": "object"}));
        req.effort = Effort::XHigh;
        req.max_tokens = 1234;
        req.system_prompt = Some("sys".into());
        let b = request_body(&req);
        assert_eq!(b["model"], DEFAULT_MODEL);
        assert_eq!(b["max_tokens"], 1234);
        assert_eq!(b["messages"][0]["role"], "user");
        assert_eq!(b["messages"][0]["content"], "write a song");
        assert_eq!(b["system"], "sys");
        assert_eq!(b["output_config"]["effort"], "xhigh");
        assert_eq!(b["output_config"]["format"]["type"], "json_schema");
        assert_eq!(b["output_config"]["format"]["schema"], json!({"type": "object"}));
    }

    #[test]
    fn body_states_the_default_effort() {
        let b = request_body(&Request::new("x"));
        assert_eq!(b["output_config"], json!({"effort": "high"}));
        assert!(b.get("system").is_none());
        assert_eq!(b["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn parses_text_reply_and_skips_thinking() {
        let v = json!({
            "content": [
                {"type": "thinking", "thinking": ""},
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
    fn parses_refusal_and_max_tokens() {
        let v = json!({
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
        let v = json!({"content": [{"type": "text", "text": "{\"title\":"}], "stop_reason": "max_tokens"});
        match parse_api_reply(&v) {
            Err(ClaudeError::MaxTokens { partial }) => assert_eq!(partial, "{\"title\":"),
            other => panic!("expected MaxTokens, got {other:?}"),
        }
    }

    fn status(code: i32, retry_after: Option<u64>) -> ClaudeError {
        ClaudeError::Status {
            code,
            body: String::new(),
            retry_after: retry_after.map(Duration::from_secs),
        }
    }

    #[test]
    fn retries_with_exponential_backoff() {
        let b = Backoff::default();
        let calls = Cell::new(0);
        let mut waits = Vec::new();
        let r: Result<(), _> = b.run(&mut |d| waits.push(d), || {
            calls.set(calls.get() + 1);
            Err(status(529, None))
        });
        assert!(matches!(r, Err(ClaudeError::Status { code: 529, .. })));
        assert_eq!(calls.get(), 4);
        assert_eq!(waits, [1, 2, 4].map(Duration::from_secs));
    }

    #[test]
    fn retry_succeeds_and_honours_retry_after() {
        let b = Backoff::default();
        let calls = Cell::new(0);
        let mut waits = Vec::new();
        let r = b.run(&mut |d| waits.push(d), || {
            calls.set(calls.get() + 1);
            match calls.get() {
                1 => Err(status(429, Some(7))),
                2 => Err(ClaudeError::Transport("reset".into())),
                _ => Ok(42),
            }
        });
        assert_eq!(r.unwrap(), 42);
        assert_eq!(waits, [7, 2].map(Duration::from_secs));
    }

    #[test]
    fn does_not_retry_client_errors() {
        for e in [
            status(400, None),
            status(401, None),
            ClaudeError::MaxTokens { partial: String::new() },
        ] {
            let calls = Cell::new(0);
            let mut e = Some(e);
            let r: Result<(), _> = Backoff::default().run(&mut |_| panic!("no wait expected"), || {
                calls.set(calls.get() + 1);
                Err(e.take().unwrap_or(ClaudeError::Parse("second call".into())))
            });
            assert!(r.is_err());
            assert_eq!(calls.get(), 1);
        }
    }

    #[test]
    fn cli_arguments_state_model_effort_and_schema() {
        let mut req = Request::new("p");
        req.effort = Effort::Max;
        req.json_schema = Some(json!({"type": "object"}));
        let a = ClaudeCli::args(&req);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).cloned();
        assert_eq!(after("--model").as_deref(), Some(DEFAULT_MODEL));
        assert_eq!(after("--effort").as_deref(), Some("max"));
        assert_eq!(after("--json-schema").as_deref(), Some(r#"{"type":"object"}"#));
        assert!(!a.iter().any(|x| x == "--bare"));
    }

    #[test]
    fn parses_cli_envelopes() {
        let ok = parse_cli_envelope(r#"{"is_error":false,"result":"PONG","subtype":"success"}"#, "claude-opus-5-5").unwrap();
        assert_eq!(ok.text, "PONG");
        assert_eq!(ok.usage, Usage::default());
        assert!(ok.per_model.is_empty());
        assert!(matches!(
            parse_cli_envelope(r#"{"is_error":true,"result":"boom","subtype":"error_during_execution"}"#, "m"),
            Err(ClaudeError::Status { .. })
        ));
        assert!(matches!(
            parse_cli_envelope(r#"{"is_error":false,"result":"{","stop_reason":"max_tokens"}"#, "m"),
            Err(ClaudeError::MaxTokens { .. })
        ));
        assert!(matches!(parse_cli_envelope("not json", "m"), Err(ClaudeError::Parse(_))));
    }

    #[test]
    fn parses_cli_usage_and_picks_the_answering_model() {
        let stdout = r#"{
            "is_error": false, "result": "song text", "stop_reason": "end_turn",
            "usage": {"input_tokens": 6120, "output_tokens": 5310,
                      "cache_read_input_tokens": 0, "cache_creation_input_tokens": 4988,
                      "output_tokens_details": {"thinking_tokens": 2760}},
            "modelUsage": {
                "claude-opus-5-5": {"inputTokens": 6000, "outputTokens": 5310,
                    "cacheReadInputTokens": 0, "cacheCreationInputTokens": 4988,
                    "thinkingTokens": 2760, "costUSD": 0.40},
                "claude-haiku-4-5": {"inputTokens": 120, "outputTokens": 8, "costUSD": 0.01}
            },
            "total_cost_usd": 0.41, "duration_ms": 61234
        }"#;
        let r = parse_cli_envelope(stdout, "claude-opus-5-5").unwrap();
        assert_eq!(r.text, "song text");
        assert_eq!(r.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(r.usage.input_tokens, Some(6120));
        assert_eq!(r.usage.output_tokens, Some(5310));
        assert_eq!(r.usage.cache_read_input_tokens, Some(0));
        assert_eq!(r.usage.cache_creation_input_tokens, Some(4988));
        assert_eq!(r.usage.thinking_tokens, Some(2760));
        assert_eq!(r.cost_usd, Some(0.41));
        assert_eq!(r.duration_ms, Some(61234));
        assert_eq!(r.per_model.len(), 2);
        assert_eq!(r.per_model[0].model, "claude-haiku-4-5"); // sorted by id
        assert_eq!(r.per_model[1].model, "claude-opus-5-5");
        assert_eq!(r.per_model[1].cost_usd, Some(0.40));
    }

    #[test]
    fn cli_model_tie_prefers_the_requested_model() {
        let per_model = vec![
            ModelUsage {
                model: "claude-a".into(),
                usage: Usage {
                    output_tokens: Some(10),
                    ..Usage::default()
                },
                cost_usd: None,
            },
            ModelUsage {
                model: "claude-b".into(),
                usage: Usage {
                    output_tokens: Some(10),
                    ..Usage::default()
                },
                cost_usd: None,
            },
        ];
        assert_eq!(pick_answering_model(&per_model, "claude-b").as_deref(), Some("claude-b"));
        assert_eq!(pick_answering_model(&per_model, "claude-z").as_deref(), Some("claude-a"));
        assert_eq!(pick_answering_model(&[], "claude-a"), None);
    }

    #[test]
    fn parses_a_minimal_cli_envelope() {
        let r = parse_cli_envelope(r#"{"is_error":false,"result":"x"}"#, "claude-opus-5-5").unwrap();
        assert_eq!(r.usage, Usage::default());
        assert!(r.per_model.is_empty());
        assert_eq!(r.cost_usd, None);
        assert_eq!(r.duration_ms, None);
        assert_eq!(r.model, None);
    }

    #[test]
    fn parses_api_usage() {
        let v = json!({
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn",
            "model": "claude-opus-5-5",
            "usage": {"input_tokens": 100, "output_tokens": 50,
                      "cache_read_input_tokens": 10, "cache_creation_input_tokens": 20},
        });
        let r = parse_api_reply(&v).unwrap();
        assert_eq!(r.usage.input_tokens, Some(100));
        assert_eq!(r.usage.output_tokens, Some(50));
        assert_eq!(r.usage.cache_read_input_tokens, Some(10));
        assert_eq!(r.usage.cache_creation_input_tokens, Some(20));
        assert_eq!(r.usage.thinking_tokens, None);
        assert_eq!(r.cost_usd, None);
        assert_eq!(
            r.per_model,
            vec![ModelUsage {
                model: "claude-opus-5-5".into(),
                usage: r.usage.clone(),
                cost_usd: None
            }]
        );
    }

    #[test]
    fn transport_parses_and_round_trips_serde() {
        assert_eq!("CLI".parse::<Transport>().unwrap(), Transport::Cli);
        assert_eq!("api".parse::<Transport>().unwrap(), Transport::Api);
        assert!("ftp".parse::<Transport>().is_err());
        assert_eq!(serde_json::to_string(&Transport::Api).unwrap(), "\"api\"");
        assert_eq!(serde_json::from_str::<Transport>("\"cli\"").unwrap(), Transport::Cli);
    }

    #[test]
    fn effort_parses_by_name() {
        assert_eq!("XHigh".parse::<Effort>().ok(), Some(Effort::XHigh));
        assert!(matches!("extreme".parse::<Effort>(), Err(ClaudeError::Config(_))));
    }
}

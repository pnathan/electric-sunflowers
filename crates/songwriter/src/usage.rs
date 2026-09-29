//! Token usage and cost of a Claude generation.
//!
//! `Usage` mirrors the fields the CLI and the Messages API report; an
//! absent count is `None`, never 0, so "not reported" is never confused
//! with "reported as zero". `Generation` records everything about one
//! call to Claude: which transport, what was requested, what answered,
//! the usage, and the cost, for the render sidecar (`crate::sidecar`) and
//! for display (`Generation::summary`).

use serde::{Deserialize, Serialize};

use crate::claude::Transport;

/// Token counts of one generation. Absent counts are `None`, never 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    /// Thinking tokens (counted inside `output_tokens`). CLI only; the
    /// Messages API does not report them separately.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_tokens: Option<u64>,
}

/// One model's share of a (possibly multi-model) generation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model: String,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Everything known about one call to Claude: transport, request,
/// answer, usage and cost. Written into the render sidecar
/// (`sidecar::RenderSidecar::generation`) and summarised for the person
/// (`Generation::summary`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generation {
    pub transport: Transport,
    pub requested_model: String,
    /// `Effort::as_str`.
    pub effort: String,
    /// The model that answered: the API reply's `model`, or, for the
    /// CLI, the `modelUsage` entry with the most output tokens (see
    /// `claude::pick_answering_model`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// Totals for the call.
    pub usage: Usage,
    /// CLI: every `modelUsage` entry, sorted by model id. API: one entry
    /// for `model`, when known.
    #[serde(default)]
    pub per_model: Vec<ModelUsage>,
    /// CLI `total_cost_usd`. The API reports no cost: stays `None` (no
    /// price table; honest absence over a guess).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// CLI `duration_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Wall time of the call as measured by this client (both
    /// transports), via `std::time::Instant` around `Claude::complete`.
    pub wall_ms: u64,
}

/// Groups digits of `n` by thousands with `,`, e.g. `6120` -> `"6,120"`.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

impl Generation {
    /// One line for display: `claude: <model> via <transport> (effort
    /// <effort>): <in> in (cache read <r>, cache write <w>), <out> out
    /// (thinking <t>), $<cost>, <secs> s`. Any value not reported by the
    /// transport is left out of the line entirely, never printed as 0
    /// (a reported 0, such as `cache_read_input_tokens: Some(0)`, is
    /// still printed: it is data, not absence). Seconds come from
    /// `duration_ms` when known, else `wall_ms`.
    pub fn summary(&self) -> String {
        let model = self
            .model
            .as_deref()
            .unwrap_or(self.requested_model.as_str());
        let mut out = format!(
            "claude: {model} via {} (effort {})",
            self.transport, self.effort
        );

        let mut clauses: Vec<String> = Vec::new();
        if let Some(inp) = self.usage.input_tokens {
            let mut s = format!("{} in", thousands(inp));
            let mut cache_bits: Vec<String> = Vec::new();
            if let Some(r) = self.usage.cache_read_input_tokens {
                cache_bits.push(format!("cache read {}", thousands(r)));
            }
            if let Some(w) = self.usage.cache_creation_input_tokens {
                cache_bits.push(format!("cache write {}", thousands(w)));
            }
            if !cache_bits.is_empty() {
                s.push_str(&format!(" ({})", cache_bits.join(", ")));
            }
            clauses.push(s);
        }
        if let Some(outp) = self.usage.output_tokens {
            let mut s = format!("{} out", thousands(outp));
            if let Some(t) = self.usage.thinking_tokens {
                s.push_str(&format!(" (thinking {})", thousands(t)));
            }
            clauses.push(s);
        }
        if let Some(cost) = self.cost_usd {
            clauses.push(format!("${cost:.2}"));
        }
        let secs = self.duration_ms.unwrap_or(self.wall_ms) as f64 / 1000.0;
        clauses.push(format!("{secs:.1} s"));

        out.push_str(": ");
        out.push_str(&clauses.join(", "));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::Effort;

    fn full() -> Generation {
        Generation {
            transport: Transport::Cli,
            requested_model: "claude-opus-5-5".into(),
            effort: Effort::High.as_str().into(),
            model: Some("claude-opus-5-5".into()),
            stop_reason: Some("end_turn".into()),
            usage: Usage {
                input_tokens: Some(6120),
                output_tokens: Some(5310),
                cache_read_input_tokens: Some(0),
                cache_creation_input_tokens: Some(4988),
                thinking_tokens: Some(2760),
            },
            per_model: vec![ModelUsage {
                model: "claude-opus-5-5".into(),
                usage: Usage::default(),
                cost_usd: Some(0.41),
            }],
            cost_usd: Some(0.41),
            duration_ms: Some(61_234),
            wall_ms: 62_010,
        }
    }

    #[test]
    fn summary_of_a_full_record() {
        // duration_ms (61234) wins over wall_ms (62010) when both are known
        // (docs/features-2.md 1.4: "seconds from duration_ms, else wall_ms").
        assert_eq!(
            full().summary(),
            "claude: claude-opus-5-5 via cli (effort high): 6,120 in (cache read 0, cache write 4,988), \
             5,310 out (thinking 2,760), $0.41, 61.2 s"
        );
    }

    #[test]
    fn summary_of_a_partial_record_omits_absent_values() {
        let g = Generation {
            transport: Transport::Api,
            requested_model: "claude-opus-5-5".into(),
            effort: Effort::Medium.as_str().into(),
            model: None,
            stop_reason: None,
            usage: Usage::default(),
            per_model: Vec::new(),
            cost_usd: None,
            duration_ms: None,
            wall_ms: 4_500,
        };
        assert_eq!(
            g.summary(),
            "claude: claude-opus-5-5 via api (effort medium): 4.5 s"
        );
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(120), "120");
        assert_eq!(thousands(6120), "6,120");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn generation_round_trips_through_json() {
        let g = full();
        let text = serde_json::to_string(&g).unwrap();
        let back: Generation = serde_json::from_str(&text).unwrap();
        assert_eq!(back, g);
    }
}

//! The `<stem>.render.json` sidecar: what a render command wrote and how
//! the song was written. One format shared by `sunflower` and `studio`,
//! read by `studio::library::RenderInfo` (wave 1 and 2 switch their own
//! writers to `RenderSidecar::write`; this module only defines the
//! format so both can be moved onto it without a wire change).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::usage::Generation;

/// The sidecar format version `RenderSidecar::write` stamps. Version 1
/// (no `version` key) is what earlier builds wrote; version 2 records the
/// song schema's arrival and changes no field. A reader ignores unknown
/// fields, so version 1 readers still read version 2.
pub const SIDECAR_VERSION: u32 = 2;

/// The `<stem>.render.json` sidecar. Every field is optional: a song
/// without the new fields (a solo song rendered before this feature)
/// still reads and writes one. Unknown JSON fields are ignored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderSidecar {
    /// Format version; `write` sets `SIDECAR_VERSION`. Absent reads as
    /// `None` (version 1).
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub seed: Option<u64>,
    /// Singer A as rendered.
    #[serde(default)]
    pub voice: Option<String>,
    /// Singer B (duet), wave 2 on.
    #[serde(default)]
    pub voice_b: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub style_label: Option<String>,
    /// Kept for old readers; equal to `generation.model` when a
    /// generation is recorded.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub generation: Option<Generation>,
    #[serde(default)]
    pub song_json: Option<String>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub sheet: Option<String>,
    /// The `<stem>.mix.json` applied, if any.
    #[serde(default)]
    pub mix: Option<String>,
    /// The `<stem>.stems` directory, if written.
    #[serde(default)]
    pub stems: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
}

impl RenderSidecar {
    /// Reads a sidecar leniently, field by field: a field of the wrong
    /// JSON type reads as `None` (or, for `generation`, is dropped)
    /// instead of failing the whole read. This is what lets a sidecar
    /// written by an earlier wave, or hand-edited, still load.
    pub fn read(path: &Path) -> Result<RenderSidecar, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::from_value(&v))
    }

    fn from_value(v: &Value) -> RenderSidecar {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        RenderSidecar {
            version: v
                .get("version")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            seed: v.get("seed").and_then(Value::as_u64),
            voice: s("voice"),
            voice_b: s("voice_b"),
            style: s("style"),
            style_label: s("style_label"),
            model: s("model"),
            generation: v
                .get("generation")
                .and_then(|g| serde_json::from_value::<Generation>(g.clone()).ok()),
            song_json: s("song_json"),
            audio: s("audio"),
            sheet: s("sheet"),
            mix: s("mix"),
            stems: s("stems"),
            created: s("created"),
        }
    }

    /// Writes atomically (`<path>.tmp` then rename). When `self.generation`
    /// is `None` and `path` already holds a sidecar with a generation for
    /// the same `song_json`, that generation (and its `model`) is kept: a
    /// re-render (a new seed, a mix change) does not erase the record of
    /// how the song was written.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut out = self.clone();
        out.version = Some(SIDECAR_VERSION);
        if out.generation.is_none() {
            if let Ok(prev) = RenderSidecar::read(path) {
                if prev.generation.is_some() && prev.song_json == out.song_json {
                    out.generation = prev.generation;
                    if out.model.is_none() {
                        out.model = prev.model;
                    }
                }
            }
        }
        let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        let mut tmp = path.as_os_str().to_os_string();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::{Effort, Transport};
    use crate::usage::Usage;

    fn tmp_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("songwriter-sidecar-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn generation() -> Generation {
        Generation {
            transport: Transport::Cli,
            requested_model: "claude-opus-5-5".into(),
            effort: Effort::High.as_str().into(),
            model: Some("claude-opus-5-5".into()),
            stop_reason: Some("end_turn".into()),
            usage: Usage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                ..Usage::default()
            },
            per_model: Vec::new(),
            cost_usd: Some(0.1),
            duration_ms: Some(1000),
            wall_ms: 1200,
        }
    }

    #[test]
    fn round_trips_through_json() {
        let side = RenderSidecar {
            version: Some(SIDECAR_VERSION),
            seed: Some(7),
            voice: Some("baritone".into()),
            voice_b: Some("alto".into()),
            style: Some("nashville".into()),
            style_label: Some("Nashville country".into()),
            model: Some("claude-opus-5-5".into()),
            generation: Some(generation()),
            song_json: Some("/tmp/porch-light.json".into()),
            audio: Some("/tmp/porch-light.ogg".into()),
            sheet: Some("/tmp/porch-light.sheet.json".into()),
            mix: None,
            stems: None,
            created: Some("2026-09-28T14:03:07Z".into()),
        };
        let path = tmp_path("round-trip.render.json");
        side.write(&path).unwrap();
        let back = RenderSidecar::read(&path).unwrap();
        assert_eq!(back, side);
    }

    #[test]
    fn write_stamps_the_version() {
        let path = tmp_path("stamped.render.json");
        RenderSidecar::default().write(&path).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["version"], SIDECAR_VERSION);
        assert_eq!(RenderSidecar::read(&path).unwrap().version, Some(2));
    }

    #[test]
    fn reads_todays_format_without_the_new_fields() {
        let path = tmp_path("today.render.json");
        std::fs::write(
            &path,
            r#"{"seed": 5, "voice": "alto", "style": "blues", "style_label": "Delta blues",
                "model": "claude-opus-5-5", "song_json": "/tmp/x.json", "audio": "/tmp/x.ogg",
                "sheet": "/tmp/x.sheet.json", "created": "2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        let side = RenderSidecar::read(&path).unwrap();
        assert_eq!(side.seed, Some(5));
        assert_eq!(side.voice.as_deref(), Some("alto"));
        assert_eq!(side.style.as_deref(), Some("blues"));
        assert_eq!(side.style_label.as_deref(), Some("Delta blues"));
        assert_eq!(side.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(side.song_json.as_deref(), Some("/tmp/x.json"));
        assert_eq!(side.audio.as_deref(), Some("/tmp/x.ogg"));
        assert_eq!(side.sheet.as_deref(), Some("/tmp/x.sheet.json"));
        assert_eq!(side.created.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(side.version, None, "no version key reads as version 1");
        assert_eq!(side.voice_b, None);
        assert_eq!(side.generation, None);
        assert_eq!(side.mix, None);
        assert_eq!(side.stems, None);
    }

    #[test]
    fn read_is_lenient_to_wrong_types() {
        let path = tmp_path("malformed.render.json");
        std::fs::write(
            &path,
            r#"{"seed": "not-a-number", "voice": 5, "generation": "oops", "model": null}"#,
        )
        .unwrap();
        let side = RenderSidecar::read(&path).unwrap();
        assert_eq!(side.seed, None);
        assert_eq!(side.voice, None);
        assert_eq!(side.generation, None);
        assert_eq!(side.model, None);
    }

    #[test]
    fn keeps_the_previous_generation_on_a_rewrite_for_the_same_song() {
        let path = tmp_path("keep-generation.render.json");
        let first = RenderSidecar {
            version: None,
            seed: Some(1),
            song_json: Some("/tmp/song.json".into()),
            model: Some("claude-opus-5-5".into()),
            generation: Some(generation()),
            ..RenderSidecar::default()
        };
        first.write(&path).unwrap();

        // A re-render (new seed, a mix change) with no generation of its own.
        let second = RenderSidecar {
            version: None,
            seed: Some(2),
            song_json: Some("/tmp/song.json".into()),
            ..RenderSidecar::default()
        };
        second.write(&path).unwrap();

        let back = RenderSidecar::read(&path).unwrap();
        assert_eq!(back.seed, Some(2));
        assert_eq!(back.generation, Some(generation()));
        assert_eq!(back.model.as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn does_not_keep_a_previous_generation_for_a_different_song() {
        let path = tmp_path("different-song.render.json");
        let first = RenderSidecar {
            song_json: Some("/tmp/song-a.json".into()),
            generation: Some(generation()),
            ..RenderSidecar::default()
        };
        first.write(&path).unwrap();

        let second = RenderSidecar {
            song_json: Some("/tmp/song-b.json".into()),
            ..RenderSidecar::default()
        };
        second.write(&path).unwrap();

        let back = RenderSidecar::read(&path).unwrap();
        assert_eq!(back.generation, None);
    }
}

//! The song library: a directory of `<stem>.json` songs with optional
//! `<stem>.ogg` audio and `<stem>.render.json` sidecars, plus the built-in
//! demo.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use song::Voice;

/// The stem the built-in demo renders to inside the library directory.
pub const DEMO_STEM: &str = "sunflower-demo";

/// Where a song's JSON comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// `engine::demo_song()`.
    Demo,
    /// A song JSON file.
    File(PathBuf),
}

/// What a render sidecar records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderInfo {
    pub seed: Option<u64>,
    pub voice: Option<Voice>,
    pub style: Option<String>,
    pub model: Option<String>,
    pub song_json: Option<PathBuf>,
    pub audio: Option<PathBuf>,
    pub created: Option<String>,
    /// How the song was written (`songwriter::sidecar::RenderSidecar`),
    /// when the sidecar records one.
    pub generation: Option<songwriter::usage::Generation>,
}

impl RenderInfo {
    /// Reads a `<stem>.render.json`. Unknown or malformed fields are left out.
    pub fn read(path: &Path) -> Result<RenderInfo, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Ok(RenderInfo {
            seed: v.get("seed").and_then(|x| x.as_u64()),
            voice: s("voice").and_then(|x| Voice::from_str(&x).ok()),
            style: s("style").filter(|x| !x.is_empty()),
            model: s("model"),
            song_json: s("song_json").map(PathBuf::from),
            audio: s("audio").map(PathBuf::from),
            created: s("created"),
            generation: v
                .get("generation")
                .and_then(|g| serde_json::from_value(g.clone()).ok()),
        })
    }
}

/// One song in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Display name: the file stem, or "Demo (built-in)".
    pub name: String,
    pub source: Source,
    /// `<dir>/<stem>`: where audio and sidecars go (`<stem>.ogg`, ...).
    pub stem: PathBuf,
    /// The audio file, when one exists.
    pub audio: Option<PathBuf>,
    /// The render sidecar, when one exists and parses.
    pub render: Option<RenderInfo>,
    /// Why the sidecar was not used, when it exists but failed to parse.
    pub render_error: Option<String>,
}

impl Entry {
    /// The entry for the song JSON at `path` (outside or inside a library).
    pub fn for_song(path: &Path) -> Entry {
        let stem = path.with_extension("");
        let name = stem
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "song".into());
        let mut e = Entry {
            name,
            source: Source::File(path.to_path_buf()),
            stem,
            audio: None,
            render: None,
            render_error: None,
        };
        e.refresh();
        e
    }

    /// The built-in demo, rendering into `dir`.
    pub fn demo(dir: &Path) -> Entry {
        let mut e = Entry {
            name: "Demo (built-in)".into(),
            source: Source::Demo,
            stem: dir.join(DEMO_STEM),
            audio: None,
            render: None,
            render_error: None,
        };
        e.refresh();
        e
    }

    /// `<stem>.<ext>`; `ext` may contain dots.
    pub fn sibling(&self, ext: &str) -> PathBuf {
        let mut s = self.stem.clone().into_os_string();
        s.push(".");
        s.push(ext);
        PathBuf::from(s)
    }

    /// Re-reads the sidecar and looks for audio on disk. A sidecar's
    /// `song_json` replaces the source when that file exists (a take of
    /// another song).
    pub fn refresh(&mut self) {
        self.render = None;
        self.render_error = None;
        let side = self.sibling("render.json");
        if side.exists() {
            match RenderInfo::read(&side) {
                Ok(r) => self.render = Some(r),
                Err(e) => self.render_error = Some(e),
            }
        }
        let ogg = self.sibling("ogg");
        self.audio = if ogg.is_file() {
            Some(ogg)
        } else {
            self.render
                .as_ref()
                .and_then(|r| r.audio.clone())
                .filter(|a| a.is_file())
        };
        // A sidecar's song_json names a take's own song when the stem has
        // none of its own (the stem's own JSON always wins).
        if self.source != Source::Demo {
            let own = matches!(&self.source, Source::File(p) if p.is_file());
            if !own {
                if let Some(p) = self
                    .render
                    .as_ref()
                    .and_then(|r| r.song_json.clone())
                    .filter(|p| p.is_file())
                {
                    self.source = Source::File(p);
                }
            }
        }
    }
}

/// The default library directory, `~/Music/sunflower`.
pub fn default_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Music").join("sunflower")
}

/// Lists `dir`: the demo first, then one entry per stem that has a song
/// JSON or a render sidecar, by name. `<stem>.sheet.json` files are skipped.
pub fn scan(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut stems: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        let stem = if let Some(s) = name.strip_suffix(".render.json") {
            s
        } else if name.ends_with(".sheet.json") {
            continue;
        } else if let Some(s) = name.strip_suffix(".json") {
            s
        } else {
            continue;
        };
        if stem.is_empty() || stem == DEMO_STEM {
            continue;
        }
        if !stems.iter().any(|x| x == stem) {
            stems.push(stem.to_string());
        }
    }
    stems.sort_by_key(|s| s.to_lowercase());
    let mut out = vec![Entry::demo(dir)];
    for s in stems {
        let json = dir.join(format!("{s}.json"));
        let e = Entry::for_song(&json);
        // A render sidecar with no song JSON beside it must name one that exists.
        if !json.is_file() && !matches!(&e.source, Source::File(p) if p.is_file()) {
            continue;
        }
        out.push(e);
    }
    Ok(out)
}

/// `dir/<slug>.json` for a new song, with `-2`, `-3`, ... added until
/// neither the JSON nor its audio exists.
pub fn fresh_stem(dir: &Path, title: &str) -> PathBuf {
    let base = slugify(title);
    for k in 1..10_000 {
        let name = if k == 1 {
            base.clone()
        } else {
            format!("{base}-{k}")
        };
        let stem = dir.join(&name);
        if !stem.with_extension("json").exists()
            && !stem.with_extension("ogg").exists()
            && !dir.join(format!("{name}.render.json")).exists()
        {
            return stem;
        }
    }
    dir.join(format!("{base}-{}", crate::jobs::random_seed() % 1_000_000))
}

/// Lowercase ASCII letters and digits, other runs as one `-`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash {
            out.push('-');
            dash = true;
        }
    }
    let t = out.trim_matches('-');
    if t.is_empty() {
        "song".into()
    } else {
        t.chars()
            .take(60)
            .collect::<String>()
            .trim_end_matches('-')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_pairs_files_by_stem() {
        let dir = std::env::temp_dir().join(format!("studio-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.json"), "{}").unwrap();
        std::fs::write(dir.join("a.ogg"), "x").unwrap();
        std::fs::write(dir.join("a.sheet.json"), "{}").unwrap();
        std::fs::write(dir.join("b.json"), "{}").unwrap();
        std::fs::write(
            dir.join("b.render.json"),
            r#"{"seed": 7, "voice": "alto", "style": "shanty"}"#,
        )
        .unwrap();
        // A take of a.json.
        let side = format!(
            r#"{{"seed": 3, "song_json": "{}"}}"#,
            dir.join("a.json").display()
        );
        std::fs::write(dir.join("c.render.json"), side).unwrap();
        // A take whose song is gone.
        std::fs::write(
            dir.join("d.render.json"),
            r#"{"seed": 3, "song_json": "/nonexistent/x.json"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("e.render.json"), "not json").unwrap();
        std::fs::write(dir.join("e.json"), "{}").unwrap();

        let v = scan(&dir).unwrap();
        let names: Vec<&str> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Demo (built-in)", "a", "b", "c", "e"]);
        assert_eq!(v[1].audio.as_deref(), Some(dir.join("a.ogg").as_path()));
        assert!(v[1].render.is_none());
        let b = v[2].render.as_ref().unwrap();
        assert_eq!(
            (b.seed, b.voice, b.style.as_deref()),
            (Some(7), Some(Voice::Alto), Some("shanty"))
        );
        assert_eq!(v[3].source, Source::File(dir.join("a.json")));
        assert!(v[4].render.is_none() && v[4].render_error.is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn slugs_and_fresh_stems() {
        assert_eq!(slugify("  Dust & Rain! "), "dust-rain");
        assert_eq!(slugify("!!!"), "song");
        let dir = std::env::temp_dir().join(format!("studio-fresh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dust.json"), "{}").unwrap();
        assert_eq!(fresh_stem(&dir, "Dust"), dir.join("dust-2"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

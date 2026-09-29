//! User settings: the Claude model, transport and effort, the songwriter's
//! default voice and duet choice, the studio's library directory, and the
//! export quality, read from and written to
//! `$XDG_CONFIG_HOME/electric-sunflowers/config.toml` (docs/features-2.md
//! section 2).
//!
//! Parsing is lenient, like the song JSON boundary (`song::wire`): a
//! missing file is not an error (`load` never fails), a bad value in the
//! file falls back to its default with a warning naming the key, and a
//! TOML syntax error falls back to all defaults with one warning. The API
//! key never goes in this file (CLAUDE.md: never ship a key); it stays in
//! `ANTHROPIC_API_KEY`.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use song::Voice;
use songwriter::claude::{Effort, Transport};

/// Default Claude model (`songwriter::claude::DEFAULT_MODEL`).
pub const DEFAULT_MODEL: &str = songwriter::claude::DEFAULT_MODEL;

/// What the songwriter should write toward: the user's own choice
/// (`--duet` / `--solo`) or the songwriter's (`Auto`, feature 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuetChoice {
    #[default]
    Auto,
    Solo,
    Duet,
}

impl DuetChoice {
    pub const ALL: &'static [DuetChoice] = &[DuetChoice::Auto, DuetChoice::Solo, DuetChoice::Duet];

    pub const fn as_str(self) -> &'static str {
        match self {
            DuetChoice::Auto => "auto",
            DuetChoice::Solo => "solo",
            DuetChoice::Duet => "duet",
        }
    }
}

impl std::fmt::Display for DuetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A duet choice that is not `auto`, `solo` or `duet`. Settings' own
/// error, not `songwriter::claude::ClaudeError`: this type belongs to
/// settings, not to the Claude transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownDuetChoice(pub String);

impl std::fmt::Display for UnknownDuetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown duet choice {:?} (want auto, solo, duet)",
            self.0
        )
    }
}

impl std::error::Error for UnknownDuetChoice {}

impl FromStr for DuetChoice {
    type Err = UnknownDuetChoice;
    fn from_str(s: &str) -> Result<Self, UnknownDuetChoice> {
        let t = s.trim();
        DuetChoice::ALL
            .iter()
            .copied()
            .find(|d| t.eq_ignore_ascii_case(d.as_str()))
            .ok_or_else(|| UnknownDuetChoice(s.to_string()))
    }
}

/// `[claude]`: how the songwriter talks to Claude.
#[derive(Clone, Debug, PartialEq)]
pub struct ClaudeSettings {
    pub model: String,
    pub transport: Transport,
    pub effort: Effort,
}

/// `[songwriter]`: what the songwriter writes toward by default.
#[derive(Clone, Debug, PartialEq)]
pub struct SongwriterSettings {
    /// Singer A; `None` is "auto" (the songwriter's choice).
    pub voice: Option<Voice>,
    pub duet: DuetChoice,
}

/// `[studio]`.
#[derive(Clone, Debug, PartialEq)]
pub struct StudioSettings {
    pub library: PathBuf,
}

/// `[export]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ExportSettings {
    pub ogg_quality: f32,
}

/// All settings, defaulting to the values documented in
/// docs/features-2.md section 2.1.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub claude: ClaudeSettings,
    pub songwriter: SongwriterSettings,
    pub studio: StudioSettings,
    pub export: ExportSettings,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            claude: ClaudeSettings {
                model: DEFAULT_MODEL.to_string(),
                transport: Transport::Cli,
                effort: Effort::High,
            },
            songwriter: SongwriterSettings {
                voice: None,
                duet: DuetChoice::Auto,
            },
            studio: StudioSettings {
                library: expand_home("~/Music/sunflower"),
            },
            export: ExportSettings { ogg_quality: 0.6 },
        }
    }
}

/// The env var this key names, or `None` when unset or empty (an empty
/// value is treated the same as unset, per docs/features-2.md 2.1).
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// The settings file path: `SUNFLOWER_CONFIG` when set, else
/// `$XDG_CONFIG_HOME/electric-sunflowers/config.toml` with
/// `XDG_CONFIG_HOME` unset or empty meaning `$HOME/.config`. `None` when
/// `SUNFLOWER_CONFIG` is unset and `HOME` is also unset (nowhere to put
/// a default path).
pub fn config_path() -> Option<PathBuf> {
    if let Some(p) = env_nonempty("SUNFLOWER_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let base = match env_nonempty("XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(env_nonempty("HOME")?).join(".config"),
    };
    Some(base.join("electric-sunflowers").join("config.toml"))
}

/// A leading ~ (`~` or `~/rest`) becomes `$HOME`; any other path is
/// returned unchanged. `HOME` unset: the `~` is left as-is (a relative
/// path starting with the literal character), since there is nowhere
/// else to put it.
pub fn expand_home(p: &str) -> PathBuf {
    if p == "~" {
        if let Some(home) = env_nonempty("HOME") {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = env_nonempty("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

/// The result of loading settings: the values (defaults where nothing
/// else applied), where they came from, and any warnings from a lenient
/// parse.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    pub path: Option<PathBuf>,
    /// Whether a settings file was found and read (a missing file is not
    /// a warning: `settings` is then simply the defaults).
    pub found: bool,
    pub warnings: Vec<String>,
}

/// Loads settings from `config_path()`. Never fails: a missing file, an
/// unreadable one, or one full of nonsense all produce `Settings::default`
/// (the last two also produce warnings) rather than an error the caller
/// must handle.
pub fn load() -> Loaded {
    let path = config_path();
    match path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        Some(text) => {
            let (settings, warnings) = parse(&text);
            Loaded {
                settings,
                path,
                found: true,
                warnings,
            }
        }
        None => Loaded {
            settings: Settings::default(),
            path,
            found: false,
            warnings: Vec::new(),
        },
    }
}

/// The value at `key`, quoted like a TOML string when it is a string,
/// else a short type name, for a "not one of ..." warning.
fn describe(v: &toml::Value) -> String {
    match v.as_str() {
        Some(s) => format!("{s:?}"),
        None => format!("a {}", v.type_str()),
    }
}

/// Parses `text` leniently: unknown top-level keys and unknown fields
/// inside a known table give a warning and are ignored; a known field of
/// the wrong type, or an unknown value for a closed vocabulary (an
/// effort name, a voice name, ...), gives its default and a warning
/// naming the key; a TOML syntax error gives every default and one
/// warning with the parser's message.
pub fn parse(text: &str) -> (Settings, Vec<String>) {
    let mut warnings = Vec::new();
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            warnings.push(format!("config.toml: {e}"));
            return (Settings::default(), warnings);
        }
    };
    let mut settings = Settings::default();
    for (key, value) in table.iter() {
        match key.as_str() {
            "claude" => parse_claude(value, &mut settings.claude, &mut warnings),
            "songwriter" => parse_songwriter(value, &mut settings.songwriter, &mut warnings),
            "studio" => parse_studio(value, &mut settings.studio, &mut warnings),
            "export" => parse_export(value, &mut settings.export, &mut warnings),
            other => warnings.push(format!("unknown key {other:?}")),
        }
    }
    (settings, warnings)
}

/// Runs `body` for every key in the `[section]` table, or, when `v` is
/// not a table at all, records one warning for the whole section.
fn each_field(
    section: &str,
    v: &toml::Value,
    warnings: &mut Vec<String>,
    mut body: impl FnMut(&str, &toml::Value, &mut Vec<String>),
) {
    match v.as_table() {
        Some(t) => {
            for (k, val) in t.iter() {
                body(k, val, warnings);
            }
        }
        None => warnings.push(format!(
            "{section}: expected a table of settings; using the defaults"
        )),
    }
}

fn parse_claude(v: &toml::Value, out: &mut ClaudeSettings, warnings: &mut Vec<String>) {
    each_field("claude", v, warnings, |k, val, warnings| match k {
        "model" => match val.as_str() {
            Some(s) => out_model(out, s),
            None => warnings.push(format!(
                "claude.model: expected a string; using {:?}",
                out.model
            )),
        },
        "transport" => match val.as_str().and_then(|s| s.parse::<Transport>().ok()) {
            Some(t) => out.transport = t,
            None => {
                warnings.push(format!(
                    "claude.transport: {} is not one of cli, api; using {}",
                    describe(val),
                    out.transport
                ));
            }
        },
        "effort" => match val.as_str().and_then(|s| s.parse::<Effort>().ok()) {
            Some(e) => out.effort = e,
            None => {
                let names = Effort::ALL
                    .iter()
                    .map(|e| e.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!(
                    "claude.effort: {} is not one of {names}; using {}",
                    describe(val),
                    out.effort
                ));
            }
        },
        other => warnings.push(format!("unknown key \"claude.{other}\"")),
    });
    // A closure cannot both capture `out` by unique reference and be
    // called with it as an argument; `out_model` sidesteps that for the
    // one field (`model`) that has no fallback other than "leave it be".
    fn out_model(out: &mut ClaudeSettings, s: &str) {
        out.model = s.to_string();
    }
}

fn parse_songwriter(v: &toml::Value, out: &mut SongwriterSettings, warnings: &mut Vec<String>) {
    each_field("songwriter", v, warnings, |k, val, warnings| match k {
        "voice" => match val.as_str() {
            Some(s) if s.trim().eq_ignore_ascii_case("auto") => out.voice = None,
            Some(s) => match s.parse::<Voice>() {
                Ok(voice) => out.voice = Some(voice),
                Err(_) => {
                    let names = Voice::ALL
                        .iter()
                        .map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    warnings.push(format!(
                        "songwriter.voice: {:?} is not one of auto, {names}; using auto",
                        s
                    ));
                    out.voice = None;
                }
            },
            None => {
                warnings.push(format!(
                    "songwriter.voice: {} is not one of auto, ...; using auto",
                    describe(val)
                ));
                out.voice = None;
            }
        },
        "duet" => match val.as_str().and_then(|s| s.parse::<DuetChoice>().ok()) {
            Some(d) => out.duet = d,
            None => {
                let names = DuetChoice::ALL
                    .iter()
                    .map(|d| d.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                warnings.push(format!(
                    "songwriter.duet: {} is not one of {names}; using auto",
                    describe(val)
                ));
                out.duet = DuetChoice::Auto;
            }
        },
        other => warnings.push(format!("unknown key \"songwriter.{other}\"")),
    });
}

fn parse_studio(v: &toml::Value, out: &mut StudioSettings, warnings: &mut Vec<String>) {
    each_field("studio", v, warnings, |k, val, warnings| match k {
        "library" => match val.as_str() {
            Some(s) => out.library = expand_home(s),
            None => warnings.push(format!(
                "studio.library: expected a string; using {}",
                out.library.display()
            )),
        },
        other => warnings.push(format!("unknown key \"studio.{other}\"")),
    });
}

fn parse_export(v: &toml::Value, out: &mut ExportSettings, warnings: &mut Vec<String>) {
    each_field("export", v, warnings, |k, val, warnings| match k {
        "ogg_quality" => {
            match val
                .as_float()
                .or_else(|| val.as_integer().map(|i| i as f64))
            {
                Some(n) => {
                    let clamped = n.clamp(-0.2, 1.0);
                    if clamped != n {
                        warnings.push(format!("export.ogg_quality: {n} is out of range -0.2 to 1.0; clamped to {clamped}"));
                    }
                    out.ogg_quality = clamped as f32;
                }
                None => warnings.push(format!(
                    "export.ogg_quality: expected a number; using {}",
                    out.ogg_quality
                )),
            }
        }
        other => warnings.push(format!("unknown key \"export.{other}\"")),
    });
}

/// Plain-typed mirror of `Settings` for `toml::to_string`: enums as their
/// wire spellings, the library path as a plain string. Kept private:
/// callers use `Settings` and `save`, never this shape.
#[derive(Serialize)]
struct SettingsMirror<'a> {
    claude: ClaudeMirror<'a>,
    songwriter: SongwriterMirror<'a>,
    studio: StudioMirror,
    export: ExportSettings,
}

#[derive(Serialize)]
struct ClaudeMirror<'a> {
    model: &'a str,
    transport: &'a str,
    effort: &'a str,
}

#[derive(Serialize)]
struct SongwriterMirror<'a> {
    voice: &'a str,
    duet: &'a str,
}

#[derive(Serialize)]
struct StudioMirror {
    library: String,
}

/// Writes `settings` to `path` as TOML: the header comment
/// (docs/features-2.md 2.1) then the values, in the same spellings
/// `parse` reads, so `parse(&fs::read_to_string(path)) == (settings,
/// [])`. Creates the parent directory if needed; writes atomically
/// (`<path>.tmp` then rename).
pub fn save(settings: &Settings, path: &Path) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mirror = SettingsMirror {
        claude: ClaudeMirror {
            model: &settings.claude.model,
            transport: settings.claude.transport.as_str(),
            effort: settings.claude.effort.as_str(),
        },
        songwriter: SongwriterMirror {
            voice: settings
                .songwriter
                .voice
                .map(Voice::as_str)
                .unwrap_or("auto"),
            duet: settings.songwriter.duet.as_str(),
        },
        studio: StudioMirror {
            library: settings.studio.library.display().to_string(),
        },
        export: settings.export,
    };
    let body = toml::to_string_pretty(&mirror).map_err(std::io::Error::other)?;
    let text = format!(
        "# electric-sunflowers settings. Written by the studio; comments are not kept.\n{body}"
    );

    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp_path = PathBuf::from(tmp);
    std::fs::write(&tmp_path, text)?;
    std::fs::rename(&tmp_path, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn duet_choice_parses_and_names_a_bad_value() {
        assert_eq!(" Duet ".parse::<DuetChoice>(), Ok(DuetChoice::Duet));
        let e = "trio".parse::<DuetChoice>().unwrap_err();
        assert_eq!(e, UnknownDuetChoice("trio".to_string()));
        assert!(e.to_string().contains("\"trio\""));
    }

    /// Serialises the env-var tests: `config_path` and `expand_home`
    /// read process-global environment state, so tests that set it
    /// cannot run concurrently with each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        keys: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(pairs: &[(&'static str, Option<&str>)]) -> EnvGuard {
            let mut keys = Vec::new();
            for (k, v) in pairs {
                keys.push((*k, std::env::var(k).ok()));
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
            EnvGuard { keys }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.keys {
                match v {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    #[test]
    fn defaults_match_the_documented_values() {
        let s = Settings::default();
        assert_eq!(s.claude.model, DEFAULT_MODEL);
        assert_eq!(s.claude.transport, Transport::Cli);
        assert_eq!(s.claude.effort, Effort::High);
        assert_eq!(s.songwriter.voice, None);
        assert_eq!(s.songwriter.duet, DuetChoice::Auto);
        assert_eq!(s.export.ogg_quality, 0.6);
    }

    #[test]
    fn config_path_follows_xdg_and_sunflower_config() {
        let _lock = ENV_LOCK.lock().unwrap();

        let _g = EnvGuard::set(&[
            ("SUNFLOWER_CONFIG", None),
            ("XDG_CONFIG_HOME", Some("/xdg")),
            ("HOME", Some("/home/u")),
        ]);
        assert_eq!(
            config_path(),
            Some(PathBuf::from("/xdg/electric-sunflowers/config.toml"))
        );

        let _g = EnvGuard::set(&[
            ("SUNFLOWER_CONFIG", None),
            ("XDG_CONFIG_HOME", Some("")),
            ("HOME", Some("/home/u")),
        ]);
        assert_eq!(
            config_path(),
            Some(PathBuf::from(
                "/home/u/.config/electric-sunflowers/config.toml"
            ))
        );

        let _g = EnvGuard::set(&[
            ("SUNFLOWER_CONFIG", None),
            ("XDG_CONFIG_HOME", None),
            ("HOME", Some("/home/u")),
        ]);
        assert_eq!(
            config_path(),
            Some(PathBuf::from(
                "/home/u/.config/electric-sunflowers/config.toml"
            ))
        );

        let _g = EnvGuard::set(&[
            ("SUNFLOWER_CONFIG", None),
            ("XDG_CONFIG_HOME", None),
            ("HOME", None),
        ]);
        assert_eq!(config_path(), None);

        let _g = EnvGuard::set(&[
            ("SUNFLOWER_CONFIG", Some("/other/config.toml")),
            ("XDG_CONFIG_HOME", Some("/xdg")),
            ("HOME", None),
        ]);
        assert_eq!(config_path(), Some(PathBuf::from("/other/config.toml")));
    }

    #[test]
    fn expand_home_handles_tilde_and_plain_paths() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = EnvGuard::set(&[("HOME", Some("/home/u"))]);
        assert_eq!(
            expand_home("~/Music/sunflower"),
            PathBuf::from("/home/u/Music/sunflower")
        );
        assert_eq!(expand_home("~"), PathBuf::from("/home/u"));
        assert_eq!(expand_home("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_home("relative/path"), PathBuf::from("relative/path"));

        let _g = EnvGuard::set(&[("HOME", None)]);
        assert_eq!(expand_home("~/x"), PathBuf::from("~/x"));
    }

    #[test]
    fn parse_reads_every_value() {
        let text = r#"
            [claude]
            model = "claude-fable-5-1"
            transport = "api"
            effort = "max"

            [songwriter]
            voice = "alto"
            duet = "duet"

            [studio]
            library = "/data/songs"

            [export]
            ogg_quality = 0.9
        "#;
        let (s, warnings) = parse(text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.claude.model, "claude-fable-5-1");
        assert_eq!(s.claude.transport, Transport::Api);
        assert_eq!(s.claude.effort, Effort::Max);
        assert_eq!(s.songwriter.voice, Some(Voice::Alto));
        assert_eq!(s.songwriter.duet, DuetChoice::Duet);
        assert_eq!(s.studio.library, PathBuf::from("/data/songs"));
        assert_eq!(s.export.ogg_quality, 0.9);
    }

    #[test]
    fn voice_auto_is_none() {
        let (s, warnings) = parse("[songwriter]\nvoice = \"auto\"\n");
        assert!(warnings.is_empty());
        assert_eq!(s.songwriter.voice, None);
    }

    #[test]
    fn bad_effort_warns_and_defaults() {
        let (s, warnings) = parse("[claude]\neffort = \"extreme\"\n");
        assert_eq!(s.claude.effort, Effort::High);
        assert_eq!(warnings, vec!["claude.effort: \"extreme\" is not one of low, medium, high, xhigh, max; using high"]);
    }

    #[test]
    fn bad_transport_warns_and_defaults() {
        let (s, warnings) = parse("[claude]\ntransport = \"ftp\"\n");
        assert_eq!(s.claude.transport, Transport::Cli);
        assert_eq!(
            warnings,
            vec!["claude.transport: \"ftp\" is not one of cli, api; using cli"]
        );
    }

    #[test]
    fn bad_voice_warns_and_defaults() {
        let (s, warnings) = parse("[songwriter]\nvoice = \"castrato\"\n");
        assert_eq!(s.songwriter.voice, None);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("songwriter.voice: \"castrato\" is not one of auto, "));
    }

    #[test]
    fn bad_duet_warns_and_defaults() {
        let (s, warnings) = parse("[songwriter]\nduet = \"trio\"\n");
        assert_eq!(s.songwriter.duet, DuetChoice::Auto);
        assert_eq!(
            warnings,
            vec!["songwriter.duet: \"trio\" is not one of auto, solo, duet; using auto"]
        );
    }

    #[test]
    fn wrong_type_warns_and_defaults() {
        let (s, warnings) = parse("[claude]\nmodel = 5\n");
        assert_eq!(s.claude.model, DEFAULT_MODEL);
        assert_eq!(
            warnings,
            vec![format!(
                "claude.model: expected a string; using {DEFAULT_MODEL:?}"
            )]
        );
    }

    #[test]
    fn unknown_keys_and_tables_warn_and_are_ignored() {
        let (s, warnings) = parse("unknown_top = 1\n[claude]\nfoo = 2\n");
        assert_eq!(s, Settings::default());
        // toml::Table iterates keys in lexicographic order by default.
        assert_eq!(
            warnings,
            vec![
                "unknown key \"claude.foo\"".to_string(),
                "unknown key \"unknown_top\"".to_string()
            ]
        );
    }

    #[test]
    fn syntax_error_gives_all_defaults_and_one_warning() {
        let (s, warnings) = parse("this is not [ valid toml");
        assert_eq!(s, Settings::default());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("config.toml: "));
    }

    #[test]
    fn ogg_quality_is_clamped_with_a_warning() {
        let (s, warnings) = parse("[export]\nogg_quality = 5.0\n");
        assert_eq!(s.export.ogg_quality, 1.0);
        assert_eq!(
            warnings,
            vec!["export.ogg_quality: 5 is out of range -0.2 to 1.0; clamped to 1".to_string()]
        );

        let (s, warnings) = parse("[export]\nogg_quality = -3.0\n");
        assert_eq!(s.export.ogg_quality, -0.2);
        assert_eq!(
            warnings,
            vec!["export.ogg_quality: -3 is out of range -0.2 to 1.0; clamped to -0.2".to_string()]
        );
    }

    #[test]
    fn save_then_parse_round_trips() {
        let dir = std::env::temp_dir().join(format!("settings-round-trip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let mut s = Settings::default();
        s.claude.model = "claude-fable-5-1".to_string();
        s.claude.transport = Transport::Api;
        s.claude.effort = Effort::Max;
        s.songwriter.voice = Some(Voice::Tenor);
        s.songwriter.duet = DuetChoice::Solo;
        s.studio.library = PathBuf::from("/data/library");
        s.export.ogg_quality = 0.42;

        save(&s, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# electric-sunflowers settings."));

        let (back, warnings) = parse(&text);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, s);
    }

    #[test]
    fn save_of_auto_voice_round_trips_to_none() {
        let dir =
            std::env::temp_dir().join(format!("settings-round-trip-auto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let s = Settings::default();
        save(&s, &path).unwrap();
        let (back, warnings) = parse(&std::fs::read_to_string(&path).unwrap());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back, s);
    }

    #[test]
    fn load_of_a_missing_file_is_not_an_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _g = EnvGuard::set(&[(
            "SUNFLOWER_CONFIG",
            Some("/nonexistent/dir/does-not-exist/config.toml"),
        )]);
        let loaded = load();
        assert!(!loaded.found);
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.settings, Settings::default());
    }
}

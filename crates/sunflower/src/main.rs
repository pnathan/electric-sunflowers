//! sunflower: command-line renderer for the Singer-Songwriter Bot. See
//! CLAUDE.md at the repo root for what the system is.
//!
//! One pipeline for every command: normalise (printing the repairs),
//! render the stems (`engine::render`, one rayon task graph; set
//! RAYON_NUM_THREADS=1 for a single thread), mix with the band, export by
//! the output file's extension.

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use engine::{BandPart, Progress};
use export::{Format, Meta};
use song::{Band, Repair, Song, Voice};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "sunflower", about = "Render songs for the Singer-Songwriter Bot")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Render settings shared by every rendering command.
#[derive(Args)]
struct RenderArgs {
    /// Song seed; random (and printed) when left out.
    #[arg(long)]
    seed: Option<u32>,
    /// Singing voice; auto keeps the song's.
    #[arg(long, value_enum, default_value = "auto")]
    voice: VoiceArg,
}

/// Output settings shared by every rendering command.
#[derive(Args)]
struct ExportArgs {
    /// Output file; the extension picks the format (.ogg, .flac, .wav).
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// WAV only: 32-bit float samples instead of 16-bit PCM.
    #[arg(long)]
    float: bool,
    /// Ogg Vorbis VBR quality, roughly -0.2 to 1.0 (0.6 is about 192 kb/s
    /// for a 44.1 kHz stereo signal).
    #[arg(long, default_value_t = 0.6)]
    quality: f32,
    /// FLAC only: 16-bit output instead of the default 24-bit.
    #[arg(long)]
    flac16: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Render the built-in demo song. Output defaults to song.ogg.
    Demo {
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        export: ExportArgs,
    },
    /// Normalize, style, render and mix a song JSON file. Output defaults
    /// to song.ogg.
    Render {
        song: PathBuf,
        #[arg(long)]
        style: Option<String>,
        /// Switch off a band part: drums, bass, harmonyGuitar, harp,
        /// violin, choir, harmonies, doubles. May be given more than once.
        #[arg(long = "no", value_name = "PART", value_parser = parse_band_part)]
        no: Vec<BandPart>,
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        export: ExportArgs,
    },
    /// Write a new song with Claude, then render and mix it. Output
    /// defaults to <title-slug>.ogg.
    Write {
        mood: String,
        #[arg(long)]
        style: Option<String>,
        #[arg(long, value_enum, default_value = "cli")]
        via: Via,
        #[arg(long)]
        model: Option<String>,
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        export: ExportArgs,
    },
    /// List style keys and labels.
    Styles,
}

#[derive(Clone, Copy, ValueEnum)]
enum Via {
    Cli,
    Api,
}

/// `--voice` values.
#[derive(Clone, Copy, ValueEnum)]
enum VoiceArg {
    Auto,
    Bass,
    Baritone,
    Tenor,
    Alto,
    Soprano,
}

impl VoiceArg {
    fn voice(self) -> Option<Voice> {
        match self {
            VoiceArg::Auto => None,
            VoiceArg::Bass => Some(Voice::Bass),
            VoiceArg::Baritone => Some(Voice::Baritone),
            VoiceArg::Tenor => Some(Voice::Tenor),
            VoiceArg::Alto => Some(Voice::Alto),
            VoiceArg::Soprano => Some(Voice::Soprano),
        }
    }
}

fn parse_band_part(s: &str) -> Result<BandPart, String> {
    BandPart::from_name(s).ok_or_else(|| {
        let names: Vec<&str> = BandPart::ALL.iter().map(|p| p.name()).collect();
        format!("unknown --no track {s:?} (want one of: {})", names.join(", "))
    })
}

fn main() {
    sfcore::fp::init_pool(None);
    sfcore::fp::flush_denormals();
    if let Err(e) = run() {
        eprintln!("sunflower: error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Demo { render, export } => {
            let song = engine::demo_song();
            let out = export.out.clone().unwrap_or_else(|| PathBuf::from("song.ogg"));
            pipeline(song, &render, song.band, &out, &export)
        }
        Cmd::Render { song, style, no, render, export } => {
            let out = export.out.clone().unwrap_or_else(|| PathBuf::from("song.ogg"));
            let text = std::fs::read_to_string(&song).with_context(|| format!("reading song JSON from {}", song.display()))?;
            let raw: serde_json::Value =
                serde_json::from_str(&text).with_context(|| format!("parsing JSON in {}", song.display()))?;
            let mut song = normalize(&raw, "song")?;
            if let Some(key) = style {
                apply_style(&mut song, &key)?;
            }
            let mut band = song.band;
            for part in no {
                part.switch_off(&mut band);
            }
            pipeline(&song, &render, band, &out, &export)
        }
        Cmd::Write { mood, style, via, model, render, export } => cmd_write(&mood, style.as_deref(), via, model.as_deref(), &render, &export),
        Cmd::Styles => cmd_styles(),
    }
}

/// Lowercases `s`, replaces runs of non-alphanumerics with a single `-`, and
/// trims leading/trailing `-`. Names a `write` output after the song title.
fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "song".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Prints each repair as a warning.
fn warn_repairs(what: &str, repairs: &[Repair]) {
    for r in repairs {
        eprintln!("sunflower: warning: {what}: {r}");
    }
}

/// Normalises a song reply, printing the repairs as warnings.
fn normalize(raw: &serde_json::Value, what: &str) -> Result<Song> {
    let (song, repairs) = song::normalize_value(raw).map_err(|e| anyhow!("{what} failed to normalize: {e}"))?;
    warn_repairs(what, &repairs);
    Ok(song)
}

/// Applies style `key` to `song`, printing the repairs as warnings.
fn apply_style(song: &mut Song, key: &str) -> Result<()> {
    let repairs = songwriter::styles::apply_style(key, song).map_err(|e| anyhow!("{e}"))?;
    warn_repairs(&format!("style {key}"), &repairs);
    Ok(())
}

/// A random 32-bit seed. Not cryptographic: process id xor the clock.
fn random_seed() -> u32 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id() as u128;
    ((nanos ^ (pid << 32) ^ 0x9E3779B97F4A7C15) as u64 ^ ((nanos >> 64) as u64)) as u32
}

/// The seed from `--seed`, or a random one, printed so the run can be
/// reproduced.
fn resolve_seed(seed: Option<u32>) -> u32 {
    seed.unwrap_or_else(|| {
        let s = random_seed();
        eprintln!("sunflower: seed {s}");
        s
    })
}

/// Prints the count of finished render tasks.
struct Report;

impl Progress for Report {
    fn advance(&self, done: usize, total: usize) {
        eprintln!("sunflower: rendered {done}/{total} tasks");
    }
}

/// Renders `song`, mixes the parts `band` turns on and writes `out` in the
/// format its extension names.
fn pipeline(song: &Song, render: &RenderArgs, band: Band, out: &Path, export: &ExportArgs) -> Result<()> {
    let fmt = Format::from_path(out, export.quality, export.flac16, export.float)?;
    let seed = resolve_seed(render.seed) as u64;
    eprintln!("sunflower: rendering on {} threads", rayon::current_num_threads());
    let (_, stems) = engine::render(song, seed, render.voice.voice(), &Report);
    eprintln!("sunflower: mixing");
    let m = engine::mix(&stems, &band, seed);
    drop(stems);
    export::write(out, &m.l, &m.r, sfcore::SR as u32, &song_meta(song), fmt)
        .with_context(|| format!("writing audio to {}", out.display()))?;
    eprintln!("sunflower: wrote {} ({:.1} s, 2 channels x {} samples)", out.display(), m.l.len() as f64 / sfcore::SR_F, m.l.len());
    Ok(())
}

/// The style label for `song.style`'s key, if a style was applied.
fn style_label(song: &Song) -> String {
    song.style.as_deref().and_then(|key| songwriter::styles::style(key).ok()).map(|s| s.label.to_string()).unwrap_or_default()
}

/// Tags for the exported file: title and liner note from the song, artist
/// "Claude" (the `Meta` default), the current year, the style label.
fn song_meta(song: &Song) -> Meta {
    Meta {
        title: song.title.clone(),
        artist: String::new(),
        comment: song.note.clone(),
        date: current_year().to_string(),
        style: style_label(song),
    }
}

fn cmd_write(mood: &str, style: Option<&str>, via: Via, model: Option<&str>, render: &RenderArgs, export: &ExportArgs) -> Result<()> {
    let seed = resolve_seed(render.seed);
    let render = RenderArgs { seed: Some(seed), voice: render.voice };
    let claude: Box<dyn songwriter::claude::Claude> = match via {
        Via::Cli => Box::new(songwriter::claude::ClaudeCli::default()),
        Via::Api => Box::new(songwriter::claude::ClaudeApi::from_env().map_err(|e| anyhow!("could not build the API client: {e}"))?),
    };
    let year = current_year(); // CLAUDE.md: age counted from 1999 as of the current year.
    let style_id = style
        .filter(|s| !s.trim().eq_ignore_ascii_case("auto"))
        .map(|s| s.parse::<songwriter::styles::StyleId>())
        .transpose()
        .map_err(|e| anyhow!("{e}"))?;
    let mut rng = songwriter::Rng::stream(seed as u64, songwriter::WRITE_TAG);
    let req = songwriter::WriteRequest {
        mood,
        voice: render.voice.voice(),
        style: style_id,
        year,
        model: model.map(|s| s.to_string()),
        effort: Default::default(),
    };

    eprintln!("sunflower: asking Claude to write the song ({})", via_label(via));
    let w = songwriter::write_song(claude.as_ref(), &req, &mut rng).map_err(|e| anyhow!("songwriter: {e}"))?;
    let (raw, dir) = (w.raw, w.direction);

    // Save the model's reply before validating it, so a rejected song is kept.
    let title = raw.get("title").and_then(|t| t.as_str()).unwrap_or("song");
    let out = export.out.clone().unwrap_or_else(|| PathBuf::from(format!("{}.ogg", slugify(title))));
    let json_path = out.with_extension("json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&raw)?)
        .with_context(|| format!("saving raw song JSON to {}", json_path.display()))?;
    eprintln!("sunflower: saved raw song JSON to {}", json_path.display());

    let mut song = normalize(&raw, "written song")?;
    apply_style(&mut song, dir.style.as_str()).with_context(|| format!("applying style {:?} to the written song", dir.style))?;

    println!("title: {}", song.title);
    println!("style: {} ({})", dir.style.as_str(), dir.label);
    println!("form: {}", dir.form);
    println!("key: {} {}", song.key, song.mode);
    println!("meter: {}", song.meter);
    println!("tempo: {:.0}", song.tempo_bpm);

    pipeline(&song, &render, song.band, &out, export)
}

fn via_label(via: Via) -> &'static str {
    match via {
        Via::Cli => "claude CLI",
        Via::Api => "Anthropic API",
    }
}

/// The current UTC year (CLAUDE.md: the songwriter persona's age is counted
/// from 1999 as of the current year).
fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Days since the epoch, then a plain proleptic-Gregorian civil-from-days
    // calculation (Howard Hinnant's algorithm) to get the year without
    // pulling in a chrono dependency.
    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }) as i32
}

fn cmd_styles() -> Result<()> {
    for (key, style) in songwriter::styles::styles() {
        println!("{key:14} {}", style.label);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_names_parse_to_band_parts() {
        for p in BandPart::ALL {
            assert_eq!(parse_band_part(p.name()), Ok(p));
        }
        assert!(parse_band_part("bogus").is_err_and(|e| e.contains("unknown --no track")));
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("  Dust & Rain! "), "dust-rain");
        assert_eq!(slugify("!!!"), "song");
    }
}

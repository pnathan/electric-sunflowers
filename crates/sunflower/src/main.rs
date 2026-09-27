//! sunflower: command-line renderer for the Singer-Songwriter Bot. See
//! CLAUDE.md at the repo root for what the system is; this binary is the
//! long-lived product the JS page's audio engine is being ported into.

mod style_glue;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use compose::song::{normalize_song, Song};
use compose::voices::Voice as ComposeVoice;
use dsp::mix::TRACKS;
use export::{BitDepth, ExportOpts, Meta};
use sfcore::tuning::Tuning;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Parser)]
#[command(name = "sunflower", about = "Render songs for the Singer-Songwriter Bot")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Render the built-in demo song.
    Demo {
        /// Output file; the extension picks the format (.ogg, .flac, .wav).
        /// Defaults to song.ogg.
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        seed: Option<u32>,
        #[arg(long)]
        voice: Option<String>,
        /// WAV only: 32-bit float samples instead of 16-bit PCM.
        #[arg(long)]
        float: bool,
        /// Ogg Vorbis VBR quality, roughly -0.2 to 1.0 (default 0.6, about
        /// 192 kb/s for a 44.1 kHz stereo signal).
        #[arg(long)]
        quality: Option<f32>,
        /// FLAC only: 16-bit output instead of the default 24-bit.
        #[arg(long)]
        flac16: bool,
        /// Use the single-threaded render/mix path instead of the default
        /// threaded one.
        #[arg(long)]
        sequential: bool,
    },
    /// Normalize, style, render and mix a song JSON file.
    Render {
        song: PathBuf,
        /// Output file; the extension picks the format (.ogg, .flac, .wav).
        /// Defaults to song.ogg.
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        seed: Option<u32>,
        #[arg(long)]
        voice: Option<String>,
        #[arg(long)]
        style: Option<String>,
        /// Disable a band track: drums, bass, harmonyGuitar, harp, violin,
        /// choir, harmonies, doubles. May be given more than once.
        #[arg(long = "no")]
        no: Vec<String>,
        /// WAV only: 32-bit float samples instead of 16-bit PCM.
        #[arg(long)]
        float: bool,
        /// Ogg Vorbis VBR quality, roughly -0.2 to 1.0 (default 0.6, about
        /// 192 kb/s for a 44.1 kHz stereo signal).
        #[arg(long)]
        quality: Option<f32>,
        /// FLAC only: 16-bit output instead of the default 24-bit.
        #[arg(long)]
        flac16: bool,
        /// Use the single-threaded render/mix path instead of the default
        /// threaded one.
        #[arg(long)]
        sequential: bool,
    },
    /// Write a new song with Claude, then render and mix it.
    Write {
        mood: String,
        #[arg(long)]
        style: Option<String>,
        #[arg(long)]
        voice: Option<String>,
        #[arg(long, value_enum, default_value = "cli")]
        via: Via,
        #[arg(long)]
        model: Option<String>,
        /// Output file; the extension picks the format (.ogg, .flac, .wav).
        /// Defaults to <title-slug>.ogg.
        #[arg(short, long)]
        out: Option<PathBuf>,
        #[arg(long)]
        seed: Option<u32>,
        /// WAV only: 32-bit float samples instead of 16-bit PCM.
        #[arg(long)]
        float: bool,
        /// Ogg Vorbis VBR quality, roughly -0.2 to 1.0 (default 0.6, about
        /// 192 kb/s for a 44.1 kHz stereo signal).
        #[arg(long)]
        quality: Option<f32>,
        /// FLAC only: 16-bit output instead of the default 24-bit.
        #[arg(long)]
        flac16: bool,
        /// Use the single-threaded render/mix path instead of the default
        /// threaded one.
        #[arg(long)]
        sequential: bool,
    },
    /// List style keys and labels.
    Styles,
}

#[derive(Clone, Copy, ValueEnum)]
enum Via {
    Cli,
    Api,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("sunflower: error: {e:#}");
        std::process::exit(1);
    }
}

/// Turns a `--float`/`--quality`/`--flac16` trio into `ExportOpts`.
fn export_opts(float: bool, quality: Option<f32>, flac16: bool) -> ExportOpts {
    ExportOpts {
        ogg_quality: quality.unwrap_or(0.6),
        flac_bits: if flac16 { BitDepth::Bits16 } else { BitDepth::Bits24 },
        wav_float: float,
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Demo { out, seed, voice, float, quality, flac16, sequential } => {
            let out = out.unwrap_or_else(|| PathBuf::from("song.ogg"));
            let opts = export_opts(float, quality, flac16);
            cmd_demo(&out, seed, voice.as_deref(), &opts, sequential)
        }
        Cmd::Render { song, out, seed, voice, style, no, float, quality, flac16, sequential } => {
            let out = out.unwrap_or_else(|| PathBuf::from("song.ogg"));
            let opts = export_opts(float, quality, flac16);
            cmd_render(&song, &out, seed, voice.as_deref(), style.as_deref(), &no, &opts, sequential)
        }
        Cmd::Write { mood, style, voice, via, model, out, seed, float, quality, flac16, sequential } => {
            let opts = export_opts(float, quality, flac16);
            cmd_write(&mood, style.as_deref(), voice.as_deref(), via, model.as_deref(), out, seed, &opts, sequential)
        }
        Cmd::Styles => cmd_styles(),
    }
}

/// Lowercases `s`, replaces runs of non-alphanumerics with a single `-`, and
/// trims leading/trailing `-`. Used to name a `write`-command's output file
/// after the song's title when `-o` is not given.
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

fn parse_voice(v: Option<&str>) -> Result<Option<ComposeVoice>> {
    match v {
        None => Ok(None),
        Some(s) if s.eq_ignore_ascii_case("auto") => Ok(None),
        Some(s) => ComposeVoice::from_str(s)
            .map(Some)
            .map_err(|_| anyhow!("unknown voice {s:?} (want bass, baritone, tenor, alto or soprano)")),
    }
}

/// A random 32-bit seed, printed so the run can be reproduced. Not
/// cryptographic: process id xor the system clock is plenty for a CLI
/// default that only needs to look different run to run.
fn random_seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    ((nanos ^ (pid << 32) ^ 0x9E3779B97F4A7C15) as u64 ^ ((nanos >> 64) as u64)) as u32
}

fn resolve_seed(seed: Option<u32>) -> u32 {
    match seed {
        Some(s) => s,
        None => {
            let s = random_seed();
            eprintln!("sunflower: seed {s}");
            s
        }
    }
}

/// Validates `--no` track names against the vocabulary `TRACKS` uses, and
/// builds the `enabled(spec)` predicate: `spec.always || song`'s own band
/// flag for that spec, with any `--no`'d name forced off. Errors (does not
/// panic) on an unknown track name.
fn validate_no(no: &[String]) -> Result<()> {
    for n in no {
        if !style_glue::BAND_TRACK_NAMES.contains(&n.as_str()) {
            bail!(
                "unknown --no track {n:?} (want one of: {})",
                style_glue::BAND_TRACK_NAMES.join(", ")
            );
        }
    }
    Ok(())
}

fn build_enabled(song: &Song, no: &[String]) -> Result<impl Fn(&dsp::mix::TrackSpec) -> bool> {
    validate_no(no)?;
    let disabled: Vec<String> = no.to_vec();
    let song = song.clone();
    Ok(move |t: &dsp::mix::TrackSpec| {
        if let Some(b) = t.band {
            if disabled.iter().any(|d| d == b) {
                return false;
            }
        }
        t.always || t.band.map(|b| style_glue::song_band_flag(&song, b)).unwrap_or(false)
    })
}

/// Renders and mixes `song`. Uses the threaded render/mix path by default
/// (`sequential=false`); `--sequential` selects the single-threaded path
/// instead (the two are bit-identical, see
/// `crates/engine/tests/threaded_parity.rs`).
fn render_and_mix(
    song: &Song,
    seed: u32,
    voice: Option<ComposeVoice>,
    no: &[String],
    sequential: bool,
) -> Result<(Vec<f32>, Vec<f32>)> {
    let tuning = Tuning::default();
    let enabled = build_enabled(song, no)?;
    if sequential {
        eprintln!("sunflower: preparing and rendering tracks (sequential)");
        let mut rendered = {
            let mut progress = |label: &str, frac: f64| {
                eprintln!("sunflower: {label} ({:.0}%)", frac * 100.0);
            };
            engine::render_song(song, seed, voice, &tuning, Some(&mut progress))
        };
        eprintln!("sunflower: mixing");
        let (l, r) = engine::mix(&mut rendered, enabled, seed);
        Ok((l, r))
    } else {
        eprintln!("sunflower: preparing and rendering tracks (threaded)");
        let mut rendered = engine::render_song_threaded(song, seed, voice, &tuning);
        eprintln!("sunflower: mixing");
        let (l, r) = engine::mix_threaded(&mut rendered, enabled, seed);
        Ok((l, r))
    }
}

/// The style label for `song.style`'s key, if any style has been applied.
fn style_label(song: &Song) -> String {
    song.style
        .as_deref()
        .and_then(|key| songwriter::styles::styles().iter().find(|(k, _)| *k == key))
        .map(|(_, s)| s.label.to_string())
        .unwrap_or_default()
}

/// Builds the tags written into the exported file: title and liner note
/// from the song itself, artist defaults to "Claude" inside `export::Meta`,
/// date is the current year, genre/style is the style's label if a style
/// was applied.
fn song_meta(song: &Song) -> Meta {
    Meta {
        title: song.title.clone(),
        artist: String::new(),
        comment: song.note.clone(),
        date: current_year().to_string(),
        style: style_label(song),
    }
}

fn write_output(out: &Path, l: &[f32], r: &[f32], meta: &Meta, opts: &ExportOpts) -> Result<()> {
    export::write_audio(out, l, r, sfcore::SR as u32, meta, opts)
        .map_err(|e| anyhow!("{e}"))
        .with_context(|| format!("writing audio to {}", out.display()))?;
    eprintln!(
        "sunflower: wrote {} ({:.1}s, {} channels x {} samples)",
        out.display(),
        l.len() as f64 / sfcore::SR_F,
        2,
        l.len()
    );
    Ok(())
}

fn cmd_demo(
    out: &Path,
    seed: Option<u32>,
    voice: Option<&str>,
    opts: &ExportOpts,
    sequential: bool,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let voice = parse_voice(voice)?;
    let raw = engine::demo_song();
    let song = normalize_song(&raw).map_err(|e| anyhow!("demo song failed to normalize: {e}"))?;
    let (l, r) = render_and_mix(&song, seed, voice, &[], sequential)?;
    write_output(out, &l, &r, &song_meta(&song), opts)
}

fn cmd_render(
    song_path: &Path,
    out: &Path,
    seed: Option<u32>,
    voice: Option<&str>,
    style: Option<&str>,
    no: &[String],
    opts: &ExportOpts,
    sequential: bool,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let voice = parse_voice(voice)?;
    let text = std::fs::read_to_string(song_path)
        .with_context(|| format!("reading song JSON from {}", song_path.display()))?;
    let raw: serde_json::Value =
        serde_json::from_str(&text).with_context(|| format!("parsing JSON in {}", song_path.display()))?;
    validate_no(no)?;
    let mut song = normalize_song(&raw).map_err(|e| anyhow!("song failed to normalize: {e}"))?;
    if let Some(key) = style {
        style_glue::apply_style_to_song(&mut song, key).map_err(|e| anyhow!(e))?;
    }
    let (l, r) = render_and_mix(&song, seed, voice, no, sequential)?;
    write_output(out, &l, &r, &song_meta(&song), opts)
}

fn cmd_write(
    mood: &str,
    style: Option<&str>,
    voice: Option<&str>,
    via: Via,
    model: Option<&str>,
    out: Option<PathBuf>,
    seed: Option<u32>,
    opts: &ExportOpts,
    sequential: bool,
) -> Result<()> {
    let seed = resolve_seed(seed);
    let voice_enum = parse_voice(voice)?;

    let claude: Box<dyn songwriter::claude::Claude> = match via {
        Via::Cli => Box::new(songwriter::claude::ClaudeCli::default()),
        Via::Api => Box::new(
            songwriter::claude::ClaudeApi::from_env()
                .map_err(|e| anyhow!("could not build the API client: {e}"))?,
        ),
    };
    let year = current_year(); // CLAUDE.md: age counted from 1999 as of "the current year".
    let mut rng = sfcore::rng::rng_for(seed, "sunflower-write");
    let mut rand = move || rng.next();

    eprintln!("sunflower: asking Claude to write the song ({})", via_label(via));
    let write_opts = songwriter::WriteSongOptions { model: model.map(|s| s.to_string()), effort: None };
    let (raw, dir) =
        songwriter::write_song(claude.as_ref(), mood, voice, style, year, &mut rand, write_opts)
            .map_err(|e| anyhow!("songwriter: {e}"))?;

    // Save the model's reply before validating it, so a rejected song is kept.
    let title = raw.get("title").and_then(|t| t.as_str()).unwrap_or("song");
    let out = out.unwrap_or_else(|| PathBuf::from(format!("{}.ogg", slugify(title))));
    let json_path = out.with_extension("json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&raw)?)
        .with_context(|| format!("saving raw song JSON to {}", json_path.display()))?;
    eprintln!("sunflower: saved raw song JSON to {}", json_path.display());

    let mut song = normalize_song(&raw).map_err(|e| anyhow!("written song failed to normalize: {e}"))?;
    style_glue::apply_style_to_song(&mut song, &dir.style)
        .map_err(|e| anyhow!("applying style {:?} to the written song: {e}", dir.style))?;

    println!("title: {}", song.title);
    println!("style: {} ({})", dir.style, dir.label);
    println!("form: {}", dir.form);
    println!("key: {} {}", pc_name(song.key_pc), song.mode);
    println!("meter: {}", song.meter_name);
    println!("tempo: {:.0}", song.tempo);

    let (l, r) = render_and_mix(&song, seed, voice_enum, &[], sequential)?;
    write_output(&out, &l, &r, &song_meta(&song), opts)
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

fn pc_name(pc: i32) -> &'static str {
    const NAMES: [&str; 12] =
        ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    NAMES[pc.rem_euclid(12) as usize]
}

// Silence an unused-import warning: TRACKS is referenced only through
// dsp::mix::TRACKS below in tests / future track listing, kept for the
// `--no` help text's vocabulary check to stay honest against TRACKS itself.
#[allow(dead_code)]
fn _assert_track_count() {
    debug_assert_eq!(TRACKS.len(), style_glue::BAND_TRACK_NAMES.len() + 2);
}

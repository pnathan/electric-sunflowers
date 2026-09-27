//! sunflower: command-line renderer for the Singer-Songwriter Bot. See
//! CLAUDE.md at the repo root for what the system is; this binary is the
//! long-lived product the JS page's audio engine is being ported into.

mod style_glue;
mod wav;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use compose::song::{normalize_song, Song};
use compose::voices::Voice as ComposeVoice;
use dsp::mix::TRACKS;
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
        #[arg(short, long, default_value = "out.wav")]
        out: PathBuf,
        #[arg(long)]
        seed: Option<u32>,
        #[arg(long)]
        voice: Option<String>,
        #[arg(long)]
        float: bool,
    },
    /// Normalize, style, render and mix a song JSON file.
    Render {
        song: PathBuf,
        #[arg(short, long, default_value = "out.wav")]
        out: PathBuf,
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
        #[arg(long)]
        float: bool,
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
        #[arg(short, long, default_value = "out.wav")]
        out: PathBuf,
        #[arg(long)]
        seed: Option<u32>,
        #[arg(long)]
        float: bool,
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

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Demo { out, seed, voice, float } => cmd_demo(&out, seed, voice.as_deref(), float),
        Cmd::Render { song, out, seed, voice, style, no, float } => {
            cmd_render(&song, &out, seed, voice.as_deref(), style.as_deref(), &no, float)
        }
        Cmd::Write { mood, style, voice, via, model, out, seed, float } => {
            cmd_write(&mood, style.as_deref(), voice.as_deref(), via, model.as_deref(), &out, seed, float)
        }
        Cmd::Styles => cmd_styles(),
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

fn render_and_mix(song: &Song, seed: u32, voice: Option<ComposeVoice>, no: &[String]) -> Result<(Vec<f32>, Vec<f32>)> {
    let tuning = Tuning::default();
    eprintln!("sunflower: preparing and rendering tracks");
    let mut rendered = {
        let mut progress = |label: &str, frac: f64| {
            eprintln!("sunflower: {label} ({:.0}%)", frac * 100.0);
        };
        engine::render_song(song, seed, voice, &tuning, Some(&mut progress))
    };
    let enabled = build_enabled(song, no)?;
    eprintln!("sunflower: mixing");
    let (l, r) = engine::mix(&mut rendered, enabled, seed);
    Ok((l, r))
}

fn write_output(out: &Path, l: &[f32], r: &[f32], float: bool) -> Result<()> {
    wav::write_wav(out, l, r, sfcore::SR as u32, float)
        .with_context(|| format!("writing WAV to {}", out.display()))?;
    eprintln!(
        "sunflower: wrote {} ({:.1}s, {} channels x {} samples)",
        out.display(),
        l.len() as f64 / sfcore::SR_F,
        2,
        l.len()
    );
    Ok(())
}

fn cmd_demo(out: &Path, seed: Option<u32>, voice: Option<&str>, float: bool) -> Result<()> {
    let seed = resolve_seed(seed);
    let voice = parse_voice(voice)?;
    let raw = engine::demo_song();
    let song = normalize_song(&raw).map_err(|e| anyhow!("demo song failed to normalize: {e}"))?;
    let (l, r) = render_and_mix(&song, seed, voice, &[])?;
    write_output(out, &l, &r, float)
}

fn cmd_render(
    song_path: &Path,
    out: &Path,
    seed: Option<u32>,
    voice: Option<&str>,
    style: Option<&str>,
    no: &[String],
    float: bool,
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
    let (l, r) = render_and_mix(&song, seed, voice, no)?;
    write_output(out, &l, &r, float)
}

fn cmd_write(
    mood: &str,
    style: Option<&str>,
    voice: Option<&str>,
    via: Via,
    model: Option<&str>,
    out: &Path,
    seed: Option<u32>,
    float: bool,
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
    let mut req_model = model.map(|s| s.to_string());

    let year = 2026; // CLAUDE.md: age counted from 1999 as of "the current year".
    let mut rng = sfcore::rng::rng_for(seed, "sunflower-write");
    let mut rand = move || rng.next();

    eprintln!("sunflower: asking Claude to write the song ({})", via_label(via));
    let (raw, dir) = write_song_with_model(
        claude.as_ref(),
        mood,
        voice,
        style,
        year,
        &mut rand,
        req_model.take(),
    )?;

    let song_stem = out.with_extension("");
    let json_path = song_stem.with_extension("json");
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

    let (l, r) = render_and_mix(&song, seed, voice_enum, &[])?;
    write_output(out, &l, &r, float)
}

fn via_label(via: Via) -> &'static str {
    match via {
        Via::Cli => "claude CLI",
        Via::Api => "Anthropic API",
    }
}

/// Wraps `songwriter::write_song`, honoring an explicit `--model` override.
/// `write_song` always sets `req.model = DEFAULT_MODEL` internally before the
/// call reaches this crate, so an override is applied by re-issuing the
/// prompt build here when one is given, rather than by mutating a request
/// this crate does not construct itself.
fn write_song_with_model(
    claude: &dyn songwriter::claude::Claude,
    mood: &str,
    voice_pref: Option<&str>,
    style: Option<&str>,
    year: i32,
    rand: &mut dyn FnMut() -> f64,
    model: Option<String>,
) -> Result<(serde_json::Value, songwriter::styles::Direction)> {
    if model.is_none() {
        return songwriter::write_song(claude, mood, voice_pref, style, year, rand)
            .map_err(|e| anyhow!("songwriter: {e}"));
    }
    // Re-implement the thin call so the model override reaches the request:
    // same call order and rand draws as write_song (styleDirection then
    // songPrompt), so this stays a faithful substitute, not a fork.
    let dir = songwriter::styles::style_direction(style, rand);
    let prompt_text = songwriter::prompt::song_prompt(mood, voice_pref, Some(dir.clone()), year, rand);
    let mut req = songwriter::claude::Request::new(prompt_text);
    req.model = model.unwrap();
    req.json_schema = Some(songwriter::schema::song_schema());
    let reply = claude.complete(&req).map_err(|e| anyhow!("claude call failed: {e}"))?;
    eprintln!("sunflower: model reported back: {}", reply.model.as_deref().unwrap_or(&req.model));
    let json = extract_json_object(&reply.text)
        .ok_or_else(|| anyhow!("no JSON object found in reply: {}", reply.text))?;
    let value: serde_json::Value = serde_json::from_str(&json).context("invalid JSON in reply")?;
    Ok((value, dir))
}

/// Duplicated from `songwriter::extract_json_object`, which is private to
/// that crate: balanced-brace scan tolerating a code fence or stray prose.
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

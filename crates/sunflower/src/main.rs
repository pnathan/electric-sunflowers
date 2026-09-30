//! sunflower: command-line renderer for the Singer-Songwriter Bot. See
//! CLAUDE.md at the repo root for what the system is.
//!
//! One pipeline for every command: normalise (printing the repairs),
//! render the stems (`engine::render`, one rayon task graph; set
//! RAYON_NUM_THREADS=1 for a single thread), mix with the band (a mix
//! sidecar next to the output, when present, is folded in), export by
//! the output file's extension. Next to the audio `<stem>.ext` every
//! rendering command writes `<stem>.render.json`
//! (`songwriter::sidecar::RenderSidecar`) and `<stem>.sheet.json`
//! (`engine::SongSheet`). `sheet` prints a song's chord sheet.
//!
//! Settings (docs/features-2.md section 2) are loaded once per run
//! (`settings::load`) and fill in anything a flag left unset: flag beats
//! settings file beats built-in default.

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use engine::{BandPart, MixSettings, Progress, Stems, TrackId};
use export::{BitDepth, Format, Meta};
use song::{Band, Repair, Song, Voice};
use songwriter::claude::{Effort, Transport};
use songwriter::sidecar::RenderSidecar;
use songwriter::usage::Generation;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "sunflower",
    about = "Render songs for the Singer-Songwriter Bot"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// Render settings shared by every rendering command.
#[derive(Args)]
struct RenderArgs {
    /// Song seed; random (and printed) when left out.
    #[arg(long)]
    seed: Option<u64>,
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
    /// for a 44.1 kHz stereo signal). Defaults to settings export.ogg_quality.
    #[arg(long)]
    quality: Option<f32>,
    /// FLAC only: 16-bit output instead of the default 24-bit.
    #[arg(long)]
    flac16: bool,
}

/// Mix and stems settings shared by every rendering command.
#[derive(Args)]
struct MixArgs {
    /// Apply this mix sidecar (see `engine::MixSettings`) instead of the
    /// default `<out-stem>.mix.json`.
    #[arg(long)]
    mix: Option<PathBuf>,
    /// Ignore any mix sidecar, even `<out-stem>.mix.json` if present.
    #[arg(long)]
    no_mix: bool,
    /// Also write `<out-stem>.stems/<track>.flac` (24-bit stereo) for
    /// every audible track plus `reverb.flac`, and `stems.json`. Opt-in:
    /// about 30 MB per track for a 3-minute song.
    #[arg(long)]
    stems: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Render the built-in demo song. Output defaults to song.ogg.
    Demo {
        /// Overwrite an existing <out-stem>.json that is not the demo song.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        export: ExportArgs,
        #[command(flatten)]
        mix: MixArgs,
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
        #[command(flatten)]
        mix: MixArgs,
    },
    /// Write a new song with Claude, then render and mix it. Output
    /// defaults to <title-slug>.ogg, or <title-slug>-2.ogg, -3, ... when
    /// that name is taken.
    Write {
        mood: String,
        /// Overwrite the files of an existing song with the same name
        /// (with -o, or the plain title slug) instead of refusing or
        /// picking a free name.
        #[arg(long)]
        force: bool,
        #[arg(long)]
        style: Option<String>,
        /// cli | api. Defaults to settings claude.transport.
        #[arg(long, value_parser = parse_transport)]
        via: Option<Transport>,
        /// Defaults to settings claude.model.
        #[arg(long)]
        model: Option<String>,
        /// low | medium | high | xhigh | max. Defaults to settings claude.effort.
        #[arg(long, value_parser = parse_effort)]
        effort: Option<Effort>,
        #[command(flatten)]
        render: RenderArgs,
        #[command(flatten)]
        export: ExportArgs,
        #[command(flatten)]
        mix: MixArgs,
    },
    /// List style keys and labels.
    Styles,
    /// Print a song's chord sheet (chords above the lyrics, by section) as
    /// the renderer would sing it with this seed and voice.
    Sheet {
        song: PathBuf,
        #[arg(long)]
        style: Option<String>,
        /// Print the sheet as JSON instead of text.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        render: RenderArgs,
    },
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
        format!(
            "unknown --no track {s:?} (want one of: {})",
            names.join(", ")
        )
    })
}

/// `--via`: `Transport` has no local `ValueEnum` impl (it lives in the
/// `songwriter` crate, which does not depend on clap), so this parses the
/// same names (`cli`, `api`) by hand, the same way `parse_band_part` does.
fn parse_transport(s: &str) -> Result<Transport, String> {
    s.parse::<Transport>().map_err(|e| e.to_string())
}

/// `--effort`: see `parse_transport`.
fn parse_effort(s: &str) -> Result<Effort, String> {
    s.parse::<Effort>().map_err(|e| e.to_string())
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
    let loaded = settings::load();
    for w in &loaded.warnings {
        eprintln!("sunflower: warning: settings: {w}");
    }
    let settings = &loaded.settings;

    match Cli::parse().cmd {
        Cmd::Demo {
            force,
            render,
            export,
            mix,
        } => {
            let song = engine::demo_song();
            let out = export
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from("song.ogg"));
            check_format(&out, &export, settings)?;
            // The demo has no file of its own; save it so the sidecar can name one.
            let json_path = stem_path(&out, "json");
            if !force
                && json_path.exists()
                && std::fs::read_to_string(&json_path).ok().as_deref() != Some(engine::DEMO_JSON)
            {
                bail!("{} exists and is not the demo song; use --force to overwrite it, or -o to name another output", json_path.display());
            }
            std::fs::write(&json_path, engine::DEMO_JSON)
                .with_context(|| format!("saving the demo song JSON to {}", json_path.display()))?;
            pipeline(
                song,
                &render,
                song.band,
                &out,
                &export,
                &mix,
                settings,
                Source {
                    song_json: json_path,
                    model: None,
                    generation: None,
                },
            )
        }
        Cmd::Render {
            song: path,
            style,
            no,
            render,
            export,
            mix,
        } => {
            let out = export
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from("song.ogg"));
            let song = load_song(&path, style.as_deref())?;
            let mut band = song.band;
            for part in no {
                part.switch_off(&mut band);
            }
            pipeline(
                &song,
                &render,
                band,
                &out,
                &export,
                &mix,
                settings,
                Source {
                    song_json: path,
                    model: None,
                    generation: None,
                },
            )
        }
        Cmd::Sheet {
            song: path,
            style,
            json,
            render,
        } => {
            let song = load_song(&path, style.as_deref())?;
            let seed = resolve_seed(render.seed);
            let sheet = sheet_of(&song, seed, render.voice.voice());
            if json {
                println!("{}", serde_json::to_string_pretty(&sheet)?);
            } else {
                print!("{}", sheet.to_text());
            }
            Ok(())
        }
        Cmd::Write {
            mood,
            force,
            style,
            via,
            model,
            effort,
            render,
            export,
            mix,
        } => {
            let via = via.unwrap_or(settings.claude.transport);
            let model = model.unwrap_or_else(|| settings.claude.model.clone());
            let effort = effort.unwrap_or(settings.claude.effort);
            cmd_write(
                &mood,
                force,
                style.as_deref(),
                via,
                &model,
                effort,
                &render,
                &export,
                &mix,
                settings,
            )
        }
        Cmd::Styles => cmd_styles(),
    }
}

/// Reads, normalises and (with `style`) styles the song JSON at `path`.
fn load_song(path: &Path, style: Option<&str>) -> Result<Song> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading song JSON from {}", path.display()))?;
    let raw: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing JSON in {}", path.display()))?;
    let mut song = normalize(&raw, "song")?;
    if let Some(key) = style {
        apply_style(&mut song, key)?;
    }
    Ok(song)
}

/// The sheet of `song` with its style label filled in.
fn sheet_of(song: &Song, seed: u64, voice: Option<Voice>) -> engine::SongSheet {
    let mut sheet = engine::song_sheet(song, seed, voice);
    sheet.style_label = Some(style_label(song)).filter(|l| !l.is_empty());
    sheet
}

/// `out` with its extension replaced by `ext` (`song.ogg` -> `song.<ext>`);
/// `ext` may itself contain dots.
fn stem_path(out: &Path, ext: &str) -> PathBuf {
    out.with_extension(ext)
}

/// Fails, before any work is done, when `out` names no audio format.
fn check_format(out: &Path, export: &ExportArgs, settings: &settings::Settings) -> Result<()> {
    let quality = export.quality.unwrap_or(settings.export.ogg_quality);
    Format::from_path(out, quality, export.flac16, export.float)
        .with_context(|| format!("output {}", out.display()))?;
    Ok(())
}

/// The files a rendering command writes for output `out`: the song JSON,
/// the audio and the sidecars.
fn song_files(out: &Path) -> Vec<PathBuf> {
    let mut v = vec![stem_path(out, "json"), out.to_path_buf()];
    v.extend(["render.json", "sheet.json"].map(|e| stem_path(out, e)));
    v
}

/// `dir/<slug>.ogg` for a new song, with `-2`, `-3`, ... added until none
/// of its files exists (as the studio's `library::fresh_stem` does).
fn fresh_out(dir: &Path, title: &str) -> PathBuf {
    let base = slugify(title);
    (1..)
        .map(|k| {
            if k == 1 {
                base.clone()
            } else {
                format!("{base}-{k}")
            }
        })
        .map(|name| dir.join(format!("{name}.ogg")))
        .find(|out| !song_files(out).iter().any(|p| p.exists()))
        .expect("an unbounded search finds a free name")
}

/// Where a rendered song came from, for the render sidecar.
struct Source {
    song_json: PathBuf,
    /// The model that wrote the song, when known.
    model: Option<String>,
    /// The Claude usage record, when the song was written this run.
    generation: Option<Generation>,
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
    let (song, repairs) =
        song::normalize_value(raw).map_err(|e| anyhow!("{what} failed to normalize: {e}"))?;
    warn_repairs(what, &repairs);
    Ok(song)
}

/// Applies style `key` to `song`, printing the repairs as warnings.
fn apply_style(song: &mut Song, key: &str) -> Result<()> {
    let repairs = songwriter::styles::apply_style(key, song).map_err(|e| anyhow!("{e}"))?;
    warn_repairs(&format!("style {key}"), &repairs);
    Ok(())
}

/// A random 64-bit seed. Not cryptographic: process id xor the clock.
fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    (nanos ^ (pid << 32) ^ 0x9E3779B97F4A7C15) as u64 ^ ((nanos >> 64) as u64)
}

/// The seed from `--seed`, or a random one, printed so the run can be
/// reproduced.
fn resolve_seed(seed: Option<u64>) -> u64 {
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

/// The mix settings to use: `--no-mix` keeps the defaults; `--mix FILE`
/// applies that sidecar; otherwise `<out-stem>.mix.json` is applied when
/// it exists. Returns the settings and the sidecar path used, if any (for
/// the render sidecar's `mix` field).
fn resolve_mix(
    mix_args: &MixArgs,
    out: &Path,
    defaults: &MixSettings,
) -> Result<(MixSettings, Option<PathBuf>)> {
    if mix_args.no_mix {
        return Ok((*defaults, None));
    }
    let path = match &mix_args.mix {
        Some(p) => Some(p.clone()),
        None => Some(stem_path(out, "mix.json")).filter(|p| p.exists()),
    };
    let Some(path) = path else {
        return Ok((*defaults, None));
    };
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading mix settings from {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("parsing JSON in {}", path.display()))?;
    eprintln!("sunflower: applying mix settings from {}", path.display());
    let (settings, warnings) = MixSettings::from_json(&value, defaults);
    for w in &warnings {
        eprintln!("sunflower: warning: mix: {w}");
    }
    Ok((settings, Some(path)))
}

/// The largest absolute sample in `s` (both channels).
fn peak_abs(s: &engine::Stereo) -> f32 {
    s.l.iter()
        .chain(s.r.iter())
        .fold(0.0f32, |m, &x| m.max(x.abs()))
}

/// Writes `<out-stem>.stems/<track>.flac` (24-bit stereo) for every
/// audible track plus `reverb.flac`, and `<out-stem>.stems/stems.json`
/// (design section 3.4). Two passes over the same cheap-to-print stems:
/// pass 1 finds the largest sample across every printed stem and the
/// reverb (each buffer dropped once measured, so only one is ever held);
/// pass 2 reprints and writes, scaled by `extra_gain` when the peak
/// exceeds 1.0. Returns the directory written.
fn write_stems(
    stems: &Stems,
    band: &Band,
    seed: u64,
    settings: &MixSettings,
    out: &Path,
) -> Result<PathBuf> {
    let gain = engine::mix_gain(stems, band, seed, settings);
    let duck = engine::mix::duck_gains(stems, band, settings);
    let audible: Vec<TrackId> = TrackId::ALL
        .into_iter()
        .filter(|&id| id.plays(band) && settings.audible(id))
        .collect();

    // Pass 1: the peak over every stem and the reverb.
    let mut peak = 0.0f32;
    for &id in &audible {
        if let Some(s) = engine::print_stem(stems, band, seed, settings, id, duck.as_deref(), gain)
        {
            peak = peak.max(peak_abs(&s));
        }
    }
    peak = peak.max(peak_abs(&engine::print_reverb(
        stems,
        band,
        seed,
        settings,
        duck.as_deref(),
        gain,
    )));
    let extra_gain = if peak > 1.0 { 0.99 / peak } else { 1.0 };

    // Pass 2: reprint, scale, write.
    let dir = stem_path(out, "stems");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let meta = Meta::default();
    let sr = sfcore::SR as u32;
    let mut names = Vec::with_capacity(audible.len());
    for &id in &audible {
        let Some(mut s) =
            engine::print_stem(stems, band, seed, settings, id, duck.as_deref(), gain)
        else {
            continue;
        };
        scale(&mut s, extra_gain);
        let path = dir.join(format!("{}.flac", id.name()));
        export::write(
            &path,
            &s.l,
            &s.r,
            sr,
            &meta,
            Format::Flac {
                bits: BitDepth::Bits24,
            },
        )
        .with_context(|| format!("writing {}", path.display()))?;
        names.push(id.name().to_string());
    }
    let mut reverb = engine::print_reverb(stems, band, seed, settings, duck.as_deref(), gain);
    scale(&mut reverb, extra_gain);
    let reverb_path = dir.join("reverb.flac");
    export::write(
        &reverb_path,
        &reverb.l,
        &reverb.r,
        sr,
        &meta,
        Format::Flac {
            bits: BitDepth::Bits24,
        },
    )
    .with_context(|| format!("writing {}", reverb_path.display()))?;

    let manifest = serde_json::json!({
        "tracks": names,
        "gain": gain,
        "extra_gain": extra_gain,
        "note": "sum of stems plus reverb equals the mix before the bus compressor",
    });
    let manifest_path = dir.join("stems.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
        .with_context(|| format!("writing {}", manifest_path.display()))?;
    eprintln!("sunflower: wrote {}", dir.display());
    Ok(dir)
}

/// Scales both channels of `s` by `g` in place.
fn scale(s: &mut engine::Stereo, g: f32) {
    if g == 1.0 {
        return;
    }
    for v in s.l.iter_mut().chain(s.r.iter_mut()) {
        *v *= g;
    }
}

/// Renders `song`, mixes the parts `band` turns on (folding in a mix
/// sidecar per `mix_args`) and writes `out` in the format its extension
/// names.
// Each parameter is a distinct CLI input; grouping them into a struct here
// would just move the same count into a constructor call at both sites.
#[allow(clippy::too_many_arguments)]
fn pipeline(
    song: &Song,
    render: &RenderArgs,
    band: Band,
    out: &Path,
    export: &ExportArgs,
    mix_args: &MixArgs,
    settings: &settings::Settings,
    src: Source,
) -> Result<()> {
    let quality = export.quality.unwrap_or(settings.export.ogg_quality);
    let fmt = Format::from_path(out, quality, export.flac16, export.float)?;
    let seed = resolve_seed(render.seed);
    eprintln!(
        "sunflower: rendering on {} threads",
        rayon::current_num_threads()
    );
    let (prepared, stems) = engine::render(song, seed, render.voice.voice(), &Report);
    eprintln!("sunflower: mixing");
    let defaults = MixSettings::default_for(&stems);
    let (mix_settings, mix_path) = resolve_mix(mix_args, out, &defaults)?;

    let stems_dir = if mix_args.stems {
        Some(write_stems(&stems, &band, seed, &mix_settings, out)?)
    } else {
        None
    };

    let m = engine::mix_with(&stems, &band, seed, &mix_settings);
    drop(stems);
    export::write(out, &m.l, &m.r, sfcore::SR as u32, &song_meta(song), fmt)
        .with_context(|| format!("writing audio to {}", out.display()))?;
    eprintln!(
        "sunflower: wrote {} ({:.1} s, 2 channels x {} samples)",
        out.display(),
        m.l.len() as f64 / sfcore::SR_F,
        m.l.len()
    );

    let mut sheet = engine::sheet_from(song, seed, &prepared);
    sheet.style_label = Some(style_label(song)).filter(|l| !l.is_empty());
    write_sidecars(out, &sheet, &src, mix_path.as_deref(), stems_dir.as_deref())
}

/// `p` canonicalised to an absolute path, or its own text when that fails
/// (e.g. it does not exist).
fn abs(p: &Path) -> String {
    std::fs::canonicalize(p)
        .unwrap_or_else(|_| p.to_path_buf())
        .display()
        .to_string()
}

/// Writes `<stem>.sheet.json` and `<stem>.render.json`
/// (`songwriter::sidecar::RenderSidecar`) next to `out`.
fn write_sidecars(
    out: &Path,
    sheet: &engine::SongSheet,
    src: &Source,
    mix_path: Option<&Path>,
    stems_dir: Option<&Path>,
) -> Result<()> {
    let sheet_path = stem_path(out, "sheet.json");
    std::fs::write(&sheet_path, serde_json::to_string_pretty(sheet)?)
        .with_context(|| format!("writing the sheet to {}", sheet_path.display()))?;

    let model = src
        .generation
        .as_ref()
        .map(|g| g.model.clone().unwrap_or_else(|| g.requested_model.clone()))
        .or_else(|| src.model.clone());
    let side = RenderSidecar {
        version: None,
        seed: Some(sheet.seed),
        voice: Some(sheet.voice.as_str().to_string()),
        voice_b: None,
        style: sheet.style.clone(),
        style_label: sheet.style_label.clone(),
        model,
        generation: src.generation.clone(),
        song_json: Some(abs(&src.song_json)),
        audio: Some(abs(out)),
        sheet: Some(abs(&sheet_path)),
        mix: mix_path.map(abs),
        stems: stems_dir.map(abs),
        created: Some(utc_now_iso()),
    };
    let side_path = stem_path(out, "render.json");
    side.write(&side_path).map_err(|e| anyhow!("{e}"))?;
    eprintln!(
        "sunflower: wrote {} and {}",
        side_path.display(),
        sheet_path.display()
    );
    Ok(())
}

/// The style label for `song.style`'s key, if a style was applied.
fn style_label(song: &Song) -> String {
    song.style
        .as_deref()
        .and_then(|key| songwriter::styles::style(key).ok())
        .map(|s| s.label.to_string())
        .unwrap_or_default()
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

// Each parameter is a distinct CLI input; grouping them into a struct here
// would just move the same count into a constructor call at both sites.
#[allow(clippy::too_many_arguments)]
fn cmd_write(
    mood: &str,
    force: bool,
    style: Option<&str>,
    via: Transport,
    model: &str,
    effort: Effort,
    render: &RenderArgs,
    export: &ExportArgs,
    mix_args: &MixArgs,
    settings: &settings::Settings,
) -> Result<()> {
    // Check -o before the (paid) call to Claude, not after it.
    if let Some(out) = &export.out {
        check_format(out, export, settings)?;
        if !force {
            if let Some(p) = song_files(out).into_iter().find(|p| p.exists()) {
                bail!(
                    "{} exists; use --force to overwrite it, or -o to name another output",
                    p.display()
                );
            }
        }
    }
    let seed = resolve_seed(render.seed);
    let render = RenderArgs {
        seed: Some(seed),
        voice: render.voice,
    };
    let claude: Box<dyn songwriter::claude::Claude> = match via {
        Transport::Cli => Box::new(songwriter::claude::ClaudeCli::default()),
        Transport::Api => Box::new(
            songwriter::claude::ClaudeApi::from_env()
                .map_err(|e| anyhow!("could not build the API client: {e}"))?,
        ),
    };
    let year = current_year(); // CLAUDE.md: age counted from 1999 as of the current year.
    let style_id = style
        .filter(|s| !s.trim().eq_ignore_ascii_case("auto"))
        .map(|s| s.parse::<songwriter::styles::StyleId>())
        .transpose()
        .map_err(|e| anyhow!("{e}"))?;
    let mut rng = songwriter::Rng::stream(seed, songwriter::WRITE_TAG);
    let req = songwriter::WriteRequest {
        mood,
        // Flag beats settings: `--voice auto` (the default) falls back to
        // settings songwriter.voice.
        voice: render.voice.voice().or(settings.songwriter.voice),
        style: style_id,
        year,
        model: Some(model.to_string()),
        effort,
    };

    eprintln!("sunflower: asking Claude to write the song ({model} via {via}, effort {effort})");
    let w = songwriter::write_song(claude.as_ref(), &req, &mut rng)
        .map_err(|e| anyhow!("songwriter: {e}"))?;
    let songwriter::Written {
        raw,
        direction: dir,
        generation,
        ..
    } = w;

    // The generation summary (cost, tokens, timing) is printed before the
    // song is validated, so a rejected song still shows what it cost.
    eprintln!("{}", generation.summary());

    // Save the model's reply before validating it, so a rejected song is kept.
    let title = raw.get("title").and_then(|t| t.as_str()).unwrap_or("song");
    let out = match &export.out {
        Some(out) => out.clone(),
        None if force => PathBuf::from(format!("{}.ogg", slugify(title))),
        None => fresh_out(Path::new(""), title),
    };
    let json_path = stem_path(&out, "json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&raw)?)
        .with_context(|| format!("saving raw song JSON to {}", json_path.display()))?;
    eprintln!("sunflower: saved raw song JSON to {}", json_path.display());

    let mut song = normalize(&raw, "written song")?;
    apply_style(&mut song, dir.style.as_str())
        .with_context(|| format!("applying style {:?} to the written song", dir.style))?;

    println!("title: {}", song.title);
    println!("style: {} ({})", dir.style.as_str(), dir.label);
    println!("form: {}", dir.form);
    println!("key: {} {}", song.key, song.mode);
    println!("meter: {}", song.meter);
    println!("tempo: {:.0}", song.tempo_bpm);

    let model_used = generation
        .model
        .clone()
        .unwrap_or_else(|| generation.requested_model.clone());
    let src = Source {
        song_json: json_path,
        model: Some(model_used),
        generation: Some(generation),
    };
    pipeline(
        &song, &render, song.band, &out, export, mix_args, settings, src,
    )
}

/// The current UTC year (CLAUDE.md: the songwriter persona's age is counted
/// from 1999 as of the current year).
fn current_year() -> i32 {
    civil_from_days((now_secs() / 86400) as i64).0
}

/// Seconds since the Unix epoch.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The current UTC time as ISO 8601, `2026-09-28T14:03:07Z`.
fn utc_now_iso() -> String {
    iso_of(now_secs())
}

/// `secs` after the epoch as ISO 8601 UTC.
fn iso_of(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let r = secs % 86400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        r / 3600,
        r / 60 % 60,
        r % 60
    )
}

/// (year, month 1-12, day 1-31) of `days` since 1970-01-01, proleptic
/// Gregorian (Howard Hinnant's civil-from-days), with no date crate.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    ((if m <= 2 { y + 1 } else { y }) as i32, m as u32, d as u32)
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
    fn via_and_effort_parse_known_names() {
        assert!(matches!(parse_transport("cli"), Ok(Transport::Cli)));
        assert!(matches!(parse_transport("api"), Ok(Transport::Api)));
        assert!(parse_transport("bogus").is_err());
        assert!(matches!(parse_effort("high"), Ok(Effort::High)));
        assert!(parse_effort("bogus").is_err());
    }

    #[test]
    fn iso_times() {
        assert_eq!(iso_of(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_of(951_782_400 + 3661), "2000-02-29T01:01:01Z");
        assert_eq!(iso_of(1_790_000_000), "2026-09-21T14:13:20Z");
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("  Dust & Rain! "), "dust-rain");
        assert_eq!(slugify("!!!"), "song");
    }

    #[test]
    fn fresh_out_skips_taken_names() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(fresh_out(d, "Dust"), d.join("dust.ogg"));
        std::fs::write(d.join("dust.json"), "{}").unwrap();
        assert_eq!(fresh_out(d, "Dust"), d.join("dust-2.ogg"));
        std::fs::write(d.join("dust-2.render.json"), "{}").unwrap();
        assert_eq!(fresh_out(d, "Dust"), d.join("dust-3.ogg"));
        assert_eq!(fresh_out(Path::new(""), "Dust"), PathBuf::from("dust.ogg"));
    }
}

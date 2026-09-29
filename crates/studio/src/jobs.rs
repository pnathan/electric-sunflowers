//! Work off the UI thread: loading a song (normalise, style, compose, sheet,
//! score), rendering its audio, and writing a new song with Claude.
//!
//! A `Job` runs on its own thread, reports a stage and a fraction through a
//! shared `Status`, and hands back one result.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use eframe::egui;
use export::{Format, Meta};
use song::{Song, Voice};

use crate::library::{self, Entry, Source};

/// What a running job is doing.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub stage: String,
    /// 0..=1, or `None` while the length is unknown (a Claude call).
    pub frac: Option<f32>,
}

/// Status shared between a job thread and the UI.
#[derive(Clone)]
pub struct Reporter {
    status: Arc<Mutex<Status>>,
    ctx: egui::Context,
}

impl Reporter {
    pub fn set(&self, stage: &str, frac: Option<f32>) {
        if let Ok(mut s) = self.status.lock() {
            s.stage = stage.to_string();
            s.frac = frac;
        }
        self.ctx.request_repaint();
    }
}

/// A background job with one result.
pub struct Job<T> {
    rx: Receiver<Result<T, String>>,
    status: Arc<Mutex<Status>>,
    started: std::time::Instant,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(ctx: &egui::Context, stage: &str, f: impl FnOnce(&Reporter) -> Result<T, String> + Send + 'static) -> Job<T> {
        let status = Arc::new(Mutex::new(Status { stage: stage.to_string(), frac: None }));
        let rep = Reporter { status: status.clone(), ctx: ctx.clone() };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&rep)))
                .unwrap_or_else(|p| Err(format!("internal error: {}", panic_text(&p))));
            let _ = tx.send(r);
            rep.ctx.request_repaint();
        });
        Job { rx, status, started: std::time::Instant::now() }
    }

    /// The result, once; `None` while running.
    pub fn poll(&self) -> Option<Result<T, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the job thread stopped without a result".into())),
        }
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

/// A song ready to show: the normalised, styled song and everything drawn
/// from one `compose::prepare` with its seed and voice.
pub struct Loaded {
    pub entry: Entry,
    pub song: Song,
    pub seed: u64,
    pub voice: Option<Voice>,
    /// The seed came from a render sidecar (or the audio is yet to be made
    /// with it), so the sheet matches the audio.
    pub seed_known: bool,
    pub sheet: engine::SongSheet,
    pub score: notation::Score,
    /// The multipart score, for the Full score view.
    pub full: notation::full::FullScore,
    /// Normaliser repairs and style problems, for the user.
    pub notes: Vec<String>,
}

/// The seed used when a song has no sidecar.
pub const DEFAULT_SEED: u64 = 1;

/// Reads the song an entry names, normalised, with the sidecar's style.
fn read_song(entry: &Entry, notes: &mut Vec<String>) -> Result<Song, String> {
    let mut song = match &entry.source {
        Source::Demo => engine::demo_song().clone(),
        Source::File(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: not JSON: {e}", p.display()))?;
            let (song, repairs) = song::normalize_value(&raw).map_err(|e| format!("{}: {e}", p.display()))?;
            notes.extend(repairs.iter().map(|r| format!("repair: {r}")));
            song
        }
    };
    if let Some(key) = entry.render.as_ref().and_then(|r| r.style.as_deref()) {
        match songwriter::styles::apply_style(key, &mut song) {
            Ok(rep) => notes.extend(rep.iter().map(|r| format!("style {key}: {r}"))),
            Err(e) => notes.push(format!("style {key} not applied: {e}")),
        }
    }
    Ok(song)
}

/// Loads an entry (runs on a job thread).
pub fn load(entry: Entry) -> Result<Loaded, String> {
    let mut notes = Vec::new();
    if let Some(e) = &entry.render_error {
        notes.push(format!("render sidecar ignored: {e}"));
    }
    let song = read_song(&entry, &mut notes)?;
    let side_seed = entry.render.as_ref().and_then(|r| r.seed);
    let seed = side_seed.unwrap_or(DEFAULT_SEED);
    let voice = entry.render.as_ref().and_then(|r| r.voice);
    let seed_known = side_seed.is_some() || entry.audio.is_none();
    let prep = compose::prepare::prepare(&song, seed, voice);
    let mut sheet = engine::sheet_from(&song, seed, &prep);
    sheet.style_label = style_label(&song);
    let score = notation::Score::new(&song, &prep);
    let arr = arrange::arrange(&song, &prep, seed);
    let full = notation::full::FullScore::new(&song, &prep, &arr);
    Ok(Loaded { entry, song, seed, voice, seed_known, sheet, score, full, notes })
}

/// The label of the song's applied style.
pub fn style_label(song: &Song) -> Option<String> {
    song.style.as_deref().and_then(|k| songwriter::styles::style(k).ok()).map(|s| s.label.to_string())
}

/// Where a render goes and what its sidecar records.
pub struct RenderPlan {
    pub song: Song,
    pub seed: u64,
    pub voice: Option<Voice>,
    /// `<stem>.ogg`, `<stem>.render.json` and `<stem>.sheet.json` are written.
    pub stem: PathBuf,
    /// The song JSON the sidecar names.
    pub song_json: PathBuf,
    pub model: Option<String>,
}

struct Progress<'a>(&'a Reporter);

impl engine::Progress for Progress<'_> {
    fn advance(&self, done: usize, total: usize) {
        let f = done as f32 / total.max(1) as f32;
        self.0.set(&format!("Rendering ({done}/{total} tracks)"), Some(0.05 + 0.8 * f));
    }
}

/// A finished render: the audio file, and the stems it was mixed from,
/// kept for the mixer.
pub struct Rendered {
    pub audio: PathBuf,
    pub stems: Arc<engine::Stems>,
}

/// Renders, mixes and exports the plan's song to `<stem>.ogg`, then writes
/// the sidecars. The mix applies `<stem>.mix.json` when it exists, as
/// `sunflower render` does.
pub fn render(plan: &RenderPlan, rep: &Reporter) -> Result<Rendered, String> {
    if let Some(dir) = plan.stem.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let out = with_ext(&plan.stem, "ogg");
    rep.set("Composing", Some(0.02));
    let (prepared, stems) = engine::render(&plan.song, plan.seed, plan.voice, &Progress(rep));
    rep.set("Mixing", Some(0.87));
    let mix_path = mix_path(&plan.stem);
    let file = read_mix(&mix_path).unwrap_or_else(|e| {
        eprintln!("studio: warning: {e}; using the default mix");
        None
    });
    let (settings, warnings) = settings_from(file.as_ref(), &stems);
    for w in &warnings {
        eprintln!("studio: warning: mix: {w}");
    }
    let m = engine::mix_with(&stems, &plan.song.band, plan.seed, &settings);
    rep.set("Encoding Ogg Vorbis", Some(0.93));
    let tmp = with_ext(&plan.stem, "ogg.part");
    export::write(&tmp, &m.l, &m.r, sfcore::SR as u32, &song_meta(&plan.song), Format::Ogg { quality: 0.6 })
        .map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &out).map_err(|e| format!("moving {} to {}: {e}", tmp.display(), out.display()))?;
    rep.set("Writing sidecars", Some(0.98));
    let mut sheet = engine::sheet_from(&plan.song, plan.seed, &prepared);
    sheet.style_label = style_label(&plan.song);
    let applied = file.is_some().then_some(mix_path.as_path());
    write_sidecars(plan, &out, &sheet, applied)?;
    rep.set("Done", Some(1.0));
    Ok(Rendered { audio: out, stems: Arc::new(stems) })
}

/// Renders the stems only, for the mixer of a song whose audio was made
/// earlier: the same song, seed and voice give the same stems. Writes
/// nothing.
pub fn stems(song: &Song, seed: u64, voice: Option<Voice>, rep: &Reporter) -> Arc<engine::Stems> {
    rep.set("Loading stems for the mixer", Some(0.02));
    let (_, stems) = engine::render(song, seed, voice, &Progress(rep));
    rep.set("Done", Some(1.0));
    Arc::new(stems)
}

/// A re-mix ready to play.
pub struct Remixed {
    pub buf: rodio::buffer::SamplesBuffer,
    /// Seconds the mix took, for the log.
    pub mix_s: f64,
}

/// Mixes `stems` with `settings` into a playable buffer.
pub fn remix(stems: &engine::Stems, band: &song::Band, seed: u64, settings: &engine::MixSettings) -> Remixed {
    let t = std::time::Instant::now();
    let m = engine::mix_with(stems, band, seed, settings);
    let mix_s = t.elapsed().as_secs_f64();
    Remixed { buf: crate::audio::stereo_buffer(&m.l, &m.r, sfcore::SR as u32), mix_s }
}

/// `<stem>.mix.json`.
pub fn mix_path(stem: &Path) -> PathBuf {
    with_ext(stem, "mix.json")
}

/// Reads a mix sidecar: `None` when there is none, an error when it
/// exists but is not JSON.
pub fn read_mix(path: &Path) -> Result<Option<serde_json::Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{}: not JSON: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// The mix settings a sidecar value gives against the defaults for
/// `stems`; the defaults when there is no sidecar.
pub fn settings_from(file: Option<&serde_json::Value>, stems: &engine::Stems) -> (engine::MixSettings, Vec<String>) {
    let defaults = engine::MixSettings::default_for(stems);
    match file {
        Some(v) => engine::MixSettings::from_json(v, &defaults),
        None => (defaults, Vec::new()),
    }
}

/// Whether a mix sidecar value changes anything: `to_json` writes only
/// the fields that differ from the defaults.
pub fn mix_changes_anything(v: &serde_json::Value) -> bool {
    v.as_object().is_some_and(|o| o.keys().any(|k| k != "version"))
}

/// Writes `settings` to `path` as a mix sidecar (only what differs from
/// `defaults`), through a `.tmp` file and a rename, so a reader never sees
/// half a file.
pub fn write_mix(path: &Path, settings: &engine::MixSettings, defaults: &engine::MixSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&settings.to_json(defaults)).map_err(|e| e.to_string())?;
    let tmp = with_ext(path, "tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn with_ext(stem: &Path, ext: &str) -> PathBuf {
    let mut s = stem.as_os_str().to_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

fn write_sidecars(plan: &RenderPlan, out: &Path, sheet: &engine::SongSheet, mix: Option<&Path>) -> Result<(), String> {
    let sheet_path = with_ext(&plan.stem, "sheet.json");
    let text = serde_json::to_string_pretty(sheet).map_err(|e| e.to_string())?;
    std::fs::write(&sheet_path, text).map_err(|e| format!("{}: {e}", sheet_path.display()))?;
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).display().to_string();
    let side = serde_json::json!({
        "seed": plan.seed,
        "voice": sheet.voice.as_str(),
        "style": sheet.style,
        "style_label": sheet.style_label,
        "model": plan.model,
        "song_json": abs(&plan.song_json),
        "audio": abs(out),
        "sheet": abs(&sheet_path),
        "mix": mix.map(abs),
        "created": iso_of(now_secs()),
    });
    let side_path = with_ext(&plan.stem, "render.json");
    let text = serde_json::to_string_pretty(&side).map_err(|e| e.to_string())?;
    std::fs::write(&side_path, text).map_err(|e| format!("{}: {e}", side_path.display()))
}

fn song_meta(song: &Song) -> Meta {
    Meta {
        title: song.title.clone(),
        artist: String::new(),
        comment: song.note.clone(),
        date: civil_from_days((now_secs() / 86400) as i64).0.to_string(),
        style: style_label(song).unwrap_or_default(),
    }
}

/// How to reach Claude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Cli,
    Api,
}

/// A request for a new song.
#[derive(Clone, Debug)]
pub struct WritePlan {
    pub mood: String,
    pub style: Option<songwriter::styles::StyleId>,
    pub voice: Option<Voice>,
    pub via: Via,
    pub model: Option<String>,
    pub dir: PathBuf,
    pub seed: u64,
}

/// A written song: its saved JSON, its stem, and its render.
pub struct Written {
    pub json: PathBuf,
    pub stem: PathBuf,
    pub rendered: Rendered,
}

/// Writes a song with Claude, saves its JSON in the library, renders it.
pub fn write(plan: &WritePlan, rep: &Reporter) -> Result<Written, String> {
    let claude: Box<dyn songwriter::claude::Claude> = match plan.via {
        Via::Cli => Box::new(songwriter::claude::ClaudeCli::default()),
        Via::Api => Box::new(songwriter::claude::ClaudeApi::from_env().map_err(|e| format!("API client: {e}"))?),
    };
    write_with(claude.as_ref(), plan, rep)
}

/// `write` with a given Claude client.
pub fn write_with(claude: &dyn songwriter::claude::Claude, plan: &WritePlan, rep: &Reporter) -> Result<Written, String> {
    std::fs::create_dir_all(&plan.dir).map_err(|e| format!("{}: {e}", plan.dir.display()))?;
    let year = civil_from_days((now_secs() / 86400) as i64).0;
    let mut rng = songwriter::Rng::stream(plan.seed, songwriter::WRITE_TAG);
    let req = songwriter::WriteRequest {
        mood: &plan.mood,
        voice: plan.voice,
        style: plan.style,
        year,
        model: plan.model.clone(),
        effort: Default::default(),
    };
    rep.set("Claude is writing the song", None);
    let w = songwriter::write_song(claude, &req, &mut rng).map_err(|e| format!("songwriter: {e}"))?;
    let title = w.raw.get("title").and_then(|t| t.as_str()).unwrap_or("song").to_string();
    let stem = library::fresh_stem(&plan.dir, &title);
    let json_path = with_ext(&stem, "json");
    let text = serde_json::to_string_pretty(&w.raw).map_err(|e| e.to_string())?;
    std::fs::write(&json_path, text).map_err(|e| format!("{}: {e}", json_path.display()))?;
    let (mut song, _) = song::normalize_value(&w.raw).map_err(|e| format!("the written song failed to normalise ({e}); saved as {}", json_path.display()))?;
    let key = w.direction.style.as_str();
    songwriter::styles::apply_style(key, &mut song).map_err(|e| format!("style {key}: {e}"))?;
    let plan = RenderPlan {
        song,
        seed: plan.seed,
        voice: plan.voice,
        stem: stem.clone(),
        song_json: json_path.clone(),
        model: w.model.or_else(|| plan.model.clone()),
    };
    let rendered = render(&plan, rep)?;
    Ok(Written { json: json_path, stem, rendered })
}

/// A random 64-bit seed from the clock and the process id (not
/// cryptographic).
pub fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let pid = std::process::id() as u128;
    (nanos ^ (pid << 32) ^ 0x9E37_79B9_7F4A_7C15) as u64 ^ ((nanos >> 64) as u64)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `secs` after the epoch as ISO 8601 UTC.
fn iso_of(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86400) as i64);
    let r = secs % 86400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", r / 3600, r / 60 % 60, r % 60)
}

/// (year, month, day) of `days` after 1970-01-01, proleptic Gregorian
/// (Hinnant's civil-from-days).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    ((if m <= 2 { y + 1 } else { y }) as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times() {
        assert_eq!(iso_of(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_of(951_782_400 + 3661), "2000-02-29T01:01:01Z");
    }

    #[test]
    fn demo_loads_with_matching_sheet_and_score() {
        let dir = std::env::temp_dir().join(format!("studio-load-{}", std::process::id()));
        let l = load(Entry::demo(&dir)).unwrap();
        assert!(l.seed_known);
        assert_eq!(l.seed, DEFAULT_SEED);
        let sung: usize = l.sheet.sections.iter().flat_map(|s| &s.lines).map(|l| l.syllables.len()).sum();
        assert!(sung > 50);
        assert!(!notation::note_boxes(&l.score).is_empty());
    }

    struct Canned(String);

    impl songwriter::claude::Claude for Canned {
        fn complete(&self, _: &songwriter::claude::Request) -> Result<songwriter::claude::Reply, songwriter::claude::ClaudeError> {
            Ok(songwriter::claude::Reply {
                model: Some("canned".into()),
                ..songwriter::claude::Reply::text_only(self.0.clone())
            })
        }
    }

    fn reporter() -> Reporter {
        Reporter { status: Arc::new(Mutex::new(Status::default())), ctx: egui::Context::default() }
    }

    #[test]
    fn write_saves_renders_and_lists_a_song() {
        let dir = std::env::temp_dir().join(format!("studio-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plan = WritePlan { mood: "x".into(), style: None, voice: Some(Voice::Alto), via: Via::Cli, model: None, dir: dir.clone(), seed: 5 };
        let reply = format!("Here it is:\n```json\n{}\n```", engine::DEMO_JSON);
        let written = write_with(&Canned(reply), &plan, &reporter()).unwrap();
        assert_eq!(written.json, dir.join("every-harbor.json"));
        // The render hands back its stems for the mixer.
        assert!(written.rendered.stems.get(engine::TrackId::Lead).is_some());
        for ext in ["ogg", "render.json", "sheet.json"] {
            assert!(dir.join(format!("every-harbor.{ext}")).is_file(), "{ext}");
        }
        let v = library::scan(&dir).unwrap();
        let e = v.iter().find(|e| e.name == "every-harbor").unwrap();
        let r = e.render.as_ref().unwrap();
        assert_eq!((r.seed, r.voice, r.model.as_deref()), (Some(5), Some(Voice::Alto), Some("canned")));
        assert!(r.style.is_some());
        // The loaded sheet uses the render's seed, voice and style.
        let l = load(e.clone()).unwrap();
        assert!(l.seed_known);
        assert_eq!(l.sheet.voice, Voice::Alto);
        assert_eq!(l.sheet.style, r.style);
        // A reply with no JSON is an error, and nothing is saved.
        assert!(write_with(&Canned("no song today".into()), &plan, &reporter()).is_err());
        assert_eq!(library::scan(&dir).unwrap().len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn render_applies_the_mix_sidecar_and_keeps_its_stems() {
        use rodio::Source;
        let dir = std::env::temp_dir().join(format!("studio-mix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let stem = dir.join("demo");
        let song_json = dir.join("demo.json");
        std::fs::write(&song_json, engine::DEMO_JSON).unwrap();
        std::fs::write(mix_path(&stem), r#"{"version": 1, "tracks": {"drums": {"mute": true}}, "duck_db": 2.0}"#).unwrap();
        let (song, _) = song::normalize_value(&serde_json::from_str(engine::DEMO_JSON).unwrap()).unwrap();
        let plan = RenderPlan { song: song.clone(), seed: 3, voice: None, stem: stem.clone(), song_json, model: None };
        let r = render(&plan, &reporter()).unwrap();
        let side: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("demo.render.json")).unwrap()).unwrap();
        assert!(side["mix"].as_str().is_some_and(|m| m.ends_with("demo.mix.json")), "{side}");

        let file = read_mix(&mix_path(&stem)).unwrap();
        let (settings, warn) = settings_from(file.as_ref(), &r.stems);
        assert!(warn.is_empty(), "{warn:?}");
        assert!(settings.tracks[engine::TrackId::Drums.index()].mute);
        assert_eq!(settings.duck_db, 2.0);
        let m = remix(&r.stems, &song.band, 3, &settings);
        eprintln!("mix_with of the demo: {:.2} s", m.mix_s);
        let want = r.stems.len as f64 / sfcore::SR as f64;
        let got = m.buf.total_duration().unwrap().as_secs_f64();
        assert!((got - want).abs() < 1e-3, "{got} vs {want}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_song_file_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("studio-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.json");
        std::fs::write(&p, "{\"title\": 3}").unwrap();
        assert!(load(Entry::for_song(&p)).is_err());
        std::fs::write(&p, "nope").unwrap();
        assert!(load(Entry::for_song(&p)).is_err());
        assert!(load(Entry::for_song(&dir.join("missing.json"))).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

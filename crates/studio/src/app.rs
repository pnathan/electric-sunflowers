//! The studio window: library, views, transport, new-song form, and the
//! scripted run used for screenshots.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, Key, RichText};
use resvg::usvg;
use song::Voice;

use crate::audio::{Output, Track};
use crate::jobs::{self, Job, Loaded, RenderPlan, Via, WritePlan};
use crate::library::{self, Entry, Source};
use crate::lyrics::{clock, LyricsView};
use crate::mixer::{self, MixerPanel, StemCache};
use crate::settings_panel::SettingsForm;
use crate::sheetview::SheetView;

/// Which views the central panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Lyrics,
    Sheet,
    Both,
}

/// Which sheet the sheet-music view is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SheetKind {
    Lead,
    Full,
}

fn sheet_of(kind: SheetKind, l: &Loaded) -> notation::Sheet {
    match kind {
        SheetKind::Lead => notation::Sheet::Lead(l.score.clone()),
        SheetKind::Full => notation::Sheet::Full(l.full.clone()),
    }
}

impl View {
    pub fn parse(s: &str) -> Option<View> {
        match s {
            "lyrics" => Some(View::Lyrics),
            "sheet" => Some(View::Sheet),
            "both" => Some(View::Both),
            _ => None,
        }
    }
}

/// Command-line options.
#[derive(Clone, Debug)]
pub struct Options {
    pub dir: PathBuf,
    /// A song JSON to open at start.
    pub song: Option<PathBuf>,
    /// A library entry (by name) to open at start.
    pub open: Option<String>,
    pub view: View,
    /// Scripted run: once the song is ready, seek, play this long, save a
    /// screenshot and quit.
    pub screenshot: Option<PathBuf>,
    pub seek: Option<f64>,
    pub play: f64,
    pub volume: f32,
    /// Open the new-song form at start.
    pub new_song: bool,
    /// Open the mixer at start.
    pub mixer: bool,
}

/// A render or a write running in the background.
enum Work {
    Render { job: Job<jobs::Rendered>, stem: PathBuf, seed: u64, select: bool },
    Write { job: Job<jobs::Written>, seed: u64 },
}

/// The result of a finished `Work`.
enum Finished {
    Rendered(jobs::Rendered),
    Written(jobs::Written),
}

/// A mixer job: stems for the song at `stem`, or a re-mix of them.
struct MixJob<T> {
    stem: PathBuf,
    seed: u64,
    job: Job<T>,
}

/// The new-song form.
struct NewSong {
    open: bool,
    mood: String,
    /// Style key; empty is songwriter's choice.
    style: String,
    voice: Option<Voice>,
    via: Via,
    model: String,
    seed: String,
}

/// The "Render a new take" form: a chosen seed and voice, rendered
/// beside the current song without replacing it.
struct TakeForm {
    open: bool,
    /// Blank is a random seed.
    seed: String,
    /// `None` keeps the song's current voice.
    voice: Option<Voice>,
}

/// Steps of the scripted run.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    WaitReady,
    Playing { until: f64, next_log: f64 },
    Settle(u32),
    Shot,
    Done,
}

pub struct StudioApp {
    opt: Options,
    svg_opt: Arc<usvg::Options<'static>>,
    entries: Vec<Entry>,
    selected: Option<usize>,
    load: Option<Job<Loaded>>,
    loaded: Option<Loaded>,
    sheet: Option<SheetView>,
    sheet_kind: SheetKind,
    lyrics: LyricsView,
    work: Option<Work>,
    /// Stems a render has been tried for, so a failure is not retried in a loop.
    tried: Vec<PathBuf>,
    output: Option<Result<Output, String>>,
    track: Option<Track>,
    audio_error: Option<String>,
    scrub: Option<f64>,
    view: View,
    errors: Vec<String>,
    info: Option<String>,
    new_song: NewSong,
    take: TakeForm,
    settings: settings::Settings,
    settings_form: SettingsForm,
    mixer: MixerPanel,
    /// The stems of the open song, once a render or a stems job made them.
    stems: Option<StemCache>,
    stems_job: Option<MixJob<Arc<engine::Stems>>>,
    remix_job: Option<MixJob<jobs::Remixed>>,
    step: Step,
    start: std::time::Instant,
}

pub(crate) fn voices() -> [Voice; 5] {
    [Voice::Bass, Voice::Baritone, Voice::Tenor, Voice::Alto, Voice::Soprano]
}

fn via_of(t: songwriter::claude::Transport) -> Via {
    match t {
        songwriter::claude::Transport::Cli => Via::Cli,
        songwriter::claude::Transport::Api => Via::Api,
    }
}

impl StudioApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opt: Options) -> StudioApp {
        let mut svg = usvg::Options::default();
        svg.fontdb_mut().load_system_fonts();
        let loaded_settings = settings::load();
        for w in &loaded_settings.warnings {
            eprintln!("studio: warning: settings: {w}");
        }
        let settings = loaded_settings.settings;
        let new_song = NewSong {
            open: opt.new_song,
            mood: String::new(),
            style: String::new(),
            voice: settings.songwriter.voice,
            via: via_of(settings.claude.transport),
            model: settings.claude.model.clone(),
            seed: String::new(),
        };
        let mut app = StudioApp {
            view: opt.view,
            svg_opt: Arc::new(svg),
            entries: Vec::new(),
            selected: None,
            load: None,
            loaded: None,
            sheet: None,
            sheet_kind: SheetKind::Lead,
            lyrics: LyricsView::new(),
            work: None,
            tried: Vec::new(),
            output: None,
            track: None,
            audio_error: None,
            scrub: None,
            errors: Vec::new(),
            info: None,
            new_song,
            take: TakeForm { open: false, seed: String::new(), voice: None },
            settings,
            settings_form: SettingsForm::new(),
            mixer: MixerPanel::new(),
            stems: None,
            stems_job: None,
            remix_job: None,
            step: if opt.screenshot.is_some() || opt.play > 0.0 || opt.seek.is_some() { Step::WaitReady } else { Step::Done },
            start: std::time::Instant::now(),
            opt,
        };
        app.mixer.open = app.opt.mixer;
        if let Err(e) = std::fs::create_dir_all(&app.opt.dir) {
            app.errors.push(format!("cannot create the library {}: {e}", app.opt.dir.display()));
        }
        app.rescan();
        let ctx = cc.egui_ctx.clone();
        if let Some(p) = app.opt.song.clone() {
            let i = app.find_or_add(&p);
            app.select(&ctx, i);
        } else if let Some(name) = app.opt.open.clone() {
            match app.entries.iter().position(|e| e.name == name || e.stem.file_name().is_some_and(|f| f.to_string_lossy() == name)) {
                Some(i) => app.select(&ctx, i),
                None => app.errors.push(format!("no song named {name:?} in {}", app.opt.dir.display())),
            }
        }
        app
    }

    fn rescan(&mut self) {
        let keep = self.selected.and_then(|i| self.entries.get(i)).map(|e| e.stem.clone());
        let extra: Vec<Entry> = self.entries.iter().filter(|e| e.stem.parent() != Some(self.opt.dir.as_path()) && e.source != Source::Demo).cloned().collect();
        match library::scan(&self.opt.dir) {
            Ok(v) => self.entries = v,
            Err(e) => {
                self.errors.push(format!("library: {e}"));
                self.entries = vec![Entry::demo(&self.opt.dir)];
            }
        }
        for mut e in extra {
            e.refresh();
            self.entries.push(e);
        }
        self.selected = keep.and_then(|s| self.entries.iter().position(|e| e.stem == s));
    }

    /// The index of the entry for a song JSON, adding it when it is outside the library.
    fn find_or_add(&mut self, path: &Path) -> usize {
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let c = canon(path);
        if let Some(i) = self.entries.iter().position(|e| canon(&e.stem.with_extension("json")) == c || canon(&e.sibling("json")) == c) {
            return i;
        }
        self.entries.push(Entry::for_song(path));
        self.entries.len() - 1
    }

    fn select(&mut self, ctx: &egui::Context, i: usize) {
        let Some(entry) = self.entries.get(i).cloned() else { return };
        self.selected = Some(i);
        self.flush_mix();
        self.track = None;
        self.audio_error = None;
        self.loaded = None;
        self.sheet = None;
        self.info = None;
        self.lyrics = LyricsView::new();
        // Stems are large (hundreds of MB): keep them only for this song.
        if self.stems.as_ref().is_some_and(|c| c.stem != entry.stem) {
            self.stems = None;
        }
        self.mixer.reset_for(&entry.stem, entry.audio.as_deref());
        eprintln!("studio: opening {}", entry.name);
        self.load = Some(Job::spawn(ctx, "Composing the melody", move |_| jobs::load(entry)));
    }

    fn open_audio(&mut self) {
        let Some(l) = &self.loaded else { return };
        let Some(path) = l.entry.audio.clone() else { return };
        if self.output.is_none() {
            self.output = Some(Output::open());
        }
        match self.output.as_ref() {
            Some(Ok(out)) => match Track::open(out, &path, l.sheet.duration_s, self.opt.volume) {
                Ok(t) => {
                    eprintln!("studio: audio {} ({:.1} s)", path.display(), t.duration);
                    self.track = Some(t);
                    self.audio_error = None;
                }
                Err(e) => self.audio_error = Some(e),
            },
            Some(Err(e)) => self.audio_error = Some(e.clone()),
            None => {}
        }
    }

    /// Starts rendering the loaded song. `take` renders to a new stem
    /// beside it instead of the entry's own, using `seed`/`voice` when
    /// given (else the song's own).
    fn start_render_as(&mut self, ctx: &egui::Context, take: bool, seed: Option<u64>, voice: Option<Option<Voice>>) {
        if self.work.is_some() {
            return;
        }
        let Some(l) = &self.loaded else { return };
        let seed = seed.unwrap_or(l.seed);
        let voice = voice.unwrap_or(l.voice);
        let (stem, song_json) = match &l.entry.source {
            Source::Demo => {
                let json = l.entry.sibling("json");
                if let Some(d) = json.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                if let Err(e) = std::fs::write(&json, engine::DEMO_JSON) {
                    self.errors.push(format!("{}: {e}", json.display()));
                    return;
                }
                (l.entry.stem.clone(), json)
            }
            Source::File(p) => {
                let stem = if take {
                    let dir = l.entry.stem.parent().map(Path::to_path_buf).unwrap_or_else(|| self.opt.dir.clone());
                    library::fresh_stem(&dir, &format!("{}-seed{}", l.entry.name, seed))
                } else {
                    l.entry.stem.clone()
                };
                (stem, p.clone())
            }
        };
        let plan = RenderPlan { song: l.song.clone(), seed, voice, stem: stem.clone(), song_json, model: None };
        // The render reads <stem>.mix.json; make it current first.
        self.flush_mix();
        self.tried.push(stem.clone());
        eprintln!("studio: rendering {} (seed {})", stem.display(), plan.seed);
        let job = Job::spawn(ctx, "Rendering", move |rep| jobs::render(&plan, rep));
        self.work = Some(Work::Render { job, stem, seed, select: take });
    }

    /// Starts rendering the loaded song. `take` renders to a new stem
    /// beside it instead of the entry's own.
    fn start_render(&mut self, ctx: &egui::Context, take: bool) {
        self.start_render_as(ctx, take, None, None);
    }

    fn start_write(&mut self, ctx: &egui::Context) {
        if self.work.is_some() {
            return;
        }
        let f = &self.new_song;
        let seed = if f.seed.trim().is_empty() {
            jobs::random_seed()
        } else {
            match f.seed.trim().parse() {
                Ok(s) => s,
                Err(_) => {
                    self.errors.push(format!("seed {:?} is not a whole number", f.seed));
                    return;
                }
            }
        };
        let style = if f.style.is_empty() { None } else { f.style.parse::<songwriter::styles::StyleId>().ok() };
        let plan = WritePlan {
            mood: f.mood.trim().to_string(),
            style,
            voice: f.voice,
            via: f.via,
            model: Some(f.model.trim().to_string()).filter(|m| !m.is_empty()),
            dir: self.opt.dir.clone(),
            seed,
        };
        eprintln!("studio: writing a song (seed {seed})");
        self.work = Some(Work::Write { job: Job::spawn(ctx, "Claude is writing the song", move |rep| jobs::write(&plan, rep)), seed });
        self.new_song.open = false;
    }

    /// Collects finished jobs.
    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.load.as_ref().and_then(|j| j.poll()) {
            self.load = None;
            match r {
                Ok(l) => {
                    eprintln!("studio: loaded {} (seed {}, {} sections, {:.1} s)", l.entry.name, l.seed, l.sheet.sections.len(), l.sheet.duration_s);
                    for n in &l.notes {
                        eprintln!("studio: {n}");
                    }
                    self.sheet = Some(SheetView::new(sheet_of(self.sheet_kind, &l), self.svg_opt.clone()));
                    // With the seed unknown, stems would be another take:
                    // play the file until the user opens the mixer.
                    if !l.seed_known {
                        self.mixer.auto_load = false;
                    }
                    self.loaded = Some(l);
                    self.open_audio();
                }
                Err(e) => self.errors.push(format!("could not open the song: {e}")),
            }
        }
        // A song without audio gets rendered once.
        if let Some(l) = &self.loaded {
            if l.entry.audio.is_none() && self.work.is_none() && !self.tried.contains(&l.entry.stem) {
                self.start_render(ctx, false);
            }
        }
        let done = match &self.work {
            Some(Work::Render { job, .. }) => job.poll().map(|r| r.map(Finished::Rendered)),
            Some(Work::Write { job, .. }) => job.poll().map(|r| r.map(Finished::Written)),
            None => None,
        };
        if let Some(r) = done {
            let work = self.work.take();
            match (work, r) {
                (Some(Work::Render { stem, seed, select, job }), Ok(Finished::Rendered(rendered))) => {
                    eprintln!("studio: rendered {} in {:.1} s", rendered.audio.display(), job.elapsed());
                    if select || self.loaded.as_ref().is_some_and(|l| l.entry.stem == stem) {
                        self.stems = Some(StemCache { stem: stem.clone(), seed, stems: rendered.stems });
                    }
                    self.rescan();
                    if select {
                        if let Some(i) = self.entries.iter().position(|e| e.stem == stem) {
                            self.select(ctx, i);
                        }
                    } else if let Some(l) = self.loaded.as_mut().filter(|l| l.entry.stem == stem) {
                        l.entry.refresh();
                        l.seed_known = true;
                        self.open_audio();
                        // The file holds the mix the sidecar had when the
                        // render began; edits made since need a re-mix.
                        let had = self.mixer.attached();
                        self.attach_stems();
                        if had {
                            self.mixer.touch_now_due(std::time::Instant::now());
                        }
                    }
                }
                (Some(Work::Write { job, seed }), Ok(Finished::Written(w))) => {
                    eprintln!("studio: wrote {} in {:.1} s", w.json.display(), job.elapsed());
                    self.stems = Some(StemCache { stem: w.stem, seed, stems: w.rendered.stems });
                    self.rescan();
                    let i = self.find_or_add(&w.json);
                    self.select(ctx, i);
                }
                (Some(Work::Render { .. }), Err(e)) => self.errors.push(format!("render failed: {e}")),
                (Some(Work::Write { .. }), Err(e)) => self.errors.push(format!("new song failed: {e}")),
                (_, Ok(_)) | (None, Err(_)) => {}
            }
        }
        self.poll_mixer(ctx);
    }

    /// Hands cached stems for the loaded song to the mixer window.
    /// Returns what `MixerPanel::attach` returns, or `None` without stems.
    fn attach_stems(&mut self) -> Option<bool> {
        let l = self.loaded.as_ref()?;
        let c = self.stems.as_ref().filter(|c| c.stem == l.entry.stem)?;
        if self.mixer.stem() != Some(l.entry.stem.as_path()) {
            return None;
        }
        Some(self.mixer.attach(&c.stems, &l.song.band))
    }

    /// Writes pending mixer edits to `<stem>.mix.json` now.
    fn flush_mix(&mut self) {
        if self.mixer.dirty() {
            if let Err(e) = self.mixer.save() {
                self.errors.push(format!("could not save the mix: {e}"));
            }
        }
    }

    /// The mixer's jobs: loads stems when the window (or a saved mix)
    /// needs them, starts a re-mix once edits settle, and plays each
    /// finished re-mix.
    fn poll_mixer(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        // Finished stems.
        if let Some(r) = self.stems_job.as_ref().and_then(|j| j.job.poll()) {
            let MixJob { stem, seed, job } = self.stems_job.take().expect("polled");
            let current = self.loaded.as_ref().is_some_and(|l| l.entry.stem == stem);
            match r {
                Ok(stems) if current => {
                    eprintln!("studio: stems for {} in {:.1} s", stem.display(), job.elapsed());
                    self.stems = Some(StemCache { stem, seed, stems });
                    // Play the sidecar's mix: the file may not hold it.
                    if self.attach_stems() == Some(true) {
                        self.mixer.touch_now_due(now);
                    }
                }
                Ok(_) => {}
                Err(e) if current => {
                    self.mixer.stems_failed = true;
                    self.mixer.auto_load = false;
                    self.errors.push(format!("could not load stems for the mixer: {e}"));
                }
                Err(_) => {}
            }
        }
        // Finished re-mix.
        if let Some(r) = self.remix_job.as_ref().and_then(|j| j.job.poll()) {
            let MixJob { stem, job, .. } = self.remix_job.take().expect("polled");
            let current = self.loaded.as_ref().is_some_and(|l| l.entry.stem == stem);
            match r {
                Ok(m) if current => {
                    eprintln!("studio: re-mix in {:.2} s (mix {:.2} s)", job.elapsed(), m.mix_s);
                    self.play_remix(m.buf);
                }
                Ok(_) => {}
                Err(e) => self.errors.push(format!("re-mix failed: {e}")),
            }
        }
        let Some(l) = &self.loaded else { return };
        let have = self.stems.as_ref().is_some_and(|c| c.stem == l.entry.stem);
        if have && !self.mixer.attached() {
            self.attach_stems();
        }
        let Some(l) = &self.loaded else { return };
        // Stems for the window, or for a saved mix to play.
        let want = (self.mixer.open || self.mixer.auto_load) && !self.mixer.stems_failed;
        if want && !have && self.stems_job.is_none() && self.work.is_none() {
            let (song, seed, voice, stem) = (l.song.clone(), l.seed, l.voice, l.entry.stem.clone());
            eprintln!("studio: loading stems for {} (seed {seed})", stem.display());
            let job = Job::spawn(ctx, "Loading stems for the mixer", move |rep| Ok(jobs::stems(&song, seed, voice, rep)));
            self.stems_job = Some(MixJob { stem, seed, job });
            return;
        }
        // A re-mix once the edits have settled; one at a time.
        if !have || self.remix_job.is_some() {
            return;
        }
        let Some(settings) = self.mixer.take_due(now) else { return };
        if let Err(e) = self.mixer.save() {
            self.errors.push(format!("could not save the mix: {e}"));
        }
        let c = self.stems.as_ref().expect("have");
        let (stems, seed, stem, band) = (c.stems.clone(), c.seed, c.stem.clone(), l.song.band);
        let job = Job::spawn(ctx, "Re-mixing", move |_| Ok(jobs::remix(&stems, &band, seed, &settings)));
        self.remix_job = Some(MixJob { stem, seed, job });
    }

    /// Swaps playback to a re-mix at the current position, keeping play
    /// or pause.
    fn play_remix(&mut self, buf: rodio::buffer::SamplesBuffer) {
        if self.output.is_none() {
            self.output = Some(Output::open());
        }
        let out = match self.output.as_ref() {
            Some(Ok(out)) => out,
            Some(Err(e)) => {
                self.audio_error = Some(e.clone());
                return;
            }
            None => return,
        };
        match Track::take_over(out, self.track.as_ref(), buf, self.opt.volume) {
            Ok(t) => {
                eprintln!("studio: playing the re-mix from {:.3} s (playing={})", t.position(), t.playing());
                // The old player stops when dropped here, after the new one runs.
                self.track = Some(t);
                self.audio_error = None;
            }
            Err(e) => self.errors.push(format!("could not play the re-mix: {e}")),
        }
    }

    fn pos(&self) -> f64 {
        self.scrub.or_else(|| self.track.as_ref().map(Track::position)).unwrap_or(0.0)
    }

    fn playing(&self) -> bool {
        self.track.as_ref().is_some_and(Track::playing)
    }

    fn seek(&mut self, t: f64) {
        if let Some(tr) = self.track.as_mut() {
            if let Err(e) = tr.seek(t) {
                self.errors.push(e);
            }
        }
    }

    fn toggle(&mut self) {
        if let Some(tr) = self.track.as_mut() {
            if let Err(e) = tr.toggle() {
                self.errors.push(e);
            }
        }
    }

    fn library_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.heading("Songs");
            if ui.small_button("Rescan").clicked() {
                self.rescan();
            }
        });
        ui.label(RichText::new(self.opt.dir.display().to_string()).small().weak());
        ui.separator();
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("library").show(ui, |ui| {
            for (i, e) in self.entries.iter().enumerate() {
                let sel = self.selected == Some(i);
                let r = ui.selectable_label(sel, RichText::new(&e.name).strong());
                let mut sub = Vec::new();
                sub.push(if e.audio.is_some() { "ogg".to_string() } else { "no audio".to_string() });
                match &e.render {
                    Some(r) => {
                        if let Some(s) = r.seed {
                            sub.push(format!("seed {s}"));
                        }
                        if let Some(v) = r.voice {
                            sub.push(v.as_str().to_string());
                        }
                        if let Some(s) = &r.style {
                            sub.push(s.clone());
                        }
                    }
                    None if e.audio.is_some() && e.source != Source::Demo => sub.push("seed unknown".into()),
                    None => {}
                }
                ui.label(RichText::new(sub.join(", ")).small().weak());
                ui.add_space(3.0);
                if r.clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            let ctx = ui.ctx().clone();
            self.select(&ctx, i);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let busy_msg = match &self.work {
                Some(Work::Write { .. }) => Some("Claude is writing a song"),
                Some(Work::Render { .. }) => Some("A render is running"),
                None => None,
            };
            if ui.add_enabled(busy_msg.is_none(), egui::Button::new("New song...")).on_disabled_hover_text(busy_msg.unwrap_or_default()).clicked() {
                self.new_song.voice = self.settings.songwriter.voice;
                self.new_song.via = via_of(self.settings.claude.transport);
                self.new_song.model = self.settings.claude.model.clone();
                self.new_song.open = true;
            }
            ui.separator();
            if ui.add_enabled(self.loaded.is_some() && busy_msg.is_none(), egui::Button::new("Render a new take...")).clicked() {
                self.take.seed.clear();
                self.take.voice = None;
                self.take.open = true;
            }
            ui.separator();
            if ui.button("Settings...").clicked() {
                self.settings_form.open_from(&self.settings);
            }
            if ui.add_enabled(self.loaded.is_some(), egui::Button::new("Mixer...")).clicked() {
                self.mixer.open = true;
            }
            ui.separator();
            ui.selectable_value(&mut self.view, View::Lyrics, "Lyrics & chords");
            ui.selectable_value(&mut self.view, View::Sheet, "Sheet music");
            ui.selectable_value(&mut self.view, View::Both, "Both");
            ui.separator();
            if let Some(s) = self.sheet.as_mut() {
                if self.view != View::Lyrics {
                    ui.label("Zoom");
                    ui.add(egui::Slider::new(&mut s.zoom, 0.5..=2.5).fixed_decimals(2));
                    ui.separator();
                    let before = self.sheet_kind;
                    ui.selectable_value(&mut self.sheet_kind, SheetKind::Lead, "Lead sheet");
                    ui.selectable_value(&mut self.sheet_kind, SheetKind::Full, "Full score");
                    if self.sheet_kind != before {
                        if let Some(l) = &self.loaded {
                            s.set_sheet(sheet_of(self.sheet_kind, l));
                        }
                    }
                }
            }
            ui.checkbox(&mut self.lyrics.follow, "Follow");
            if let Some(s) = self.sheet.as_mut() {
                s.follow = self.lyrics.follow;
            }
        });
    }

    fn transport(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        // Jobs.
        let status = match &self.work {
            Some(Work::Render { job, .. }) => Some((job.status(), job.elapsed())),
            Some(Work::Write { job, .. }) => Some((job.status(), job.elapsed())),
            None => None,
        };
        if let Some((st, el)) = status {
            ui.horizontal(|ui| {
                ui.spinner();
                let text = format!("{}  ({:.0} s)", st.stage, el);
                match st.frac {
                    Some(f) => {
                        ui.add(egui::ProgressBar::new(f).text(text).desired_width(ui.available_width().min(600.0)));
                    }
                    None => {
                        ui.label(text);
                    }
                }
            });
        }
        if let Some(j) = &self.load {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(j.status().stage);
            });
        }
        if let Some(j) = &self.stems_job {
            ui.horizontal(|ui| {
                ui.spinner();
                let st = j.job.status();
                ui.label(format!("{}  ({:.0} s)", st.stage, j.job.elapsed()));
            });
        }
        let mut dismiss = None;
        for (i, e) in self.errors.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("x").clicked() {
                    dismiss = Some(i);
                }
                ui.colored_label(Color32::from_rgb(230, 80, 70), e);
            });
        }
        if let Some(i) = dismiss {
            self.errors.remove(i);
        }

        ui.horizontal(|ui| {
            let has = self.track.is_some();
            let playing = self.playing();
            let label = if playing { "Pause" } else { "Play" };
            if ui.add_enabled(has, egui::Button::new(RichText::new(label).size(16.0)).min_size(egui::vec2(70.0, 28.0))).clicked() {
                self.toggle();
            }
            let dur = self.track.as_ref().map(|t| t.duration).or(self.loaded.as_ref().map(|l| l.sheet.duration_s)).unwrap_or(0.0);
            let pos = self.pos();
            ui.label(RichText::new(format!("{} / {}", clock(pos), clock(dur))).monospace());
            let vol_w = 160.0;
            ui.spacing_mut().slider_width = (ui.available_width() - vol_w - 90.0).max(80.0);
            let mut p = pos;
            let r = ui.add_enabled(has, egui::Slider::new(&mut p, 0.0..=dur.max(0.01)).show_value(false));
            if r.dragged() {
                self.scrub = Some(p);
            } else if r.drag_stopped() || r.changed() {
                self.scrub = None;
                self.seek(p);
            }
            if !r.dragged() && !r.drag_stopped() {
                self.scrub = None;
            }
            ui.spacing_mut().slider_width = vol_w - 40.0;
            ui.label("Vol");
            if ui.add(egui::Slider::new(&mut self.opt.volume, 0.0..=1.0).show_value(false)).changed() {
                if let Some(t) = &self.track {
                    t.set_volume(self.opt.volume);
                }
            }
        });
        if let Some(e) = &self.audio_error {
            ui.colored_label(Color32::from_rgb(230, 80, 70), e);
        }
        ui.add_space(4.0);
    }

    fn banner(&mut self, ui: &mut egui::Ui) {
        let Some(l) = &self.loaded else { return };
        let mut rerender = false;
        if !l.seed_known {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    Color32::from_rgb(230, 170, 40),
                    format!("No render sidecar: the seed of this audio is unknown. Sheet and lyrics use seed {}; notes and timing may not match what you hear.", l.seed),
                );
                if ui.add_enabled(self.work.is_none(), egui::Button::new(format!("Render a new take with seed {}", l.seed))).clicked() {
                    rerender = true;
                }
            });
        }
        if !l.notes.is_empty() {
            egui::CollapsingHeader::new(RichText::new(format!("{} normaliser notes", l.notes.len())).small()).id_salt("notes").show(ui, |ui| {
                for n in &l.notes {
                    ui.label(RichText::new(n).small());
                }
            });
        }
        if let Some(g) = l.entry.render.as_ref().and_then(|r| r.generation.as_ref()) {
            ui.label(RichText::new(g.summary()).small().weak());
        }
        if l.entry.audio.is_none() && self.track.is_none() && self.work.is_none() && self.tried.contains(&l.entry.stem) {
            ui.horizontal(|ui| {
                ui.label("No audio.");
                if ui.button("Render audio").clicked() {
                    self.tried.retain(|s| *s != l.entry.stem);
                }
            });
        }
        if let Some(i) = &self.info {
            ui.label(i);
        }
        if rerender {
            let ctx = ui.ctx().clone();
            self.start_render(&ctx, true);
        }
    }

    fn central(&mut self, ui: &mut egui::Ui) {
        self.banner(ui);
        let Some(l) = &self.loaded else {
            ui.centered_and_justified(|ui| {
                if self.load.is_some() {
                    ui.spinner();
                } else {
                    ui.label("Pick a song on the left, or write a new one.");
                }
            });
            return;
        };
        let pos = self.pos();
        let playing = self.playing();
        let sheet = &l.sheet;
        let mut seek = None;
        match self.view {
            View::Lyrics => seek = self.lyrics.ui(ui, sheet, pos, playing),
            View::Sheet => {
                if let Some(s) = self.sheet.as_mut() {
                    seek = s.ui(ui, pos, playing);
                }
            }
            View::Both => {
                let lyrics = &mut self.lyrics;
                let sv = &mut self.sheet;
                let full = ui.available_rect_before_wrap();
                let wl = (full.width() * 0.38).max(260.0).min(full.width() - 200.0).max(0.0);
                let left = egui::Rect::from_min_size(full.min, egui::vec2(wl, full.height()));
                let right = egui::Rect::from_min_max(egui::pos2(full.left() + wl + 12.0, full.top()), full.max);
                let down = egui::Layout::top_down(egui::Align::Min);
                ui.scope_builder(egui::UiBuilder::new().max_rect(left).layout(down), |ui| {
                    if let Some(t) = lyrics.ui(ui, sheet, pos, playing) {
                        seek = Some(t);
                    }
                });
                let x = full.left() + wl + 6.0;
                ui.painter().vline(x, full.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
                ui.scope_builder(egui::UiBuilder::new().max_rect(right).layout(down), |ui| {
                    if let Some(s) = sv.as_mut() {
                        if let Some(t) = s.ui(ui, pos, playing) {
                            seek = Some(t);
                        }
                    }
                });
            }
        }
        if let Some(t) = seek {
            self.seek(t);
        }
    }

    fn new_song_window(&mut self, ctx: &egui::Context) {
        if !self.new_song.open {
            return;
        }
        let mut open = true;
        let mut submit = false;
        egui::Window::new("New song").open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
            let f = &mut self.new_song;
            ui.label("What should the song be about? (mood, subject, a line, a place)");
            ui.add(egui::TextEdit::multiline(&mut f.mood).desired_rows(4).desired_width(f32::INFINITY));
            egui::Grid::new("new-song-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Style");
                let cur = if f.style.is_empty() {
                    "Songwriter's choice".to_string()
                } else {
                    songwriter::styles::style(&f.style).map(|s| s.label.to_string()).unwrap_or_else(|_| f.style.clone())
                };
                egui::ComboBox::from_id_salt("style").selected_text(cur).width(280.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.style, String::new(), "Songwriter's choice");
                    for (key, s) in songwriter::styles::styles() {
                        ui.selectable_value(&mut f.style, key.to_string(), s.label);
                    }
                });
                ui.end_row();
                ui.label("Voice");
                egui::ComboBox::from_id_salt("voice").selected_text(f.voice.map(|v| v.label()).unwrap_or("Songwriter's choice")).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.voice, None, "Songwriter's choice");
                    for v in voices() {
                        ui.selectable_value(&mut f.voice, Some(v), v.label());
                    }
                });
                ui.end_row();
                ui.label("Claude via");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.via, Via::Cli, "claude CLI (logged-in account)");
                    ui.radio_value(&mut f.via, Via::Api, "API (ANTHROPIC_API_KEY)");
                });
                ui.end_row();
                ui.label("Model");
                ui.add(egui::TextEdit::singleline(&mut f.model).hint_text("default").desired_width(220.0));
                ui.end_row();
                ui.label("Seed");
                ui.add(egui::TextEdit::singleline(&mut f.seed).hint_text("random").desired_width(220.0));
                ui.end_row();
            });
            if f.via == Via::Api && std::env::var_os("ANTHROPIC_API_KEY").is_none() {
                ui.colored_label(Color32::from_rgb(230, 170, 40), "ANTHROPIC_API_KEY is not set.");
            }
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Saved to {}. Writing takes a minute or more; rendering about 20 s.", self.opt.dir.display())).small().weak());
            let ok = !f.mood.trim().is_empty();
            if ui.add_enabled(ok, egui::Button::new("Write and render")).clicked() {
                submit = true;
            }
        });
        if !open {
            self.new_song.open = false;
        }
        if submit {
            self.start_write(ctx);
        }
    }

    fn take_window(&mut self, ctx: &egui::Context) {
        if !self.take.open {
            return;
        }
        let mut open = true;
        let mut submit = false;
        egui::Window::new("Render a new take").open(&mut open).collapsible(false).default_width(360.0).show(ctx, |ui| {
            let f = &mut self.take;
            egui::Grid::new("take-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Seed");
                ui.add(egui::TextEdit::singleline(&mut f.seed).hint_text("random").desired_width(220.0));
                ui.end_row();
                ui.label("Voice");
                egui::ComboBox::from_id_salt("take-voice").selected_text(f.voice.map(|v| v.label()).unwrap_or("Same as now")).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.voice, None, "Same as now");
                    for v in voices() {
                        ui.selectable_value(&mut f.voice, Some(v), v.label());
                    }
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Renders to a new stem beside this song; the current one is untouched.").small().weak());
            if ui.button("Render").clicked() {
                submit = true;
            }
        });
        if !open {
            self.take.open = false;
        }
        if submit {
            self.take.open = false;
            let seed = if self.take.seed.trim().is_empty() {
                jobs::random_seed()
            } else {
                match self.take.seed.trim().parse() {
                    Ok(s) => s,
                    Err(_) => {
                        self.errors.push(format!("seed {:?} is not a whole number", self.take.seed));
                        return;
                    }
                }
            };
            let voice = self.take.voice.map(Some);
            self.start_render_as(ctx, true, Some(seed), voice);
        }
    }

    fn mixer_window(&mut self, ctx: &egui::Context) {
        if !self.mixer.open {
            return;
        }
        let Some(l) = &self.loaded else {
            let info = mixer::Info { status: mixer::Status::NoStems, lead_voice: "", seed_known: true };
            self.mixer.ui(ctx, &info);
            return;
        };
        let have = self.stems.as_ref().is_some_and(|c| c.stem == l.entry.stem);
        let status = if have {
            mixer::Status::Ready { remixing: self.remix_job.is_some() }
        } else if let Some(j) = &self.stems_job {
            let st = j.job.status();
            mixer::Status::Loading { stage: st.stage, frac: st.frac }
        } else if matches!(&self.work, Some(Work::Render { stem, .. }) if *stem == l.entry.stem) {
            mixer::Status::WaitRender
        } else if self.mixer.stems_failed {
            mixer::Status::NoStems
        } else {
            // `poll_mixer` starts the stems job on the next frame, or once
            // the running render or write ends.
            ctx.request_repaint();
            let stage = if self.work.is_some() { "Waiting for the running job" } else { "Starting" };
            mixer::Status::Loading { stage: stage.into(), frac: None }
        };
        let info = mixer::Info { status, lead_voice: l.sheet.voice.label(), seed_known: l.seed_known };
        if self.mixer.ui(ctx, &info) == mixer::Ask::RetryStems {
            self.mixer.stems_failed = false;
        }
    }

    /// Advances the scripted run.
    fn script(&mut self, ctx: &egui::Context) {
        let t = self.start.elapsed().as_secs_f64();
        if t > 900.0 && self.step != Step::Done {
            eprintln!("studio: scripted run timed out");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            self.step = Step::Done;
            return;
        }
        match self.step {
            Step::WaitReady => {
                // With no song to open (--new and no SONG.json), the form
                // itself is what the screenshot should show.
                let ready = if self.loaded.is_none() && self.opt.song.is_none() && self.opt.open.is_none() {
                    self.new_song.open
                } else {
                    let sheet_ready = self.view == View::Lyrics || self.sheet.as_ref().is_some_and(SheetView::ready);
                    let audio_ready = self.track.is_some() || self.audio_error.is_some();
                    // Mixer work (stems for an open mixer or a saved mix,
                    // then its re-mix) is done.
                    let mixer_idle = self.stems_job.is_none() && self.remix_job.is_none() && !self.mixer.dirty() && !self.mixer.auto_load;
                    let mixer_ready = mixer_idle && (!self.mixer.open || self.mixer.attached() || self.mixer.stems_failed);
                    self.loaded.is_some() && sheet_ready && audio_ready && self.work.is_none() && mixer_ready
                };
                if ready {
                    if let Some(s) = self.opt.seek {
                        self.seek(s);
                        eprintln!("studio: seek to {s:.2} s -> position {:.3} s", self.pos());
                    }
                    if self.opt.play > 0.0 && self.track.is_some() {
                        self.toggle();
                        eprintln!("studio: play at {:.3} s (playing={})", self.pos(), self.playing());
                        self.step = Step::Playing { until: t + self.opt.play, next_log: t + 0.5 };
                    } else {
                        self.step = Step::Settle(10);
                    }
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Step::Playing { until, next_log } => {
                if t >= next_log {
                    eprintln!("studio: t={:.2} s wall, position {:.3} s, playing={}", t, self.pos(), self.playing());
                    self.step = Step::Playing { until, next_log: next_log + 0.5 };
                }
                if t >= until {
                    self.step = Step::Settle(10);
                }
                ctx.request_repaint();
            }
            Step::Settle(n) => {
                self.step = if n == 0 {
                    if self.opt.screenshot.is_some() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                        Step::Shot
                    } else {
                        Step::Done
                    }
                } else {
                    Step::Settle(n - 1)
                };
                ctx.request_repaint();
            }
            Step::Shot => {
                let img = ctx.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
                });
                if let Some(img) = img {
                    if let Some(p) = &self.opt.screenshot {
                        match save_png(&img, p) {
                            Ok(()) => eprintln!("studio: screenshot {} ({}x{}), position {:.3} s", p.display(), img.size[0], img.size[1], self.pos()),
                            Err(e) => eprintln!("studio: screenshot failed: {e}"),
                        }
                    }
                    self.step = Step::Done;
                    self.track = None;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ctx.request_repaint();
            }
            Step::Done => {}
        }
    }
}

impl Drop for StudioApp {
    /// Saves mixer edits made within the last `mixer::DEBOUNCE`.
    fn drop(&mut self) {
        if self.mixer.dirty() {
            if let Err(e) = self.mixer.save() {
                eprintln!("studio: could not save the mix: {e}");
            }
        }
    }
}

fn save_png(img: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let [w, h] = img.size;
    let mut data = Vec::with_capacity(w * h * 4);
    for c in &img.pixels {
        data.extend_from_slice(&[c.r(), c.g(), c.b(), 255]);
    }
    let size = resvg::tiny_skia::IntSize::from_wh(w as u32, h as u32).ok_or("empty screenshot")?;
    let pm = resvg::tiny_skia::Pixmap::from_vec(data, size).ok_or("bad screenshot size")?;
    pm.save_png(path).map_err(|e| format!("{}: {e}", path.display()))
}

impl eframe::App for StudioApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(Key::Space)) {
            self.toggle();
        }
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(2.0);
        });
        egui::Panel::bottom("transport").show(ui, |ui| self.transport(ui));
        egui::Panel::left("library").default_size(240.0).resizable(true).show(ui, |ui| self.library_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.central(ui));
        self.new_song_window(&ctx);
        self.take_window(&ctx);
        if let Some(s) = self.settings_form.ui(&ctx) {
            self.settings = s;
        }
        self.mixer_window(&ctx);
        self.script(&ctx);
        if self.playing() || self.work.is_some() || self.load.is_some() || self.stems_job.is_some() || self.remix_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        } else if self.mixer.dirty() {
            // Wake up when the debounce runs out.
            ctx.request_repaint_after(mixer::DEBOUNCE);
        }
    }
}

//! The mixer window: per-track fader, pan, mute and solo, and the ducking
//! depth, over `engine::MixSettings` (design features-2 section 3.5).
//!
//! The window edits settings only. The app owns the work: it keeps the
//! stems of the open song (`StemCache`), starts a re-mix job once the
//! settings have been still for `DEBOUNCE`, writes `<stem>.mix.json`, and
//! swaps playback to the new mix at the current position.
//!
//! A song opened with a `<stem>.mix.json` that changes anything and is
//! newer than its audio file (so the file does not hold that mix) loads
//! its stems at once and plays the saved mix, without the window being
//! opened: what plays is what the sidecar says. When the audio file is
//! newer, the render that made it applied the sidecar already, and the
//! file plays as it is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Color32, RichText};
use engine::mixset::{DUCK_DB_MIN, GAIN_DB_MIN, PAN_MAX, PAN_MIN};
use engine::{MixSettings, Stems, TrackId};
use serde_json::Value;

use crate::jobs;

/// Stillness after the last edit before a re-mix starts.
pub const DEBOUNCE: Duration = Duration::from_millis(250);
/// Fader travel, dB. The bottom of the travel is -inf (the engine's floor).
pub const FADER_MIN_DB: f32 = -60.0;
pub const FADER_MAX_DB: f32 = 6.0;
/// Ducking travel, dB.
pub const DUCK_UI_MAX_DB: f32 = 10.0;

/// The stems of one render of one song, kept for re-mixing.
pub struct StemCache {
    /// The entry stem the stems belong to.
    pub stem: PathBuf,
    /// The seed they were rendered with; the re-mix's reverb uses it too.
    pub seed: u64,
    pub stems: Arc<Stems>,
}

/// What the app is doing for the window, for its status line.
pub enum Status {
    /// No stems yet, and nothing loading them.
    NoStems,
    /// A render for this song is running and will bring its stems.
    WaitRender,
    Loading { stage: String, frac: Option<f32> },
    Ready { remixing: bool },
}

/// What the window shows besides the settings.
pub struct Info<'a> {
    pub status: Status,
    /// The lead's voice, for the lead's label.
    pub lead_voice: &'a str,
    /// False when the song's seed is unknown: the stems then do not match
    /// the audio file.
    pub seed_known: bool,
}

/// What the user asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Nothing,
    /// Try to load the stems again after a failure.
    RetryStems,
}

/// Display name of a track.
pub fn track_label(id: TrackId) -> &'static str {
    match id {
        TrackId::Lead => "Lead",
        TrackId::Doubles => "Doubles",
        TrackId::Harmony => "Harmony",
        TrackId::Choir => "Choir",
        TrackId::Guitar => "Guitar",
        TrackId::HarmonyGuitar => "Harmony guitar",
        TrackId::Bass => "Bass",
        TrackId::Drums => "Drums",
        TrackId::Harp => "Harp",
        TrackId::Violin => "Violin",
    }
}

/// True once `DEBOUNCE` has passed since the last edit.
pub fn due(dirty_since: Option<Instant>, now: Instant) -> bool {
    dirty_since.is_some_and(|t| now.saturating_duration_since(t) >= DEBOUNCE)
}

/// The fader value a slider position gives: the bottom of the travel is
/// the engine's floor (-inf in practice).
pub fn fader_from_slider(v: f32) -> f32 {
    if v <= FADER_MIN_DB {
        GAIN_DB_MIN
    } else {
        v.min(FADER_MAX_DB)
    }
}

/// The slider position for a fader value.
pub fn slider_from_fader(db: f32) -> f32 {
    db.clamp(FADER_MIN_DB, FADER_MAX_DB)
}

/// Whether a song opened with a mix sidecar should play the re-mix: the
/// sidecar changes something and is newer than the audio file (an unknown
/// time counts as newer, so the saved mix wins).
pub fn sidecar_needs_remix(file: Option<&Value>, mix_time: Option<SystemTime>, audio_time: Option<SystemTime>) -> bool {
    let changes = file.is_some_and(jobs::mix_changes_anything);
    let newer = match (mix_time, audio_time) {
        (Some(m), Some(a)) => m > a,
        _ => true,
    };
    changes && newer
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The settings once the stems are known.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Live {
    settings: MixSettings,
    defaults: MixSettings,
}

pub struct MixerPanel {
    pub open: bool,
    /// The entry stem this state is for.
    stem: Option<PathBuf>,
    /// `<stem>.mix.json` as read when the song opened.
    file: Option<Value>,
    live: Option<Live>,
    /// Tracks with a stem that the band plays.
    present: [bool; engine::track::N_TRACKS],
    dirty_since: Option<Instant>,
    /// Load the stems without the window open (see the module doc).
    pub auto_load: bool,
    /// Sidecar warnings and errors, and failures of the mixer's jobs.
    pub notes: Vec<String>,
    /// The stems of this song failed to load; do not retry by itself.
    pub stems_failed: bool,
}

impl MixerPanel {
    pub fn new() -> MixerPanel {
        MixerPanel {
            open: false,
            stem: None,
            file: None,
            live: None,
            present: [false; engine::track::N_TRACKS],
            dirty_since: None,
            auto_load: false,
            notes: Vec::new(),
            stems_failed: false,
        }
    }

    /// The entry stem the state is for.
    pub fn stem(&self) -> Option<&Path> {
        self.stem.as_deref()
    }

    /// Starts over for the song at `stem` (its audio at `audio`): reads
    /// `<stem>.mix.json` and decides whether to load stems at once.
    pub fn reset_for(&mut self, stem: &Path, audio: Option<&Path>) {
        let open = self.open;
        *self = MixerPanel::new();
        self.open = open;
        self.stem = Some(stem.to_path_buf());
        let path = jobs::mix_path(stem);
        match jobs::read_mix(&path) {
            Ok(f) => self.file = f,
            Err(e) => self.notes.push(format!("{e}; using the default mix")),
        }
        // No audio: the render that makes it applies the sidecar.
        self.auto_load = audio.is_some() && sidecar_needs_remix(self.file.as_ref(), mtime(&path), audio.and_then(mtime));
    }

    /// Takes in the song's stems. The first time, the settings come from
    /// the sidecar (else the defaults). Returns true when the settings
    /// differ from the defaults, so the audio file may not match them.
    pub fn attach(&mut self, stems: &Stems, band: &song::Band) -> bool {
        for id in TrackId::ALL {
            self.present[id.index()] = stems.get(id).is_some() && id.plays(band);
        }
        let live = match self.live {
            Some(l) => l,
            None => {
                let defaults = MixSettings::default_for(stems);
                let (settings, warn) = jobs::settings_from(self.file.as_ref(), stems);
                self.notes.extend(warn.into_iter().map(|w| format!("mix sidecar: {w}")));
                let l = Live { settings, defaults };
                self.live = Some(l);
                l
            }
        };
        self.auto_load = false;
        self.stems_failed = false;
        live.settings != live.defaults
    }

    pub fn attached(&self) -> bool {
        self.live.is_some()
    }

    #[cfg(test)]
    fn settings(&self) -> Option<MixSettings> {
        self.live.map(|l| l.settings)
    }

    pub fn dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    /// Marks the settings changed at `now`.
    pub fn touch(&mut self, now: Instant) {
        self.dirty_since = Some(now);
    }

    /// Marks the settings changed long enough ago to re-mix at once.
    pub fn touch_now_due(&mut self, now: Instant) {
        self.dirty_since = Some(now.checked_sub(DEBOUNCE).unwrap_or(now));
    }

    /// Once the settings have been still for `DEBOUNCE`: clears the mark
    /// and returns the settings to re-mix with.
    pub fn take_due(&mut self, now: Instant) -> Option<MixSettings> {
        let l = self.live?;
        if !due(self.dirty_since, now) {
            return None;
        }
        self.dirty_since = None;
        Some(l.settings)
    }

    /// Writes the settings to `<stem>.mix.json`.
    pub fn save(&self) -> Result<(), String> {
        let (Some(stem), Some(l)) = (&self.stem, self.live) else { return Ok(()) };
        jobs::write_mix(&jobs::mix_path(stem), &l.settings, &l.defaults)
    }

    /// Draws the window. Edits mark the settings changed.
    pub fn ui(&mut self, ctx: &egui::Context, info: &Info) -> Ask {
        if !self.open {
            return Ask::Nothing;
        }
        let mut open = true;
        let mut ask = Ask::Nothing;
        let mut changed = false;
        egui::Window::new("Mixer").open(&mut open).collapsible(true).resizable(false).default_width(560.0).show(ctx, |ui| {
            match &info.status {
                Status::NoStems if self.stems_failed => {
                    ui.horizontal(|ui| {
                        ui.label("The stems did not load.");
                        if ui.button("Retry").clicked() {
                            ask = Ask::RetryStems;
                        }
                    });
                }
                Status::NoStems => {
                    ui.label("Open a song to mix it.");
                }
                Status::WaitRender => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Waiting for the running render; the mixer uses its stems.");
                    });
                }
                Status::Loading { stage, frac } => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        match frac {
                            Some(f) => ui.add(egui::ProgressBar::new(*f).text(stage.as_str()).desired_width(360.0)),
                            None => ui.label(stage.as_str()),
                        };
                    });
                    ui.label(RichText::new("The mixer renders the song's stems once (same seed and voice as the audio).").small().weak());
                }
                Status::Ready { remixing } => {
                    ui.horizontal(|ui| {
                        if *remixing || self.dirty_since.is_some() {
                            ui.spinner();
                            ui.label(RichText::new("Re-mixing").small());
                        } else {
                            ui.label(RichText::new("Playing this mix").small().weak());
                        }
                    });
                }
            }
            if !info.seed_known && self.live.is_some() {
                ui.colored_label(Color32::from_rgb(230, 170, 40), "The seed of the audio file is unknown: the mixer's stems use the sheet's seed and will not sound the same as the file.");
            }
            if let Some(live) = self.live.as_mut() {
                changed |= strips(ui, live, &self.present, info.lead_voice);
            }
            for n in &self.notes {
                ui.label(RichText::new(n).small().color(Color32::from_rgb(230, 170, 40)));
            }
            if let Some(stem) = &self.stem {
                ui.label(RichText::new(format!("Saved to {} on each change.", jobs::mix_path(stem).display())).small().weak());
            }
        });
        if !open {
            self.open = false;
        }
        if changed {
            self.touch(Instant::now());
        }
        ask
    }
}

/// The track strips, the ducking slider and Reset. Returns true on an edit.
fn strips(ui: &mut egui::Ui, live: &mut Live, present: &[bool], lead_voice: &str) -> bool {
    let mut changed = false;
    egui::Grid::new("mixer-grid").num_columns(5).spacing([10.0, 6.0]).striped(true).show(ui, |ui| {
        ui.label(RichText::new("Track").strong());
        ui.label(RichText::new("Fader").strong());
        ui.label(RichText::new("Pan").strong());
        ui.label(RichText::new("Mute").strong());
        ui.label(RichText::new("Solo").strong());
        ui.end_row();
        for id in TrackId::ALL.into_iter().filter(|id| present[id.index()]) {
            let t = &mut live.settings.tracks[id.index()];
            let label = if id == TrackId::Lead { format!("Lead ({lead_voice})") } else { track_label(id).to_string() };
            ui.label(label);
            let mut g = slider_from_fader(t.gain_db);
            // No `step_by` on these sliders: egui snaps the value to the
            // step when it draws it and reports that as a change, which
            // moved strip pans such as 0.28 and started a re-mix unasked.
            let fader = egui::Slider::new(&mut g, FADER_MIN_DB..=FADER_MAX_DB)
                .custom_formatter(|v, _| if v as f32 <= FADER_MIN_DB { "-inf".into() } else { format!("{v:+.1} dB") });
            if ui.add(fader).changed() {
                t.gain_db = fader_from_slider(g);
                changed = true;
            }
            let pan = egui::Slider::new(&mut t.pan, PAN_MIN..=PAN_MAX).custom_formatter(|v, _| pan_text(v as f32));
            changed |= ui.add(pan).changed();
            changed |= ui.toggle_value(&mut t.mute, "M").changed();
            changed |= ui.toggle_value(&mut t.solo, "S").changed();
            ui.end_row();
        }
    });
    let absent: Vec<&str> = TrackId::ALL.into_iter().filter(|id| !present[id.index()]).map(track_label).collect();
    if !absent.is_empty() {
        ui.label(RichText::new(format!("Not in this arrangement: {}.", absent.join(", "))).small().weak());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Ducking under the lead");
        let mut d = live.settings.duck_db.clamp(DUCK_DB_MIN, DUCK_UI_MAX_DB);
        let r = ui.add(egui::Slider::new(&mut d, DUCK_DB_MIN..=DUCK_UI_MAX_DB).suffix(" dB"));
        if r.changed() {
            live.settings.duck_db = d;
            changed = true;
        }
        ui.separator();
        if ui.add_enabled(live.settings != live.defaults, egui::Button::new("Reset")).clicked() {
            live.settings = live.defaults;
            changed = true;
        }
    });
    changed
}

fn pan_text(p: f32) -> String {
    let pct = (p * 100.0).round() as i32;
    match pct {
        0 => "C".into(),
        n if n < 0 => format!("L{}", -n),
        n => format!("R{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_stems() -> Stems {
        Stems { len: 0, tracks: Default::default() }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("studio-mixer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn debounce_waits_for_stillness() {
        let t0 = Instant::now();
        assert!(!due(None, t0 + Duration::from_secs(5)));
        assert!(!due(Some(t0), t0));
        assert!(!due(Some(t0), t0 + DEBOUNCE - Duration::from_millis(1)));
        assert!(due(Some(t0), t0 + DEBOUNCE));
        // A clock that reads earlier than the edit is not due.
        assert!(!due(Some(t0 + Duration::from_secs(1)), t0));
    }

    #[test]
    fn fader_bottom_is_minus_infinity() {
        assert_eq!(fader_from_slider(FADER_MIN_DB), GAIN_DB_MIN);
        assert_eq!(fader_from_slider(-12.5), -12.5);
        assert_eq!(fader_from_slider(20.0), FADER_MAX_DB);
        assert_eq!(slider_from_fader(GAIN_DB_MIN), FADER_MIN_DB);
        assert_eq!(slider_from_fader(0.0), 0.0);
        // A sidecar value past the travel shows at the end of it.
        assert_eq!(slider_from_fader(10.0), FADER_MAX_DB);
    }

    #[test]
    fn remix_on_open_only_for_a_newer_sidecar_with_changes() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let later = t + Duration::from_secs(10);
        let empty = serde_json::json!({"version": 1});
        let some = serde_json::json!({"version": 1, "duck_db": 2.0});
        assert!(!sidecar_needs_remix(None, Some(later), Some(t)));
        assert!(!sidecar_needs_remix(Some(&empty), Some(later), Some(t)));
        assert!(sidecar_needs_remix(Some(&some), Some(later), Some(t)));
        assert!(!sidecar_needs_remix(Some(&some), Some(t), Some(later)));
        assert!(sidecar_needs_remix(Some(&some), None, Some(t)));
    }

    #[test]
    fn pan_reads_as_left_centre_right() {
        assert_eq!(pan_text(0.0), "C");
        assert_eq!(pan_text(-0.25), "L25");
        assert_eq!(pan_text(1.0), "R100");
    }

    #[test]
    fn sidecar_round_trip_through_the_panel() {
        let dir = temp("round");
        let stem = dir.join("song");
        let stems = stub_stems();
        let band = song::Band::default();

        // No sidecar: the defaults, and nothing to re-mix.
        let mut p = MixerPanel::new();
        p.reset_for(&stem, None);
        assert!(!p.auto_load);
        assert!(!p.attach(&stems, &band));
        assert_eq!(p.settings(), Some(MixSettings::default_for(&stems)));

        // Edit, wait out the debounce, save.
        let t0 = Instant::now();
        p.live.as_mut().unwrap().settings.tracks[TrackId::Bass.index()].gain_db = -6.0;
        p.live.as_mut().unwrap().settings.tracks[TrackId::Drums.index()].mute = true;
        p.live.as_mut().unwrap().settings.duck_db = 3.0;
        p.touch(t0);
        assert!(p.take_due(t0 + Duration::from_millis(10)).is_none());
        let s = p.take_due(t0 + DEBOUNCE).expect("due");
        assert!(!p.dirty());
        p.save().unwrap();
        assert!(!jobs::mix_path(&stem).with_extension("tmp").exists());

        // The file holds only the changes, and a fresh panel reads them back.
        let v = jobs::read_mix(&jobs::mix_path(&stem)).unwrap().unwrap();
        assert_eq!(v["tracks"]["bass"]["gain_db"], -6.0);
        assert!(v["tracks"].get("lead").is_none());
        let mut q = MixerPanel::new();
        q.reset_for(&stem, None);
        assert!(q.attach(&stems, &band));
        assert_eq!(q.settings(), Some(s));
        assert!(q.notes.is_empty(), "{:?}", q.notes);

        // With audio older than the sidecar, the song re-mixes on open.
        let audio = dir.join("song.ogg");
        std::fs::write(&audio, b"x").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options().write(true).open(&audio).unwrap().set_modified(old).unwrap();
        let mut r = MixerPanel::new();
        r.reset_for(&stem, Some(&audio));
        assert!(r.auto_load);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_bad_sidecar_is_a_note_not_a_failure() {
        let dir = temp("bad");
        let stem = dir.join("song");
        std::fs::write(jobs::mix_path(&stem), "nope").unwrap();
        let mut p = MixerPanel::new();
        p.reset_for(&stem, None);
        assert!(!p.notes.is_empty());
        let stems = stub_stems();
        assert!(!p.attach(&stems, &song::Band::default()));
        std::fs::write(jobs::mix_path(&stem), r#"{"tracks": {"bogus": {}}}"#).unwrap();
        let mut q = MixerPanel::new();
        q.reset_for(&stem, None);
        q.attach(&stems, &song::Band::default());
        assert!(q.notes.iter().any(|n| n.contains("bogus")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reattach_keeps_the_live_settings() {
        let stems = stub_stems();
        let band = song::Band::default();
        let mut p = MixerPanel::new();
        p.reset_for(&std::env::temp_dir().join("studio-mixer-none/song"), None);
        p.attach(&stems, &band);
        p.live.as_mut().unwrap().settings.duck_db = 1.0;
        // New stems for the same song (a re-render) keep the edits.
        assert!(p.attach(&stems, &band));
        assert_eq!(p.settings().unwrap().duck_db, 1.0);
    }
}

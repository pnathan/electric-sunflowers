//! The settings window: edits and saves the settings crate's config file
//! (model, transport, effort, default voice, duet choice, library dir,
//! Ogg quality). The New song form takes its defaults from the same
//! `settings::Settings` the app loads at start.

use eframe::egui::{self, Color32, RichText};
use songwriter::claude::{Effort, Transport};

use crate::app::voices;

pub struct SettingsForm {
    pub open: bool,
    model: String,
    transport: Transport,
    effort: Effort,
    voice: Option<song::Voice>,
    duet: settings::DuetChoice,
    library: String,
    ogg_quality: String,
    error: Option<String>,
    saved: bool,
}

impl SettingsForm {
    pub fn new() -> SettingsForm {
        SettingsForm {
            open: false,
            model: String::new(),
            transport: Transport::Cli,
            effort: Effort::High,
            voice: None,
            duet: settings::DuetChoice::Auto,
            library: String::new(),
            ogg_quality: String::new(),
            error: None,
            saved: false,
        }
    }

    /// Opens the form, filled from the current settings.
    pub fn open_from(&mut self, s: &settings::Settings) {
        self.open = true;
        self.error = None;
        self.saved = false;
        self.model = s.claude.model.clone();
        self.transport = s.claude.transport;
        self.effort = s.claude.effort;
        self.voice = s.songwriter.voice;
        self.duet = s.songwriter.duet;
        self.library = s.studio.library.display().to_string();
        self.ogg_quality = format!("{}", s.export.ogg_quality);
    }

    /// Draws the window. Returns the new settings once they have been
    /// parsed and saved to disk.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<settings::Settings> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut saved_settings = None;
        egui::Window::new("Settings").open(&mut open).collapsible(false).default_width(420.0).show(ctx, |ui| {
            egui::Grid::new("settings-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Model");
                ui.add(egui::TextEdit::singleline(&mut self.model).desired_width(240.0));
                ui.end_row();

                ui.label("Claude via");
                ui.horizontal(|ui| {
                    for t in Transport::ALL {
                        ui.radio_value(&mut self.transport, *t, t.as_str());
                    }
                });
                ui.end_row();

                ui.label("Effort");
                egui::ComboBox::from_id_salt("settings-effort").selected_text(self.effort.as_str()).show_ui(ui, |ui| {
                    for e in Effort::ALL {
                        ui.selectable_value(&mut self.effort, *e, e.as_str());
                    }
                });
                ui.end_row();

                ui.label("Default voice");
                egui::ComboBox::from_id_salt("settings-voice").selected_text(self.voice.map(|v| v.label()).unwrap_or("Songwriter's choice")).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.voice, None, "Songwriter's choice");
                    for v in voices() {
                        ui.selectable_value(&mut self.voice, Some(v), v.label());
                    }
                });
                ui.end_row();

                ui.label("Duet");
                egui::ComboBox::from_id_salt("settings-duet").selected_text(self.duet.as_str()).show_ui(ui, |ui| {
                    for d in settings::DuetChoice::ALL {
                        ui.selectable_value(&mut self.duet, *d, d.as_str());
                    }
                });
                ui.end_row();

                ui.label("Library");
                ui.add(egui::TextEdit::singleline(&mut self.library).desired_width(280.0));
                ui.end_row();

                ui.label("Ogg quality (-0.2..1)");
                ui.add(egui::TextEdit::singleline(&mut self.ogg_quality).desired_width(80.0));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("The library dir here takes effect next time studio opens (unless started with --dir).").small().weak());
            if let Some(e) = &self.error {
                ui.colored_label(Color32::from_rgb(230, 80, 70), e);
            }
            if self.saved {
                ui.label(RichText::new("Saved.").small().weak());
            }
            if ui.button("Save").clicked() {
                self.saved = false;
                match self.ogg_quality.trim().parse::<f32>() {
                    Ok(q) => {
                        let s = settings::Settings {
                            claude: settings::ClaudeSettings { model: self.model.trim().to_string(), transport: self.transport, effort: self.effort },
                            songwriter: settings::SongwriterSettings { voice: self.voice, duet: self.duet },
                            studio: settings::StudioSettings { library: settings::expand_home(self.library.trim()) },
                            export: settings::ExportSettings { ogg_quality: q },
                        };
                        match settings::config_path() {
                            Some(p) => match settings::save(&s, &p) {
                                Ok(()) => {
                                    self.error = None;
                                    self.saved = true;
                                    saved_settings = Some(s);
                                }
                                Err(e) => self.error = Some(format!("{}: {e}", p.display())),
                            },
                            None => self.error = Some("no home directory: nowhere to save settings".into()),
                        }
                    }
                    Err(_) => self.error = Some(format!("Ogg quality {:?} is not a number", self.ogg_quality)),
                }
            }
        });
        if !open {
            self.open = false;
        }
        saved_settings
    }
}

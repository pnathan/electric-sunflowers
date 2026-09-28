//! Lyrics with chords: the song sheet as a chord chart, chords over the
//! syllables where they change, with the sung line and syllable lit.

use eframe::egui::{self, Align, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use engine::{SheetLine, SheetSection, SongSheet};

/// Time as m:ss.
pub fn clock(t: f64) -> String {
    let s = t.max(0.0).floor() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Byte index of char offset `at` in `s` (the end when past it).
fn byte_at(s: &str, at: usize) -> usize {
    s.char_indices().nth(at).map(|(b, _)| b).unwrap_or(s.len())
}

struct Palette {
    chord: Color32,
    carried: Color32,
    line_bg: Color32,
    syl_bg: Color32,
    hover_bg: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            chord: Color32::from_rgb(245, 175, 70),
            carried: Color32::from_rgb(160, 125, 70),
            line_bg: Color32::from_rgba_unmultiplied(255, 200, 80, 30),
            syl_bg: Color32::from_rgba_unmultiplied(90, 150, 255, 110),
            hover_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
        }
    } else {
        Palette {
            chord: Color32::from_rgb(175, 80, 0),
            carried: Color32::from_rgb(190, 140, 90),
            line_bg: Color32::from_rgba_unmultiplied(255, 190, 40, 50),
            syl_bg: Color32::from_rgba_unmultiplied(60, 120, 255, 70),
            hover_bg: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
        }
    }
}

/// Which line is lit at time `t`: (section, line). The last line that has
/// started, while its section lasts.
pub fn current_line(sheet: &SongSheet, t: f64) -> Option<(usize, usize)> {
    let mut best = None;
    // A pickup line may start before its section does.
    for (si, s) in sheet.sections.iter().enumerate() {
        if t >= s.t1 {
            continue;
        }
        for (li, l) in s.lines.iter().enumerate() {
            if l.t0 <= t {
                best = Some((si, li));
            }
        }
    }
    best
}

#[derive(Default)]
pub struct LyricsView {
    pub follow: bool,
    last: Option<(usize, usize)>,
    last_section: Option<usize>,
}

impl LyricsView {
    pub fn new() -> LyricsView {
        LyricsView { follow: true, last: None, last_section: None }
    }

    /// Draws the sheet; returns a time to seek to when the user clicks.
    pub fn ui(&mut self, ui: &mut egui::Ui, sheet: &SongSheet, pos: f64, playing: bool) -> Option<f64> {
        let pal = palette(ui.visuals().dark_mode);
        let mut seek = None;
        let cur = current_line(sheet, pos);
        let cur_section = sheet.sections.iter().position(|s| s.t0 <= pos && pos < s.t1);
        egui::ScrollArea::vertical().id_salt("lyrics-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(&sheet.title).size(26.0).strong());
            let mut sub = format!("{} {}, {}, {:.0} bpm, {}", sheet.key, sheet.mode, sheet.meter, sheet.tempo, sheet.voice.label());
            if let Some(l) = &sheet.style_label {
                sub.push_str(&format!(" - {l}"));
            }
            ui.label(RichText::new(sub).weak());
            if !sheet.note.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(&sheet.note).italics());
            }
            ui.add_space(10.0);
            for (si, sec) in sheet.sections.iter().enumerate() {
                ui.add_space(8.0);
                let hdr = ui.add(
                    egui::Label::new(RichText::new(format!("{}   {}", sec.label, clock(sec.t0))).strong().size(15.0)).sense(Sense::click()),
                );
                if hdr.clicked() {
                    seek = Some(sec.t0);
                }
                let has_text = sec.lines.iter().any(|l| !l.text.trim().is_empty());
                if !sec.sung || !has_text {
                    let lit = cur_section == Some(si);
                    if let Some(t) = bars_row(ui, sec, pos, lit, &pal) {
                        seek = Some(t);
                    }
                    if lit && playing && self.follow && self.last_section != Some(si) {
                        ui.scroll_to_cursor(Some(Align::Center));
                    }
                    continue;
                }
                for (li, line) in sec.lines.iter().enumerate() {
                    let lit = cur == Some((si, li));
                    let (r, t) = line_row(ui, line, pos, lit, &pal);
                    if t.is_some() {
                        seek = t;
                    }
                    if lit && playing && self.follow && self.last != cur {
                        ui.scroll_to_rect(r, Some(Align::Center));
                    }
                }
            }
            ui.add_space(40.0);
        });
        if playing {
            self.last = cur;
            self.last_section = cur_section;
        }
        seek
    }
}

/// An instrumental section as a row of bar cells, `| Eb | Ab |`.
fn bars_row(ui: &mut egui::Ui, sec: &SheetSection, pos: f64, lit: bool, pal: &Palette) -> Option<f64> {
    let mut seek = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (bi, bar) in sec.bars.iter().enumerate() {
            let t1 = sec.bars.get(bi + 1).map(|b| b.t0).unwrap_or(sec.t1);
            let now = lit && bar.t0 <= pos && pos < t1;
            let text = format!("  {}  ", if bar.chords.is_empty() { "%".to_string() } else { bar.chords.join(" ") });
            let mut rt = RichText::new(text).size(16.0).color(pal.chord);
            if now {
                rt = rt.background_color(pal.syl_bg);
            }
            ui.label(RichText::new("|").size(16.0).weak());
            if ui.add(egui::Label::new(rt).sense(Sense::click())).clicked() {
                seek = Some(bar.t0);
            }
        }
        ui.label(RichText::new("|").size(16.0).weak());
    });
    seek
}

/// One lyric line with its chords above. Returns its rect and a seek time
/// when clicked (the clicked syllable, else the line start).
fn line_row(ui: &mut egui::Ui, line: &SheetLine, pos: f64, lit: bool, pal: &Palette) -> (Rect, Option<f64>) {
    let lyric_font = FontId::proportional(19.0);
    let chord_font = FontId::proportional(15.0);
    let text_color = ui.visuals().text_color();
    let painter = ui.painter().clone();
    let width_of = |s: &str, f: &FontId| painter.layout_no_wrap(s.to_string(), f.clone(), text_color).size().x;
    let x_at = |at: usize| width_of(&line.text[..byte_at(&line.text, at)], &lyric_font);

    // Chord x positions, pushed right so no two overlap.
    let gap = 8.0;
    let mut chords = Vec::with_capacity(line.chords.len());
    let mut end = f32::NEG_INFINITY;
    for c in &line.chords {
        let w = width_of(&c.name, &chord_font);
        let x = x_at(c.at).max(end + gap);
        end = x + w;
        chords.push((x, c));
    }
    let lyric = painter.layout_no_wrap(line.text.clone(), lyric_font.clone(), text_color);
    let chord_h = painter.layout_no_wrap("C".into(), chord_font.clone(), text_color).size().y;
    let w = lyric.size().x.max(end).max(40.0) + 16.0;
    let h = chord_h + lyric.size().y + 6.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w.max(ui.available_width()), h), Sense::click());
    let o = Pos2::new(rect.left() + 8.0, rect.top() + 2.0);
    let lyric_top = o.y + chord_h;

    if lit {
        painter.rect_filled(rect, 4.0, pal.line_bg);
    } else if resp.hovered() {
        painter.rect_filled(rect, 4.0, pal.hover_bg);
    }
    let mut clicked_syl = None;
    let click = resp.clicked().then(|| resp.interact_pointer_pos()).flatten();
    for s in &line.syllables {
        let x0 = o.x + x_at(s.at);
        let x1 = o.x + x_at(s.at + s.text.chars().count());
        let r = Rect::from_min_max(Pos2::new(x0 - 1.0, lyric_top), Pos2::new(x1 + 1.0, lyric_top + lyric.size().y));
        if lit && s.t0 <= pos && pos < s.t1 {
            painter.rect_filled(r, 3.0, pal.syl_bg);
        }
        if let Some(p) = click {
            if p.x >= r.left() && p.x < r.right() {
                clicked_syl = Some(s.t0);
            }
        }
    }
    for (x, c) in &chords {
        let col = if c.carried { pal.carried } else { pal.chord };
        let g = painter.layout_no_wrap(c.name.clone(), chord_font.clone(), col);
        painter.galley(Pos2::new(o.x + x, o.y), g, col);
    }
    painter.galley(Pos2::new(o.x, lyric_top), lyric, text_color);
    if lit {
        painter.rect_stroke(rect, 4.0, Stroke::new(1.0, pal.chord.gamma_multiply(0.5)), StrokeKind::Inside);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let seek = resp.clicked().then(|| clicked_syl.unwrap_or(line.t0));
    (rect, seek)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_offsets() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.9), "1:01");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(byte_at("cafe\u{301} x", 5), 6);
        assert_eq!(byte_at("ab", 9), 2);
    }

    #[test]
    fn current_line_follows_the_demo() {
        let sheet = engine::song_sheet(engine::demo_song(), 7, None);
        assert_eq!(current_line(&sheet, -1.0), None);
        let (si, li) = sheet
            .sections
            .iter()
            .enumerate()
            .find_map(|(si, s)| (s.sung && !s.lines.is_empty()).then_some((si, 0)))
            .unwrap();
        let l = &sheet.sections[si].lines[li];
        assert_eq!(current_line(&sheet, l.t0 + 0.01), Some((si, li)));
        // Every line is lit at its own start.
        for (si, s) in sheet.sections.iter().enumerate() {
            for (li, l) in s.lines.iter().enumerate() {
                if l.t0 < s.t1 {
                    assert_eq!(current_line(&sheet, l.t0), Some((si, li)), "{} line {li}", s.label);
                }
            }
        }
        assert_eq!(current_line(&sheet, sheet.duration_s + 10.0), None);
    }
}

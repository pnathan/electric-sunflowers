//! Sheet music: the notation SVG laid out to the view width, rasterised
//! with resvg into texture tiles, with the sounding note highlighted.

use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Vec2};
use resvg::{tiny_skia, usvg};

use notation::{Score, TimedBox};

/// Height of one raster tile in pixels.
const TILE: u32 = 2048;
/// A width change waits this long before a new layout.
const SETTLE: Duration = Duration::from_millis(150);

/// One laid-out page and its raster.
struct Page {
    /// Layout width in SVG px.
    width: f64,
    height: f64,
    notes: Vec<TimedBox>,
    systems: Vec<TimedBox>,
    /// Pixels per SVG px of the tiles.
    scale: f32,
    /// (top in SVG px, height in SVG px, texture), top to bottom.
    tiles: Vec<(f32, f32, TextureHandle)>,
}

pub struct SheetView {
    score: Score,
    opt: Arc<usvg::Options<'static>>,
    page: Option<Page>,
    /// (layout width, pixel scale) wanted, and when it was first asked for.
    want: Option<(f64, f32, Instant)>,
    pub zoom: f32,
    pub follow: bool,
    last_system: Option<usize>,
    pub error: Option<String>,
}

/// Index of the last box starting at or before `t` that still covers it.
fn covering(boxes: &[TimedBox], t: f64) -> Option<usize> {
    let i = boxes.partition_point(|b| b.0 <= t);
    (i > 0 && t < boxes[i - 1].1).then(|| i - 1)
}

impl SheetView {
    pub fn new(score: Score, opt: Arc<usvg::Options<'static>>) -> SheetView {
        SheetView { score, opt, page: None, want: None, zoom: 1.0, follow: true, last_system: None, error: None }
    }

    /// Lays out and rasterises the page at `width` SVG px and `scale` px per SVG px.
    fn build(&self, ctx: &egui::Context, width: f64, scale: f32) -> Result<Page, String> {
        let score = self.score.clone().with_width(width);
        let svg = notation::engrave(&score);
        let notes = notation::note_boxes(&score);
        let systems = notation::system_boxes(&score);
        let (pw, ph) = notation::page_size(&score);
        let tree = usvg::Tree::from_str(&svg, &self.opt).map_err(|e| format!("sheet SVG: {e}"))?;
        let max_side = ctx.input(|i| i.max_texture_side).max(512) as f32;
        let scale = scale.min(max_side / pw as f32).max(0.1);
        let px_w = (pw as f32 * scale).ceil().max(1.0) as u32;
        let px_h = (ph as f32 * scale).ceil().max(1.0) as u32;
        let tile = TILE.min(max_side as u32);
        let mut tiles = Vec::new();
        let mut y = 0u32;
        while y < px_h {
            let h = tile.min(px_h - y);
            let mut pm = tiny_skia::Pixmap::new(px_w, h).ok_or("empty raster tile")?;
            pm.fill(tiny_skia::Color::WHITE);
            let tf = tiny_skia::Transform::from_scale(scale, scale).post_translate(0.0, -(y as f32));
            resvg::render(&tree, tf, &mut pm.as_mut());
            let img = egui::ColorImage::from_rgba_premultiplied([px_w as usize, h as usize], pm.data());
            let tex = ctx.load_texture(format!("sheet-{y}"), img, TextureOptions::LINEAR);
            tiles.push((y as f32 / scale, h as f32 / scale, tex));
            y += h;
        }
        Ok(Page { width: pw, height: ph, notes, systems, scale, tiles })
    }

    /// True once a page is ready to show.
    pub fn ready(&self) -> bool {
        self.page.is_some()
    }

    /// Draws the page; `pos` is the playback time. Returns a time to seek
    /// to when the user clicks a note.
    pub fn ui(&mut self, ui: &mut egui::Ui, pos: f64, playing: bool) -> Option<f64> {
        let avail = ui.available_width().max(100.0);
        let ppp = ui.ctx().pixels_per_point();
        // Layout width in SVG px so that the page fills the view at this zoom.
        let lw = ((avail - 16.0) / self.zoom).max(300.0) as f64;
        let scale = self.zoom * ppp;
        let stale = match &self.page {
            None => true,
            Some(p) => (p.width - lw).abs() > 4.0 || (p.scale - scale).abs() > 0.01 * scale,
        };
        if stale {
            let now = Instant::now();
            let first = self.page.is_none();
            match self.want {
                Some((w, s, since)) if (w - lw).abs() <= 4.0 && (s - scale).abs() <= 0.01 * scale => {
                    if first || now.duration_since(since) >= SETTLE {
                        match self.build(ui.ctx(), lw, scale) {
                            Ok(p) => {
                                self.page = Some(p);
                                self.error = None;
                            }
                            Err(e) => self.error = Some(e),
                        }
                        self.want = None;
                    } else {
                        ui.ctx().request_repaint_after(SETTLE);
                    }
                }
                _ => {
                    self.want = Some((lw, scale, now));
                    ui.ctx().request_repaint_after(if first { Duration::ZERO } else { SETTLE });
                }
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(200, 40, 40), e);
        }
        let Some(page) = &self.page else {
            ui.spinner();
            return None;
        };

        let mut seek = None;
        let mut scroll_to = None;
        egui::ScrollArea::both().id_salt("sheet-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let k = self.zoom;
            let size = Vec2::new(page.width as f32 * k, page.height as f32 * k);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let o = rect.min;
            let painter = ui.painter_at(rect.expand(2.0));
            for (top, h, tex) in &page.tiles {
                let r = Rect::from_min_size(Pos2::new(o.x, o.y + top * k), Vec2::new(page.width as f32 * k, h * k));
                painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            let to_screen = |b: &TimedBox| Rect::from_min_size(Pos2::new(o.x + b.2 as f32 * k, o.y + b.3 as f32 * k), Vec2::new(b.4 as f32 * k, b.5 as f32 * k));

            let sys = covering(&page.systems, pos).or_else(|| {
                // Between systems: the last one that started.
                let i = page.systems.partition_point(|b| b.0 <= pos);
                (i > 0 && playing).then(|| i - 1)
            });
            if let Some(i) = sys {
                let r = to_screen(&page.systems[i]);
                painter.rect_filled(r.expand(3.0), 4.0, Color32::from_rgba_unmultiplied(255, 200, 60, 28));
                // Cursor: time interpolated across the current note, else the system.
                let x = match covering(&page.notes, pos) {
                    Some(n) => {
                        let nb = to_screen(&page.notes[n]);
                        painter.rect_filled(nb, 3.0, Color32::from_rgba_unmultiplied(40, 120, 255, 60));
                        painter.rect_stroke(nb, 3.0, Stroke::new(1.5, Color32::from_rgb(40, 110, 230)), StrokeKind::Inside);
                        None
                    }
                    None => {
                        let b = &page.systems[i];
                        let f = ((pos - b.0) / (b.1 - b.0).max(1e-6)).clamp(0.0, 1.0) as f32;
                        Some(r.left() + f * r.width())
                    }
                };
                if let Some(x) = x {
                    painter.line_segment([Pos2::new(x, r.top()), Pos2::new(x, r.bottom())], Stroke::new(2.0, Color32::from_rgba_unmultiplied(40, 110, 230, 160)));
                }
                if self.follow && playing && self.last_system != Some(i) {
                    scroll_to = Some(r);
                }
                self.last_system = Some(i);
            }
            if let Some(r) = scroll_to {
                ui.scroll_to_rect(r, Some(egui::Align::Center));
            }
            if resp.clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let hit = |b: &&TimedBox| to_screen(b).contains(p);
                    seek = page.notes.iter().find(hit).map(|b| b.0).or_else(|| {
                        page.systems.iter().find(hit).map(|b| {
                            let r = to_screen(b);
                            b.0 + ((p.x - r.left()) / r.width().max(1.0)).clamp(0.0, 1.0) as f64 * (b.1 - b.0)
                        })
                    });
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });
        seek
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covering_finds_the_sounding_box() {
        let b: Vec<TimedBox> = vec![(1.0, 2.0, 0., 0., 1., 1.), (2.0, 2.5, 0., 0., 1., 1.), (3.0, 4.0, 0., 0., 1., 1.)];
        assert_eq!(covering(&b, 0.5), None);
        assert_eq!(covering(&b, 1.0), Some(0));
        assert_eq!(covering(&b, 2.2), Some(1));
        assert_eq!(covering(&b, 2.7), None);
        assert_eq!(covering(&b, 3.9), Some(2));
        assert_eq!(covering(&b, 9.0), None);
        assert_eq!(covering(&[], 1.0), None);
    }
}

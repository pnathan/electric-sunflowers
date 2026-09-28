//! Music engraving: the sung melody with lyrics and chord symbols, laid out as SVG with SMuFL (Bravura) glyphs.
//!
//! `Score::new(song, prepared)` quantises the prepared lead melody to
//! sixteenths, splits it into bars and notatable values (ties across beats
//! and bars, rests in gaps) and spells it in the transposed key. `engrave`
//! lays it out: one system per lyric line (wrapped when too wide), one per
//! run of bars without lyrics; treble clef, an octave down for bass,
//! baritone and tenor. Each note group carries `data-t0`/`data-t1` in
//! seconds; `note_boxes` and `system_boxes` give the same spans with their
//! boxes on the page, for a player's highlight.
//!
//! Page units are px at the SVG's own `width` x `height`.

pub mod full;
pub mod glyphs;
mod layout;
mod score;

pub use layout::TimedBox;
pub use score::{Score, DEFAULT_WIDTH};

/// The score as a standalone SVG document.
pub fn engrave(score: &Score) -> String {
    layout::layout(score).svg
}

/// Each engraved note (each tied part separately), in time order:
/// (t0, t1, x, y, w, h), seconds and px. The box spans the notehead column
/// from a space above the staff to below the lyric.
pub fn note_boxes(score: &Score) -> Vec<TimedBox> {
    layout::layout(score).notes
}

/// Each system (a lyric line, or a run of bars without lyrics), in order:
/// (t0, t1, x, y, w, h), seconds and px, from the first bar's start to the
/// last bar's end.
pub fn system_boxes(score: &Score) -> Vec<TimedBox> {
    layout::layout(score).systems
}

/// Page size in px: (width, height).
pub fn page_size(score: &Score) -> (f64, f64) {
    let p = layout::layout(score);
    (p.width, p.height)
}

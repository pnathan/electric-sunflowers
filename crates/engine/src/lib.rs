//! The song engine: render a `song::Song` into processed stems, then mix.
//!
//! - `track`: `TrackId`, `BandPart` and the channel-strip table `STRIPS`.
//! - `stem`: block-sparse whole-song buffers (`SparseBuf`, `Stem`).
//! - `vocals`, `band`: render one track's events into a stem.
//! - `strip`: EQ, gated loudness, compressor, slapback.
//! - `render`: the task graph (`render`), producing `Stems`.
//! - `mix`: the block mixer, bus compressor and peak normalisation.
//! - `sheet`: the song sheet (sections, lyrics, chords, times) as rendered.
//!
//! `render` once, then `mix` as often as the band changes.

use std::sync::OnceLock;

pub mod band;
pub mod mix;
pub mod render;
pub mod sheet;
pub mod stem;
pub mod strip;
pub mod track;
pub mod vocals;

pub use mix::{mix, Stereo};
pub use render::{render, NoProgress, Progress, Stems};
pub use sheet::{sheet_from, song_sheet, SheetBar, SheetChord, SheetLine, SheetSection, SheetSyllable, SheetWord, SongSheet};
pub use stem::{SparseBuf, Stem};
pub use strip::ProcessedStem;
pub use track::{BandPart, TrackId, STRIPS};

/// The demo song's reply JSON, embedded at compile time.
pub const DEMO_JSON: &str = include_str!("demo.json");

/// The demo song, normalised (it normalises with no repairs). Built once.
pub fn demo_song() -> &'static song::Song {
    static DEMO: OnceLock<song::Song> = OnceLock::new();
    DEMO.get_or_init(|| match serde_json::from_str(DEMO_JSON).map_err(|e| e.to_string()).and_then(|v| {
        song::normalize_value(&v).map_err(|e| e.to_string())
    }) {
        Ok((s, _)) => s,
        Err(e) => panic!("crates/engine/src/demo.json does not normalise: {e}"),
    })
}

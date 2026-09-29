//! The song engine: render a `song::Song` into processed stems, then mix.
//!
//! - `track`: `TrackId`, `BandPart` and the channel-strip table `STRIPS`.
//! - `stem`: block-sparse whole-song buffers (`SparseBuf`, `Stem`).
//! - `vocals`, `band`: render one track's events into a stem.
//! - `strip`: EQ, gated loudness, compressor, slapback.
//! - `render`: the task graph (`render`), producing `Stems`.
//! - `mix`: the block mixer, bus compressor and peak normalisation,
//!   parameterised by `MixSettings`.
//! - `mixset`: `MixSettings`, the per-track fader/pan/mute/solo and the
//!   ducking depth (a sidecar, never a writing decision; design section 3.2).
//! - `print`: one printed stem or the printed reverb return (design
//!   section 3.3).
//! - `sheet`: the song sheet (sections, lyrics, chords, times) as rendered.
//!
//! `render` once, then `mix` as often as the band or the mix settings change.

use std::sync::OnceLock;

pub mod band;
pub mod mix;
pub mod mixset;
pub mod print;
pub mod render;
pub mod sheet;
pub mod stem;
pub mod strip;
pub mod track;
pub mod vocals;

pub use mix::{mix, mix_with, premix, Stereo};
pub use mixset::{MixSettings, TrackMix};
pub use print::{mix_gain, print_reverb, print_stem};
pub use render::{render, render_with, NoProgress, Progress, Stems};
pub use sheet::{
    sheet_from, song_sheet, song_sheet_with, SheetBar, SheetChord, SheetLine, SheetPart,
    SheetSection, SheetSyllable, SheetWord, SongSheet,
};
pub use stem::{SparseBuf, Stem};
pub use strip::ProcessedStem;
pub use track::{BandPart, TrackId, STRIPS};

/// The demo song's reply JSON, embedded at compile time.
pub const DEMO_JSON: &str = include_str!("demo.json");

/// The demo duet song's reply JSON, embedded at compile time.
pub const DEMO_DUET_JSON: &str = include_str!("demo_duet.json");

/// The demo song, normalised (it normalises with no repairs). Built once.
pub fn demo_song() -> &'static song::Song {
    static DEMO: OnceLock<song::Song> = OnceLock::new();
    DEMO.get_or_init(|| {
        match serde_json::from_str(DEMO_JSON)
            .map_err(|e| e.to_string())
            .and_then(|v| song::normalize_value(&v).map_err(|e| e.to_string()))
        {
            Ok((s, _)) => s,
            Err(e) => panic!("crates/engine/src/demo.json does not normalise: {e}"),
        }
    })
}

/// The demo duet song (a solo song's duet counterpart, used by wave 2's
/// tests and examples), normalised (it normalises with no repairs). Built
/// once.
pub fn demo_duet_song() -> &'static song::Song {
    static DEMO: OnceLock<song::Song> = OnceLock::new();
    DEMO.get_or_init(|| {
        match serde_json::from_str(DEMO_DUET_JSON)
            .map_err(|e| e.to_string())
            .and_then(|v| song::normalize_value(&v).map_err(|e| e.to_string()))
        {
            Ok((s, _)) => s,
            Err(e) => panic!("crates/engine/src/demo_duet.json does not normalise: {e}"),
        }
    })
}

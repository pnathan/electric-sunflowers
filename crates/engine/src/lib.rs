//! The song renderer: renderSong (engine.js lines 867-952) over the compose, voice,
//! arrange and dsp crates, and the entry point for the mix.

pub mod band;
pub mod render;
pub mod vocals;

pub use render::{mix, mix_threaded, render_song, render_song_threaded, RenderedSong};

/// The demo song's reply JSON, embedded at compile time.
pub fn demo_song() -> serde_json::Value {
    serde_json::from_str(include_str!("demo.json")).expect("crates/engine/src/demo.json is valid JSON")
}

/// The demo song, normalised. The demo normalises with no repairs.
pub fn demo() -> song::Song {
    match song::normalize_value(&demo_song()) {
        Ok((s, _)) => s,
        Err(e) => panic!("crates/engine/src/demo.json does not normalise: {e}"),
    }
}

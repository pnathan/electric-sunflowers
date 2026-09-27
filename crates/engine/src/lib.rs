//! The song renderer: renderSong (engine.js lines 867-952) over the compose, voice,
//! arrange and dsp crates, and the entry point for the mix.

pub mod band;
pub mod render;
pub mod vocals;

pub use render::{mix, mix_threaded, render_song, render_song_threaded, RenderedSong};

/// `DEMO_SONG` (src/demo.js), converted to JSON once with node and embedded
/// at compile time. Parsed fresh on every call rather than cached, since it
/// is only ever used by tests and small examples.
pub fn demo_song() -> serde_json::Value {
    serde_json::from_str(include_str!("demo.json")).expect("crates/engine/src/demo.json is valid JSON")
}

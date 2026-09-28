//! The singing voice: voice-type parameters, phoneme acoustics,
//! articulation (segment plan and control tracks), the Liljencrants-Fant
//! glottal source and the formant tract. Entry point: `render_phrases`.

pub mod articulation;
pub mod controls;
pub mod glottal;
pub mod params;
pub mod phoneme;
pub mod synth;
pub mod tract;
pub mod tuning;

pub use params::{voice_params, VoiceParams};
pub use synth::{render_phrases, VoiceSettings, VoiceSynth};

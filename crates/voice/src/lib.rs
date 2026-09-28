//! The singing voice: voice-type parameters, phoneme acoustics, articulation
//! control tracks, the Liljencrants-Fant glottal source and the formant
//! tract.

pub mod controls;
pub mod params;
pub mod phoneme;
pub mod synth;

pub use params::{voice_params, VoiceParams};

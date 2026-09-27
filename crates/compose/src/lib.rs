//! Composition: theory, phonetics, song normalization, form, timeline, rhythm,
//! pitch, melody, and the voice-range transposition (engine.js lines 18-430,
//! 843-866).
//!
//! JS parity: only theory, phonetics and song normalization are ported so
//! far in this pass. form/timeline/rhythm/pitch/melody/voices/prepare remain
//! stubs; see the crate README / task notes for status.

pub mod phonetics;
pub mod song;
pub mod theory;

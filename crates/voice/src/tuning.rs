//! Voice gains and corners that were set by ear against the tools of
//! CLAUDE.md ("Measurement loop") rather than derived. One place, named, so
//! a listening test changes one line.

/// Spectral tilt corner of the glottal source = voice preset tilt times this.
pub const TILT_SCALE: f64 = 1.25;

/// Aspiration noise gain into the cascade (on top of the 0.9 source scale).
pub const ASPIRATION_GAIN: f64 = 0.8;

/// Breath-noise low-pass corner, Hz. Only breath noise is low-passed;
/// aspiration is white (design section 11 lists low-passing it as a later,
/// ear-gated change).
pub const BREATH_LP_HZ: f64 = 2600.0;

/// Stop burst gain on the frication amplitude (onset 0.7, coda 0.4, times
/// the note amplitude). CLAUDE.md, "Consonants": bursts at 0.8.
pub const BURST_GAIN: f64 = 0.8;

/// Frication band-pass output gain, added after the tract.
pub const FRICATION_GAIN: f64 = 2.2;

/// High shelf after the cascade: +16 dB at 5.2 kHz, Q 0.7 (CLAUDE.md, "Voice"). It
/// replaced a parallel high-frequency branch, which filled the vowels'
/// spectral valleys.
pub const SHELF_DB: f64 = 16.0;
pub const SHELF_HZ: f64 = 5200.0;
pub const SHELF_Q: f64 = 0.7;

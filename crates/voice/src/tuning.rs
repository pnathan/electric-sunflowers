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

/// High-frequency voiced bed (issue 22): white noise shaped by the glottal
/// flow, high-passed (4th order, corner `HF_BRANCH_HZ`) and added after the
/// cascade at `HF_BRANCH_GAIN` (0 turns it off). The cascade leaves vowels
/// dark above 4 kHz, so every noise consonant stands out as a spike (median
/// rise over its surroundings 30 dB; real singers 14 dB). A bed of voiced hiss
/// fills the gap. A high-passed copy of the source pulse did the same but
/// sounded buzzy.
pub const HF_BRANCH_HZ: f64 = 3800.0;
pub const HF_BRANCH_GAIN: f64 = 0.0;

/// Scale on the F1-F3 bandwidths (1 is the preset: 60 + 80 breath, 90, 130
/// Hz). Wider formants fill the valleys between harmonics; real voices show
/// valleys 3 to 6 dB shallower than the engine's (issue 22).
pub const BW_SCALE: f64 = 1.0;

/// Vibrato variation, per note (issue 22): the rate varies by
/// +-`VIB_RATE_VAR` and the depth by +-`VIB_DEPTH_VAR` (fractions, drawn
/// from the note's onset time, so a render stays deterministic), and the
/// slow rate wobble gets a second, incommensurate sine of
/// `VIB_WOBBLE2` (fraction of the rate). All 0 is today's steady vibrato.
pub const VIB_RATE_VAR: f64 = 0.0;
pub const VIB_DEPTH_VAR: f64 = 0.0;
pub const VIB_WOBBLE2: f64 = 0.0;

/// High shelf after the cascade: +16 dB at 5.2 kHz, Q 0.7 (CLAUDE.md, "Voice"). It
/// replaced a parallel high-frequency branch, which filled the vowels'
/// spectral valleys.
pub const SHELF_DB: f64 = 16.0;
pub const SHELF_HZ: f64 = 5200.0;
pub const SHELF_Q: f64 = 0.7;

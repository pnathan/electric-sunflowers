//! Sound gate probes: a rough "about right" check of rendered audio that
//! runs without neural taggers. See docs/engine-design.md section 12.
//!
//! - `ltas`: 1/3-octave long-term spectrum, gated level, activity, peak.
//! - `pitch`: YIN f0 per note against the intended MIDI pitch.
//! - `compare`: the gate thresholds between a baseline and a new run.
//! - `mean`: take-robust statistics over seeds and their comparison.
//!
//! This crate depends on no engine crate and carries its own FFT.

pub mod compare;
pub mod fft;
pub mod ltas;
pub mod mean;
pub mod pitch;
pub mod wav;

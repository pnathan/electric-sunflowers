//! Shared foundations for the engine crates: scalar math, the floating-point
//! environment (flush-to-zero), seeded random streams, the seconds-to-sample
//! rule, and the constants every crate agrees on.

pub mod fp;
pub mod math;
pub mod random;
pub mod time;

/// Rendering sample rate (Hz).
pub const SR: usize = 44100;
/// `SR` as `f64`, for coefficient design.
pub const SR_F: f64 = 44100.0;
/// Voice control period (samples): one frame of the control tracks, 689 Hz.
pub const HOP: usize = 64;
/// Silence before the first bar (s).
pub const LEAD_IN: f64 = 0.6;
/// Time after the last bar for tails and reverb (s).
pub const TAIL: f64 = 4.5;

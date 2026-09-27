//! Shared foundations for the engine crates: JS number semantics, seeded random
//! numbers, constants and tuning switches. Parity with src/engine.js is the contract.

pub mod js;
pub mod rng;
pub mod tuning;
pub mod v8math;

pub const SR: usize = 44100;
pub const SR_F: f64 = 44100.0;
pub const HOP: usize = 64;
pub const LEAD_IN: f64 = 0.6;
pub const TAIL: f64 = 4.5;

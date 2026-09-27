//! Signal processing: filters, compression, FFT convolution, reverb, panning, instrument bodies, plucked and bowed strings, and the mix (engine.js lines 597-611, 953-1216).

pub mod body;
pub mod dynamics;
pub mod fft;
pub mod filter;
pub mod mix;
pub mod noise;
pub mod pan;
pub mod pluck;
pub mod reverb;
pub mod violin;

pub mod biquad;
pub mod conv;
pub mod delay;
pub mod onepole;
pub mod resonator;
pub mod smoother;
pub mod stochastic;

/// JS `v||default`: 0, NaN and (by construction, since Rust has no
/// undefined) any other non-finite-zero falsy value all fall back to
/// `default`; any other value passes through unchanged.
pub fn or_default(v: f64, default: f64) -> f64 {
    if v == 0.0 || v.is_nan() {
        default
    } else {
        v
    }
}

/// JS truthiness of a number: `if(v)` is false exactly when `v` is 0 or NaN.
pub fn truthy(v: f64) -> bool {
    !(v == 0.0 || v.is_nan())
}

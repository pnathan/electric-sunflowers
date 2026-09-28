//! Generic signal-processing primitives (design section 2): biquads, one-poles,
//! Klatt resonators, delay lines and allpasses, parameter ramps, stochastic
//! control processes, FFT and overlap-add convolution, the feed-forward
//! compressor, pan laws and the FDN reverb. Product sound models live in
//! `instruments` and `voice`; the track table and mixer live in `engine`.

pub mod biquad;
pub mod conv;
pub mod delay;
pub mod dynamics;
pub mod fft;
pub mod onepole;
pub mod pan;
pub mod resonator;
pub mod reverb;
pub mod smoother;
pub mod stochastic;

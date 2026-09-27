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

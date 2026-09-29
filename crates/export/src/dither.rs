//! Quantisation of `f32` samples to integer PCM, with TPDF dither at 16 bits.
//!
//! Dither: non-subtractive triangular-PDF dither of 2 LSB peak to peak (the
//! difference of two independent uniforms on [0, 1)), added before rounding.
//! This is the smallest dither that makes the first and second moments of
//! the total error independent of the signal (Lipshitz, Wannamaker, Vanderkooy,
//! "Quantization and dither: a theoretical survey", JAES 40(5), 1992;
//! Wannamaker, Lipshitz, Vanderkooy, "A theory of nonsubtractive dither",
//! IEEE Trans. Signal Processing 48(2), 2000). Total error: mean 0, variance
//! 1/6 (dither) + 1/12 (rounding) = 1/4 LSB^2, i.e. about -96.3 dBFS RMS at
//! 16 bits. At 24 bits the rounding error alone is near -144 dBFS, far below
//! the engine's own noise floor, so no dither is added.
//!
//! Determinism: the dither sequence is a pure function of the sample-frame
//! index. Frames are grouped in blocks of `DITHER_BLOCK`; block k draws from
//! `Rng::event(DITHER_SEED, DITHER_TAG, k)`, left then right channel per
//! frame, two draws per value. So WAV and FLAC exports of one signal carry
//! identical samples, and FLAC frames encoded in any order on any thread
//! agree with a serial pass.

use sfcore::random::{tag, Rng, Tag};

use crate::BitDepth;

/// Sample frames per dither stream.
pub const DITHER_BLOCK: usize = 4096;
/// Interleaved channels the dither sequence serves.
const CHANNELS: usize = 2;
/// Fixed seed: dither is not part of the song's randomness.
const DITHER_SEED: u64 = 0x6469_7468_6572; // "dither"
const DITHER_TAG: Tag = tag("export.dither");

/// A TPDF dither source positioned at a sample frame.
pub struct Tpdf {
    rng: Rng,
    /// Index of the current dither block.
    block: u64,
    /// Values left in the current block before the next stream starts.
    left: usize,
}

impl Tpdf {
    /// The dither sequence starting at sample frame `frame`.
    pub fn at(frame: usize) -> Tpdf {
        let block = (frame / DITHER_BLOCK) as u64;
        let mut rng = Rng::event(DITHER_SEED, DITHER_TAG, block);
        let skip = frame % DITHER_BLOCK * CHANNELS;
        for _ in 0..2 * skip {
            rng.next_u32();
        }
        Tpdf {
            rng,
            block,
            left: DITHER_BLOCK * CHANNELS - skip,
        }
    }

    /// Next dither value in LSB: triangular on (-1, 1), mean 0, variance 1/6.
    #[inline]
    #[allow(clippy::should_implement_trait)] // an endless stream; not an Iterator
    pub fn next(&mut self) -> f64 {
        if self.left == 0 {
            self.block += 1;
            self.rng = Rng::event(DITHER_SEED, DITHER_TAG, self.block);
            self.left = DITHER_BLOCK * CHANNELS;
        }
        self.left -= 1;
        self.rng.uniform() - self.rng.uniform()
    }
}

impl BitDepth {
    /// Bits per sample.
    pub fn bits(self) -> u32 {
        match self {
            BitDepth::Bits16 => 16,
            BitDepth::Bits24 => 24,
        }
    }

    /// Integer value of +1.0: 2^(bits-1) - 1. -1.0 maps to its negation, so
    /// the scale is symmetric and the most negative code is used only by
    /// dither or overs.
    pub fn full_scale(self) -> f64 {
        ((1i64 << (self.bits() - 1)) - 1) as f64
    }
}

/// Quantises one sample: scale by `bits.full_scale()`, add TPDF dither at
/// 16 bits (one value drawn from `dither`; none drawn at 24 bits), round
/// half away from zero (`f64::round`), clamp to the code range. NaN gives 0.
#[inline]
pub fn quantize(x: f32, bits: BitDepth, dither: &mut Tpdf) -> i32 {
    let fs = bits.full_scale();
    let mut y = x as f64 * fs;
    if bits == BitDepth::Bits16 {
        y += dither.next();
    }
    // `as` saturates and maps NaN to 0.
    y.round().clamp(-fs - 1.0, fs) as i32
}

/// Quantises the stereo frames `l[i], r[i]` into `out` as interleaved
/// L, R pairs. `dither` must be positioned at the first frame
/// (`Tpdf::at`). `out.len()` must be at least `2 * l.len()`.
pub fn quantize_interleaved(
    l: &[f32],
    r: &[f32],
    bits: BitDepth,
    dither: &mut Tpdf,
    out: &mut [i32],
) {
    debug_assert_eq!(l.len(), r.len());
    for ((&a, &b), o) in l.iter().zip(r).zip(out.as_chunks_mut::<2>().0) {
        o[0] = quantize(a, bits, dither);
        o[1] = quantize(b, bits, dither);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positioned_stream_matches_serial() {
        let mut serial = Tpdf::at(0);
        let mut all = vec![];
        for _ in 0..3 * DITHER_BLOCK * CHANNELS {
            all.push(serial.next());
        }
        for &start in &[
            0usize,
            1,
            100,
            DITHER_BLOCK - 1,
            DITHER_BLOCK,
            DITHER_BLOCK + 7,
            2 * DITHER_BLOCK + 3,
        ] {
            let mut t = Tpdf::at(start);
            for k in 0..200.min(all.len() - start * CHANNELS) {
                assert_eq!(t.next(), all[start * CHANNELS + k], "start {start} k {k}");
            }
        }
    }

    #[test]
    fn clamps_and_handles_nan() {
        let mut d = Tpdf::at(0);
        assert_eq!(quantize(1.5, BitDepth::Bits24, &mut d), 8_388_607);
        assert_eq!(quantize(-1.5, BitDepth::Bits24, &mut d), -8_388_608);
        assert_eq!(quantize(f32::NAN, BitDepth::Bits24, &mut d), 0);
        assert_eq!(quantize(0.5, BitDepth::Bits24, &mut d), 4_194_304); // 4194303.5 rounds away from 0
        assert!(quantize(2.0, BitDepth::Bits16, &mut d) <= 32767);
        assert!(quantize(-2.0, BitDepth::Bits16, &mut d) >= -32768);
    }
}

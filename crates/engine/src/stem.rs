//! Whole-song audio buffers, block-sparse (design section 3.5).
//!
//! A `SparseBuf` holds `len` samples in blocks of `STEM_BLOCK` (4096)
//! frames. A block is allocated on its first write; an absent block reads
//! as zeros. On the demo about half of the vocal blocks and a quarter of
//! the band blocks are silent, so the stems cost far less than dense
//! buffers.
//!
//! Recursive filters over a sparse buffer (`process_sparse`) run over the
//! present blocks. After a present block the filter still rings, so the
//! following absent blocks are allocated and filtered as zeros while the
//! output holds a mean square above `RING_FLOOR` (1e-12, -120 dB); then
//! the filter state is reset and absent blocks are skipped.

/// Frames per stem block.
pub const STEM_BLOCK: usize = 4096;

/// Mean square of a zero-input output block below which a filter's ring
/// is treated as ended (-120 dB).
pub const RING_FLOOR: f64 = 1e-12;

type Block = Box<[f32; STEM_BLOCK]>;

fn zero_block() -> Block {
    // Through a Vec so the 16 KB array is never built on the stack.
    match vec![0.0f32; STEM_BLOCK].into_boxed_slice().try_into() {
        Ok(b) => b,
        Err(_) => unreachable!("the Vec has STEM_BLOCK elements"),
    }
}

/// A mono buffer of `len` samples stored as optional 4096-frame blocks.
#[derive(Clone, Debug, Default)]
pub struct SparseBuf {
    len: usize,
    blocks: Vec<Option<Block>>,
}

impl SparseBuf {
    /// An all-zero buffer of `len` samples (no block allocated).
    pub fn new(len: usize) -> Self {
        SparseBuf {
            len,
            blocks: (0..len.div_ceil(STEM_BLOCK)).map(|_| None).collect(),
        }
    }

    /// A buffer holding `x`; all-zero blocks stay absent.
    pub fn from_dense(x: &[f32]) -> Self {
        let mut s = SparseBuf::new(x.len());
        for (b, chunk) in x.chunks(STEM_BLOCK).enumerate() {
            if chunk.iter().any(|&v| v != 0.0) {
                s.block_mut(b)[..chunk.len()].copy_from_slice(chunk);
            }
        }
        s
    }

    /// Length in samples.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of blocks (present or not).
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Number of allocated blocks.
    pub fn present_blocks(&self) -> usize {
        self.blocks.iter().filter(|b| b.is_some()).count()
    }

    /// True when no block is allocated.
    pub fn is_silent(&self) -> bool {
        self.blocks.iter().all(Option::is_none)
    }

    /// Samples of block `b` that lie inside the buffer.
    pub fn block_len(&self, b: usize) -> usize {
        self.len.saturating_sub(b * STEM_BLOCK).min(STEM_BLOCK)
    }

    /// Block `b`, trimmed to the buffer length, if present.
    pub fn block(&self, b: usize) -> Option<&[f32]> {
        let n = self.block_len(b);
        self.blocks.get(b)?.as_deref().map(|blk| &blk[..n])
    }

    /// Block `b`, trimmed to the buffer length, allocated (zeroed) if absent.
    /// `b` must be below `block_count()`.
    pub fn block_mut(&mut self, b: usize) -> &mut [f32] {
        let n = self.block_len(b);
        &mut self.blocks[b].get_or_insert_with(zero_block)[..n]
    }

    /// Block `b` if present, mutable.
    pub fn block_mut_if_present(&mut self, b: usize) -> Option<&mut [f32]> {
        let n = self.block_len(b);
        self.blocks
            .get_mut(b)?
            .as_deref_mut()
            .map(|blk| &mut blk[..n])
    }

    /// Frames `start .. start + n` if they are inside one present block.
    /// `None` when that block is absent; `n` must not cross a block edge.
    pub fn span(&self, start: usize, n: usize) -> Option<&[f32]> {
        let (b, off) = (start / STEM_BLOCK, start % STEM_BLOCK);
        debug_assert!(off + n <= STEM_BLOCK);
        self.block(b).map(|blk| &blk[off..(off + n).min(blk.len())])
    }

    /// Present blocks in time order as (first frame, samples).
    pub fn iter_blocks(&self) -> impl Iterator<Item = (usize, &[f32])> + '_ {
        (0..self.blocks.len()).filter_map(move |b| self.block(b).map(|x| (b * STEM_BLOCK, x)))
    }

    /// Adds `src * gain` at frame `start`. Frames before 0 and past the end
    /// are dropped (a negative start clips the head of `src`). An all-zero
    /// span of `src` allocates no block.
    pub fn add_at(&mut self, start: isize, src: &[f32], gain: f32) {
        let skip = start.min(0).unsigned_abs();
        if skip >= src.len() {
            return;
        }
        let mut at = start.max(0) as usize;
        let end = self.len.min(at.saturating_add(src.len() - skip));
        let mut i = skip;
        while at < end {
            let (b, off) = (at / STEM_BLOCK, at % STEM_BLOCK);
            let n = (STEM_BLOCK - off).min(end - at);
            let src = &src[i..i + n];
            at += n;
            i += n;
            // Zeros into an absent block leave it absent.
            if self.blocks[b].is_none() && src.iter().all(|&v| v == 0.0) {
                continue;
            }
            let dst = &mut self.block_mut(b)[off..off + n];
            for (d, s) in dst.iter_mut().zip(src) {
                *d += s * gain;
            }
        }
    }

    /// Adds `other` sample by sample (lengths may differ; the overlap is
    /// added). Only `other`'s present blocks are touched.
    pub fn add(&mut self, other: &SparseBuf) {
        let nb = self.blocks.len().min(other.blocks.len());
        for b in 0..nb {
            let Some(src) = other.block(b) else { continue };
            let dst = self.block_mut(b);
            let n = dst.len().min(src.len());
            for (d, s) in dst[..n].iter_mut().zip(&src[..n]) {
                *d += s;
            }
        }
    }

    /// Multiplies every present sample by `g`.
    pub fn scale(&mut self, g: f32) {
        for blk in self.blocks.iter_mut().flatten() {
            blk.iter_mut().for_each(|v| *v *= g);
        }
    }

    /// Copies frames `start .. start + out.len()` into `out`; absent blocks
    /// and frames outside the buffer read as zero.
    pub fn read_into(&self, start: isize, out: &mut [f32]) {
        out.fill(0.0);
        let skip = start.min(0).unsigned_abs().min(out.len());
        let mut at = start.max(0) as usize;
        let end = self.len.min(at.saturating_add(out.len() - skip));
        let mut i = skip;
        while at < end {
            let (b, off) = (at / STEM_BLOCK, at % STEM_BLOCK);
            let n = (STEM_BLOCK - off).min(end - at);
            if let Some(blk) = self.block(b) {
                out[i..i + n].copy_from_slice(&blk[off..off + n]);
            }
            at += n;
            i += n;
        }
    }

    /// The whole buffer as a dense vector.
    pub fn to_dense(&self) -> Vec<f32> {
        let mut out = vec![0.0f32; self.len];
        for (at, x) in self.iter_blocks() {
            out[at..at + x.len()].copy_from_slice(x);
        }
        out
    }
}

/// A block processor run by `process_sparse`: filters a block in place and
/// forgets its state on `reset`.
pub trait Process {
    fn process(&mut self, buf: &mut [f32]);
    fn reset(&mut self);
}

/// Runs `p` over `buf` as if over the dense buffer: present blocks are
/// processed; absent blocks after a present one are allocated and processed
/// while the output rings above `RING_FLOOR` (mean square); then `p` is
/// reset and absent blocks are skipped. `each` sees every processed block
/// (index, output) after `p`, for statistics in the same pass.
pub fn process_sparse<P: Process>(
    buf: &mut SparseBuf,
    p: &mut P,
    mut each: impl FnMut(usize, &[f32]),
) {
    let mut ringing = false;
    for b in 0..buf.block_count() {
        let present = buf.blocks[b].is_some();
        if !present && !ringing {
            continue;
        }
        let x = buf.block_mut(b);
        p.process(x);
        if present {
            ringing = true;
        } else {
            let ms =
                x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len().max(1) as f64;
            if ms < RING_FLOOR {
                p.reset();
                ringing = false;
            }
        }
        each(b, x);
    }
}

/// A track's audio: one channel or two.
#[derive(Clone, Debug)]
pub enum Stem {
    Mono(SparseBuf),
    Stereo([SparseBuf; 2]),
}

impl Stem {
    pub fn len(&self) -> usize {
        match self {
            Stem::Mono(x) => x.len(),
            Stem::Stereo([l, _]) => l.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The channels, one or two.
    pub fn channels(&self) -> &[SparseBuf] {
        match self {
            Stem::Mono(x) => std::slice::from_ref(x),
            Stem::Stereo(lr) => lr,
        }
    }

    pub fn channels_mut(&mut self) -> &mut [SparseBuf] {
        match self {
            Stem::Mono(x) => std::slice::from_mut(x),
            Stem::Stereo(lr) => lr,
        }
    }

    /// True when every channel is silent.
    pub fn is_silent(&self) -> bool {
        self.channels().iter().all(SparseBuf::is_silent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_at_clips_and_spans_blocks() {
        let mut s = SparseBuf::new(10_000);
        let src: Vec<f32> = (0..5000).map(|i| i as f32).collect();
        s.add_at(-10, &src, 2.0);
        s.add_at(9_000, &src, 1.0);
        let d = s.to_dense();
        assert_eq!(d[0], 20.0);
        assert_eq!(d[4989], 2.0 * 4999.0);
        assert_eq!(d[4990], 0.0);
        assert_eq!(d[9_000], 0.0);
        assert_eq!(d[9_999], 999.0);
        assert_eq!(s.present_blocks(), 3);
        s.add_at(-6000, &src, 1.0);
        s.add_at(20_000, &src, 1.0);
        assert_eq!(s.to_dense(), d);
    }

    #[test]
    fn dense_round_trip_skips_zero_blocks() {
        let mut x = vec![0.0f32; 3 * STEM_BLOCK + 17];
        x[STEM_BLOCK + 5] = 1.0;
        x[3 * STEM_BLOCK + 16] = -1.0;
        let s = SparseBuf::from_dense(&x);
        assert_eq!(s.present_blocks(), 2);
        assert_eq!(s.to_dense(), x);
        let starts: Vec<usize> = s.iter_blocks().map(|(a, _)| a).collect();
        assert_eq!(starts, vec![STEM_BLOCK, 3 * STEM_BLOCK]);
        let mut out = vec![9.0f32; 10];
        s.read_into(STEM_BLOCK as isize, &mut out);
        assert_eq!(out[5], 1.0);
        assert_eq!(out[0], 0.0);
        s.read_into(-3, &mut out);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    struct Lp(dsp::biquad::Biquad);
    impl Process for Lp {
        fn process(&mut self, buf: &mut [f32]) {
            self.0.process(buf)
        }
        fn reset(&mut self) {
            self.0.reset()
        }
    }

    #[test]
    fn sparse_filter_matches_dense() {
        use dsp::biquad::{Biquad, BiquadCoeffs};
        let c = BiquadCoeffs::highpass(44_100.0, 30.0, 0.7);
        let mut x = vec![0.0f32; 40 * STEM_BLOCK];
        for (i, v) in x[STEM_BLOCK..3 * STEM_BLOCK].iter_mut().enumerate() {
            *v = ((i as f32) * 0.05).sin();
        }
        x[30 * STEM_BLOCK + 3] = 1.0;
        let mut dense = x.clone();
        Biquad::new(c).process(&mut dense);
        let mut s = SparseBuf::from_dense(&x);
        process_sparse(&mut s, &mut Lp(Biquad::new(c)), |_, _| {});
        let err = s
            .to_dense()
            .iter()
            .zip(&dense)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(err < 1e-5, "max error {err}");
        assert!(
            s.present_blocks() < 20,
            "ring kept {} blocks",
            s.present_blocks()
        );
    }
}

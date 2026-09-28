//! Mono-to-stereo convolution with a fixed impulse response: FFT overlap-add
//! (Stockham 1966, "High-speed convolution and correlation", AFIPS 28).
//!
//! `StereoIr::new` precomputes both half spectra once (in f64, with the 1/n
//! inverse scale folded in, stored as f32). Per input block of `block`
//! samples: one forward real FFT of size n, two complex products, two inverse
//! real FFTs; the n output samples (block plus the IR tail) are added into
//! the output. FFT size n = next power of two >= 4 * IR length, so
//! block = n - len + 1 >= 3 * len; all-zero input blocks are skipped.
//!
//! Parallel: rayon over contiguous ranges of blocks (about 4 per worker).
//! Each range writes its own span of the output directly and its IR tail
//! into a private buffer; the tails are added afterwards in time order.
//! Because block >= IR length, at most two blocks overlap at any sample, so
//! each output sample is `0 + a + b` with the same two terms at any range
//! split, and float addition is commutative: the result is bit-identical at
//! any thread count (tests/fft.rs asserts it).

use rayon::prelude::*;
use realfft::RealFftPlanner;

use crate::fft::{RealFft, C32};

/// A stereo impulse response with its half spectra precomputed for
/// overlap-add at FFT size `n`.
#[derive(Clone)]
pub struct StereoIr {
    /// Taps (the longer of the two channels).
    len: usize,
    /// FFT size: next power of two >= 4 * len.
    n: usize,
    /// Input samples per block: n - len + 1.
    block: usize,
    fft: RealFft,
    /// Left and right spectra (n/2 + 1 bins), scaled by 1/n.
    hl: Vec<C32>,
    hr: Vec<C32>,
}

impl StereoIr {
    /// Builds the IR from left and right taps (lengths may differ; the
    /// shorter is zero-padded). An empty IR convolves to silence.
    pub fn new(l: &[f64], r: &[f64]) -> Self {
        let len = l.len().max(r.len()).max(1);
        let n = (4 * len).next_power_of_two();
        let block = n - len + 1;
        let fft = RealFft::new(n);
        let mut planner = RealFftPlanner::<f64>::new();
        let plan = planner.plan_fft_forward(n);
        let mut time = vec![0.0f64; n];
        let mut spec = plan.make_output_vec();
        let mut scratch = plan.make_scratch_vec();
        let scale = 1.0 / n as f64;
        let mut half = |h: &[f64]| -> Vec<C32> {
            time.fill(0.0);
            time[..h.len()].copy_from_slice(h);
            // Lengths come from the plan itself; the call cannot fail.
            let ok = plan.process_with_scratch(&mut time, &mut spec, &mut scratch);
            debug_assert!(ok.is_ok());
            spec.iter().map(|c| C32::new((c.re * scale) as f32, (c.im * scale) as f32)).collect()
        };
        let hl = half(l);
        let hr = half(r);
        StereoIr { len, n, block, fft, hl, hr }
    }

    /// Converts f32 taps and builds the IR.
    pub fn from_f32(l: &[f32], r: &[f32]) -> Self {
        let l: Vec<f64> = l.iter().map(|&v| v as f64).collect();
        let r: Vec<f64> = r.iter().map(|&v| v as f64).collect();
        StereoIr::new(&l, &r)
    }

    /// Number of taps (at least 1).
    pub fn len(&self) -> usize {
        self.len
    }

    /// Always false: an empty IR is stored as one zero tap.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// FFT size.
    pub fn fft_len(&self) -> usize {
        self.n
    }

    /// Input samples per overlap-add block.
    pub fn block_len(&self) -> usize {
        self.block
    }
}

/// Per-worker buffers for one FFT block.
struct Work {
    time: Vec<f32>,
    spec: Vec<C32>,
    prod: Vec<C32>,
    scratch: Vec<C32>,
}

impl Work {
    fn new(ir: &StereoIr) -> Self {
        let k = ir.fft.spectrum_len();
        Work { time: vec![0.0; ir.n], spec: vec![C32::default(); k], prod: vec![C32::default(); k], scratch: ir.fft.make_scratch() }
    }
}

/// One contiguous range of blocks and the output span it owns.
struct Job<'a> {
    b0: usize,
    b1: usize,
    /// First output sample of `l`/`r`.
    start: usize,
    l: &'a mut [f32],
    r: &'a mut [f32],
}

/// Convolves mono `x` with `ir` and returns `[left, right]`, each of
/// `out_len` samples: y[t] = sum_k x[t - k] h[k] for t < out_len (input past
/// `out_len` and output past `out_len` are dropped; output past
/// x.len() + ir.len() - 1 is zero).
pub fn convolve_mono_to_stereo(x: &[f32], ir: &StereoIr, out_len: usize) -> [Vec<f32>; 2] {
    let mut out_l = vec![0.0f32; out_len];
    let mut out_r = vec![0.0f32; out_len];
    let m = ir.block;
    let used = x.len().min(out_len);
    let nb = used.div_ceil(m);
    if nb == 0 {
        return [out_l, out_r];
    }
    let nr = nb.min(4 * rayon::current_num_threads()).max(1);

    // Split the output into one span per range: range i owns
    // [b0 * m, b1 * m), the last range owns up to out_len.
    let mut jobs = Vec::with_capacity(nr);
    {
        let mut rest_l: &mut [f32] = &mut out_l;
        let mut rest_r: &mut [f32] = &mut out_r;
        let mut start = 0usize;
        for i in 0..nr {
            let b0 = i * nb / nr;
            let b1 = (i + 1) * nb / nr;
            let end = if i + 1 == nr { out_len } else { (b1 * m).min(out_len) };
            let (l, tl) = std::mem::take(&mut rest_l).split_at_mut(end - start);
            let (r, tr) = std::mem::take(&mut rest_r).split_at_mut(end - start);
            rest_l = tl;
            rest_r = tr;
            jobs.push(Job { b0, b1, start, l, r });
            start = end;
        }
    }

    let tails: Vec<(usize, [Vec<f32>; 2])> = jobs
        .into_par_iter()
        .map_init(|| Work::new(ir), |w, job| run_range(x, ir, used, job, w))
        .collect();

    // Tails in time order. Each tail (len - 1 samples) lies inside the next
    // span (>= block >= len samples, or up to out_len for the last span).
    for (at, [tl, tr]) in tails {
        let e = (at + tl.len()).min(out_len);
        if at >= e {
            continue;
        }
        for (o, v) in out_l[at..e].iter_mut().zip(&tl) {
            *o += *v;
        }
        for (o, v) in out_r[at..e].iter_mut().zip(&tr) {
            *o += *v;
        }
    }
    [out_l, out_r]
}

/// Renders blocks b0..b1 into the job's span; returns (first sample, tail).
fn run_range(x: &[f32], ir: &StereoIr, used: usize, job: Job<'_>, w: &mut Work) -> (usize, [Vec<f32>; 2]) {
    let m = ir.block;
    let span_end = job.start + job.l.len();
    let mut tail = [vec![0.0f32; ir.n - m], vec![0.0f32; ir.n - m]];
    for b in job.b0..job.b1 {
        let s = b * m;
        let seg = &x[s..(s + m).min(used)];
        if seg.iter().all(|&v| v == 0.0) {
            continue;
        }
        w.time[..seg.len()].copy_from_slice(seg);
        w.time[seg.len()..].fill(0.0);
        // Buffer lengths are set from the same plan in Work::new.
        let ok = ir.fft.forward(&mut w.time, &mut w.spec, &mut w.scratch);
        debug_assert!(ok.is_ok());
        for (ch, h) in [&ir.hl, &ir.hr].into_iter().enumerate() {
            for ((p, a), b) in w.prod.iter_mut().zip(&w.spec).zip(h.iter()) {
                *p = a * b;
            }
            let ok = ir.fft.inverse_unscaled(&mut w.prod, &mut w.time, &mut w.scratch);
            debug_assert!(ok.is_ok());
            let (span, tl) = if ch == 0 { (&mut *job.l, &mut tail[0]) } else { (&mut *job.r, &mut tail[1]) };
            // Samples s .. s + valid (the linear convolution support; the
            // rest of the n-point cycle is rounding noise and is dropped):
            // the part below span_end goes to the span, the rest to the tail.
            let valid = seg.len() + ir.len - 1;
            let in_span = span_end.saturating_sub(s).min(valid);
            for (o, v) in span[s - job.start..s - job.start + in_span].iter_mut().zip(&w.time[..in_span]) {
                *o += *v;
            }
            for (o, v) in tl.iter_mut().zip(&w.time[in_span..valid]) {
                *o += *v;
            }
        }
    }
    (span_end, tail)
}

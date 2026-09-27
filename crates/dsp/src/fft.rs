//! FFT: thin typed wrappers over `rustfft` (complex) and `realfft`
//! (real-to-complex), single precision, interleaved complex (`C32`).
//!
//! Algorithms are the libraries': rustfft plans a mixed-radix Cooley-Tukey
//! transform (radix 4/8 butterflies, runtime-selected AVX or SSE kernels) for
//! power-of-two sizes; realfft computes a real transform of size n from one
//! complex transform of size n/2 plus the split (post-processing) step
//! (Sorensen, Jones, Heideman, Burrus 1987, "Real-valued fast Fourier
//! transform algorithms", IEEE Trans. ASSP 35(6)).
//!
//! Numeric policy (design 4): data `f32`; twiddles are computed by the
//! libraries in f64 and stored as f32. Measured round-trip error at n = 65536
//! is 4.7e-7 of the signal peak (tests/fft.rs), a floor near -126 dB.
//!
//! Conventions: `forward` is the unscaled DFT X[k] = sum x[t] e^{-2 pi i k t / n}.
//! `inverse` scales by 1/n so `inverse(forward(x)) == x`; `inverse_unscaled`
//! omits the scale for callers that fold 1/n into a precomputed spectrum.
//! Every method takes caller-owned scratch, so a steady-state caller allocates
//! nothing. Length mismatches return `FftError`. Plan lengths must be >= 1
//! (`RealFft`: >= 2); they come from code, never from user input.

use std::sync::Arc;

use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use rustfft::FftPlanner;

pub use realfft::FftError;

/// Single-precision complex sample, `#[repr(C)]` { re, im }.
pub type C32 = realfft::num_complex::Complex32;

/// Complex transform of fixed length `n` (any n >= 1; powers of two are fastest).
#[derive(Clone)]
pub struct Fft {
    n: usize,
    fwd: Arc<dyn rustfft::Fft<f32>>,
    inv: Arc<dyn rustfft::Fft<f32>>,
    scratch_len: usize,
}

impl Fft {
    /// Plans forward and inverse transforms of length `n`.
    pub fn new(n: usize) -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);
        let scratch_len = fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len());
        Fft { n, fwd, inv, scratch_len }
    }

    /// Transform length n.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when n == 0.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Minimum scratch length for `forward` and `inverse`.
    pub fn scratch_len(&self) -> usize {
        self.scratch_len
    }

    /// A zeroed scratch buffer of `scratch_len()`.
    pub fn make_scratch(&self) -> Vec<C32> {
        vec![C32::default(); self.scratch_len]
    }

    /// In-place forward DFT of `data` (length n), unscaled.
    pub fn forward(&self, data: &mut [C32], scratch: &mut [C32]) -> Result<(), FftError> {
        self.check(data, scratch)?;
        self.fwd.process_with_scratch(data, &mut scratch[..self.scratch_len]);
        Ok(())
    }

    /// In-place inverse DFT of `data` (length n), scaled by 1/n.
    pub fn inverse(&self, data: &mut [C32], scratch: &mut [C32]) -> Result<(), FftError> {
        self.check(data, scratch)?;
        self.inv.process_with_scratch(data, &mut scratch[..self.scratch_len]);
        let s = 1.0 / self.n as f32;
        for v in data.iter_mut() {
            *v *= s;
        }
        Ok(())
    }

    fn check(&self, data: &[C32], scratch: &[C32]) -> Result<(), FftError> {
        if data.len() != self.n {
            return Err(FftError::InputBuffer(self.n, data.len()));
        }
        if scratch.len() < self.scratch_len {
            return Err(FftError::ScratchBuffer(self.scratch_len, scratch.len()));
        }
        Ok(())
    }
}

/// Real transform of fixed length `n` (n >= 2, even; powers of two are
/// fastest): n real samples <-> n/2 + 1 complex bins (DC .. Nyquist).
#[derive(Clone)]
pub struct RealFft {
    n: usize,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    scratch_len: usize,
}

impl RealFft {
    /// Plans forward and inverse real transforms of length `n`.
    pub fn new(n: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);
        let scratch_len = fwd.get_scratch_len().max(inv.get_scratch_len());
        RealFft { n, fwd, inv, scratch_len }
    }

    /// Real length n.
    pub fn len(&self) -> usize {
        self.n
    }

    /// True when n == 0.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Number of complex bins, n/2 + 1.
    pub fn spectrum_len(&self) -> usize {
        self.n / 2 + 1
    }

    /// Minimum scratch length for `forward` and the inverses.
    pub fn scratch_len(&self) -> usize {
        self.scratch_len
    }

    /// A zeroed scratch buffer of `scratch_len()`.
    pub fn make_scratch(&self) -> Vec<C32> {
        vec![C32::default(); self.scratch_len]
    }

    /// Forward DFT of `input` (length n) into `output` (n/2 + 1 bins),
    /// unscaled. `input` is used as work space and is garbage afterwards.
    pub fn forward(&self, input: &mut [f32], output: &mut [C32], scratch: &mut [C32]) -> Result<(), FftError> {
        self.fwd.process_with_scratch(input, output, scratch)
    }

    /// Inverse DFT of the half spectrum `input` (n/2 + 1 bins) into `output`
    /// (length n), without the 1/n scale. The imaginary parts of the DC and
    /// Nyquist bins are set to zero first (a real signal has none; rounding
    /// in a spectral product can leave a trace). `input` is garbage afterwards.
    pub fn inverse_unscaled(&self, input: &mut [C32], output: &mut [f32], scratch: &mut [C32]) -> Result<(), FftError> {
        if let Some(v) = input.first_mut() {
            v.im = 0.0;
        }
        if self.n % 2 == 0 {
            if let Some(v) = input.get_mut(self.n / 2) {
                v.im = 0.0;
            }
        }
        self.inv.process_with_scratch(input, output, scratch)
    }

    /// `inverse_unscaled` followed by the 1/n scale, so that
    /// `inverse(forward(x)) == x`.
    pub fn inverse(&self, input: &mut [C32], output: &mut [f32], scratch: &mut [C32]) -> Result<(), FftError> {
        self.inverse_unscaled(input, output, scratch)?;
        let s = 1.0 / self.n as f32;
        for v in output.iter_mut() {
            *v *= s;
        }
        Ok(())
    }
}

/// Shim: deleted in wave 5. Old `conv_stereo` entry point; see `crate::conv`.
pub use crate::conv::conv_stereo;

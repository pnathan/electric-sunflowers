//! Uniform noise buffers. Shim: `sfcore::random::Rng::fill_bipolar` replaces it.

use sfcore::rng::Rng;

/// `n` samples of uniform noise in [-1, 1), one draw per sample.
pub fn noise_buf(n: usize, r: &mut Rng) -> Vec<f32> {
    let mut a = vec![0.0f32; n];
    for v in a.iter_mut() {
        *v = (r.next() * 2.0 - 1.0) as f32;
    }
    a
}

//! `noiseBuf` (engine.js line ~611).

use sfcore::js;
use sfcore::rng::Rng;

/// `noiseBuf(n,r)`: n samples of uniform noise in [-1, 1), stored as Float32Array.
pub fn noise_buf(n: usize, r: &mut Rng) -> Vec<f32> {
    let mut a = vec![0.0f32; n];
    for v in a.iter_mut() {
        *v = js::f32r(r.next() * 2.0 - 1.0) as f32;
    }
    a
}

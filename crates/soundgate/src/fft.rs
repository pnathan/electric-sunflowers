//! Complex radix-2 FFT in f64: iterative decimation-in-time Cooley-Tukey
//! (Cooley and Tukey 1965) with a precomputed bit-reversal permutation and
//! twiddle table. Unnormalised forward transform; the inverse divides by n.
//! Kept inside this crate so the gate shares no code with the engine it
//! measures.

use std::f64::consts::PI;

/// A plan for one power-of-two length.
pub struct Fft {
    n: usize,
    rev: Vec<usize>,
    /// cos and sin of -2*pi*k/n for k < n/2.
    cos: Vec<f64>,
    sin: Vec<f64>,
}

impl Fft {
    /// Plans a transform of length `n`. Returns `None` unless `n` is a
    /// power of two of at least 2.
    pub fn new(n: usize) -> Option<Fft> {
        if n < 2 || !n.is_power_of_two() {
            return None;
        }
        let bits = n.trailing_zeros();
        let rev = (0..n)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let half = n / 2;
        let cos = (0..half)
            .map(|k| (-2.0 * PI * k as f64 / n as f64).cos())
            .collect();
        let sin = (0..half)
            .map(|k| (-2.0 * PI * k as f64 / n as f64).sin())
            .collect();
        Some(Fft { n, rev, cos, sin })
    }

    pub fn len(&self) -> usize {
        self.n
    }

    /// In-place transform of `re` + i*`im` (both of length n). `inverse`
    /// conjugates the twiddles and scales by 1/n.
    pub fn run(&self, re: &mut [f64], im: &mut [f64], inverse: bool) {
        let n = self.n;
        debug_assert!(re.len() == n && im.len() == n);
        for i in 0..n {
            let j = self.rev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let sgn = if inverse { -1.0 } else { 1.0 };
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            let mut start = 0;
            while start < n {
                for k in 0..half {
                    let wr = self.cos[k * step];
                    let wi = sgn * self.sin[k * step];
                    let a = start + k;
                    let b = a + half;
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
                start += len;
            }
            len <<= 1;
        }
        if inverse {
            let s = 1.0 / n as f64;
            for v in re.iter_mut() {
                *v *= s;
            }
            for v in im.iter_mut() {
                *v *= s;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_dft() {
        for &n in &[2usize, 4, 8, 64, 512] {
            let fft = Fft::new(n).unwrap();
            let x: Vec<f64> = (0..n).map(|i| ((i * 7919) % 13) as f64 - 6.0).collect();
            let mut re = x.clone();
            let mut im = vec![0.0; n];
            fft.run(&mut re, &mut im, false);
            for k in 0..n {
                let (mut sr, mut si) = (0.0, 0.0);
                for (i, &v) in x.iter().enumerate() {
                    let ph = -2.0 * PI * (k * i) as f64 / n as f64;
                    sr += v * ph.cos();
                    si += v * ph.sin();
                }
                assert!(
                    (sr - re[k]).abs() < 1e-8 * n as f64 && (si - im[k]).abs() < 1e-8 * n as f64
                );
            }
            fft.run(&mut re, &mut im, true);
            for i in 0..n {
                assert!((re[i] - x[i]).abs() < 1e-9);
            }
        }
        assert!(Fft::new(12).is_none());
    }
}

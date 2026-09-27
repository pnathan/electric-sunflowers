//! `makeFFT`, `convStereo` (engine.js lines ~1140-1150): radix-2 complex FFT and
//! stereo real convolution built from one complex transform per block.
//!
//! JS parity: this is the JS radix-2 FFT ported exactly (bit-reversal permutation,
//! precomputed twiddles), not a faster/different-ordered FFT. A future speed-up
//! is a separate step that keeps this as the reference.

use sfcore::js;

/// `makeFFT(n)`: an in-place complex FFT/IFFT closure over arrays of length `n`.
pub struct Fft {
    n: usize,
    rev: Vec<u32>,
    cs: Vec<f64>,
    sn: Vec<f64>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        let lg = (n as f64).log2() as i64 as u32;
        let mut rev = vec![0u32; n];
        for i in 0..n {
            let mut r: u32 = 0;
            let mut x = i as u32;
            for _ in 0..lg {
                r = (r << 1) | (x & 1);
                x >>= 1;
            }
            rev[i] = r;
        }
        let half = n / 2;
        let mut cs = vec![0.0; half];
        let mut sn = vec![0.0; half];
        for i in 0..half {
            cs[i] = js::cos(2.0 * std::f64::consts::PI * i as f64 / n as f64);
            sn[i] = -js::sin(2.0 * std::f64::consts::PI * i as f64 / n as f64);
        }
        Fft { n, rev, cs, sn }
    }

    /// The `(re,im,inv)` call: forward when `inv` is false, inverse (with 1/n scaling) when true.
    pub fn run(&self, re: &mut [f64], im: &mut [f64], inv: bool) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut size = 2usize;
        while size <= n {
            let h = size >> 1;
            let step = n / size;
            let mut i = 0usize;
            while i < n {
                let mut k = 0usize;
                for j in 0..h {
                    let wr = self.cs[k];
                    let wi = if inv { -self.sn[k] } else { self.sn[k] };
                    let a = i + j;
                    let b = a + h;
                    let xr = re[b] * wr - im[b] * wi;
                    let xi = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                    k += step;
                }
                i += size;
            }
            size <<= 1;
        }
        if inv {
            let s = 1.0 / n as f64;
            for i in 0..n {
                re[i] *= s;
                im[i] *= s;
            }
        }
    }
}

/// `makeFFT(n)`: construct the transform for length `n`.
pub fn make_fft(n: usize) -> Fft {
    Fft::new(n)
}

/// `convStereo(x,hL,hR,outLen)`: convolve mono `x` with the stereo IR `[hL,hR]`
/// -> `[yL,yR]`, one complex FFT per block carrying both channel products.
pub fn conv_stereo(x: &[f32], h_l: &[f32], h_r: &[f32], out_len: usize) -> (Vec<f32>, Vec<f32>) {
    let h = h_l.len().max(h_r.len());
    let mut n = 1usize;
    while n < 4 * h {
        n <<= 1;
    }
    let m = n - h + 1;
    let fft = Fft::new(n);

    let mut ar = vec![0.0f64; n];
    let mut ai = vec![0.0f64; n];
    for i in 0..h_l.len() {
        ar[i] = h_l[i] as f64;
    }
    for i in 0..h_r.len() {
        ai[i] = h_r[i] as f64;
    }
    fft.run(&mut ar, &mut ai, false);

    let mut lr = vec![0.0f64; n];
    let mut li = vec![0.0f64; n];
    let mut rr = vec![0.0f64; n];
    let mut ri = vec![0.0f64; n];
    for k in 0..n {
        let j = (n - k) % n;
        lr[k] = (ar[k] + ar[j]) / 2.0;
        li[k] = (ai[k] - ai[j]) / 2.0;
        rr[k] = (ai[k] + ai[j]) / 2.0;
        ri[k] = -(ar[k] - ar[j]) / 2.0;
    }

    let mut y_l = vec![0.0f32; out_len];
    let mut y_r = vec![0.0f32; out_len];
    let mut re = vec![0.0f64; n];
    let mut im = vec![0.0f64; n];
    let mut s = 0usize;
    while s < x.len() {
        for v in re.iter_mut() {
            *v = 0.0;
        }
        for v in im.iter_mut() {
            *v = 0.0;
        }
        let mut any = false;
        let mut k = 0usize;
        while k < m && s + k < x.len() {
            let v = x[s + k] as f64;
            re[k] = v;
            if v != 0.0 {
                any = true;
            }
            k += 1;
        }
        if !any {
            s += m;
            continue;
        }
        fft.run(&mut re, &mut im, false);
        for k in 0..n {
            let xr = re[k];
            let xi = im[k];
            let a_r = xr * lr[k] - xi * li[k];
            let a_i = xr * li[k] + xi * lr[k];
            let b_r = xr * rr[k] - xi * ri[k];
            let b_i = xr * ri[k] + xi * rr[k];
            re[k] = a_r - b_i;
            im[k] = a_i + b_r;
        }
        fft.run(&mut re, &mut im, true);
        let e = n.min(out_len.saturating_sub(s));
        for k in 0..e {
            y_l[s + k] = js::f32r(y_l[s + k] as f64 + re[k]) as f32;
            y_r[s + k] = js::f32r(y_r[s + k] as f64 + im[k]) as f32;
        }
        s += m;
    }
    (y_l, y_r)
}

//! FFT and convolution timing (design 5.10, 10).
//!
//! Prints ms per 65536-point complex forward transform for the rustfft
//! wrapper (`dsp::fft::Fft`) and for the previous hand-written scalar
//! radix-2 f64 FFT (a copy kept here only as the reference for this bench),
//! ms per 131072-point real transform (the body convolution size), and the
//! wall time of `convolve_mono_to_stereo` on 186 s of noise with a
//! 17000-tap stereo IR at one thread and at all threads, plus the previous
//! convolution (two complex f64 transforms per block, serial) for scale.
//!
//! usage: cargo run --release -p dsp --example fftbench

use std::hint::black_box;
use std::time::Instant;

use dsp::conv::{convolve_mono_to_stereo, StereoIr};
use dsp::fft::{Fft, RealFft, C32};

fn noise(n: usize, seed: u32) -> Vec<f32> {
    let mut s = seed.max(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f64 / 2147483648.0 - 1.0) as f32
        })
        .collect()
}

/// The previous engine FFT: in-place radix-2 decimation in time, bit-reversal
/// permutation, strided f64 twiddle table. Reference for timing only.
struct OldFft {
    n: usize,
    rev: Vec<u32>,
    cs: Vec<f64>,
    sn: Vec<f64>,
}

impl OldFft {
    fn new(n: usize) -> Self {
        let lg = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| if lg == 0 { 0 } else { i.reverse_bits() >> (32 - lg) }).collect();
        let w = |i: usize| 2.0 * std::f64::consts::PI * i as f64 / n as f64;
        let cs = (0..n / 2).map(|i| w(i).cos()).collect();
        let sn = (0..n / 2).map(|i| -w(i).sin()).collect();
        OldFft { n, rev, cs, sn }
    }

    fn run(&self, re: &mut [f64], im: &mut [f64], inv: bool) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut size = 2;
        while size <= n {
            let h = size / 2;
            let step = n / size;
            for i in (0..n).step_by(size) {
                for j in 0..h {
                    let (wr, ws) = (self.cs[j * step], self.sn[j * step]);
                    let wi = if inv { -ws } else { ws };
                    let (a, b) = (i + j, i + j + h);
                    let xr = re[b] * wr - im[b] * wi;
                    let xi = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                }
            }
            size *= 2;
        }
        if inv {
            let s = 1.0 / n as f64;
            re.iter_mut().chain(im.iter_mut()).for_each(|v| *v *= s);
        }
    }
}

/// The previous convolution: both IR channels packed in one complex FFT,
/// two full complex f64 transforms per block, serial.
fn old_conv(x: &[f32], hl: &[f32], hr: &[f32], out_len: usize) -> [Vec<f32>; 2] {
    let h = hl.len().max(hr.len());
    let n = (4 * h).next_power_of_two();
    let m = n - h + 1;
    let fft = OldFft::new(n);
    let (mut ar, mut ai) = (vec![0.0; n], vec![0.0; n]);
    hl.iter().enumerate().for_each(|(i, &v)| ar[i] = v as f64);
    hr.iter().enumerate().for_each(|(i, &v)| ai[i] = v as f64);
    fft.run(&mut ar, &mut ai, false);
    let (mut lr, mut li, mut rr, mut ri) = (vec![0.0; n], vec![0.0; n], vec![0.0; n], vec![0.0; n]);
    for k in 0..n {
        let j = (n - k) % n;
        lr[k] = (ar[k] + ar[j]) / 2.0;
        li[k] = (ai[k] - ai[j]) / 2.0;
        rr[k] = (ai[k] + ai[j]) / 2.0;
        ri[k] = -(ar[k] - ar[j]) / 2.0;
    }
    let (mut yl, mut yr) = (vec![0.0f32; out_len], vec![0.0f32; out_len]);
    let (mut re, mut im) = (vec![0.0; n], vec![0.0; n]);
    let mut s = 0;
    while s < x.len() {
        re.fill(0.0);
        im.fill(0.0);
        let e = (s + m).min(x.len());
        re[..e - s].iter_mut().zip(&x[s..e]).for_each(|(o, &v)| *o = v as f64);
        fft.run(&mut re, &mut im, false);
        for k in 0..n {
            let (xr, xi) = (re[k], im[k]);
            let (a_r, a_i) = (xr * lr[k] - xi * li[k], xr * li[k] + xi * lr[k]);
            let (b_r, b_i) = (xr * rr[k] - xi * ri[k], xr * ri[k] + xi * rr[k]);
            re[k] = a_r - b_i;
            im[k] = a_i + b_r;
        }
        fft.run(&mut re, &mut im, true);
        for k in 0..n.min(out_len.saturating_sub(s)) {
            yl[s + k] += re[k] as f32;
            yr[s + k] += im[k] as f32;
        }
        s += m;
    }
    [yl, yr]
}

fn ms_per<F: FnMut()>(reps: usize, mut f: F) -> f64 {
    f();
    let t = Instant::now();
    for _ in 0..reps {
        f();
    }
    t.elapsed().as_secs_f64() * 1e3 / reps as f64
}

fn main() {
    sfcore::fp::flush_denormals();
    const N: usize = 65536;
    let src: Vec<C32> = noise(2 * N, 1).chunks(2).map(|p| C32::new(p[0], p[1])).collect();

    let fft = Fft::new(N);
    let mut data = src.clone();
    let mut scratch = fft.make_scratch();
    let t_new = ms_per(200, || {
        data.copy_from_slice(&src);
        fft.forward(black_box(&mut data), &mut scratch).expect("sized from plan");
    });
    println!("complex fft n=65536 (rustfft f32):   {t_new:.3} ms per transform (200 runs)");

    let old = OldFft::new(N);
    let mut re: Vec<f64> = src.iter().map(|c| c.re as f64).collect();
    let mut im: Vec<f64> = src.iter().map(|c| c.im as f64).collect();
    let t_old = ms_per(40, || {
        re.iter_mut().zip(&src).for_each(|(o, c)| *o = c.re as f64);
        im.iter_mut().zip(&src).for_each(|(o, c)| *o = c.im as f64);
        old.run(black_box(&mut re), &mut im, false);
    });
    println!("complex fft n=65536 (old radix-2 f64): {t_old:.3} ms per transform (40 runs), {:.1}x slower", t_old / t_new);

    let rf = RealFft::new(2 * N);
    let xs = noise(2 * N, 2);
    let mut xin = xs.clone();
    let mut spec = vec![C32::default(); rf.spectrum_len()];
    let mut rs = rf.make_scratch();
    let t_real = ms_per(200, || {
        xin.copy_from_slice(&xs);
        rf.forward(black_box(&mut xin), &mut spec, &mut rs).expect("sized from plan");
    });
    println!("real fft n=131072 (realfft f32):      {t_real:.3} ms per transform (200 runs)");

    let len = 186 * sfcore::SR;
    let x = noise(len, 3);
    let hl: Vec<f64> = noise(17000, 4).iter().map(|&v| v as f64 * 0.01).collect();
    let hr: Vec<f64> = noise(17000, 5).iter().map(|&v| v as f64 * 0.01).collect();
    let t = Instant::now();
    let ir = StereoIr::new(&hl, &hr);
    println!("StereoIr::new 17000 taps (n={}): {:.1} ms", ir.fft_len(), t.elapsed().as_secs_f64() * 1e3);
    let out_len = len + 17000;
    let pool1 = rayon::ThreadPoolBuilder::new().num_threads(1).build().expect("pool");
    let mut best1 = f64::MAX;
    let mut bestn = f64::MAX;
    let mut y1 = [Vec::new(), Vec::new()];
    for _ in 0..3 {
        let t = Instant::now();
        y1 = pool1.install(|| convolve_mono_to_stereo(&x, &ir, out_len));
        best1 = best1.min(t.elapsed().as_secs_f64());
        let t = Instant::now();
        black_box(convolve_mono_to_stereo(&x, &ir, out_len));
        bestn = bestn.min(t.elapsed().as_secs_f64());
    }
    println!("convolve 186 s, 17000 taps, 1 thread:  {best1:.3} s (best of 3)");
    println!("convolve 186 s, 17000 taps, {} threads: {bestn:.3} s (best of 3)", rayon::current_num_threads());

    let hl32: Vec<f32> = hl.iter().map(|&v| v as f32).collect();
    let hr32: Vec<f32> = hr.iter().map(|&v| v as f32).collect();
    let t = Instant::now();
    let yo = old_conv(&x, &hl32, &hr32, out_len);
    println!("old convolve 186 s, 1 thread:          {:.3} s", t.elapsed().as_secs_f64());
    let peak = yo[0].iter().fold(0.0f32, |a, v| a.max(v.abs()));
    let diff = y1[0].iter().zip(&yo[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    println!("new vs old convolution, left: max diff / peak = {:.2e}", diff / peak);
}

//! FFT and convolution against direct O(n^2) references (design 9).

use dsp::conv::{convolve_mono_to_stereo, StereoIr};
use dsp::fft::{Fft, RealFft, C32};

/// Deterministic uniform noise in [-1, 1) (xorshift32; test data only).
fn noise(n: usize, seed: u32) -> Vec<f64> {
    let mut s = seed.max(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            s as f64 / 2147483648.0 - 1.0
        })
        .collect()
}

fn dft(re: &[f64], im: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let n = re.len();
    let mut or = vec![0.0; n];
    let mut oi = vec![0.0; n];
    for k in 0..n {
        let (mut sr, mut si) = (0.0, 0.0);
        for t in 0..n {
            let a = -2.0 * std::f64::consts::PI * ((k * t) % n) as f64 / n as f64;
            let (s, c) = a.sin_cos();
            sr += re[t] * c - im[t] * s;
            si += re[t] * s + im[t] * c;
        }
        or[k] = sr;
        oi[k] = si;
    }
    (or, oi)
}

#[test]
fn complex_fft_matches_dft() {
    let mut n = 4;
    while n <= 4096 {
        let re = noise(n, n as u32);
        let im = noise(n, n as u32 + 7);
        let (rr, ri) = dft(&re, &im);
        let fft = Fft::new(n);
        let mut data: Vec<C32> = re
            .iter()
            .zip(&im)
            .map(|(&a, &b)| C32::new(a as f32, b as f32))
            .collect();
        let mut scratch = fft.make_scratch();
        fft.forward(&mut data, &mut scratch).unwrap();
        let peak = rr
            .iter()
            .zip(&ri)
            .map(|(a, b)| a.hypot(*b))
            .fold(0.0, f64::max);
        let err = data
            .iter()
            .zip(rr.iter().zip(&ri))
            .map(|(c, (a, b))| (c.re as f64 - a).hypot(c.im as f64 - b))
            .fold(0.0, f64::max);
        println!("n={n} max err / peak = {:.2e}", err / peak);
        assert!(err / peak < 1e-5, "n={n}: {:.2e}", err / peak);
        n *= 2;
    }
}

#[test]
fn complex_round_trip_65536() {
    let n = 65536;
    let re = noise(n, 11);
    let im = noise(n, 12);
    let fft = Fft::new(n);
    let orig: Vec<C32> = re
        .iter()
        .zip(&im)
        .map(|(&a, &b)| C32::new(a as f32, b as f32))
        .collect();
    let mut data = orig.clone();
    let mut scratch = fft.make_scratch();
    fft.forward(&mut data, &mut scratch).unwrap();
    fft.inverse(&mut data, &mut scratch).unwrap();
    let err = data
        .iter()
        .zip(&orig)
        .map(|(a, b)| (a - b).norm())
        .fold(0.0f32, f32::max);
    let peak = orig.iter().map(|c| c.norm()).fold(0.0f32, f32::max);
    println!("round trip n=65536: {:.2e}", err / peak);
    assert!(err / peak < 1e-6, "round trip {:.2e}", err / peak);
}

#[test]
fn real_fft_matches_complex_and_round_trips() {
    for n in [2usize, 4, 16, 256, 4096, 65536] {
        let x = noise(n, 3 + n as u32);
        let cf = Fft::new(n);
        let rf = RealFft::new(n);
        let mut c: Vec<C32> = x.iter().map(|&v| C32::new(v as f32, 0.0)).collect();
        let mut cs = cf.make_scratch();
        cf.forward(&mut c, &mut cs).unwrap();
        let mut xin: Vec<f32> = x.iter().map(|&v| v as f32).collect();
        let mut spec = vec![C32::default(); rf.spectrum_len()];
        let mut rs = rf.make_scratch();
        rf.forward(&mut xin, &mut spec, &mut rs).unwrap();
        let peak = c.iter().map(|v| v.norm()).fold(0.0f32, f32::max);
        let err = spec
            .iter()
            .zip(&c)
            .map(|(a, b)| (a - b).norm())
            .fold(0.0f32, f32::max);
        assert!(
            err / peak < 1e-5,
            "n={n}: real vs complex {:.2e}",
            err / peak
        );

        let mut back = vec![0.0f32; n];
        rf.inverse(&mut spec, &mut back, &mut rs).unwrap();
        let rt = back
            .iter()
            .zip(&x)
            .map(|(a, b)| (*a as f64 - b).abs())
            .fold(0.0, f64::max);
        assert!(rt < 1e-6, "n={n}: real round trip {rt:.2e}");
    }
}

#[test]
fn wrong_lengths_return_errors() {
    let f = Fft::new(64);
    let mut d = vec![C32::default(); 63];
    let mut s = f.make_scratch();
    assert!(f.forward(&mut d, &mut s).is_err());
    let r = RealFft::new(64);
    let mut x = vec![0.0f32; 64];
    let mut o = vec![C32::default(); 10];
    let mut rs = r.make_scratch();
    assert!(r.forward(&mut x, &mut o, &mut rs).is_err());
}

fn direct(x: &[f32], h: &[f64], out_len: usize) -> Vec<f64> {
    let mut y = vec![0.0f64; out_len];
    for (i, &xv) in x.iter().enumerate() {
        if xv == 0.0 {
            continue;
        }
        for (k, &hv) in h.iter().enumerate() {
            if i + k < out_len {
                y[i + k] += xv as f64 * hv;
            }
        }
    }
    y
}

fn rel_err(y: &[f32], r: &[f64]) -> f64 {
    let peak = r.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    y.iter()
        .zip(r)
        .map(|(a, b)| (*a as f64 - b).abs())
        .fold(0.0, f64::max)
        / peak
}

/// Input with silent stretches, so zero-block skipping is exercised.
fn test_input(n: usize) -> Vec<f32> {
    let mut x: Vec<f32> = noise(n, 99).iter().map(|&v| v as f32).collect();
    for v in &mut x[5000..14000] {
        *v = 0.0;
    }
    x
}

#[test]
fn convolution_matches_direct() {
    let x = test_input(20000);
    let hl = noise(3000, 5);
    let hr = noise(2500, 6); // unequal lengths: the shorter is zero-padded
    let ir = StereoIr::new(&hl, &hr);
    assert_eq!(ir.fft_len(), 16384);
    for out_len in [20000 + 3000 - 1, 30000, 12345, 0] {
        let [yl, yr] = convolve_mono_to_stereo(&x, &ir, out_len);
        assert_eq!((yl.len(), yr.len()), (out_len, out_len));
        if out_len == 0 {
            continue;
        }
        let el = rel_err(&yl, &direct(&x, &hl, out_len));
        let er = rel_err(&yr, &direct(&x, &hr, out_len));
        println!("conv out_len={out_len}: L {el:.2e} R {er:.2e}");
        assert!(
            el < 1e-5 && er < 1e-5,
            "out_len={out_len}: L {el:.2e} R {er:.2e}"
        );
    }
    // Past x.len() + ir.len() - 1 the output is exactly zero.
    let [yl, _] = convolve_mono_to_stereo(&x, &ir, 30000);
    assert!(yl[22999..].iter().all(|&v| v == 0.0));
}

#[test]
fn convolution_is_linear() {
    let a = test_input(20000);
    let b: Vec<f32> = noise(20000, 1234).iter().map(|&v| v as f32).collect();
    let ir = StereoIr::new(&noise(3000, 5), &noise(3000, 6));
    let sum: Vec<f32> = a.iter().zip(&b).map(|(p, q)| 0.5 * p + 2.0 * q).collect();
    let [ya, _] = convolve_mono_to_stereo(&a, &ir, 23000);
    let [yb, _] = convolve_mono_to_stereo(&b, &ir, 23000);
    let [ys, _] = convolve_mono_to_stereo(&sum, &ir, 23000);
    let want: Vec<f64> = ya
        .iter()
        .zip(&yb)
        .map(|(p, q)| 0.5 * *p as f64 + 2.0 * *q as f64)
        .collect();
    let e = rel_err(&ys, &want);
    assert!(e < 1e-5, "linearity {e:.2e}");
}

#[test]
fn convolution_edge_cases() {
    let ir = StereoIr::new(&[], &[]);
    let [l, r] = convolve_mono_to_stereo(&[1.0, 2.0], &ir, 4);
    assert!(l.iter().chain(&r).all(|&v| v == 0.0));
    let ir = StereoIr::new(&[1.0], &[0.5]);
    let [l, r] = convolve_mono_to_stereo(&[1.0, 2.0, 3.0], &ir, 5);
    assert_eq!(l, vec![1.0, 2.0, 3.0, 0.0, 0.0]);
    assert_eq!(r, vec![0.5, 1.0, 1.5, 0.0, 0.0]);
    let [l, _] = convolve_mono_to_stereo(&[], &ir, 3);
    assert_eq!(l, vec![0.0; 3]);
    let [l, _] = convolve_mono_to_stereo(&[0.0; 1000], &ir, 1000);
    assert!(l.iter().all(|&v| v == 0.0));
}

#[test]
fn convolution_is_thread_count_invariant() {
    let x = test_input(200000);
    let ir = StereoIr::new(&noise(3000, 5), &noise(3000, 6));
    let run = |t: usize| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build()
            .unwrap()
            .install(|| convolve_mono_to_stereo(&x, &ir, 203000))
    };
    let one = run(1);
    let eight = run(8);
    let three = run(3);
    assert!(
        one[0] == eight[0] && one[1] == eight[1],
        "1 vs 8 threads differ"
    );
    assert!(
        one[0] == three[0] && one[1] == three[1],
        "1 vs 3 threads differ"
    );
    assert!(one[0]
        .iter()
        .zip(&eight[0])
        .all(|(a, b)| a.to_bits() == b.to_bits()));
}

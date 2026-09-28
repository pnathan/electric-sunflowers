//! Seed-to-seed spread of the body IRs (design section 5.6).
//!
//! For each body, 32 seeds (from the first argument, default 0): the 1/3-octave band powers of each channel of
//! `Body::taps` (FFT of 32768 points, power averaged over the bins of the
//! band), in dB. Prints per band the mean level and the standard deviation
//! over the seeds (the larger of the two channels), then the largest
//! standard deviation from 80 Hz up, and the mean magnitude of the L/R
//! correlation coefficient of the taps (broadband, and below 300 Hz by a 4th-order
//! low-pass in the frequency domain).

use dsp::fft::{RealFft, C32};
use instruments::body::Body;
use sfcore::random::{tag, Rng};
use sfcore::SR_F;

const SEEDS: u64 = 32;
const N: usize = 32768;

fn main() {
    // Optional first seed (default 0).
    let first: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(0);
    let fft = RealFft::new(N);
    let mut scratch = fft.make_scratch();
    let mut spec = [vec![C32::default(); fft.spectrum_len()], vec![C32::default(); fft.spectrum_len()]];
    let mut time = vec![0.0f32; N];
    let df = SR_F / N as f64;
    let centres: Vec<f64> = (0..23).map(|k| 80.0 * 2f64.powf(k as f64 / 3.0)).collect();
    let t0 = std::time::Instant::now();
    let mut t_taps = std::time::Duration::ZERO;
    for body in [Body::Guitar, Body::Harp, Body::Violin] {
        // db[ch][band][seed]
        let mut db = vec![vec![vec![0.0f64; SEEDS as usize]; centres.len()]; 2];
        let (mut corr, mut corr_lo) = (0.0, 0.0f64);
        for seed in 0..SEEDS {
            let mut rng = Rng::stream(first + seed, tag("band.body"));
            let t = std::time::Instant::now();
            let taps = body.taps(&mut rng);
            t_taps += t.elapsed();
            for (c, ch) in taps.iter().enumerate() {
                time.fill(0.0);
                for (t, &x) in time.iter_mut().zip(ch.iter()) {
                    *t = x as f32;
                }
                fft.forward(&mut time, &mut spec[c], &mut scratch).unwrap();
                for (b, &fc) in centres.iter().enumerate() {
                    let (f0, f1) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
                    let (k0, k1) = ((f0 / df).ceil() as usize, (f1 / df).floor() as usize);
                    let p: f64 = spec[c][k0..=k1].iter().map(|z| (z.re as f64).powi(2) + (z.im as f64).powi(2)).sum::<f64>()
                        / (k1 - k0 + 1) as f64;
                    db[c][b][seed as usize] = 10.0 * p.log10();
                }
            }
            let dot: f64 = taps[0].iter().zip(&taps[1]).map(|(a, b)| a * b).sum();
            corr += dot.abs(); // unit energy per channel
            // Low band: cross- and auto-spectra weighted by |H|^2 of a
            // 4th-order Butterworth low-pass at 300 Hz.
            let (mut x, mut e0, mut e1) = (0.0, 0.0, 0.0);
            for k in 0..spec[0].len() {
                let w = 1.0 / (1.0 + (k as f64 * df / 300.0).powi(8));
                let (a, b) = (spec[0][k], spec[1][k]);
                x += w * (a.re as f64 * b.re as f64 + a.im as f64 * b.im as f64);
                e0 += w * ((a.re as f64).powi(2) + (a.im as f64).powi(2));
                e1 += w * ((b.re as f64).powi(2) + (b.im as f64).powi(2));
            }
            corr_lo += (x / (e0 * e1).sqrt()).abs();
        }
        println!("{body:?}");
        println!("  band_hz  mean_db  std_db");
        let mut worst: f64 = 0.0;
        for (b, &fc) in centres.iter().enumerate() {
            let mut sd: f64 = 0.0;
            let mut mean = 0.0;
            for c in 0..2 {
                let v = &db[c][b];
                let m = v.iter().sum::<f64>() / v.len() as f64;
                let s = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt();
                sd = sd.max(s);
                mean += m / 2.0;
            }
            worst = worst.max(sd);
            println!("  {fc:7.0}  {mean:7.1}  {sd:6.2}");
        }
        println!(
            "  worst std {worst:.2} dB; mean |L/R correlation| {:.3} broadband, {:.3} below 300 Hz",
            corr / SEEDS as f64,
            corr_lo / SEEDS as f64
        );
    }
    println!("taps: {:.1} ms per IR (total run {:.2} s)", t_taps.as_secs_f64() * 1e3 / (3 * SEEDS) as f64, t0.elapsed().as_secs_f64());
}

//! Body impulse responses: unit energy, and a 1/3-octave spectrum that
//! follows the measured curve.

use dsp::fft::{RealFft, C32};
use instruments::body::Body;
use sfcore::random::{tag, Rng};
use sfcore::SR_F;

const BODIES: [Body; 3] = [Body::Guitar, Body::Harp, Body::Violin];

#[test]
fn body_taps_have_unit_energy() {
    for body in BODIES {
        for seed in 0..4u64 {
            let mut rng = Rng::stream(seed, tag("test.body"));
            let taps = body.taps(&mut rng);
            for ch in &taps {
                let e: f64 = ch.iter().map(|x| x * x).sum();
                assert!((e - 1.0).abs() < 0.01, "{body:?} seed {seed}: energy {e}");
                assert!(ch.iter().all(|x| x.is_finite()));
                assert_eq!(ch.len(), (body.spec().secs * SR_F).round() as usize);
            }
            assert_ne!(taps[0], taps[1], "channels must differ");
        }
    }
}

#[test]
fn body_level_extrapolates_12_db_per_octave() {
    for body in BODIES {
        let c = body.curve();
        assert!((body.level_db(40.0) - (c[0] - 12.0)).abs() < 1e-9);
        let top = 80.0 * 2f64.powf(85.0 / 12.0);
        assert!((body.level_db(top * 2.0) - (c[85] - 12.0)).abs() < 1e-9);
        assert!((body.level_db(160.0) - c[12]).abs() < 1e-9);
    }
}

/// 1/3-octave band powers of the IR spectrum against the curve power
/// averaged over the same bins, both in dB, averaged over 8 seeds and both
/// channels. The mean offset is removed. Every body from its lowest full
/// band to 8 kHz: within 1.5 dB above 300 Hz (measured worst 0.8 dB) and
/// 2.5 dB below, where the fixed low modes are one draw of 1-3 modes per
/// 1/12 octave (measured worst 2.0 dB, guitar 160 Hz). The group energy
/// constraint and the removal of each group's onset step and DC (design
/// 5.6) keep the bands on the curve; the earlier model's onset skirts put
/// the harp 4-8 dB above its curve from 3 to 8 kHz.
#[test]
fn body_spectrum_follows_curve() {
    let n = 32768;
    let fft = RealFft::new(n);
    let mut scratch = fft.make_scratch();
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    let mut time = vec![0.0f32; n];
    let df = SR_F / n as f64;
    let centres: Vec<f64> = (0..).map(|k| 100.0 * 2f64.powf(k as f64 / 3.0)).take_while(|&f| f <= 8100.0).collect();
    for body in BODIES {
        let (lo, hi, tol) = match body {
            Body::Guitar | Body::Harp => (100.0, 8100.0, 1.5),
            Body::Violin => (250.0, 8100.0, 1.5),
        };
        let mut pow = vec![0.0f64; centres.len()];
        for seed in 0..8u64 {
            let mut rng = Rng::stream(seed, tag("test.body.spec"));
            for ch in body.taps(&mut rng) {
                time.fill(0.0);
                for (t, &x) in time.iter_mut().zip(ch.iter()) {
                    *t = x as f32;
                }
                fft.forward(&mut time, &mut spec, &mut scratch).unwrap();
                for (b, &fc) in centres.iter().enumerate() {
                    let (f0, f1) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
                    let (k0, k1) = ((f0 / df).ceil() as usize, (f1 / df).floor() as usize);
                    let p: f64 = spec[k0..=k1].iter().map(|c| (c.re as f64).powi(2) + (c.im as f64).powi(2)).sum::<f64>() / (k1 - k0 + 1) as f64;
                    pow[b] += p;
                }
            }
        }
        let mut dev = Vec::new();
        for (b, &fc) in centres.iter().enumerate() {
            if fc < lo || fc > hi {
                continue;
            }
            let (f0, f1) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            let (k0, k1) = ((f0 / df).ceil() as usize, (f1 / df).floor() as usize);
            let curve: f64 = (k0..=k1).map(|k| 10f64.powf(body.level_db(k as f64 * df) / 10.0)).sum::<f64>() / (k1 - k0 + 1) as f64;
            dev.push((fc, 10.0 * pow[b].log10() - 10.0 * curve.log10()));
        }
        let mean = dev.iter().map(|d| d.1).sum::<f64>() / dev.len() as f64;
        let worst = dev.iter().map(|d| (d.1 - mean).abs()).fold(0.0, f64::max);
        println!("{body:?}: worst 1/3-octave deviation {worst:.2} dB");
        for &(fc, d) in &dev {
            // Below the crossover: one fixed draw of 1-3 modes per 1/12
            // octave, so a wider tolerance.
            let tol = if fc < 300.0 { tol + 1.0 } else { tol };
            assert!((d - mean).abs() < tol, "{body:?} {fc:.0} Hz: {:+.2} dB", d - mean);
        }
    }
}

#[test]
fn body_impulse_response_is_trimmed() {
    let mut a = Rng::stream(1, tag("test.body.ir"));
    let ir = Body::Guitar.impulse_response(&mut a);
    assert_eq!(ir.len(), (0.36 * SR_F).round() as usize);
}


/// 1/3-octave band power of each channel of `taps` in dB, bands centred
/// at 80 Hz * 2^(k/3), k = 0..23.
fn band_db(taps: &[Vec<f64>; 2]) -> [Vec<f64>; 2] {
    let n = 32768;
    let fft = RealFft::new(n);
    let mut scratch = fft.make_scratch();
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    let mut time = vec![0.0f32; n];
    let df = SR_F / n as f64;
    let mut out = [Vec::new(), Vec::new()];
    for (c, ch) in taps.iter().enumerate() {
        time.fill(0.0);
        for (t, &x) in time.iter_mut().zip(ch.iter()) {
            *t = x as f32;
        }
        fft.forward(&mut time, &mut spec, &mut scratch).unwrap();
        for k in 0..23 {
            let fc = 80.0 * 2f64.powf(k as f64 / 3.0);
            let (f0, f1) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            let (k0, k1) = ((f0 / df).ceil() as usize, (f1 / df).floor() as usize);
            let p: f64 = spec[k0..=k1].iter().map(|z| (z.re as f64).powi(2) + (z.im as f64).powi(2)).sum::<f64>() / (k1 - k0 + 1) as f64;
            out[c].push(10.0 * p.log10());
        }
    }
    out
}

/// Seed-to-seed spread: the standard deviation over 32 seeds of each
/// 1/3-octave band, per channel. Below the 300 Hz crossover the modes are
/// the body's own, the same for every seed: under 0.6 dB (guitar and harp
/// under 0.25 dB; the violin, whose energy is mostly in per-seed modes,
/// carries the spread of the unit-energy gain, 0.45 dB at 250 Hz). Above
/// it: under 2 dB (measured worst 1.2 guitar, 1.5 harp, 1.4 violin;
/// `examples/bodyspread.rs` prints the table).
/// The violin has no modes below 180 Hz, so its bands from 80 to 160 Hz
/// are skipped.
#[test]
fn body_bands_are_stable_across_seeds() {
    for body in BODIES {
        let runs: Vec<[Vec<f64>; 2]> =
            (0..32u64).map(|seed| band_db(&body.taps(&mut Rng::stream(seed, tag("test.body.spread"))))).collect();
        for k in 0..23 {
            let fc = 80.0 * 2f64.powf(k as f64 / 3.0);
            if body == Body::Violin && fc < 200.0 {
                continue;
            }
            for c in 0..2 {
                let v: Vec<f64> = runs.iter().map(|r| r[c][k]).collect();
                let m = v.iter().sum::<f64>() / v.len() as f64;
                let sd = (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt();
                let lim = if fc * 2f64.powf(1.0 / 6.0) < 300.0 { 0.6 } else { 2.0 };
                assert!(sd < lim, "{body:?} {fc:.0} Hz ch {c}: std {sd:.2} dB");
            }
        }
    }
}

/// The two channels stay decorrelated: mean |correlation coefficient| over
/// 16 seeds under 0.25 (measured 0.03 guitar, 0.04 harp, 0.10 violin; the
/// earlier model 0.20-0.22).
#[test]
fn body_channels_are_decorrelated() {
    for body in BODIES {
        let mut acc = 0.0;
        for seed in 0..16u64 {
            let [l, r] = body.taps(&mut Rng::stream(seed, tag("test.body.corr")));
            acc += l.iter().zip(&r).map(|(a, b)| a * b).sum::<f64>().abs();
        }
        let mean = acc / 16.0;
        assert!(mean < 0.25, "{body:?}: mean |L/R correlation| {mean:.3}");
    }
}

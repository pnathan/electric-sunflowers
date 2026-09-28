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
/// channels (the modes have random Gaussian amplitudes, so a single IR
/// scatters by a few dB per band). The mean offset is removed.
///
/// Guitar: 100 Hz-8 kHz within 3 dB. Violin: 250 Hz-6.3 kHz within 3 dB
/// (its modes start at 180 Hz; its 8 kHz band reads +3.2 dB from effect (1)
/// below, where its curve falls to -40 dB). Harp: 100 Hz-2.5 kHz within 4 dB. Two properties
/// of the model (design 5.6), measured identically on the original
/// dsp::body IR, keep the harp outside 3 dB: (1) each mode starts at
/// `a sin(ph)`, a step, whose spectrum falls only 6 dB per octave; the sum
/// of these onset skirts lies near -50 dB, above the harp curve (-40 to
/// -60 dB) from 3 to 8 kHz, so those bands read +4 to +8 dB; (2) the harp's
/// low Q (18) spreads the narrow 217 Hz peak into its neighbours, so the
/// 200 Hz band reads about -4 dB. Both are part of the shipped harp sound.
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
            Body::Guitar => (100.0, 8100.0, 3.0),
            Body::Violin => (250.0, 6500.0, 3.0),
            Body::Harp => (100.0, 2600.0, 4.0),
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


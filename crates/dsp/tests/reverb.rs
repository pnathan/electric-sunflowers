//! Fdn8: decay time against the absorption design, stability, decorrelation.

use dsp::biquad::{Biquad, BiquadCoeffs};
use std::f64::consts::FRAC_1_SQRT_2;
use dsp::reverb::{absorption, Fdn8, T60_DC, T60_NYQ};
use sfcore::random::{tag, Rng};

const FS: f64 = 44_100.0;

/// Design T60 at `f` Hz: mean over the lines of `-3 d / (fs log10 |H(e^jw)|)`
/// for each line's absorption one-pole.
fn design_t60(fdn: &Fdn8, f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / FS;
    let d = fdn.delays();
    let mut acc = 0.0;
    for &dk in &d {
        let (b, a) = absorption(dk, FS, T60_DC, T60_NYQ);
        let mag = b / (1.0 - 2.0 * a * w.cos() + a * a).sqrt();
        acc += -3.0 * dk as f64 / (FS * mag.log10());
    }
    acc / d.len() as f64
}

/// T60 from the Schroeder backward integral of `e` (energy per sample), by a
/// least-squares line over the -5 to -35 dB range.
fn schroeder_t60(e: &[f64]) -> f64 {
    let mut edc = vec![0.0; e.len()];
    let mut acc = 0.0;
    for i in (0..e.len()).rev() {
        acc += e[i];
        edc[i] = acc;
    }
    let tot = edc[0];
    let (mut sx, mut sy, mut sxx, mut sxy, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &v) in edc.iter().enumerate() {
        let db = 10.0 * (v / tot).log10();
        if (-35.0..=-5.0).contains(&db) {
            let t = i as f64 / FS;
            sx += t;
            sy += db;
            sxx += t * t;
            sxy += t * db;
            n += 1.0;
        }
    }
    let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
    -60.0 / slope
}

#[test]
fn octave_250_500_t60_matches_design() {
    let mut fdn = Fdn8::new(FS, 1234, T60_DC, T60_NYQ);
    let n = (6.0 * FS) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    for i in 0..n {
        let x = if i == 0 { 1.0 } else { 0.0 };
        [l[i], r[i]] = fdn.process([x, x]);
    }
    // Octave band 250-500 Hz: two second-order high-passes at 250 Hz and two
    // low-passes at 500 Hz (fourth-order Butterworth-like skirts).
    let band = |x: &mut [f32]| {
        for c in [
            BiquadCoeffs::highpass(FS, 250.0, FRAC_1_SQRT_2),
            BiquadCoeffs::highpass(FS, 250.0, FRAC_1_SQRT_2),
            BiquadCoeffs::lowpass(FS, 500.0, FRAC_1_SQRT_2),
            BiquadCoeffs::lowpass(FS, 500.0, FRAC_1_SQRT_2),
        ] {
            Biquad::new(c).process(x);
        }
    };
    band(&mut l);
    band(&mut r);
    let e: Vec<f64> = l.iter().zip(&r).map(|(a, b)| (*a as f64).powi(2) + (*b as f64).powi(2)).collect();
    let got = schroeder_t60(&e);
    let want = design_t60(&fdn, (250.0f64 * 500.0).sqrt());
    println!("octave 250-500 Hz: T60 measured {got:.3} s, design {want:.3} s");
    assert!((got / want - 1.0).abs() < 0.10, "T60 {got:.3} s vs design {want:.3} s");
}

#[test]
fn sixty_seconds_of_noise_stays_bounded() {
    let mut fdn = Fdn8::new(FS, 2718, T60_DC, T60_NYQ);
    let mut rng = Rng::stream(7, tag("test.reverb.noise"));
    let block = 1024;
    let mut sl = vec![0.0f32; block];
    let mut sr = vec![0.0f32; block];
    let mut ol = vec![0.0f32; block];
    let mut or = vec![0.0f32; block];
    let mut peak = 0.0f32;
    let mut last_rms = 0.0f64;
    let blocks = (60.0 * FS) as usize / block;
    for bi in 0..blocks {
        rng.fill_bipolar(&mut sl);
        rng.fill_bipolar(&mut sr);
        ol.fill(0.0);
        or.fill(0.0);
        fdn.process_block([&sl, &sr], [&mut ol, &mut or], 1.0);
        let mut e = 0.0f64;
        for (a, b) in ol.iter().zip(&or) {
            assert!(a.is_finite() && b.is_finite());
            peak = peak.max(a.abs()).max(b.abs());
            e += (*a as f64).powi(2) + (*b as f64).powi(2);
        }
        if bi == blocks - 1 {
            last_rms = (e / (2 * block) as f64).sqrt();
        }
    }
    println!("60 s noise: peak {peak:.3}, last-block rms {last_rms:.3}");
    assert!(peak < 20.0, "peak {peak}");
    assert!(last_rms > 1e-3 && last_rms < 5.0, "rms {last_rms}");
}

#[test]
fn mono_input_gives_decorrelated_outputs() {
    let mut fdn = Fdn8::new(FS, 1234, T60_DC, T60_NYQ);
    let mut rng = Rng::stream(11, tag("test.reverb.mono"));
    let n = (10.0 * FS) as usize;
    let mut x = vec![0.0f32; n];
    rng.fill_bipolar(&mut x);
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    fdn.process_block([&x, &x], [&mut l, &mut r], 1.0);
    let skip = FS as usize; // past the build-up
    let (mut sll, mut srr, mut slr) = (0.0f64, 0.0f64, 0.0f64);
    for i in skip..n {
        let (a, b) = (l[i] as f64, r[i] as f64);
        sll += a * a;
        srr += b * b;
        slr += a * b;
    }
    let rho = slr / (sll * srr).sqrt();
    println!("mono in: L/R correlation {rho:.4}");
    assert!(rho.abs() < 0.3, "correlation {rho}");
}

#[test]
fn seed_offsets_stay_in_range() {
    for seed in [0u64, 1, 1234, 2718, u64::MAX] {
        let d = Fdn8::new(FS, seed, T60_DC, T60_NYQ).delays();
        for (k, &dk) in d.iter().enumerate() {
            let base = dsp::reverb::BASE_DELAYS[k];
            assert!(dk >= base && dk <= base + 30, "seed {seed} line {k}: {dk}");
        }
    }
}

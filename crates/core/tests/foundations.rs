//! Tests for sfcore's foundations: math, fp, random, time.

use sfcore::fp::flush_denormals;
use sfcore::math::*;
use sfcore::random::{tag, Rng, Tag};
use sfcore::time::{len_samples, sample_at};
use std::hint::black_box;

const N: usize = 1_000_000;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// ---- math

#[test]
fn math_basics() {
    assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
    assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
    assert_eq!(clamp(0.5, 1.0, 0.0), 0.0); // lo > hi: no panic, gives hi
    assert_eq!(lerp(2.0, 4.0, 0.25), 2.5);
    assert_eq!(smoothstep(0.0, 1.0, -1.0), 0.0);
    assert_eq!(smoothstep(0.0, 1.0, 2.0), 1.0);
    assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
    assert_eq!(smoothstep(1.0, 1.0, 1.0), 1.0);
    assert_eq!(smoothstep(1.0, 1.0, 0.9), 0.0);
}

#[test]
fn mtof_equal_temperament() {
    assert_eq!(mtof(69.0), 440.0);
    assert_eq!(mtof(81.0), 880.0);
    assert_eq!(mtof(57.0), 220.0);
    assert!(close(mtof(60.0), 261.625_565_300_598_6, 1e-9));
}

#[test]
fn db_gain_round_trip() {
    assert!(close(db_to_gain(0.0), 1.0, 1e-15));
    assert!(close(db_to_gain(-20.0), 0.1, 1e-15));
    assert!(close(db_to_gain(6.020_599_913_279_624), 2.0, 1e-12));
    for db in [-120.0, -60.0, -3.0, 0.0, 12.0] {
        assert!(close(gain_to_db(db_to_gain(db)), db, 1e-10));
    }
    assert_eq!(gain_to_db(0.0), DB_FLOOR);
    assert_eq!(gain_to_db(f64::NAN), DB_FLOOR);
    assert_eq!(gain_to_db(1e-300), DB_FLOOR);
    assert!(close(gain_to_db(-0.5), gain_to_db(0.5), 0.0));
}

#[test]
fn one_pole_coefficients() {
    let fs = 44100.0;
    // Step response after tau seconds is 1 - 1/e.
    let a = one_pole_coeff_tau(0.01, fs);
    let mut y = 0.0;
    for _ in 0..441 {
        y += a * (1.0 - y);
    }
    assert!(close(y, 1.0 - (-1.0f64).exp(), 1e-3), "{y}");
    assert_eq!(one_pole_coeff_tau(0.0, fs), 1.0);
    // The Hz form is the tau form with tau = 1 / (2 pi fc).
    let fc = 100.0;
    assert!(close(
        one_pole_coeff_hz(fc, fs),
        one_pole_coeff_tau(1.0 / (std::f64::consts::TAU * fc), fs),
        1e-15
    ));
}

#[test]
fn fold_octave_into_range() {
    assert_eq!(fold_octave(40.0, 48.0, 60.0), 52.0);
    assert_eq!(fold_octave(75.0, 48.0, 60.0), 51.0);
    assert_eq!(fold_octave(48.0, 48.0, 60.0), 48.0);
    assert_eq!(fold_octave(60.0, 48.0, 60.0), 60.0);
    assert_eq!(fold_octave(54.5, 48.0, 72.0), 54.5);
    assert_eq!(fold_octave(36.0, 48.0, 60.0), 48.0);
    assert_eq!(fold_octave(84.0, 48.0, 60.0), 60.0);
    // Range narrower than an octave: the transposition nearer the centre.
    assert_eq!(fold_octave(58.0, 50.0, 52.0), 46.0); // centre 51: 46 is 5 away, 58 is 7
    assert_eq!(fold_octave(45.0, 50.0, 55.0), 57.0); // centre 52.5: 57 is 4.5 away, 45 is 7.5
    assert!(fold_octave(f64::NAN, 48.0, 60.0).is_nan());
}

// ---- fp

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn ftz_flushes_subnormals() {
    flush_denormals();
    assert_eq!(black_box(1e-310_f64) * black_box(1.0), 0.0);
    assert_eq!(black_box(1e-40_f32) * black_box(1.0), 0.0);
    // Normal numbers are untouched.
    assert_eq!(black_box(1e-300_f64) * black_box(1.0), 1e-300);
}

#[test]
fn pool_workers_flush_subnormals() {
    sfcore::fp::init_pool(Some(2));
    let r: Vec<f64> = (0..8)
        .map(|_| rayon::join(|| black_box(1e-310_f64) * black_box(1.0), || 0.0).0)
        .collect();
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    {
        // rayon::join from outside the pool runs on a worker.
        assert!(r.iter().all(|&x| x == 0.0), "{r:?}");
    }
    let _ = r;
}

// ---- random

fn mean_var(xs: impl Iterator<Item = f64>) -> (f64, f64) {
    let (mut n, mut s, mut s2) = (0.0, 0.0, 0.0);
    for x in xs {
        n += 1.0;
        s += x;
        s2 += x * x;
    }
    let m = s / n;
    (m, s2 / n - m * m)
}

fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        sab += (x - ma) * (y - mb);
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
    }
    sab / (saa * sbb).sqrt()
}

const T_A: Tag = tag("test.a");
const T_B: Tag = tag("test.b");

#[test]
fn fnv1a_known_values() {
    assert_eq!(tag("").0, 0xcbf2_9ce4_8422_2325);
    assert_eq!(tag("a").0, 0xaf63_dc4c_8601_ec8c);
    assert_eq!(tag("foobar").0, 0x8594_4171_f739_67e8);
    assert_ne!(T_A, T_B);
}

#[test]
fn uniform_moments() {
    let mut r = Rng::stream(1234, T_A);
    let (m, v) = mean_var((0..N).map(|_| r.uniform()));
    assert!(close(m, 0.5, 0.005), "mean {m}");
    assert!(close(v, 1.0 / 12.0, 0.01 / 12.0), "var {v}");
    let mut r = Rng::stream(1234, T_A);
    assert!((0..N).all(|_| {
        let x = r.uniform();
        (0.0..1.0).contains(&x)
    }));
    let mut r = Rng::stream(1234, T_A);
    assert!((0..N).all(|_| {
        let x = r.bipolar();
        (-1.0..1.0).contains(&x)
    }));
}

#[test]
fn gauss_moments() {
    let mut r = Rng::stream(2718, T_B);
    let (m, v) = mean_var((0..N).map(|_| r.gauss()));
    assert!(m.abs() < 0.01, "mean {m}");
    assert!(close(v, 1.0, 0.01), "var {v}");
}

#[test]
fn fill_bipolar_range_and_moments() {
    let mut r = Rng::stream(7, T_A);
    let mut buf = vec![0.0f32; N];
    r.fill_bipolar(&mut buf);
    assert!(buf.iter().all(|&x| (-1.0..1.0).contains(&x)));
    let (m, v) = mean_var(buf.iter().map(|&x| x as f64));
    assert!(m.abs() < 0.005, "mean {m}");
    assert!(close(v, 1.0 / 3.0, 0.01 / 3.0), "var {v}");
}

#[test]
fn below_and_pick() {
    let mut r = Rng::stream(99, T_A);
    let mut counts = [0usize; 7];
    for _ in 0..700_000 {
        counts[r.below(7) as usize] += 1;
    }
    for c in counts {
        assert!(close(c as f64, 100_000.0, 1500.0), "{counts:?}");
    }
    assert_eq!(r.below(0), 0);
    assert_eq!(r.below(1), 0);
    let empty: [u8; 0] = [];
    assert!(r.pick(&empty).is_none());
    let items = [10, 20, 30];
    for _ in 0..100 {
        assert!(items.contains(r.pick(&items).unwrap()));
    }
}

fn seq(mut r: Rng, n: usize) -> Vec<f64> {
    (0..n).map(|_| r.uniform()).collect()
}

#[test]
fn streams_uncorrelated() {
    let a = seq(Rng::stream(1234, T_A), N);
    let b = seq(Rng::stream(1234, T_B), N);
    let c = seq(Rng::stream(1235, T_A), N);
    let e0 = seq(Rng::event(1234, T_A, 0), N);
    let e1 = seq(Rng::event(1234, T_A, 1), N);
    let pairs = [
        ("tag", &a, &b),
        ("seed", &a, &c),
        ("index", &e0, &e1),
        ("stream/event", &a, &e0),
    ];
    for (name, x, y) in pairs {
        let r = corr(x, y);
        assert!(r.abs() < 0.01, "{name}: r = {r}");
    }
    // Neighbouring event streams also differ at the first draw across many indices.
    let firsts: Vec<f64> = (0..10_000)
        .map(|i| Rng::event(1234, T_A, i).uniform())
        .collect();
    let (m, v) = mean_var(firsts.iter().copied());
    assert!(
        close(m, 0.5, 0.01) && close(v, 1.0 / 12.0, 0.004),
        "{m} {v}"
    );
    let lag1 = corr(&firsts[..9_999], &firsts[1..]);
    assert!(lag1.abs() < 0.03, "lag1 {lag1}");
}

#[test]
fn streams_deterministic() {
    assert_eq!(
        seq(Rng::stream(5, T_A), 1000),
        seq(Rng::stream(5, T_A), 1000)
    );
    assert_eq!(
        seq(Rng::event(5, T_A, 3), 1000),
        seq(Rng::event(5, T_A, 3), 1000)
    );
    let mut g1 = Rng::event(5, T_B, 3);
    let mut g2 = Rng::event(5, T_B, 3);
    for _ in 0..1000 {
        assert_eq!(g1.gauss().to_bits(), g2.gauss().to_bits());
    }
    assert_ne!(
        seq(Rng::event(5, T_A, 3), 10),
        seq(Rng::event(5, T_A, 4), 10)
    );
}

// ---- time

#[test]
fn time_conversions() {
    assert_eq!(sample_at(0.0), 0);
    assert_eq!(sample_at(1.0), 44100);
    assert_eq!(sample_at(-1.0), -44100);
    assert_eq!(sample_at(0.5 / 44100.0), 1); // half rounds away from zero
    assert_eq!(sample_at(f64::NAN), 0);
    assert_eq!(len_samples(1.0), 44100);
    assert_eq!(len_samples(1.0 + 0.1 / 44100.0), 44101);
    assert_eq!(len_samples(-3.0), 0);
    assert_eq!(len_samples(f64::NAN), 0);
}

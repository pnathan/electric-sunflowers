//! Filter, smoother, dynamics and pan tests (design section 9): each test
//! states a design property, not a reference dump.

use dsp::biquad::{Biquad, BiquadCoeffs, Cascade, EqBand, EqKind, StereoBiquad};
use dsp::dynamics::{Compressor, GainComputer, Link, PeakDetector};
use dsp::onepole::{
    zero_phase_smooth, zero_phase_smooth_lanes, DcBlocker, OnePole, UnequalLengths,
};
use dsp::pan::{add_mono, balance, equal_power};
use dsp::resonator::{coeffs, Resonator};
use dsp::smoother::Ramp;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

const FS: f64 = 44_100.0;

/// xorshift64*: deterministic test noise in [-1, 1).
struct Noise(u64);
impl Noise {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

fn close(a: f64, b: f64, tol: f64, what: &str) {
    assert!((a - b).abs() <= tol, "{what}: {a} vs {b} (tol {tol})");
}

#[test]
fn lowpass_highpass_minus_3db_at_corner() {
    for &f in &[30.0, 90.0, 1000.0, 6500.0, 15000.0] {
        let lp = BiquadCoeffs::lowpass(FS, f, FRAC_1_SQRT_2);
        let hp = BiquadCoeffs::highpass(FS, f, FRAC_1_SQRT_2);
        close(lp.magnitude_db(f, FS), -3.0103, 0.05, "LP at f");
        close(hp.magnitude_db(f, FS), -3.0103, 0.05, "HP at f");
        close(lp.magnitude_db(1e-3, FS), 0.0, 1e-6, "LP at DC");
        close(
            hp.magnitude_db(FS / 2.0 - 1e-3, FS),
            0.0,
            1e-3,
            "HP at Nyquist",
        );
    }
}

#[test]
fn peaking_and_bandpass_gain_at_centre() {
    for &(f, q, db) in &[
        (250.0, 1.0, -1.5),
        (2900.0, 1.0, 1.5),
        (115.0, 0.9, 3.0),
        (5000.0, 2.0, -12.0),
    ] {
        let pk = BiquadCoeffs::peaking(FS, f, q, db);
        close(pk.magnitude_db(f, FS), db, 0.05, "peaking at f");
        close(pk.magnitude_db(1e-3, FS), 0.0, 0.05, "peaking at DC");
    }
    let bp = BiquadCoeffs::bandpass(FS, 1800.0, 6.0);
    close(bp.magnitude_db(1800.0, FS), 0.0, 0.05, "band-pass at f");
}

#[test]
fn shelf_gains_at_dc_and_nyquist() {
    for &db in &[-6.0, -2.0, 5.0, 7.0, 16.0] {
        let hs = BiquadCoeffs::high_shelf(FS, 5200.0, 0.7, db);
        close(hs.magnitude_db(FS / 2.0, FS), db, 0.05, "HS at Nyquist");
        close(hs.magnitude_db(0.0, FS), 0.0, 0.05, "HS at DC");
        close(hs.magnitude_db(5200.0, FS), db / 2.0, 0.05, "HS at f");
        let ls = BiquadCoeffs::low_shelf(FS, 300.0, 0.7, db);
        close(ls.magnitude_db(0.0, FS), db, 0.05, "LS at DC");
        close(ls.magnitude_db(FS / 2.0, FS), 0.0, 0.05, "LS at Nyquist");
    }
}

#[test]
fn design_clamps_bad_input() {
    for c in [
        BiquadCoeffs::lowpass(FS, 1e9, 0.7),
        BiquadCoeffs::highpass(FS, -5.0, 0.0),
        BiquadCoeffs::peaking(FS, f64::NAN, f64::INFINITY, f64::NAN),
        EqBand {
            kind: EqKind::HighShelf,
            f: 0.0,
            q: -1.0,
            db: 3.0,
        }
        .design(FS),
    ] {
        for v in [c.b0, c.b1, c.b2, c.a1, c.a2] {
            assert!(v.is_finite(), "{c:?}");
        }
    }
}

/// TDF-II against the direct form I difference equation with the same coefficients.
#[test]
fn tdf2_equals_direct_form() {
    let mut nz = Noise(0x9E37_79B9_7F4A_7C15);
    for c in [
        BiquadCoeffs::highpass(FS, 30.0, 0.7),
        BiquadCoeffs::peaking(FS, 2900.0, 1.0, 1.5),
        BiquadCoeffs::high_shelf(FS, 6500.0, 0.7, -5.0),
        BiquadCoeffs::bandpass(FS, 190.0, 6.0),
    ] {
        let mut bq = Biquad::new(c);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0, 0.0, 0.0);
        let mut worst = 0.0f64;
        for _ in 0..50_000 {
            let x = nz.next();
            let yd = c.b0 * x + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = yd;
            worst = worst.max((bq.tick(x) - yd).abs());
        }
        assert!(worst < 1e-9, "TDF-II vs DF-I {worst:e} for {c:?}");
    }
}

#[test]
fn process_matches_tick_and_keeps_state() {
    let c = BiquadCoeffs::lowpass(FS, 2000.0, FRAC_1_SQRT_2);
    let mut nz = Noise(7);
    let x: Vec<f32> = (0..3000).map(|_| nz.next() as f32).collect();
    let mut a = Biquad::new(c);
    let want: Vec<f32> = x.iter().map(|&v| a.tick(v as f64) as f32).collect();
    let mut b = Biquad::new(c);
    let mut got = x.clone();
    let (h1, h2) = got.split_at_mut(1234);
    b.process(h1);
    b.process(h2);
    assert_eq!(got, want);

    let mut st = StereoBiquad::new(c);
    let (mut l, mut r) = (x.clone(), vec![0.0f32; x.len()]);
    st.process_stereo(&mut l, &mut r);
    assert_eq!(l, want);
    assert!(r.iter().all(|&v| v == 0.0));

    let mut cas = Cascade::new([c, BiquadCoeffs::IDENTITY]);
    let mut y = x.clone();
    cas.process(&mut y);
    assert_eq!(y, want);
    close(cas.magnitude_db(2000.0, FS), -3.0103, 0.05, "cascade");
}

#[test]
fn impulse_decays_and_flush_zeroes_state() {
    let mut bq = Biquad::new(BiquadCoeffs::highpass(FS, 30.0, 0.7));
    bq.tick(1.0);
    let mut last = 0.0;
    for _ in 0..FS as usize * 2 {
        last = bq.tick(0.0);
    }
    assert!(last.abs() < 1e-6, "30 Hz HP tail after 2 s {last:e}");
    for _ in 0..FS as usize * 30 {
        bq.tick(0.0);
    }
    bq.flush_denormals();
    assert_eq!(bq.tick(0.0), 0.0);
}

#[test]
fn onepole_time_constant() {
    for &tau in &[0.002, 0.009, 0.15] {
        let mut lp = OnePole::from_tau(tau, FS);
        let n = (tau * FS).round() as usize;
        let mut y = 0.0;
        for _ in 0..n {
            y = lp.tick(1.0);
        }
        let want = 1.0 - (-(n as f64) / (tau * FS)).exp();
        assert!(
            (y - 0.632_12).abs() < 0.01 * 0.632_12,
            "step at tau {tau}: {y}"
        );
        close(y, want, 1e-9, "exact step");
    }
    let mut hp = OnePole::from_hz(100.0, FS);
    let mut y = 1.0;
    for _ in 0..44_100 {
        y = hp.tick_hp(1.0);
    }
    assert!(y.abs() < 1e-9);
}

#[test]
fn dc_blocker_removes_dc() {
    let mut d = DcBlocker::new(0.995);
    let mut y = 1.0;
    for _ in 0..20_000 {
        y = d.tick(0.5);
    }
    assert!(y.abs() < 1e-9, "{y}");
}

#[test]
fn zero_phase_smooth_is_symmetric_and_keeps_constants() {
    let mut c = vec![0.3f32; 50];
    zero_phase_smooth(&mut c, 0.2);
    assert!(c.iter().all(|&v| (v - 0.3).abs() < 1e-7));
    // A centred pulse stays centred: the smoothed track is symmetric about it
    // apart from the forward pass's edge seed.
    let mut p = vec![0.0f32; 201];
    p[100] = 1.0;
    zero_phase_smooth(&mut p, 0.1);
    for k in 1..60 {
        assert!(
            (p[100 - k] - p[100 + k]).abs() < 1e-6,
            "k {k}: {} {}",
            p[100 - k],
            p[100 + k]
        );
    }
    let mut e: Vec<f32> = vec![];
    zero_phase_smooth(&mut e, 0.5);
}

#[test]
fn zero_phase_smooth_lanes_equals_separate_calls() {
    let a: Vec<f32> = (0..300)
        .map(|i| ((i * 37 % 101) as f32 / 50.0) - 1.0)
        .collect();
    let b: Vec<f32> = (0..300)
        .map(|i| ((i * 11 % 53) as f32 / 26.0) - 1.0)
        .collect();
    let (mut a1, mut b1) = (a.clone(), b.clone());
    zero_phase_smooth(&mut a1, 0.2);
    zero_phase_smooth(&mut b1, 0.05);
    let (mut a2, mut b2) = (a.clone(), b.clone());
    assert_eq!(
        zero_phase_smooth_lanes([&mut a2, &mut b2], [0.2, 0.05]),
        Ok(())
    );
    assert!(a1.iter().zip(&a2).all(|(x, y)| x.to_bits() == y.to_bits()));
    assert!(b1.iter().zip(&b2).all(|(x, y)| x.to_bits() == y.to_bits()));
    // Unequal lengths: an error, and no track changes.
    let (mut a3, mut short) = (a.clone(), b[..299].to_vec());
    assert_eq!(
        zero_phase_smooth_lanes([&mut a3, &mut short], [0.2, 0.05]),
        Err(UnequalLengths)
    );
    assert_eq!(a3, a);
}

/// Peak and -3 dB bandwidth of the Klatt resonator, by its z-domain response.
fn res_mag(a: f64, b: f64, c: f64, f: f64) -> f64 {
    let w = 2.0 * PI * f / FS;
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let dr = 1.0 - b * c1 - c * c2;
    let di = b * s1 + c * s2;
    a / (dr * dr + di * di).sqrt()
}

#[test]
fn resonator_unity_dc_peak_and_bandwidth() {
    for &(f, bw) in &[
        (300.0, 60.0),
        (700.0, 80.0),
        (2200.0, 120.0),
        (3000.0, 200.0),
        (5500.0, 250.0),
    ] {
        let (a, b, c) = coeffs(f, bw, FS);
        close(res_mag(a, b, c, 0.0), 1.0, 1e-12, "DC gain");
        // Time domain DC gain too.
        let mut r = Resonator::new(f, bw, FS);
        let mut y = 0.0;
        for _ in 0..20_000 {
            y = r.tick(1.0);
        }
        close(y, 1.0, 1e-9, "step settles at 1");
        // Scan on a 0.1 Hz grid.
        let grid: Vec<(f64, f64)> = (1..80_000)
            .map(|k| k as f64 * 0.1)
            .map(|g| (g, res_mag(a, b, c, g)))
            .collect();
        let (fp, mp) = grid
            .iter()
            .copied()
            .fold((0.0, 0.0), |m, v| if v.1 > m.1 { v } else { m });
        assert!((fp - f).abs() < 0.01 * f, "peak {fp} for f {f}");
        let half = mp * FRAC_1_SQRT_2;
        let lo = grid
            .iter()
            .rev()
            .find(|v| v.0 < fp && v.1 < half)
            .map(|v| v.0)
            .unwrap();
        let hi = grid
            .iter()
            .find(|v| v.0 > fp && v.1 < half)
            .map(|v| v.0)
            .unwrap();
        let meas = hi - lo;
        assert!(
            (meas - bw).abs() < 0.05 * bw,
            "bandwidth {meas} for bw {bw} at f {f}"
        );
    }
}

#[test]
fn ramped_resonator_stays_bounded() {
    let mut nz = Noise(42);
    let mut r = Resonator::new(250.0, 40.0, FS);
    let mut peak = 0.0f64;
    for i in 0..400 {
        // Alternate between extreme stable designs: narrow and wide, low and high.
        let (f, bw) = if i % 2 == 0 {
            (60.0 + 5000.0 * nz.next().abs(), 20.0)
        } else {
            (4000.0 + 12000.0 * nz.next().abs(), 400.0)
        };
        r.set_ramped(coeffs(f, bw, FS), 64);
        for _ in 0..64 {
            let y = r.tick(0.1 * nz.next());
            assert!(y.is_finite());
            peak = peak.max(y.abs());
        }
    }
    assert!(peak < 50.0, "peak {peak}");
    // The ramp ends on the target exactly.
    let t = coeffs(1000.0, 90.0, FS);
    r.set_ramped(t, 10);
    for _ in 0..10 {
        r.tick(0.0);
    }
    assert_eq!((r.a, r.b, r.c), t);
}

#[test]
fn ramp_reaches_target() {
    let mut r = Ramp::new(0.0);
    r.set_target(1.0, 4);
    let v: Vec<f64> = (0..6).map(|_| r.next()).collect();
    assert_eq!(v, vec![0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
    r.set_target(-2.0, 0);
    assert_eq!(r.next(), -2.0);
    r.set_target(0.1, 3);
    for _ in 0..3 {
        r.next();
    }
    assert_eq!(r.value, 0.1);
}

fn comp(ratio: f64, knee: f64, link: Link) -> Compressor {
    Compressor::new(
        PeakDetector::new(0.008, 0.15, FS),
        GainComputer {
            thr_db: -19.0,
            ratio,
            knee_db: knee,
        },
        link,
    )
}

#[test]
fn compressor_static_curve_equals_gain_computer() {
    for &level in &[0.01f64, 0.08, 0.11, 0.14, 0.3, 0.9] {
        let mut c = comp(3.0, 6.0, Link::Mono);
        let mut x = vec![level as f32; 44_100];
        c.process_mono(&mut x);
        let got = 20.0 * (x[x.len() - 1] as f64 / level as f32 as f64).log10();
        let want = c.gc.gain_db(20.0 * (level as f32 as f64 + 1e-9).log10());
        close(got, want, 1e-3, "mono static gain");

        let mut c = comp(2.2, 10.0, Link::StereoMax);
        let mut l = vec![level as f32; 44_100];
        let mut r = vec![-(level as f32) * 0.5; 44_100];
        c.process_stereo(&mut l, &mut r);
        let got_l = 20.0 * (l[l.len() - 1] as f64 / level as f32 as f64).log10();
        let got_r = 20.0 * (r[r.len() - 1] as f64 / (-(level as f32) * 0.5) as f64).log10();
        let want = c.gc.gain_db(20.0 * (level as f32 as f64 + 1e-9).log10());
        close(got_l, want, 1e-3, "linked static gain L");
        close(got_r, want, 1e-3, "linked static gain R");
    }
}

#[test]
fn gain_computer_knee_is_continuous() {
    for &w in &[0.0, 6.0, 10.0] {
        let gc = GainComputer {
            thr_db: -20.0,
            ratio: 3.0,
            knee_db: w,
        };
        let mut prev = gc.gain_db(-60.0);
        let mut l = -60.0;
        while l < 20.0 {
            l += 0.001;
            let g = gc.gain_db(l);
            assert!((g - prev).abs() < 0.001, "jump at {l} knee {w}");
            assert!(g <= 0.0);
            prev = g;
        }
        close(
            gc.gain_db(-20.0 + 20.0),
            -20.0 * (1.0 - 1.0 / 3.0),
            1e-12,
            "above knee",
        );
        assert_eq!(gc.gain_db(-40.0), 0.0);
    }
}

#[test]
fn compressor_gain_has_no_steps() {
    // A level jump: the per-sample gain moves at most one period's change
    // spread over 16 samples.
    let mut c = comp(4.0, 6.0, Link::Mono);
    let mut x = vec![0.02f32; 4000];
    x[2000..].iter_mut().for_each(|v| *v = 0.8);
    let orig = x.clone();
    c.process_mono(&mut x);
    let g: Vec<f64> = x
        .iter()
        .zip(&orig)
        .map(|(a, b)| *a as f64 / *b as f64)
        .collect();
    let max_step = g
        .windows(2)
        .skip(2001)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f64::max);
    assert!(max_step < 0.01, "max per-sample gain step {max_step}");
}

#[test]
fn equal_power_sums_to_unit_power() {
    for k in 0..=40 {
        let p = -1.0 + k as f64 / 20.0;
        let [l, r] = equal_power(p);
        close((l * l + r * r) as f64, 1.0, 1e-6, "power");
    }
    assert!(equal_power(-1.0)[1].abs() < 1e-7);
    close(equal_power(0.0)[0] as f64, FRAC_1_SQRT_2, 1e-7, "centre");
}

#[test]
fn balance_law() {
    assert_eq!(balance(0.0)[0][0], 1.0);
    assert!(
        balance(0.0)[0][1].abs() < 1e-7
            && balance(0.0)[1][0] == 0.0
            && (balance(0.0)[1][1] - 1.0).abs() < 1e-7
    );
    // Hard left: both channels to the left.
    let m = balance(-1.0);
    assert_eq!(m, [[1.0, 1.0], [0.0, 0.0]]);
    let m = balance(1.0);
    assert!(m[0][0].abs() < 1e-7 && m[1][0] == 1.0 && m[1][1] == 1.0);
    // Far channel energy is conserved across the fold.
    for p in [-0.4, 0.34, 0.45] {
        let m = balance(p);
        let far = if p <= 0.0 {
            m[0][1].powi(2) + m[1][1].powi(2)
        } else {
            m[0][0].powi(2) + m[1][0].powi(2)
        };
        close(far as f64, 1.0, 1e-6, "far channel power");
    }
}

#[test]
fn add_mono_clips_both_ends() {
    let (mut l, mut r) = (vec![0.0f32; 8], vec![0.0f32; 8]);
    add_mono(&mut l, &mut r, -2, &[1.0, 2.0, 3.0, 4.0], [0.5, 1.0]);
    assert_eq!(l[..3], [1.5, 2.0, 0.0]);
    assert_eq!(r[..2], [3.0, 4.0]);
    add_mono(&mut l, &mut r, 6, &[1.0, 1.0, 1.0, 1.0], [1.0, 1.0]);
    assert_eq!(l[5..], [0.0, 1.0, 1.0]);
    add_mono(&mut l, &mut r, 100, &[1.0], [1.0, 1.0]);
    add_mono(&mut l, &mut r, -100, &[1.0], [1.0, 1.0]);
    add_mono(&mut l, &mut r, isize::MIN, &[1.0], [1.0, 1.0]);
}

#[test]
fn coefficients_come_from_sfcore_math() {
    use sfcore::math::{db_to_gain, gain_to_db, one_pole_coeff_tau};
    // OnePole a equals the sfcore coefficient; a bad tau gives a = 1.
    for tau in [1e-4, 0.005, 0.1, 2.0] {
        assert_eq!(
            OnePole::from_tau(tau, FS).a,
            one_pole_coeff_tau(tau, FS),
            "tau {tau}"
        );
    }
    for tau in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(OnePole::from_tau(tau, FS).a, 1.0, "tau {tau}");
    }
    // Detector pole k = 1 - a; a bad time is instant (k = 0).
    let d = PeakDetector::new(0.01, 0.2, FS);
    close(
        d.attack_coef,
        1.0 - one_pole_coeff_tau(0.01, FS),
        1e-15,
        "attack pole",
    );
    close(
        d.release_coef,
        1.0 - one_pole_coeff_tau(0.2, FS),
        1e-15,
        "release pole",
    );
    let d = PeakDetector::new(f64::NAN, f64::INFINITY, FS);
    assert_eq!((d.attack_coef, d.release_coef), (0.0, 0.0));
    // Level in dB: sfcore gain_to_db, with its floor for silence.
    let mut c = comp(4.0, 6.0, Link::Mono);
    assert_eq!(c.level_db(), gain_to_db(0.0));
    let mut x = vec![0.5f32; 4096];
    c.process_mono(&mut x);
    assert_eq!(c.level_db(), gain_to_db(c.det.env));
    close(db_to_gain(c.level_db()), c.det.env, 1e-12, "dB round trip");
}

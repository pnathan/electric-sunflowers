//! Delay lines, allpasses and stochastic processes.

use dsp::delay::{one_pole_phase_delay, DelayLine, SchroederAllpass, Thiran1};
use dsp::stochastic::{OuProcess, RandomWalk};
use std::f64::consts::PI;

/// Test-local normal source: SplitMix64 (Steele, Lea, Flood 2014) and
/// Box-Muller, so these tests do not depend on the engine generator.
struct Normal {
    s: u64,
}

impl Normal {
    fn new(seed: u64) -> Self {
        Normal { s: seed }
    }
    fn uniform(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let u = self.uniform();
        let v = self.uniform();
        (-2.0 * u.ln()).sqrt() * (2.0 * PI * v).cos()
    }
}

#[test]
fn read_int_returns_sample_pushed_d_steps_ago() {
    let mut l = DelayLine::new(100);
    assert!(l.capacity().is_power_of_two() && l.capacity() >= 103);
    for n in 0..1000 {
        l.push(n as f32);
        for d in [0usize, 1, 7, 50, 100] {
            if d <= n {
                assert_eq!(l.read_int(d), (n - d) as f32);
            }
        }
    }
    l.clear();
    assert_eq!(l.read_int(0), 0.0);
    assert_eq!(l.read_int(100), 0.0);
}

#[test]
fn fractional_reads_of_slow_sine() {
    let w = 2.0 * PI * 50.0 / 44100.0;
    let mut l = DelayLine::new(64);
    let n_total = 2000usize;
    let mut worst_lin = 0.0f64;
    let mut worst_lag = 0.0f64;
    for n in 0..n_total {
        l.push((w * n as f64).sin() as f32);
        if n < 100 {
            continue;
        }
        for &d in &[1.0, 1.25, 1.5, 3.77, 10.1, 33.5, 60.9] {
            let want = (w * (n as f64 - d)).sin();
            worst_lin = worst_lin.max((l.read_linear(d) - want).abs());
            worst_lag = worst_lag.max((l.read_lagrange3(d) - want).abs());
        }
    }
    eprintln!("slow sine max error: linear {worst_lin:.2e}, lagrange3 {worst_lag:.2e}");
    assert!(worst_lin < 1e-4, "linear {worst_lin}");
    assert!(worst_lag < 1e-4, "lagrange3 {worst_lag}");
}

/// RMS gain of a sine at `w` read at delay `d` in steady state.
fn gain_at(w: f64, d: f64, lagrange: bool) -> f64 {
    let mut l = DelayLine::new(32);
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for n in 0..4096 {
        let x = (w * n as f64 + 0.3).sin();
        l.push(x as f32);
        if n >= 64 {
            let y = if lagrange { l.read_lagrange3(d) } else { l.read_linear(d) };
            sx += x * x;
            sy += y * y;
        }
    }
    (sy / sx).sqrt()
}

#[test]
fn lagrange3_flatter_than_linear_at_quarter_rate() {
    let w = PI / 2.0;
    for &d in &[4.25, 4.5, 4.75] {
        let glin = gain_at(w, d, false);
        let glag = gain_at(w, d, true);
        eprintln!("0.25 fs, d {d}: linear {glin:.4}, lagrange3 {glag:.4}");
        assert!((1.0 - glag).abs() < (1.0 - glin).abs());
    }
    // Analytic value at fraction 0.5: 2 (0.5625 - 0.0625) cos(pi/4).
    assert!((gain_at(w, 4.5, true) - 1.25 * (PI / 4.0).cos()).abs() < 2e-3);
}

#[test]
fn thiran1_phase_delay_matches_delta() {
    for &delta in &[0.5, 0.73, 1.0, 1.29, 1.49] {
        let t = Thiran1::new(delta);
        let pd = t.phase_delay(1e-3);
        assert!((pd - delta).abs() < 1e-3, "delta {delta}: phase delay {pd}");
        assert!((t.phase_delay(0.0) - delta).abs() < 1e-12);

        // Simulated: a low sine through the filter lags by delta samples.
        let w = 0.01;
        let mut f = Thiran1::new(delta);
        let mut worst = 0.0f64;
        for n in 0..5000 {
            let y = f.tick((w * n as f64).sin());
            if n > 200 {
                worst = worst.max((y - (w * (n as f64 - delta)).sin()).abs());
            }
        }
        // A phase error e gives an amplitude error of about w * e.
        assert!(worst < w * 1e-3, "delta {delta}: simulated error {worst}");
    }
}

#[test]
fn thiran1_range_clamps() {
    // `new` clamps to [0.5, 1.5]; `with_max` widens the top.
    assert!((Thiran1::new(3.0).phase_delay(0.0) - 1.5).abs() < 1e-12);
    assert!((Thiran1::new(0.1).phase_delay(0.0) - 0.5).abs() < 1e-12);
    let mut t = Thiran1::with_max(3.0, 3.5);
    assert!((t.phase_delay(0.0) - 3.0).abs() < 1e-12);
    t.set_delay(9.0);
    assert!((t.phase_delay(0.0) - 3.5).abs() < 1e-12);
    t.set_delay(f64::NAN);
    assert!((t.phase_delay(0.0) - 1.0).abs() < 1e-12);
}

#[test]
fn one_pole_phase_delay_limits() {
    let a: f64 = 0.2;
    assert!((one_pole_phase_delay(a, 0.0) - 0.25).abs() < 1e-12);
    assert!((one_pole_phase_delay(a, 1e-4) - 0.25).abs() < 1e-6);
    // The exact form at w: atan2(a sin w, 1 - a cos w) / w.
    let w: f64 = 0.3;
    let want = (a * w.sin()).atan2(1.0 - a * w.cos()) / w;
    assert!((one_pole_phase_delay(a, w) - want).abs() < 1e-15);
}

#[test]
fn schroeder_allpass_unit_energy() {
    for &(m, g) in &[(142usize, 0.65f32), (379, 0.62), (107, 0.58), (1, 0.5), (29, -0.7)] {
        let mut ap = SchroederAllpass::new(m, g);
        let mut e = 0.0f64;
        for n in 0..60000 {
            let y = ap.tick(if n == 0 { 1.0 } else { 0.0 }) as f64;
            e += y * y;
        }
        assert!((e - 1.0).abs() < 1e-3, "M {m} g {g}: energy {e}");
        // First output is -g, first echo at M.
        let mut ap = SchroederAllpass::new(m, g);
        assert!((ap.tick(1.0) + g).abs() < 1e-7);
    }
}

#[test]
fn ou_stationary_variance() {
    // Coarse step (dt = tau / 2): exact discretisation holds at any dt, where
    // Euler-Maruyama would give variance 1 / (1 - 0.25) = 1.33.
    for &(theta, sigma, dt) in &[(2.0, 0.7, 0.25), (1.0 / 0.12, (2.0f64 / 0.12).sqrt(), 16.0 / 44100.0)] {
        let mut p = OuProcess::new(0.3, theta, sigma, dt);
        let mut g = Normal::new(42);
        let n = if dt > 0.1 { 400_000 } else { 8_000_000 };
        let (mut s, mut s2) = (0.0f64, 0.0f64);
        for _ in 0..n {
            let x = p.tick(|| g.gauss());
            s += x;
            s2 += x * x;
        }
        let mean = s / n as f64;
        let var = s2 / n as f64 - mean * mean;
        let want = sigma * sigma / (2.0 * theta);
        eprintln!("OU theta {theta:.3} dt {dt:.2e}: mean {mean:.4}, var {var:.4}, want {want:.4}");
        assert!((var / want - 1.0).abs() < 0.05);
        assert!((p.stationary_variance() - want).abs() < 1e-12);
    }
    // Degenerate parameters freeze at the mean instead of producing NaN.
    let mut p = OuProcess::new(0.5, 0.0, 1.0, 0.01);
    assert_eq!(p.tick(|| 1.0), 0.5);
    let mut p = OuProcess::unit(0.25, f64::NAN);
    assert_eq!(p.tick(|| 1.0), 0.0);
}

#[test]
fn random_walk_stays_within_limit() {
    let mut g = Normal::new(7);
    let mut walks = [
        RandomWalk::bounded(0.02, 0.97, 0.01, 0.02),
        RandomWalk::bounded(0.0004, 0.97, 0.05, 0.0015),
        RandomWalk::leaky(0.004, 0.985, 0.998, 0.12),
    ];
    for wk in walks.iter_mut() {
        let lim = wk.limit;
        let mut hit = 0usize;
        for _ in 0..200_000 {
            let y = wk.tick(|| g.gauss());
            assert!(y.is_finite() && y.abs() <= lim);
            if y.abs() >= lim * 0.999 {
                hit += 1;
            }
        }
        // The walk moves: it reaches the wall at least sometimes.
        assert!(hit > 0, "limit {lim} never reached");
    }
}

#[test]
fn step_draws_from_engine_rng() {
    use sfcore::random::{tag, Rng};
    let mut r = Rng::stream(1234, tag("test.walk"));
    let mut wk = RandomWalk::leaky(0.004, 0.985, 0.998, 0.12);
    let mut ou = OuProcess::unit(0.25, 16.0 / 44100.0);
    for _ in 0..10_000 {
        assert!(wk.step(&mut r).abs() <= 0.12);
        assert!(ou.step(&mut r).is_finite());
    }
}

#[test]
fn fractional_reads_clamp_bad_delays() {
    let mut line = DelayLine::new(13);
    let cap = line.capacity();
    for n in 0..cap {
        line.push(n as f32 + 1.0);
    }
    let top = (cap - 3) as f64;
    // Huge, infinite and past-capacity delays read as capacity - 3.
    for d in [f64::INFINITY, 1e300, usize::MAX as f64, top + 0.5, cap as f64] {
        assert_eq!(line.read_linear(d), line.read_int(cap - 3) as f64, "linear {d}");
        assert_eq!(line.read_lagrange3(d), line.read_int(cap - 3) as f64, "lagrange {d}");
    }
    // NaN and negative delays read the lower limit (0 linear, 1 Lagrange).
    for d in [f64::NAN, f64::NEG_INFINITY, -5.0] {
        assert_eq!(line.read_linear(d), line.read_int(0) as f64, "linear {d}");
        assert_eq!(line.read_lagrange3(d), line.read_int(1) as f64, "lagrange {d}");
    }
    // In range, the clamp does nothing: integer delays are exact.
    for i in 1..=cap - 3 {
        assert_eq!(line.read_lagrange3(i as f64), line.read_int(i) as f64, "lagrange {i}");
        assert_eq!(line.read_linear(i as f64), line.read_int(i) as f64, "linear {i}");
    }
}

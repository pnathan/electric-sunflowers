//! Drum voices: kick sweep end frequency, ride band-limiting against a naive
//! square cluster, and finite, bounded output for every kind.

use dsp::biquad::{Biquad, BiquadCoeffs};
use dsp::fft::{RealFft, C32};
use instruments::drums::{
    blep_square, hit_len, render_drums, render_hit, DrumScratch, DRUM_HIT, RIDE_BASE_HZ, RIDE_SCALE,
};
use sfcore::random::Rng;
use sfcore::SR_F;
use song::events::{DrumHit, DrumKind};

const ALL: [DrumKind; 10] = [
    DrumKind::Kick,
    DrumKind::Snare,
    DrumKind::Rim,
    DrumKind::Tap,
    DrumKind::Swish { dur: 0.5 },
    DrumKind::Hat,
    DrumKind::Shaker,
    DrumKind::Tom { hz: 110.0 },
    DrumKind::Tom { hz: 180.0 },
    DrumKind::Ride,
];

fn hit(kind: DrumKind) -> DrumHit {
    DrumHit {
        t: 0.0,
        kind,
        vel: 1.0,
        pan: -1.0,
    }
}

/// Mono voice at unit velocity: hard left, so the left gain is 1.
fn mono(kind: DrumKind, seed: u64) -> Vec<f32> {
    let n = hit_len(kind);
    let [l, _] = render_drums(&[hit(kind)], seed, n);
    l
}

#[test]
fn kick_sweep_ends_near_47_hz() {
    let x = mono(DrumKind::Kick, 1);
    // Zero crossings (upward) between 0.2 s and the end.
    let a = (0.2 * SR_F) as usize;
    let ups: Vec<f64> = (a + 1..x.len())
        .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
        .map(|i| {
            let (p, q) = (x[i - 1] as f64, x[i] as f64);
            (i - 1) as f64 + p / (p - q)
        })
        .collect();
    assert!(ups.len() >= 3, "{} crossings", ups.len());
    let per = (ups[ups.len() - 1] - ups[0]) / (ups.len() - 1) as f64;
    let f = SR_F / per;
    println!("kick tail frequency {f:.2} Hz");
    assert!((f - 47.0).abs() < 1.0, "kick ends at {f} Hz");
}

/// The ride with naive (aliasing) squares: same draws as the voice
/// (six uniform start phases, then the ping noise), same filters and gains.
fn naive_ride(seed: u64) -> Vec<f32> {
    let n = hit_len(DrumKind::Ride);
    let mut rng = Rng::event(seed, DRUM_HIT, 0);
    let mut ph = [0.0f64; 6];
    for p in ph.iter_mut() {
        *p = rng.uniform();
    }
    let dt = RIDE_BASE_HZ.map(|f| f * RIDE_SCALE / SR_F);
    let mut out = vec![0.0f32; n];
    for v in out.iter_mut() {
        let mut acc = 0.0;
        for k in 0..6 {
            ph[k] += dt[k];
            if ph[k] >= 1.0 {
                ph[k] -= 1.0;
            }
            acc += if ph[k] < 0.5 { 1.0 } else { -1.0 };
        }
        *v = (acc / 6.0) as f32;
    }
    Biquad::new(BiquadCoeffs::highpass(SR_F, 3500.0, 0.7)).process(&mut out);
    let mut ping = vec![0.0f32; n];
    rng.fill_bipolar(&mut ping);
    Biquad::new(BiquadCoeffs::bandpass(SR_F, 5200.0, 4.0)).process(&mut ping);
    for (i, (v, p)) in out.iter_mut().zip(&ping).enumerate() {
        let t = i as f64 / SR_F;
        *v = (*v as f64 * 0.35 * (-t / 0.7).exp() + *p as f64 * 0.6 * (-t / 0.04).exp()) as f32;
    }
    out
}

/// Energy of `x` (first 65536 samples, Hann window) in [f_lo, f_hi).
fn energy_in(x: &[f32], f_lo: f64, f_hi: f64) -> f64 {
    let n = 65536;
    let mut buf: Vec<f32> = (0..n)
        .map(|i| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            x.get(i).copied().unwrap_or(0.0) * w as f32
        })
        .collect();
    let fft = RealFft::new(n);
    let mut spec = vec![C32::new(0.0, 0.0); fft.spectrum_len()];
    let mut scratch = fft.make_scratch();
    fft.forward(&mut buf, &mut spec, &mut scratch)
        .expect("fft sizes");
    let k0 = (f_lo / SR_F * n as f64).ceil() as usize;
    let k1 = ((f_hi / SR_F * n as f64).ceil() as usize).min(spec.len());
    spec[k0..k1].iter().map(|c| c.norm_sqr() as f64).sum()
}

/// The ride's square cluster three ways over 65536 samples: naive, PolyBLEP
/// (the voice's `blep_square`), and the additive band-limited ideal (odd
/// harmonics `4 / (pi h)` below Nyquist). Returns the aliasing (difference
/// from the ideal) of the naive and PolyBLEP clusters.
fn cluster_aliasing(phase0: [f64; 6]) -> (Vec<f32>, Vec<f32>) {
    let n = 65536;
    let dt = RIDE_BASE_HZ.map(|f| f * RIDE_SCALE / SR_F);
    let mut naive_err = vec![0.0f32; n];
    let mut blep_err = vec![0.0f32; n];
    for k in 0..6 {
        let (mut pn, mut pb) = (phase0[k], phase0[k]);
        let n_harm = ((0.5 / dt[k] - 1.0) / 2.0).floor() as usize + 1;
        for i in 0..n {
            pn += dt[k];
            if pn >= 1.0 {
                pn -= 1.0;
            }
            let naive = if pn < 0.5 { 1.0 } else { -1.0 };
            let blep = blep_square(&mut pb, dt[k]);
            let mut ideal = 0.0;
            for j in 0..n_harm {
                let h = (2 * j + 1) as f64;
                ideal += (4.0 / (std::f64::consts::PI * h))
                    * (2.0 * std::f64::consts::PI * h * pn).sin();
            }
            naive_err[i] += ((naive - ideal) / 6.0) as f32;
            blep_err[i] += ((blep - ideal) / 6.0) as f32;
        }
    }
    (naive_err, blep_err)
}

#[test]
fn ride_polyblep_cuts_aliasing() {
    let (naive, blep) = cluster_aliasing([0.1, 0.3, 0.5, 0.7, 0.9, 0.2]);
    let db =
        |lo: f64, hi: f64| 10.0 * (energy_in(&naive, lo, hi) / energy_in(&blep, lo, hi)).log10();
    let low = db(0.0, 3500.0);
    let mid = db(3500.0, 10_000.0);
    println!("ride cluster aliasing, naive / PolyBLEP: below 3.5 kHz {low:.1} dB, 3.5-10 kHz {mid:.1} dB");
    assert!(low >= 15.0, "aliasing below 3.5 kHz only {low:.1} dB lower");
    assert!(mid >= 8.0, "aliasing 3.5-10 kHz only {mid:.1} dB lower");
}

#[test]
fn ride_hit_energy_above_15k_below_naive() {
    for seed in [1u64, 2, 3] {
        let blep = mono(DrumKind::Ride, seed);
        let naive = naive_ride(seed);
        let db =
            10.0 * (energy_in(&naive, 15_000.0, SR_F) / energy_in(&blep, 15_000.0, SR_F)).log10();
        println!("seed {seed}: ride energy above 15 kHz, naive / PolyBLEP = {db:.1} dB");
        assert!(db >= 5.0, "only {db:.1} dB below the naive squares");
    }
}

#[test]
fn every_kind_is_finite_and_bounded() {
    let mut scratch = DrumScratch::new();
    for (j, &kind) in ALL.iter().enumerate() {
        for seed in 0..8u64 {
            let n = hit_len(kind);
            let mut l = vec![0.0f32; n];
            let mut r = vec![0.0f32; n];
            let mut rng = Rng::event(seed, DRUM_HIT, j as u64);
            render_hit(
                &DrumHit {
                    t: 0.0,
                    kind,
                    vel: 1.0,
                    pan: 0.0,
                },
                &mut rng,
                &mut l,
                &mut r,
                &mut scratch,
            );
            // Undo the centre pan gain to get the unit-velocity voice.
            let g = std::f32::consts::FRAC_1_SQRT_2;
            let peak = l.iter().map(|v| (v / g).abs()).fold(0.0f32, f32::max);
            assert!(
                l.iter().chain(&r).all(|v| v.is_finite()),
                "{kind:?} not finite"
            );
            assert!(peak <= 1.5, "{kind:?} seed {seed} peak {peak}");
            assert!(peak > 0.01, "{kind:?} seed {seed} silent");
        }
    }
}

#[test]
fn bad_input_does_not_panic() {
    let mut l = vec![0.0f32; 1000];
    let mut r = vec![0.0f32; 1000];
    let mut scratch = DrumScratch::new();
    let mut rng = Rng::from_seed(1);
    let odd = [
        DrumHit {
            t: -0.3,
            kind: DrumKind::Ride,
            vel: 1.0,
            pan: 5.0,
        },
        DrumHit {
            t: 1e12,
            kind: DrumKind::Kick,
            vel: 1.0,
            pan: 0.0,
        },
        DrumHit {
            t: f64::NAN,
            kind: DrumKind::Kick,
            vel: 1.0,
            pan: 0.0,
        },
        DrumHit {
            t: 0.0,
            kind: DrumKind::Swish { dur: f32::INFINITY },
            vel: 1.0,
            pan: f32::NAN,
        },
        DrumHit {
            t: 0.0,
            kind: DrumKind::Swish { dur: -1.0 },
            vel: f32::NAN,
            pan: 0.0,
        },
        DrumHit {
            t: 0.0,
            kind: DrumKind::Tom { hz: f32::NAN },
            vel: 1.0,
            pan: 0.0,
        },
        DrumHit {
            t: 0.0,
            kind: DrumKind::Tom { hz: 1e9 },
            vel: 1.0,
            pan: 0.0,
        },
    ];
    for h in &odd {
        render_hit(h, &mut rng, &mut l, &mut r, &mut scratch);
    }
    assert!(l.iter().chain(&r).all(|v| v.is_finite()));
}

#[test]
fn render_drums_places_hits_and_pans() {
    let len = SR_F as usize;
    let hits = [DrumHit {
        t: 0.5,
        kind: DrumKind::Hat,
        vel: 0.5,
        pan: 1.0,
    }];
    let [l, r] = render_drums(&hits, 9, len);
    let at = (0.5 * SR_F) as usize;
    assert!(r[..at].iter().all(|v| *v == 0.0));
    assert!(r[at..].iter().any(|v| *v != 0.0));
    assert!(l.iter().all(|v| v.abs() < 1e-6), "hard right leaks left");
    // Same seed, same output.
    assert_eq!(render_drums(&hits, 9, len), [l, r]);
}

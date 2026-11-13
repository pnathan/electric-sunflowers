//! Drum voices: determinism, variation between hits, brightness against
//! velocity, band curves against the reference tables, levels, and finite,
//! bounded output for every kind.

use dsp::fft::{RealFft, C32};
use instruments::drum_data::{self as data, VoiceData, BANDS};
use instruments::drums::{hit_len, render_drums, render_hit, DrumScratch, DRUM_HIT};
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

fn hit_at(kind: DrumKind, t: f64, vel: f32) -> DrumHit {
    DrumHit {
        t,
        kind,
        vel,
        pan: -1.0,
    }
}

/// `n` consecutive hits of `kind` at velocity `vel`, 3.2 s apart, as mono
/// slices (hard left, so the left gain is 1).
fn hits_of(kind: DrumKind, vel: f32, n: usize, seed: u64) -> Vec<Vec<f32>> {
    let gap = 3.2;
    let hits: Vec<DrumHit> = (0..n).map(|k| hit_at(kind, k as f64 * gap, vel)).collect();
    let len = (n as f64 * gap * SR_F) as usize;
    let [l, _] = render_drums(&hits, seed, len);
    let g = (gap * SR_F) as usize;
    (0..n).map(|k| l[k * g..(k + 1) * g].to_vec()).collect()
}

/// Energy fractions of the first 0.6 s in the 1/3-octave `BANDS`.
fn band_fractions(x: &[f32]) -> Vec<f64> {
    let n = (0.6 * SR_F) as usize;
    let mut buf: Vec<f32> = (0..n)
        .map(|i| {
            let w = if i + 256 >= n {
                0.5 + 0.5 * (std::f64::consts::PI * (i + 256 - n) as f64 / 256.0).cos()
            } else {
                1.0
            };
            x.get(i).copied().unwrap_or(0.0) * w as f32
        })
        .collect();
    let fft = RealFft::new(n);
    let mut spec = vec![C32::new(0.0, 0.0); fft.spectrum_len()];
    let mut scratch = fft.make_scratch();
    fft.forward(&mut buf, &mut spec, &mut scratch)
        .expect("fft sizes");
    let e: Vec<f64> = BANDS
        .iter()
        .map(|&c| {
            let k0 = (c / 2f64.powf(1.0 / 6.0) / SR_F * n as f64).ceil() as usize;
            let k1 = ((c * 2f64.powf(1.0 / 6.0) / SR_F * n as f64).ceil() as usize).min(spec.len());
            spec[k0..k1].iter().map(|v| v.norm_sqr() as f64).sum()
        })
        .collect();
    let total: f64 = e.iter().sum();
    e.iter().map(|v| v / total.max(1e-30)).collect()
}

/// Energy of `x` in [f_lo, f_hi) over its energy in [0, 20 kHz), first 3 s.
fn share(x: &[f32], f_lo: f64, f_hi: f64) -> f64 {
    let n = (3.0 * SR_F) as usize;
    let mut buf: Vec<f32> = (0..n).map(|i| x.get(i).copied().unwrap_or(0.0)).collect();
    let fft = RealFft::new(n);
    let mut spec = vec![C32::new(0.0, 0.0); fft.spectrum_len()];
    let mut scratch = fft.make_scratch();
    fft.forward(&mut buf, &mut spec, &mut scratch)
        .expect("fft sizes");
    let bin = |f: f64| ((f / SR_F * n as f64).ceil() as usize).min(spec.len());
    let sum = |a: usize, b: usize| -> f64 { spec[a..b].iter().map(|v| v.norm_sqr() as f64).sum() };
    sum(bin(f_lo), bin(f_hi)) / sum(0, bin(20000.0))
}

fn rms_0p3(x: &[f32]) -> f64 {
    let n = (0.3 * SR_F) as usize;
    (x[..n].iter().map(|&v| v as f64 * v as f64).sum::<f64>() / n as f64).sqrt()
}

#[test]
fn rendering_is_deterministic_for_a_seed() {
    for kind in ALL {
        let a = hits_of(kind, 0.7, 3, 11);
        let b = hits_of(kind, 0.7, 3, 11);
        assert_eq!(a, b, "{kind:?} differs between two renders of one seed");
    }
    for kind in [DrumKind::Kick, DrumKind::Snare, DrumKind::Ride] {
        let a = hits_of(kind, 0.7, 2, 11);
        let b = hits_of(kind, 0.7, 2, 12);
        assert_ne!(a, b, "{kind:?} ignores the seed");
    }
}

#[test]
fn consecutive_hits_are_never_alike() {
    for kind in [
        DrumKind::Kick,
        DrumKind::Snare,
        DrumKind::Rim,
        DrumKind::Tap,
        DrumKind::Hat,
        DrumKind::Tom { hz: 110.0 },
        DrumKind::Ride,
    ] {
        let hits = hits_of(kind, 0.6, 8, 1234);
        let mut worst = 0.0f64;
        for w in hits.windows(2) {
            let dot: f64 = w[0]
                .iter()
                .zip(&w[1])
                .map(|(&a, &b)| a as f64 * b as f64)
                .sum();
            let na: f64 = w[0]
                .iter()
                .map(|&a| a as f64 * a as f64)
                .sum::<f64>()
                .sqrt();
            let nb: f64 = w[1]
                .iter()
                .map(|&a| a as f64 * a as f64)
                .sum::<f64>()
                .sqrt();
            worst = worst.max(dot / (na * nb));
        }
        println!("{kind:?}: worst consecutive correlation {worst:.3}");
        assert!(worst < 0.98, "{kind:?} repeats: correlation {worst}");
    }
}

#[test]
fn brightness_follows_velocity() {
    // Share of the 2 kHz octave band (1.41-2.83 kHz) over 8 hits.
    let mean_share = |kind: DrumKind, v: f32| -> f64 {
        let h = hits_of(kind, v, 8, 7);
        h.iter().map(|x| share(x, 1414.0, 2828.0)).sum::<f64>() / h.len() as f64
    };
    let (k_soft, k_hard) = (
        mean_share(DrumKind::Kick, 0.25),
        mean_share(DrumKind::Kick, 0.95),
    );
    println!("kick 2 kHz share: soft {k_soft:.2e} hard {k_hard:.2e}");
    assert!(k_hard > 2.0 * k_soft, "kick: {k_soft} -> {k_hard}");
    let (t_soft, t_hard) = (
        mean_share(DrumKind::Tap, 0.25),
        mean_share(DrumKind::Tap, 0.95),
    );
    println!("tap 2 kHz share: soft {t_soft:.2e} hard {t_hard:.2e}");
    assert!(t_hard < 0.5 * t_soft, "tap: {t_soft} -> {t_hard}");
}

/// RMS over bands (63 Hz to 12.5 kHz, table above -35 dB) of the mean dB
/// curve of 16 hits at velocity 0.95 against the hard layer of the table.
fn curve_error(kind: DrumKind, table: &[f64; 28]) -> (f64, f64) {
    let hits = hits_of(kind, 0.95, 16, 5);
    let mut mean = vec![0.0f64; 28];
    for h in &hits {
        for (m, f) in mean.iter_mut().zip(band_fractions(h)) {
            *m += 10.0 * (f + 1e-12).log10() / hits.len() as f64;
        }
    }
    let d: Vec<f64> = (0..28)
        .filter(|&i| BANDS[i] >= 63.0 && BANDS[i] <= 12500.0 && table[i] > -35.0)
        .map(|i| mean[i] - table[i])
        .collect();
    let rms = (d.iter().map(|v| v * v).sum::<f64>() / d.len() as f64).sqrt();
    let worst = d
        .iter()
        .fold(0.0f64, |a, &v| if v.abs() > a.abs() { v } else { a });
    (rms, worst)
}

#[test]
fn band_curves_follow_their_tables() {
    let voices: [(&str, DrumKind, &VoiceData); 6] = [
        ("kick", DrumKind::Kick, &data::KICK),
        ("snare", DrumKind::Snare, &data::SNARE),
        ("rim", DrumKind::Rim, &data::RIM),
        ("tap", DrumKind::Tap, &data::TAP),
        ("hat", DrumKind::Hat, &data::HAT),
        ("ride", DrumKind::Ride, &data::RIDE),
    ];
    for (name, kind, vd) in voices {
        let (rms, worst) = curve_error(kind, &vd.layers[2].curve);
        println!("{name}: curve rms {rms:.2} dB, worst {worst:+.1} dB");
        assert!(rms < 6.0, "{name}: curve rms {rms} dB");
    }
    let (rms, worst) = curve_error(
        DrumKind::Tom {
            hz: data::TOM_F0 as f32,
        },
        &data::TOM.layers[2].curve,
    );
    println!("tom: curve rms {rms:.2} dB, worst {worst:+.1} dB");
    assert!(rms < 6.0, "tom: curve rms {rms} dB");
    let (rms, worst) = curve_error(DrumKind::Swish { dur: 0.5 }, &data::SWISH_CURVE);
    println!("swish: curve rms {rms:.2} dB, worst {worst:+.1} dB");
    assert!(rms < 6.0, "swish: curve rms {rms} dB");
}

#[test]
fn hard_layer_keeps_the_level_of_the_old_voices() {
    // First-0.3-s RMS at velocity 0.95 of the oscillator voices this model
    // replaced (mean of 8 hits); the new voices stay within 1 dB.
    let old = [
        (DrumKind::Kick, 0.35238),
        (DrumKind::Snare, 0.18030),
        (DrumKind::Rim, 0.05912),
        (DrumKind::Tap, 0.11191),
        (DrumKind::Hat, 0.07175),
        (DrumKind::Tom { hz: 110.0 }, 0.43119),
        (DrumKind::Ride, 0.05259),
    ];
    for (kind, want) in old {
        let h = hits_of(kind, 0.95, 8, 1234);
        let got = h.iter().map(|x| rms_0p3(x)).sum::<f64>() / h.len() as f64;
        let db = 20.0 * (got / want).log10();
        println!("{kind:?}: {got:.4} against {want:.4}: {db:+.2} dB");
        assert!(db.abs() < 1.0, "{kind:?} is {db:+.2} dB from the old level");
    }
}

#[test]
fn every_kind_is_finite_and_bounded() {
    let mut scratch = DrumScratch::new(3);
    let mut peak_all = 0.0f32;
    for (j, &kind) in ALL.iter().enumerate() {
        for vel in [0.2f32, 0.6, 1.0] {
            let n = hit_len(kind);
            // Room after the hit: nothing may be written past hit_len.
            let mut l = vec![0.0f32; n + 2000];
            let mut r = vec![0.0f32; n + 2000];
            let mut rng = Rng::event(3, DRUM_HIT, j as u64);
            render_hit(
                &DrumHit {
                    t: 0.0,
                    kind,
                    vel,
                    pan: 0.0,
                },
                &mut rng,
                &mut l,
                &mut r,
                &mut scratch,
            );
            // Undo the centre pan gain and the velocity to get the voice.
            let g = std::f32::consts::FRAC_1_SQRT_2 * vel;
            let peak = l.iter().map(|v| (v / g).abs()).fold(0.0f32, f32::max);
            peak_all = peak_all.max(peak);
            assert!(
                l.iter().chain(&r).all(|v| v.is_finite()),
                "{kind:?} not finite"
            );
            assert!(
                l[n..].iter().chain(&r[n..]).all(|&v| v == 0.0),
                "{kind:?} runs past its length"
            );
            assert!(peak <= 8.0, "{kind:?} vel {vel} peak {peak}");
            assert!(peak > 0.01, "{kind:?} vel {vel} silent");
        }
    }
    println!("largest unit-velocity peak {peak_all:.2}");
}

#[test]
fn bad_input_does_not_panic() {
    let mut l = vec![0.0f32; 1000];
    let mut r = vec![0.0f32; 1000];
    let mut scratch = DrumScratch::new(1);
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
        DrumHit {
            t: 0.0,
            kind: DrumKind::Snare,
            vel: 7.0,
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

#[test]
fn no_hits_is_silence() {
    // `DrumKit::None` plans no hits (arrange) and the engine makes no drum
    // stem for it (engine/tests/render_test.rs); here, no hits render zeros.
    let [l, r] = render_drums(&[], 1, 5000);
    assert!(l.iter().chain(&r).all(|v| *v == 0.0));
}

#[test]
fn hits_past_the_song_end_are_clipped_safely() {
    // A hit that starts near the end, one that starts before the start and
    // one past the end: the stem keeps its length, nothing panics, and the
    // part inside the stem is the head of the hit.
    let len = 3000usize;
    for kind in ALL {
        let t_end = (len - 100) as f64 / SR_F;
        let hits = [
            hit_at(kind, t_end, 0.9),
            hit_at(kind, -0.05, 0.9),
            hit_at(kind, len as f64 / SR_F + 1.0, 0.9),
        ];
        let [l, r] = render_drums(&hits, 2, len);
        assert_eq!((l.len(), r.len()), (len, len));
        assert!(l.iter().chain(&r).all(|v| v.is_finite()), "{kind:?}");
    }
}

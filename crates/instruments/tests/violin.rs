//! Bowed string: Helmholtz sweep, tuning, spectrum against the earlier
//! linear-read model, finite output, planner rules.

mod helmholtz;

use dsp::fft::{RealFft, C32};
use helmholtz::{note_sweep, phrase_sweep};
use instruments::violin::{plan_strokes, render_violin};
use sfcore::math::mtof;
use sfcore::random::Rng;
use sfcore::SR_F;
use song::events::BowNote;

fn note(t0: f64, t1: f64, midi: f32, vel: f32, vibrato: bool) -> BowNote {
    BowNote {
        t0,
        t1,
        midi,
        vel,
        vibrato,
    }
}

#[test]
fn helmholtz_single_notes() {
    let (ok, total, bad) = note_sweep();
    assert_eq!(total, 216);
    assert!(ok >= 208, "stable {ok}/{total}, failing {bad:?}");
}

#[test]
fn helmholtz_phrases() {
    let (ok, total, bad) = phrase_sweep();
    assert!(
        ok * 100 >= total * 95,
        "phrase notes {ok}/{total}, failing {bad:?}"
    );
}

/// Mean f0 over `x` by autocorrelation at about `periods` periods of the
/// expected f0 (lag search within +-3%), refined by a parabola through the
/// peak. A long lag makes one sample of lag error small in cents.
fn f0_autocorr(x: &[f32], f_expect: f64, periods: usize) -> f64 {
    let lag0 = SR_F / f_expect * periods as f64;
    let lo = (lag0 * 0.97).floor() as usize;
    let hi = (lag0 * 1.03).ceil() as usize;
    let n = x.len() - hi - 1;
    let r = |lag: usize| -> f64 { (0..n).map(|i| x[i] as f64 * x[i + lag] as f64).sum() };
    let vals: Vec<f64> = (lo - 1..=hi + 1).map(r).collect();
    let bi = (1..vals.len() - 1)
        .max_by(|&i, &j| vals[i].total_cmp(&vals[j]))
        .unwrap_or(1);
    let (a, b, c) = (vals[bi - 1], vals[bi], vals[bi + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-30 {
        0.5 * (a - c) / den
    } else {
        0.0
    };
    let lag = (lo - 1 + bi) as f64 + off;
    SR_F / lag * periods as f64
}

#[test]
fn sustained_f0_within_10_cents() {
    let len = (2.0 * SR_F) as usize;
    for (i, m) in [55.0f32, 62.0, 69.0, 76.0, 81.0, 88.0]
        .into_iter()
        .enumerate()
    {
        let x = render_violin(&[note(0.1, 1.8, m, 0.6, false)], len, 40 + i as u64);
        let seg = &x[(0.6 * SR_F) as usize..(1.6 * SR_F) as usize];
        let f = f0_autocorr(seg, mtof(m as f64), 8);
        let cents = 1200.0 * (f / mtof(m as f64)).log2();
        println!("midi {m}: {cents:+.2} cents");
        assert!(cents.abs() < 10.0, "midi {m}: {cents:.2} cents");
    }
}

#[test]
fn output_finite_and_bounded() {
    let notes = [
        note(0.0, 0.4, 60.0, 0.5, true),
        note(0.42, 3.9, 72.0, 1.0, true),
        note(3.92, 4.2, 90.0, 0.85, false),
        note(5.0, 5.3, 40.0, 0.7, true),  // below MIN_F0: raised
        note(6.0, 6.2, 120.0, 0.7, true), // above MAX_F0: lowered
        note(7.0, 6.0, 64.0, 0.7, true),  // empty: dropped
        note(f64::NAN, 1.0, 64.0, 0.7, true),
        note(-0.5, 0.3, 67.0, 0.9, true), // starts before the buffer
        note(7.9, 9.0, 67.0, 0.9, true),  // runs past the end
    ];
    let x = render_violin(&notes, (8.0 * SR_F) as usize, 3);
    assert!(x.iter().all(|v| v.is_finite()));
    let peak = x.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    assert!(peak > 0.01 && peak < 10.0, "peak {peak}");
    assert!(render_violin(&[], 100, 1).iter().all(|&v| v == 0.0));
    assert!(render_violin(&notes, 0, 1).is_empty());
}

#[test]
fn deterministic() {
    let notes = [
        note(0.1, 0.8, 64.0, 0.6, true),
        note(0.82, 1.5, 71.0, 0.6, true),
    ];
    assert_eq!(
        render_violin(&notes, 80_000, 9),
        render_violin(&notes, 80_000, 9)
    );
}

#[test]
fn planner_splits_groups_and_alternates() {
    let notes = [
        note(0.0, 5.0, 60.0, 0.6, true), // split into 3 strokes (1.6-2.2 s each)
        note(5.02, 5.3, 67.0, 0.6, true), // same phrase (gap 0.02)
        note(6.0, 6.5, 62.0, 0.6, false), // new phrase (gap 0.7)
    ];
    let mut rng = Rng::from_seed(5);
    let ph = plan_strokes(&notes, &mut rng);
    assert_eq!(ph.len(), 2);
    let s = &ph[0].strokes;
    assert!(s.len() == 4 || s.len() == 5, "{} strokes", s.len());
    for w in s.windows(2) {
        if w[1].rebow {
            assert_eq!(w[1].dir, -w[0].dir);
        } else {
            assert_eq!(w[1].dir, w[0].dir);
        }
    }
    assert!(s[1].rebow && s[1].vib_delay == 0.0);
    assert_eq!(ph[1].strokes[0].vib_depth, 0.0);
    assert!((0.13..0.15).contains(&ph[0].beta));
}

/// 1/3-octave band powers (IEC 61260 base-10 centres 200 Hz - 8 kHz, edges
/// at fc * 2^(+-1/6)) of `x` by a Hann-windowed Welch average, as dB
/// relative to the total power of the frames.
fn third_octave_db(x: &[f32]) -> Vec<f64> {
    const N: usize = 8192;
    let fft = RealFft::new(N);
    let mut scratch = fft.make_scratch();
    let mut buf = vec![0.0f32; N];
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    let mut pow = vec![0.0f64; fft.spectrum_len()];
    let win: Vec<f32> = (0..N)
        .map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / N as f64).cos()) as f32)
        .collect();
    let mut start = 0;
    while start + N <= x.len() {
        for i in 0..N {
            buf[i] = x[start + i] * win[i];
        }
        fft.forward(&mut buf, &mut spec, &mut scratch).unwrap();
        for (p, c) in pow.iter_mut().zip(&spec) {
            *p += (c.re as f64).powi(2) + (c.im as f64).powi(2);
        }
        start += N / 2;
    }
    let total: f64 = pow.iter().sum();
    (-7..=9)
        .map(|k: i32| {
            let fc = 1000.0 * 10f64.powf(k as f64 / 10.0);
            let (lo, hi) = (fc * 2f64.powf(-1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            let bin = |f: f64| (f * N as f64 / SR_F).round() as usize;
            let e: f64 = pow[bin(lo)..bin(hi)].iter().sum();
            10.0 * (e / total).max(1e-30).log10()
        })
        .collect()
}

/// 1/3-octave levels (dB re total, 200 Hz - 8 kHz) of the earlier bowed
/// string (linear delay reads, same loss), averaged over seeds 100-111,
/// for `spectrum_matches_earlier_model`. Measured at commit 50ade40, the
/// last tree that held that model.
const EARLIER_MODEL_DB: [f64; 17] = [
    -35.42, -35.57, -34.95, -2.72, -9.74, -37.55, -8.33, -13.62, -11.48, -15.02, -14.39, -16.90,
    -19.10, -20.16, -22.78, -24.45, -27.52,
];

/// A sustained A4, mf (velocity 0.6), vibrato on, 2.2 s: the 1/3-octave
/// spectrum of the model, averaged over 12 seeds, within 3 dB of the
/// earlier model (`EARLIER_MODEL_DB`) over 200 Hz - 8 kHz. The Lagrange
/// reads and the re-tuned loss were chosen against it (design 5.7).
#[test]
fn spectrum_matches_earlier_model() {
    let len = (2.6 * SR_F) as usize;
    let (a, b) = ((0.4 * SR_F) as usize, (2.2 * SR_F) as usize);
    let seeds = 12;
    let mut new_db = [0.0; 17];
    for s in 0..seeds {
        let x = render_violin(&[note(0.1, 2.3, 69.0, 0.6, true)], len, 100 + s);
        for (acc, v) in new_db.iter_mut().zip(third_octave_db(&x[a..b])) {
            *acc += v / seeds as f64;
        }
    }
    let mut worst = 0.0f64;
    let mut report = String::new();
    for (k, (n, o)) in new_db.iter().zip(&EARLIER_MODEL_DB).enumerate() {
        let fc = 1000.0 * 10f64.powf((k as f64 - 7.0) / 10.0);
        report += &format!(
            "{fc:7.0} Hz  new {n:7.2}  old {o:7.2}  diff {:+6.2}\n",
            n - o
        );
        worst = worst.max((n - o).abs());
    }
    println!("{report}");
    assert!(
        worst <= 3.0,
        "worst band difference {worst:.2} dB\n{report}"
    );
}

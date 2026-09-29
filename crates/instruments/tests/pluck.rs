//! Plucked string, guitar strings, sympathetic strings and bass: tuning,
//! decay, DC, bounds, and the string semantics.

use dsp::biquad::{Biquad, BiquadCoeffs};
use instruments::guitar::{
    render_bass, render_guitar, render_plucks, render_strings, Sympathetic, OPEN_STRINGS,
};
use instruments::pluck::{pluck_into, PluckParams, PluckScratch};
use sfcore::math::mtof;
use sfcore::random::{tag, Rng};
use sfcore::SR_F;
use song::events::{PluckNote, StringNote};
use std::f64::consts::TAU;

const MIDIS: [u8; 5] = [40, 52, 64, 76, 88];

/// Resolved parameters for one note at velocity 0.8.
fn resolve(preset: &PluckParams, midi: u8, seed: u64) -> (PluckParams, Rng) {
    let mut rng = Rng::event(seed, tag("test.pluck"), midi as u64);
    let p = preset.note(mtof(midi as f64), 0.8, &mut rng);
    (p, rng)
}

fn render(p: &PluckParams, f0: f64, secs: f64, rng: &mut Rng) -> Vec<f32> {
    let n = (secs * SR_F) as usize;
    let mut out = vec![0.0f32; n];
    let mut scratch = PluckScratch::new();
    pluck_into(&mut out, 0, f0, n, p, rng, &mut scratch);
    out
}

/// Fundamental frequency by autocorrelation. The signal is band-passed
/// around `f_nom` (Q 4) so the stretched upper partials of the loop do not
/// pull the estimate; the lag is searched near the multiple of the period
/// closest to 2000 samples and refined by a parabola, so the resolution is
/// well under a cent.
fn f0_autocorr(x: &[f32], f_nom: f64, from: usize, win: usize) -> f64 {
    let mut bp = Biquad::new(BiquadCoeffs::bandpass(SR_F, f_nom, 4.0));
    let y: Vec<f64> = x.iter().map(|&v| bp.tick(v as f64)).collect();
    let n0 = SR_F / f_nom;
    let m = (2000.0 / n0).round().max(1.0);
    let centre = m * n0;
    let r = |k: usize| -> f64 { (from..from + win).map(|i| y[i] * y[i + k]).sum() };
    let lo = (centre - 0.4 * n0).floor().max(1.0) as usize;
    let hi = (centre + 0.4 * n0).ceil() as usize;
    let mut best = lo;
    let mut best_r = f64::MIN;
    for k in lo..=hi {
        let v = r(k);
        if v > best_r {
            best_r = v;
            best = k;
        }
    }
    let (a, b, c) = (r(best - 1), best_r, r(best + 1));
    let off = 0.5 * (a - c) / (a - 2.0 * b + c);
    m * SR_F / (best as f64 + off)
}

fn cents(f: f64, r: f64) -> f64 {
    1200.0 * (f / r).log2()
}

#[test]
fn pluck_tuning_within_2_cents() {
    for (name, preset) in [("GUITAR", PluckParams::GUITAR), ("HARP", PluckParams::HARP)] {
        for &m in &MIDIS {
            let f = mtof(m as f64);
            let (p, mut rng) = resolve(&preset, m, 7);
            let x = render(&p, f, 1.5, &mut rng);
            let est = f0_autocorr(&x, f, (0.4 * SR_F) as usize, 20000);
            let c = cents(est, f);
            println!("{name} midi {m}: {est:.3} Hz, {c:+.3} cents");
            assert!(c.abs() < 2.0, "{name} midi {m}: {c:+.3} cents");
        }
    }
}

/// Goertzel magnitude of `x` at `f`.
fn goertzel(x: &[f32], f: f64) -> f64 {
    let w = TAU * f / SR_F;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt()
}

/// T60 of the fundamental: least-squares slope of the Goertzel level in
/// 4096-sample frames between 0.2 and 0.8 of the requested T60.
fn measured_t60(x: &[f32], f: f64, t60: f64) -> f64 {
    let frame = 4096;
    let hop = 1024;
    let (mut st, mut sy, mut stt, mut sty, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let mut i = (0.2 * t60 * SR_F) as usize;
    let end = ((0.8 * t60 * SR_F) as usize).min(x.len() - frame);
    while i < end {
        let t = (i + frame / 2) as f64 / SR_F;
        let y = 20.0 * goertzel(&x[i..i + frame], f).log10();
        st += t;
        sy += y;
        stt += t * t;
        sty += t * y;
        n += 1.0;
        i += hop;
    }
    let slope = (n * sty - st * sy) / (n * stt - st * st);
    -60.0 / slope
}

#[test]
fn pluck_fundamental_t60_within_10_percent() {
    for (name, preset) in [("GUITAR", PluckParams::GUITAR), ("HARP", PluckParams::HARP)] {
        for &m in &MIDIS {
            let f = mtof(m as f64);
            let (mut p, mut rng) = resolve(&preset, m, 11);
            p.release = 0.0;
            let x = render(&p, f, 0.9 * p.t60 + 0.2, &mut rng);
            let t = measured_t60(&x, f, p.t60);
            let err = t / p.t60 - 1.0;
            println!(
                "{name} midi {m}: T60 asked {:.3} s, measured {t:.3} s ({:+.1}%)",
                p.t60,
                100.0 * err
            );
            assert!(err.abs() < 0.10, "{name} midi {m}: {:+.1}%", 100.0 * err);
        }
    }
}

#[test]
fn pluck_has_no_dc_and_is_bounded() {
    let presets = [
        ("GUITAR", PluckParams::GUITAR),
        ("BASS", PluckParams::BASS),
        ("HARP", PluckParams::HARP),
        ("HG_LEAD", PluckParams::HG_LEAD),
        ("HG_ARP", PluckParams::HG_ARP),
    ];
    for (name, preset) in presets {
        for m in (28u8..=100).step_by(6) {
            let f = mtof(m as f64);
            // Render the full decay (the -74 dB stop ends it): the mean of a
            // truncated note is its final displacement, not DC.
            let (p, mut rng) = resolve(&preset, m, 3);
            let x = render(&p, f, 1.4 * p.t60 + 0.5, &mut rng);
            assert!(x.iter().all(|v| v.is_finite()), "{name} {m}: non-finite");
            let peak = x.iter().fold(0.0f64, |a, &v| a.max((v as f64).abs()));
            let mean = x.iter().map(|&v| v as f64).sum::<f64>() / x.len() as f64;
            assert!(peak > 0.0, "{name} {m}: silent");
            assert!(
                peak <= 1.6 * p.amp,
                "{name} {m}: peak {peak} for amp {}",
                p.amp
            );
            // The bass's 1.8 ms attack ramp (brightness 0.12) removes onset
            // area from the velocity output: a one-time transient, measured
            // at up to 1.8e-4 of the peak. The loop itself carries no net DC.
            let limit = if name == "BASS" { 2.5e-4 } else { 1e-4 };
            assert!(
                mean.abs() < limit * peak,
                "{name} {m}: mean {:.2e} of peak",
                mean / peak
            );
        }
    }
}

#[test]
fn pluck_ignores_bad_input() {
    let mut out = vec![0.0f32; 1000];
    let mut rng = Rng::from_seed(1);
    let mut s = PluckScratch::new();
    let p = PluckParams::default();
    pluck_into(&mut out, 0, 10.0, 1000, &p, &mut rng, &mut s);
    pluck_into(&mut out, 0, f64::NAN, 1000, &p, &mut rng, &mut s);
    pluck_into(&mut out, 5000, 220.0, 1000, &p, &mut rng, &mut s);
    pluck_into(&mut out, 0, 220.0, 0, &p, &mut rng, &mut s);
    pluck_into(&mut out, 0, 30000.0, 1000, &p, &mut rng, &mut s);
    assert!(out.iter().all(|&v| v == 0.0));
    for nan in [
        PluckParams { amp: f64::NAN, ..p },
        PluckParams {
            noise: f64::NAN,
            ..p
        },
        PluckParams {
            glide: f64::INFINITY,
            ..p
        },
    ] {
        pluck_into(&mut out, 0, 220.0, 1000, &nan, &mut rng, &mut s);
    }
    assert!(out.iter().all(|&v| v == 0.0));
    // Clipped at the buffer end, extreme parameters: finite, no panic.
    let wild = PluckParams {
        pick: 3.0,
        bright: -2.0,
        damp: 5.0,
        t60: -1.0,
        glide: 500.0,
        attack_noise: 1.0,
        ..p
    };
    pluck_into(&mut out, 900, 220.0, 100_000, &wild, &mut rng, &mut s);
    assert!(out.iter().all(|v| v.is_finite()));
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|&v| (v as f64) * (v as f64)).sum()
}

#[test]
fn string_restrike_stops_previous_note() {
    let len = (2.0 * SR_F) as usize;
    let one = StringNote {
        t: 0.1,
        stop: 1.9,
        string: 0,
        midi: 45,
        vel: 0.8,
    };
    let mut strings: [Vec<StringNote>; 6] = Default::default();
    strings[1].push(one);
    let a = render_strings(&strings, &PluckParams::GUITAR, 5, len);
    // Same note, restruck at 0.5 s: the first note ends at 0.504 s, and is
    // rendered by the same per-event stream, so up to 0.5 s the output matches.
    strings[1].push(StringNote { t: 0.5, ..one });
    let b = render_strings(&strings, &PluckParams::GUITAR, 5, len);
    let cut = (0.48 * SR_F) as usize;
    assert_eq!(&a[..cut], &b[..cut]);
    // A stop at 0.3 s silences the string after it.
    strings[1] = vec![StringNote { stop: 0.3, ..one }];
    let c = render_strings(&strings, &PluckParams::GUITAR, 5, len);
    assert!(energy(&c[(0.31 * SR_F) as usize..]) == 0.0);
    assert!(energy(&c[..(0.3 * SR_F) as usize]) > 0.0);
}

#[test]
fn sympathetic_strings_ring_at_open_pitches() {
    // Excite with a short burst of noise, then silence: the open strings ring on.
    let len = (1.5 * SR_F) as usize;
    let mut x = vec![0.0f32; len];
    let mut rng = Rng::from_seed(9);
    rng.fill_bipolar(&mut x[..2000]);
    let mut y = x.clone();
    let mut s = Sympathetic::new(1.0);
    s.process(&mut y);
    assert!(y.iter().all(|v| v.is_finite()));
    // One second from 0.3 s, Hann window: 1 Hz bins; E2 against F2.
    let tail: Vec<f32> = y[(0.3 * SR_F) as usize..(1.3 * SR_F) as usize]
        .iter()
        .enumerate()
        .map(|(i, &v)| v * (0.5 - 0.5 * (TAU * i as f64 / SR_F).cos()) as f32)
        .collect();
    let e2 = goertzel(&tail, mtof(OPEN_STRINGS[0] as f64));
    let off = goertzel(&tail, mtof(OPEN_STRINGS[0] as f64 + 1.0));
    println!("E2 {e2:.4} F2 {off:.4}");
    assert!(e2 > 4.0 * off, "E2 {e2} off {off}");
    // Silence in, silence out.
    let mut z = vec![0.0f32; 10_000];
    Sympathetic::new(1.0).process(&mut z);
    assert!(z.iter().all(|&v| v == 0.0));
}

#[test]
fn guitar_bass_harp_render_finite() {
    let len = (3.0 * SR_F) as usize;
    let mut strings: [Vec<StringNote>; 6] = Default::default();
    for (s, notes) in strings.iter_mut().enumerate() {
        notes.push(StringNote {
            t: 0.2 + 0.01 * s as f64,
            stop: 2.5,
            string: s as u8,
            midi: OPEN_STRINGS[s] + 2,
            vel: 0.7,
        });
        notes.push(StringNote {
            t: 1.2,
            stop: 8.0,
            string: s as u8,
            midi: OPEN_STRINGS[s],
            vel: 0.6,
        });
    }
    let g = render_guitar(&strings, 1234, len);
    assert!(g.iter().all(|v| v.is_finite()) && energy(&g) > 0.0);
    let notes = [
        PluckNote {
            t0: 0.1,
            t1: 1.0,
            midi: 40.0,
            vel: 0.9,
        },
        PluckNote {
            t0: 2.9,
            t1: 5.0,
            midi: 43.0,
            vel: 0.7,
        },
        PluckNote {
            t0: -1.0,
            t1: 1.0,
            midi: 40.0,
            vel: 0.9,
        },
    ];
    let b = render_bass(&notes, 1234, len);
    assert!(b.iter().all(|v| v.is_finite()) && energy(&b) > 0.0);
    let pk = b.iter().fold(0.0f32, |a, &v| a.max(v.abs()));
    assert!(pk < 2.0, "bass peak {pk}");
    let h = render_plucks(&notes, &PluckParams::HARP, 1234, tag("harp.note"), len);
    assert!(h.iter().all(|v| v.is_finite()) && energy(&h) > 0.0);
}

/// The tension glide starts sharp and relaxes (70 ms time constant): over
/// 20-120 ms the pitch sits above the same note without glide, and the
/// difference is gone after 0.5 s.
#[test]
fn glide_raises_onset_pitch() {
    for m in [40u8, 52, 64] {
        let f = mtof(m as f64);
        let mut est = [[0.0; 2]; 2];
        for (k, glide) in [0.0, 5.0].into_iter().enumerate() {
            let (mut p, mut rng) = resolve(&PluckParams::GUITAR, m, 21);
            p.glide = glide;
            let x = render(&p, f, 1.5, &mut rng);
            est[k][0] = f0_autocorr(&x, f, (0.02 * SR_F) as usize, (0.1 * SR_F) as usize - 2100);
            est[k][1] = f0_autocorr(&x, f, (0.5 * SR_F) as usize, 20000);
        }
        let early = cents(est[1][0], est[0][0]);
        let late = cents(est[1][1], est[0][1]);
        println!("midi {m}: glide 5 cents: {early:+.2} cents early, {late:+.2} cents late");
        assert!(early > 1.0 && early < 5.0, "midi {m}: early {early:+.2}");
        assert!(late.abs() < 0.3, "midi {m}: late {late:+.2}");
    }
}

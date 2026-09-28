//! Physics and numerics of the voice source and tract (design section 9).

use dsp::fft::{RealFft, C32};
use dsp::resonator::{coeffs, Resonator};
use sfcore::random::{tag, Rng};
use sfcore::{HOP, SR_F};
use song::events::VocalNote;
use song::{Phoneme, Voice};
use voice::glottal::{level_max_harmonic, level_top_hz, lf_table, GlottalSource, LfParams, N_LEVELS, TABLE_LEN};
use voice::tract::{Formants, Tract};
use voice::{render_phrases, voice_params, VoiceSettings};

const RDS: [f64; 6] = [0.3, 0.6, 1.0, 1.5, 1.91, 2.7];

/// Zero net flow: the closed-form integral of the LF derivative over one
/// period, and a 200k-point Simpson quadrature of it, are below 1e-6 (the
/// derivative's minimum is -1).
#[test]
fn lf_zero_net_flow() {
    for rd in RDS {
        let p = LfParams::from_rd(rd);
        assert!(p.net_flow().abs() < 1e-6, "rd {rd}: closed form {}", p.net_flow());
        // Simpson on [0, te] and [te, 1] separately (E has a corner at te).
        let simpson = |a: f64, b: f64, n: usize| {
            let h = (b - a) / n as f64;
            let mut s = p.eval(a) + p.eval(b);
            for i in 1..n {
                s += p.eval(a + i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
            }
            s * h / 3.0
        };
        let q = simpson(0.0, p.te, 100_000) + simpson(p.te, 1.0, 100_000);
        assert!(q.abs() < 1e-6, "rd {rd}: quadrature {q}");
        assert!((p.eval(p.te) + 1.0).abs() < 1e-12, "rd {rd}: E(te) = {}", p.eval(p.te));
    }
}

/// The derivative tables have zero mean over a period (no DC) at every
/// mip level.
#[test]
fn lf_tables_have_zero_mean() {
    for rd in RDS {
        let t = lf_table(rd);
        for j in 0..N_LEVELS {
            let row = t.level(j);
            let mean = row[..TABLE_LEN].iter().map(|&x| x as f64).sum::<f64>() / TABLE_LEN as f64;
            assert!(mean.abs() < 1e-6, "rd {rd} level {j}: mean {mean}");
            assert_eq!(row[TABLE_LEN], row[0]);
        }
    }
}

/// Fant's Rd identity: Rd = (0.5 + 1.2 Rk)(Rk / (4 Rg) + Ra) / 0.11
/// recovers the Rd the ratios were derived from.
#[test]
fn rd_to_rg_identity() {
    for k in 12..=108 {
        let rd = k as f64 / 40.0;
        let p = LfParams::from_rd(rd);
        let back = LfParams::rd_from_ratios(p.ra, p.rk, p.rg);
        assert!((back - rd).abs() < 1e-12, "rd {rd} -> {back}");
        assert!(p.tp > 0.0 && p.tp < p.te && p.te < 1.0, "rd {rd}: tp {} te {}", p.tp, p.te);
    }
}

/// Each mip level, played at its top f0, has energy above SR/2 at least
/// 90 dB below its total energy.
#[test]
fn band_limited_tables_do_not_alias() {
    let fft = RealFft::new(TABLE_LEN);
    let mut scratch = fft.make_scratch();
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    for rd in RDS {
        let t = lf_table(rd);
        for j in 0..N_LEVELS {
            let mut x: Vec<f32> = t.level(j)[..TABLE_LEN].to_vec();
            fft.forward(&mut x, &mut spec, &mut scratch).expect("sized from the plan");
            let top = level_top_hz(j);
            let (mut above, mut total) = (0.0f64, 0.0f64);
            for (h, c) in spec.iter().enumerate().skip(1) {
                let e = c.re as f64 * c.re as f64 + c.im as f64 * c.im as f64;
                total += e;
                if h as f64 * top > SR_F * 0.5 {
                    above += e;
                }
            }
            let db = 10.0 * (above / total + 1e-300).log10();
            assert!(db < -90.0, "rd {rd} level {j} (top {top:.1} Hz, {} harmonics): {db:.1} dB", level_max_harmonic(j));
        }
    }
}

/// The tract cascade (five formants, four high resonances, shelf) has unity
/// DC gain: by coefficients, and a unit step settles at 1.
#[test]
fn tract_unity_dc() {
    for v in [Voice::Bass, Voice::Baritone, Voice::Tenor, Voice::Alto, Voice::Soprano] {
        let p = voice_params(v);
        let mut t = Tract::new(&p, 4);
        let f = Formants { f: [700.0, 1200.0, 2600.0], bw: [80.0, 90.0, 130.0] };
        t.set_formants(&f, &f);
        assert!((t.cascade_dc_gain() - 1.0).abs() < 1e-9, "{v:?}: {}", t.cascade_dc_gain());
        let mut y = 0.0;
        for _ in 0..2000 {
            t.set_formants(&f, &f);
            let mut b = [1.0f64; HOP];
            t.cascade_block(&mut b);
            y = b[HOP - 1];
        }
        assert!((y - 1.0).abs() < 1e-6, "{v:?}: step settles at {y}");
    }
}

/// Stable triangle of y = a x + b y1 + c y2: |c| < 1 and |b| < 1 - c.
fn stable(b: f64, c: f64) -> bool {
    c.abs() < 1.0 && b.abs() < 1.0 - c
}

/// Per-sample coefficient ramps between random stable formant designs
/// (the tract's ramp over HOP samples) never leave the stable triangle and
/// keep a = 1 - b - c.
#[test]
fn ramped_coefficients_stay_stable() {
    let mut rng = Rng::stream(7, tag("test.ramp"));
    let mut r = Resonator::default();
    r.set_coeffs(coeffs(500.0, 60.0, SR_F));
    for _ in 0..2000 {
        // Wide range, including very narrow and very wide bands and
        // centres near DC and near the 0.45 fs clamp.
        let f = rng.range(20.0, SR_F * 0.45);
        let bw = rng.range(5.0, 2000.0);
        r.set_ramped(coeffs(f, bw, SR_F), HOP as u32);
        for _ in 0..HOP {
            r.tick(0.0);
            assert!(stable(r.b, r.c), "b {} c {} (f {f} bw {bw})", r.b, r.c);
            assert!((r.a - (1.0 - r.b - r.c)).abs() < 1e-12);
        }
    }
}

fn note(t0: f64, t1: f64, midi: f32, phones: Vec<Phoneme>) -> VocalNote {
    VocalNote { t0, t1, midi, phones, amp: 1.0, stress: true, phrase_start: true, phrase_end: true, grace: None }
}

/// A sustained /aa/ renders finite, audible, bounded output for every
/// voice, including the soprano top where the mip levels matter.
#[test]
fn sustained_aa_is_finite() {
    for (v, midi) in [(Voice::Bass, 40.0), (Voice::Baritone, 52.0), (Voice::Alto, 67.0), (Voice::Soprano, 84.0)] {
        let len = (3.0 * SR_F) as usize;
        let notes = [note(0.5, 2.3, midi, vec![Phoneme::Aa])];
        let mut out = vec![0.0f32; len];
        render_phrases(&notes, v, &VoiceSettings::default(), 11, len, |s, x| out[s..s + x.len()].copy_from_slice(x));
        assert!(out.iter().all(|x| x.is_finite()), "{v:?}: non-finite sample");
        let mid = &out[(1.0 * SR_F) as usize..(2.0 * SR_F) as usize];
        let rms = (mid.iter().map(|&x| x as f64 * x as f64).sum::<f64>() / mid.len() as f64).sqrt();
        let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(rms > 1e-3 && peak < 20.0, "{v:?}: rms {rms} peak {peak}");
    }
}

/// Phrases split at gaps of 0.3 s or more; emitted ranges are disjoint,
/// ordered and inside the song; a long gap stays silent.
#[test]
fn phrases_are_disjoint_and_ordered() {
    let notes = vec![
        note(0.5, 1.0, 55.0, vec![Phoneme::S, Phoneme::Aa]),
        note(1.05, 1.6, 57.0, vec![Phoneme::T, Phoneme::Iy]),
        note(3.5, 4.0, 55.0, vec![Phoneme::D, Phoneme::Uw]),
        note(4.35, 4.8, 52.0, vec![Phoneme::M, Phoneme::Eh]),
    ];
    let groups = voice::synth::phrases(&notes);
    assert_eq!(groups, vec![0..2, 2..3, 3..4]);
    let len = (5.0 * SR_F) as usize;
    let mut spans = Vec::new();
    let mut out = vec![0.0f32; len];
    render_phrases(&notes, Voice::Tenor, &VoiceSettings::default(), 5, len, |s, x| {
        spans.push(s..s + x.len());
        out[s..s + x.len()].copy_from_slice(x);
    });
    for w in spans.windows(2) {
        assert!(w[0].end <= w[1].start, "{spans:?}");
    }
    assert!(spans.last().is_some_and(|s| s.end <= len));
    assert!(out.iter().all(|x| x.is_finite()));
    // Silence well inside the long gap.
    let gap = &out[(2.4 * SR_F) as usize..(2.7 * SR_F) as usize];
    assert!(gap.iter().all(|&x| x == 0.0));
}

/// Settings from the lead SingStyle leave the preset unchanged.
#[test]
fn lead_settings_are_identity() {
    let p = voice_params(Voice::Alto);
    assert_eq!(VoiceSettings::default().apply(p), p);
}

/// The source output is finite and periodic-ish over a glide that crosses
/// several mip levels (no blow-up at level changes).
#[test]
fn source_glide_across_levels_is_finite() {
    let mut g = GlottalSource::new(1.05, 7000.0, 0.003, 0.03);
    let mut rng = Rng::stream(1, tag("test.glide"));
    let n = 4 * 44_100;
    let mut prev = 0.0;
    let mut max_step = 0.0f64;
    for i in 0..n {
        let f0 = 80.0 * (i as f64 / n as f64 * 4.5).exp2();
        let (p, flow) = g.tick(f0, 0.8, &mut rng);
        assert!(p.is_finite() && (0.0..=1.0 + 1e-6).contains(&flow));
        max_step = max_step.max((p - prev).abs());
        prev = p;
    }
    assert!(max_step < 1.0, "largest sample step {max_step}");
}

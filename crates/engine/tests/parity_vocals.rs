//! Parity test against tests/parity/song_vocals.js's reference dump
//! (ref/parity/song_vocals.json). Run tests/parity/gen.sh first.
//!
//! Checks the four vocal tracks `renderSong` builds (lead, harmony, doubles,
//! choir) over DEMO_SONG at seed 1234, voice 'auto': each channel is
//! compared on its 25%/50%/75% 2s windows, a whole-buffer stride-97 sample,
//! and sum/sumsq over the whole buffer (computed in f64 in index order on
//! both sides, matching the JS reference's accumulation order).

use compose::prepare::prepare;
use compose::song::normalize_song;
use engine::demo_song;
use engine::vocals::{render_choir, render_doubles, render_harmony, render_lead};
use serde_json::Value;
use sfcore::tuning::Tuning;
use std::path::PathBuf;

const SEED: u32 = 1234;

fn ref_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/song_vocals.json"))
}

fn reference() -> Value {
    let s = std::fs::read_to_string(ref_path())
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", ref_path().display()));
    serde_json::from_str(&s).unwrap()
}

fn f64_arr(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// max abs error over reference peak, computed over a slice comparison.
fn err_metric(rust: &[f32], reference: &[f64]) -> f64 {
    assert_eq!(rust.len(), reference.len(), "length mismatch");
    let mut peak = 0.0f64;
    let mut maxerr = 0.0f64;
    for i in 0..rust.len() {
        let r = reference[i];
        let a = r.abs();
        if a > peak {
            peak = a;
        }
        let e = (rust[i] as f64 - r).abs();
        if e > maxerr {
            maxerr = e;
        }
    }
    if peak == 0.0 {
        maxerr
    } else {
        maxerr / peak
    }
}

/// Checks one channel against its reference entry: the three 2s windows,
/// the stride-97 sample, sumsq (relative), and sum (absolute, scaled by
/// sumsq and length; see the comment at its assertion).
fn check_channel(name: &str, buf: &[f32], chan_ref: &Value) {
    let len = chan_ref["len"].as_u64().unwrap() as usize;
    assert_eq!(buf.len(), len, "{name}: length mismatch");

    for win_key in ["win25", "win50", "win75"] {
        let w = &chan_ref[win_key];
        let start = w["start"].as_u64().unwrap() as usize;
        let data = f64_arr(&w["data"]);
        let e = err_metric(&buf[start..start + data.len()], &data);
        assert!(e < TOL, "{name} {win_key}: relative error {e} >= {TOL}");
    }

    let stride = chan_ref["stride"].as_u64().unwrap() as usize;
    let strided_ref = f64_arr(&chan_ref["strided"]);
    let strided_rust: Vec<f32> = buf.iter().step_by(stride).copied().collect();
    let e = err_metric(&strided_rust, &strided_ref);
    assert!(e < TOL, "{name} strided: relative error {e} >= {TOL}");

    // sum/sumsq: f64 accumulation in index order, matching the JS reference.
    let mut sum = 0.0f64;
    let mut sumsq = 0.0f64;
    for &v in buf {
        let v = v as f64;
        sum += v;
        sumsq += v * v;
    }
    let ref_sum = chan_ref["checksum"]["sum"].as_f64().unwrap();
    let ref_sumsq = chan_ref["checksum"]["sumsq"].as_f64().unwrap();
    let scale = ref_sumsq.abs().max(1e-12);
    assert!((sumsq - ref_sumsq).abs() / scale < TOL, "{name}: sumsq {sumsq} vs ref {ref_sumsq}");
    // `sum` is a near-total cancellation of an oscillating signal (audio
    // signal; a stray sub-ulp difference per sample survives division by an
    // already-tiny reference and looks huge in relative terms), so it is
    // checked against an absolute bound derived from the signal's actual
    // scale (sumsq) and length instead of against itself.
    let sum_bound = TOL * (ref_sumsq.max(0.0) * len as f64).sqrt().max(1.0);
    assert!((sum - ref_sum).abs() < sum_bound, "{name}: sum {sum} vs ref {ref_sum} (bound {sum_bound})");
}

fn check_track(name: &str, chs: &[Vec<f32>], track_ref: &Value) {
    let chs_ref = track_ref.as_array().unwrap();
    assert_eq!(chs.len(), chs_ref.len(), "{name}: channel count mismatch");
    for (i, (c, cr)) in chs.iter().zip(chs_ref.iter()).enumerate() {
        check_channel(&format!("{name}[{i}]"), c, cr);
    }
}

// Voice synthesis routes every transcendental through sfcore::js, which is
// up to ~1 ulp off V8 per call and compounds over many samples/filter
// stages. Measured worst case across all four tracks' windows and strided
// samples: 2.6e-8 relative to peak (doubles[1], win75). TOL is kept three
// orders of magnitude above that, still tight enough to catch a real
// divergence (a wrong rng draw, a missed note, a mismatched opts field) but
// not so tight it fails on harmless fp-reordering noise.
const TOL: f64 = 5e-5;

#[test]
fn vocal_tracks_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song();
    let song = normalize_song(&raw).expect("normalize_song");
    let seed = SEED;
    let p = prepare(&song, seed, None);
    let len = (p.timeline.end * sfcore::SR_F).ceil() as usize;
    let tuning = Tuning::default();

    let reference = reference();
    assert_eq!(reference["seed"].as_u64().unwrap() as u32, seed);
    assert_eq!(reference["len"].as_u64().unwrap() as usize, len);

    let lead = render_lead(&p, seed, len, &tuning);
    check_track("lead", &lead, &reference["lead"]);

    let harmony = render_harmony(&p, &song, seed, len, &tuning);
    check_track("harmony", &harmony, &reference["harmony"]);

    let doubles = render_doubles(&p, seed, len, &tuning);
    check_track("doubles", &doubles, &reference["doubles"]);

    let choir = render_choir(&p, seed, len, &tuning);
    check_track("choir", &choir, &reference["choir"]);
}

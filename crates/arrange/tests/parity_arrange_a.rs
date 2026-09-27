//! Parity tests against tests/parity/arrange_a.js reference dumps in
//! ref/parity/arrange_a. Run tests/parity/gen.sh first.
//!
//! Guitar, bass and harp all draw from the same `Rng` stream JS uses
//! (`rngFor(seed,'gtr'|'bass'|'harp')`) and route every draw through
//! `sfcore::js`. Measured: guitar, bass and harp are all bit-exact (err 0)
//! on every case. Bass's earlier 8.2e-8 was misdiagnosed as `sfcore::js`
//! sin/exp being 1 ulp off V8 -- they are bit-exact with node on 300k
//! probes (crates/core/examples/wideprobe.rs) -- the real cause was the
//! sine-tail loop advancing `ph` and testing `idx<0` in the wrong order
//! relative to JS's `for(i=0;i<L&&s+i<len;i++)`, plus a double f32
//! rounding on the accumulate; both are fixed in bass.rs. `TOL` (1e-6)
//! still covers harp's f64 transcendental chain against the f32 dump, and
//! `TOL_GUITAR` (1e-9) covers guitar, tight enough to catch a regression
//! back to double rounding but not so tight that harmless fp reordering
//! fails the build. `TOL_BASS` is exact (0.0) now that the loop and the
//! accumulate both match JS exactly.
//! Each buffer is checked two ways: the first 20 s in
//! full, plus every 97th sample (a prime stride, so it cannot alias any of
//! the engine's periodic structure) across the whole buffer, so a real
//! divergence anywhere in a multi-minute render would still be caught
//! without storing the full buffer on disk. Reported as max-abs-error over
//! the reference peak.

use arrange::bass::gen_bass;
use arrange::guitar::{gen_guitar, guitar_voicing};
use arrange::harp::gen_harp;
use compose::prepare::prepare;
use compose::song::normalize_song;
use compose::theory::parse_chord;
use serde_json::{json, Value};
use sfcore::tuning::Tuning;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/arrange_a"))
}

fn read_bin(name: &str) -> Vec<f32> {
    let p = dir().join(format!("{name}.bin"));
    let bytes = std::fs::read(&p).unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn read_index() -> Value {
    let p = dir().join("index.json");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn case<'a>(idx: &'a Value, name: &str) -> &'a Value {
    idx["cases"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()
}

/// Every `stride`-th sample of `a`, matching tests/parity/arrange_a.js's
/// `writeBuf`.
fn strided(a: &[f32], stride: usize) -> Vec<f32> {
    a.iter().step_by(stride).copied().collect()
}

fn err_metric(rust: &[f32], reference: &[f32]) -> f64 {
    assert_eq!(rust.len(), reference.len(), "length mismatch");
    let mut peak = 0.0f64;
    let mut maxerr = 0.0f64;
    for i in 0..rust.len() {
        let r = reference[i] as f64;
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

const SEED: u32 = 1234;
// Guitar is now bit-exact (measured 0 on every case, both head and the
// full-duration strided sample) after fixing the sympathetic-string
// accumulator's double rounding (`y[i]+=d as f32` rounded twice; JS rounds
// the f64 sum once). Kept as a small nonzero bound rather than requiring
// exact equality so a future genuinely negligible fp reordering does not
// fail the build over noise, but any measured error here is a regression.
const TOL_GUITAR: f64 = 1e-9;
// Harp routes through sfcore::js's sin/exp against an f32-dumped reference;
// measured worst case here is far below this.
const TOL: f64 = 1e-6;
// Bass is bit-exact against the JS reference (measured 0 on every case,
// head and full-duration strided samples alike) now that the sine-tail
// loop's bounds check and phase advance are ordered like JS's
// `for(i=0;i<L&&s+i<len;i++)` and the accumulate rounds once, not twice.
const TOL_BASS: f64 = 0.0;

/// Loads the exact song literal tests/parity/arrange_a.js dumped
/// (`JSON.stringify`d verbatim from tests/formtest.js / src/demo.js), so the
/// input is byte-identical to the one the JS reference ran on rather than a
/// hand-transcribed copy.
fn load_song_json(name: &str) -> Value {
    let p = dir().join(format!("{name}.json"));
    let s = std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn blues_song() -> Value {
    load_song_json("blues_song")
}

fn sixeight_song() -> Value {
    load_song_json("sixeight_song")
}

fn demo_song() -> Value {
    load_song_json("demo_song")
}

fn prepared_for(raw: &Value) -> (compose::song::Song, compose::form::Form, compose::timeline::Timeline) {
    let song = normalize_song(raw).expect("normalize_song");
    let p = prepare(&song, SEED, None);
    (song, p.form, p.timeline)
}

fn check_song(case_name: &str, raw: &Value) {
    let idx = read_index();
    let c = case(&idx, case_name);
    let (song, form, tl) = prepared_for(raw);
    let tuning = Tuning::default();

    let g = gen_guitar(&song, &form, &tl, SEED, &tuning);
    let b = gen_bass(&song, &form, &tl, SEED);
    let h = gen_harp(&song, &form, &tl, SEED);

    for (label, rust, info_key) in [("guitar", &g, "guitar_info"), ("bass", &b, "bass_info"), ("harp", &h, "harp_info")] {
        let tol = if label == "guitar" {
            TOL_GUITAR
        } else if label == "bass" {
            TOL_BASS
        } else {
            TOL
        };
        let info = &c[info_key];
        let full_len = info["full_len"].as_u64().unwrap() as usize;
        let dumped_len = info["dumped_len"].as_u64().unwrap() as usize;
        let stride = info["stride"].as_u64().unwrap() as usize;
        assert_eq!(rust.len(), full_len, "{case_name}/{label}: length mismatch");

        let bin_name = format!("{case_name}_{label}");
        let head = read_bin(&bin_name);
        let err = err_metric(&rust[..dumped_len], &head);
        assert!(err <= tol, "{case_name}/{label}: head err {err} > {tol}");

        let stride_bin = format!("{case_name}_{label}_stride");
        let ref_strided = read_bin(&stride_bin);
        let rust_strided = strided(rust, stride);
        let err2 = err_metric(&rust_strided, &ref_strided);
        assert!(err2 <= tol, "{case_name}/{label}: strided full-duration err {err2} > {tol}");
    }
}

#[test]
fn demo_auto_matches_js() {
    check_song("demo_auto", &demo_song());
}

#[test]
fn demo_strum_matches_js() {
    let mut raw = demo_song();
    raw["guitar"] = json!("strum");
    check_song("demo_strum", &raw);
}

#[test]
fn demo_fingerpick_matches_js() {
    let mut raw = demo_song();
    raw["guitar"] = json!("fingerpick");
    check_song("demo_fingerpick", &raw);
}

#[test]
fn demo_travis_matches_js() {
    let mut raw = demo_song();
    raw["guitar"] = json!("travis");
    check_song("demo_travis", &raw);
}

#[test]
fn demo_arpeggio_matches_js() {
    let mut raw = demo_song();
    raw["guitar"] = json!("arpeggio");
    check_song("demo_arpeggio", &raw);
}

#[test]
fn blues_matches_js() {
    check_song("blues", &blues_song());
}

#[test]
fn sixeight_matches_js() {
    check_song("sixeight", &sixeight_song());
}

#[test]
fn guitar_voicing_matches_js() {
    let idx = read_index();
    let c = case(&idx, "guitar_voicing");
    let chords: Vec<String> = c["chords"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let reference = read_bin("guitar_voicing");
    for (i, name) in chords.iter().enumerate() {
        let ch = parse_chord(name);
        let v = guitar_voicing(&ch);
        for s in 0..6 {
            let expect = reference[i * 6 + s];
            let got = v[s].map(|x| x as f32).unwrap_or(-1.0);
            assert_eq!(got, expect, "chord {name} string {s}");
        }
    }
}

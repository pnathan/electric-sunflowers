//! Full-song parity test against tests/parity/song.js's reference dump
//! (ref/parity/song.json). Run tests/parity/gen.sh first.
//!
//! Four cases, matching the JS reference exactly:
//!  - "demo": DEMO_SONG, seed 1234, voice 'auto', every track enabled.
//!  - "blues": the 12-bar-blues literal from tests/formtest.js, seed 7,
//!    voice 'alto', every track enabled, no style applied (breakLead stays
//!    unset/'both').
//!  - "blues_guitar_lead": the same literal with style 'blues' applied
//!    (lead:'guitar'), same seed/voice.
//!  - "blues_violin_lead": the same literal with style 'oldtime' applied
//!    (lead:'violin', and turns the violin band track on), same seed/voice.
//!
//! Each of `renderSong`'s raw tracks (`RenderedSong::raw_tracks`, in `TRACKS`
//! order) and the final mix (`mix`) are compared on their win25/win50/win75
//! windows (chosen in song.js by energy, not a fixed fraction: win50 is the
//! 2s window around the highest-RMS 0.5s block, win25/win75 are the
//! 25%/75% points of the channel's non-silent span), a whole-buffer
//! stride-97 sample, and sum/sumsq over the whole buffer (f64, index order,
//! matching the JS reference's accumulation order). A channel silent for
//! its entire length is recorded with `silent:true`, and every non-silent
//! reference window is asserted to actually carry a nonzero peak (so a
//! window can't silently degenerate into a no-op check). Reports max abs
//! error / peak per track/channel to stderr.

use compose::song::{normalize_song, Band as SongBand, BreakLead, Song};
use compose::voices::Voice;
use dsp::mix::TRACKS;
use engine::{demo_song, mix, render_song};
use serde_json::Value;
use sfcore::js::{clamp, round};
use sfcore::tuning::Tuning;
use songwriter::styles::apply_style;
use std::path::PathBuf;
use std::time::Instant;

/// Ports styles.js applyStyle(song,key) onto a `compose::song::Song`, same
/// as `sunflower::style_glue::apply_style_to_song` (duplicated here rather
/// than depended on, to avoid a cross-crate test-only dependency on the
/// `sunflower` binary crate).
fn apply_style_to_song(song: &mut Song, key: &str) {
    let arr = apply_style(key, &song.meter_name).unwrap_or_else(|| panic!("unknown style: {key}"));
    song.style = Some(key.to_string());
    song.guitar = arr.guitar.to_string();
    song.break_lead = Some(BreakLead::from_str(arr.break_lead));
    song.band = SongBand {
        drums: arr.drums.to_string(),
        bass: arr.band.bass,
        harmony_guitar: arr.band.harmony_guitar,
        harp: arr.band.harp,
        violin: arr.band.violin,
        choir: arr.band.choir,
        harmonies: arr.band.harmonies,
        doubles: arr.band.doubles,
    };
    if let Some((lo, hi)) = arr.tempo_clamp {
        song.tempo = round(clamp(song.tempo, lo, hi));
    }
}

fn ref_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/song.json"))
}

fn reference() -> Value {
    let s = std::fs::read_to_string(ref_path())
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", ref_path().display()));
    serde_json::from_str(&s).unwrap()
}

fn f64_arr(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

/// max abs error over reference peak, over a slice comparison. Returns
/// (relative error, peak) so a silent track (peak 0) is reported honestly
/// rather than folded into "0 error".
fn err_metric(rust: &[f32], reference: &[f64]) -> (f64, f64) {
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
        (maxerr, peak)
    } else {
        (maxerr / peak, peak)
    }
}

/// Checks one channel against its reference entry (windows, strided
/// sample, sum/sumsq), tracking the worst relative error seen so the
/// caller can report a single max-abs-error/peak number per channel.
fn check_channel(name: &str, buf: &[f32], chan_ref: &Value, tol: f64) -> f64 {
    let len = chan_ref["len"].as_u64().unwrap() as usize;
    assert_eq!(buf.len(), len, "{name}: length mismatch");

    let mut worst = 0.0f64;
    // song.js's energyWindows: a channel silent for its whole length has no
    // energy to find, so win25/win50/win75 fall back to fixed points and
    // stay all-zero; a non-silent channel's windows are chosen to land on
    // real signal, so each must carry a nonzero peak.
    let silent = chan_ref["silent"].as_bool().unwrap_or(false);

    for win_key in ["win25", "win50", "win75"] {
        let w = &chan_ref[win_key];
        let start = w["start"].as_u64().unwrap() as usize;
        let data = f64_arr(&w["data"]);
        let (e, peak) = err_metric(&buf[start..start + data.len()], &data);
        if e > worst {
            worst = e;
        }
        assert!(e < tol, "{name} {win_key}: relative error {e} >= {tol}");
        if !silent {
            assert!(peak > 0.0, "{name} {win_key}: reference window is silent but chan_ref says non-silent");
        }
    }

    let stride = chan_ref["stride"].as_u64().unwrap() as usize;
    let strided_ref = f64_arr(&chan_ref["strided"]);
    let strided_rust: Vec<f32> = buf.iter().step_by(stride).copied().collect();
    let (e, _) = err_metric(&strided_rust, &strided_ref);
    if e > worst {
        worst = e;
    }
    assert!(e < tol, "{name} strided: relative error {e} >= {tol}");

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
    let e_sumsq = (sumsq - ref_sumsq).abs() / scale;
    if e_sumsq > worst {
        worst = e_sumsq;
    }
    assert!(e_sumsq < tol, "{name}: sumsq {sumsq} vs ref {ref_sumsq}");
    // `sum` is a near-total cancellation of an oscillating signal, so it is
    // checked against an absolute bound derived from the signal's actual
    // scale (sumsq) and length, not against itself (see parity_vocals.rs).
    let sum_bound = tol * (ref_sumsq.max(0.0) * len as f64).sqrt().max(1.0);
    assert!((sum - ref_sum).abs() < sum_bound, "{name}: sum {sum} vs ref {ref_sum} (bound {sum_bound})");

    worst
}

fn check_track(name: &str, chs: &[Vec<f32>], track_ref: &Value, tol: f64) {
    if track_ref.is_null() {
        assert!(chs.is_empty() || chs.iter().all(|c| c.iter().all(|&v| v == 0.0)), "{name}: JS has no track but Rust produced signal");
        return;
    }
    let chs_ref = track_ref.as_array().unwrap();
    assert_eq!(chs.len(), chs_ref.len(), "{name}: channel count mismatch");
    let mut worst = 0.0f64;
    for (i, (c, cr)) in chs.iter().zip(chs_ref.iter()).enumerate() {
        let e = check_channel(&format!("{name}[{i}]"), c, cr, tol);
        if e > worst {
            worst = e;
        }
    }
    eprintln!("  {name}: max relative error {worst:.3e} (tol {tol:.1e})");
}

// Same rationale as parity_vocals.rs's TOL: sfcore::js's transcendentals
// are up to ~1 ulp off V8 per call, and errors compound over the mix's
// EQ/compression/reverb chain (more filter stages than a bare voice
// track). Measured worst case across both cases' raw tracks and the final
// mix: 1.7e-7 relative to peak (demo mix L). TOL is kept two orders of
// magnitude above that, tight enough to catch a real divergence (a wrong
// rng draw, a missed note, a dropped track) but not so tight it fails on
// harmless fp-reordering noise.
const TOL: f64 = 1e-5;

fn run_case(name: &str, song_json: &Value, seed: u32, voice: Option<Voice>, style_key: Option<&str>) {
    eprintln!("case {name}:");
    let mut song = normalize_song(song_json).expect("song normalizes");
    if let Some(key) = style_key {
        apply_style_to_song(&mut song, key);
    }
    let tuning = Tuning::default();

    let t0 = Instant::now();
    let mut rendered = render_song(&song, seed, voice, &tuning, None);
    let render_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = Instant::now();
    let (l, r) = mix(&mut rendered, |_t| true, seed);
    let mix_ms = t1.elapsed().as_secs_f64() * 1000.0;
    eprintln!("  rust render {render_ms:.0} ms, mix {mix_ms:.0} ms, len {}", rendered.len);

    let reference = reference();
    let case_ref = &reference[name];
    assert_eq!(case_ref["seed"].as_u64().unwrap() as u32, seed);
    assert_eq!(case_ref["len"].as_u64().unwrap() as usize, rendered.len);
    eprintln!(
        "  node render {} ms, mix {} ms",
        case_ref["renderMs"].as_u64().unwrap(),
        case_ref["mixMs"].as_u64().unwrap()
    );

    for (key, chs) in &rendered.raw_tracks {
        check_track(key, chs, &case_ref["tracks"][key], TOL);
    }
    // Every TRACKS key the JS reference could have populated is covered
    // (raw_tracks is built in TRACKS order in render.rs), so this also
    // confirms no track was silently dropped.
    assert_eq!(rendered.raw_tracks.len(), TRACKS.len());

    let el = check_channel("mix[L]", &l, &case_ref["mix"]["L"], TOL);
    let er = check_channel("mix[R]", &r, &case_ref["mix"]["R"], TOL);
    eprintln!("  mix: max relative error L {el:.3e}, R {er:.3e} (tol {TOL:.1e})");
}

#[test]
fn demo_song_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    run_case("demo", &demo_song(), 1234, None, None);
}

/// tests/formtest.js's 12-bar-blues literal, copied verbatim (see song.js).
fn blues_literal() -> Value {
    let l = |syl: &str, ph: &str, ch: &[&str]| {
        serde_json::json!({"syl": syl, "ph": ph, "chords": ch})
    };
    serde_json::json!({
        "title": "Rent Day Blues", "note": "", "key": "E", "mode": "mixolydian", "meter": "4/4", "tempo": 84,
        "guitar": "travis", "voice": "baritone",
        "band": {"drums": "brushes", "bass": true, "harmonyGuitar": true, "harp": false, "violin": false, "choir": false, "harmonies": false, "doubles": false},
        "sections": [
            {"type": "intro", "chords": ["E7", "A7", "E7", "B7"]},
            {"type": "verse", "lines": [
                l("the *land-lord *knocks at *half past *eight", "dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t", &["E7", "E7", "E7", "E7"]),
                l("the *land-lord *knocks at *half past *eight", "dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t", &["A7", "A7", "E7", "E7"]),
                l("I *told him *twice the *check is *late", "ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t", &["B7", "A7", "E7", "B7"])
            ]},
            {"type": "verse", "lines": [
                l("my *coat is *thin, my *boots are *worn", "m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n", &["E7", "E7", "E7", "E7"]),
                l("my *coat is *thin, my *boots are *worn", "m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n", &["A7", "A7", "E7", "E7"]),
                l("but I *sing so *loud the *roof gets *torn", "b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n", &["B7", "A7", "E7", "B7"])
            ]},
            {"type": "interlude", "chords": ["E7", "A7", "E7", "E7", "B7", "A7", "E7", "B7"]},
            {"type": "outro", "chords": ["E7", "A7", "E7", "E7"]}
        ]
    })
}

#[test]
fn blues_literal_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    run_case("blues", &blues_literal(), 7, Some(Voice::Alto), None);
}

/// Style 'blues' (lead:'guitar'): the violin break is silenced instead of
/// played, and applyStyle's tempo clamp also fires (blues's only declared
/// meter, 4/4, matches BLUES's own meter).
#[test]
fn blues_guitar_lead_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    run_case("blues_guitar_lead", &blues_literal(), 7, Some(Voice::Alto), Some("blues"));
}

/// Style 'oldtime' (lead:'violin'): the harmony-guitar break is silenced
/// instead, and the style's band turns the violin track on (BLUES's own
/// band leaves it off), so this also exercises a track BLUES alone never
/// renders as non-silent.
#[test]
fn blues_violin_lead_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    run_case("blues_violin_lead", &blues_literal(), 7, Some(Voice::Alto), Some("oldtime"));
}

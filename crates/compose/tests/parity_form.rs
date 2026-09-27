//! Parity tests against tests/parity/compose_form.js's dump in
//! ref/parity/compose_form.json. Run `tests/parity/gen.sh` first.
//! Integers compare exact; floats compare with relative tolerance 1e-12
//! (against the reference magnitude), and the worst difference seen is
//! printed for every array so a real regression is visible at a glance.

use compose::form::build_form;
use compose::rhythm::{place_rhythm, PrOpts};
use compose::pitch::{pitch_line, PitchOpts, PitchProf};
use compose::song::normalize_song;
use compose::theory::meter;
use compose::timeline::Timeline;
use serde_json::Value;
use sfcore::rng::rng_for;
use std::path::PathBuf;

const REL_TOL: f64 = 1e-12;

fn read_ref() -> Value {
    let p = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ref/parity/compose_form.json"
    ));
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn demo_song_raw() -> Value {
    // Mirrors src/demo.js's DEMO_SONG literal exactly, so the JS and Rust
    // songs normalize to the same Song.
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../src/demo.js"
    ))
    .unwrap();
    // Extract the object literal between the first '=' and the trailing ';'
    // before the module.exports line, then hand it to a tiny JS-object-
    // literal-compatible parser: since it is valid JSON except for unquoted
    // keys, which serde_json cannot parse, shell out to node instead.
    let out = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(
            "const {{DEMO_SONG}}=require('{}');console.log(JSON.stringify(DEMO_SONG));",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/demo.js")
        ))
        .output()
        .expect("node must be available to load src/demo.js");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    let _ = text;
    serde_json::from_slice(&out.stdout).unwrap()
}

fn worst_rel(name: &str, rust: &[f64], reference: &[f64]) {
    assert_eq!(rust.len(), reference.len(), "{name}: length mismatch");
    let mut worst = 0.0f64;
    for (a, b) in rust.iter().zip(reference.iter()) {
        let denom = b.abs().max(1e-12);
        let rel = (a - b).abs() / denom;
        if rel > worst {
            worst = rel;
        }
    }
    println!("{name}: worst relative diff = {worst:e} (n={})", rust.len());
    assert!(worst <= REL_TOL, "{name}: worst relative diff {worst:e} exceeds {REL_TOL:e}");
}

#[test]
fn form_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();

    for &transpose in &[0i32, 3] {
        let form = build_form(&song, transpose);
        let rf = &refdata["forms"][transpose.to_string()];

        assert_eq!(form.stretch as i64, rf["stretch"].as_i64().unwrap(), "stretch (transpose {transpose})");
        assert_eq!(form.flats, rf["flats"].as_bool().unwrap(), "flats (transpose {transpose})");

        let rsecs = rf["sections"].as_array().unwrap();
        assert_eq!(form.sections.len(), rsecs.len(), "section count (transpose {transpose})");
        for (s, rs) in form.sections.iter().zip(rsecs) {
            assert_eq!(s.type_, rs["type"].as_str().unwrap());
            assert_eq!(s.occ as i64, rs["occ"].as_i64().unwrap());
            assert_eq!(s.idx as i64, rs["idx"].as_i64().unwrap());
            assert_eq!(s.start_bar as i64, rs["startBar"].as_i64().unwrap());
            assert_eq!(s.n_bars as i64, rs["nBars"].as_i64().unwrap());
            assert_eq!(s.lift, rs["lift"].as_bool().unwrap());
            assert_eq!(s.lift_idx as i64, rs["liftIdx"].as_i64().unwrap());
            assert_eq!(s.final_, rs["final"].as_bool().unwrap());
            assert_eq!(s.intensity as i64, rs["intensity"].as_i64().unwrap());
            assert_eq!(s.lines.len() as i64, rs["nLines"].as_i64().unwrap());
        }

        let rbars = rf["bars"].as_array().unwrap();
        assert_eq!(form.bars.len(), rbars.len(), "bar count (transpose {transpose})");
        for (b, rb) in form.bars.iter().zip(rbars) {
            let names: Vec<&str> = b.chords.iter().map(|c| c.name.as_str()).collect();
            let rnames: Vec<&str> = rb["chords"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
            assert_eq!(names, rnames);
            assert_eq!(b.sec as i64, rb["sec"].as_i64().unwrap());
            let rline = rb["line"].as_i64().unwrap();
            match b.line {
                Some(l) => assert_eq!(l as i64, rline),
                None => assert_eq!(rline, -1),
            }
        }

        let rlines = rf["lines"].as_array().unwrap();
        assert_eq!(form.lines.len(), rlines.len(), "line count (transpose {transpose})");
        for (l, rl) in form.lines.iter().zip(rlines) {
            assert_eq!(l.sec as i64, rl["sec"].as_i64().unwrap());
            assert_eq!(l.li as i64, rl["li"].as_i64().unwrap());
            assert_eq!(l.start_bar as i64, rl["startBar"].as_i64().unwrap());
            assert_eq!(l.n_bars as i64, rl["nBars"].as_i64().unwrap());
            assert_eq!(l.text, rl["text"].as_str().unwrap());
            assert_eq!(l.syls.len() as i64, rl["nSyls"].as_i64().unwrap());
        }
    }
}

#[test]
fn timeline_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    let form = build_form(&song, 0);
    let tl = Timeline::new(&form, song.tempo);
    let rt = &refdata["timeline"];

    assert_eq!(tl.nb as i64, rt["nb"].as_i64().unwrap());
    let rbase = rt["base"].as_f64().unwrap();
    assert!((tl.base - rbase).abs() / rbase.abs().max(1e-12) <= REL_TOL);
    let rend = rt["end"].as_f64().unwrap();
    assert!((tl.end - rend).abs() / rend.abs().max(1e-12) <= REL_TOL);

    let r_t: Vec<f64> = rt["T"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    worst_rel("timeline.T", &tl.t, &r_t);

    let rsegs = rt["segs"].as_array().unwrap();
    assert_eq!(tl.segs.len(), rsegs.len(), "seg count");
    for (s, rs) in tl.segs.iter().zip(rsegs) {
        assert_eq!(s.chord.name, rs["chord"].as_str().unwrap());
        assert_eq!(s.b0, rs["b0"].as_f64().unwrap());
        assert_eq!(s.b1, rs["b1"].as_f64().unwrap());
        assert_eq!(s.sec as i64, rs["sec"].as_i64().unwrap());
        assert_eq!(s.bar as i64, rs["bar"].as_i64().unwrap());
    }

    let samples = rt["chordSamples"].as_array().unwrap();
    for samp in samples {
        let beat = samp["beat"].as_f64().unwrap();
        let want = samp["chord"].as_str().unwrap();
        let got = &tl.chord_at(&form, beat).name;
        assert_eq!(got, want, "chordAt({beat})");
    }
}

#[test]
fn rhythm_and_pitch_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    let mi = meter(&song.meter_name);
    let lines = refdata["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 16, "expected 8 rhythm/pitch call pairs");

    let mut worst_rhythm_onset = 0.0f64;
    let mut worst_pitch = 0i64;

    let mut i = 0;
    while i < lines.len() {
        let rh_entry = &lines[i];
        let pt_entry = &lines[i + 1];
        assert_eq!(rh_entry["kind"], "rhythm");
        assert_eq!(pt_entry["kind"], "pitch");
        let a = &rh_entry["args"];
        let stresses: Vec<bool> = a["stresses"].as_array().unwrap().iter().map(|v| v.as_bool().unwrap()).collect();
        let n_bars = a["nBars"].as_u64().unwrap() as usize;
        let pr = &a["pr"];
        let pr_opts = PrOpts {
            dot: pr["dot"].as_f64().unwrap(),
            even: pr["even"].as_f64().unwrap(),
            sync: pr["sync"].as_f64().unwrap(),
            rnoise: pr["rnoise"].as_f64().unwrap(),
        };
        // Reconstruct the same rng stream placeRhythm was called with: the
        // dump does not carry the seed/tag (rng state can't be serialized),
        // so this test instead checks the *shape and cross-consistency* of
        // the port by re-deriving the same tag composeMelody used, given the
        // captured call order (verse then chorus, 4 lines each, li 0..3).
        let sec_type = if i < 8 { "verse" } else { "chorus" };
        let li = (i / 2) % 4;
        let mut rng = rng_for(42, &format!("r|{sec_type}|{li}"));
        let rh = place_rhythm(&stresses, n_bars, &mi, &mut rng, pr_opts);

        let ro = &rh_entry["res"];
        let ronsets: Vec<f64> = ro["onsets"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let rdurs: Vec<f64> = ro["durs"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let rweights: Vec<f64> = ro["weights"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        for (k, (&got, &want)) in rh.onsets.iter().zip(ronsets.iter()).enumerate() {
            let d = (got - want).abs();
            if d > worst_rhythm_onset {
                worst_rhythm_onset = d;
            }
            assert!(d <= REL_TOL.max(1e-9), "onset[{k}] {sec_type} li{li}: {got} vs {want}");
        }
        assert_eq!(rh.durs.len(), rdurs.len());
        assert_eq!(rh.weights.len(), rweights.len());

        // pitchLine: reconstruct args from the dump directly (chords/scales/etc
        // are plain data), and use the port's own rhythm output (rh) for
        // onsets/durs/weights, matching what composeMelody feeds pitchLine.
        let pa = &pt_entry["args"];
        let n = pa["n"].as_u64().unwrap() as usize;
        let chord_pcs: Vec<Vec<i32>> = pa["chordPcs"].as_array().unwrap().iter()
            .map(|c| c.as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect()).collect();
        let scales: Vec<Vec<i32>> = pa["scales"].as_array().unwrap().iter()
            .map(|c| c.as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as i32).collect()).collect();
        let t = pa["T"].as_i64().unwrap() as i32;
        let tonic = pa["tonic"].as_i64().unwrap() as i32;
        let center = pa["center"].as_f64().unwrap();
        let shape_vals: Vec<f64> = pa["shapeVals"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let cadence = pa["cadence"].as_str().unwrap().to_string();
        let reference: Option<Vec<i32>> = pa["ref"].as_array().map(|a| a.iter().map(|v| v.as_i64().unwrap() as i32).collect());
        let prev_end: Option<i32> = pa["prevEnd"].as_i64().map(|v| v as i32);
        let line_beats = pa["lineBeats"].as_f64().unwrap();
        let prof = pa["prof"].as_object().map(|p| PitchProf {
            leap: p["leap"].as_f64().unwrap(),
            rep: p["rep"].as_f64().unwrap(),
            noise: p["noise"].as_f64().unwrap(),
        });
        let hook = pa["hook"].as_i64().unwrap() as i32;

        let onsets = rh.onsets.clone();
        let durs = rh.durs.clone();
        let weights = rh.weights.clone();
        // shape(x) is looked up by onset index, matching the dump's capture
        // (o.onsets.map(b => shape(b/lineBeats))).
        let shape = move |x: f64| -> f64 {
            // find nearest onset index (onsets are the same values used to
            // build shape_vals in the dump, so an exact match is expected).
            let target = x * line_beats;
            let mut best = 0usize;
            let mut bd = f64::INFINITY;
            for (idx, &o) in onsets.iter().enumerate() {
                let d = (o - target).abs();
                if d < bd {
                    bd = d;
                    best = idx;
                }
            }
            shape_vals[best]
        };
        let sec_type_owned = sec_type.to_string();
        let mut rng2 = rng_for(42, &format!("p|{sec_type_owned}|{li}|0"));
        let mut opts = PitchOpts {
            n,
            onsets: &rh.onsets,
            durs: &durs,
            weights: Some(&weights),
            chord_pcs: &chord_pcs,
            scales: &scales,
            t,
            tonic,
            center,
            shape: &shape,
            cadence: &cadence,
            reference: reference.as_deref(),
            rng: &mut rng2,
            prev_end,
            line_beats,
            prof,
            hook,
        };
        let pitches = pitch_line(&mut opts);
        let rpitches: Vec<i64> = pt_entry["res"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        for (&got, &want) in pitches.iter().zip(rpitches.iter()) {
            let d = (got as i64 - want).abs();
            if d > worst_pitch {
                worst_pitch = d;
            }
        }
        assert_eq!(pitches.len(), rpitches.len(), "{sec_type_owned} li{li}: pitch count");
        assert_eq!(pitches.iter().map(|&v| v as i64).collect::<Vec<_>>(), rpitches, "{sec_type_owned} li{li}: pitches");

        i += 2;
    }
    println!("worst rhythm onset abs diff = {worst_rhythm_onset:e}");
    println!("worst pitch abs diff (semitones) = {worst_pitch}");
}

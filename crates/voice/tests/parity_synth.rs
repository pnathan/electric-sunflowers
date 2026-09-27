//! Parity for synthVoice/renderVoice against tests/parity/voice.js's dump in
//! ref/parity/voice/. Run tests/parity/gen.sh first.
//!
//! Each case's audio is compared sample by sample and reported as max abs
//! error / peak reference amplitude.
//!
//! Investigation of the sub-ulp differences (worst measured: 6.15e-9 rel on
//! `doubles`, 3.02e-9 on `sing2_alto_2`, both at a sample deep into a long
//! render; short cases like `vowels` and `choir` are exact, rel 0):
//!  - `voice_controls` (parity_controls.rs) is bit-exact (tolerance 0) for
//!    every case, including the long `demo_lead_30` one, so the mismatch is
//!    not in the control tracks (F1/F2/F3/AV/AH/AF/M/...) fed into
//!    synthVoice, nor in any `Rng`/`gauss` draw (those are shared with
//!    voiceControls's vibrato/drift pass, already proven exact).
//!  - Every per-sample synthVoice operation was checked against engine.js
//!    line by line for matching operand order (the resonator recurrence,
//!    the DC blocker, the tilt/formant mix) and all match; f32-vs-f64
//!    rounding is not a candidate either, since ry1/ry2/vbl1/vbl2/fy1/fy2/
//!    tiltY/tiltY2/etc. are f64 (Float64Array in JS, `f64` locals here) for
//!    the whole per-sample recurrence, with the single f32 store
//!    (`f32r(d)`) only at the very end, on both sides.
//!  - That leaves sfcore::js's transcendentals (`sin`/`cos`/`exp` inside the
//!    per-hop `res()` resonator-coefficient calls, and `exp`/`sin` in
//!    lfTable): crates/core/tests/parity_math.rs checks these against node
//!    on 200k inputs plus hard cases, not exhaustively, so an input outside
//!    that sample landing on a genuine (rare) 1-ulp difference from V8's
//!    algorithm remains possible and was not ruled out further at this
//!    effort level. A long render's per-sample IIR recurrences (the
//!    resonator bank, the DC blocker) compound one such ulp over hundreds
//!    of thousands of samples, which matches both which cases show the
//!    error (the long ones) and its size (still far below audible or
//!    visible-on-a-waveform magnitude). Not chased further; TOL_REL below
//!    is set from the measured worst case with headroom, not loosened
//!    blindly.
//! `TOL_REL` is 1e-7: about 16x the worst measured relative error above
//! (6.151103e-9 on `doubles`), matching crates/dsp/tests/parity_dsp.rs's
//! convention of reporting the tightest passing bound rather than 1e-6
//! (which was a much looser catch-all placeholder).

use serde_json::Value;
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use std::path::PathBuf;

use compose::voices::{voice_params, Voice, VoiceParams};
use voice::controls::{VoiceNote, VoiceOpts};
use voice::synth::render_voice;

fn ref_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/voice"))
}

fn read_index() -> Value {
    let p = ref_dir().join("index.json");
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn case_len(index: &Value, name: &str) -> usize {
    for c in index["cases"].as_array().unwrap() {
        if c["name"] == name {
            return c["len"].as_u64().unwrap() as usize;
        }
    }
    panic!("no case named {name} in index.json");
}

fn read_case(name: &str, len: usize) -> Vec<f32> {
    let bytes = std::fs::read(ref_dir().join(format!("{name}.bin")))
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({name}.bin)"));
    assert_eq!(bytes.len(), len * 4, "{name}: unexpected .bin length");
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

/// Compares one case, reporting max abs error and max abs error / peak.
/// Target below `TOL_REL`; see the module doc comment for why exact 0
/// sometimes is not reached.
const TOL_REL: f32 = 1e-7;

fn assert_case_exact(name: &str, got: &[f32], index: &Value) {
    let len = case_len(index, name);
    let want = read_case(name, len);
    assert_eq!(got.len(), want.len(), "{name}: length mismatch");
    let mut worst = (0usize, 0.0f32);
    let mut peak = 0.0f32;
    for i in 0..got.len() {
        let d = (got[i] - want[i]).abs();
        if d > worst.1 {
            worst = (i, d);
        }
        if want[i].abs() > peak {
            peak = want[i].abs();
        }
    }
    let rel = if peak > 0.0 { worst.1 / peak } else { 0.0 };
    println!("{name}: max abs err {} (rel {rel}), peak {peak}, at sample {}", worst.1, worst.0);
    assert!(
        rel <= TOL_REL,
        "{name}: mismatch at sample {} (got {}, want {}), rel {rel} > {TOL_REL}",
        worst.0, got[worst.0], want[worst.0]
    );
}

fn note(t0: f64, t1: f64, midi: i32, ph: Option<&[&str]>, nu: Option<&[&str]>, amp: f64, phrase_start: bool, phrase_end: bool) -> VoiceNote {
    VoiceNote {
        t0,
        t1,
        midi,
        ph: ph.map(|p| p.iter().map(|s| s.to_string()).collect()),
        nu: nu.map(|p| p.iter().map(|s| s.to_string()).collect()),
        amp,
        phrase_start,
        phrase_end,
        grace: None,
        stress: false,
    }
}

fn len_for(len_secs: f64) -> usize {
    (len_secs * sfcore::SR_F).ceil() as usize
}

#[test]
fn vowels_match_js() {
    let index = read_index();
    let vw = ["iy", "ih", "eh", "ae", "aa", "ao", "ow", "uw", "ah", "er"];
    let mut sp = Vec::new();
    let mut t = 0.5f64;
    for v in vw {
        sp.push(note(t, t + 1.2, 52, Some(&["hh", v]), None, 1.0, false, false));
        t += 1.6;
    }
    let len = len_for(t + 1.0);
    let p = voice_params(Voice::Baritone);
    let mut opts = VoiceOpts { seed: Some(3), rng: Some(rng_for(3, "v")), vib_scale: Some(0.0), no_scoop: true, ..Default::default() };
    let tuning = Tuning::default();
    let audio = render_voice(&sp, &p, len, &mut opts, &tuning);
    assert_case_exact("vowels", &audio, &index);
}

fn ph(word: &str) -> Vec<&'static str> {
    match word {
        "the" => vec!["dh", "ax"],
        "dark" => vec!["d", "aa", "r", "k"],
        "tide" => vec!["t", "ay", "d"],
        "turned" => vec!["t", "er", "n", "d"],
        "to" => vec!["t", "uw"],
        "day" => vec!["d", "ey"],
        "take" => vec!["t", "ey", "k"],
        "road" => vec!["r", "ow", "d"],
        "down" => vec!["d", "aw", "n"],
        "town" => vec!["t", "aw", "n"],
        "a" => vec!["ax"],
        "tired" => vec!["t", "ay", "er", "d"],
        "dog" => vec!["d", "ao", "g"],
        "waits" => vec!["w", "ey", "t", "s"],
        "at" => vec!["ae", "t"],
        "door" => vec!["d", "ao", "r"],
        "I" => vec!["ay"],
        "told" => vec!["t", "ow", "l", "d"],
        "you" => vec!["y", "uw"],
        "twice" => vec!["t", "w", "ay", "s"],
        "stay" => vec!["s", "t", "ey"],
        other => panic!("no PH entry for {other}"),
    }
}

const LINES: [&[&str]; 4] = [
    &["the", "dark", "tide", "turned", "to", "day"],
    &["take", "the", "road", "down", "to", "town"],
    &["a", "tired", "dog", "waits", "at", "the", "door"],
    &["I", "told", "you", "twice", "to", "stay"],
];

#[test]
fn sing2_first_4_lines_match_js() {
    let index = read_index();
    let tuning = Tuning::default();
    for (vk, voice) in [("baritone", Voice::Baritone), ("alto", Voice::Alto)] {
        let p = voice_params(voice);
        let base = ((p.lo + p.hi) as f64 / 2.0).round() as i32 - 2;
        let mel = [0, 2, 4, 2, 0, -1, 0, 2, 4];
        for (li, ws) in LINES.iter().enumerate() {
            let mut notes = Vec::new();
            let mut t = 0.5f64;
            let mut k = 0usize;
            for w in *ws {
                let sy = ph(w);
                let d = 0.42;
                notes.push(note(t, t + d * 0.92, base + mel[k % mel.len()], Some(&sy), None, 1.0, k == 0, false));
                t += d;
                k += 1;
            }
            let last = notes.len() - 1;
            notes[last].phrase_end = true;
            notes[last].t1 += 0.4;
            let len = len_for(t + 1.0);
            let seed = 7 + li as u32;
            let mut opts = VoiceOpts { seed: Some(seed), rng: Some(rng_for(seed, "s")), ..Default::default() };
            let audio = render_voice(&notes, &p, len, &mut opts, &tuning);
            assert_case_exact(&format!("sing2_{vk}_{li}"), &audio, &index);
        }
    }
}

fn demo_song_raw() -> Value {
    let out = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(
            "const {{DEMO_SONG}}=require('{}');console.log(JSON.stringify(DEMO_SONG));",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/demo.js")
        ))
        .output()
        .expect("node must be available to load src/demo.js");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn demo_lead_30_notes() -> (Vec<VoiceNote>, VoiceParams, Voice) {
    let raw = demo_song_raw();
    let song = compose::song::normalize_song(&raw).expect("DEMO_SONG normalizes");
    let seed = 1234u32;
    let prepared = compose::prepare::prepare(&song, seed, None);
    let lead: Vec<_> = prepared.comp.lead.iter().take(30).cloned().collect();
    let vnotes = compose::prepare::vocal_notes(&lead, 1.0);
    let notes: Vec<VoiceNote> = vnotes.iter().map(VoiceNote::from).collect();
    let vp = voice_params(prepared.voice);
    (notes, vp, prepared.voice)
}

#[test]
fn demo_lead_30_matches_js() {
    let index = read_index();
    let (notes, vp, _voice) = demo_lead_30_notes();
    let seed = 1234u32;
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { seed: Some(seed ^ 11), rng: Some(rng_for(seed, "lead")), ..Default::default() };
    let tuning = Tuning::default();
    let audio = render_voice(&notes, &vp, len, &mut opts, &tuning);
    assert_case_exact("demo_lead_30", &audio, &index);
}

/// The harmony-voice case (renderSong ~ engine.js line 886): vibScale .8,
/// breathScale 1.2. Uses the same notes/voice-params as demo_lead_30 for a
/// fixed, reproducible input (renderSong's real harmony line differs in
/// content, only the opts shape is what this exercises).
#[test]
fn harmony_voice_matches_js() {
    let index = read_index();
    let (notes, vp, _voice) = demo_lead_30_notes();
    let seed = 1234u32;
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts {
        seed: Some(seed ^ 23),
        rng: Some(rng_for(seed, "harm")),
        vib_scale: Some(0.8),
        breath_scale: Some(1.2),
        ..Default::default()
    };
    let tuning = Tuning::default();
    let audio = render_voice(&notes, &vp, len, &mut opts, &tuning);
    assert_case_exact("harmony", &audio, &index);
}

/// The doubled-melody case (renderSong ~ engine.js line 899), the `k=1` pan
/// tuple: `fs*1.03`, `breath+.05`, `detune:-.06`, `vibScale:.7`,
/// `rateScale:1.07`, `noBreath:true`.
#[test]
fn doubles_match_js() {
    let index = read_index();
    let (notes, mut vp, _voice) = demo_lead_30_notes();
    vp.fs *= 1.03;
    vp.breath += 0.05;
    let seed = 1234u32;
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts {
        seed: Some(seed ^ 32),
        rng: Some(rng_for(seed, "dbl1")),
        detune: Some(-0.06),
        vib_scale: Some(0.7),
        rate_scale: Some(1.07),
        no_breath: true,
        ..Default::default()
    };
    let tuning = Tuning::default();
    let audio = render_voice(&notes, &vp, len, &mut opts, &tuning);
    assert_case_exact("doubles", &audio, &index);
}

#[test]
fn choir_style_notes_match_js() {
    let index = read_index();
    let notes = vec![
        note(0.5, 1.3, 60, None, Some(&["aa"]), 0.8, true, false),
        note(1.3, 2.1, 62, None, Some(&["aa"]), 0.8, false, false),
        note(2.1, 3.0, 64, None, Some(&["aa"]), 0.8, false, true),
    ];
    let mut p = voice_params(Voice::Alto);
    p.breath += 0.05;
    p.fs *= 0.98;
    p.jitter *= 1.6;
    p.shimmer *= 1.4;
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts {
        seed: Some(999),
        rng: Some(rng_for(999, "ch00")),
        rd_scale: Some(1.15),
        hf_gain: Some(0.0),
        n_high: Some(2.0), // CHH default
        av_tau: Some(0.05),
        vib_scale: Some(0.7),
        rate_scale: Some(0.9),
        detune: Some(0.05),
        no_scoop: true,
        no_breath: true,
        glide: Some(0.05),
        ..Default::default()
    };
    let tuning = Tuning::default();
    let audio = render_voice(&notes, &p, len, &mut opts, &tuning);
    assert_case_exact("choir", &audio, &index);
}

/// VF.legacy=0: the non-legacy stop branch, audio-level parity.
#[test]
fn legacy0_stops_match_js() {
    let index = read_index();
    let notes = vec![
        note(0.5, 0.9, 55, Some(&["p", "ae", "t"]), None, 1.0, true, false),
        note(0.95, 1.35, 57, Some(&["t", "aa", "k"]), None, 1.0, false, false),
        note(1.4, 1.9, 55, Some(&["d", "ih", "g"]), None, 1.0, false, true),
    ];
    let p = voice_params(Voice::Baritone);
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { seed: Some(42), rng: Some(rng_for(42, "v")), ..Default::default() };
    let mut tuning = Tuning::default();
    tuning.vf.legacy = 0.0;
    let audio = render_voice(&notes, &p, len, &mut opts, &tuning);
    assert_case_exact("legacy0_stops", &audio, &index);
}

/// Explicit grace notes, audio-level parity.
#[test]
fn grace_notes_match_js() {
    let index = read_index();
    let mut notes = vec![
        note(0.5, 1.0, 60, Some(&["m", "ae"]), None, 1.0, true, false),
        note(1.05, 1.6, 63, Some(&["l", "ey"]), None, 1.0, false, false),
        note(1.65, 2.3, 60, Some(&["n", "ow"]), None, 1.0, false, true),
    ];
    notes[0].grace = Some(58);
    notes[1].grace = Some(61);
    let p = voice_params(Voice::Alto);
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { seed: Some(55), rng: Some(rng_for(55, "v")), ..Default::default() };
    let tuning = Tuning::default();
    let audio = render_voice(&notes, &p, len, &mut opts, &tuning);
    assert_case_exact("grace_notes", &audio, &index);
}

/// synthVoice is 24% of render time (CLAUDE.md); this is not a strict
/// benchmark (no criterion here) but records ns/sample so a regression shows
/// up in `cargo test -- --nocapture`.
#[test]
fn timing_ns_per_sample() {
    let (notes, vp, _voice) = demo_lead_30_notes();
    let seed = 1234u32;
    let len = len_for(notes.last().unwrap().t1 + 1.0);
    let tuning = Tuning::default();
    let mut opts = VoiceOpts { seed: Some(seed ^ 11), rng: Some(rng_for(seed, "lead")), ..Default::default() };
    let start = std::time::Instant::now();
    let audio = render_voice(&notes, &vp, len, &mut opts, &tuning);
    let elapsed = start.elapsed();
    let ns_per_sample = elapsed.as_nanos() as f64 / audio.len() as f64;
    println!("demo_lead_30: {} samples in {:?} = {:.1} ns/sample", audio.len(), elapsed, ns_per_sample);
}

//! Parity for voiceControls against tests/parity/voice_controls.js's dump in
//! ref/parity/voice_controls/. Run tests/parity/gen.sh first.
//!
//! Every track is stored (both sides) as an f32 written through the same
//! "store into a Float32Array" rounding at each mutation, so tracks compare
//! bit-exact (tolerance 0) unless noted otherwise.

use serde_json::Value;
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use sfcore::HOP;
use std::path::PathBuf;

use compose::voices::{voice_params, Voice, VoiceParams};
use voice::controls::{voice_controls, VoiceControls, VoiceNote, VoiceOpts};

const ORDER: [&str; 12] = ["av", "ah", "af", "ff", "fbw", "f1", "f2", "f3", "nas", "m", "vb", "b1x"];

fn ref_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/voice_controls"))
}

fn read_index() -> Value {
    let p = ref_dir().join("index.json");
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn case_nf(index: &Value, name: &str) -> usize {
    for c in index["cases"].as_array().unwrap() {
        if c["name"] == name {
            return c["nF"].as_u64().unwrap() as usize;
        }
    }
    panic!("no case named {name} in index.json");
}

/// Reads one case's .bin into the twelve tracks, keyed by ORDER's names.
fn read_case(name: &str, n_f: usize) -> std::collections::HashMap<&'static str, Vec<f32>> {
    let bytes = std::fs::read(ref_dir().join(format!("{name}.bin")))
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({name}.bin)"));
    assert_eq!(bytes.len(), n_f * 12 * 4, "{name}: unexpected .bin length");
    let mut out = std::collections::HashMap::new();
    for (i, key) in ORDER.iter().enumerate() {
        let mut v = Vec::with_capacity(n_f);
        for j in 0..n_f {
            let off = (i * n_f + j) * 4;
            v.push(f32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()));
        }
        out.insert(*key, v);
    }
    out
}

fn tracks_of(ctl: &VoiceControls) -> std::collections::HashMap<&'static str, &[f32]> {
    let mut m = std::collections::HashMap::new();
    m.insert("av", ctl.av.as_slice());
    m.insert("ah", ctl.ah.as_slice());
    m.insert("af", ctl.af.as_slice());
    m.insert("ff", ctl.ff.as_slice());
    m.insert("fbw", ctl.fbw.as_slice());
    m.insert("f1", ctl.f1.as_slice());
    m.insert("f2", ctl.f2.as_slice());
    m.insert("f3", ctl.f3.as_slice());
    m.insert("nas", ctl.nas.as_slice());
    m.insert("m", ctl.m.as_slice());
    m.insert("vb", ctl.vb.as_slice());
    m.insert("b1x", ctl.b1x.as_slice());
    m
}

/// Compares one case exactly (tolerance 0): every f32 sample on every track
/// must match the JS reference bit-for-bit. Reports the worst offender.
fn assert_case_exact(name: &str, got: &VoiceControls, index: &Value) {
    let n_f = case_nf(index, name);
    let want = read_case(name, n_f);
    let got_tracks = tracks_of(got);
    for key in ORDER.iter() {
        let g = got_tracks[key];
        let w = &want[key];
        assert_eq!(g.len(), w.len(), "{name}.{key}: length mismatch");
        let mut worst = (0usize, 0.0f32);
        for i in 0..g.len() {
            let d = (g[i] - w[i]).abs();
            if d > worst.1 {
                worst = (i, d);
            }
        }
        assert_eq!(
            worst.1, 0.0,
            "{name}.{key}: mismatch at frame {} (got {}, want {})",
            worst.0, g[worst.0], w[worst.0]
        );
    }
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

fn n_f_for(len_secs: f64) -> usize {
    let len = (len_secs * sfcore::SR_F).ceil() as usize;
    len.div_ceil(HOP) + 2
}

#[test]
fn vowels_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let index = read_index();
    let vw = ["iy", "ih", "eh", "ae", "aa", "ao", "ow", "uw", "ah", "er"];
    let mut sp = Vec::new();
    let mut t = 0.5f64;
    for v in vw {
        sp.push(note(t, t + 1.2, 52, Some(&["hh", v]), None, 1.0, false, false));
        t += 1.6;
    }
    let n_f = n_f_for(t + 1.0);
    let p = voice_params(Voice::Baritone);
    let mut opts = VoiceOpts { rng: Some(rng_for(3, "v")), vib_scale: Some(0.0), no_scoop: true, ..Default::default() };
    let tuning = Tuning::default();
    let ctl = voice_controls(&sp, &p, n_f, &mut opts, &tuning);
    assert_case_exact("vowels", &ctl, &index);
}

/// A slice of tests/words.js's PH table: only the words the first 4 LINES
/// entries use.
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

/// tests/words.js's LINES, first 4 entries only (each word here is a single
/// syllable in PH, so no `[[...],[...]]` multi-syllable case appears).
const LINES: [&[&str]; 4] = [
    &["the", "dark", "tide", "turned", "to", "day"],
    &["take", "the", "road", "down", "to", "town"],
    &["a", "tired", "dog", "waits", "at", "the", "door"],
    &["I", "told", "you", "twice", "to", "stay"],
];

#[test]
fn sing2_first_4_lines_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
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
                // Every word above is one syllable in PH, so this always
                // renders as a single note per word (j==0, sy.len()==1 ->
                // d=0.42, matching sing2.js's branch for non-multi-syllable
                // words).
                let d = 0.42;
                notes.push(note(
                    t,
                    t + d * 0.92,
                    base + mel[k % mel.len()],
                    Some(&sy),
                    None,
                    1.0,
                    k == 0,
                    false,
                ));
                t += d;
                k += 1;
            }
            let last = notes.len() - 1;
            notes[last].phrase_end = true;
            notes[last].t1 += 0.4;
            let n_f = n_f_for(t + 1.0);
            let mut opts = VoiceOpts { rng: Some(rng_for(7 + li as u32, "s")), ..Default::default() };
            let ctl = voice_controls(&notes, &p, n_f, &mut opts, &tuning);
            assert_case_exact(&format!("sing2_{vk}_{li}"), &ctl, &index);
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

#[test]
fn demo_lead_30_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let index = read_index();
    let raw = demo_song_raw();
    let song = compose::song::normalize_song(&raw).expect("DEMO_SONG normalizes");
    let seed = 1234u32;
    let prepared = compose::prepare::prepare(&song, seed, None);
    let lead: Vec<_> = prepared.comp.lead.iter().take(30).cloned().collect();
    let vnotes = compose::prepare::vocal_notes(&lead, 1.0);
    let notes: Vec<VoiceNote> = vnotes.iter().map(VoiceNote::from).collect();
    let vp: VoiceParams = voice_params(prepared.voice);
    let n_f = n_f_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { rng: Some(rng_for(seed, "lead")), ..Default::default() };
    let tuning = Tuning::default();
    let ctl = voice_controls(&notes, &vp, n_f, &mut opts, &tuning);
    assert_case_exact("demo_lead_30", &ctl, &index);
}

#[test]
fn choir_style_notes_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
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
    let n_f = n_f_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts {
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
    let ctl = voice_controls(&notes, &p, n_f, &mut opts, &tuning);
    assert_case_exact("choir", &ctl, &index);
}

/// VF.legacy=0: the non-legacy stop branch in emit_cons (voiced/voiceless
/// stops going through the vb/burst/aspr path instead of clos/burst/aspr
/// with VF.burst/VF.asp gains).
#[test]
fn legacy0_stops_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let index = read_index();
    let notes = vec![
        note(0.5, 0.9, 55, Some(&["p", "ae", "t"]), None, 1.0, true, false),
        note(0.95, 1.35, 57, Some(&["t", "aa", "k"]), None, 1.0, false, false),
        note(1.4, 1.9, 55, Some(&["d", "ih", "g"]), None, 1.0, false, true),
    ];
    let p = voice_params(Voice::Baritone);
    let n_f = n_f_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { rng: Some(rng_for(42, "v")), ..Default::default() };
    let mut tuning = Tuning::default();
    tuning.vf.legacy = 0.0;
    let ctl = voice_controls(&notes, &p, n_f, &mut opts, &tuning);
    assert_case_exact("legacy0_stops", &ctl, &index);
}

/// Explicit grace notes (`n.grace = Some(midi)`), driving voiceControls's
/// `M[i]=n.grace` backfill for the first part of the note.
#[test]
fn grace_notes_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let index = read_index();
    let mut notes = vec![
        note(0.5, 1.0, 60, Some(&["m", "ae"]), None, 1.0, true, false),
        note(1.05, 1.6, 63, Some(&["l", "ey"]), None, 1.0, false, false),
        note(1.65, 2.3, 60, Some(&["n", "ow"]), None, 1.0, false, true),
    ];
    notes[0].grace = Some(58);
    notes[1].grace = Some(61);
    let p = voice_params(Voice::Alto);
    let n_f = n_f_for(notes.last().unwrap().t1 + 1.0);
    let mut opts = VoiceOpts { rng: Some(rng_for(55, "v")), ..Default::default() };
    let tuning = Tuning::default();
    let ctl = voice_controls(&notes, &p, n_f, &mut opts, &tuning);
    assert_case_exact("grace_notes", &ctl, &index);
}

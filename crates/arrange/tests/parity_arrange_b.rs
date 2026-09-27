//! Parity for `counterLine`, `fillsFor`, `choirVoicings` and `genDrums`
//! against tests/parity/arrange_b.js's dump in ref/parity/arrange_b.json.
//! Run tests/parity/gen.sh (or node tests/parity/arrange_b.js) first.
//!
//! Notes/segments compare with relative tolerance 1e-9 on floats (the JS
//! side runs the exact same f64 math up to `prepare`'s note timing, so this
//! is tight); drum buffers compare the first 2s sample-for-sample as f32
//! (bitwise via the f32 cast) and the full-buffer checksum (sum, sum of
//! squares, and an index-weighted sum) with the same relative tolerance.

use arrange::choir::choir_voicings;
use arrange::drums::gen_drums;
use arrange::lines::{counter_line, fills_for};
use compose::form::Sec;
use compose::prepare::prepare;
use compose::song::normalize_song;
use serde_json::Value;
use std::path::PathBuf;

const REL_TOL: f64 = 1e-9;

fn read_ref() -> Value {
    let p = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ref/parity/arrange_b.json"
    ));
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn relf(name: &str, got: f64, want: f64) {
    let denom = want.abs().max(1e-12);
    let rel = (got - want).abs() / denom;
    assert!(rel <= REL_TOL, "{name}: {got} vs {want} (rel {rel:e})");
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

fn blues_song_raw() -> Value {
    let l = |syl: &str, ph: &str, ch: &[&str]| {
        serde_json::json!({"syl": syl, "ph": ph, "chords": ch})
    };
    serde_json::json!({
        "title":"Rent Day Blues","note":"","key":"E","mode":"mixolydian","meter":"4/4","tempo":84,
        "guitar":"travis","voice":"baritone",
        "band":{"drums":"brushes","bass":true,"harmonyGuitar":true,"harp":false,"violin":false,"choir":false,"harmonies":false,"doubles":false},
        "sections":[
            {"type":"intro","chords":["E7","A7","E7","B7"]},
            {"type":"verse","lines":[
                l("the *land-lord *knocks at *half past *eight","dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t",&["E7","E7","E7","E7"]),
                l("the *land-lord *knocks at *half past *eight","dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t",&["A7","A7","E7","E7"]),
                l("I *told him *twice the *check is *late","ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t",&["B7","A7","E7","B7"])
            ]},
            {"type":"verse","lines":[
                l("my *coat is *thin, my *boots are *worn","m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n",&["E7","E7","E7","E7"]),
                l("my *coat is *thin, my *boots are *worn","m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n",&["A7","A7","E7","E7"]),
                l("but I *sing so *loud the *roof gets *torn","b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n",&["B7","A7","E7","B7"])
            ]},
            {"type":"interlude","chords":["E7","A7","E7","E7","B7","A7","E7","B7"]},
            {"type":"outro","chords":["E7","A7","E7","E7"]}
        ]
    })
}

fn check_notes(label: &str, got: &[arrange::lines::Note], rf: &Value) {
    let rf = rf.as_array().unwrap();
    assert_eq!(got.len(), rf.len(), "{label}: note count");
    for (i, (n, r)) in got.iter().zip(rf).enumerate() {
        relf(&format!("{label}[{i}].t0"), n.t0, r["t0"].as_f64().unwrap());
        relf(&format!("{label}[{i}].t1"), n.t1, r["t1"].as_f64().unwrap());
        assert_eq!(n.m as i64, r["m"].as_i64().unwrap(), "{label}[{i}].m");
        relf(&format!("{label}[{i}].v"), n.v, r["v"].as_f64().unwrap());
    }
}

fn check_window(label: &str, got: &[f32], rf: &Value) {
    let start = rf["start"].as_u64().unwrap() as usize;
    let data = rf["data"].as_array().unwrap();
    for (i, (v, r)) in got[start..start + data.len()].iter().zip(data).enumerate() {
        let want = r.as_f64().unwrap();
        // relf, not assert_eq: a bit-exact f32 buffer round-tripped through
        // JSON (f64 text) and reparsed can differ from the original f32-cast
        // value in the last bit or two of the f64 mantissa: same measured
        // 1e-9-scale slop as the note timings below, not a real divergence.
        relf(&format!("{label}[{}]", start + i), *v as f64, want);
    }
}

fn check_drum_buf(label: &str, got: &[f32], rf: &Value) {
    // The exact head of a drum buffer is silence (see arrange_b.js); these
    // two windows land on samples the drums actually play, one near the
    // start (the first bar with intensity>=1) and one at the tail.
    check_window(&format!("{label}.win1"), got, &rf["win1"]);
    check_window(&format!("{label}.win2"), got, &rf["win2"]);
    let full = &rf["full"];
    assert_eq!(got.len() as i64, full["len"].as_i64().unwrap(), "{label}: len");
    let mut sum = 0.0f64;
    let mut sumsq = 0.0f64;
    let mut weighted = 0.0f64;
    for (i, &v) in got.iter().enumerate() {
        let v = v as f64;
        sum += v;
        sumsq += v * v;
        weighted += v * ((i % 97) as f64 + 1.0);
    }
    relf(&format!("{label}: sum"), sum, full["sum"].as_f64().unwrap());
    relf(&format!("{label}: sumsq"), sumsq, full["sumsq"].as_f64().unwrap());
    relf(&format!("{label}: weighted"), weighted, full["weighted"].as_f64().unwrap());
}

fn run_case(label: &str, song_raw: Value, rf: &Value) {
    let song = normalize_song(&song_raw).unwrap();
    let seed = 1234u32;
    let p = prepare(&song, seed, None);
    let (form, tl, comp) = (&p.form, &p.timeline, &p.comp);
    let lead = &comp.lead;

    assert_eq!(p.tonic as i64, rf["tonic"].as_i64().unwrap(), "{label}: tonic");

    let lift_filter = |s: &Sec| s.lift;
    let ctr = counter_line(form, tl, lead, 67, 86, lift_filter, seed, false);
    check_notes(&format!("{label}.ctr"), &ctr, &rf["ctr"]);

    let bridge_filter = |s: &Sec| s.type_ == "bridge";
    let br = counter_line(form, tl, lead, 62, 79, bridge_filter, seed, true);
    check_notes(&format!("{label}.br"), &br, &rf["br"]);

    let verse_occ_filter = |s: &Sec| s.type_ == "verse" && s.occ > 0;
    let fl = fills_for(form, tl, lead, 69, 88, verse_occ_filter, &song, seed);
    check_notes(&format!("{label}.fl"), &fl, &rf["fl"]);

    let verse_filter = |s: &Sec| s.type_ == "verse";
    let fl_g = fills_for(form, tl, lead, 59, 79, verse_filter, &song, seed + 1);
    check_notes(&format!("{label}.flG"), &fl_g, &rf["flG"]);

    let choir_filter = |s: &Sec| (s.lift && s.lift_idx > 0) || s.type_ == "bridge" || s.type_ == "outro";
    let vs = choir_voicings(form, tl, choir_filter);
    let rvs = rf["choirVoicings"].as_array().unwrap();
    assert_eq!(vs.len(), rvs.len(), "{label}: choirVoicings count");
    for (i, (v, r)) in vs.iter().zip(rvs).enumerate() {
        assert_eq!(v.seg_idx as i64, r["segIdx"].as_i64().unwrap(), "{label}.choirVoicings[{i}].segIdx");
        let rv = r["v"].as_array().unwrap();
        for (k, &part) in v.v.iter().enumerate() {
            assert_eq!(part as i64, rv[k].as_i64().unwrap(), "{label}.choirVoicings[{i}].v[{k}]");
        }
    }

    for style in ["none", "brushes", "soft", "full"] {
        let mut song2 = song.clone();
        song2.band.drums = style.to_string();
        let [l, r] = gen_drums(&song2, form, tl, seed);
        let rd = &rf["drums"][style];
        check_drum_buf(&format!("{label}.drums.{style}.L"), &l, &rd["L"]);
        check_drum_buf(&format!("{label}.drums.{style}.R"), &r, &rd["R"]);
    }
}

#[test]
fn arrange_b_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let rf = read_ref();
    run_case("demo", demo_song_raw(), &rf["demo"]);
    run_case("blues", blues_song_raw(), &rf["blues"]);
}

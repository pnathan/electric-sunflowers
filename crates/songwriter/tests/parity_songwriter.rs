//! Parity tests against tests/parity/songwriter.js's dump in
//! ref/parity/songwriter.json. Run `tests/parity/gen.sh` first.
//!
//! The JS side replaces `Math.random` with `rngFor(seed, tag)()` for the
//! duration of each call, so both sides draw the same sequence of numbers
//! from the same named stream; this test drives the Rust functions with
//! `sfcore::rng::rng_for(seed, tag)` using the identical (seed, tag) pairs.

use serde_json::Value;
use sfcore::rng::rng_for;
use songwriter::prompt::song_prompt;
use songwriter::styles::{apply_style, form_text, style_direction};
use std::path::PathBuf;

fn read_ref() -> Value {
    let p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/songwriter.json"));
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn rand_for(seed: u32, tag: &str) -> impl FnMut() -> f64 {
    let mut r = rng_for(seed, tag);
    move || r.next()
}

fn band_json(band: &songwriter::styles::Band) -> Value {
    serde_json::json!({
        "bass": band.bass,
        "harmonyGuitar": band.harmony_guitar,
        "harp": band.harp,
        "violin": band.violin,
        "choir": band.choir,
        "harmonies": band.harmonies,
        "doubles": band.doubles,
    })
}

fn direction_json(d: &songwriter::styles::Direction) -> Value {
    serde_json::json!({
        "style": d.style,
        "label": d.label,
        "idiom": d.idiom,
        "mode": d.mode,
        "meter": d.meter,
        "tempoLo": d.tempo_lo,
        "tempoHi": d.tempo_hi,
        "form": d.form,
        "guitar": d.guitar,
        "drums": d.drums,
        "band": band_json(&d.band),
        "lead": d.lead,
        "world": d.world,
    })
}

const SEEDS: [u32; 5] = [1, 2, 3, 4, 5];

#[test]
fn style_direction_matches_js_for_every_style() {
    let refdata = read_ref();
    let style_keys: Vec<String> =
        refdata["styleKeys"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();

    for key in &style_keys {
        let ref_cases = refdata["styleDirections"][key].as_array().unwrap();
        for (i, &seed) in SEEDS.iter().enumerate() {
            let mut rand = rand_for(seed, &format!("styleDirection|{key}"));
            let dir = style_direction(Some(key), &mut rand);
            let got = direction_json(&dir);
            assert_eq!(got, ref_cases[i], "style {key} seed {seed}");
        }
    }
}

#[test]
fn style_direction_matches_js_for_null_key() {
    let refdata = read_ref();
    let ref_cases = refdata["nullStyleDirections"].as_array().unwrap();
    for (i, &seed) in SEEDS.iter().enumerate() {
        let mut rand = rand_for(seed, "styleDirection|null");
        let dir = style_direction(None, &mut rand);
        let got = direction_json(&dir);
        assert_eq!(got, ref_cases[i], "null key seed {seed}");
    }
}

#[test]
fn form_text_matches_js_for_every_form() {
    let refdata = read_ref();
    let form_keys: Vec<String> =
        refdata["formKeys"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();

    for fk in &form_keys {
        let r = form_text(fk);
        let rf = &refdata["formTexts"][fk];
        assert_eq!(r.label, rf["label"].as_str().unwrap(), "form {fk} label");
        assert_eq!(r.note, rf["note"].as_str().unwrap(), "form {fk} note");
        assert_eq!(r.text, rf["text"].as_str().unwrap(), "form {fk} text");
        assert_eq!(r.bars as i64, rf["bars"].as_i64().unwrap(), "form {fk} bars");
    }
}

#[test]
fn apply_style_clamps_match_js() {
    let refdata = read_ref();
    let cases = refdata["applyStyleClamps"].as_array().unwrap();

    for c in cases {
        let key = c["key"].as_str().unwrap();
        let meter = c["meter"].as_str().unwrap();
        let tempo_in = c["tempoIn"].as_f64().unwrap();

        match apply_style(key, meter) {
            None => {
                assert_eq!(
                    c.get("unchanged").and_then(|v| v.as_bool()),
                    Some(true),
                    "expected unchanged for unknown style {key}"
                );
                assert_eq!(tempo_in, c["tempoOut"].as_f64().unwrap());
            }
            Some(arr) => {
                let (lo, hi) = arr.tempo_clamp.expect("style declares tempo for its own meter");
                let clamped = sfcore::js::round(sfcore::js::clamp(tempo_in, lo, hi));
                assert_eq!(clamped, c["tempoOut"].as_f64().unwrap(), "tempoOut for {key}/{meter} in={tempo_in}");
                assert_eq!(arr.guitar, c["guitar"].as_str().unwrap(), "guitar for {key}");
                assert_eq!(arr.break_lead, c["breakLead"].as_str().unwrap(), "breakLead for {key}");
                assert_eq!(arr.drums, c["band"]["drums"].as_str().unwrap(), "drums for {key}");
                assert_eq!(band_json(&arr.band), {
                    let mut b = c["band"].clone();
                    b.as_object_mut().unwrap().remove("drums");
                    b
                }, "band for {key}");
            }
        }
    }
}

#[test]
fn song_prompt_matches_js_for_every_style_and_voice() {
    let refdata = read_ref();
    let style_keys: Vec<String> =
        refdata["styleKeys"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let year = refdata["fixedYear"].as_i64().unwrap() as i32;

    let voice_cases: [(&str, Option<&str>); 3] =
        [("undefined", None), ("auto", Some("auto")), ("alto", Some("alto"))];

    for key in &style_keys {
        for (label, voice) in voice_cases {
            let mut rand = rand_for(7, &format!("songPrompt|{key}|{label}"));
            let dir = style_direction(Some(key), &mut rand);
            let mood = format!("a song about {key}");
            let got = song_prompt(&mood, voice, Some(dir), year, &mut rand);
            let want = refdata["songPrompts"][key][label].as_str().unwrap();
            assert_eq!(got, want, "songPrompt for style {key} voice {label}");
        }
    }
}

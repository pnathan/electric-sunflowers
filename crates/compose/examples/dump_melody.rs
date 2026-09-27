//! Dumps the lead-vocal melody (midi per note, grouped by section) for a
//! song, so before/after shapeFor renders can be compared note-for-note.
//!
//! Usage:
//!   dump_melody demo --seed 1234 [--voice baritone]
//!   dump_melody <song.json> --seed 1234 [--voice baritone]

use compose::melody::melody_profile;
use compose::prepare::prepare;
use compose::song::normalize_song;
use compose::voices::Voice;
use std::str::FromStr;

fn demo_song_raw() -> serde_json::Value {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let out = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(
            "const {{DEMO_SONG}}=require('{}');console.log(JSON.stringify(DEMO_SONG));",
            format!("{manifest}/../../src/demo.js")
        ))
        .output()
        .expect("node must be available to load src/demo.js");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: dump_melody <demo|song.json> [--seed N] [--voice V]");
        std::process::exit(1);
    }
    let target = &args[1];
    let mut seed: u32 = 1234;
    let mut voice: Option<Voice> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                seed = args[i + 1].parse().unwrap();
                i += 2;
            }
            "--voice" => {
                voice = Some(Voice::from_str(&args[i + 1]).expect("unknown voice"));
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }

    let raw = if target == "demo" {
        demo_song_raw()
    } else {
        let text = std::fs::read_to_string(target).expect("read song json");
        serde_json::from_str(&text).expect("parse song json")
    };
    let song = normalize_song(&raw).expect("normalize song");
    let prof = melody_profile(seed, &song);
    eprintln!(
        "# shape verse={:?} prechorus={:?} chorus={:?} bridge={:?} inst={:?}",
        prof.shape.verse, prof.shape.prechorus, prof.shape.chorus, prof.shape.bridge, prof.shape.inst
    );
    let prepared = prepare(&song, seed, voice);

    for n in &prepared.comp.lead {
        let sec_idx = prepared.form.lines[n.line_idx].sec;
        let sec_type = &prepared.form.sections[sec_idx].type_;
        let sec_occ = prepared.form.sections[sec_idx].occ;
        let li = prepared.form.lines[n.line_idx].li;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            sec_type, sec_occ, li, n.i, n.midi, n.lift as u8
        );
    }
}

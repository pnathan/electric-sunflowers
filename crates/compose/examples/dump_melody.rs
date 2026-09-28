//! Dumps the lead-vocal melody (midi per note, grouped by section) for a
//! song, so before/after shapeFor renders can be compared note-for-note.
//!
//! Usage:
//!   dump_melody demo --seed 1234 [--voice baritone]
//!   dump_melody <song.json> --seed 1234 [--voice baritone]

use compose::melody::melody_profile;
use compose::prepare::prepare;
use song::Voice;
use std::str::FromStr;

fn demo_song_raw() -> serde_json::Value {
    serde_json::from_str(include_str!("../../engine/src/demo.json")).expect("demo.json is JSON")
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
    let (song, repairs) = song::normalize_value(&raw).expect("normalize song");
    for r in &repairs {
        eprintln!("# repair: {r}");
    }
    let prof = melody_profile(seed, &song);
    eprintln!(
        "# shape verse={:?} prechorus={:?} chorus={:?} bridge={:?} inst={:?}",
        prof.shape.verse, prof.shape.prechorus, prof.shape.chorus, prof.shape.bridge, prof.shape.inst
    );
    let prepared = prepare(&song, seed, voice);

    for n in &prepared.comp.lead {
        let sec_idx = prepared.form.lines[n.line_idx].sec;
        let sec_type = prepared.form.sections[sec_idx].kind;
        let sec_occ = prepared.form.sections[sec_idx].occ;
        let li = prepared.form.lines[n.line_idx].li;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            sec_type, sec_occ, li, n.i, n.midi, n.lift as u8
        );
    }
}

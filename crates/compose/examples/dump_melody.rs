//! Dumps the lead melody of a song, one note per line:
//! section kind, occurrence, line, syllable, MIDI, lifted (0/1).
//! Reports the compose time (`prepare`, best of 20 runs) on stderr.
//!
//! Usage:
//!   dump_melody <song.json | demo> [--seed N] [--voice V]
//!
//! `demo` reads crates/engine/src/demo.json.

use std::process::ExitCode;
use std::str::FromStr;
use std::time::Instant;

use compose::melody::melody_profile;
use compose::prepare::prepare;
use song::{SectionKind, Voice};

const DEMO: &str = include_str!("../../engine/src/demo.json");

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = args.first().ok_or("usage: dump_melody <song.json|demo> [--seed N] [--voice V]")?;
    let mut seed: u64 = 1234;
    let mut voice: Option<Voice> = None;
    let mut i = 1;
    while i < args.len() {
        let val = args.get(i + 1).ok_or_else(|| format!("{} needs a value", args[i]))?;
        match args[i].as_str() {
            "--seed" => seed = val.parse().map_err(|e| format!("--seed {val}: {e}"))?,
            "--voice" => voice = Some(Voice::from_str(val).map_err(|e| format!("--voice {val}: {e}"))?),
            other => return Err(format!("unknown option {other}")),
        }
        i += 2;
    }

    let text = if target == "demo" {
        DEMO.to_string()
    } else {
        std::fs::read_to_string(target).map_err(|e| format!("{target}: {e}"))?
    };
    let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{target}: {e}"))?;
    let (song, repairs) = song::normalize_value(&raw).map_err(|e| format!("{target}: {e}"))?;
    for r in &repairs {
        eprintln!("# repair: {r}");
    }
    let prof = melody_profile(seed, &song);
    for k in [SectionKind::Verse, SectionKind::Prechorus, SectionKind::Chorus, SectionKind::Bridge] {
        eprintln!("# shape {k}: {:?}", prof.shape(k));
    }

    let mut best = f64::INFINITY;
    let mut prepared = prepare(&song, seed, voice);
    for _ in 0..20 {
        let t = Instant::now();
        prepared = prepare(&song, seed, voice);
        best = best.min(t.elapsed().as_secs_f64());
    }
    eprintln!(
        "# compose {:.3} ms, {} notes, key shift {}",
        best * 1e3,
        prepared.comp.lead.len(),
        prepared.key_shift
    );

    for n in &prepared.comp.lead {
        let line = &prepared.form.lines[n.line_idx];
        let sec = &prepared.form.sections[line.sec];
        println!("{}\t{}\t{}\t{}\t{}\t{}", sec.kind, sec.occ, line.li, n.i, n.midi, u8::from(n.lift));
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dump_melody: {e}");
            ExitCode::FAILURE
        }
    }
}

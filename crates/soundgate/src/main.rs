//! `soundgate`: command-line front end for the sound gate probes.
//!
//! - `soundgate ltas DIR`: analyse every *.wav in DIR, write DIR/ltas.json;
//!   exit 1 when any file holds NaN/inf samples.
//! - `soundgate compare BASE NEW --tol-mid DB --tol-edge DB`: compare two
//!   ltas.json files; exit 1 on any failure.
//! - `soundgate pitch WAV NOTES`: YIN note check, write WAV's stem + .pitch.json.
//! - `soundgate pitch-compare BASE NEW`: compare two pitch reports; exit 1
//!   on failure.

use clap::{Parser, Subcommand};
use soundgate::compare::{compare_ltas, compare_pitch, table, LtasFile, Tolerances};
use soundgate::ltas::{analyse, NOMINAL_HZ};
use soundgate::pitch::{check, Note, PitchReport};
use soundgate::wav;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "soundgate", about = "Sound gate probes: LTAS, level, YIN pitch")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 1/3-octave LTAS and gated level of every *.wav in DIR; writes DIR/ltas.json.
    Ltas { dir: PathBuf },
    /// Compare two ltas.json files against the gate thresholds.
    Compare {
        base: PathBuf,
        new: PathBuf,
        /// Tolerance for bands 100 Hz-10 kHz, dB.
        #[arg(long, default_value_t = 3.0)]
        tol_mid: f64,
        /// Tolerance for the other bands, dB.
        #[arg(long, default_value_t = 6.0)]
        tol_edge: f64,
    },
    /// YIN pitch check of WAV against NOTES ([{t0,t1,midi}]).
    Pitch { wav: PathBuf, notes: PathBuf },
    /// Compare two pitch reports (fraction within 50 cents, octave errors).
    PitchCompare { base: PathBuf, new: PathBuf },
}

fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> Result<T, String> {
    let s = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", p.display()))
}

fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> Result<(), String> {
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    std::fs::write(p, s + "\n").map_err(|e| format!("{}: {e}", p.display()))
}

fn cmd_ltas(dir: &Path) -> Result<bool, String> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("{}: no .wav files", dir.display()));
    }
    let mut files = BTreeMap::new();
    for p in &paths {
        let a = wav::read(p)?;
        let l = analyse(&a.channels, a.sample_rate);
        let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        println!(
            "{:<16} active {:5.1}%  gated {:7.2} dBFS  peak {:.4}  nonfinite {}",
            stem,
            100.0 * l.active_fraction,
            l.gated_rms_dbfs,
            l.peak,
            l.nonfinite
        );
        files.insert(stem, l);
    }
    let bad: Vec<String> = files.iter().filter(|(_, l)| l.nonfinite > 0).map(|(k, l)| format!("{k} ({})", l.nonfinite)).collect();
    let out = dir.join("ltas.json");
    write_json(&out, &LtasFile { centres_hz: NOMINAL_HZ.to_vec(), files })?;
    println!("wrote {}", out.display());
    if !bad.is_empty() {
        println!("NaN/inf samples: {}", bad.join(", "));
    }
    Ok(bad.is_empty())
}

fn cmd_compare(base: &Path, new: &Path, tol: Tolerances) -> Result<bool, String> {
    if !(tol.mid_db >= 0.0 && tol.edge_db >= 0.0) {
        return Err("tolerances must be non-negative".into());
    }
    let b: LtasFile = read_json(base)?;
    let n: LtasFile = read_json(new)?;
    let rows = compare_ltas(&b, &n, &tol);
    print!("{}", table(&rows));
    Ok(!rows.is_empty() && rows.iter().all(|r| r.fails.is_empty()))
}

fn cmd_pitch(wav_path: &Path, notes_path: &Path) -> Result<bool, String> {
    let a = wav::read(wav_path)?;
    let notes: Vec<Note> = read_json(notes_path)?;
    // mono: mean of channels
    let n = a.frames();
    let k = a.channels.len() as f64;
    let x: Vec<f64> = (0..n).map(|i| a.channels.iter().map(|c| c[i]).sum::<f64>() / k).collect();
    let r = check(&x, a.sample_rate, &notes);
    let stem = wav_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let out = wav_path.with_file_name(format!("{stem}.pitch.json"));
    write_json(&out, &r)?;
    println!(
        "pitch {}: {}/{} notes within 50 cents ({:.3}), octave errors {}, median |cents| {}",
        stem,
        r.within_50c,
        r.notes_analysed,
        r.fraction_within_50c,
        r.octave_errors,
        r.median_abs_cents.map_or("-".into(), |c| format!("{c:.1}"))
    );
    println!("wrote {}", out.display());
    Ok(true)
}

fn cmd_pitch_compare(base: &Path, new: &Path) -> Result<bool, String> {
    let b: PitchReport = read_json(base)?;
    let n: PitchReport = read_json(new)?;
    let f = compare_pitch(&b, &n);
    println!(
        "pitch fraction {:.3} (base {:.3})  octave errors {} (base {})  {}",
        n.fraction_within_50c,
        b.fraction_within_50c,
        n.octave_errors,
        b.octave_errors,
        if f.is_empty() { "PASS".to_string() } else { format!("FAIL {}", f.join(", ")) }
    );
    Ok(f.is_empty())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let r = match cli.cmd {
        Cmd::Ltas { dir } => cmd_ltas(&dir),
        Cmd::Compare { base, new, tol_mid, tol_edge } => {
            cmd_compare(&base, &new, Tolerances { mid_db: tol_mid, edge_db: tol_edge })
        }
        Cmd::Pitch { wav, notes } => cmd_pitch(&wav, &notes),
        Cmd::PitchCompare { base, new } => cmd_pitch_compare(&base, &new),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("soundgate: {e}");
            ExitCode::from(2)
        }
    }
}

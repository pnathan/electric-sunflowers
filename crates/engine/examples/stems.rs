//! Sound gate stems: renders the demo song and writes each processed track
//! (after the channel strip: EQ, level to 0.1 active RMS, lead and harmony
//! compression; before pan and sends) as 32-bit float WAV, then the full
//! mix. Also writes notes.json (lead vocal notes: t0, t1, midi after
//! transposition) and render.json (wall time per stage).
//!
//! Usage: cargo run --release -p engine --features capture_raw --example stems -- --seed S --out DIR
//!
//! The channel strip repeats `dsp::mix::compute_track` from its public parts
//! (`eq_for`, `bq`/`run_bq`, `active_rms`, `compress`), since the processed
//! tracks are private to `dsp::mix::Render`.

#[cfg(not(feature = "capture_raw"))]
fn main() {
    eprintln!(
        "stems: needs the engine feature capture_raw (raw tracks are only kept with it).\n\
         run: cargo run --release -p engine --features capture_raw --example stems -- --seed S --out DIR"
    );
    std::process::exit(2);
}

#[cfg(feature = "capture_raw")]
fn main() {
    if let Err(e) = run() {
        eprintln!("stems: {e}");
        std::process::exit(1);
    }
}

/// Writes interleaved 32-bit IEEE float WAV (format tag 3) by hand.
#[cfg(feature = "capture_raw")]
fn write_f32_wav(path: &std::path::Path, chs: &[&[f32]], sr: u32) -> std::io::Result<()> {
    use std::io::Write;
    let nch = chs.len() as u16;
    let frames = chs.first().map_or(0, |c| c.len());
    let data_bytes = (frames * chs.len() * 4) as u32;
    let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data_bytes).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?;
    w.write_all(&nch.to_le_bytes())?;
    w.write_all(&sr.to_le_bytes())?;
    w.write_all(&(sr * nch as u32 * 4).to_le_bytes())?;
    w.write_all(&(nch * 4).to_le_bytes())?;
    w.write_all(&32u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;
    for i in 0..frames {
        for c in chs {
            w.write_all(&c[i].to_le_bytes())?;
        }
    }
    w.flush()
}

/// The strip of `dsp::mix::compute_track`, without the lead slapback.
/// Returns `None` for a silent track.
#[cfg(feature = "capture_raw")]
fn strip(key: &str, mut chs: Vec<Vec<f32>>) -> Option<Vec<Vec<f32>>> {
    use dsp::dynamics::{compress, db_of};
    use dsp::filter::{bq, run_bq};
    use dsp::mix::{active_rms, eq_for};
    use sfcore::js;

    for &(ty, f, q, g) in eq_for(key) {
        let co = bq(ty, f, q, g);
        for c in chs.iter_mut() {
            run_bq(c, &co);
        }
    }
    let mut rms = 0.0f64;
    for c in &chs {
        rms = js::max(rms, active_rms(c));
    }
    if rms < 1e-6 {
        return None;
    }
    let g0 = 0.1 / rms;
    for c in chs.iter_mut() {
        for v in c.iter_mut() {
            *v = js::f32r(*v as f64 * g0) as f32;
        }
    }
    if key == "lead" || key == "harmony" {
        for c in chs.iter_mut() {
            compress(c, db_of(0.1) + 1.0, 3.0, 0.008, 0.15, Some(6.0));
        }
    }
    Some(chs)
}

#[cfg(feature = "capture_raw")]
fn file_name(key: &str) -> &str {
    match key {
        "hg" => "harmony_guitar",
        k => k,
    }
}

#[cfg(feature = "capture_raw")]
fn run() -> Result<(), String> {
    use sfcore::tuning::Tuning;
    use std::path::PathBuf;
    use std::time::Instant;

    let mut seed: u32 = 1234;
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => {
                let v = args.next().ok_or("--seed needs a value")?;
                seed = v.parse().map_err(|_| format!("bad seed {v}"))?;
            }
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a value")?)),
            _ => return Err(format!("unknown argument {a}; usage: stems --seed S --out DIR")),
        }
    }
    let out = out.ok_or("--out DIR is required")?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let sr = sfcore::SR as u32;

    let (song, _) = song::normalize_value(&engine::demo_song()).map_err(|e| format!("demo song: {e}"))?;
    let t = Instant::now();
    let mut rendered = engine::render_song_threaded(&song, seed, None, &Tuning::default());
    let render_s = t.elapsed().as_secs_f64();

    let notes: Vec<serde_json::Value> = rendered
        .prepared
        .comp
        .lead
        .iter()
        .map(|n| serde_json::json!({ "t0": n.t0, "t1": n.t1, "midi": n.midi }))
        .collect();
    let p = out.join("notes.json");
    std::fs::write(&p, serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| format!("{}: {e}", p.display()))?;

    let t = Instant::now();
    let raw = std::mem::take(&mut rendered.raw_tracks);
    let mut written = Vec::new();
    for (key, chs) in raw {
        let Some(chs) = strip(key, chs) else { continue };
        let refs: Vec<&[f32]> = chs.iter().map(|c| c.as_slice()).collect();
        let p = out.join(format!("{}.wav", file_name(key)));
        write_f32_wav(&p, &refs, sr).map_err(|e| format!("{}: {e}", p.display()))?;
        written.push(file_name(key).to_string());
    }
    let strip_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let (l, r) = engine::mix_threaded(&mut rendered, |_| true, seed);
    let mix_s = t.elapsed().as_secs_f64();
    let p = out.join("mix.wav");
    write_f32_wav(&p, &[&l, &r], sr).map_err(|e| format!("{}: {e}", p.display()))?;

    let info = serde_json::json!({
        "seed": seed,
        "samples": rendered.len,
        "render_s": render_s,
        "strip_s": strip_s,
        "mix_s": mix_s,
        "stems": written,
    });
    let p = out.join("render.json");
    std::fs::write(&p, serde_json::to_string_pretty(&info).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| format!("{}: {e}", p.display()))?;
    eprintln!(
        "stems: seed {seed}: render {render_s:.2} s, strip {strip_s:.2} s, mix {mix_s:.2} s, {} stems + mix in {}",
        written.len(),
        out.display()
    );
    Ok(())
}

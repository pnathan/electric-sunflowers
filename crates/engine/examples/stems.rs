//! Sound gate stems: renders a song and writes each processed stem (after
//! the channel strip, level applied, before pan and sends) as 32-bit float
//! WAV named by track, then the full mix as mix.wav. Also writes notes.json
//! (lead vocal notes: t0, t1, midi after transposition) and render.json
//! (wall time per stage). `--song duet` additionally writes lead_b.wav and
//! notes_b.json for singer B.
//!
//! Usage: cargo run --release -p engine --example stems -- --seed S --out DIR [--song demo|duet]

use std::path::{Path, PathBuf};
use std::time::Instant;

use compose::prepare::VoiceChoice;
use engine::{demo_duet_song, demo_song, mix, render, render_with, NoProgress, Stem, TrackId};

fn main() {
    if let Err(e) = run() {
        eprintln!("stems: {e}");
        std::process::exit(1);
    }
}

/// Writes interleaved 32-bit IEEE float WAV (format tag 3).
fn write_f32_wav(path: &Path, chs: &[&[f32]], sr: u32) -> std::io::Result<()> {
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

fn run() -> Result<(), String> {
    let mut seed: u64 = 1234;
    let mut out: Option<PathBuf> = None;
    let mut duet = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => {
                let v = args.next().ok_or("--seed needs a value")?;
                seed = v.parse().map_err(|_| format!("bad seed {v}"))?;
            }
            "--out" => out = Some(PathBuf::from(args.next().ok_or("--out needs a value")?)),
            "--song" => {
                let v = args.next().ok_or("--song needs a value")?;
                duet = match v.as_str() {
                    "demo" => false,
                    "duet" => true,
                    _ => return Err(format!("bad --song {v}; expected demo or duet")),
                };
            }
            _ => {
                return Err(format!(
                    "unknown argument {a}; usage: stems --seed S --out DIR [--song demo|duet]"
                ))
            }
        }
    }
    let out = out.ok_or("--out DIR is required")?;
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let sr = sfcore::SR as u32;
    sfcore::fp::init_pool(None);

    let t = Instant::now();
    // The solo path's notes.json stays sourced from `prepared.comp.lead`,
    // not `arrangement.vocals.lead.notes`: checked directly (run both,
    // diff), the two differ, since `ComposedNote::midi` is an integer and
    // `song::events::VocalNote::midi` is `f32` (49 vs 49.0 once
    // serialised), so switching the demo song's source would move the
    // gate's committed baseline files by a byte for no reason. The
    // arrangement-based source is used only for `--song duet`, where it is
    // also the only source that gives singer B's notes (`comp.lead` holds
    // both singers' notes together, undistinguished, and misses a shared
    // line's `Comp.second` notes for whichever singer carries the
    // harmony).
    let (song, prepared, stems) = if duet {
        let song = demo_duet_song();
        let (prepared, stems) = render_with(song, seed, VoiceChoice::default(), &NoProgress);
        (song, prepared, stems)
    } else {
        let song = demo_song();
        let (prepared, stems) = render(song, seed, None, &NoProgress);
        (song, prepared, stems)
    };
    let render_s = t.elapsed().as_secs_f64();

    if duet {
        let arrangement = arrange::arrange(song, &prepared, seed);
        let notes_of = |notes: &[song::events::VocalNote]| -> Vec<serde_json::Value> {
            notes
                .iter()
                .map(|n| serde_json::json!({ "t0": n.t0, "t1": n.t1, "midi": n.midi }))
                .collect()
        };
        let notes = notes_of(&arrangement.vocals.lead.notes);
        let p = out.join("notes.json");
        std::fs::write(
            &p,
            serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())? + "\n",
        )
        .map_err(|e| format!("{}: {e}", p.display()))?;
        if let Some(b) = arrangement.vocals.lead_b.as_ref() {
            let notes_b = notes_of(&b.notes);
            let p = out.join("notes_b.json");
            std::fs::write(
                &p,
                serde_json::to_string_pretty(&notes_b).map_err(|e| e.to_string())? + "\n",
            )
            .map_err(|e| format!("{}: {e}", p.display()))?;
        }
    } else {
        let notes: Vec<serde_json::Value> = prepared
            .comp
            .lead
            .iter()
            .map(|n| serde_json::json!({ "t0": n.t0, "t1": n.t1, "midi": n.midi }))
            .collect();
        let p = out.join("notes.json");
        std::fs::write(
            &p,
            serde_json::to_string_pretty(&notes).map_err(|e| e.to_string())? + "\n",
        )
        .map_err(|e| format!("{}: {e}", p.display()))?;
    }

    let t = Instant::now();
    let mut written = Vec::new();
    let mut blocks = serde_json::Map::new();
    for id in TrackId::ALL {
        let Some(ps) = stems.get(id) else { continue };
        let chs: Vec<Vec<f32>> = ps
            .audio
            .channels()
            .iter()
            .map(|c| c.to_dense().into_iter().map(|v| v * ps.level).collect())
            .collect();
        let refs: Vec<&[f32]> = chs.iter().map(|c| c.as_slice()).collect();
        debug_assert_eq!(
            refs.len(),
            if matches!(ps.audio, Stem::Mono(_)) {
                1
            } else {
                2
            }
        );
        let p = out.join(format!("{}.wav", id.name()));
        write_f32_wav(&p, &refs, sr).map_err(|e| format!("{}: {e}", p.display()))?;
        written.push(id.name());
        let present: usize = ps.audio.channels().iter().map(|c| c.present_blocks()).sum();
        let total: usize = ps.audio.channels().iter().map(|c| c.block_count()).sum();
        blocks.insert(id.name().into(), serde_json::json!([present, total]));
    }
    let write_s = t.elapsed().as_secs_f64();

    let t = Instant::now();
    let m = mix(&stems, &song.band, seed);
    let mix_s = t.elapsed().as_secs_f64();
    let p = out.join("mix.wav");
    write_f32_wav(&p, &[&m.l, &m.r], sr).map_err(|e| format!("{}: {e}", p.display()))?;

    let info = serde_json::json!({
        "seed": seed,
        "samples": stems.len,
        "render_s": render_s,
        "stem_write_s": write_s,
        "mix_s": mix_s,
        "stems": written,
        "blocks_present_total": blocks,
        "slapback_blocks": stems.get(TrackId::Lead).and_then(|p| p.slap.as_ref()).map(|s| s.present_blocks()),
    });
    let p = out.join("render.json");
    std::fs::write(
        &p,
        serde_json::to_string_pretty(&info).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| format!("{}: {e}", p.display()))?;
    eprintln!(
        "stems: seed {seed}: render {render_s:.2} s, mix {mix_s:.2} s, {} stems + mix in {}",
        written.len(),
        out.display()
    );
    Ok(())
}

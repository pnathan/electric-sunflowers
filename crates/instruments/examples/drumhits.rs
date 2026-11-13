//! Writes the drum voices as mono 32-bit float WAV files for measurement
//! (tools/drum_target.py `eng`): for each kind and each velocity in
//! {0.25, 0.6, 0.95}, 8 consecutive hits of that kind (1.5 s apart, the tom and
//! ride 3 s), rendered by `render_drums`, the code path of a song render.
//!
//! Usage: `drumhits DIR [--seed N] [--time]`. `--time` also prints, per kind, the
//! time to build its pools (all three layers, in a fresh scratch). Files are `<kind>_<v>.wav`; the tom is
//! `tom85` and `tom110`.

use instruments::drums::{render_drums, render_hit, DrumScratch, DRUM_HIT};
use sfcore::random::Rng;
use sfcore::SR;
use song::events::{DrumHit, DrumKind};
use std::io::Write;

const VELS: [f32; 3] = [0.25, 0.6, 0.95];
const HITS: usize = 8;

fn wav(path: &std::path::Path, x: &[f32]) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let bytes = (x.len() * 4) as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + bytes).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&(SR as u32).to_le_bytes())?;
    f.write_all(&((SR * 4) as u32).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&bytes.to_le_bytes())?;
    for v in x {
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: drumhits DIR [--seed N]");
    let mut seed = 1234u64;
    let mut time = false;
    while let Some(a) = args.next() {
        if a == "--seed" {
            seed = args.next().and_then(|s| s.parse().ok()).expect("--seed N");
        } else if a == "--time" {
            time = true;
        }
    }
    std::fs::create_dir_all(&dir)?;
    let kinds: [(&str, DrumKind, f64); 10] = [
        ("kick", DrumKind::Kick, 1.5),
        ("snare", DrumKind::Snare, 1.5),
        ("rim", DrumKind::Rim, 1.5),
        ("tap", DrumKind::Tap, 1.5),
        ("swish", DrumKind::Swish { dur: 0.5 }, 1.5),
        ("hat", DrumKind::Hat, 1.5),
        ("shaker", DrumKind::Shaker, 1.5),
        ("tom85", DrumKind::Tom { hz: 85.0 }, 3.0),
        ("tom110", DrumKind::Tom { hz: 110.0 }, 3.0),
        ("ride", DrumKind::Ride, 3.0),
    ];
    let mut total = 0.0;
    if time {
        for (name, kind, _) in kinds {
            println!(
                "hit length {name}: {:.2} s",
                instruments::drums::hit_len(kind) as f64 / SR as f64
            );
        }
    }
    for (name, kind, gap) in kinds {
        if time {
            let mut sc = DrumScratch::new(seed);
            let (mut l, mut r) = (vec![0.0f32; 1000], vec![0.0f32; 1000]);
            for v in VELS {
                let hit = DrumHit {
                    t: 0.0,
                    kind,
                    vel: v,
                    pan: 0.0,
                };
                render_hit(
                    &hit,
                    &mut Rng::event(seed, DRUM_HIT, 0),
                    &mut l,
                    &mut r,
                    &mut sc,
                );
            }
            println!("pool build {name}: {:.3} s", sc.build_secs());
            total += sc.build_secs();
        }
        for v in VELS {
            let hits: Vec<DrumHit> = (0..HITS)
                .map(|k| DrumHit {
                    t: k as f64 * gap,
                    kind,
                    vel: v,
                    pan: -1.0,
                })
                .collect();
            let len = (HITS as f64 * gap * SR as f64) as usize;
            let [l, _] = render_drums(&hits, seed, len);
            wav(
                &std::path::Path::new(&dir).join(format!("{name}_{v}.wav")),
                &l,
            )?;
        }
    }
    if time {
        println!("pool build total: {total:.3} s");
    }
    Ok(())
}

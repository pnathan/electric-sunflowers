//! Renders single harp notes through the engine's harp chain (pluck model,
//! then the harp body), for comparison with recordings by
//! `tools/harp_compare.py`. Writes one 32-bit float stereo WAV per note and
//! seed: `<out>/harp_<midi>_s<seed>.wav`.
//!
//! Run: bazel run -c opt //crates/engine:harpnotes -- OUT_DIR [SEEDS]

use engine::band::apply_body;
use engine::TrackId;
use instruments::guitar::render_plucks;
use instruments::pluck::PluckParams;
use sfcore::random::tag;
use sfcore::SR;
use song::events::PluckNote;
use std::io::Write;

/// The recorded notes (VSCO 2 CE, `tools/fetch_harp.sh`) inside the range the
/// harp plays (MIDI 55-88, `arrange::harp`), one octave-and-a-bit wider at the
/// top so the highest partials are covered.
const MIDIS: [u8; 11] = [55, 59, 62, 65, 69, 72, 76, 79, 83, 86, 89];
const VEL: f32 = 0.6;
const SECS: f64 = 7.0;

fn write_wav(path: &std::path::Path, l: &[f32], r: &[f32]) -> std::io::Result<()> {
    let n = l.len() as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let data = n * 8;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?; // IEEE float
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&(SR as u32).to_le_bytes())?;
    f.write_all(&(SR as u32 * 8).to_le_bytes())?;
    f.write_all(&8u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data.to_le_bytes())?;
    for i in 0..l.len() {
        f.write_all(&l[i].to_le_bytes())?;
        f.write_all(&r[i].to_le_bytes())?;
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let out = std::path::PathBuf::from(args.next().expect("usage: harpnotes OUT_DIR [SEEDS]"));
    let seeds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);
    std::fs::create_dir_all(&out)?;
    let mount = TrackId::Harp
        .strip()
        .body
        .expect("the harp strip has a body");
    let len = (SECS * SR as f64) as usize + SR;
    for seed in 1..=seeds {
        for &m in &MIDIS {
            let note = PluckNote {
                t0: 0.1,
                t1: 0.1,
                midi: m as f32,
                vel: VEL,
            };
            let mono = render_plucks(&[note], &PluckParams::HARP, seed, tag("harp.note"), len);
            let [l, r] = apply_body(mount, seed, len, &mono);
            let p = out.join(format!("harp_{m}_s{seed}.wav"));
            write_wav(&p, &l.to_dense(), &r.to_dense())?;
        }
    }
    Ok(())
}

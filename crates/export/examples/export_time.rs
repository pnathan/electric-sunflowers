//! Export timing: 186 s of stereo noise (-6 dBFS peak) written as WAV 16,
//! WAV float, FLAC 24, FLAC 16 and Ogg 0.6. Prints wall time and file size.
//! With a stereo 32-bit float WAV argument, times that signal instead.
//! Run: cargo run --release -p export --example export_time [-- input.wav]

use export::{BitDepth, Format, Meta, WavSample};
use sfcore::random::{tag, Rng};
use std::time::Instant;

fn main() {
    let dir = std::env::temp_dir();
    let (l, r) = match std::env::args().nth(1) {
        Some(p) => match read_float_wav(&p) {
            Some(x) => x,
            None => {
                eprintln!("{p}: not a stereo 32-bit float WAV");
                std::process::exit(1);
            }
        },
        None => noise(186 * 44100),
    };
    println!("{:.1} s stereo", l.len() as f64 / 44100.0);
    let meta = Meta { title: "timing".into(), ..Meta::default() };
    let cases = [
        ("wav16.wav", Format::Wav { sample: WavSample::Pcm16 }),
        ("wavf.wav", Format::Wav { sample: WavSample::Float32 }),
        ("flac24.flac", Format::Flac { bits: BitDepth::Bits24 }),
        ("flac16.flac", Format::Flac { bits: BitDepth::Bits16 }),
        ("ogg.ogg", Format::Ogg { quality: 0.6 }),
    ];
    for (name, fmt) in cases {
        let path = dir.join(format!("export_time_{name}"));
        let t = Instant::now();
        if let Err(e) = export::write(&path, &l, &r, 44100, &meta, fmt) {
            eprintln!("{name}: {e}");
            continue;
        }
        let dt = t.elapsed().as_secs_f64();
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        println!("{name:12} {dt:7.3} s  {:8.1} MB", size as f64 / 1e6);
        std::fs::remove_file(&path).ok();
    }
}

fn noise(n: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    Rng::stream(1, tag("export.time.l")).fill_bipolar(&mut l);
    Rng::stream(1, tag("export.time.r")).fill_bipolar(&mut r);
    for x in l.iter_mut().chain(r.iter_mut()) {
        *x *= 0.5;
    }
    (l, r)
}

/// Minimal reader: the `data` chunk of a stereo float WAV.
fn read_float_wav(path: &str) -> Option<(Vec<f32>, Vec<f32>)> {
    let b = std::fs::read(path).ok()?;
    let mut i = 12;
    while i + 8 <= b.len() {
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let body = b.get(i + 8..i + 8 + len)?;
        if &b[i..i + 4] == b"fmt " && (u16::from_le_bytes([body[0], body[1]]) != 3 || body[2] != 2) {
            return None;
        }
        if &b[i..i + 4] == b"data" {
            let x: Vec<f32> = body.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            let l = x.iter().step_by(2).copied().collect();
            let r = x.iter().skip(1).step_by(2).copied().collect();
            return Some((l, r));
        }
        i += 8 + len + len % 2;
    }
    None
}

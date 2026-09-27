//! Minimal WAV writer (PCM16 or float32, stereo, 44100 Hz). No external
//! dependency: hound is not in the offline registry index this repo builds
//! against, and the format is a 44-byte header plus interleaved samples.
//!
//! PCM16 scaling matches tests/mt.js exactly: `Math.round(clamp(x,-1,1)*32767)`
//! per sample, via sfcore::js::round (JS Math.round: halves round toward
//! +infinity, not Rust's round-half-away-from-zero).

use sfcore::js;
use std::io::{self, Write};

pub fn write_wav(path: &std::path::Path, l: &[f32], r: &[f32], sr: u32, float: bool) -> io::Result<()> {
    assert_eq!(l.len(), r.len(), "L/R channel length mismatch");
    let n = l.len();
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    let (bits, fmt_tag, bytes_per_sample) = if float { (32u16, 3u16, 4usize) } else { (16u16, 1u16, 2usize) };
    let block_align = bytes_per_sample as u32 * 2;
    let byte_rate = sr * block_align;
    let data_bytes = (n * bytes_per_sample * 2) as u32;

    f.write_all(b"RIFF")?;
    write_u32(&mut f, 36 + data_bytes)?;
    f.write_all(b"WAVE")?;
    f.write_all(b"fmt ")?;
    write_u32(&mut f, 16)?;
    write_u16(&mut f, fmt_tag)?;
    write_u16(&mut f, 2)?; // stereo
    write_u32(&mut f, sr)?;
    write_u32(&mut f, byte_rate)?;
    write_u16(&mut f, block_align as u16)?;
    write_u16(&mut f, bits)?;
    f.write_all(b"data")?;
    write_u32(&mut f, data_bytes)?;

    if float {
        for i in 0..n {
            write_f32(&mut f, l[i])?;
            write_f32(&mut f, r[i])?;
        }
    } else {
        for i in 0..n {
            write_i16(&mut f, pcm16(l[i]))?;
            write_i16(&mut f, pcm16(r[i]))?;
        }
    }
    f.flush()
}

fn pcm16(x: f32) -> i16 {
    let clamped = (x as f64).max(-1.0).min(1.0);
    js::round(clamped * 32767.0) as i16
}

fn write_u32<W: Write>(w: &mut W, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn write_u16<W: Write>(w: &mut W, v: u16) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn write_i16<W: Write>(w: &mut W, v: i16) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn write_f32<W: Write>(w: &mut W, v: f32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_rounds_js_style() {
        // Math.round(0.5) == 1 in JS (round half toward +infinity).
        assert_eq!(js::round(0.5), 1.0);
        assert_eq!(js::round(-0.5), 0.0);
        assert_eq!(pcm16(1.5), 32767); // clamp first
        assert_eq!(pcm16(-1.5), -32767);
    }
}

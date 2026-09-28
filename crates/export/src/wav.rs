//! Direct RIFF/WAVE writer: stereo 16-bit PCM, 24-bit PCM or 32-bit float.
//!
//! Layout (Microsoft/IBM "Multimedia Programming Interface and Data
//! Specifications 1.0", 1991): `RIFF` size `WAVE`, `fmt ` (16 bytes for PCM,
//! 18 with cbSize = 0 for IEEE float, format tag 3), `fact` (float only, as
//! the spec requires for non-PCM data), `LIST`/`INFO` (INAM, IART, ICMT,
//! ICRD, IGNR; NUL-terminated, word-aligned), then `data`. Every size is
//! known before the first byte, so the file is written once, in order.
//! 24-bit PCM uses the plain PCM format tag 1 with a block align of 6, which
//! every common reader accepts.

use std::io::Write;
use std::path::Path;

use crate::dither::{quantize_interleaved, Tpdf, DITHER_BLOCK};
use crate::{BitDepth, ExportError, Meta, WavSample};

const CHANNELS: u16 = 2;

impl WavSample {
    fn bytes_per_sample(self) -> usize {
        match self {
            WavSample::Pcm16 => 2,
            WavSample::Pcm24 => 3,
            WavSample::Float32 => 4,
        }
    }
    fn format_tag(self) -> u16 {
        match self {
            WavSample::Float32 => 3,
            _ => 1,
        }
    }
}

/// The `LIST` chunk body: `INFO` plus one sub-chunk per non-empty tag. The
/// artist always defaults, so the chunk is never empty.
fn list_info(meta: &Meta) -> Vec<u8> {
    let mut info = Vec::new();
    info.extend_from_slice(b"INFO");
    for (id, value) in [
        (b"INAM", meta.title.as_str()),
        (b"IART", meta.artist_or_default()),
        (b"ICMT", meta.comment.as_str()),
        (b"ICRD", meta.date.as_str()),
        (b"IGNR", meta.style.as_str()),
    ] {
        if value.is_empty() {
            continue;
        }
        let len = value.len() + 1; // NUL terminator
        info.extend_from_slice(id);
        info.extend_from_slice(&(len as u32).to_le_bytes());
        info.extend_from_slice(value.as_bytes());
        info.push(0);
        if len % 2 == 1 {
            info.push(0);
        }
    }
    info
}

/// Builds every byte before the samples: RIFF header, fmt, fact, LIST, data
/// header. Fails if the RIFF size does not fit in 32 bits.
fn header(frames: usize, sr: u32, meta: &Meta, sample: WavSample) -> Result<Vec<u8>, ExportError> {
    let bps = sample.bytes_per_sample();
    let block_align = CHANNELS as usize * bps;
    let data_len = frames as u64 * block_align as u64;
    let float = sample == WavSample::Float32;
    let fmt_len: u32 = if float { 18 } else { 16 };
    let info = list_info(meta);

    let riff_len = 4
        + 8 + fmt_len as u64
        + if float { 12 } else { 0 }
        + 8 + info.len() as u64
        + 8 + data_len;
    if riff_len > u32::MAX as u64 {
        return Err(ExportError::TooLarge { bytes: riff_len + 8 });
    }

    let mut h = Vec::with_capacity(64 + info.len());
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(riff_len as u32).to_le_bytes());
    h.extend_from_slice(b"WAVE");

    h.extend_from_slice(b"fmt ");
    h.extend_from_slice(&fmt_len.to_le_bytes());
    h.extend_from_slice(&sample.format_tag().to_le_bytes());
    h.extend_from_slice(&CHANNELS.to_le_bytes());
    h.extend_from_slice(&sr.to_le_bytes());
    let byte_rate = (sr as u64 * block_align as u64).min(u32::MAX as u64) as u32;
    h.extend_from_slice(&byte_rate.to_le_bytes());
    h.extend_from_slice(&(block_align as u16).to_le_bytes());
    h.extend_from_slice(&((bps * 8) as u16).to_le_bytes());
    if float {
        h.extend_from_slice(&0u16.to_le_bytes()); // cbSize
        h.extend_from_slice(b"fact");
        h.extend_from_slice(&4u32.to_le_bytes());
        h.extend_from_slice(&(frames as u32).to_le_bytes());
    }

    h.extend_from_slice(b"LIST");
    h.extend_from_slice(&(info.len() as u32).to_le_bytes());
    h.extend_from_slice(&info);

    h.extend_from_slice(b"data");
    h.extend_from_slice(&(data_len as u32).to_le_bytes());
    Ok(h)
}

/// Writes `l`, `r` (equal lengths, checked by the caller) to `path`.
pub(crate) fn write(
    path: &Path,
    l: &[f32],
    r: &[f32],
    sr: u32,
    meta: &Meta,
    sample: WavSample,
) -> Result<(), ExportError> {
    let head = header(l.len(), sr, meta, sample)?;
    let mut out = std::io::BufWriter::with_capacity(1 << 16, std::fs::File::create(path)?);
    out.write_all(&head)?;

    let bps = sample.bytes_per_sample();
    let mut bytes = vec![0u8; DITHER_BLOCK * 2 * bps];
    let mut ints = vec![0i32; DITHER_BLOCK * 2];
    let mut dither = Tpdf::at(0);
    for (lc, rc) in l.chunks(DITHER_BLOCK).zip(r.chunks(DITHER_BLOCK)) {
        let n = lc.len() * 2;
        match sample {
            WavSample::Float32 => {
                for ((&a, &b), o) in lc.iter().zip(rc).zip(bytes.as_chunks_mut::<8>().0) {
                    o[..4].copy_from_slice(&a.to_le_bytes());
                    o[4..].copy_from_slice(&b.to_le_bytes());
                }
            }
            WavSample::Pcm16 => {
                quantize_interleaved(lc, rc, BitDepth::Bits16, &mut dither, &mut ints);
                for (&v, o) in ints[..n].iter().zip(bytes.as_chunks_mut::<2>().0) {
                    o.copy_from_slice(&(v as i16).to_le_bytes());
                }
            }
            WavSample::Pcm24 => {
                quantize_interleaved(lc, rc, BitDepth::Bits24, &mut dither, &mut ints);
                for (&v, o) in ints[..n].iter().zip(bytes.as_chunks_mut::<3>().0) {
                    o.copy_from_slice(&v.to_le_bytes()[..3]);
                }
            }
        }
        out.write_all(&bytes[..n * bps])?;
    }
    // Stereo frames are an even number of bytes, so `data` needs no pad byte.
    out.flush()?;
    Ok(())
}

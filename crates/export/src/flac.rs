//! FLAC writer: frames encoded in parallel by flacenc, stream assembled here.
//!
//! Format: RFC 9639 (Free Lossless Audio Codec). A fixed-blocksize stream:
//! `fLaC`, a STREAMINFO block and a VORBIS_COMMENT block (both built here),
//! then frames. flacenc (`encode_fixed_size_frame`, LPC with stereo
//! decorrelation) encodes each frame; nothing else of its stream type is used.
//!
//! flacenc 0.4 misjudges subframe sizes at 24 bits: its rice parameter is
//! capped at 14 (no escape codes), so a residual near 2^20 costs about 64
//! unary bits per sample, and its fixed-predictor path can return subframes
//! larger than verbatim. Measured on the demo mix at 24 bits: 176 of
//! 2003 frames larger than verbatim with fixed predictors on, 3 with them
//! off; white noise at -6 dBFS gave 277 KB frames against 24.6 KB verbatim.
//! So 24-bit encoding turns fixed predictors off, and any frame larger than
//! its verbatim size is encoded again with verbatim subframes only.
//!
//! Parallelism and memory: frames are encoded in batches of `BATCH` with
//! rayon, each task quantising its own frame into a per-thread buffer, then
//! written in order. Only one batch of compressed frames is held at a time;
//! no whole-song integer buffer and no `Frame` list exist. STREAMINFO needs
//! the frame-size range, known only at the end, so its 34 bytes are
//! rewritten in place after the last frame.
//!
//! Block size: 4096 frames, or the largest size below it that leaves a final
//! block of 0 or at least 64 samples (flacenc's minimum). A fixed-blocksize
//! stream allows only the last block to be shorter.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use flacenc::component::{BitRepr, StreamInfo};
use flacenc::error::Verify;
use flacenc::source::{Fill, FrameBuf};
use rayon::prelude::*;

use crate::dither::{quantize_interleaved, Tpdf};
use crate::{BitDepth, ExportError, Meta};

const CHANNELS: usize = 2;
const BLOCK: usize = 4096;
/// flacenc's `MIN_BLOCK_SIZE`.
const MIN_BLOCK: usize = flacenc::constant::MIN_BLOCK_SIZE;
/// Frames encoded per parallel batch (about 24 s at 44.1 kHz).
const BATCH: usize = 256;
/// STREAMINFO sample-rate field width: 20 bits.
const MAX_RATE: u32 = (1 << 20) - 1;

fn flac_err(e: impl std::fmt::Debug) -> ExportError {
    ExportError::Flac(format!("{e:?}"))
}

/// Block size for `n` samples per channel (see the module note).
fn block_size(n: usize) -> Result<usize, ExportError> {
    if n == 0 {
        return Ok(BLOCK);
    }
    if n < MIN_BLOCK {
        return Err(ExportError::Flac(format!(
            "FLAC export needs at least {MIN_BLOCK} samples, got {n}"
        )));
    }
    if n <= BLOCK {
        return Ok(n);
    }
    // n > BLOCK >= b, so a size with an acceptable tail is found within a
    // few steps (the tail grows by the frame count per step).
    (MIN_BLOCK..=BLOCK)
        .rev()
        .find(|b| n.is_multiple_of(*b) || n % b >= MIN_BLOCK)
        .ok_or_else(|| ExportError::Flac("no valid block size".into()))
}

/// STREAMINFO body (RFC 9639 section 8.2), 34 bytes; MD5 left zero ("not
/// computed").
fn streaminfo(
    block: usize,
    min_frame: usize,
    max_frame: usize,
    sr: u32,
    bits: u32,
    total: u64,
) -> [u8; 34] {
    let mut b = [0u8; 34];
    b[0..2].copy_from_slice(&(block as u16).to_be_bytes());
    b[2..4].copy_from_slice(&(block as u16).to_be_bytes());
    b[4..7].copy_from_slice(&(min_frame as u32).to_be_bytes()[1..]);
    b[7..10].copy_from_slice(&(max_frame as u32).to_be_bytes()[1..]);
    let packed = (sr as u64) << 44
        | ((CHANNELS as u64 - 1) << 41)
        | ((bits as u64 - 1) << 36)
        | (total & ((1 << 36) - 1));
    b[10..18].copy_from_slice(&packed.to_be_bytes());
    b
}

/// Metadata block header: last flag, 7-bit type, 24-bit length.
fn block_header(last: bool, kind: u8, len: usize) -> [u8; 4] {
    let l = (len as u32).to_be_bytes();
    [kind | if last { 0x80 } else { 0 }, l[1], l[2], l[3]]
}

/// VORBIS_COMMENT body (RFC 9639 section 8.6; the Ogg Vorbis comment
/// layout without framing bit): vendor string, count, then "TAG=value"
/// entries, each u32-LE length-prefixed. flacenc has no typed block for it.
fn vorbis_comment(meta: &Meta) -> Vec<u8> {
    let tags = meta.tags();
    let vendor = b"electric-sunflowers export";
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for (tag, value) in &tags {
        let entry = format!("{tag}={value}");
        out.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        out.extend_from_slice(entry.as_bytes());
    }
    out
}

/// Upper bound in bits of a verbatim stereo frame of `len` samples: side
/// channel at bps + 1, plus header and footer (at most 16 bytes) and padding.
fn verbatim_bits(len: usize, bps: u32) -> usize {
    len * (2 * bps as usize + 1) + 256
}

/// Encoder settings for `bits` (see the module note), and the verbatim-only
/// fallback.
fn configs(bits: BitDepth) -> Result<(Config, Config), ExportError> {
    let mut main = flacenc::config::Encoder::default();
    main.subframe_coding.use_fixed = bits == BitDepth::Bits16;
    let mut plain = flacenc::config::Encoder::default();
    plain.subframe_coding.use_fixed = false;
    plain.subframe_coding.use_lpc = false;
    let v = |c: flacenc::config::Encoder| c.into_verified().map_err(|(_, e)| flac_err(e));
    Ok((v(main)?, v(plain)?))
}

type Config = flacenc::error::Verified<flacenc::config::Encoder>;

/// Per-thread scratch: flacenc's frame buffer and the interleaved codes.
struct Scratch {
    fb: Option<FrameBuf>,
    ints: Vec<i32>,
}

/// Quantises and encodes frame `k` to bytes.
fn encode_frame(
    k: usize,
    s: &mut Scratch,
    [l, r]: [&[f32]; 2],
    block: usize,
    bits: BitDepth,
    config: &(Config, Config),
    info: &StreamInfo,
) -> Result<Vec<u8>, ExportError> {
    let start = k * block;
    let end = (start + block).min(l.len());
    let len = end - start;
    let fb =
        s.fb.as_mut()
            .ok_or_else(|| ExportError::Flac("frame buffer".into()))?;
    if fb.size() != len {
        fb.resize(len);
    }
    let ints = &mut s.ints[..len * CHANNELS];
    let mut dither = Tpdf::at(start);
    quantize_interleaved(&l[start..end], &r[start..end], bits, &mut dither, ints);
    fb.fill_interleaved(ints).map_err(flac_err)?;
    let mut frame = flacenc::encode_fixed_size_frame(&config.0, fb, k, info).map_err(flac_err)?;
    if frame.count_bits() > verbatim_bits(len, bits.bits()) {
        frame = flacenc::encode_fixed_size_frame(&config.1, fb, k, info).map_err(flac_err)?;
    }
    let mut sink = flacenc::bitsink::ByteSink::with_capacity(frame.count_bits());
    frame
        .write(&mut sink)
        .map_err(|e| ExportError::Flac(format!("{e}")))?;
    Ok(sink.into_inner())
}

/// Writes `l`, `r` (equal lengths, checked by the caller) to `path`.
pub(crate) fn write(
    path: &Path,
    l: &[f32],
    r: &[f32],
    sr: u32,
    meta: &Meta,
    bits: BitDepth,
) -> Result<(), ExportError> {
    if sr > MAX_RATE {
        return Err(ExportError::InvalidSampleRate(sr));
    }
    let n = l.len();
    let block = block_size(n)?;
    let frames = n.div_ceil(block);
    let bps = bits.bits();
    let config = configs(bits)?;
    let info = StreamInfo::new(sr as usize, CHANNELS, bps as usize).map_err(flac_err)?;

    let mut out = std::io::BufWriter::with_capacity(1 << 16, std::fs::File::create(path)?);
    let comment = vorbis_comment(meta);
    out.write_all(b"fLaC")?;
    out.write_all(&block_header(false, 0, 34))?;
    let info_at = 8u64;
    out.write_all(&streaminfo(block, 0, 0, sr, bps, n as u64))?;
    out.write_all(&block_header(true, 4, comment.len()))?;
    out.write_all(&comment)?;

    let (mut min_frame, mut max_frame) = (usize::MAX, 0usize);
    let mut k0 = 0;
    while k0 < frames {
        let k1 = (k0 + BATCH).min(frames);
        let encoded: Vec<Result<Vec<u8>, ExportError>> = (k0..k1)
            .into_par_iter()
            .map_init(
                || Scratch {
                    fb: FrameBuf::with_size(CHANNELS, block).ok(),
                    ints: vec![0; block * CHANNELS],
                },
                |s, k| encode_frame(k, s, [l, r], block, bits, &config, &info),
            )
            .collect();
        for bytes in encoded {
            let bytes = bytes?;
            min_frame = min_frame.min(bytes.len());
            max_frame = max_frame.max(bytes.len());
            out.write_all(&bytes)?;
        }
        k0 = k1;
    }
    if frames == 0 {
        min_frame = 0;
    }

    let mut file = out
        .into_inner()
        .map_err(|e| ExportError::Io(e.into_error()))?;
    file.seek(SeekFrom::Start(info_at))?;
    file.write_all(&streaminfo(block, min_frame, max_frame, sr, bps, n as u64))?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_sizes_leave_valid_tails() {
        for n in [
            64,
            100,
            4096,
            4097,
            4096 * 3 + 10,
            4096 * 50 + 63,
            8_202_600,
            8_202_601,
        ] {
            let b = block_size(n).unwrap();
            assert!((MIN_BLOCK..=BLOCK).contains(&b), "n {n} b {b}");
            let t = n % b;
            assert!(t == 0 || t >= MIN_BLOCK, "n {n} b {b} tail {t}");
        }
        assert!(block_size(10).is_err());
    }
}

//! Ogg Vorbis writer: libvorbis through vorbis_rs, quality-based VBR.
//!
//! `quality` is libvorbis's VBR quality, about -0.2 to 1.0; 0.6 gives about
//! 192 kb/s for 44.1 kHz stereo. Tags go into the Vorbis comment header.
//! Encoding is serial: libvorbis has no frame-parallel API.

use std::num::{NonZeroU32, NonZeroU8};
use std::path::Path;

use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

use crate::{ExportError, Meta};

/// Frames per `encode_audio_block` call: 8192 frames is 0.19 s at 44.1 kHz.
/// The block size does not change the encoded stream, only call overhead.
const BLOCK: usize = 8192;

/// Writes `l`, `r` (equal lengths, checked by the caller) to `path`.
pub(crate) fn write(path: &Path, l: &[f32], r: &[f32], sr: u32, meta: &Meta, quality: f32) -> Result<(), ExportError> {
    let sr = NonZeroU32::new(sr).ok_or(ExportError::InvalidSampleRate(sr))?;
    let channels = NonZeroU8::MIN.saturating_add(1);
    let sink = std::io::BufWriter::new(std::fs::File::create(path)?);

    let mut builder = VorbisEncoderBuilder::new(sr, channels, sink)?;
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr { target_quality: quality });
    for (tag, value) in meta.tags() {
        builder.comment_tag(tag, value)?;
    }
    let mut encoder = builder.build()?;
    for (lc, rc) in l.chunks(BLOCK).zip(r.chunks(BLOCK)) {
        encoder.encode_audio_block([lc, rc])?;
    }
    encoder.finish()?;
    Ok(())
}

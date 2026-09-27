//! Audio file export: WAV (hound), Ogg Vorbis (vorbis_rs) and FLAC (flacenc).
//!
//! Format is chosen from the output path's extension. Errors are returned,
//! never panics.

use std::path::Path;

/// Tags written into the file as Vorbis comments (Ogg, FLAC) or a WAV
/// LIST/INFO chunk.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: String,
    /// Empty string becomes "Claude" (the songwriter persona; CLAUDE.md).
    pub artist: String,
    /// The song's liner note (`song.note`).
    pub comment: String,
    /// Year or full date string. Empty means no date tag is written.
    pub date: String,
    /// The style label (e.g. "Cowboy waltz"). Empty means no genre tag.
    pub style: String,
}

impl Meta {
    fn artist_or_default(&self) -> &str {
        if self.artist.is_empty() {
            "Claude"
        } else {
            &self.artist
        }
    }

    /// Vorbis comment pairs (TAG, value), skipping empty values except artist,
    /// which always defaults.
    fn tags(&self) -> Vec<(&'static str, String)> {
        let mut t = vec![];
        if !self.title.is_empty() {
            t.push(("TITLE", self.title.clone()));
        }
        t.push(("ARTIST", self.artist_or_default().to_string()));
        if !self.comment.is_empty() {
            t.push(("COMMENT", self.comment.clone()));
        }
        if !self.date.is_empty() {
            t.push(("DATE", self.date.clone()));
        }
        if !self.style.is_empty() {
            t.push(("GENRE", self.style.clone()));
        }
        t
    }
}

/// Bit depth for FLAC and WAV integer output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Bits16,
    Bits24,
}

/// Format-specific export options. Fields not relevant to the chosen format
/// (from the path extension) are ignored.
#[derive(Clone, Debug)]
pub struct ExportOpts {
    /// Ogg Vorbis VBR quality, roughly [-0.2, 1.0]. Default 0.6 (about
    /// 192 kb/s for a 44.1 kHz stereo signal).
    pub ogg_quality: f32,
    /// FLAC bit depth. Default 24.
    pub flac_bits: BitDepth,
    /// WAV sample format: false = 16-bit PCM (default), true = 32-bit float.
    pub wav_float: bool,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self { ogg_quality: 0.6, flac_bits: BitDepth::Bits24, wav_float: false }
    }
}

/// All the ways `write_audio` can fail.
#[derive(Debug)]
pub enum ExportError {
    /// The output path has no extension, or one that is not ogg/flac/wav.
    UnknownExtension(String),
    /// Left/right channel lengths differ.
    ChannelLengthMismatch { left: usize, right: usize },
    Io(std::io::Error),
    Vorbis(vorbis_rs::VorbisError),
    Flac(String),
    Wav(hound::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownExtension(ext) => {
                write!(f, "unknown output extension {ext:?} (want .ogg, .flac or .wav)")
            }
            Self::ChannelLengthMismatch { left, right } => {
                write!(f, "left/right channel length mismatch: {left} vs {right}")
            }
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Vorbis(e) => write!(f, "Ogg Vorbis encoding error: {e}"),
            Self::Flac(e) => write!(f, "FLAC encoding error: {e}"),
            Self::Wav(e) => write!(f, "WAV write error: {e}"),
        }
    }
}

impl std::error::Error for ExportError {}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<vorbis_rs::VorbisError> for ExportError {
    fn from(e: vorbis_rs::VorbisError) -> Self {
        Self::Vorbis(e)
    }
}
impl From<hound::Error> for ExportError {
    fn from(e: hound::Error) -> Self {
        Self::Wav(e)
    }
}

/// Writes a stereo `f32` signal (nominally in [-1, 1]) to `path`. The format
/// is chosen from `path`'s extension: `.ogg` (Ogg Vorbis, VBR), `.flac`
/// (FLAC, lossless) or `.wav` (PCM or float). `meta` is written as tags
/// where the format supports it.
pub fn write_audio(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    meta: &Meta,
    opts: &ExportOpts,
) -> Result<(), ExportError> {
    if left.len() != right.len() {
        return Err(ExportError::ChannelLengthMismatch { left: left.len(), right: right.len() });
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "ogg" => write_ogg(path, left, right, sample_rate, meta, opts.ogg_quality),
        "flac" => write_flac(path, left, right, sample_rate, meta, opts.flac_bits),
        "wav" => write_wav(path, left, right, sample_rate, meta, opts.wav_float),
        other => Err(ExportError::UnknownExtension(other.to_string())),
    }
}

// ---------------------------------------------------------------- Ogg Vorbis

fn write_ogg(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    meta: &Meta,
    quality: f32,
) -> Result<(), ExportError> {
    use std::num::{NonZeroU32, NonZeroU8};
    use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

    let sr = NonZeroU32::new(sample_rate)
        .ok_or_else(|| ExportError::Vorbis(vorbis_rs::VorbisError::InvalidAudioBlockSampleCount {
            expected: 1,
            actual: 0,
        }))?;
    let channels = NonZeroU8::new(2).expect("2 != 0");
    let file = std::fs::File::create(path)?;
    let sink = std::io::BufWriter::new(file);

    let mut builder = VorbisEncoderBuilder::new(sr, channels, sink)?;
    builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: quality,
    });
    for (tag, value) in meta.tags() {
        builder.comment_tag(tag, value)?;
    }
    let mut encoder = builder.build()?;

    // A few seconds per block, per the crate's own guidance.
    const BLOCK: usize = 8192;
    let mut i = 0;
    while i < left.len() {
        let end = (i + BLOCK).min(left.len());
        encoder.encode_audio_block([&left[i..end], &right[i..end]])?;
        i = end;
    }
    encoder.finish()?;
    Ok(())
}

// -------------------------------------------------------------------- FLAC

fn write_flac(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    meta: &Meta,
    bits: BitDepth,
) -> Result<(), ExportError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    use flacenc::source::{Fill, FrameBuf};

    let bits_per_sample: usize = match bits {
        BitDepth::Bits16 => 16,
        BitDepth::Bits24 => 24,
    };
    // Scale as the legacy WAV writer does (clamp then JS-style round), just
    // at the chosen bit depth's full-scale integer instead of 32767.
    let full_scale = ((1i64 << (bits_per_sample - 1)) - 1) as f64;
    let mut interleaved = Vec::with_capacity(left.len() * 2);
    for i in 0..left.len() {
        interleaved.push(scale_to_int(left[i], full_scale));
        interleaved.push(scale_to_int(right[i], full_scale));
    }

    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|(_, e)| ExportError::Flac(format!("bad encoder config: {e:?}")))?;

    // Encode frame by frame. flacenc's encode_with_fixed_block_size reuses one
    // full-size frame buffer and so repeats stale samples in a short final
    // block; here the last frame gets a buffer of its own, exactly as long as
    // the samples left. Stream::add_frame accumulates the total sample count;
    // the MD5 stays zero, which FLAC defines as "not computed".
    let block_size = config.block_size;
    let flac_err = |e: &dyn std::fmt::Debug| ExportError::Flac(format!("{e:?}"));
    let info = flacenc::component::StreamInfo::new(sample_rate as usize, 2, bits_per_sample)
        .map_err(|e| flac_err(&e))?;
    let mut stream = flacenc::component::Stream::with_stream_info(info);
    let mut full = FrameBuf::with_size(2, block_size).map_err(|e| flac_err(&e))?;
    for (n, chunk) in interleaved.chunks(block_size * 2).enumerate() {
        let len = chunk.len() / 2;
        let mut last;
        let fb = if len == block_size {
            &mut full
        } else {
            last = FrameBuf::with_size(2, len).map_err(|e| flac_err(&e))?;
            &mut last
        };
        fb.fill_interleaved(chunk).map_err(|e| flac_err(&e))?;
        let frame = flacenc::encode_fixed_size_frame(&config, fb, n, stream.stream_info())
            .map_err(|e| flac_err(&e))?;
        stream.add_frame(frame);
    }

    if let Some(block) = vorbis_comment_block(meta) {
        stream.add_metadata_block(
            flacenc::component::MetadataBlockData::new_unknown(4, &block)
                .map_err(|e| ExportError::Flac(format!("{e:?}")))?,
        );
    }

    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| ExportError::Flac(format!("{e:?}")))?;
    std::fs::write(path, sink.as_slice())?;
    Ok(())
}

/// Scales a nominally-[-1,1] sample to an integer sample at `full_scale`
/// (32767 for 16-bit, 8388607 for 24-bit): clamp to [-1,1], multiply, then
/// JS `Math.round` (halves round toward +infinity), matching the legacy WAV
/// writer's scaling rule at whatever bit depth is in use.
fn scale_to_int(x: f32, full_scale: f64) -> i32 {
    let clamped = (x as f64).max(-1.0).min(1.0);
    sfcore::js::round(clamped * full_scale) as i32
}

/// Builds a raw FLAC `VORBIS_COMMENT` metadata block body (block type 4;
/// same tag layout as an Ogg Vorbis comment header, without the packet
/// framing): a vendor string, a comment count, then each "TAG=value" comment,
/// all as `u32`-length-prefixed little-endian byte strings. flacenc has no
/// typed Vorbis-comment block of its own (only `MetadataBlockData::Unknown`),
/// so this is written by hand. Returns `None` if there are no tags at all.
fn vorbis_comment_block(meta: &Meta) -> Option<Vec<u8>> {
    let tags = meta.tags();
    if tags.is_empty() {
        return None;
    }
    let vendor = b"electric-sunflowers export";
    let mut out = Vec::new();
    out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    out.extend_from_slice(vendor);
    out.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for (tag, value) in &tags {
        let entry = format!("{tag}={value}");
        let bytes = entry.as_bytes();
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    Some(out)
}

// --------------------------------------------------------------------- WAV

fn write_wav(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    meta: &Meta,
    float: bool,
) -> Result<(), ExportError> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: if float { 32 } else { 16 },
        sample_format: if float { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    if float {
        for i in 0..left.len() {
            writer.write_sample(left[i])?;
            writer.write_sample(right[i])?;
        }
    } else {
        for i in 0..left.len() {
            writer.write_sample(pcm16(left[i]))?;
            writer.write_sample(pcm16(right[i]))?;
        }
    }
    writer.finalize()?;

    // hound 3.5 has no LIST/INFO chunk support (checked its public API: no
    // metadata-writing method exists), so append one by hand after the
    // fact and patch the RIFF size. This is the "if hound supports it"
    // case failing narrowly -- hound doesn't write it, but a WAV file with
    // extra chunks after `data` is still valid, so we can still add it.
    append_list_info(path, meta)?;
    Ok(())
}

fn pcm16(x: f32) -> i16 {
    let clamped = (x as f64).max(-1.0).min(1.0);
    sfcore::js::round(clamped * 32767.0) as i16
}

/// Appends a `LIST` `INFO` chunk (INAM/IART/ICMT/ICRD/IGNR) to an
/// already-finalized WAV file at `path`, and fixes up the RIFF chunk size.
fn append_list_info(path: &Path, meta: &Meta) -> Result<(), ExportError> {
    let mut info = Vec::new();
    info.extend_from_slice(b"INFO");
    push_info_sub(&mut info, b"INAM", &meta.title);
    push_info_sub(&mut info, b"IART", meta.artist_or_default());
    push_info_sub(&mut info, b"ICMT", &meta.comment);
    push_info_sub(&mut info, b"ICRD", &meta.date);
    push_info_sub(&mut info, b"IGNR", &meta.style);
    if info.len() == 4 {
        // Nothing but the "INFO" tag: no tags were set, skip the chunk.
        return Ok(());
    }

    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"LIST");
    chunk.extend_from_slice(&(info.len() as u32).to_le_bytes());
    chunk.extend_from_slice(&info);
    if chunk.len() % 2 != 0 {
        chunk.push(0); // RIFF chunks are word-aligned.
    }

    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let old_len = file.metadata()?.len();
    file.seek_and_write_all(std::io::SeekFrom::End(0), &chunk)?;

    let new_riff_size = (old_len as u64 + chunk.len() as u64).saturating_sub(8);
    let mut riff_size = std::io::Cursor::new(Vec::new());
    riff_size.write_all(&(new_riff_size as u32).to_le_bytes())?;
    use std::io::{Seek, Write as _};
    file.seek(std::io::SeekFrom::Start(4))?;
    file.write_all(riff_size.get_ref())?;
    Ok(())
}

fn push_info_sub(info: &mut Vec<u8>, id: &[u8; 4], value: &str) {
    if value.is_empty() {
        return;
    }
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0); // NUL-terminated per RIFF INFO convention.
    info.extend_from_slice(id);
    info.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    info.extend_from_slice(&bytes);
    if bytes.len() % 2 != 0 {
        info.push(0);
    }
}

/// Small helper so `append_list_info` reads as one seek-then-write.
trait SeekWriteAll {
    fn seek_and_write_all(&mut self, pos: std::io::SeekFrom, buf: &[u8]) -> std::io::Result<()>;
}
impl<T: std::io::Seek + std::io::Write> SeekWriteAll for T {
    fn seek_and_write_all(&mut self, pos: std::io::SeekFrom, buf: &[u8]) -> std::io::Result<()> {
        self.seek(pos)?;
        self.write_all(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_rounds_js_style() {
        assert_eq!(sfcore::js::round(0.5), 1.0);
        assert_eq!(sfcore::js::round(-0.5), 0.0);
        assert_eq!(pcm16(1.5), 32767); // clamp first
        assert_eq!(pcm16(-1.5), -32767);
    }

    #[test]
    fn unknown_extension_errors() {
        let meta = Meta::default();
        let opts = ExportOpts::default();
        let err = write_audio(Path::new("/tmp/x.mp3"), &[0.0], &[0.0], 44100, &meta, &opts)
            .unwrap_err();
        assert!(matches!(err, ExportError::UnknownExtension(_)));
    }

    #[test]
    fn channel_mismatch_errors() {
        let meta = Meta::default();
        let opts = ExportOpts::default();
        let err = write_audio(Path::new("/tmp/x.wav"), &[0.0, 0.0], &[0.0], 44100, &meta, &opts)
            .unwrap_err();
        assert!(matches!(err, ExportError::ChannelLengthMismatch { left: 2, right: 1 }));
    }

    #[test]
    fn wav_round_trip_tags() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("export_test_{}.wav", std::process::id()));
        let meta = Meta {
            title: "Test Song".to_string(),
            artist: String::new(),
            comment: "a liner note".to_string(),
            date: "2026".to_string(),
            style: "cowboy".to_string(),
        };
        let opts = ExportOpts::default();
        let l = vec![0.0f32, 0.5, -0.5];
        let r = vec![0.0f32, -0.5, 0.5];
        write_audio(&path, &l, &r, 44100, &meta, &opts).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        // "LIST"..."INFO" must be present, with the default artist since
        // none was given.
        let hay = String::from_utf8_lossy(&bytes);
        assert!(hay.contains("LIST"));
        assert!(hay.contains("INFO"));
        assert!(hay.contains("Claude"));
        assert!(hay.contains("Test Song"));
        std::fs::remove_file(&path).ok();
    }
}

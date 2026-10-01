//! Audio file export: WAV (direct RIFF writer), FLAC (flacenc, frames in
//! parallel) and Ogg Vorbis (vorbis_rs).
//!
//! `write` takes a typed `Format`; `Format::from_path` picks one from the
//! file extension. Integer output is quantised by `dither::quantize`
//! (round half away from zero; TPDF dither at 16 bits only). Errors are
//! returned, never panics.

use std::path::Path;

pub mod dither;
mod flac;
mod ogg;
mod wav;

pub use dither::{quantize, Tpdf};

/// Written into every file (Vorbis PERFORMER, WAV ISFT): the singer is
/// synthetic and is not a person (CLAUDE.md, voice rule).
pub const VOICE_NOTICE: &str = "Synthetic voice: Electric Sunflowers engine. No human singer.";

/// Tags written into the file as Vorbis comments (Ogg, FLAC) or a WAV
/// LIST/INFO chunk.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: String,
    /// Empty string becomes "Claude" (the songwriter persona; CLAUDE.md).
    pub artist: String,
    /// The song's liner note.
    pub comment: String,
    /// Year or full date string. Empty means no date tag.
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

    /// Vorbis comment pairs (TAG, value); empty values are skipped, except
    /// the artist, which defaults, and the performer, always `VOICE_NOTICE`.
    fn tags(&self) -> Vec<(&'static str, &str)> {
        let mut t = vec![];
        if !self.title.is_empty() {
            t.push(("TITLE", self.title.as_str()));
        }
        t.push(("ARTIST", self.artist_or_default()));
        t.push(("PERFORMER", VOICE_NOTICE));
        for (k, v) in [
            ("COMMENT", &self.comment),
            ("DATE", &self.date),
            ("GENRE", &self.style),
        ] {
            if !v.is_empty() {
                t.push((k, v.as_str()));
            }
        }
        t
    }
}

/// Bit depth for FLAC output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Bits16,
    Bits24,
}

/// WAV sample format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WavSample {
    /// 16-bit PCM, TPDF dithered.
    Pcm16,
    /// 24-bit PCM, undithered.
    Pcm24,
    /// 32-bit IEEE float, samples written unchanged.
    Float32,
}

/// Output format and its settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    /// Ogg Vorbis, VBR at libvorbis `quality` (about -0.2 to 1.0).
    Ogg { quality: f32 },
    /// FLAC, lossless at `bits`.
    Flac { bits: BitDepth },
    /// RIFF/WAVE.
    Wav { sample: WavSample },
}

impl Format {
    /// Picks the format from `path`'s extension (case-insensitive): `.ogg`
    /// with `quality`; `.flac` at 16 bits if `flac16`, else 24; `.wav` as
    /// 32-bit float if `float`, else 16-bit PCM.
    pub fn from_path(
        path: &Path,
        quality: f32,
        flac16: bool,
        float: bool,
    ) -> Result<Format, ExportError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "ogg" => Ok(Format::Ogg { quality }),
            "flac" => Ok(Format::Flac {
                bits: if flac16 {
                    BitDepth::Bits16
                } else {
                    BitDepth::Bits24
                },
            }),
            "wav" => Ok(Format::Wav {
                sample: if float {
                    WavSample::Float32
                } else {
                    WavSample::Pcm16
                },
            }),
            _ => Err(ExportError::UnknownExtension(ext)),
        }
    }
}

/// All the ways an export can fail.
#[derive(Debug)]
pub enum ExportError {
    /// The output path has no extension, or one that is not ogg/flac/wav.
    UnknownExtension(String),
    /// Left/right channel lengths differ.
    ChannelLengthMismatch {
        left: usize,
        right: usize,
    },
    /// Zero, or above the format's limit (FLAC: 2^20 - 1 Hz).
    InvalidSampleRate(u32),
    /// The WAV file would exceed the 4 GiB RIFF limit.
    TooLarge {
        bytes: u64,
    },
    Io(std::io::Error),
    Vorbis(vorbis_rs::VorbisError),
    Flac(String),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownExtension(ext) => {
                write!(
                    f,
                    "unknown output extension {ext:?} (want .ogg, .flac or .wav)"
                )
            }
            Self::ChannelLengthMismatch { left, right } => {
                write!(f, "left/right channel length mismatch: {left} vs {right}")
            }
            Self::InvalidSampleRate(sr) => write!(f, "invalid sample rate {sr} Hz"),
            Self::TooLarge { bytes } => {
                write!(f, "WAV file of {bytes} bytes exceeds the 4 GiB RIFF limit")
            }
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Vorbis(e) => write!(f, "Ogg Vorbis encoding error: {e}"),
            Self::Flac(e) => write!(f, "FLAC encoding error: {e}"),
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

/// Writes the stereo signal `left`, `right` (nominally in [-1, 1]) at
/// `sample_rate` to `path` as `fmt`, with `meta` as tags.
pub fn write(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
    meta: &Meta,
    fmt: Format,
) -> Result<(), ExportError> {
    if left.len() != right.len() {
        return Err(ExportError::ChannelLengthMismatch {
            left: left.len(),
            right: right.len(),
        });
    }
    if sample_rate == 0 {
        return Err(ExportError::InvalidSampleRate(0));
    }
    match fmt {
        Format::Ogg { quality } => ogg::write(path, left, right, sample_rate, meta, quality),
        Format::Flac { bits } => flac::write(path, left, right, sample_rate, meta, bits),
        Format::Wav { sample } => wav::write(path, left, right, sample_rate, meta, sample),
    }
}

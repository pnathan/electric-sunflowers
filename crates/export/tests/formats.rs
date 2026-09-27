//! Format-level tests: Ogg header/tag presence (parsed by hand, no ffmpeg
//! needed), and a FLAC round trip decoded by `ffmpeg` (skipped if it is not
//! on PATH).

use export::{ExportOpts, Meta};
use std::path::PathBuf;
use std::process::Command;

fn test_signal(n: usize) -> (Vec<f32>, Vec<f32>) {
    // A couple of audible sine tones, well away from silence so no encoder
    // can drop the block as inaudible.
    let sr = 44100.0f32;
    let l: Vec<f32> =
        (0..n).map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr).sin()).collect();
    let r: Vec<f32> =
        (0..n).map(|i| 0.3 * (2.0 * std::f32::consts::PI * 660.0 * i as f32 / sr).sin()).collect();
    (l, r)
}

fn test_meta() -> Meta {
    Meta {
        title: "Format Test".to_string(),
        artist: String::new(),
        comment: "a liner note for the format test".to_string(),
        date: "2026".to_string(),
        style: "cowboy".to_string(),
    }
}

fn tmp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("export_fmt_test_{}_{name}", std::process::id()))
}

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg").arg("-version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// An Ogg file is a sequence of pages, each starting "OggS"; the comment
/// header packet carries "vorbis" plus our tag strings in plain ASCII, so a
/// presence check does not need a full Vorbis parser.
#[test]
fn ogg_has_header_and_tags() {
    let path = tmp_path("tags.ogg");
    let (l, r) = test_signal(44100 * 2);
    export::write_audio(&path, &l, &r, 44100, &test_meta(), &ExportOpts::default()).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"OggS", "not a valid Ogg file");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("vorbis"), "no vorbis codec marker");
    assert!(text.contains("TITLE=Format Test"));
    assert!(text.contains("ARTIST=Claude"));
    assert!(text.contains("COMMENT=a liner note for the format test"));
    assert!(text.contains("DATE=2026"));
    assert!(text.contains("GENRE=cowboy"));

    std::fs::remove_file(&path).ok();
}

/// FLAC round trip: encode, decode with `ffmpeg`, and check the header
/// magic plus that the file is not silent. Skipped if `ffmpeg` is missing;
/// `crates/export/README` (repo `CLAUDE.md`) notes ffmpeg is expected to be
/// installed for the fuller manual verification this only spot-checks.
#[test]
fn flac_round_trips_through_ffmpeg() {
    if !ffmpeg_available() {
        eprintln!("skipping flac_round_trips_through_ffmpeg: ffmpeg not on PATH");
        return;
    }
    let path = tmp_path("roundtrip.flac");
    let (l, r) = test_signal(44100 * 2);
    export::write_audio(&path, &l, &r, 44100, &test_meta(), &ExportOpts::default()).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[0..4], b"fLaC", "not a valid FLAC file");

    let decoded = tmp_path("roundtrip_decoded.wav");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            path.to_str().unwrap(),
            "-f",
            "wav",
            "-acodec",
            "pcm_f32le",
            decoded.to_str().unwrap(),
            "-hide_banner",
            "-loglevel",
            "error",
        ])
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg failed to decode the FLAC file");

    let wav_bytes = std::fs::read(&decoded).unwrap();
    // Find the "data" chunk (may follow an extended "fmt " chunk) and check
    // it is non-trivial and not all zero.
    let data = find_data_chunk(&wav_bytes);
    assert!(data.len() > 44100 * 4, "decoded FLAC is implausibly short");
    assert!(data.iter().any(|&b| b != 0), "decoded FLAC audio is silent");

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&decoded).ok();
}

fn find_data_chunk(b: &[u8]) -> &[u8] {
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        if id == b"data" {
            return &b[i + 8..i + 8 + len];
        }
        i += 8 + len + (len % 2);
    }
    panic!("no data chunk found");
}

/// The 16-bit PCM scaling must match the legacy `sunflower::wav` writer
/// exactly: clamp to [-1,1], multiply by 32767, then JS-style `Math.round`.
/// This is the same formula the old writer used (see git history), checked
/// again here so a future change to `export` cannot silently drift from it.
#[test]
fn wav_16bit_scaling_matches_legacy_formula() {
    let path = tmp_path("scale.wav");
    let l = vec![1.5f32, -1.5, 0.0, 0.49999, -0.49999];
    let r = vec![0.0f32; 5];
    let opts = ExportOpts { wav_float: false, ..ExportOpts::default() };
    export::write_audio(&path, &l, &r, 44100, &Meta::default(), &opts).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let data = find_data_chunk(&bytes);
    let mut samples = vec![];
    let mut i = 0;
    while i + 1 < data.len() {
        samples.push(i16::from_le_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    // Interleaved L,R,L,R,...; left channel only.
    let left: Vec<i16> = samples.iter().step_by(2).copied().collect();
    assert_eq!(left, vec![32767, -32767, 0, 16383, -16383], "scaling drifted from the legacy formula");

    std::fs::remove_file(&path).ok();
}

/// A FLAC file whose length is not a multiple of the block size must decode
/// to exactly its true length, and its short final block must hold the real
/// tail (flacenc's fixed-block helper repeats stale samples there).
#[test]
fn flac_partial_final_block_has_true_length_and_tail() {
    if !ffmpeg_available() {
        eprintln!("skipping flac_partial_final_block_has_true_length_and_tail: ffmpeg not on PATH");
        return;
    }
    let n = 44100 * 2 + 77; // deliberately not a multiple of the 4096 block size
    let (l, r) = test_signal(n);
    let path = tmp_path("partial_block.flac");
    export::write_audio(&path, &l, &r, 44100, &Meta::default(), &ExportOpts::default()).unwrap();

    let decoded = tmp_path("partial_block_decoded.wav");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-i",
            path.to_str().unwrap(),
            "-f",
            "wav",
            "-acodec",
            "pcm_s32le",
            decoded.to_str().unwrap(),
            "-hide_banner",
            "-loglevel",
            "error",
        ])
        .status()
        .expect("run ffmpeg");
    assert!(status.success());

    let wav_bytes = std::fs::read(&decoded).unwrap();
    let data = find_data_chunk(&wav_bytes);
    let mut samples = vec![];
    let mut i = 0;
    while i + 3 < data.len() {
        samples.push(i32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]));
        i += 4;
    }
    // Interleaved stereo, 24-bit samples in the top bits of s32.
    assert_eq!(samples.len(), n * 2, "decoded length differs from the input");
    let full = 8388607.0f64;
    for k in (n - 200)..n {
        for (ch, x) in [(0, l[k]), (1, r[k])] {
            let want = ((x as f64).clamp(-1.0, 1.0) * full + 0.5).floor() as i32;
            assert_eq!(samples[k * 2 + ch] >> 8, want, "tail sample {k} channel {ch}");
        }
    }

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&decoded).ok();
}

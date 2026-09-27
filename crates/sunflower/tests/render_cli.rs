//! Integration tests: run the built `sunflower` binary against
//! `tests/small_song.json` and check the WAV it writes.

use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sunflower"))
}

fn song_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/small_song.json")
}

/// Reads a WAV file's header fields (as sunflower's own writer lays them
/// out: 44-byte PCM/float header) and the raw sample bytes.
struct Wav {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    data: Vec<u8>,
}

fn read_wav(path: &std::path::Path) -> Wav {
    let bytes = std::fs::read(path).expect("read wav");
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(&bytes[12..16], b"fmt ");
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
    assert_eq!(&bytes[36..40], b"data");
    let data_len = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
    let data = bytes[44..44 + data_len].to_vec();
    Wav { channels, sample_rate, bits_per_sample, data }
}

fn pcm16_peak(w: &Wav) -> i16 {
    assert_eq!(w.bits_per_sample, 16);
    let mut peak = 0i16;
    let mut i = 0;
    while i + 1 < w.data.len() {
        let s = i16::from_le_bytes([w.data[i], w.data[i + 1]]);
        peak = peak.max(s.abs());
        i += 2;
    }
    peak
}

#[test]
fn render_produces_a_valid_nonsilent_wav() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("small.wav");

    let status = Command::new(bin())
        .args(["render", song_path().to_str().unwrap(), "--seed", "7", "-o", out.to_str().unwrap()])
        .status()
        .expect("run sunflower render");
    assert!(status.success(), "sunflower render exited with {status:?}");

    let w = read_wav(&out);
    assert_eq!(w.channels, 2);
    assert_eq!(w.sample_rate, 44100);
    assert_eq!(w.bits_per_sample, 16);
    // A few seconds of a three-section song at 96 bpm.
    let n_frames = w.data.len() / 4;
    assert!(n_frames > 44100, "expected more than a second of audio, got {n_frames} frames");
    assert!(pcm16_peak(&w) > 1000, "expected an audible peak, got {}", pcm16_peak(&w));
}

#[test]
fn same_seed_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let out1 = dir.path().join("a.wav");
    let out2 = dir.path().join("b.wav");

    for out in [&out1, &out2] {
        let status = Command::new(bin())
            .args(["render", song_path().to_str().unwrap(), "--seed", "1234", "-o", out.to_str().unwrap()])
            .status()
            .expect("run sunflower render");
        assert!(status.success());
    }

    let a = std::fs::read(&out1).unwrap();
    let b = std::fs::read(&out2).unwrap();
    assert_eq!(a, b, "same seed should reproduce byte-identical output");
}

#[test]
fn unknown_style_errors_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let output = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--style",
            "not-a-real-style",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run sunflower render");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown style"), "stderr: {stderr}");
    assert!(!out.exists(), "should not have written an output file on error");
}

#[test]
fn missing_song_file_errors_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let output = Command::new(bin())
        .args(["render", "/no/such/song.json", "-o", out.to_str().unwrap()])
        .output()
        .expect("run sunflower render");
    assert!(!output.status.success());
}

#[test]
fn unknown_no_track_errors_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.wav");
    let output = Command::new(bin())
        .args(["render", song_path().to_str().unwrap(), "--no", "bogus", "-o", out.to_str().unwrap()])
        .output()
        .expect("run sunflower render");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown --no track"), "stderr: {stderr}");
}

#[test]
fn styles_command_lists_known_keys() {
    let output = Command::new(bin()).arg("styles").output().expect("run sunflower styles");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cowboy"));
    assert!(stdout.contains("bluegrass"));
}

/// Default output (no `-o`) is now `song.ogg`, not a WAV file, and it
/// carries the song's title/note as Vorbis comments.
#[test]
fn default_output_is_ogg_with_tags() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new(bin())
        .current_dir(&dir)
        .args(["render", song_path().to_str().unwrap(), "--seed", "7"])
        .status()
        .expect("run sunflower render");
    assert!(status.success());

    let out = dir.path().join("song.ogg");
    assert!(out.exists(), "expected default output at song.ogg");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"OggS");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("vorbis"));
    assert!(text.contains("ARTIST=Claude"));
}

/// `-o` picks the format from the extension; `.flac` produces a valid FLAC
/// file (magic bytes "fLaC").
#[test]
fn dash_o_flac_extension_picks_flac_format() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("small.flac");
    let status = Command::new(bin())
        .args(["render", song_path().to_str().unwrap(), "--seed", "7", "-o", out.to_str().unwrap()])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[0..4], b"fLaC");
}

/// `--quality` is accepted for an Ogg render and does not error.
#[test]
fn quality_flag_is_accepted_for_ogg() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("q.ogg");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--quality",
            "0.2",
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    assert!(out.exists());
}

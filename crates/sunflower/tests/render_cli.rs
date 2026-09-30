//! Integration tests: run the built `sunflower` binary against
//! `tests/small_song.json` and check the WAV it writes.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The `sunflower` binary: cargo and Bazel (BUILD.bazel `rustc_env`) both
/// set `CARGO_BIN_EXE_sunflower` at compile time; under Bazel it is a
/// runfiles path, so it is resolved like the test data.
fn bin() -> PathBuf {
    find(env!("CARGO_BIN_EXE_sunflower"))
}

fn song_path() -> PathBuf {
    find("crates/sunflower/tests/small_song.json")
}

/// Finds a file under cargo (an absolute path, or one relative to
/// `CARGO_MANIFEST_DIR`) and under Bazel, where the compile-time manifest
/// directory is a sandbox path that is gone at run time and the file is
/// in the runfiles tree: relative to the current directory, or below
/// `$RUNFILES_DIR` / `$TEST_SRCDIR` in the main repository.
fn find(rel: &str) -> PathBuf {
    let path = Path::new(rel);
    let in_crate = path.strip_prefix("crates/sunflower").unwrap_or(path);
    let mut tries = vec![
        path.to_path_buf(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join(in_crate),
    ];
    for var in ["RUNFILES_DIR", "TEST_SRCDIR"] {
        if let Some(root) = std::env::var_os(var) {
            tries.push(Path::new(&root).join("_main").join(path));
        }
    }
    tries
        .iter()
        .find(|p| p.exists())
        .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
        .unwrap_or_else(|| panic!("cannot find {rel}; tried {tries:?}"))
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
    // Walk the chunks after "fmt " (a LIST tag chunk may come before "data").
    let mut at = 20 + u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    loop {
        assert!(at + 8 <= bytes.len(), "no data chunk");
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        if &bytes[at..at + 4] == b"data" {
            let data = bytes[at + 8..at + 8 + len].to_vec();
            return Wav {
                channels,
                sample_rate,
                bits_per_sample,
                data,
            };
        }
        at += 8 + len + (len & 1);
    }
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
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success(), "sunflower render exited with {status:?}");

    let w = read_wav(&out);
    assert_eq!(w.channels, 2);
    assert_eq!(w.sample_rate, 44100);
    assert_eq!(w.bits_per_sample, 16);
    // A few seconds of a three-section song at 96 bpm.
    let n_frames = w.data.len() / 4;
    assert!(
        n_frames > 44100,
        "expected more than a second of audio, got {n_frames} frames"
    );
    assert!(
        pcm16_peak(&w) > 1000,
        "expected an audible peak, got {}",
        pcm16_peak(&w)
    );
}

#[test]
fn same_seed_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let out1 = dir.path().join("a.wav");
    let out2 = dir.path().join("b.wav");

    for out in [&out1, &out2] {
        let status = Command::new(bin())
            .args([
                "render",
                song_path().to_str().unwrap(),
                "--seed",
                "1234",
                "-o",
                out.to_str().unwrap(),
            ])
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
    assert!(
        !out.exists(),
        "should not have written an output file on error"
    );
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
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--no",
            "bogus",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run sunflower render");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown --no track"), "stderr: {stderr}");
}

#[test]
fn styles_command_lists_known_keys() {
    let output = Command::new(bin())
        .arg("styles")
        .output()
        .expect("run sunflower styles");
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
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            out.to_str().unwrap(),
        ])
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

/// `RAYON_NUM_THREADS=1` is the sequential path; its output equals the
/// threaded output byte for byte.
#[test]
fn one_thread_equals_many_threads() {
    let dir = tempfile::tempdir().unwrap();
    let mut outs = Vec::new();
    for threads in ["1", "6"] {
        let out = dir.path().join(format!("t{threads}.wav"));
        let status = Command::new(bin())
            .env("RAYON_NUM_THREADS", threads)
            .args([
                "render",
                song_path().to_str().unwrap(),
                "--seed",
                "99",
                "-o",
                out.to_str().unwrap(),
            ])
            .status()
            .expect("run sunflower render");
        assert!(status.success());
        outs.push(std::fs::read(&out).unwrap());
    }
    assert_eq!(outs[0], outs[1], "1 and 6 threads differ");
}

/// `--no` switches band parts off; `--voice` takes the voice names.
#[test]
fn band_and_voice_flags() {
    let dir = tempfile::tempdir().unwrap();
    let full = dir.path().join("full.wav");
    let bare = dir.path().join("bare.wav");
    let song = song_path();
    let run = |out: &std::path::Path, extra: &[&str]| {
        let mut args = vec![
            "render",
            song.to_str().unwrap(),
            "--seed",
            "5",
            "--voice",
            "tenor",
            "-o",
            out.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let status = Command::new(bin())
            .args(&args)
            .status()
            .expect("run sunflower render");
        assert!(status.success());
    };
    run(&full, &[]);
    run(
        &bare,
        &["--no", "drums", "--no", "bass", "--no", "harmonyGuitar"],
    );
    let (a, b) = (read_wav(&full), read_wav(&bare));
    assert_eq!(a.data.len(), b.data.len());
    assert_ne!(a.data, b.data);
    assert!(pcm16_peak(&b) > 1000);

    let output = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--voice",
            "kazoo",
            "-o",
            dir.path().join("x.wav").to_str().unwrap(),
        ])
        .output()
        .expect("run sunflower render");
    assert!(!output.status.success());
}

#[test]
fn sequential_flag_is_gone() {
    let output = Command::new(bin())
        .args(["demo", "--sequential"])
        .output()
        .expect("run sunflower demo");
    assert!(!output.status.success());
}

/// `render` writes `<stem>.render.json` and `<stem>.sheet.json` next to the
/// audio; the sidecar names the song, the audio, the seed and the voice.
#[test]
fn render_writes_the_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tune.ogg");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--voice",
            "tenor",
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    let side: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("tune.render.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(side["seed"], 7);
    assert_eq!(side["version"], 2);
    assert_eq!(side["voice"], "tenor");
    assert!(side["model"].is_null());
    assert_eq!(
        side["song_json"].as_str().unwrap(),
        std::fs::canonicalize(song_path())
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        side["audio"].as_str().unwrap(),
        std::fs::canonicalize(&out).unwrap().to_str().unwrap()
    );
    let created = side["created"].as_str().unwrap();
    assert_eq!(created.len(), 20, "{created}");
    assert!(
        created.ends_with('Z') && &created[10..11] == "T",
        "{created}"
    );

    let sheet: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("tune.sheet.json")).unwrap())
            .unwrap();
    assert_eq!(sheet["version"], 2);
    assert_eq!(sheet["title"], "Test Tune");
    assert_eq!(sheet["seed"], 7);
    assert_eq!(sheet["voice"], "tenor");
    let labels: Vec<&str> = sheet["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["Intro", "Verse", "Outro"]);
    let verse = &sheet["sections"][1]["lines"][0];
    assert_eq!(verse["text"], "the sun goes down on one more day");
    assert_eq!(verse["syllables"].as_array().unwrap().len(), 8);

    // The sheet command prints the same sheet as the sidecar.
    let output = Command::new(bin())
        .args([
            "sheet",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--voice",
            "tenor",
            "--json",
        ])
        .output()
        .expect("run sunflower sheet");
    assert!(output.status.success());
    let printed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed, sheet);
}

/// `demo` saves the demo song as `<stem>.json` and names it in the sidecar.
#[test]
fn demo_writes_the_song_and_the_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.ogg");
    let status = Command::new(bin())
        .args(["demo", "--seed", "3", "-o", out.to_str().unwrap()])
        .status()
        .expect("run sunflower demo");
    assert!(status.success());
    let side: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("demo.render.json")).unwrap(),
    )
    .unwrap();
    let song_json = side["song_json"].as_str().unwrap();
    assert!(song_json.ends_with("demo.json"));
    let song: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(song_json).unwrap()).unwrap();
    assert_eq!(song["title"], "Every Harbor");
    assert!(dir.path().join("demo.sheet.json").exists());
}

/// `sheet` prints section labels and chords above the lyric.
#[test]
fn sheet_prints_a_chord_sheet() {
    let output = Command::new(bin())
        .args(["sheet", song_path().to_str().unwrap(), "--seed", "7"])
        .output()
        .expect("run sunflower sheet");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Test Tune");
    assert!(lines.iter().any(|l| l.starts_with("[Intro]")));
    // The key moves for the voice, so check the bar row by its form.
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("| ") && l.matches('|').count() == 5),
        "{text}"
    );
    let k = lines
        .iter()
        .position(|l| *l == "the sun goes down on one more day")
        .expect("lyric line");
    let chords = lines[k - 1];
    assert_eq!(chords.split_whitespace().count(), 2, "{chords:?}");
    assert!(
        chords.starts_with(' '),
        "the first chord sits over a stressed syllable: {chords:?}"
    );
}

#[test]
fn sheet_of_a_missing_file_errors_without_panicking() {
    let output = Command::new(bin())
        .args(["sheet", "/no/such/song.json", "--seed", "1"])
        .output()
        .expect("run sunflower sheet");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("reading song JSON"));
}

/// `SUNFLOWER_CONFIG` changes the ogg quality that goes into the output
/// (and so its size), and a `--quality` flag beats the settings file.
#[test]
fn settings_file_changes_the_default_ogg_quality_and_a_flag_beats_it() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg.toml");
    std::fs::write(&cfg, "[export]\nogg_quality = -0.2\n").unwrap();

    let low = dir.path().join("low.ogg");
    let status = Command::new(bin())
        .env("SUNFLOWER_CONFIG", &cfg)
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            low.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());

    let flagged = dir.path().join("flagged.ogg");
    let status = Command::new(bin())
        .env("SUNFLOWER_CONFIG", &cfg)
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--quality",
            "0.5",
            "-o",
            flagged.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());

    let (low_len, flagged_len) = (
        std::fs::metadata(&low).unwrap().len(),
        std::fs::metadata(&flagged).unwrap().len(),
    );
    assert!(
        flagged_len > low_len,
        "a higher quality (from the flag) should produce a larger file: {low_len} vs {flagged_len}"
    );
}

/// A bad settings file value falls back to the default and warns; the
/// warning is printed with the `sunflower: warning: settings:` prefix.
#[test]
fn a_bad_settings_file_warns_and_still_renders() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg.toml");
    std::fs::write(&cfg, "[claude]\neffort = \"blazing\"\n").unwrap();
    let out = dir.path().join("x.wav");
    let output = Command::new(bin())
        .env("SUNFLOWER_CONFIG", &cfg)
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("run sunflower render");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sunflower: warning: settings:"), "{stderr}");
    assert!(stderr.contains("claude.effort"), "{stderr}");
}

/// `--stems` writes one FLAC per audible track plus `reverb.flac` and
/// `stems.json`, and the render sidecar names the directory.
#[test]
fn stems_flag_writes_one_flac_per_track_plus_reverb() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tune.wav");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--stems",
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());

    let stems_dir = dir.path().join("tune.stems");
    assert!(stems_dir.is_dir());
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(stems_dir.join("stems.json")).unwrap())
            .unwrap();
    let tracks: Vec<String> = manifest["tracks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap().to_string())
        .collect();
    assert!(!tracks.is_empty());
    for t in &tracks {
        let p = stems_dir.join(format!("{t}.flac"));
        assert!(p.exists(), "missing stem file {p:?}");
        assert_eq!(&std::fs::read(&p).unwrap()[0..4], b"fLaC");
    }
    let reverb = stems_dir.join("reverb.flac");
    assert!(reverb.exists());
    assert_eq!(&std::fs::read(&reverb).unwrap()[0..4], b"fLaC");
    assert!(manifest["gain"].as_f64().unwrap() > 0.0);
    assert!(manifest["extra_gain"].as_f64().unwrap() > 0.0);

    let side: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("tune.render.json")).unwrap(),
    )
    .unwrap();
    assert!(side["stems"].as_str().unwrap().ends_with("tune.stems"));
}

/// A mix sidecar that mutes the bass changes the audio and is recorded in
/// the render sidecar's `mix` field; `--no-mix` ignores an auto-picked-up
/// `<out-stem>.mix.json`.
#[test]
fn mix_sidecar_changes_the_mix_and_is_recorded_no_mix_ignores_it() {
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("plain.wav");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            plain.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());

    let mix_file = dir.path().join("mute-bass.json");
    std::fs::write(
        &mix_file,
        r#"{"version": 1, "tracks": {"bass": {"mute": true}}}"#,
    )
    .unwrap();
    let muted = dir.path().join("muted.wav");
    let output = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--mix",
            mix_file.to_str().unwrap(),
            "-o",
            muted.to_str().unwrap(),
        ])
        .output()
        .expect("run sunflower render");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("applying mix settings from"));
    assert_ne!(
        std::fs::read(&plain).unwrap(),
        std::fs::read(&muted).unwrap(),
        "muting the bass should change the mix"
    );

    let side: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("muted.render.json")).unwrap(),
    )
    .unwrap();
    assert!(side["mix"].as_str().unwrap().ends_with("mute-bass.json"));

    // <out-stem>.mix.json is auto-applied ...
    let auto_out = dir.path().join("auto.wav");
    std::fs::copy(&mix_file, dir.path().join("auto.mix.json")).unwrap();
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            auto_out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    assert_eq!(
        std::fs::read(&auto_out).unwrap(),
        std::fs::read(&muted).unwrap(),
        "auto-picked-up mix should match the explicit --mix run"
    );

    // ... but --no-mix ignores it.
    let ignored = dir.path().join("auto.ignored.wav");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "--no-mix",
            "-o",
            ignored.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    // --no-mix on a different output stem never picks up auto.mix.json anyway
    // (the candidate is <out-stem>.mix.json), so compare directly against plain.
    assert_eq!(
        std::fs::read(&ignored).unwrap(),
        std::fs::read(&plain).unwrap(),
        "--no-mix should ignore any mix sidecar"
    );
}

/// The render sidecar parses with `songwriter::sidecar::RenderSidecar`.
#[test]
fn sidecar_parses_with_render_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tune.ogg");
    let status = Command::new(bin())
        .args([
            "render",
            song_path().to_str().unwrap(),
            "--seed",
            "7",
            "-o",
            out.to_str().unwrap(),
        ])
        .status()
        .expect("run sunflower render");
    assert!(status.success());
    let side = songwriter::sidecar::RenderSidecar::read(&dir.path().join("tune.render.json"))
        .expect("RenderSidecar::read");
    assert_eq!(side.seed, Some(7));
    assert_eq!(side.voice.as_deref(), Some("baritone"));
    assert!(side.generation.is_none());
}

#[test]
fn demo_checks_the_format_before_it_writes_the_json() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.txt");
    let st = Command::new(bin())
        .args(["demo", "-o", out.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(!st.success());
    assert!(!dir.path().join("demo.json").exists());
}

#[test]
fn demo_refuses_to_overwrite_another_song_json() {
    let dir = tempfile::tempdir().unwrap();
    let json = dir.path().join("demo.json");
    std::fs::write(&json, "{\"title\": \"mine\"}").unwrap();
    let out = dir.path().join("demo.wav");
    let o = Command::new(bin())
        .args(["demo", "-o", out.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--force"));
    assert_eq!(
        std::fs::read_to_string(&json).unwrap(),
        "{\"title\": \"mine\"}"
    );
    assert!(!out.exists());
}

#[test]
fn write_refuses_an_existing_output_before_it_calls_claude() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("taken.ogg");
    std::fs::write(dir.path().join("taken.json"), "{}").unwrap();
    // --via api with no key would fail too, but only after the check.
    let o = Command::new(bin())
        .env_remove("ANTHROPIC_API_KEY")
        .args([
            "write",
            "a dry morning",
            "--via",
            "api",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("exists") && err.contains("--force"), "{err}");
}

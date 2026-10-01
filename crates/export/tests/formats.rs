//! Format tests: WAV headers parsed by hand; FLAC STREAMINFO parsed by hand
//! and decoded by `ffmpeg` when it is on PATH; Ogg header and tags; 16-bit
//! dither statistics.

use export::{BitDepth, ExportError, Format, Meta, Tpdf, WavSample};
use sfcore::random::{tag, Rng};
use std::path::{Path, PathBuf};
use std::process::Command;

fn test_signal(n: usize) -> (Vec<f32>, Vec<f32>) {
    let sr = 44100.0f32;
    let l = (0..n)
        .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sr).sin())
        .collect();
    let r = (0..n)
        .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 660.0 * i as f32 / sr).sin())
        .collect();
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
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// RIFF chunks after the 12-byte header: (id, body).
fn chunks(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = vec![];
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let id = [b[i], b[i + 1], b[i + 2], b[i + 3]];
        let len = u32le(b, i + 4) as usize;
        out.push((id, &b[i + 8..(i + 8 + len).min(b.len())]));
        i += 8 + len + len % 2;
    }
    out
}

fn chunk<'a>(b: &'a [u8], id: &[u8; 4]) -> &'a [u8] {
    chunks(b)
        .into_iter()
        .find(|(c, _)| c == id)
        .map(|(_, d)| d)
        .expect("chunk present")
}

#[test]
fn wav_headers_for_all_sample_types() {
    let n = 1001;
    let (l, r) = test_signal(n);
    for (sample, tag, bits) in [
        (WavSample::Pcm16, 1u16, 16u16),
        (WavSample::Pcm24, 1, 24),
        (WavSample::Float32, 3, 32),
    ] {
        let path = tmp_path(&format!("hdr_{bits}.wav"));
        export::write(&path, &l, &r, 44100, &test_meta(), Format::Wav { sample }).unwrap();
        let b = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(u32le(&b, 4) as usize, b.len() - 8, "RIFF size {bits}");
        let ids: Vec<[u8; 4]> = chunks(&b).iter().map(|c| c.0).collect();
        let want: Vec<&[u8; 4]> = if sample == WavSample::Float32 {
            vec![b"fmt ", b"fact", b"LIST", b"data"]
        } else {
            vec![b"fmt ", b"LIST", b"data"]
        };
        assert_eq!(ids.iter().collect::<Vec<_>>(), want, "chunk order {bits}");

        let fmt = chunk(&b, b"fmt ");
        let block_align = 2 * bits / 8;
        assert_eq!(u16le(fmt, 0), tag);
        assert_eq!(u16le(fmt, 2), 2);
        assert_eq!(u32le(fmt, 4), 44100);
        assert_eq!(u32le(fmt, 8), 44100 * block_align as u32);
        assert_eq!(u16le(fmt, 12), block_align);
        assert_eq!(u16le(fmt, 14), bits);
        if sample == WavSample::Float32 {
            assert_eq!(fmt.len(), 18);
            assert_eq!(u32le(chunk(&b, b"fact"), 0) as usize, n);
        } else {
            assert_eq!(fmt.len(), 16);
        }

        let list = chunk(&b, b"LIST");
        assert_eq!(&list[0..4], b"INFO");
        let text = String::from_utf8_lossy(list);
        for s in [
            "INAM",
            "Format Test",
            "IART",
            "Claude",
            "ICMT",
            "ICRD",
            "2026",
            "IGNR",
            "cowboy",
            "ISFT",
            export::VOICE_NOTICE,
        ] {
            assert!(text.contains(s), "LIST/INFO lacks {s}");
        }

        let data = chunk(&b, b"data");
        assert_eq!(data.len(), n * block_align as usize, "data size {bits}");
        // Spot-check sample 100, left channel.
        let x = l[100] as f64;
        let off = 100 * block_align as usize;
        match sample {
            WavSample::Float32 => assert_eq!(
                f32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
                l[100]
            ),
            WavSample::Pcm24 => {
                let v = i32::from_le_bytes([0, data[off], data[off + 1], data[off + 2]]) >> 8;
                assert_eq!(v, (x * 8_388_607.0).round() as i32);
            }
            WavSample::Pcm16 => {
                let v = i16::from_le_bytes([data[off], data[off + 1]]) as f64;
                assert!(
                    (v - x * 32767.0).abs() <= 1.5,
                    "16-bit sample off by more than the dither"
                );
            }
        }
    }
}

#[test]
fn errors_on_bad_input() {
    let meta = Meta::default();
    let wav = Format::Wav {
        sample: WavSample::Pcm16,
    };
    let p = tmp_path("bad.wav");
    assert!(matches!(
        export::write(&p, &[0.0; 2], &[0.0], 44100, &meta, wav),
        Err(ExportError::ChannelLengthMismatch { left: 2, right: 1 })
    ));
    for fmt in [
        wav,
        Format::Ogg { quality: 0.5 },
        Format::Flac {
            bits: BitDepth::Bits24,
        },
    ] {
        assert!(matches!(
            export::write(&p, &[0.0], &[0.0], 0, &meta, fmt),
            Err(ExportError::InvalidSampleRate(0))
        ));
    }
    assert!(matches!(
        Format::from_path(Path::new("x.mp3"), 0.6, false, false),
        Err(ExportError::UnknownExtension(_))
    ));
    assert_eq!(
        Format::from_path(Path::new("x.FLAC"), 0.6, true, false).unwrap(),
        Format::Flac {
            bits: BitDepth::Bits16
        }
    );
    assert_eq!(
        Format::from_path(Path::new("x.wav"), 0.6, false, true).unwrap(),
        Format::Wav {
            sample: WavSample::Float32
        }
    );
    std::fs::remove_file(&p).ok();
}

/// STREAMINFO (RFC 9639 8.2): (min block, max block, min frame, max frame,
/// sample rate, channels, bits, total samples).
fn streaminfo(b: &[u8]) -> (u16, u16, u32, u32, u32, u32, u32, u64) {
    assert_eq!(&b[0..4], b"fLaC");
    assert_eq!(b[4] & 0x7f, 0, "first block is STREAMINFO");
    let s = &b[8..42];
    let be24 = |i: usize| u32::from_be_bytes([0, s[i], s[i + 1], s[i + 2]]);
    let packed = u64::from_be_bytes(s[10..18].try_into().unwrap());
    (
        u16::from_be_bytes([s[0], s[1]]),
        u16::from_be_bytes([s[2], s[3]]),
        be24(4),
        be24(7),
        (packed >> 44) as u32,
        ((packed >> 41) & 7) as u32 + 1,
        ((packed >> 36) & 31) as u32 + 1,
        packed & ((1 << 36) - 1),
    )
}

#[test]
fn flac_streaminfo_length_and_tags() {
    for n in [64, 4096, 44100 * 2 + 77, 4096 * 7 + 10] {
        let (l, r) = test_signal(n);
        let path = tmp_path(&format!("si_{n}.flac"));
        export::write(
            &path,
            &l,
            &r,
            44100,
            &test_meta(),
            Format::Flac {
                bits: BitDepth::Bits24,
            },
        )
        .unwrap();
        let b = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        let (bmin, bmax, fmin, fmax, sr, ch, bits, total) = streaminfo(&b);
        assert_eq!(total, n as u64);
        assert_eq!((sr, ch, bits), (44100, 2, 24));
        assert_eq!(bmin, bmax);
        assert!(bmin >= 64 && bmin as usize <= 4096);
        assert!(fmin > 0 && fmin <= fmax);
        let text = String::from_utf8_lossy(&b[42..600.min(b.len())]);
        assert!(text.contains("TITLE=Format Test") && text.contains("ARTIST=Claude"));
    }
    let p = tmp_path("short.flac");
    assert!(export::write(
        &p,
        &[0.0; 10],
        &[0.0; 10],
        44100,
        &Meta::default(),
        Format::Flac {
            bits: BitDepth::Bits16
        }
    )
    .is_err());
    std::fs::remove_file(&p).ok();
}

/// Decodes `path` with ffmpeg to interleaved s32 (24-bit samples in the top
/// bits for a 24-bit source).
fn ffmpeg_decode_s32(path: &Path) -> Vec<i32> {
    let decoded = path.with_extension("decoded.wav");
    let status = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-f", "wav", "-acodec", "pcm_s32le"])
        .arg(&decoded)
        .status()
        .expect("run ffmpeg");
    assert!(status.success(), "ffmpeg failed to decode");
    let b = std::fs::read(&decoded).unwrap();
    std::fs::remove_file(&decoded).ok();
    chunk(&b, b"data")
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| i32::from_le_bytes(*c))
        .collect()
}

#[test]
fn flac_24bit_round_trip_through_ffmpeg() {
    if !ffmpeg_available() {
        eprintln!("skipping flac_24bit_round_trip_through_ffmpeg: ffmpeg not on PATH");
        return;
    }
    // Noise plus tones, and a length that is not a multiple of 4096.
    let n = 44100 * 3 + 77;
    let (mut l, mut r) = test_signal(n);
    let mut rng = Rng::stream(7, tag("export.test"));
    for (a, b) in l.iter_mut().zip(r.iter_mut()) {
        *a += 0.2 * rng.bipolar() as f32;
        *b += 0.2 * rng.bipolar() as f32;
    }
    let path = tmp_path("rt24.flac");
    export::write(
        &path,
        &l,
        &r,
        44100,
        &test_meta(),
        Format::Flac {
            bits: BitDepth::Bits24,
        },
    )
    .unwrap();
    let s = ffmpeg_decode_s32(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(s.len(), n * 2, "decoded length");
    let mut worst = 0.0f64;
    for k in 0..n {
        for (ch, x) in [(0, l[k]), (1, r[k])] {
            let got = (s[k * 2 + ch] >> 8) as f64;
            worst = worst.max((got - x as f64 * 8_388_607.0).abs());
        }
    }
    assert!(worst <= 1.0, "24-bit round trip error {worst} LSB");
}

/// White noise at 24 bits drives flacenc's fixed predictors past verbatim
/// size; the writer must fall back and stay lossless and bounded.
#[test]
fn flac_24bit_noise_is_bounded_and_lossless() {
    let n = 4096 * 3 + 100;
    let mut l = vec![0f32; n];
    let mut r = vec![0f32; n];
    Rng::stream(3, tag("export.test.l")).fill_bipolar(&mut l);
    Rng::stream(3, tag("export.test.r")).fill_bipolar(&mut r);
    let path = tmp_path("noise24.flac");
    export::write(
        &path,
        &l,
        &r,
        44100,
        &Meta::default(),
        Format::Flac {
            bits: BitDepth::Bits24,
        },
    )
    .unwrap();
    let size = std::fs::metadata(&path).unwrap().len() as usize;
    assert!(
        size <= n * 2 * 3 + n / 4 + 4096,
        "noise FLAC {size} bytes, verbatim {}",
        n * 6
    );
    if ffmpeg_available() {
        let s = ffmpeg_decode_s32(&path);
        assert_eq!(s.len(), n * 2);
        for k in 0..n {
            assert_eq!(
                s[2 * k] >> 8,
                (l[k] as f64 * 8_388_607.0).round() as i32,
                "sample {k}"
            );
            assert_eq!(
                s[2 * k + 1] >> 8,
                (r[k] as f64 * 8_388_607.0).round() as i32,
                "sample {k}"
            );
        }
    }
    std::fs::remove_file(&path).ok();
}

#[test]
fn flac16_equals_wav16_through_ffmpeg() {
    if !ffmpeg_available() {
        eprintln!("skipping flac16_equals_wav16_through_ffmpeg: ffmpeg not on PATH");
        return;
    }
    let n = 44100 + 4097;
    let (l, r) = test_signal(n);
    let pf = tmp_path("eq16.flac");
    let pw = tmp_path("eq16.wav");
    export::write(
        &pf,
        &l,
        &r,
        44100,
        &Meta::default(),
        Format::Flac {
            bits: BitDepth::Bits16,
        },
    )
    .unwrap();
    export::write(
        &pw,
        &l,
        &r,
        44100,
        &Meta::default(),
        Format::Wav {
            sample: WavSample::Pcm16,
        },
    )
    .unwrap();
    let f: Vec<i32> = ffmpeg_decode_s32(&pf).iter().map(|v| v >> 16).collect();
    let wb = std::fs::read(&pw).unwrap();
    let w: Vec<i32> = chunk(&wb, b"data")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c) as i32)
        .collect();
    std::fs::remove_file(&pf).ok();
    std::fs::remove_file(&pw).ok();
    assert_eq!(
        f, w,
        "FLAC and WAV 16-bit samples differ (dither must depend on position only)"
    );
}

/// TPDF dither statistics over a signal spread across many codes: error in
/// LSB has mean ~0 and variance 1/6 (dither) + 1/12 (rounding) = 1/4.
#[test]
fn dither_16bit_statistics() {
    let n = 1_000_000;
    let mut rng = Rng::stream(11, tag("export.test.dither"));
    let mut d = Tpdf::at(0);
    let (mut sum, mut sum2) = (0.0f64, 0.0f64);
    for _ in 0..n {
        let x = (0.5 * rng.bipolar()) as f32;
        let q = export::quantize(x, BitDepth::Bits16, &mut d);
        let e = q as f64 - x as f64 * 32767.0;
        sum += e;
        sum2 += e * e;
    }
    let mean = sum / n as f64;
    let var = sum2 / n as f64 - mean * mean;
    eprintln!("dither: mean {mean:.5} LSB, variance {var:.5} LSB^2 (want 0.25)");
    assert!(mean.abs() < 0.01, "mean error {mean}");
    assert!((var - 0.25).abs() < 0.2 * 0.25, "error variance {var}");
}

/// An Ogg file is a sequence of pages starting "OggS"; the comment header
/// carries "vorbis" and the tags in plain text.
#[test]
fn ogg_has_header_and_tags() {
    let path = tmp_path("tags.ogg");
    let (l, r) = test_signal(44100 * 2);
    export::write(
        &path,
        &l,
        &r,
        44100,
        &test_meta(),
        Format::Ogg { quality: 0.6 },
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(
        bytes.len() > 4096,
        "Ogg file implausibly small: {}",
        bytes.len()
    );
    assert_eq!(&bytes[0..4], b"OggS");
    let text = String::from_utf8_lossy(&bytes);
    for s in [
        "vorbis",
        "TITLE=Format Test",
        "ARTIST=Claude",
        "COMMENT=a liner note for the format test",
        "DATE=2026",
        "GENRE=cowboy",
        "PERFORMER=Synthetic voice: Electric Sunflowers engine. No human singer.",
    ] {
        assert!(text.contains(s), "Ogg lacks {s}");
    }
}

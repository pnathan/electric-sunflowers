//! Playback of a song's audio through rodio: one output device for the
//! app, one player per open song. The audio is the song's file on disk, or
//! a mixer re-mix held in memory.

use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

/// The default output device, opened once.
pub struct Output {
    sink: MixerDeviceSink,
}

impl Output {
    pub fn open() -> Result<Output, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| format!("could not open the audio output: {e}"))?;
        sink.log_on_drop(false);
        Ok(Output { sink })
    }
}

/// Where a track's samples come from.
enum Media {
    File(PathBuf),
    /// A re-mix in memory. Cloning shares the samples.
    Memory(SamplesBuffer),
}

/// One song's audio loaded into a player, paused at the start.
pub struct Track {
    player: Player,
    media: Media,
    /// Length in seconds (from the decoder, else the caller's estimate).
    pub duration: f64,
}

fn decoder(path: &Path) -> Result<Decoder<std::io::BufReader<std::fs::File>>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Decoder::try_from(f).map_err(|e| format!("{}: cannot decode: {e}", path.display()))
}

impl Track {
    /// Loads `path` paused at 0. `fallback` is the length to use when the
    /// decoder does not know it.
    pub fn open(out: &Output, path: &Path, fallback: f64, volume: f32) -> Result<Track, String> {
        let dec = decoder(path)?;
        let duration = dec.total_duration().map(|d| d.as_secs_f64()).filter(|d| *d > 0.0).unwrap_or(fallback);
        let player = Player::connect_new(out.sink.mixer());
        player.pause();
        player.set_volume(volume);
        player.append(dec);
        Ok(Track { player, media: Media::File(path.to_path_buf()), duration })
    }

    /// Loads an in-memory stereo mix (see `stereo_buffer`) paused at 0.
    pub fn from_memory(out: &Output, buf: SamplesBuffer, volume: f32) -> Track {
        let duration = buf.total_duration().map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let player = Player::connect_new(out.sink.mixer());
        player.pause();
        player.set_volume(volume);
        player.append(buf.clone());
        Track { player, media: Media::Memory(buf), duration }
    }

    /// A track for `buf` that takes over from `old`: at `old`'s position,
    /// and playing if `old` was. The caller drops `old` after this returns,
    /// so the new player is running before the old one stops.
    pub fn take_over(out: &Output, old: Option<&Track>, buf: SamplesBuffer, volume: f32) -> Result<Track, String> {
        let (pos, playing) = old.map_or((0.0, false), |t| (t.position(), t.playing()));
        let mut t = Track::from_memory(out, buf, volume);
        if pos > 0.0 {
            t.seek(pos)?;
        }
        if playing {
            t.play()?;
        }
        Ok(t)
    }

    /// True while sound is advancing.
    pub fn playing(&self) -> bool {
        !self.player.is_paused() && !self.player.empty()
    }

    /// Playback position in seconds; the end once the file has run out.
    pub fn position(&self) -> f64 {
        if self.player.empty() {
            self.duration
        } else {
            self.player.get_pos().as_secs_f64().min(self.duration)
        }
    }

    pub fn play(&mut self) -> Result<(), String> {
        if self.player.empty() {
            self.seek(0.0)?;
        }
        self.player.play();
        Ok(())
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn toggle(&mut self) -> Result<(), String> {
        if self.playing() {
            self.pause();
            Ok(())
        } else {
            self.play()
        }
    }

    /// Moves to `t` seconds, reloading the file if it has run out.
    pub fn seek(&mut self, t: f64) -> Result<(), String> {
        if self.player.empty() {
            match &self.media {
                Media::File(p) => self.player.append(decoder(p)?),
                Media::Memory(b) => self.player.append(b.clone()),
            }
        }
        let t = t.clamp(0.0, (self.duration - 0.05).max(0.0));
        self.player.try_seek(Duration::from_secs_f64(t)).map_err(|e| format!("seek failed: {e}"))
    }

    pub fn set_volume(&self, v: f32) {
        self.player.set_volume(v);
    }
}

/// Interleaves `l` and `r` into a stereo rodio buffer at `sr` Hz. The
/// shorter channel sets the length.
pub fn stereo_buffer(l: &[f32], r: &[f32], sr: u32) -> SamplesBuffer {
    let two = NonZero::new(2).expect("2 is not zero");
    let rate = NonZero::new(sr.max(1)).expect("max(1) is not zero");
    SamplesBuffer::new(two, rate, interleave(l, r))
}

/// `[l0, r0, l1, r1, ...]`, as long as the shorter channel.
fn interleave(l: &[f32], r: &[f32]) -> Vec<rodio::Sample> {
    let mut v = Vec::with_capacity(2 * l.len().min(r.len()));
    for (&a, &b) in l.iter().zip(r) {
        v.push(a as rodio::Sample);
        v.push(b as rodio::Sample);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_left_then_right() {
        assert_eq!(interleave(&[1.0, 2.0, 3.0], &[-1.0, -2.0]), vec![1.0, -1.0, 2.0, -2.0]);
    }

    #[test]
    fn memory_buffer_knows_its_length_and_seeks() {
        let n = 48_000;
        let l: Vec<f32> = (0..n).map(|i| i as f32).collect();
        let r: Vec<f32> = l.iter().map(|x| -x).collect();
        let mut b = stereo_buffer(&l, &r, 48_000);
        assert_eq!(b.channels().get(), 2);
        assert_eq!(b.total_duration(), Some(Duration::from_secs(1)));
        b.try_seek(Duration::from_millis(500)).unwrap();
        // The next samples are frame 24000, left then right.
        assert_eq!(b.next(), Some(24_000.0));
        assert_eq!(b.next(), Some(-24_000.0));
    }

    /// Needs an audio output; passes with nothing to check when there is
    /// none (as on a build machine without a sound device).
    #[test]
    fn take_over_keeps_position_and_pause() {
        let Ok(out) = Output::open() else {
            eprintln!("no audio output: take_over not exercised");
            return;
        };
        let silent = |secs: usize| stereo_buffer(&vec![0.0; 48_000 * secs], &vec![0.0; 48_000 * secs], 48_000);
        let mut old = Track::from_memory(&out, silent(10), 0.0);
        old.seek(4.0).unwrap();
        assert!((old.position() - 4.0).abs() < 0.05, "{}", old.position());
        let new = Track::take_over(&out, Some(&old), silent(10), 0.0).unwrap();
        assert!((new.position() - 4.0).abs() < 0.05, "{}", new.position());
        assert!(!new.playing());
        old.play().unwrap();
        let playing = Track::take_over(&out, Some(&old), silent(10), 0.0).unwrap();
        assert!(playing.playing());
        assert!(playing.position() >= 4.0 - 0.05, "{}", playing.position());
    }
}

//! Playback of a song's audio file through rodio: one output device for the
//! app, one player per open song.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

/// The default output device, opened once.
pub struct Output {
    sink: MixerDeviceSink,
}

impl Output {
    pub fn open() -> Result<Output, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|e| format!("could not open the audio output: {e}"))?;
        sink.log_on_drop(false);
        Ok(Output { sink })
    }
}

/// One audio file loaded into a player, paused at the start.
pub struct Track {
    player: Player,
    pub path: PathBuf,
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
        let duration = dec
            .total_duration()
            .map(|d| d.as_secs_f64())
            .filter(|d| *d > 0.0)
            .unwrap_or(fallback);
        let player = Player::connect_new(out.sink.mixer());
        player.pause();
        player.set_volume(volume);
        player.append(dec);
        Ok(Track {
            player,
            path: path.to_path_buf(),
            duration,
        })
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
            self.player.append(decoder(&self.path)?);
        }
        let t = t.clamp(0.0, (self.duration - 0.05).max(0.0));
        self.player
            .try_seek(Duration::from_secs_f64(t))
            .map_err(|e| format!("seek failed: {e}"))
    }

    pub fn set_volume(&self, v: f32) {
        self.player.set_volume(v);
    }
}

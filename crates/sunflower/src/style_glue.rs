//! Glue between `songwriter::styles` (style/band data, independent of the
//! compose crate) and `compose::song::Song`. Ports the song-mutating half of
//! `applyStyle(song,key)` from src/styles.js: song.guitar, song.band (with
//! drums folded in), and the tempo clamp. `song.breakLead` and `song.style`
//! have no field on `compose::song::Song` (see crate docs on `Prepared` /
//! `render_violin_and_harmony_guitar`: `song.breakLead` is always absent in
//! this port already), so there is nothing to set for them here.

use compose::song::{Band as SongBand, Song};
use sfcore::js::clamp;
use songwriter::styles::{apply_style, Band as StyleBand};

/// Applies style `key`'s arrangement to `song` in place. Returns an error
/// string (not a panic) when `key` is not a known style, so the CLI can
/// report it and exit non-zero instead of crashing on bad user input.
pub fn apply_style_to_song(song: &mut Song, key: &str) -> Result<(), String> {
    let arr = apply_style(key, &song.meter_name).ok_or_else(|| format!("unknown style: {key}"))?;
    song.guitar = arr.guitar.to_string();
    song.band = to_song_band(arr.band, arr.drums);
    if let Some((lo, hi)) = arr.tempo_clamp {
        song.tempo = clamp(song.tempo, lo, hi);
    }
    Ok(())
}

fn to_song_band(b: StyleBand, drums: &str) -> SongBand {
    SongBand {
        drums: drums.to_string(),
        bass: b.bass,
        harmony_guitar: b.harmony_guitar,
        harp: b.harp,
        violin: b.violin,
        choir: b.choir,
        harmonies: b.harmonies,
        doubles: b.doubles,
    }
}

/// Maps a `--no TRACK` name (as the page's band toggles spell it: drums,
/// bass, harmonyGuitar, harp, violin, choir, harmonies, doubles) to the
/// matching `dsp::mix::TrackSpec.band` string, so the CLI can validate
/// `--no` arguments against the same vocabulary `TRACKS` uses.
pub const BAND_TRACK_NAMES: [&str; 8] =
    ["drums", "bass", "harmonyGuitar", "harp", "violin", "choir", "harmonies", "doubles"];

/// Reads one band flag off `song.band` by its `TrackSpec.band` name. `drums`
/// is a style string, not a bool; JS's band toggle test is `T.always ||
/// B[T.band]`, and JS truthiness makes any non-empty string true --
/// including the literal `'none'` (normalizeSong always stores one of
/// 'none'/'brushes'/'soft'/'full', never `''`). So the drums *track* is
/// enabled by default regardless of the style; a style of 'none' silences
/// it inside `render_drums` itself, not via this toggle. JS parity.
pub fn song_band_flag(song: &Song, name: &str) -> bool {
    match name {
        "drums" => !song.band.drums.is_empty(),
        "bass" => song.band.bass,
        "harmonyGuitar" => song.band.harmony_guitar,
        "harp" => song.band.harp,
        "violin" => song.band.violin,
        "choir" => song.band.choir,
        "harmonies" => song.band.harmonies,
        "doubles" => song.band.doubles,
        _ => false,
    }
}

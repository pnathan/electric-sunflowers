//! Glue between `songwriter::styles` (style/band data, independent of the
//! compose crate) and `compose::song::Song`. Ports `applyStyle(song,key)`
//! from src/styles.js: song.style, song.guitar, song.breakLead, song.band
//! (with drums folded in), and the rounded tempo clamp.

use compose::song::{Band as SongBand, BreakLead, Song};
use sfcore::js::{clamp, round};
use songwriter::styles::{apply_style, Band as StyleBand};

/// Applies style `key`'s arrangement to `song` in place. Returns an error
/// string (not a panic) when `key` is not a known style, so the CLI can
/// report it and exit non-zero instead of crashing on bad user input.
pub fn apply_style_to_song(song: &mut Song, key: &str) -> Result<(), String> {
    let arr = apply_style(key, &song.meter_name).ok_or_else(|| format!("unknown style: {key}"))?;
    song.style = Some(key.to_string());
    song.guitar = arr.guitar.to_string();
    song.break_lead = Some(BreakLead::from_str(arr.break_lead));
    song.band = to_song_band(arr.band, arr.drums);
    if let Some((lo, hi)) = arr.tempo_clamp {
        // styles.js applyStyle: song.tempo = Math.round(clamp(...)).
        song.tempo = round(clamp(song.tempo, lo, hi));
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

#[cfg(test)]
mod tests {
    use super::*;

    // Reference from node: applyStyle({meter:'3/4',tempo:t},'cowboy') gives 120 -> 114,
    // 60 -> 72, and breakLead 'violin'. The cowboy 3/4 range is [80,104], so the upper
    // bound 104*1.1 = 114.4 is not an integer and Math.round changes it.
    #[test]
    fn style_tempo_clamp_rounds_as_js() {
        let raw = serde_json::json!({
            "meter": "3/4",
            "tempo": 100,
            "sections": [{"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]
        });
        for (t, want) in [(120.0, 114.0), (60.0, 72.0)] {
            let mut song = compose::song::normalize_song(&raw).unwrap();
            song.tempo = t;
            apply_style_to_song(&mut song, "cowboy").unwrap();
            assert_eq!(song.tempo, want, "tempo {t}");
            assert!(song.break_lead.is_some());
        }
    }
}

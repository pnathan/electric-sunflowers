//! Mix settings (design section 3.2): per-track fader, pan, mute and solo,
//! plus the global ducking depth. This is not a writing decision (the owner:
//! "claude's role is to write the song, then the song engine works
//! deterministically from there"), so it lives in a sidecar next to the
//! audio, never in the song JSON.
//!
//! `MixSettings::default_for` reproduces today's mix exactly: 0 dB faders
//! (`db_to_gain(0.0)` is `1.0` exactly, so the default route gain multiplies
//! by exactly `1.0`), the strip pans, nothing muted or soloed, and the
//! shipped ducking depth.

use serde_json::{Map, Value};

use crate::render::Stems;
use crate::track::{TrackId, N_TRACKS};

/// Pan of the two lead tracks in a duet (design section 3.2; wave 2 adds
/// `TrackId::LeadB` and pans the leads here instead of at the strip pan).
/// Unused until then.
pub const DUET_PAN_A: f32 = -0.25;
pub const DUET_PAN_B: f32 = 0.25;

/// Accepted range of a track's fader, dB.
pub const GAIN_DB_MIN: f32 = -120.0;
pub const GAIN_DB_MAX: f32 = 12.0;
/// Accepted range of a track's pan.
pub const PAN_MIN: f32 = -1.0;
pub const PAN_MAX: f32 = 1.0;
/// Accepted range of the ducking depth, dB.
pub const DUCK_DB_MIN: f32 = 0.0;
pub const DUCK_DB_MAX: f32 = 20.0;

/// One track's mix controls: fader relative to the strip gain, absolute
/// pan, mute and solo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackMix {
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

/// The whole mix: one `TrackMix` per track (indexed by `TrackId::index()`)
/// plus the ducking depth.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixSettings {
    pub tracks: [TrackMix; N_TRACKS],
    /// Accompaniment reduction while a lead sings, dB; 0 turns the ducker
    /// off. Defaults to `mix::DUCK_DB`.
    pub duck_db: f32,
}

impl MixSettings {
    /// Strip pans, 0 dB faders, nothing muted or soloed, the shipped
    /// ducking depth. When `stems` has a `TrackId::LeadB` stem (a duet),
    /// the two leads pan to `DUET_PAN_A`/`DUET_PAN_B` instead of their
    /// strip pan (both 0); a solo song (no `LeadB` stem) is unchanged.
    pub fn default_for(stems: &Stems) -> MixSettings {
        let duet = stems.get(TrackId::LeadB).is_some();
        let tracks = std::array::from_fn(|i| {
            let id = TrackId::ALL[i];
            let pan = match id {
                TrackId::Lead if duet => DUET_PAN_A,
                TrackId::LeadB if duet => DUET_PAN_B,
                _ => id.strip().pan,
            };
            TrackMix {
                gain_db: 0.0,
                pan,
                mute: false,
                solo: false,
            }
        });
        MixSettings {
            tracks,
            duck_db: crate::mix::DUCK_DB as f32,
        }
    }

    /// Whether `id` sounds: not muted, and soloed when any track is soloed.
    pub fn audible(&self, id: TrackId) -> bool {
        let t = self.tracks[id.index()];
        let any_solo = self.tracks.iter().any(|t| t.solo);
        !t.mute && (!any_solo || t.solo)
    }

    /// The sidecar JSON: `{"version": 1, "tracks": {...}, "duck_db": ...}`,
    /// writing only the fields that differ from `defaults`.
    pub fn to_json(&self, defaults: &MixSettings) -> Value {
        let mut tracks = Map::new();
        for id in TrackId::ALL {
            let (t, d) = (self.tracks[id.index()], defaults.tracks[id.index()]);
            let mut obj = Map::new();
            if t.gain_db != d.gain_db {
                obj.insert("gain_db".into(), Value::from(t.gain_db));
            }
            if t.pan != d.pan {
                obj.insert("pan".into(), Value::from(t.pan));
            }
            if t.mute != d.mute {
                obj.insert("mute".into(), Value::from(t.mute));
            }
            if t.solo != d.solo {
                obj.insert("solo".into(), Value::from(t.solo));
            }
            if !obj.is_empty() {
                tracks.insert(id.name().into(), Value::Object(obj));
            }
        }
        let mut out = Map::new();
        out.insert("version".into(), Value::from(1));
        if !tracks.is_empty() {
            out.insert("tracks".into(), Value::Object(tracks));
        }
        if self.duck_db != defaults.duck_db {
            out.insert("duck_db".into(), Value::from(self.duck_db));
        }
        Value::Object(out)
    }

    /// Reads a sidecar value against `defaults`: unknown track names,
    /// unknown fields, and fields of the wrong type are a warning and the
    /// default is kept; `gain_db`, `pan` and `duck_db` are clamped to their
    /// accepted range.
    pub fn from_json(v: &Value, defaults: &MixSettings) -> (MixSettings, Vec<String>) {
        let mut out = *defaults;
        let mut warn = Vec::new();
        let Some(obj) = v.as_object() else {
            warn.push("mix settings: not a JSON object, using defaults".to_string());
            return (out, warn);
        };
        for (key, val) in obj {
            match key.as_str() {
                "version" => {}
                "tracks" => read_tracks(val, &mut out, &mut warn),
                "duck_db" => match val.as_f64() {
                    Some(d) => out.duck_db = (d as f32).clamp(DUCK_DB_MIN, DUCK_DB_MAX),
                    None => warn.push("duck_db: not a number, using default".to_string()),
                },
                other => warn.push(format!("{other}: unknown field, ignored")),
            }
        }
        (out, warn)
    }
}

fn read_tracks(val: &Value, out: &mut MixSettings, warn: &mut Vec<String>) {
    let Some(map) = val.as_object() else {
        warn.push("tracks: not an object, ignored".to_string());
        return;
    };
    for (name, tv) in map {
        let Some(id) = TrackId::ALL.into_iter().find(|id| id.name() == name) else {
            warn.push(format!("{name}: unknown track, ignored"));
            continue;
        };
        let Some(tobj) = tv.as_object() else {
            warn.push(format!("{name}: not an object, ignored"));
            continue;
        };
        let t = &mut out.tracks[id.index()];
        for (field, fv) in tobj {
            match field.as_str() {
                "gain_db" => match fv.as_f64() {
                    Some(g) => t.gain_db = (g as f32).clamp(GAIN_DB_MIN, GAIN_DB_MAX),
                    None => warn.push(format!("{name}.gain_db: not a number, using default")),
                },
                "pan" => match fv.as_f64() {
                    Some(p) => t.pan = (p as f32).clamp(PAN_MIN, PAN_MAX),
                    None => warn.push(format!("{name}.pan: not a number, using default")),
                },
                "mute" => match fv.as_bool() {
                    Some(b) => t.mute = b,
                    None => warn.push(format!("{name}.mute: not a bool, using default")),
                },
                "solo" => match fv.as_bool() {
                    Some(b) => t.solo = b,
                    None => warn.push(format!("{name}.solo: not a bool, using default")),
                },
                other => warn.push(format!("{name}.{other}: unknown field, ignored")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_stems() -> Stems {
        Stems {
            len: 0,
            tracks: Default::default(),
        }
    }

    #[test]
    fn default_matches_strip_table() {
        let d = MixSettings::default_for(&stub_stems());
        for id in TrackId::ALL {
            let t = d.tracks[id.index()];
            assert_eq!(t.gain_db, 0.0);
            assert_eq!(t.pan, id.strip().pan);
            assert!(!t.mute && !t.solo);
        }
        assert_eq!(d.duck_db, crate::mix::DUCK_DB as f32);
    }

    #[test]
    fn solo_makes_only_soloed_tracks_audible() {
        let mut s = MixSettings::default_for(&stub_stems());
        s.tracks[TrackId::Bass.index()].solo = true;
        assert!(s.audible(TrackId::Bass));
        assert!(!s.audible(TrackId::Lead));
        assert!(!s.audible(TrackId::Drums));
    }

    #[test]
    fn mute_wins_over_solo() {
        let mut s = MixSettings::default_for(&stub_stems());
        s.tracks[TrackId::Bass.index()].solo = true;
        s.tracks[TrackId::Bass.index()].mute = true;
        assert!(!s.audible(TrackId::Bass));
    }

    #[test]
    fn json_round_trip_writes_only_differences() {
        let d = MixSettings::default_for(&stub_stems());
        let mut s = d;
        s.tracks[TrackId::Violin.index()].gain_db = -3.0;
        s.tracks[TrackId::Harp.index()].mute = true;
        s.tracks[TrackId::Lead.index()].pan = -0.1;
        s.duck_db = 4.0;
        let j = s.to_json(&d);
        assert_eq!(j["version"], 1);
        assert_eq!(j["tracks"]["violin"]["gain_db"], -3.0);
        assert_eq!(j["tracks"]["harp"]["mute"], true);
        assert_eq!(j["tracks"]["lead"]["pan"].as_f64().unwrap() as f32, -0.1f32);
        assert_eq!(j["duck_db"], 4.0);
        assert!(j["tracks"].get("bass").is_none());
        let (back, warn) = MixSettings::from_json(&j, &d);
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(back, s);
    }

    #[test]
    fn from_json_is_lenient() {
        let d = MixSettings::default_for(&stub_stems());
        let v = serde_json::json!({
            "version": 1,
            "tracks": {
                "violin": {"gain_db": "loud", "pan": 2.0},
                "bogus": {"mute": true},
                "bass": {"weird_field": 1},
            },
            "duck_db": 500.0,
            "unknown_top": true,
        });
        let (out, warn) = MixSettings::from_json(&v, &d);
        assert_eq!(
            out.tracks[TrackId::Violin.index()].gain_db,
            d.tracks[TrackId::Violin.index()].gain_db
        );
        assert_eq!(out.tracks[TrackId::Violin.index()].pan, PAN_MAX);
        assert_eq!(out.duck_db, DUCK_DB_MAX);
        assert!(warn.iter().any(|w| w.contains("violin.gain_db")));
        assert!(warn.iter().any(|w| w.contains("bogus")));
        assert!(warn.iter().any(|w| w.contains("bass.weird_field")));
        assert!(warn.iter().any(|w| w.contains("unknown_top")));
    }
}

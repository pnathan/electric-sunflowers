//! The ten tracks and their channel strips (design sections 3.6 and 5.13).
//!
//! `STRIPS[id as usize]` holds each track's mix settings: strip gain, pan
//! (-1 left .. +1 right), reverb send, the band part that switches it,
//! the EQ cascade (RBJ cookbook bands, `dsp::biquad`), the instrument body
//! it is played through, and the lead's compressor and slapback. The
//! values are the shipped mix; they set the balance of the whole song.

use dsp::biquad::{EqBand, EqKind};
use instruments::body::Body;
use song::{Band, DrumKit};

/// A track of the mix, in mix order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TrackId {
    Lead,
    /// Singer B of a duet; present only when the song is a duet.
    LeadB,
    Doubles,
    Harmony,
    Choir,
    Guitar,
    HarmonyGuitar,
    Bass,
    Drums,
    Harp,
    Violin,
}

/// Number of tracks.
pub const N_TRACKS: usize = 11;

impl TrackId {
    /// Every track in mix order (`id as usize` is the position).
    pub const ALL: [TrackId; N_TRACKS] = [
        TrackId::Lead,
        TrackId::LeadB,
        TrackId::Doubles,
        TrackId::Harmony,
        TrackId::Choir,
        TrackId::Guitar,
        TrackId::HarmonyGuitar,
        TrackId::Bass,
        TrackId::Drums,
        TrackId::Harp,
        TrackId::Violin,
    ];

    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn strip(self) -> &'static Strip {
        &STRIPS[self as usize]
    }

    /// File-name form ("harmony_guitar").
    pub const fn name(self) -> &'static str {
        match self {
            TrackId::Lead => "lead",
            TrackId::LeadB => "lead_b",
            TrackId::Doubles => "doubles",
            TrackId::Harmony => "harmony",
            TrackId::Choir => "choir",
            TrackId::Guitar => "guitar",
            TrackId::HarmonyGuitar => "harmony_guitar",
            TrackId::Bass => "bass",
            TrackId::Drums => "drums",
            TrackId::Harp => "harp",
            TrackId::Violin => "violin",
        }
    }

    /// Whether the track plays with `band`. Tracks with no band part (lead,
    /// guitar) always play.
    pub fn plays(self, band: &Band) -> bool {
        self.strip().band.is_none_or(|p| p.on(band))
    }
}

/// A switchable part of `song::Band`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BandPart {
    Doubles,
    Harmonies,
    Choir,
    HarmonyGuitar,
    Bass,
    Drums,
    Harp,
    Violin,
}

impl BandPart {
    pub const ALL: [BandPart; 8] = [
        BandPart::Drums,
        BandPart::Bass,
        BandPart::HarmonyGuitar,
        BandPart::Harp,
        BandPart::Violin,
        BandPart::Choir,
        BandPart::Harmonies,
        BandPart::Doubles,
    ];

    /// The name in the song JSON's `band` object.
    pub const fn name(self) -> &'static str {
        match self {
            BandPart::Doubles => "doubles",
            BandPart::Harmonies => "harmonies",
            BandPart::Choir => "choir",
            BandPart::HarmonyGuitar => "harmonyGuitar",
            BandPart::Bass => "bass",
            BandPart::Drums => "drums",
            BandPart::Harp => "harp",
            BandPart::Violin => "violin",
        }
    }

    /// Parses `name` (exact, as `name` spells it).
    pub fn from_name(name: &str) -> Option<BandPart> {
        BandPart::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Whether `band` turns the part on. Drums play unless the kit is
    /// `DrumKit::None`.
    pub fn on(self, band: &Band) -> bool {
        match self {
            BandPart::Doubles => band.doubles,
            BandPart::Harmonies => band.harmonies,
            BandPart::Choir => band.choir,
            BandPart::HarmonyGuitar => band.harmony_guitar,
            BandPart::Bass => band.bass,
            BandPart::Drums => band.drums != DrumKit::None,
            BandPart::Harp => band.harp,
            BandPart::Violin => band.violin,
        }
    }

    /// Turns the part off in `band` (drums: kit `None`).
    pub fn switch_off(self, band: &mut Band) {
        match self {
            BandPart::Doubles => band.doubles = false,
            BandPart::Harmonies => band.harmonies = false,
            BandPart::Choir => band.choir = false,
            BandPart::HarmonyGuitar => band.harmony_guitar = false,
            BandPart::Bass => band.bass = false,
            BandPart::Drums => band.drums = DrumKit::None,
            BandPart::Harp => band.harp = false,
            BandPart::Violin => band.violin = false,
        }
    }
}

/// A body a track is convolved with, and the offset added to the song seed
/// for its impulse response. The level trim is `Body::spec().trim`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyMount {
    pub body: Body,
    pub seed_offset: u64,
}

/// Feed-forward compressor settings (`dsp::dynamics`). The threshold is
/// `above_target_db` over the track's target level `TARGET_RMS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompSpec {
    pub ratio: f64,
    pub attack: f64,
    pub release: f64,
    pub knee_db: f64,
    pub above_target_db: f64,
}

/// Feedback comb echo: `y[n] = LP(x[n - d] + feedback y[n - d])`, output
/// times `level`, added to the main and send buses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slapback {
    pub delay_s: f64,
    pub feedback: f64,
    pub lp_hz: f64,
    pub lp_q: f64,
    pub level: f32,
}

/// One channel strip.
#[derive(Clone, Copy, Debug)]
pub struct Strip {
    pub label: &'static str,
    pub gain: f32,
    pub pan: f32,
    pub send: f32,
    /// `None`: always on.
    pub band: Option<BandPart>,
    pub eq: &'static [EqBand],
    pub body: Option<BodyMount>,
    pub comp: Option<CompSpec>,
    pub slapback: Option<Slapback>,
}

/// Presence cut on the accompaniment that shares the voice's intelligibility
/// band (guitars, violin, choir): a peaking dip centred where consonant and
/// upper-formant energy carries the words.
const PRESENCE_HZ: f64 = 2500.0;
const PRESENCE_CUT_DB: f64 = -3.0;

/// Gated RMS every track is brought to before its strip gain.
pub const TARGET_RMS: f64 = 0.1;

const fn hp(f: f64) -> EqBand {
    EqBand { kind: EqKind::Highpass, f, q: 0.7, db: 0.0 }
}
const fn lp(f: f64) -> EqBand {
    EqBand { kind: EqKind::Lowpass, f, q: 0.7, db: 0.0 }
}
const fn pk(f: f64, q: f64, db: f64) -> EqBand {
    EqBand { kind: EqKind::Peaking, f, q, db }
}
const fn hs(f: f64, db: f64) -> EqBand {
    EqBand { kind: EqKind::HighShelf, f, q: 0.7, db }
}

/// Lead and harmony compressor: 3:1, 8/150 ms, 6 dB knee, 1 dB over target.
pub const VOCAL_COMP: CompSpec = CompSpec { ratio: 3.0, attack: 0.008, release: 0.15, knee_db: 6.0, above_target_db: 1.0 };

/// Lead slapback: 340 ms, feedback 0.22 through a 3.2 kHz low-pass, level 0.07.
pub const LEAD_SLAP: Slapback = Slapback { delay_s: 0.34, feedback: 0.22, lp_hz: 3200.0, lp_q: 0.7, level: 0.07 };

/// The strips, indexed by `TrackId as usize`.
pub const STRIPS: [Strip; N_TRACKS] = [
    Strip {
        label: "Lead vocal",
        gain: 1.25,
        pan: 0.0,
        send: 0.2,
        band: None,
        eq: &[hp(90.0), pk(250.0, 1.0, -1.5), pk(2900.0, 1.0, 1.5)],
        body: None,
        comp: Some(VOCAL_COMP),
        slapback: Some(LEAD_SLAP),
    },
    Strip {
        label: "Lead vocal B",
        gain: 1.25,
        pan: 0.0,
        send: 0.2,
        band: None,
        eq: &[hp(90.0), pk(250.0, 1.0, -1.5), pk(2900.0, 1.0, 1.5)],
        body: None,
        comp: Some(VOCAL_COMP),
        slapback: Some(LEAD_SLAP),
    },
    Strip {
        label: "Melody doubles",
        gain: 0.3,
        pan: 0.0,
        send: 0.34,
        band: Some(BandPart::Doubles),
        eq: &[hp(140.0), hs(6000.0, -6.0)],
        body: None,
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Harmony vocal",
        gain: 0.4,
        pan: 0.28,
        send: 0.32,
        band: Some(BandPart::Harmonies),
        eq: &[hp(130.0), hs(6500.0, -5.0)],
        body: None,
        comp: Some(VOCAL_COMP),
        slapback: None,
    },
    Strip {
        label: "Backing choir",
        gain: 0.36,
        pan: 0.0,
        send: 0.5,
        band: Some(BandPart::Choir),
        eq: &[hp(120.0), pk(PRESENCE_HZ, 1.0, PRESENCE_CUT_DB), lp(6500.0)],
        body: None,
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Guitar",
        gain: 0.5,
        pan: -0.2,
        send: 0.16,
        band: None,
        eq: &[hp(70.0), pk(115.0, 0.9, 3.0), pk(PRESENCE_HZ, 1.0, PRESENCE_CUT_DB), hs(9500.0, -2.0)],
        body: Some(BodyMount { body: Body::Guitar, seed_offset: 0 }),
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Harmony guitar",
        gain: 0.36,
        pan: 0.45,
        send: 0.28,
        band: Some(BandPart::HarmonyGuitar),
        eq: &[hp(120.0), pk(PRESENCE_HZ, 1.0, PRESENCE_CUT_DB), hs(9000.0, -2.0)],
        body: Some(BodyMount { body: Body::Guitar, seed_offset: 17 }),
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Bass",
        gain: 0.5,
        pan: 0.0,
        send: 0.04,
        band: Some(BandPart::Bass),
        eq: &[lp(2000.0), pk(85.0, 1.0, 2.0)],
        body: None,
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Drums",
        gain: 0.42,
        pan: 0.0,
        send: 0.14,
        band: Some(BandPart::Drums),
        eq: &[hp(30.0)],
        body: None,
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Harp",
        gain: 0.36,
        pan: 0.34,
        send: 0.4,
        band: Some(BandPart::Harp),
        eq: &[hp(75.0), pk(180.0, 0.8, 1.5)],
        body: Some(BodyMount { body: Body::Harp, seed_offset: 0 }),
        comp: None,
        slapback: None,
    },
    Strip {
        label: "Violin",
        gain: 0.34,
        pan: -0.4,
        send: 0.42,
        band: Some(BandPart::Violin),
        eq: &[hp(190.0), hs(2800.0, 7.0), pk(PRESENCE_HZ, 1.0, PRESENCE_CUT_DB), hs(7000.0, 5.0)],
        body: Some(BodyMount { body: Body::Violin, seed_offset: 0 }),
        comp: None,
        slapback: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_index_the_strips() {
        for (i, id) in TrackId::ALL.into_iter().enumerate() {
            assert_eq!(id.index(), i);
        }
    }

    #[test]
    fn band_names_round_trip() {
        for p in BandPart::ALL {
            assert_eq!(BandPart::from_name(p.name()), Some(p));
            let mut b = Band { harp: true, ..Band::default() };
            assert!(p.on(&b));
            p.switch_off(&mut b);
            assert!(!p.on(&b));
        }
        assert_eq!(BandPart::from_name("bogus"), None);
    }

    #[test]
    fn body_trims_are_the_shipped_levels() {
        assert_eq!(Body::Guitar.spec().trim, 5.4);
        assert_eq!(Body::Harp.spec().trim, 7.3);
        assert_eq!(Body::Violin.spec().trim, 2.4);
    }
}

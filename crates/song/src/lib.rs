//! Song vocabulary shared by every engine crate: the typed song model, pitch classes,
//! chords, phonemes and G2P, the loose-JSON boundary, the reply schema and note events.
//!
//! Data flow: model JSON -> `wire::WireSong` (serde, lenient) -> `wire::normalize`
//! -> `model::Song` (typed, every value in range) plus a list of `wire::Repair`.
//! Nothing after `normalize` parses strings except lyric text and titles.

use std::fmt;

/// Error from `FromStr` on a closed vocabulary enum (Mode, Voice, ...).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownName {
    /// The enum's name, for messages ("mode", "voice", ...).
    pub kind: &'static str,
    pub text: String,
}

impl fmt::Display for UnknownName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown {} {:?}", self.kind, self.text)
    }
}

impl std::error::Error for UnknownName {}

/// Declares a fieldless enum whose values have fixed JSON spellings, with
/// `ALL`, `as_str`, `FromStr` (ASCII case-insensitive, surrounding whitespace
/// ignored), `Display` and `Serialize` (as the spelling). The schema's enum
/// lists are generated from `ALL`, so parser and schema cannot disagree.
macro_rules! named_enum {
    ($(#[$m:meta])* $vis:vis enum $name:ident ($kind:literal) { $($(#[$vm:meta])* $var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        $vis enum $name { $($(#[$vm])* $var),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];
            /// Every JSON spelling, in declaration order.
            pub const NAMES: &'static [&'static str] = &[$($s),+];

            /// The JSON spelling.
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::UnknownName;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let t = s.trim();
                $(if t.eq_ignore_ascii_case($s) { return Ok($name::$var); })+
                Err($crate::UnknownName { kind: $kind, text: s.to_string() })
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
    };
}

pub mod chord;
pub mod events;
pub mod g2p;
pub mod model;
pub mod phoneme;
pub mod pitch;
pub mod schema;
pub mod tune;
pub mod wire;

pub use chord::{Chord, ChordError, ChordId, ChordTable};
pub use model::{
    Band, BarChords, Blend, BreakLead, ChoirVoicing, Delivery, DrumKit, Duet, Endings,
    GuitarPattern, KeyChange, Line, Meter, MeterGrid, Mode, Part, Phrasing, Rubato, Section,
    SectionBody, SectionKind, SectionRole, SingerId, Song, Syllable, VocalRange, Voice,
    MELISMA_MAX_NOTES, SCHEMA_LATEST, SCHEMA_V1, SCHEMA_V2, SCHEMA_V3,
};
pub use phoneme::Phoneme;
pub use pitch::{Pc, PcSet};
pub use tune::{parse_tune, tune_text, TuneNote, TunePitch};
pub use wire::{normalize, normalize_value, Repair, SongError, WireSong};

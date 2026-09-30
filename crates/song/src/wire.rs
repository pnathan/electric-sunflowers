//! The model-JSON boundary: lenient serde structs for the model's reply
//! (`WireSong`) and `normalize`, which turns one into a typed `Song` plus the
//! list of every repair made on the way. Nothing is dropped or defaulted
//! without a `Repair`.
//!
//! Rules (section indices in repairs are positions in the input `sections`
//! array; line and bar indices are positions in the input too):
//! - title <= 120 chars ("Untitled" if absent), note <= 400 chars.
//! - key: note name at the start, default C. mode: one of the four modes,
//!   case-insensitive; otherwise read from the key's suffix ("m", "min",
//!   "minor", "maj", "major", or a mode name), else major.
//! - meter 4/4, 3/4 or 6/8 (default 4/4). tempo: number or numeric string,
//!   rounded half away from zero (`f64::round`), default 88, clamped to `Meter::tempo_range`.
//! - guitar default fingerpick, voice default baritone, band parts default
//!   on except the harp, drums default brushes.
//! - Section type: case and non-letters ignored; "pre" is prechorus,
//!   "refrain" is chorus; other text starting "chor"/"refrain" is chorus;
//!   anything else is verse.
//! - `same: true` copies the latest earlier sung section of the same type.
//! - A line: `syl` (or `lyric`, `text`) split into words at whitespace and
//!   syllables at '-'; '*' marks stress; a part with no letter or digit
//!   ("...", "*", a lone "-") is not a syllable and is recorded as a
//!   `DroppedSyllable` repair (empty parts between hyphens are not). With no '*' in the line, the first syllable of each
//!   multi-syllable word and every single-syllable word that is not a
//!   function word are stressed. `ph`: one '|'-separated ARPAbet group per
//!   syllable; a group that is missing, has an unknown token or has no
//!   vowel is replaced by G2P for that syllable; when the group count does
//!   not match the syllables, the whole line uses G2P.
//! - Chords: an array, or a string split at ',' and '|'. Each entry is one
//!   bar; whitespace separates up to 2 chords in the bar. A line has 1-4
//!   bars, an instrumental section 1-8. An unparseable chord is removed from
//!   its bar; a bar left empty keeps the previous chord (the tonic triad at
//!   the start of the song), and so does a line with no chords.
//! - A section with no usable line is instrumental if it has chords (a verse
//!   or chorus becomes an interlude), else it is dropped.
//! - Hard errors: not a JSON object; no sung section; no parseable chord.
//! - `phrasing` (top level and `duet.phrasing`): absent is `None`, silently
//!   (a style, or `Phrasing::default`, applies later); present but not an
//!   object is `None` too, with both fields reported defaulted; an object
//!   fills each field, defaulting an absent or unknown one.
//! - `duet`: absent is solo, silently. Present but not an object, or with no
//!   usable `voice`, is solo, reported. A line's `sing` (`A`, `B` or `both`,
//!   case-insensitive; a section's is the default of its lines) picks its
//!   `Part`; `B`/`both` without a duet reads as `A`. `lead`/`blend` on a
//!   shared line pick the melody singer and the blend; on a line that is
//!   not shared they are ignored. A duet where singer B never sings is
//!   read as solo.

use crate::chord::transpose_symbol;
use crate::chord::{parse_detail, Chord, ChordId, ChordTable};
use crate::g2p::g2p;
use crate::model::{
    Band, BarChords, Blend, BreakLead, ChoirVoicing, Delivery, Duet, Endings, GuitarPattern,
    KeyChange, Line, Meter, Mode, Part, Phrasing, Rubato, Section, SectionBody, SectionKind,
    SectionRole, SingerId, Song, Syllable, Voice, MELISMA_MAX_NOTES, SCHEMA_LATEST, SCHEMA_V1,
    SCHEMA_V2,
};
use crate::phoneme::Phoneme;
use crate::pitch::Pc;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;

pub const TITLE_MAX_CHARS: usize = 120;
pub const NOTE_MAX_CHARS: usize = 400;
pub const LINE_MAX_BARS: usize = 4;
pub const INSTRUMENTAL_MAX_BARS: usize = 8;
pub const BAR_MAX_CHORDS: usize = 2;
pub const DEFAULT_TEMPO: f64 = 88.0;

// ---------------------------------------------------------------- wire structs

/// The model's song reply as parsed, before validation. Every field accepts
/// any JSON value; a value of the wrong type reads as absent.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WireSong {
    /// The schema version the document declares; absent is version 1 unless
    /// a version-2 field is used (see `resolve_version`).
    #[serde(deserialize_with = "loose_num")]
    pub schema_version: Option<f64>,
    #[serde(deserialize_with = "loose_str")]
    pub title: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub note: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub key: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub mode: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub meter: Option<String>,
    #[serde(deserialize_with = "loose_num")]
    pub tempo: Option<f64>,
    #[serde(deserialize_with = "loose_str")]
    pub guitar: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub voice: Option<String>,
    #[serde(deserialize_with = "loose_obj")]
    pub band: Option<WireBand>,
    /// Outer `None`: the key is absent (solo, silently). `Some(None)`: present
    /// but not an object.
    #[serde(deserialize_with = "loose_obj_seen")]
    pub phrasing: Option<Option<WirePhrasing>>,
    /// Outer `None`: the key is absent (solo, silently). `Some(None)`:
    /// present but not an object.
    #[serde(deserialize_with = "loose_obj_seen")]
    pub duet: Option<Option<WireDuet>>,
    /// Schema 2: the song's rubato.
    #[serde(deserialize_with = "loose_str")]
    pub rubato: Option<String>,
    /// `None` entries were not JSON objects.
    #[serde(deserialize_with = "loose_objs")]
    pub sections: Vec<Option<WireSection>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WirePhrasing {
    #[serde(deserialize_with = "loose_str")]
    pub delivery: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub endings: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WireDuet {
    #[serde(deserialize_with = "loose_str")]
    pub voice: Option<String>,
    /// Same presence-sensitive shape as `WireSong::phrasing`.
    #[serde(deserialize_with = "loose_obj_seen")]
    pub phrasing: Option<Option<WirePhrasing>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WireBand {
    #[serde(deserialize_with = "loose_str")]
    pub drums: Option<String>,
    #[serde(deserialize_with = "loose_bool")]
    pub bass: Option<bool>,
    #[serde(
        rename = "harmonyGuitar",
        alias = "harmony_guitar",
        deserialize_with = "loose_bool"
    )]
    pub harmony_guitar: Option<bool>,
    #[serde(deserialize_with = "loose_bool")]
    pub harp: Option<bool>,
    #[serde(deserialize_with = "loose_bool")]
    pub violin: Option<bool>,
    #[serde(deserialize_with = "loose_bool")]
    pub choir: Option<bool>,
    #[serde(deserialize_with = "loose_bool")]
    pub harmonies: Option<bool>,
    #[serde(deserialize_with = "loose_bool")]
    pub doubles: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WireSection {
    #[serde(rename = "type", deserialize_with = "loose_str")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "loose_bool")]
    pub same: Option<bool>,
    /// Default part of this section's lines (`A`, `B` or `both`).
    #[serde(deserialize_with = "loose_str")]
    pub sing: Option<String>,
    /// Default melody singer of a shared line in this section.
    #[serde(deserialize_with = "loose_str")]
    pub lead: Option<String>,
    /// Default blend of a shared line in this section.
    #[serde(deserialize_with = "loose_str")]
    pub blend: Option<String>,
    /// Schema 2: default choir voicing of this section's choir lines.
    #[serde(deserialize_with = "loose_str")]
    pub voicing: Option<String>,
    /// Schema 2: the key (and optionally mode) from this section on.
    #[serde(deserialize_with = "loose_str")]
    pub key: Option<String>,
    /// Schema 2: this section's rubato.
    #[serde(deserialize_with = "loose_str")]
    pub rubato: Option<String>,
    /// `None` entries were not JSON objects.
    #[serde(deserialize_with = "loose_objs")]
    pub lines: Vec<Option<WireLine>>,
    #[serde(deserialize_with = "loose_chords")]
    pub chords: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct WireLine {
    #[serde(deserialize_with = "loose_str")]
    pub syl: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub lyric: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub text: Option<String>,
    #[serde(deserialize_with = "loose_str")]
    pub ph: Option<String>,
    #[serde(deserialize_with = "loose_chords")]
    pub chords: Option<Vec<String>>,
    /// This line's own part; absent means the section default.
    #[serde(deserialize_with = "loose_str")]
    pub sing: Option<String>,
    /// This line's own melody singer; presence matters even when unusable,
    /// for `IgnoredPartField` on a line that turns out not to be shared.
    #[serde(deserialize_with = "loose_str_seen")]
    pub lead: Option<Option<String>>,
    /// This line's own blend; same presence rule as `lead`.
    #[serde(deserialize_with = "loose_str_seen")]
    pub blend: Option<Option<String>>,
    /// Schema 2: this choir line's voicing.
    #[serde(deserialize_with = "loose_str")]
    pub voicing: Option<String>,
}

/// String, or a number or boolean written as text; anything else is absent.
fn loose_str<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => Some(s),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    })
}

/// Finite number, or a string holding one.
fn loose_num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|x| x.is_finite()))
}

/// Boolean; also 0/1 and the strings true/false/yes/no.
fn loose_bool<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Bool(b) => Some(b),
        Value::Number(n) => n.as_f64().map(|x| x != 0.0),
        Value::String(s) => {
            let t = s.trim();
            if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes") {
                Some(true)
            } else if t.eq_ignore_ascii_case("false") || t.eq_ignore_ascii_case("no") {
                Some(false)
            } else {
                None
            }
        }
        _ => None,
    })
}

/// Chords as an array of strings, or one string split at ',' and '|'.
/// Entries are trimmed; empty entries are removed.
fn loose_chords<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<String>>, D::Error> {
    let v: Vec<String> = match Value::deserialize(d)? {
        Value::Array(a) => a
            .into_iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s),
                Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .collect(),
        Value::String(s) => s.split([',', '|']).map(str::to_string).collect(),
        _ => return Ok(None),
    };
    Ok(Some(
        v.into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    ))
}

fn loose_obj<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(if v.is_object() {
        T::deserialize(v).ok()
    } else {
        None
    })
}

/// Wraps `loose_str` so absence of the JSON key can be told apart from a
/// present-but-unusable value: serde only calls a field's
/// `deserialize_with` when the key is in the input (a missing key uses
/// `Default::default()` from the struct's `#[serde(default)]`), so wrapping
/// the result in an extra `Some` here means outer `None` is exactly "key
/// absent" and outer `Some` is exactly "key present" (inner `None` when its
/// value was not a usable string, number or boolean).
fn loose_str_seen<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Ok(Some(loose_str(d)?))
}

/// Same presence-tracking wrapper as `loose_str_seen`, for `loose_obj`.
fn loose_obj_seen<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(loose_obj(d)?))
}

fn loose_objs<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a>>(
    d: D,
) -> Result<Vec<Option<T>>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(a) => a
            .into_iter()
            .map(|v| {
                if v.is_object() {
                    T::deserialize(v).ok()
                } else {
                    None
                }
            })
            .collect(),
        _ => Vec::new(),
    })
}

// ---------------------------------------------------------------- errors, repairs

/// A reply that cannot become a song.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongError {
    /// Not parseable JSON.
    NotJson(String),
    /// JSON, but not an object.
    NotObject,
    /// No section with a usable lyric line.
    NoLyrics,
    /// No chord in the reply parses.
    NoChords,
    /// More than 65536 distinct chords.
    TooManyChords,
    /// `schema_version` newer than this build reads (`SCHEMA_LATEST`).
    UnsupportedSchema(i64),
}

impl fmt::Display for SongError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SongError::NotJson(e) => write!(f, "song reply is not JSON: {e}"),
            SongError::NotObject => f.write_str("song reply is not a JSON object"),
            SongError::NoLyrics => f.write_str("song has no section with a lyric line"),
            SongError::NoChords => f.write_str("song has no chord that parses"),
            SongError::TooManyChords => f.write_str("song has more than 65536 distinct chords"),
            SongError::UnsupportedSchema(v) => write!(
                f,
                "song schema_version {v} is newer than this build reads ({SCHEMA_LATEST})"
            ),
        }
    }
}

impl std::error::Error for SongError {}

/// A soft correction made by `normalize`. Indices refer to the input JSON.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "repair", rename_all = "snake_case")]
pub enum Repair {
    /// Field absent or unusable; the default was used.
    DefaultedField {
        field: &'static str,
    },
    /// Text cut to `chars` characters.
    TruncatedField {
        field: &'static str,
        chars: usize,
    },
    ClampedTempo {
        from: f64,
        to: f64,
    },
    /// Mode absent or unknown; read from the key's suffix.
    ModeFromKey {
        mode: Mode,
    },
    /// Section type absent, unknown, or changed (an instrumental verse or
    /// chorus becomes an interlude).
    SectionType {
        section: usize,
        text: String,
        kind: SectionKind,
    },
    /// Section not an object, or with neither lines nor chords.
    DroppedSection {
        section: usize,
    },
    /// A lyric part with no letter or digit ("...", "*", a word of only
    /// hyphens); not sung. `text` is the part, or the whole word when the
    /// word is only hyphens.
    DroppedSyllable {
        section: usize,
        line: usize,
        text: String,
    },
    /// Line not an object, or with no syllable.
    DroppedLine {
        section: usize,
        line: usize,
    },
    /// A chord beyond the bar, line or section limit. `line` is `None` in
    /// an instrumental section.
    DroppedChord {
        section: usize,
        line: Option<usize>,
        bar: usize,
        symbol: String,
    },
    /// Chord that does not parse; removed from its bar.
    UnknownChord {
        section: usize,
        symbol: String,
    },
    /// Chord quality text not understood and ignored ("#9" in "C7#9").
    ChordSuffixIgnored {
        symbol: String,
        ignored: String,
    },
    /// A line (or a bar) with no usable chord keeps the previous chord.
    DefaultedChords {
        section: usize,
        line: Option<usize>,
        bar: usize,
        symbol: String,
    },
    /// G2P used for one syllable (ARPAbet group missing or unusable).
    PhonemeFallback {
        section: usize,
        line: usize,
        syllable: usize,
    },
    /// `same: true` with no earlier sung section of that type.
    MissingRepeatSource {
        section: usize,
    },
    /// `duet` not an object, or its `voice` absent or unknown; the song
    /// stays solo.
    DuetDropped {
        reason: &'static str,
    },
    /// `sing` text not `A`, `B` or `both`; read as `A`.
    UnknownPart {
        section: usize,
        line: Option<usize>,
        text: String,
    },
    /// `sing` named `B` or `both` in a song with no duet; read as `A`. One
    /// per section when it comes from the section's own default, rather
    /// than once per line that inherits it.
    PartWithoutDuet {
        section: usize,
        line: Option<usize>,
    },
    /// `lead` or `blend` given on a line that is not shared; ignored.
    IgnoredPartField {
        section: usize,
        line: usize,
        field: &'static str,
    },
    /// A duet where singer B sings no line, alone, as the melody or as the
    /// other voice; the song is read as solo.
    UnusedDuet,
    /// No `schema_version`, but the document uses a version-2 field; read
    /// as version 2.
    SchemaVersionInferred,
    /// A field of a newer schema than the document declares; ignored.
    /// `section` and `line` locate it; both `None` for a top-level field.
    FieldNeedsSchema {
        field: &'static str,
        needs: u32,
        section: Option<usize>,
        line: Option<usize>,
    },
    /// A melisma length outside 2..=`MELISMA_MAX_NOTES`; clamped.
    ClampedMelisma {
        section: usize,
        line: usize,
        text: String,
        to: u8,
    },
    /// A choir line in a song whose band has no choir; the choir is
    /// switched on.
    ChoirEnabled,
}

impl fmt::Display for Repair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Repair::*;
        match self {
            DefaultedField { field } => write!(f, "{field}: absent or unusable, default used"),
            TruncatedField { field, chars } => write!(f, "{field}: cut to {chars} characters"),
            ClampedTempo { from, to } => write!(f, "tempo {from} clamped to {to}"),
            ModeFromKey { mode } => write!(f, "mode {mode} read from the key"),
            SectionType {
                section,
                text,
                kind,
            } => write!(f, "section {section}: type {text:?} read as {kind}"),
            DroppedSection { section } => {
                write!(f, "section {section}: dropped (no lines and no chords)")
            }
            DroppedSyllable {
                section,
                line,
                text,
            } => {
                write!(
                    f,
                    "section {section} line {line}: {text:?} has no letter or digit, dropped"
                )
            }
            DroppedLine { section, line } => {
                write!(f, "section {section} line {line}: dropped (no syllables)")
            }
            DroppedChord {
                section,
                line: Some(l),
                bar,
                symbol,
            } => {
                write!(f, "section {section} line {l} bar {bar}: chord {symbol:?} over the limit, dropped")
            }
            DroppedChord {
                section,
                line: None,
                bar,
                symbol,
            } => {
                write!(
                    f,
                    "section {section} bar {bar}: chord {symbol:?} over the limit, dropped"
                )
            }
            UnknownChord { section, symbol } => {
                write!(f, "section {section}: chord {symbol:?} does not parse")
            }
            ChordSuffixIgnored { symbol, ignored } => {
                write!(f, "chord {symbol:?}: {ignored:?} ignored")
            }
            DefaultedChords {
                section,
                line: Some(l),
                bar,
                symbol,
            } => {
                write!(
                    f,
                    "section {section} line {l} bar {bar}: no chord, {symbol} kept"
                )
            }
            DefaultedChords {
                section,
                line: None,
                bar,
                symbol,
            } => {
                write!(f, "section {section} bar {bar}: no chord, {symbol} kept")
            }
            PhonemeFallback {
                section,
                line,
                syllable,
            } => {
                write!(
                    f,
                    "section {section} line {line} syllable {syllable}: ARPAbet unusable, G2P used"
                )
            }
            MissingRepeatSource { section } => {
                write!(f, "section {section}: same without an earlier section")
            }
            DuetDropped { reason } => write!(f, "duet dropped: {reason}"),
            UnknownPart {
                section,
                line: Some(l),
                text,
            } => {
                write!(
                    f,
                    "section {section} line {l}: sing {text:?} unknown, read as A"
                )
            }
            UnknownPart {
                section,
                line: None,
                text,
            } => {
                write!(f, "section {section}: sing {text:?} unknown, read as A")
            }
            PartWithoutDuet {
                section,
                line: Some(l),
            } => {
                write!(
                    f,
                    "section {section} line {l}: sing without a duet, read as A"
                )
            }
            PartWithoutDuet {
                section,
                line: None,
            } => {
                write!(f, "section {section}: sing without a duet, read as A")
            }
            IgnoredPartField {
                section,
                line,
                field,
            } => {
                write!(
                    f,
                    "section {section} line {line}: {field} ignored, the line is not shared"
                )
            }
            UnusedDuet => f.write_str("duet dropped: singer B sings no line"),
            SchemaVersionInferred => {
                f.write_str("schema_version absent, read as 2 (version-2 fields are used)")
            }
            FieldNeedsSchema {
                field,
                needs,
                section,
                line,
            } => {
                write!(f, "{field}")?;
                if let Some(s) = section {
                    write!(f, " (section {s}")?;
                    if let Some(l) = line {
                        write!(f, " line {l}")?;
                    }
                    f.write_str(")")?;
                }
                write!(f, ": needs schema_version {needs}, ignored")
            }
            ClampedMelisma {
                section,
                line,
                text,
                to,
            } => write!(
                f,
                "section {section} line {line}: melisma {text:?} clamped to {to} notes"
            ),
            ChoirEnabled => f.write_str("choir line without band.choir: the choir is on"),
        }
    }
}

// ---------------------------------------------------------------- normalize

/// Parses a JSON text and normalises it.
pub fn normalize_str(json: &str) -> Result<(Song, Vec<Repair>), SongError> {
    let v: Value = serde_json::from_str(json).map_err(|e| SongError::NotJson(e.to_string()))?;
    normalize_value(&v)
}

/// Normalises a parsed JSON value.
pub fn normalize_value(v: &Value) -> Result<(Song, Vec<Repair>), SongError> {
    if !v.is_object() {
        return Err(SongError::NotObject);
    }
    let w = WireSong::deserialize(v).map_err(|_| SongError::NotObject)?;
    normalize(w)
}

/// Function words left unstressed when a line has no '*' marks.
const FUNCTION_WORDS: &[&str] = &[
    "a", "an", "the", "and", "but", "or", "of", "to", "in", "on", "at", "by", "for", "with",
    "from", "as", "is", "was", "be", "are", "am", "i", "my", "me", "you", "your", "he", "she",
    "it", "its", "we", "our", "they", "their", "them", "his", "her", "that", "this", "than",
    "then", "so", "if", "nor", "oh", "o", "yet",
];

fn truncate_chars(s: String, max: usize, field: &'static str, rep: &mut Vec<Repair>) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => {
            rep.push(Repair::TruncatedField { field, chars: max });
            s[..i].to_string()
        }
        None => s,
    }
}

/// Picks an enum value from optional text; absent or unknown text gives the
/// default and a `DefaultedField` repair.
fn pick<T: std::str::FromStr + Copy>(
    v: Option<&str>,
    default: T,
    field: &'static str,
    rep: &mut Vec<Repair>,
) -> T {
    match v.and_then(|s| s.parse::<T>().ok()) {
        Some(x) => x,
        None => {
            rep.push(Repair::DefaultedField { field });
            default
        }
    }
}

fn flag(v: Option<bool>, default: bool, field: &'static str, rep: &mut Vec<Repair>) -> bool {
    v.unwrap_or_else(|| {
        rep.push(Repair::DefaultedField { field });
        default
    })
}

/// A `phrasing` object (top-level, or `duet.phrasing`): absent gives `None`
/// silently (the style, or `Phrasing::default`, applies later); present but
/// not an object gives `None` too, since nothing in it is usable, but both
/// fields are still reported defaulted; present as an object fills each
/// field, defaulting an absent or unknown one with its own `DefaultedField`.
fn phrasing_field(
    p: Option<Option<WirePhrasing>>,
    delivery_field: &'static str,
    endings_field: &'static str,
    rep: &mut Vec<Repair>,
) -> Option<Phrasing> {
    let wp = match p? {
        Some(wp) => wp,
        None => {
            rep.push(Repair::DefaultedField {
                field: delivery_field,
            });
            rep.push(Repair::DefaultedField {
                field: endings_field,
            });
            return None;
        }
    };
    let delivery = pick(
        wp.delivery.as_deref(),
        Delivery::Flowing,
        delivery_field,
        rep,
    );
    let endings = pick(wp.endings.as_deref(), Endings::Released, endings_field, rep);
    Some(Phrasing { delivery, endings })
}

/// The top-level `duet` object: absent gives solo silently; present but not
/// an object, or with no usable `voice`, gives solo with `DuetDropped`; a
/// usable duet keeps its own phrasing under `duet.phrasing`.
fn duet_header(d: Option<Option<WireDuet>>, rep: &mut Vec<Repair>) -> Option<Duet> {
    let wd = match d? {
        Some(wd) => wd,
        None => {
            rep.push(Repair::DuetDropped {
                reason: "duet is not an object",
            });
            return None;
        }
    };
    let voice = match wd.voice.as_deref().and_then(|v| v.parse::<Voice>().ok()) {
        Some(v) => v,
        None => {
            rep.push(Repair::DuetDropped {
                reason: "duet.voice is absent or unknown",
            });
            return None;
        }
    };
    let phrasing = phrasing_field(
        wd.phrasing,
        "duet.phrasing.delivery",
        "duet.phrasing.endings",
        rep,
    );
    Some(Duet { voice, phrasing })
}

/// A `sing` value: `A`, `B` or `both`, case-insensitive, surrounding
/// whitespace ignored.
#[derive(Clone, Copy, PartialEq)]
enum SingText {
    A,
    B,
    Both,
    Choir,
}

fn parse_sing(text: &str) -> Option<SingText> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("A") {
        Some(SingText::A)
    } else if t.eq_ignore_ascii_case("B") {
        Some(SingText::B)
    } else if t.eq_ignore_ascii_case("both") {
        Some(SingText::Both)
    } else if t.eq_ignore_ascii_case("choir") {
        Some(SingText::Choir)
    } else {
        None
    }
}

/// One `sing` value (a section's own default, or a line's), given its
/// already-flattened text (`None`: absent, or a JSON type with no text to
/// read; both are silent). Unknown text reads as `A` with `UnknownPart`.
/// `B`/`both` in a song without a duet reads as `A` with `PartWithoutDuet`;
/// called once for a section's own default regardless of how many lines
/// inherit it, and once per line that gives its own `sing`.
fn resolve_sing(
    text: Option<String>,
    is_duet: bool,
    section: usize,
    line: Option<usize>,
    rep: &mut Vec<Repair>,
) -> SingText {
    match text {
        None => SingText::A,
        Some(text) => match parse_sing(&text) {
            None => {
                rep.push(Repair::UnknownPart {
                    section,
                    line,
                    text,
                });
                SingText::A
            }
            Some(SingText::A) => SingText::A,
            Some(SingText::Choir) => SingText::Choir,
            Some(_) if !is_duet => {
                rep.push(Repair::PartWithoutDuet { section, line });
                SingText::A
            }
            Some(sel) => sel,
        },
    }
}

/// A shared line's `lead` (default `SingerId::A`) or `blend` (default
/// `Blend::Harmony`): absent text keeps `default`; present but unparseable
/// reads as `default` with a `DefaultedField` named `field` ("lines.lead" /
/// "lines.blend").
fn resolve_named<T: std::str::FromStr + Copy>(
    text: Option<String>,
    default: T,
    field: &'static str,
    rep: &mut Vec<Repair>,
) -> T {
    match text {
        None => default,
        Some(t) => match t.parse::<T>() {
            Ok(v) => v,
            Err(_) => {
                rep.push(Repair::DefaultedField { field });
                default
            }
        },
    }
}

fn ignore_if_present(
    present: bool,
    field: &'static str,
    section: usize,
    line: usize,
    rep: &mut Vec<Repair>,
) {
    if present {
        rep.push(Repair::IgnoredPartField {
            section,
            line,
            field,
        });
    }
}

/// Mode named by the text after the key's note name, if any.
fn mode_from_key_suffix(rest: &str) -> Option<Mode> {
    let t = rest.trim();
    if t.is_empty() {
        return None;
    }
    for (names, mode) in [
        (&["m", "min", "minor", "-", "aeolian"][..], Mode::Minor),
        (&["maj", "major", "ionian"][..], Mode::Major),
        (&["dorian"][..], Mode::Dorian),
        (&["mixolydian", "mixo"][..], Mode::Mixolydian),
    ] {
        if names.iter().any(|n| t.eq_ignore_ascii_case(n)) {
            return Some(mode);
        }
    }
    None
}

/// Section type from free text: case and non-letters ignored.
fn section_kind(text: &str) -> (SectionKind, bool) {
    let t: String = text
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if let Ok(k) = t.parse::<SectionKind>() {
        return (k, true);
    }
    match t.as_str() {
        "pre" => (SectionKind::Prechorus, true),
        "refrain" => (SectionKind::Chorus, true),
        _ if t.starts_with("chor") || t.starts_with("refrain") => (SectionKind::Chorus, false),
        _ => (SectionKind::Verse, false),
    }
}

/// Chord state shared by every bar of the song while normalising.
struct Chords<'a> {
    table: ChordTable,
    /// Chords parsed from the reply (defaults excluded).
    parsed: usize,
    /// Last chord placed; the start value is the tonic triad.
    prev: Option<ChordId>,
    tonic: Chord,
    ignored_seen: Vec<String>,
    rep: &'a mut Vec<Repair>,
    overflow: bool,
}

impl Chords<'_> {
    fn intern(&mut self, c: Chord) -> Option<ChordId> {
        let id = self.table.intern(c);
        if id.is_none() {
            self.overflow = true;
        }
        id
    }

    /// `id` moved up `semis` semitones; `id` itself when the result does not parse.
    fn transposed(&mut self, id: ChordId, semis: i32, flats: bool) -> ChordId {
        let sym = transpose_symbol(&self.table.get(id).symbol, semis, flats);
        match Chord::parse(&sym) {
            Ok(c) => self.intern(c).unwrap_or(id),
            Err(_) => id,
        }
    }

    /// Chord to use where none is given: the previous one, else the tonic triad.
    fn fallback(&mut self) -> Option<ChordId> {
        match self.prev {
            Some(id) => Some(id),
            None => self.intern(self.tonic.clone()),
        }
    }

    /// One bar from its text ("G" or "G D"). `line` is `None` in an
    /// instrumental section.
    fn bar(
        &mut self,
        text: &str,
        section: usize,
        line: Option<usize>,
        bar: usize,
    ) -> Option<BarChords> {
        let mut ids: [Option<ChordId>; BAR_MAX_CHORDS] = [None; BAR_MAX_CHORDS];
        let mut n = 0;
        for tok in text.split_whitespace() {
            match parse_detail(tok) {
                Ok((c, ignored)) => {
                    self.parsed += 1;
                    if let Some(ig) = ignored {
                        if !self.ignored_seen.contains(&c.symbol) {
                            self.ignored_seen.push(c.symbol.clone());
                            self.rep.push(Repair::ChordSuffixIgnored {
                                symbol: c.symbol.clone(),
                                ignored: ig,
                            });
                        }
                    }
                    if n == BAR_MAX_CHORDS {
                        self.rep.push(Repair::DroppedChord {
                            section,
                            line,
                            bar,
                            symbol: c.symbol,
                        });
                        continue;
                    }
                    ids[n] = self.intern(c);
                    n += 1;
                }
                Err(_) => self.rep.push(Repair::UnknownChord {
                    section,
                    symbol: tok.to_string(),
                }),
            }
        }
        let out = match (ids[0], ids[1]) {
            (Some(a), Some(b)) if n == 2 => BarChords::two(a, b),
            (Some(a), _) => BarChords::one(a),
            _ => {
                let id = self.fallback()?;
                let symbol = self.table.get(id).symbol.clone();
                self.rep.push(Repair::DefaultedChords {
                    section,
                    line,
                    bar,
                    symbol,
                });
                BarChords::one(id)
            }
        };
        self.prev = Some(out.last());
        Some(out)
    }

    /// Bars from chord entries, at most `max`; later entries are dropped
    /// with a repair. No entries gives one bar with the fallback chord.
    fn bars(
        &mut self,
        entries: &[String],
        max: usize,
        section: usize,
        line: Option<usize>,
    ) -> Vec<BarChords> {
        let mut out = Vec::with_capacity(entries.len().min(max).max(1));
        for (bi, e) in entries.iter().enumerate() {
            if bi >= max {
                for tok in e.split_whitespace() {
                    self.rep.push(Repair::DroppedChord {
                        section,
                        line,
                        bar: bi,
                        symbol: tok.to_string(),
                    });
                }
                continue;
            }
            if let Some(b) = self.bar(e, section, line, bi) {
                out.push(b);
            }
        }
        if out.is_empty() {
            if let Some(b) = self.bar("", section, line, 0) {
                out.push(b);
            }
        }
        out
    }
}

/// Syllables of one lyric text. Returns an empty list when there are none.
/// Each non-empty part with no letter or digit, and each word made only of
/// hyphens, is recorded as `Repair::DroppedSyllable`.
///
/// With `melisma` (schema 2), a part may end in `~` or `~N`: the syllable is
/// sung over N notes (2 for a bare `~`), clamped to 2..=`MELISMA_MAX_NOTES`
/// with `Repair::ClampedMelisma`.
fn syllables(
    text: &str,
    melisma: bool,
    section: usize,
    line: usize,
    rep: &mut Vec<Repair>,
) -> Vec<Syllable> {
    let mut out = Vec::new();
    let mut word: u16 = 0;
    for w in text.split_whitespace() {
        let start = out.len();
        let mut reported = false;
        for part in w.split('-') {
            let (part, notes) = if melisma {
                split_melisma(part, section, line, rep)
            } else {
                (part, 1)
            };
            if !part.chars().any(char::is_alphanumeric) {
                if !part.is_empty() {
                    rep.push(Repair::DroppedSyllable {
                        section,
                        line,
                        text: part.to_string(),
                    });
                    reported = true;
                }
                continue;
            }
            out.push(Syllable {
                text: part.chars().filter(|&c| c != '*').collect(),
                word,
                stress: part.contains('*'),
                word_start: out.len() == start,
                word_end: false,
                phones: Vec::new(),
                notes,
            });
        }
        if out.len() > start {
            if let Some(last) = out.last_mut() {
                last.word_end = true;
            }
            word = word.saturating_add(1);
        } else if !reported {
            rep.push(Repair::DroppedSyllable {
                section,
                line,
                text: w.to_string(),
            });
        }
    }
    if !out.iter().any(|s| s.stress) {
        default_stress(&mut out);
    }
    out
}

/// Splits a trailing melisma mark (`~` or `~N`) off a syllable part: the
/// part without it, and the note count (1 when there is no mark).
fn split_melisma<'a>(
    part: &'a str,
    section: usize,
    line: usize,
    rep: &mut Vec<Repair>,
) -> (&'a str, u8) {
    let Some(pos) = part.rfind('~') else {
        return (part, 1);
    };
    let tail = &part[pos + 1..];
    if !tail.chars().all(|c| c.is_ascii_digit()) {
        return (part, 1);
    }
    let asked: u64 = if tail.is_empty() {
        2
    } else {
        tail.parse().unwrap_or(u64::MAX)
    };
    let notes = asked.clamp(2, MELISMA_MAX_NOTES as u64) as u8;
    if asked != notes as u64 {
        rep.push(Repair::ClampedMelisma {
            section,
            line,
            text: part[pos..].to_string(),
            to: notes,
        });
    }
    (&part[..pos], notes)
}

/// Stress for a line written without '*': the first syllable of every
/// multi-syllable word, and every single-syllable word that is not a
/// function word.
fn default_stress(syls: &mut [Syllable]) {
    let mut i = 0;
    while i < syls.len() {
        let mut j = i + 1;
        while j < syls.len() && syls[j].word == syls[i].word {
            j += 1;
        }
        if j - i > 1 {
            syls[i].stress = true;
        } else {
            let bare: String = syls[i]
                .text
                .chars()
                .flat_map(char::to_lowercase)
                .filter(|c| c.is_ascii_lowercase() || *c == '\'')
                .collect();
            syls[i].stress = !FUNCTION_WORDS.contains(&bare.as_str());
        }
        i = j;
    }
}

/// One ARPAbet group: every token known and at least one vowel.
fn phone_group(g: &str) -> Option<Vec<Phoneme>> {
    let mut v = Vec::new();
    for tok in g.split_whitespace() {
        v.push(Phoneme::parse_token(tok)?);
    }
    v.iter().any(|p| p.is_vowel()).then_some(v)
}

/// Fills `phones` for every syllable from the `ph` field, with G2P where needed.
fn assign_phones(
    syls: &mut [Syllable],
    ph: Option<&str>,
    section: usize,
    line: usize,
    rep: &mut Vec<Repair>,
) {
    let groups: Vec<&str> = match ph {
        Some(p) => {
            let all: Vec<&str> = p.split('|').collect();
            if all.len() == syls.len() {
                all
            } else {
                // Stray separators ("a||b", "a|b|") are not groups.
                let nonempty: Vec<&str> =
                    all.into_iter().filter(|g| !g.trim().is_empty()).collect();
                if nonempty.len() == syls.len() {
                    nonempty
                } else {
                    Vec::new()
                }
            }
        }
        None => Vec::new(),
    };
    for (i, s) in syls.iter_mut().enumerate() {
        match groups.get(i).and_then(|g| phone_group(g)) {
            Some(p) => s.phones = p,
            None => {
                s.phones = g2p(&s.text);
                rep.push(Repair::PhonemeFallback {
                    section,
                    line,
                    syllable: i,
                });
            }
        }
    }
}

/// Whether the document uses any version-2 field.
fn uses_v2(w: &WireSong) -> bool {
    let choir = |t: &Option<String>| {
        t.as_deref()
            .is_some_and(|t| parse_sing(t) == Some(SingText::Choir))
    };
    w.rubato.is_some()
        || w.sections.iter().flatten().any(|s| {
            s.key.is_some()
                || s.rubato.is_some()
                || s.voicing.is_some()
                || choir(&s.sing)
                || s.lines.iter().flatten().any(|l| {
                    l.voicing.is_some()
                        || choir(&l.sing)
                        || [&l.syl, &l.lyric, &l.text]
                            .into_iter()
                            .flatten()
                            .any(|t| t.contains('~'))
                })
        })
}

/// Settles the document's schema version and removes the version-2 fields
/// of a document that declares version 1. Absent: version 1, or 2 when a
/// version-2 field is used (`SchemaVersionInferred`). A `~` in a version-1
/// lyric stays part of the syllable's text, as before.
fn resolve_version(w: &mut WireSong, rep: &mut Vec<Repair>) -> Result<u32, SongError> {
    let declared = match w.schema_version {
        Some(v) if v.is_finite() && v >= 1.0 => Some(v.round() as i64),
        Some(_) => {
            rep.push(Repair::DefaultedField {
                field: "schema_version",
            });
            None
        }
        None => None,
    };
    let version = match declared {
        Some(v) if v > SCHEMA_LATEST as i64 => return Err(SongError::UnsupportedSchema(v)),
        Some(v) => v as u32,
        None if uses_v2(w) => {
            rep.push(Repair::SchemaVersionInferred);
            SCHEMA_V2
        }
        None => SCHEMA_V1,
    };
    if version >= SCHEMA_V2 {
        return Ok(version);
    }
    let mut needs = |field: &'static str, section: Option<usize>, line: Option<usize>| {
        rep.push(Repair::FieldNeedsSchema {
            field,
            needs: SCHEMA_V2,
            section,
            line,
        });
    };
    if w.rubato.take().is_some() {
        needs("rubato", None, None);
    }
    let is_choir = |t: &Option<String>| {
        t.as_deref()
            .is_some_and(|t| parse_sing(t) == Some(SingText::Choir))
    };
    for (si, s) in w.sections.iter_mut().enumerate() {
        let Some(s) = s else { continue };
        if s.key.take().is_some() {
            needs("key", Some(si), None);
        }
        if s.rubato.take().is_some() {
            needs("rubato", Some(si), None);
        }
        if s.voicing.take().is_some() {
            needs("voicing", Some(si), None);
        }
        if is_choir(&s.sing) {
            s.sing = None;
            needs("sing: choir", Some(si), None);
        }
        for (li, l) in s.lines.iter_mut().enumerate() {
            let Some(l) = l else { continue };
            if l.voicing.take().is_some() {
                needs("voicing", Some(si), Some(li));
            }
            if is_choir(&l.sing) {
                l.sing = None;
                needs("sing: choir", Some(si), Some(li));
            }
        }
    }
    Ok(version)
}

/// A section's `key` text: the tonic, and a mode from its suffix (the
/// running mode when there is none). Unusable text is ignored with a
/// `DefaultedField`.
fn parse_section_key(
    text: Option<String>,
    run: (Pc, Mode),
    rep: &mut Vec<Repair>,
) -> Option<(Pc, Mode)> {
    let text = text?;
    let t = text.trim();
    let Some((pc, n)) = Pc::parse_prefix(t) else {
        rep.push(Repair::DefaultedField {
            field: "sections.key",
        });
        return None;
    };
    let rest = t[n..].trim();
    if rest.is_empty() {
        return Some((pc, run.1));
    }
    match mode_from_key_suffix(rest) {
        Some(m) => Some((pc, m)),
        None => {
            rep.push(Repair::DefaultedField {
                field: "sections.key",
            });
            Some((pc, run.1))
        }
    }
}

/// `body` with every chord moved up `semis` semitones, spelled for `flats`.
fn transposed_body(body: &SectionBody, semis: i32, flats: bool, ch: &mut Chords) -> SectionBody {
    let mut bar = |b: &BarChords| -> BarChords {
        let ids: Vec<ChordId> = b
            .as_slice()
            .iter()
            .map(|&id| ch.transposed(id, semis, flats))
            .collect();
        match ids[..] {
            [a] => BarChords::one(a),
            [a, b] => BarChords::two(a, b),
            _ => *b,
        }
    };
    match body {
        SectionBody::Sung(lines) => SectionBody::Sung(
            lines
                .iter()
                .map(|l| Line {
                    bars: l.bars.iter().map(&mut bar).collect(),
                    ..l.clone()
                })
                .collect(),
        ),
        SectionBody::Instrumental(bars) => {
            SectionBody::Instrumental(bars.iter().map(&mut bar).collect())
        }
    }
}

/// Validates a wire song. See the module doc for the rules.
pub fn normalize(mut w: WireSong) -> Result<(Song, Vec<Repair>), SongError> {
    let mut rep = Vec::new();
    let version = resolve_version(&mut w, &mut rep)?;

    let title = match w.title.filter(|s| !s.trim().is_empty()) {
        Some(t) => truncate_chars(t, TITLE_MAX_CHARS, "title", &mut rep),
        None => {
            rep.push(Repair::DefaultedField { field: "title" });
            "Untitled".to_string()
        }
    };
    let note = match w.note {
        Some(n) => truncate_chars(n, NOTE_MAX_CHARS, "note", &mut rep),
        None => {
            rep.push(Repair::DefaultedField { field: "note" });
            String::new()
        }
    };

    let key_text = w.key.unwrap_or_default();
    let key_text = key_text.trim();
    let (key, key_rest) = match Pc::parse_prefix(key_text) {
        Some((pc, n)) => (pc, &key_text[n..]),
        None => {
            rep.push(Repair::DefaultedField { field: "key" });
            (Pc::C, "")
        }
    };
    let mode = match w.mode.as_deref().and_then(|m| m.parse::<Mode>().ok()) {
        Some(m) => m,
        None => match mode_from_key_suffix(key_rest) {
            Some(m) => {
                rep.push(Repair::ModeFromKey { mode: m });
                m
            }
            None => {
                rep.push(Repair::DefaultedField { field: "mode" });
                Mode::Major
            }
        },
    };
    let meter = pick(w.meter.as_deref(), Meter::Four4, "meter", &mut rep);

    let tempo_in = match w.tempo.filter(|t| *t > 0.0) {
        Some(t) => t,
        None => {
            rep.push(Repair::DefaultedField { field: "tempo" });
            DEFAULT_TEMPO
        }
    };
    let rounded = tempo_in.round();
    let (lo, hi) = meter.tempo_range();
    let tempo_bpm = rounded.clamp(lo as f64, hi as f64);
    if tempo_bpm != rounded {
        rep.push(Repair::ClampedTempo {
            from: tempo_in,
            to: tempo_bpm,
        });
    }

    let guitar = pick(
        w.guitar.as_deref(),
        GuitarPattern::Fingerpick,
        "guitar",
        &mut rep,
    );
    let voice = pick(w.voice.as_deref(), Voice::Baritone, "voice", &mut rep);

    let rubato = resolve_named(w.rubato.take(), Rubato::Steady, "rubato", &mut rep);

    let d = Band::default();
    let mut band = match w.band {
        Some(b) => Band {
            drums: pick(b.drums.as_deref(), d.drums, "band.drums", &mut rep),
            bass: flag(b.bass, d.bass, "band.bass", &mut rep),
            harmony_guitar: flag(
                b.harmony_guitar,
                d.harmony_guitar,
                "band.harmonyGuitar",
                &mut rep,
            ),
            harp: flag(b.harp, d.harp, "band.harp", &mut rep),
            violin: flag(b.violin, d.violin, "band.violin", &mut rep),
            choir: flag(b.choir, d.choir, "band.choir", &mut rep),
            harmonies: flag(b.harmonies, d.harmonies, "band.harmonies", &mut rep),
            doubles: flag(b.doubles, d.doubles, "band.doubles", &mut rep),
        },
        None => {
            rep.push(Repair::DefaultedField { field: "band" });
            d
        }
    };

    let duet = duet_header(w.duet, &mut rep);
    let is_duet = duet.is_some();
    let phrasing = phrasing_field(
        w.phrasing,
        "phrasing.delivery",
        "phrasing.endings",
        &mut rep,
    );

    let mut sec_rep = Vec::new();
    let mut ch = Chords {
        table: ChordTable::new(),
        parsed: 0,
        prev: None,
        tonic: Chord::triad(key, mode.is_minor(), mode.prefers_flats(key)),
        ignored_seen: Vec::new(),
        rep: &mut sec_rep,
        overflow: false,
    };
    let mut sections: Vec<Section> = Vec::new();
    let mut last_sung: HashMap<SectionKind, usize> = HashMap::new();
    // The key in force after each pushed section, and the running key.
    let mut eff_keys: Vec<(Pc, Mode)> = Vec::new();
    let mut run_key = (key, mode);
    let mut choir_line = false;

    for (si, ws) in w.sections.into_iter().enumerate() {
        let Some(ws) = ws else {
            ch.rep.push(Repair::DroppedSection { section: si });
            continue;
        };
        let text = ws.kind.unwrap_or_default();
        let (mut kind, exact) = section_kind(&text);
        if !exact {
            ch.rep.push(Repair::SectionType {
                section: si,
                text: text.clone(),
                kind,
            });
        }

        let target_key = parse_section_key(ws.key, run_key, ch.rep).unwrap_or(run_key);
        let key_change = (target_key != run_key).then_some(KeyChange {
            tonic: target_key.0,
            mode: target_key.1,
        });
        let sec_rubato = ws.rubato.and_then(|t| match t.parse::<Rubato>() {
            Ok(r) => Some(r),
            Err(_) => {
                ch.rep.push(Repair::DefaultedField {
                    field: "sections.rubato",
                });
                None
            }
        });

        if ws.same == Some(true) {
            match last_sung
                .get(&kind)
                .and_then(|&i| Some((i, u16::try_from(i).ok()?)))
            {
                Some((i, src)) => {
                    // A repeat sounds in the running key: its chords move by
                    // the interval between the source's key and that key.
                    let semis = target_key.0.get() as i32 - eff_keys[i].0.get() as i32;
                    let body = if semis.rem_euclid(12) == 0 {
                        sections[i].body.clone()
                    } else {
                        let flats = target_key.1.prefers_flats(target_key.0);
                        transposed_body(&sections[i].body.clone(), semis, flats, &mut ch)
                    };
                    let inherited = sections[i].rubato;
                    sections.push(Section {
                        kind,
                        role: SectionRole::Plain,
                        body,
                        repeat_of: Some(src),
                        key_change,
                        rubato: sec_rubato.or(inherited),
                    });
                    eff_keys.push(target_key);
                    run_key = target_key;
                    continue;
                }
                None => ch.rep.push(Repair::MissingRepeatSource { section: si }),
            }
        }

        // The section's own defaults for lines that give no `sing`/`lead`/
        // `blend` of their own; computed once, so many lines inheriting an
        // unusable default are one repair, not one each.
        let sing_default = resolve_sing(ws.sing, is_duet, si, None, ch.rep);
        let lead_default = resolve_named(ws.lead, SingerId::A, "lines.lead", ch.rep);
        let blend_default = resolve_named(ws.blend, Blend::Harmony, "lines.blend", ch.rep);
        let voicing_default =
            resolve_named(ws.voicing, ChoirVoicing::Unison, "lines.voicing", ch.rep);

        let mut lines = Vec::new();
        for (li, wl) in ws.lines.into_iter().enumerate() {
            let Some(wl) = wl else {
                ch.rep.push(Repair::DroppedLine {
                    section: si,
                    line: li,
                });
                continue;
            };
            let text = [wl.syl, wl.lyric, wl.text]
                .into_iter()
                .flatten()
                .find(|s| !s.trim().is_empty());
            let mut syls = text
                .as_deref()
                .map(|t| syllables(t, version >= SCHEMA_V2, si, li, ch.rep))
                .unwrap_or_default();
            if syls.is_empty() {
                ch.rep.push(Repair::DroppedLine {
                    section: si,
                    line: li,
                });
                continue;
            }
            assign_phones(&mut syls, wl.ph.as_deref(), si, li, ch.rep);
            let bars = ch.bars(
                wl.chords.as_deref().unwrap_or(&[]),
                LINE_MAX_BARS,
                si,
                Some(li),
            );

            let sing_sel = match wl.sing {
                Some(t) => resolve_sing(Some(t), is_duet, si, Some(li), ch.rep),
                None => sing_default,
            };
            let lead_present = wl.lead.is_some();
            let blend_present = wl.blend.is_some();
            let lead_text = wl.lead.flatten();
            let blend_text = wl.blend.flatten();
            let voicing_present = wl.voicing.is_some();
            let voicing_text = wl.voicing;
            let part = match sing_sel {
                SingText::Choir => {
                    ignore_if_present(lead_present, "lead", si, li, ch.rep);
                    ignore_if_present(blend_present, "blend", si, li, ch.rep);
                    choir_line = true;
                    let v = match voicing_text {
                        Some(t) => {
                            resolve_named(Some(t), ChoirVoicing::Unison, "lines.voicing", ch.rep)
                        }
                        None => voicing_default,
                    };
                    Part::Choir(v)
                }
                SingText::A => {
                    ignore_if_present(voicing_present, "voicing", si, li, ch.rep);
                    ignore_if_present(lead_present, "lead", si, li, ch.rep);
                    ignore_if_present(blend_present, "blend", si, li, ch.rep);
                    Part::Solo(SingerId::A)
                }
                SingText::B => {
                    ignore_if_present(voicing_present, "voicing", si, li, ch.rep);
                    ignore_if_present(lead_present, "lead", si, li, ch.rep);
                    ignore_if_present(blend_present, "blend", si, li, ch.rep);
                    Part::Solo(SingerId::B)
                }
                SingText::Both => {
                    ignore_if_present(voicing_present, "voicing", si, li, ch.rep);
                    let melody = match lead_text {
                        Some(t) => resolve_named(Some(t), SingerId::A, "lines.lead", ch.rep),
                        None => lead_default,
                    };
                    let blend = match blend_text {
                        Some(t) => resolve_named(Some(t), Blend::Harmony, "lines.blend", ch.rep),
                        None => blend_default,
                    };
                    Part::Both { melody, blend }
                }
            };
            lines.push(Line {
                syllables: syls,
                bars,
                part,
            });
        }

        if !lines.is_empty() {
            last_sung.insert(kind, sections.len());
            sections.push(Section {
                kind,
                role: SectionRole::Plain,
                body: SectionBody::Sung(lines),
                repeat_of: None,
                key_change,
                rubato: sec_rubato,
            });
            eff_keys.push(target_key);
            run_key = target_key;
            continue;
        }
        let entries = ws.chords.unwrap_or_default();
        if entries.is_empty() {
            ch.rep.push(Repair::DroppedSection { section: si });
            continue;
        }
        if matches!(kind, SectionKind::Verse | SectionKind::Chorus) {
            kind = SectionKind::Interlude;
            ch.rep.push(Repair::SectionType {
                section: si,
                text,
                kind,
            });
        }
        let bars = ch.bars(&entries, INSTRUMENTAL_MAX_BARS, si, None);
        sections.push(Section {
            kind,
            role: SectionRole::Plain,
            body: SectionBody::Instrumental(bars),
            repeat_of: None,
            key_change,
            rubato: sec_rubato,
        });
        eff_keys.push(target_key);
        run_key = target_key;
    }

    let (parsed, overflow, chords) = (ch.parsed, ch.overflow, ch.table);
    if overflow {
        return Err(SongError::TooManyChords);
    }
    if !sections.iter().any(Section::is_sung) {
        return Err(SongError::NoLyrics);
    }
    if parsed == 0 {
        return Err(SongError::NoChords);
    }
    rep.extend(sec_rep);

    let duet = if duet.is_some() && !duet_used(&sections) {
        rep.push(Repair::UnusedDuet);
        None
    } else {
        duet
    };

    if choir_line && !band.choir {
        band.choir = true;
        rep.push(Repair::ChoirEnabled);
    }

    let song = Song {
        schema_version: version,
        title,
        note,
        key,
        mode,
        meter,
        tempo_bpm,
        guitar,
        voice,
        band,
        break_lead: BreakLead::Both,
        style: None,
        phrasing,
        duet,
        rubato,
        sections,
        chords,
    };
    Ok((song, rep))
}

/// Whether singer B sings any line: alone, as the melody of a shared line,
/// or as the other voice of one. If not, a duet header is unused.
fn duet_used(sections: &[Section]) -> bool {
    sections
        .iter()
        .flat_map(Section::lines)
        .any(|l| matches!(l.part, Part::Solo(SingerId::B) | Part::Both { .. }))
}

// ---------------------------------------------------------------- back to wire

fn bars_text(song: &Song, bars: &[BarChords]) -> Vec<Value> {
    bars.iter()
        .map(|b| {
            let names: Vec<&str> = b
                .as_slice()
                .iter()
                .map(|&id| song.chord(id).symbol.as_str())
                .collect();
            Value::String(names.join(" "))
        })
        .collect()
}

/// The song in the reply format (`schema::json_schema`), so that
/// `normalize_value(&to_wire(&s))` gives `s` back (style and break lead
/// are not part of the reply and are not written).
pub fn to_wire(song: &Song) -> Value {
    let mut sections = Vec::with_capacity(song.sections.len());
    for s in &song.sections {
        let mut o = serde_json::Map::new();
        o.insert("type".into(), Value::String(s.kind.as_str().into()));
        if let Some(k) = s.key_change {
            o.insert(
                "key".into(),
                Value::String(format!(
                    "{} {}",
                    k.tonic.name(k.mode.prefers_flats(k.tonic)),
                    k.mode
                )),
            );
        }
        if let Some(r) = s.rubato {
            o.insert("rubato".into(), Value::String(r.as_str().into()));
        }
        if s.repeat_of.is_some() {
            o.insert("same".into(), Value::Bool(true));
        } else {
            match &s.body {
                SectionBody::Sung(lines) => {
                    let ls: Vec<Value> = lines
                        .iter()
                        .map(|l| {
                            let mut syl = String::new();
                            let mut ph = String::new();
                            for (i, x) in l.syllables.iter().enumerate() {
                                if i > 0 {
                                    syl.push(if x.word_start { ' ' } else { '-' });
                                    ph.push('|');
                                }
                                if x.stress {
                                    syl.push('*');
                                }
                                syl.push_str(&x.text);
                                if x.notes > 1 {
                                    syl.push_str(&format!("~{}", x.notes));
                                }
                                for (k, p) in x.phones.iter().enumerate() {
                                    if k > 0 {
                                        ph.push(' ');
                                    }
                                    ph.push_str(p.symbol());
                                }
                            }
                            let mut lo = serde_json::json!({
                                "syl": syl, "ph": ph, "chords": bars_text(song, &l.bars)
                            });
                            if l.part != Part::default() {
                                let o = lo.as_object_mut().expect("object literal");
                                match l.part {
                                    Part::Solo(id) => {
                                        o.insert("sing".into(), Value::String(id.as_str().into()));
                                    }
                                    Part::Choir(v) => {
                                        o.insert("sing".into(), Value::String("choir".into()));
                                        if v != ChoirVoicing::Unison {
                                            o.insert(
                                                "voicing".into(),
                                                Value::String(v.as_str().into()),
                                            );
                                        }
                                    }
                                    Part::Both { melody, blend } => {
                                        o.insert("sing".into(), Value::String("both".into()));
                                        if melody != SingerId::A {
                                            o.insert(
                                                "lead".into(),
                                                Value::String(melody.as_str().into()),
                                            );
                                        }
                                        if blend != Blend::Harmony {
                                            o.insert(
                                                "blend".into(),
                                                Value::String(blend.as_str().into()),
                                            );
                                        }
                                    }
                                }
                            }
                            lo
                        })
                        .collect();
                    o.insert("lines".into(), Value::Array(ls));
                }
                SectionBody::Instrumental(bars) => {
                    o.insert("chords".into(), Value::Array(bars_text(song, bars)));
                }
            }
        }
        sections.push(Value::Object(o));
    }
    let mut top = serde_json::json!({
        "title": song.title,
        "note": song.note,
        "key": song.key.name(song.flats()),
        "mode": song.mode.as_str(),
        "meter": song.meter.as_str(),
        "tempo": song.tempo_bpm,
        "guitar": song.guitar.as_str(),
        "voice": song.voice.as_str(),
        "band": {
            "drums": song.band.drums.as_str(),
            "bass": song.band.bass,
            "harmonyGuitar": song.band.harmony_guitar,
            "harp": song.band.harp,
            "violin": song.band.violin,
            "choir": song.band.choir,
            "harmonies": song.band.harmonies,
            "doubles": song.band.doubles,
        },
        "sections": sections,
    });
    let o = top.as_object_mut().expect("object literal");
    if song.schema_version >= SCHEMA_V2 {
        o.insert("schema_version".into(), Value::from(song.schema_version));
        if song.rubato != Rubato::Steady {
            o.insert("rubato".into(), Value::String(song.rubato.as_str().into()));
        }
    }
    if let Some(p) = song.phrasing {
        o.insert("phrasing".into(), phrasing_to_wire(p));
    }
    if let Some(d) = &song.duet {
        let mut dv = serde_json::json!({ "voice": d.voice.as_str() });
        if let Some(p) = d.phrasing {
            dv.as_object_mut()
                .expect("object literal")
                .insert("phrasing".into(), phrasing_to_wire(p));
        }
        o.insert("duet".into(), dv);
    }
    top
}

fn phrasing_to_wire(p: Phrasing) -> Value {
    serde_json::json!({ "delivery": p.delivery.as_str(), "endings": p.endings.as_str() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_line(extra: Value) -> Value {
        let mut v = json!({
            "title": "t", "note": "", "key": "C", "mode": "major", "meter": "4/4", "tempo": 90,
            "guitar": "strum", "voice": "tenor",
            "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false, "violin": false,
                     "choir": false, "harmonies": false, "doubles": false},
            "sections": [{"type": "verse", "lines": [{"syl": "one *two", "ph": "w ah n|t uw", "chords": ["C"]}]}]
        });
        for (k, x) in extra.as_object().into_iter().flatten() {
            if x.is_null() {
                v.as_object_mut().map(|o| o.remove(k));
            } else {
                v[k] = x.clone();
            }
        }
        v
    }

    #[test]
    fn clean_song_has_no_repairs() {
        let (s, r) = normalize_value(&one_line(json!({}))).unwrap();
        assert!(r.is_empty(), "{r:?}");
        assert_eq!(s.voice, Voice::Tenor);
        assert_eq!(
            s.sections[0].lines()[0].syllables[1].phones,
            vec![Phoneme::T, Phoneme::Uw]
        );
    }

    #[test]
    fn mode_from_key_suffix_any_case() {
        for (key, mode) in [
            ("D Minor", Mode::Minor),
            ("Dm", Mode::Minor),
            ("Bbmin", Mode::Minor),
            ("C maj", Mode::Major),
            ("A dorian", Mode::Dorian),
            ("F#m", Mode::Minor),
        ] {
            let (s, r) = normalize_value(&one_line(json!({"key": key, "mode": null}))).unwrap();
            assert_eq!(s.mode, mode, "{key}");
            assert_eq!(r, vec![Repair::ModeFromKey { mode }]);
        }
        let (s, r) = normalize_value(&one_line(json!({"key": "G", "mode": "lydian"}))).unwrap();
        assert_eq!(s.mode, Mode::Major);
        assert_eq!(r, vec![Repair::DefaultedField { field: "mode" }]);
    }

    #[test]
    fn tempo_forms() {
        let t = |v: Value| normalize_value(&one_line(json!({"tempo": v}))).unwrap();
        assert_eq!(t(json!("120")).0.tempo_bpm, 120.0);
        assert_eq!(t(json!(" 96.5 ")).0.tempo_bpm, 97.0);
        let (s, r) = t(json!(300));
        assert_eq!(s.tempo_bpm, 150.0);
        assert_eq!(
            r,
            vec![Repair::ClampedTempo {
                from: 300.0,
                to: 150.0
            }]
        );
        let (s, r) = t(json!("fast"));
        assert_eq!(s.tempo_bpm, 88.0);
        assert_eq!(r, vec![Repair::DefaultedField { field: "tempo" }]);
        let (s, _) = normalize_value(&one_line(json!({"tempo": 100, "meter": "6/8"}))).unwrap();
        assert_eq!(s.tempo_bpm, 84.0);
    }

    #[test]
    fn bass_voice_accepted() {
        let (s, r) = normalize_value(&one_line(json!({"voice": "bass"}))).unwrap();
        assert_eq!(s.voice, Voice::Bass);
        assert!(r.is_empty());
    }

    #[test]
    fn bad_chord_is_repaired() {
        let v = one_line(json!({"sections": [{"type": "verse", "lines": [
            {"syl": "one *two", "ph": "w ah n|t uw", "chords": ["G", "Xq G7#9", "H"]}]}]}));
        let (s, r) = normalize_value(&v).unwrap();
        let bars = &s.sections[0].lines()[0].bars;
        assert_eq!(bars.len(), 3);
        assert_eq!(s.chord(bars[1].first()).symbol, "G7#9");
        assert_eq!(s.chord(bars[2].first()).symbol, "G7#9");
        assert!(r.contains(&Repair::UnknownChord {
            section: 0,
            symbol: "Xq".into()
        }));
        assert!(r.contains(&Repair::UnknownChord {
            section: 0,
            symbol: "H".into()
        }));
        assert!(r.contains(&Repair::ChordSuffixIgnored {
            symbol: "G7#9".into(),
            ignored: "#9".into()
        }));
        assert!(r
            .iter()
            .any(|x| matches!(x, Repair::DefaultedChords { bar: 2, .. })));
    }

    #[test]
    fn limits_and_string_chords() {
        let v = one_line(json!({"sections": [
            {"type": "verse", "lines": [{"syl": "a *b", "ph": "ey|b iy", "chords": "C, F G Am | G | C | F"}]},
            {"type": "Chorus", "chords": "C,F,G,C,F,G,C,F,G"}]}));
        let (s, r) = normalize_value(&v).unwrap();
        assert_eq!(s.sections[0].lines()[0].bars.len(), 4);
        assert_eq!(s.sections[1].kind, SectionKind::Interlude);
        assert_eq!(s.sections[1].n_bars(), 8);
        assert!(r.contains(&Repair::DroppedChord {
            section: 0,
            line: Some(0),
            bar: 1,
            symbol: "Am".into()
        }));
        assert!(r.contains(&Repair::DroppedChord {
            section: 0,
            line: Some(0),
            bar: 4,
            symbol: "F".into()
        }));
        assert!(r.contains(&Repair::DroppedChord {
            section: 1,
            line: None,
            bar: 8,
            symbol: "G".into()
        }));
    }

    #[test]
    fn phoneme_fallback_per_syllable_and_line() {
        let v = one_line(json!({"sections": [{"type": "verse", "lines": [
            {"syl": "*sun-ny day", "ph": "s ah|n zz|d ey", "chords": ["C"]},
            {"syl": "*sun-ny day", "ph": "s ah n|d ey", "chords": ["C"]}]}]}));
        let (s, r) = normalize_value(&v).unwrap();
        let l0 = &s.sections[0].lines()[0].syllables;
        assert_eq!(l0[0].phones, vec![Phoneme::S, Phoneme::Ah]);
        assert_eq!(l0[1].phones, g2p("ny"));
        assert_eq!(
            r.iter()
                .filter(|x| matches!(x, Repair::PhonemeFallback { line: 0, .. }))
                .count(),
            1
        );
        assert_eq!(
            r.iter()
                .filter(|x| matches!(x, Repair::PhonemeFallback { line: 1, .. }))
                .count(),
            3
        );
    }

    #[test]
    fn same_repeats_and_missing_source() {
        let v = one_line(json!({"sections": [
            {"type": "chorus", "same": true, "lines": [{"syl": "la", "ph": "l aa", "chords": ["C"]}]},
            {"type": "chorus", "same": true},
            {"type": "verse", "lines": [{"syl": "*two", "ph": "t uw", "chords": ["G"]}]}]}));
        let (s, r) = normalize_value(&v).unwrap();
        assert_eq!(s.sections.len(), 3);
        assert_eq!(s.sections[1].repeat_of, Some(0));
        assert_eq!(s.sections[1].body, s.sections[0].body);
        assert_eq!(r, vec![Repair::MissingRepeatSource { section: 0 }]);
    }

    #[test]
    fn default_stress_skips_function_words() {
        let v = one_line(json!({"sections": [{"type": "verse", "lines": [
            {"syl": "the ri-ver and home", "ph": "dh ax|r ih|v er|ae n d|hh ow m", "chords": ["C"]}]}]}));
        let (s, _) = normalize_value(&v).unwrap();
        let st: Vec<bool> = s.sections[0].lines()[0]
            .syllables
            .iter()
            .map(|x| x.stress)
            .collect();
        assert_eq!(st, vec![false, true, false, false, true]);
    }

    #[test]
    fn hard_errors() {
        assert_eq!(
            normalize_value(&json!([1])).unwrap_err(),
            SongError::NotObject
        );
        assert!(matches!(normalize_str("{"), Err(SongError::NotJson(_))));
        let v = json!({"sections": [{"type": "intro", "chords": ["C"]}]});
        assert_eq!(normalize_value(&v).unwrap_err(), SongError::NoLyrics);
        let v = json!({"sections": [{"type": "verse", "lines": [{"syl": "la", "chords": ["?"]}]}]});
        assert_eq!(normalize_value(&v).unwrap_err(), SongError::NoChords);
    }

    #[test]
    fn garbage_types_do_not_fail() {
        let v = json!({"title": 5, "tempo": [1], "band": "yes", "voice": null,
            "sections": [7, {"type": 3, "lines": ["x", {"syl": "*hey", "ph": 12, "chords": "D"}]}]});
        let (s, r) = normalize_value(&v).unwrap();
        assert_eq!(s.title, "5");
        assert_eq!(s.band, Band::default());
        assert!(r.contains(&Repair::DroppedSection { section: 0 }));
        assert!(r.contains(&Repair::DroppedLine {
            section: 1,
            line: 0
        }));
    }

    #[test]
    fn title_truncated_by_chars() {
        let title = "\u{e9}".repeat(130);
        let (s, r) = normalize_value(&one_line(json!({ "title": title }))).unwrap();
        assert_eq!(s.title.chars().count(), 120);
        assert_eq!(
            r,
            vec![Repair::TruncatedField {
                field: "title",
                chars: 120
            }]
        );
    }
}

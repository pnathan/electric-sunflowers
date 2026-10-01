//! Note events: plain data in seconds and fractional MIDI, shared by the
//! planners (`arrange`) and the renderers (`instruments`, `voice`, `engine`).
//! Times are `f64` seconds from the start of the song; levels are linear
//! `f32` in 0..=1 unless stated.

use crate::model::{Phrasing, Voice};
use crate::phoneme::Phoneme;
use serde::{Deserialize, Serialize};

/// A plucked note on a free string model (bass, harp, harmony guitar).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluckNote {
    pub t0: f64,
    /// Release (damping) time.
    pub t1: f64,
    pub midi: f32,
    pub vel: f32,
}

/// A note on one of the six accompaniment guitar strings. Each string holds
/// one list; a new note on a string ends the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StringNote {
    pub t: f64,
    /// Time the string is damped.
    pub stop: f64,
    /// String index, 0 = low E.
    pub string: u8,
    pub midi: u8,
    pub vel: f32,
}

/// A bowed violin note.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BowNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: f32,
    pub vel: f32,
    pub vibrato: bool,
}

/// Drum voice.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DrumKind {
    Kick,
    Snare,
    Rim,
    /// Brush tap on the snare head.
    Tap,
    /// Brush swirl lasting `dur` seconds.
    Swish {
        dur: f32,
    },
    Hat,
    Shaker,
    /// Tom tuned to `hz`.
    Tom {
        hz: f32,
    },
    Ride,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrumHit {
    pub t: f64,
    pub kind: DrumKind,
    pub vel: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
}

/// One sung note: a syllable (or a vowel for the choir) on one pitch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VocalNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: f32,
    /// Phonemes of the syllable; the choir sings a single vowel.
    pub phones: Vec<Phoneme>,
    pub amp: f32,
    pub stress: bool,
    pub phrase_start: bool,
    pub phrase_end: bool,
    /// Grace note: pitch (fractional MIDI) the note slides from into `midi`.
    pub grace: Option<f32>,
    /// Continuation note of a melisma: the vowel of the note before holds
    /// through, with no onset, no breath and no new attack; only the pitch
    /// moves. `false` for every ordinary syllable.
    pub legato: bool,
    /// Expression marks from the arranger pass (`docs/expression.md`);
    /// neutral, and absent from JSON, unless Claude marked the syllable.
    #[serde(default, skip_serializing_if = "Expr::is_neutral")]
    pub expr: Expr,
}

/// A note shape across its length (`Expr::shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Flat,
    /// Rises to 1.1 at 70% of the note, then back to 1.
    Swell,
    /// Falls to 0.55 over the second half.
    Fade,
    /// 1.3 at the onset, back to 1 by 30%.
    Accent,
}

/// Expression marks the voice reads (`docs/expression.md`). Timing and
/// level marks are applied to `t0` and `amp` by the arranger and are not
/// stored here. Every field neutral renders exactly as an unmarked note.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Expr {
    /// Scoop into the note: (semitones below the note, negative above;
    /// seconds to reach it). Replaces the default phrase-initial scoop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoop: Option<(f32, f32)>,
    /// Fall off the note: (semitones of offset reached at the note's end;
    /// seconds before the end where it starts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall: Option<(f32, f32)>,
    /// Vibrato depth scale; 0 removes it, any value above 0 also allows it
    /// on short notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vibrato: Option<f32>,
    #[serde(default, skip_serializing_if = "Shape::is_flat")]
    pub shape: Shape,
}

impl Shape {
    pub fn is_flat(&self) -> bool {
        *self == Shape::Flat
    }
}

impl Expr {
    pub fn is_neutral(&self) -> bool {
        *self == Expr::default()
    }
}

/// Per-singer performance settings: how one singer departs from the voice
/// type's preset. Scales are multipliers on the preset (1 = unchanged).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SingStyle {
    /// Constant pitch offset in cents.
    pub detune_cents: f32,
    /// Onset lag in seconds added to each note's t0 (not t1) at render time.
    pub lateness: f32,
    /// Vibrato depth scale.
    pub vibrato_scale: f32,
    /// Vibrato rate scale.
    pub vibrato_rate_scale: f32,
    /// Formant frequency scale (vocal tract length); 1.03 is a shorter tract.
    pub formant_scale: f32,
    /// Extra scale on F1 only.
    pub f1_scale: f32,
    /// Breath noise scale.
    pub breath_scale: f32,
    /// Breath noise added to the preset's level after scaling.
    pub breath_add: f32,
    /// Glottal Rd scale (above 1: laxer, breathier source).
    pub rd_scale: f32,
    /// Jitter (period perturbation) scale.
    pub jitter_scale: f32,
    /// Shimmer (amplitude perturbation) scale.
    pub shimmer_scale: f32,
    /// Number of high resonances (5.5-8.8 kHz) above the five formants, 0-4.
    pub n_high: u8,
    /// Time constant in seconds of the voicing-amplitude smoother.
    pub av_tau: f32,
    /// Time constant in seconds of the pitch glide between notes.
    pub glide: f32,
    /// Pitch scoop into phrase-initial notes.
    pub scoop: bool,
    /// Audible breaths in pauses between phrases.
    pub breath_pauses: bool,
    /// Delivery and endings; sets `voice::phrasing::PhrasingParams`.
    pub phrasing: Phrasing,
}

impl SingStyle {
    /// The lead singer: the preset unchanged.
    pub const LEAD: SingStyle = SingStyle {
        detune_cents: 0.0,
        lateness: 0.0,
        vibrato_scale: 1.0,
        vibrato_rate_scale: 1.0,
        formant_scale: 1.0,
        f1_scale: 1.0,
        breath_scale: 1.0,
        breath_add: 0.0,
        rd_scale: 1.0,
        jitter_scale: 1.0,
        shimmer_scale: 1.0,
        n_high: 4,
        av_tau: 0.009,
        glide: 0.028,
        scoop: true,
        breath_pauses: true,
        phrasing: Phrasing {
            delivery: crate::model::Delivery::Flowing,
            endings: crate::model::Endings::Released,
        },
    };
}

impl Default for SingStyle {
    fn default() -> Self {
        SingStyle::LEAD
    }
}

/// One singer's part: the notes and how to sing them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Singer {
    pub voice: Voice,
    pub style: SingStyle,
    pub notes: Vec<VocalNote>,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Seconds added to every note's t0 and t1 at render time.
    pub offset: f64,
}

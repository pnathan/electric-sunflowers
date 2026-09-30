//! Arrangement: pure event planners for the band and the singers.
//!
//! Every part is planned from the composed song (`compose::prepare::Prepared`)
//! into note events of `song::events`: plain data in seconds and MIDI. No
//! part renders audio and no part sets a synthesis parameter; `engine`
//! binds each event list to an instrument (`instruments`) or a voice
//! (`voice`).
//!
//! Randomness: every draw comes from `sfcore::random::Rng`, keyed by the
//! song seed, a const tag declared next to its use, and an event index
//! (bar and stroke, segment, line, singer), so one event's draws do not
//! depend on any other event.
//!
//! Parts:
//! - `guitar`: fretboard voicing search and strum or picking patterns, one
//!   note list per string.
//! - `bass`: roots on chord changes, root-fifth patterns, approach notes.
//! - `harp`: rolled chords, echoes, glissandi into the bridge and the final lift.
//! - `drums`: per-kit patterns and fills (none for `DrumKit::None`).
//! - `violin`: the instrumental lead, counter-lines, bridge long tones, fills.
//! - `harmony_guitar`: the instrumental lead an octave down, fills, arpeggios.
//! - `vocals`: lead, harmony, doubles and the choir singers.
//! - `choir` (voicings) and `lines` (counter-lines, fills) are shared helpers.

pub mod bass;
pub mod choir;
pub mod drums;
pub mod guitar;
pub mod harmony_guitar;
pub mod harp;
pub mod lines;
pub mod violin;
pub mod vocals;

use compose::prepare::Prepared;
use song::events::{BowNote, DrumHit, PluckNote, StringNote};
use song::Song;

pub use harmony_guitar::HarmonyGuitar;
pub use vocals::Vocals;

/// Every part of a song as note events.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arrangement {
    /// Accompaniment guitar, one list per string (0 = low E), each sorted by onset.
    pub guitar: [Vec<StringNote>; 6],
    pub bass: Vec<PluckNote>,
    pub harp: Vec<PluckNote>,
    /// `None` when the song has no drum kit: no drum track at all.
    pub drums: Option<Vec<DrumHit>>,
    pub violin: Vec<BowNote>,
    pub harmony_guitar: HarmonyGuitar,
    pub vocals: Vocals,
}

/// Plans every part of `song` from its composition `prepared` with the song
/// seed `seed`. Deterministic: the same inputs give the same events.
pub fn arrange(song: &Song, prepared: &Prepared, seed: u64) -> Arrangement {
    let form = &prepared.form;
    let tl = &prepared.timeline;
    Arrangement {
        guitar: guitar::plan(song, form, tl, seed),
        bass: bass::plan(form, tl, seed),
        harp: harp::plan(form, tl, seed),
        drums: drums::plan(song.band.drums, form, tl, seed),
        violin: violin::plan(song, prepared, seed),
        harmony_guitar: harmony_guitar::plan(song, prepared, seed),
        vocals: vocals::plan(song, prepared, seed),
    }
}

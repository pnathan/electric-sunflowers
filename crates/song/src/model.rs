//! Typed song model: Song, Section, Mode, Meter, SectionKind, DrumKit, Voice, Band,
//! duet parts (Part, SingerId, Blend, Duet) and singer phrasing (Phrasing,
//! Delivery, Endings). Every value is in range after `wire::normalize`; the
//! render path does not validate again.

use crate::chord::{ChordId, ChordTable};
use crate::phoneme::Phoneme;
use crate::pitch::{Pc, PcSet};
use serde::Serialize;

named_enum! {
    /// Scale mode of the song. Church-mode names; minor is natural minor (Aeolian).
    pub enum Mode ("mode") {
        Major = "major",
        Minor = "minor",
        Dorian = "dorian",
        Mixolydian = "mixolydian",
    }
}

impl Mode {
    /// Scale degrees 1-7 as semitones above the tonic.
    pub const fn degrees(self) -> &'static [u8; 7] {
        match self {
            Mode::Major => &[0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Mode::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Mode::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
        }
    }

    /// The scale on tonic C; transpose by the key for the song's scale.
    pub const fn scale(self) -> PcSet {
        PcSet::from_intervals(Pc::C, self.degrees())
    }

    /// Whether note names in this key read better with flats. Major and
    /// Mixolydian: tonics F, Bb, Eb, Ab, Db. Minor and Dorian: D, G, C, F, Bb, Eb.
    pub const fn prefers_flats(self, tonic: Pc) -> bool {
        let t = tonic.get();
        match self {
            Mode::Major | Mode::Mixolydian => matches!(t, 5 | 10 | 3 | 8 | 1),
            Mode::Minor | Mode::Dorian => matches!(t, 2 | 7 | 0 | 5 | 10 | 3),
        }
    }

    /// Whether the tonic triad is minor.
    pub const fn is_minor(self) -> bool {
        matches!(self, Mode::Minor | Mode::Dorian)
    }
}

named_enum! {
    /// Time signature.
    pub enum Meter ("meter") {
        Four4 = "4/4",
        Three4 = "3/4",
        Six8 = "6/8",
    }
}

/// The metric grid of one bar: `beats` beats of `sub` slots each. `weights`
/// has `beats * sub` entries, the metric accent of each slot (1 on the
/// downbeat); `split` is the slot count after which a two-chord bar changes
/// chord, in beats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterGrid {
    pub beats: u8,
    pub sub: u8,
    pub split: u8,
    pub weights: &'static [f32],
}

impl MeterGrid {
    /// Slots per bar.
    pub const fn slots(&self) -> usize {
        self.beats as usize * self.sub as usize
    }
}

const GRID_44: MeterGrid = MeterGrid {
    beats: 4,
    sub: 2,
    split: 2,
    weights: &[1.0, 0.15, 0.5, 0.15, 0.8, 0.15, 0.5, 0.15],
};
const GRID_34: MeterGrid = MeterGrid {
    beats: 3,
    sub: 2,
    split: 2,
    weights: &[1.0, 0.15, 0.45, 0.15, 0.5, 0.15],
};
const GRID_68: MeterGrid = MeterGrid {
    beats: 2,
    sub: 3,
    split: 1,
    weights: &[1.0, 0.2, 0.35, 0.8, 0.2, 0.35],
};

impl Meter {
    /// Metric grid: 4/4 is 4 beats of eighths, 3/4 is 3 beats of eighths, 6/8
    /// is 2 dotted-quarter beats of eighths.
    pub const fn grid(self) -> &'static MeterGrid {
        match self {
            Meter::Four4 => &GRID_44,
            Meter::Three4 => &GRID_34,
            Meter::Six8 => &GRID_68,
        }
    }

    /// Allowed tempo in beats per minute (the beat as in `grid().beats`):
    /// 36-84 for 6/8 (dotted-quarter beat), 52-150 otherwise.
    pub const fn tempo_range(self) -> (u16, u16) {
        match self {
            Meter::Six8 => (36, 84),
            Meter::Four4 | Meter::Three4 => (52, 150),
        }
    }
}

named_enum! {
    /// Section type as the model writes it.
    pub enum SectionKind ("section type") {
        Intro = "intro",
        Verse = "verse",
        Prechorus = "prechorus",
        Chorus = "chorus",
        Bridge = "bridge",
        Interlude = "interlude",
        Outro = "outro",
    }
}

named_enum! {
    /// What a section does in the arrangement beyond its kind: a plain
    /// section, an instrumental break where the lead instrument plays the
    /// tune, or a closing tag that echoes the hook.
    pub enum SectionRole ("section role") {
        Plain = "plain",
        Break = "break",
        Tag = "tag",
    }
}

named_enum! {
    /// Accompaniment guitar pattern.
    pub enum GuitarPattern ("guitar pattern") {
        Strum = "strum",
        Fingerpick = "fingerpick",
        Travis = "travis",
        Arpeggio = "arpeggio",
    }
}

named_enum! {
    /// Drum kit; `None` means no drum track.
    pub enum DrumKit ("drums") {
        None = "none",
        Brushes = "brushes",
        Soft = "soft",
        Full = "full",
    }
}

named_enum! {
    /// Lead instrument of instrumental breaks.
    pub enum BreakLead ("break lead") {
        Violin = "violin",
        Guitar = "guitar",
        Both = "both",
    }
}

named_enum! {
    /// Lead singer's voice type.
    pub enum Voice ("voice") {
        Bass = "bass",
        Baritone = "baritone",
        Tenor = "tenor",
        Alto = "alto",
        Soprano = "soprano",
    }
}

/// Comfortable sung range as MIDI notes, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct VocalRange {
    pub lo: u8,
    pub hi: u8,
}

impl VocalRange {
    /// Centre of the range in MIDI notes.
    pub fn centre(self) -> f64 {
        0.5 * (self.lo as f64 + self.hi as f64)
    }
}

impl Voice {
    /// Comfortable range: bass D2-A#3, baritone A2-F4, tenor D3-A#4,
    /// alto G3-D#5, soprano C4-A5.
    pub const fn range(self) -> VocalRange {
        match self {
            Voice::Bass => VocalRange { lo: 38, hi: 58 },
            Voice::Baritone => VocalRange { lo: 45, hi: 65 },
            Voice::Tenor => VocalRange { lo: 50, hi: 70 },
            Voice::Alto => VocalRange { lo: 55, hi: 75 },
            Voice::Soprano => VocalRange { lo: 60, hi: 81 },
        }
    }

    /// Display label ("Baritone").
    pub const fn label(self) -> &'static str {
        match self {
            Voice::Bass => "Bass",
            Voice::Baritone => "Baritone",
            Voice::Tenor => "Tenor",
            Voice::Alto => "Alto",
            Voice::Soprano => "Soprano",
        }
    }
}

/// Which band parts play. The guitar and the lead voice always play.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    pub drums: DrumKit,
    pub bass: bool,
    pub harmony_guitar: bool,
    pub harp: bool,
    pub violin: bool,
    pub choir: bool,
    pub harmonies: bool,
    pub doubles: bool,
}

impl Default for Band {
    /// The defaults for a field the model leaves out: brushes, every part
    /// on except the harp.
    fn default() -> Self {
        Band {
            drums: DrumKit::Brushes,
            bass: true,
            harmony_guitar: true,
            harp: false,
            violin: true,
            choir: true,
            harmonies: true,
            doubles: true,
        }
    }
}

named_enum! {
    /// Note delivery: how legato or clipped the singer is (design 5.1).
    /// Sets `voice::phrasing::PhrasingParams` (sustain, onset share, lead-in,
    /// vibrato, glide, swell, breath).
    pub enum Delivery ("delivery") {
        Legato = "legato",
        Flowing = "flowing",
        Parlando = "parlando",
        Detached = "detached",
    }
}

named_enum! {
    /// How a phrase-final note ends. Sets the phrase-end fade in
    /// `voice::phrasing::PhrasingParams` (end_len, fade_depth, fade_from).
    pub enum Endings ("endings") {
        Held = "held",
        Released = "released",
        Clipped = "clipped",
    }
}

/// A singer's articulation: how notes are delivered and how phrases end.
/// `Default` is Flowing + Released, today's articulation exactly, so a song
/// without a `phrasing` field renders unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Phrasing {
    pub delivery: Delivery,
    pub endings: Endings,
}

impl Default for Phrasing {
    fn default() -> Phrasing {
        Phrasing {
            delivery: Delivery::Flowing,
            endings: Endings::Released,
        }
    }
}

named_enum! {
    /// Which singer of a duet, `A` or `B`. `A` is always the top-level
    /// `Song::voice`; `B` is `Duet::voice`.
    pub enum SingerId ("singer") {
        A = "A",
        B = "B",
    }
}

named_enum! {
    /// How the other singer sings a shared line against the melody: a
    /// harmony interval (`harmony_line`, chord tones first) or the same
    /// tune in another octave.
    pub enum Blend ("blend") {
        Harmony = "harmony",
        Octave = "octave",
    }
}

/// Who sings one lyric line: one singer alone, or both, one carrying the
/// melody and the other in harmony or another octave (design 4.3).
/// `Default` is `Solo(A)`, today's only case, so a song without duet fields
/// renders unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    Solo(SingerId),
    Both { melody: SingerId, blend: Blend },
}

impl Default for Part {
    fn default() -> Part {
        Part::Solo(SingerId::A)
    }
}

impl Part {
    /// The singer who carries the tune.
    pub fn melody(self) -> SingerId {
        match self {
            Part::Solo(s) => s,
            Part::Both { melody, .. } => melody,
        }
    }

    /// The other singer and how they blend, on a shared line; `None` when
    /// the line is sung by one singer alone.
    pub fn other(self) -> Option<(SingerId, Blend)> {
        match self {
            Part::Solo(_) => None,
            Part::Both { melody, blend } => {
                let other = match melody {
                    SingerId::A => SingerId::B,
                    SingerId::B => SingerId::A,
                };
                Some((other, blend))
            }
        }
    }
}

/// `{"sing": "A"|"B"|"both", "lead"?: "A"|"B", "blend"?: "harmony"|"octave"}`,
/// the wire shape of a shared line's part (design 4.3). `lead` and `blend`
/// are present only when the line is shared.
impl Serialize for Part {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Part::Solo(id) => {
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("sing", id.as_str())?;
                m.end()
            }
            Part::Both { melody, blend } => {
                let mut m = s.serialize_map(Some(3))?;
                m.serialize_entry("sing", "both")?;
                m.serialize_entry("lead", melody.as_str())?;
                m.serialize_entry("blend", blend.as_str())?;
                m.end()
            }
        }
    }
}

/// Singer B of a duet: their voice type, and their own phrasing (falls back
/// to the song's `phrasing`, then to `Phrasing::default`, in `phrasing_of`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Duet {
    pub voice: Voice,
    pub phrasing: Option<Phrasing>,
}

/// The chords of one bar: one or two, in order. A two-chord bar changes
/// chord after `MeterGrid::split` beats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarChords {
    ids: [ChordId; 2],
    n: u8,
}

impl BarChords {
    pub const fn one(a: ChordId) -> BarChords {
        BarChords { ids: [a, a], n: 1 }
    }

    pub const fn two(a: ChordId, b: ChordId) -> BarChords {
        BarChords { ids: [a, b], n: 2 }
    }

    /// The chords in order (length 1 or 2).
    pub fn as_slice(&self) -> &[ChordId] {
        &self.ids[..self.n as usize]
    }

    pub const fn first(&self) -> ChordId {
        self.ids[0]
    }

    pub const fn last(&self) -> ChordId {
        self.ids[self.n as usize - 1]
    }

    pub const fn len(&self) -> usize {
        self.n as usize
    }

    pub const fn is_empty(&self) -> bool {
        false
    }
}

impl Serialize for BarChords {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(s)
    }
}

/// One sung syllable.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Syllable {
    /// Syllable text without stress marks.
    pub text: String,
    /// Index of the word within its line.
    pub word: u16,
    pub stress: bool,
    pub word_start: bool,
    pub word_end: bool,
    /// Phonemes, with at least one vowel.
    pub phones: Vec<Phoneme>,
}

/// One lyric line: at least one syllable and 1-4 bars.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Line {
    pub syllables: Vec<Syllable>,
    pub bars: Vec<BarChords>,
    /// Which singer(s) carry this line. `Part::default()` (`Solo(A)`) outside
    /// a duet, always.
    pub part: Part,
}

impl Line {
    /// Words of the line, syllables joined.
    pub fn words(&self) -> impl Iterator<Item = String> + '_ {
        let mut i = 0;
        std::iter::from_fn(move || {
            let first = self.syllables.get(i)?;
            let mut w = String::new();
            while let Some(s) = self.syllables.get(i) {
                if s.word != first.word {
                    break;
                }
                w.push_str(&s.text);
                i += 1;
            }
            Some(w)
        })
    }

    /// Syllable texts joined by single spaces.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, s) in self.syllables.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&s.text);
        }
        out
    }
}

/// Content of a section: sung lines, or bars of chords with no voice (1-8 bars).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionBody {
    Sung(Vec<Line>),
    Instrumental(Vec<BarChords>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Section {
    pub kind: SectionKind,
    pub role: SectionRole,
    pub body: SectionBody,
    /// Index (in `Song::sections`) of the section this one repeats verbatim
    /// (the model's `same: true`); the body is a copy of that section's.
    pub repeat_of: Option<u16>,
}

impl Section {
    pub fn is_sung(&self) -> bool {
        matches!(self.body, SectionBody::Sung(_))
    }

    /// Sung lines; empty for an instrumental section.
    pub fn lines(&self) -> &[Line] {
        match &self.body {
            SectionBody::Sung(l) => l,
            SectionBody::Instrumental(_) => &[],
        }
    }

    /// Number of bars in the section.
    pub fn n_bars(&self) -> usize {
        match &self.body {
            SectionBody::Sung(l) => l.iter().map(|x| x.bars.len()).sum(),
            SectionBody::Instrumental(b) => b.len(),
        }
    }

    /// Every bar in order.
    pub fn bars(&self) -> impl Iterator<Item = &BarChords> {
        let (a, b): (&[Line], &[BarChords]) = match &self.body {
            SectionBody::Sung(l) => (l, &[]),
            SectionBody::Instrumental(b) => (&[], b),
        };
        a.iter().flat_map(|l| l.bars.iter()).chain(b.iter())
    }
}

/// A song after normalisation. Every value is in range.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Song {
    /// At most 120 characters.
    pub title: String,
    /// At most 400 characters.
    pub note: String,
    /// Tonic.
    pub key: Pc,
    pub mode: Mode,
    pub meter: Meter,
    /// Integer beats per minute inside `meter.tempo_range()`.
    pub tempo_bpm: f64,
    pub guitar: GuitarPattern,
    pub voice: Voice,
    pub band: Band,
    pub break_lead: BreakLead,
    /// Style id, set when a style is applied.
    pub style: Option<String>,
    /// Singer A's phrasing, and the song's default; `None` means the style's
    /// default (or, unstyled, `Phrasing::default()`) applies (design 5.1).
    pub phrasing: Option<Phrasing>,
    /// Singer B, when the song is a duet; `None` is solo.
    pub duet: Option<Duet>,
    /// At least one section is sung.
    pub sections: Vec<Section>,
    /// Every chord the song uses; `BarChords` index into it.
    pub chords: ChordTable,
}

impl Song {
    pub fn grid(&self) -> &'static MeterGrid {
        self.meter.grid()
    }

    /// Whether note names in this song's key read better with flats.
    pub fn flats(&self) -> bool {
        self.mode.prefers_flats(self.key)
    }

    /// The song's scale as pitch classes.
    pub fn scale(&self) -> PcSet {
        self.mode.scale().transpose(self.key.get() as i32)
    }

    pub fn chord(&self, id: ChordId) -> &crate::chord::Chord {
        self.chords.get(id)
    }

    /// Whether the song has a singer B.
    pub fn is_duet(&self) -> bool {
        self.duet.is_some()
    }

    /// A singer's voice type: `A` is always `Some(self.voice)`; `B` is the
    /// duet's voice, or `None` outside a duet.
    pub fn voice_of(&self, s: SingerId) -> Option<Voice> {
        match s {
            SingerId::A => Some(self.voice),
            SingerId::B => self.duet.as_ref().map(|d| d.voice),
        }
    }

    /// A singer's phrasing: `B`'s own if set, else the song's, else
    /// `Phrasing::default()`; `A`'s is the song's, else the default.
    pub fn phrasing_of(&self, s: SingerId) -> Phrasing {
        match s {
            SingerId::B => self
                .duet
                .as_ref()
                .and_then(|d| d.phrasing)
                .or(self.phrasing)
                .unwrap_or_default(),
            SingerId::A => self.phrasing.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_parse_their_names() {
        for m in Mode::ALL {
            assert_eq!(m.as_str().parse::<Mode>(), Ok(*m));
        }
        assert_eq!("  Dorian ".parse::<Mode>(), Ok(Mode::Dorian));
        assert_eq!("6/8".parse::<Meter>(), Ok(Meter::Six8));
        assert_eq!("bass".parse::<Voice>(), Ok(Voice::Bass));
        assert!("lydian".parse::<Mode>().is_err());
        assert_eq!(SectionKind::NAMES.len(), 7);
    }

    #[test]
    fn scales_and_flats() {
        assert_eq!(Mode::Major.scale().bits(), 0b1010_1011_0101);
        assert!(Mode::Major.prefers_flats(Pc::new(5)));
        assert!(!Mode::Major.prefers_flats(Pc::new(7)));
        assert!(Mode::Minor.prefers_flats(Pc::new(2)));
        for m in [Meter::Four4, Meter::Three4, Meter::Six8] {
            assert_eq!(m.grid().weights.len(), m.grid().slots());
        }
    }

    #[test]
    fn line_words() {
        let syl = |t: &str, w: u16| Syllable {
            text: t.into(),
            word: w,
            stress: false,
            word_start: false,
            word_end: false,
            phones: vec![],
        };
        let l = Line {
            syllables: vec![syl("hel", 0), syl("lo", 0), syl("world", 1)],
            bars: vec![],
            part: Part::default(),
        };
        assert_eq!(l.words().collect::<Vec<_>>(), vec!["hello", "world"]);
        assert_eq!(l.text(), "hel lo world");
    }
}

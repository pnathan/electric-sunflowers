//! Styles and forms: typed tables, the numbered form plan the prompt shows,
//! and the random style direction.
//!
//! A style fixes label, meters with a tempo range per meter, modes, the
//! harmonic idiom, guitar pattern, band parts and drum kit, the lead of
//! instrumental breaks, and the forms it may use. A form is a list of steps;
//! each step has a section kind, a role (plain, break, tag) and a body
//! (instrumental bars, sung lines, or a repeat). `STYLES` and `FORMS` are
//! indexed by their id enums, so a lookup by id cannot fail; a lookup by
//! name returns an error on an unknown name.
//!
//! Random choices draw from `sfcore::random::Rng` in a fixed order: style
//! (when not given), meter, form, mode, the world-flavour roll, and the
//! world-flavour pick when the roll succeeds.

use std::fmt;
use std::str::FromStr;

use sfcore::random::Rng;
use song::{
    Band, BreakLead, Delivery, DrumKit, Endings, GuitarPattern, Meter, Mode, Phrasing, Repair,
    SectionKind, SectionRole, Song,
};

/// Declares a fieldless id enum with its spellings: `ALL`, `as_str`,
/// `Display`, and `FromStr` (exact spelling, surrounding whitespace ignored)
/// that returns `$err` on an unknown name.
macro_rules! id_enum {
    ($(#[$m:meta])* $name:ident, $err:ident { $($var:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $($var),+ }

        impl $name {
            /// Every id, in table order.
            pub const ALL: &'static [$name] = &[$($name::$var),+];

            /// The id's spelling.
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$var => $s),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = $err;
            fn from_str(s: &str) -> Result<Self, $err> {
                let t = s.trim();
                $(if t == $s { return Ok($name::$var); })+
                Err($err(s.to_string()))
            }
        }
    };
}

/// A style id that names no style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStyle(pub String);

impl fmt::Display for UnknownStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown style {:?} (known: ", self.0)?;
        for (i, id) in StyleId::ALL.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            f.write_str(id.as_str())?;
        }
        f.write_str(")")
    }
}

impl std::error::Error for UnknownStyle {}

/// A form id that names no form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownForm(pub String);

impl fmt::Display for UnknownForm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown form {:?}", self.0)
    }
}

impl std::error::Error for UnknownForm {}

id_enum! {
    /// Style id; indexes `STYLES`.
    StyleId, UnknownStyle {
        Appalachian = "appalachian",
        Oldtime = "oldtime",
        Bluegrass = "bluegrass",
        Cowboy = "cowboy",
        Bakersfield = "bakersfield",
        Texas = "texas",
        Cajun = "cajun",
        Zydeco = "zydeco",
        Acadian = "acadian",
        Broadside = "broadside",
        Scottish = "scottish",
        IrishAir = "irishair",
        IrishPub = "irishpub",
        Welsh = "welsh",
        Breton = "breton",
        Blues = "blues",
        Gospel = "gospel",
        Revival = "revival",
        Laurel = "laurel",
        Nashville = "nashville",
        Americana = "americana",
        Shanty = "shanty",
    }
}

id_enum! {
    /// Form id; indexes `FORMS`.
    FormId, UnknownForm {
        Vc = "vc",
        VcBreaks = "vcBreaks",
        Strophic = "strophic",
        Refrain = "refrain",
        Aaba = "aaba",
        ChorusFirst = "chorusFirst",
        Prechorus = "prechorus",
        WaltzBreaks = "waltzBreaks",
        Blues12 = "blues12",
        Hymn = "hymn",
    }
}

impl StyleId {
    /// The style's table entry.
    pub fn style(self) -> &'static Style {
        &STYLES[self as usize]
    }
}

impl FormId {
    /// The form's table entry.
    pub fn form(self) -> &'static Form {
        &FORMS[self as usize]
    }
}

/// Number of lyric lines a sung step asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lines {
    Exactly(u8),
    /// An inclusive range; the songwriter chooses.
    Between(u8, u8),
}

impl Lines {
    /// Whether the count is exactly one line (singular in the plan text).
    pub const fn is_one(self) -> bool {
        matches!(self, Lines::Exactly(1))
    }
}

impl fmt::Display for Lines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Lines::Exactly(n) => write!(f, "{n}"),
            Lines::Between(a, b) => write!(f, "{a} to {b}"),
        }
    }
}

/// What a form step holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepBody {
    /// Instrumental: this many chord entries (bars), no lines.
    Instrumental { bars: u8 },
    /// Sung: this many lines, `Form::bars_per_line` chord entries each.
    Sung { lines: Lines },
    /// A literal repeat of the previous section of the same kind.
    Repeat,
}

/// One step of a form plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormStep {
    pub kind: SectionKind,
    /// `Break`: the lead instrument plays the tune. `Tag`: a closing echo
    /// of the hook.
    pub role: SectionRole,
    pub body: StepBody,
}

const fn inst(kind: SectionKind, bars: u8) -> FormStep {
    FormStep {
        kind,
        role: SectionRole::Plain,
        body: StepBody::Instrumental { bars },
    }
}

const fn brk(kind: SectionKind, bars: u8) -> FormStep {
    FormStep {
        kind,
        role: SectionRole::Break,
        body: StepBody::Instrumental { bars },
    }
}

const fn sung(kind: SectionKind, lines: Lines) -> FormStep {
    FormStep {
        kind,
        role: SectionRole::Plain,
        body: StepBody::Sung { lines },
    }
}

const fn same(kind: SectionKind) -> FormStep {
    FormStep {
        kind,
        role: SectionRole::Plain,
        body: StepBody::Repeat,
    }
}

const fn tag(lines: Lines) -> FormStep {
    FormStep {
        kind: SectionKind::Outro,
        role: SectionRole::Tag,
        body: StepBody::Sung { lines },
    }
}

/// A song form.
#[derive(Clone, Copy, Debug)]
pub struct Form {
    pub id: FormId,
    pub label: &'static str,
    /// Extra instruction for the songwriter; empty when none.
    pub note: &'static str,
    /// Chord entries (bars) per sung line.
    pub bars_per_line: u8,
    pub steps: &'static [FormStep],
}

impl Form {
    /// The numbered plan shown in the prompt, one step per line. Sung
    /// steps are counted per section kind ("verse 2"); the step's role
    /// selects the break and tag wording.
    pub fn plan_text(&self) -> String {
        let mut count = [0u32; SectionKind::ALL.len()];
        let mut out = String::new();
        for (i, step) in self.steps.iter().enumerate() {
            let n = i + 1;
            let kind = step.kind.as_str();
            let c = &mut count[step.kind as usize];
            *c += 1;
            if i > 0 {
                out.push('\n');
            }
            let line = match (step.body, step.role) {
                (StepBody::Instrumental { bars }, role) => format!(
                    "{n}. {kind}: instrumental{}, {bars} chord entries, no lines",
                    if role == SectionRole::Break { " break (the lead instrument plays the tune)" } else { "" },
                ),
                (StepBody::Repeat, _) => {
                    format!("{n}. {kind}: repeat, written {{\"type\":\"{kind}\",\"same\":true}}")
                }
                (StepBody::Sung { lines }, SectionRole::Tag) => format!(
                    "{n}. {kind}: a tag of {lines} lyric line{} (a closing echo of the hook), {} chord entries per line",
                    if lines.is_one() { "" } else { "s" },
                    self.bars_per_line,
                ),
                (StepBody::Sung { lines }, _) => format!(
                    "{n}. {kind} {c}: {lines} lines, {} chord entries per line",
                    self.bars_per_line
                ),
            };
            out.push_str(&line);
        }
        out
    }
}

use Lines::{Between, Exactly};
use SectionKind::{Bridge, Chorus, Interlude, Intro, Outro, Prechorus, Verse};

/// The forms, indexed by `FormId`.
pub static FORMS: [Form; 10] = [
    Form {
        id: FormId::Vc,
        label: "verse and chorus",
        note: "",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Chorus, Exactly(4)),
            sung(Verse, Exactly(4)),
            same(Chorus),
            sung(Bridge, Between(2, 4)),
            same(Chorus),
            inst(Outro, 3),
        ],
    },
    Form {
        id: FormId::VcBreaks,
        label: "verse and chorus with instrumental breaks",
        note: "",
        bars_per_line: 2,
        steps: &[
            brk(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Chorus, Exactly(4)),
            brk(Interlude, 8),
            sung(Verse, Exactly(4)),
            same(Chorus),
            brk(Interlude, 4),
            same(Chorus),
            tag(Between(1, 2)),
        ],
    },
    Form {
        id: FormId::Strophic,
        label: "strophic ballad",
        note: "No chorus. The story runs across the verses; each verse moves it forward.",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            inst(Interlude, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            inst(Outro, 2),
        ],
    },
    Form {
        id: FormId::Refrain,
        label: "verses with a refrain line",
        note: "No chorus. Every verse ends with the same refrain line, word for word; the verses change what it means.",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            inst(Interlude, 4),
            sung(Verse, Exactly(4)),
            inst(Outro, 2),
        ],
    },
    Form {
        id: FormId::Aaba,
        label: "AABA (32-bar song form)",
        note: "The A sections share one melody; the B section (bridge) contrasts in harmony and angle.",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            sung(Bridge, Exactly(4)),
            sung(Verse, Exactly(4)),
            inst(Interlude, 4),
            same(Bridge),
            sung(Verse, Exactly(4)),
            inst(Outro, 2),
        ],
    },
    Form {
        id: FormId::ChorusFirst,
        label: "chorus first",
        note: "Open cold on the chorus, then tell the story.",
        bars_per_line: 2,
        steps: &[
            sung(Chorus, Exactly(4)),
            sung(Verse, Exactly(4)),
            same(Chorus),
            sung(Verse, Exactly(4)),
            same(Chorus),
            same(Chorus),
            inst(Outro, 3),
        ],
    },
    Form {
        id: FormId::Prechorus,
        label: "verse, pre-chorus, chorus",
        note: "",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Prechorus, Exactly(2)),
            sung(Chorus, Exactly(4)),
            sung(Verse, Exactly(4)),
            same(Prechorus),
            same(Chorus),
            sung(Bridge, Between(2, 4)),
            same(Chorus),
            inst(Outro, 3),
        ],
    },
    Form {
        id: FormId::WaltzBreaks,
        label: "verse and chorus with a break",
        note: "",
        bars_per_line: 2,
        steps: &[
            brk(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Chorus, Exactly(4)),
            brk(Interlude, 4),
            sung(Verse, Exactly(4)),
            same(Chorus),
            same(Chorus),
            inst(Outro, 3),
        ],
    },
    Form {
        id: FormId::Blues12,
        label: "12-bar blues",
        note: "Each verse is three lines, and each of those lines spans FOUR chord entries (4 bars), making 12 bars: I7 I7 I7 I7 | IV7 IV7 I7 I7 | V7 IV7 I7 V7 (variants welcome). Line two repeats line one, perhaps with a small change; line three answers or twists it.",
        bars_per_line: 4,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(3)),
            sung(Verse, Exactly(3)),
            brk(Interlude, 8),
            sung(Verse, Exactly(3)),
            sung(Verse, Exactly(3)),
            inst(Outro, 4),
        ],
    },
    Form {
        id: FormId::Hymn,
        label: "hymn stanzas",
        note: "Stanzas in a hymn meter (8.6.8.6 or 8.7.8.7 syllables per line), plainly sung; the tag is one closing line.",
        bars_per_line: 2,
        steps: &[
            inst(Intro, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            inst(Interlude, 4),
            sung(Verse, Exactly(4)),
            sung(Verse, Exactly(4)),
            tag(Exactly(1)),
        ],
    },
];

/// How welcome a duet is in a style (design 4.7). Sets the prompt's
/// "In this style a duet is common | occasional | rare" line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DuetFit {
    Welcome,
    Occasional,
    Rare,
}

impl DuetFit {
    /// The prompt's word for this fit.
    pub const fn as_str(self) -> &'static str {
        match self {
            DuetFit::Welcome => "common",
            DuetFit::Occasional => "occasional",
            DuetFit::Rare => "rare",
        }
    }
}

impl fmt::Display for DuetFit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A style.
#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub id: StyleId,
    pub label: &'static str,
    pub meters: &'static [Meter],
    /// Tempo range per meter: (meter, lo, hi) in felt beats per minute.
    pub tempo: &'static [(Meter, u32, u32)],
    pub modes: &'static [Mode],
    /// Harmonic idiom, quoted in the prompt.
    pub idiom: &'static str,
    pub guitar: GuitarPattern,
    /// Band parts and drum kit.
    pub band: Band,
    pub forms: &'static [FormId],
    /// Lead instrument of instrumental breaks.
    pub lead: BreakLead,
    /// How welcome a duet is in this style (design 4.7).
    pub duet: DuetFit,
    /// The style's default articulation (design 5.1); `Style::apply` fills
    /// `song.phrasing` with this only when it is `None`.
    pub phrasing: Phrasing,
}

impl Style {
    /// The style's tempo range for `meter`, if it plays in that meter.
    pub fn tempo_for(&self, meter: Meter) -> Option<(u32, u32)> {
        self.tempo
            .iter()
            .find(|(m, _, _)| *m == meter)
            .map(|(_, lo, hi)| (*lo, *hi))
    }

    /// Imposes the style's arrangement on `song`: guitar pattern, break
    /// lead, band parts and drum kit, the style id, and the tempo clamped
    /// to the style's range for the song's meter widened by 10% each way,
    /// then rounded. Returns a `ClampedTempo` repair when the tempo moves.
    pub fn apply(&self, song: &mut Song) -> Vec<Repair> {
        song.guitar = self.guitar;
        song.break_lead = self.lead;
        song.band = self.band;
        song.style = Some(self.id.as_str().to_string());
        // The style's default, only when the song did not write its own
        // (no repair: this is the style's default, as the guitar pattern
        // is). Singer B's phrasing falls back on its own (`phrasing_of`).
        if song.phrasing.is_none() {
            song.phrasing = Some(self.phrasing);
        }
        let mut rep = Vec::new();
        if let Some((lo, hi)) = self.tempo_for(song.meter) {
            let from = song.tempo_bpm;
            let to = from.max(lo as f64 * 0.9).min(hi as f64 * 1.1).round();
            if to != from {
                rep.push(Repair::ClampedTempo { from, to });
            }
            song.tempo_bpm = to;
        }
        rep
    }
}

/// The style named `key`.
pub fn style(key: &str) -> Result<&'static Style, UnknownStyle> {
    key.parse::<StyleId>().map(StyleId::style)
}

/// Applies the style named `key` to `song` (see `Style::apply`).
pub fn apply_style(key: &str, song: &mut Song) -> Result<Vec<Repair>, UnknownStyle> {
    Ok(style(key)?.apply(song))
}

/// Every style with its id spelling, in table order.
pub fn styles() -> impl Iterator<Item = (&'static str, &'static Style)> {
    STYLES.iter().map(|s| (s.id.as_str(), s))
}

/// A band: the kit, the bass on, every other part off. Struct update
/// syntax turns parts on per style.
const fn kit(drums: DrumKit) -> Band {
    Band {
        drums,
        bass: true,
        harmony_guitar: false,
        harp: false,
        violin: false,
        choir: false,
        harmonies: false,
        doubles: false,
    }
}

use DrumKit::{Brushes, Full, Soft};
use GuitarPattern::{Arpeggio, Fingerpick, Strum, Travis};
use Meter::{Four4, Six8, Three4};
use Mode::{Dorian, Major, Minor, Mixolydian};

/// The styles, indexed by `StyleId`.
pub static STYLES: [Style; 22] = [
    Style {
        id: StyleId::Appalachian,
        label: "Appalachian ballad",
        meters: &[Three4, Four4],
        tempo: &[(Three4, 72, 96), (Four4, 66, 88)],
        modes: &[Mixolydian, Dorian, Minor, Major],
        idiom: "modal and spare: two or three chords (I, bVII, IV or i, bVII, v), a long story sung plainly",
        guitar: Fingerpick,
        band: Band { bass: false, violin: true, ..kit(DrumKit::None) },
        forms: &[FormId::Strophic, FormId::Refrain],
        lead: BreakLead::Violin,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Oldtime,
        label: "Old-time string band",
        meters: &[Four4],
        tempo: &[(Four4, 100, 128)],
        modes: &[Major, Mixolydian],
        idiom: "I, IV, V with drive; the fiddle takes the breaks",
        guitar: Strum,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, ..kit(DrumKit::None) },
        forms: &[FormId::VcBreaks, FormId::Refrain],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Clipped },
    },
    Style {
        id: StyleId::Bluegrass,
        label: "Bluegrass",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 108, 140), (Three4, 100, 132)],
        modes: &[Major],
        idiom: "I, IV, V with the occasional II major; high lonesome harmony on the chorus; fiddle and guitar trade breaks",
        guitar: Strum,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, ..kit(DrumKit::None) },
        forms: &[FormId::VcBreaks],
        lead: BreakLead::Both,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Clipped },
    },
    Style {
        id: StyleId::Cowboy,
        label: "Western and cowboy song",
        meters: &[Three4, Four4],
        tempo: &[(Three4, 80, 104), (Four4, 76, 100)],
        modes: &[Major],
        idiom: "open-range major harmony, I, IV, V and the II7 on the way home",
        guitar: Travis,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, ..kit(Brushes) },
        forms: &[FormId::Vc, FormId::WaltzBreaks, FormId::Strophic],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Parlando, endings: Endings::Released },
    },
    Style {
        id: StyleId::Bakersfield,
        label: "Bakersfield country",
        meters: &[Four4],
        tempo: &[(Four4, 112, 144)],
        modes: &[Major, Mixolydian],
        idiom: "shuffle-driven I, IV, V with a bVII; a twangy guitar break",
        guitar: Strum,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, doubles: true, ..kit(Full) },
        forms: &[FormId::VcBreaks, FormId::Vc, FormId::ChorusFirst],
        lead: BreakLead::Guitar,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Texas,
        label: "Texas songwriter",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 78, 104), (Three4, 84, 108)],
        modes: &[Major, Minor],
        idiom: "plain chords, long verses, detail-heavy storytelling",
        guitar: Travis,
        band: Band { harmony_guitar: true, ..kit(Brushes) },
        forms: &[FormId::Refrain, FormId::Strophic, FormId::Aaba],
        lead: BreakLead::Guitar,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Parlando, endings: Endings::Released },
    },
    Style {
        id: StyleId::Cajun,
        label: "Cajun waltz",
        meters: &[Three4],
        tempo: &[(Three4, 104, 138)],
        modes: &[Major],
        idiom: "two or three chords (I, V, IV); the fiddle answers the voice",
        guitar: Strum,
        band: Band { violin: true, harmonies: true, ..kit(Soft) },
        forms: &[FormId::WaltzBreaks],
        lead: BreakLead::Violin,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Released },
    },
    Style {
        id: StyleId::Zydeco,
        label: "Creole and zydeco two-step",
        meters: &[Four4],
        tempo: &[(Four4, 108, 132)],
        modes: &[Major, Mixolydian],
        idiom: "I, IV, V with a push; a call you can shout back",
        guitar: Strum,
        band: Band {
            harmony_guitar: true,
            violin: true,
            choir: true,
            harmonies: true,
            doubles: true,
            ..kit(Full)
        },
        forms: &[FormId::ChorusFirst, FormId::VcBreaks],
        lead: BreakLead::Both,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Clipped },
    },
    Style {
        id: StyleId::Acadian,
        label: "Acadian fiddle song",
        meters: &[Six8, Three4],
        tempo: &[(Six8, 62, 80), (Three4, 96, 120)],
        modes: &[Major, Mixolydian],
        idiom: "kitchen-party harmony, I, IV, V; the fiddle leads the dance",
        guitar: Strum,
        band: Band { violin: true, harmonies: true, ..kit(Soft) },
        forms: &[FormId::VcBreaks, FormId::Refrain],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Broadside,
        label: "English broadside ballad",
        meters: &[Six8, Three4, Four4],
        tempo: &[(Six8, 52, 68), (Three4, 80, 104), (Four4, 76, 98)],
        modes: &[Major, Dorian, Mixolydian],
        idiom: "a tune for a news story or a scandal, simple diatonic harmony",
        guitar: Fingerpick,
        band: Band { bass: false, violin: true, ..kit(DrumKit::None) },
        forms: &[FormId::Strophic, FormId::Refrain],
        lead: BreakLead::Violin,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Parlando, endings: Endings::Released },
    },
    Style {
        id: StyleId::Scottish,
        label: "Scottish ballad",
        meters: &[Three4, Four4],
        tempo: &[(Three4, 66, 88), (Four4, 60, 80)],
        modes: &[Dorian, Mixolydian, Minor],
        idiom: "modal and dignified: i, bVII, bVI or I, bVII",
        guitar: Arpeggio,
        band: Band { harp: true, violin: true, ..kit(DrumKit::None) },
        forms: &[FormId::Strophic, FormId::Refrain, FormId::Aaba],
        lead: BreakLead::Violin,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Legato, endings: Endings::Released },
    },
    Style {
        id: StyleId::IrishAir,
        label: "Irish air",
        meters: &[Three4, Six8],
        tempo: &[(Three4, 60, 80), (Six8, 44, 60)],
        modes: &[Major, Dorian, Mixolydian],
        idiom: "a long-breathed tune over few chords; ornament rather than harmony",
        guitar: Arpeggio,
        band: Band { harp: true, violin: true, ..kit(DrumKit::None) },
        forms: &[FormId::Strophic, FormId::Aaba],
        lead: BreakLead::Violin,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Legato, endings: Endings::Held },
    },
    Style {
        id: StyleId::IrishPub,
        label: "Irish drinking song",
        meters: &[Six8, Four4],
        tempo: &[(Six8, 96, 116), (Four4, 120, 144)],
        modes: &[Major, Mixolydian],
        idiom: "rollicking and danceable, lineage in Irish and Scottish session fiddling (reels, polkas, jigs); I, IV, V with a bVII in Mixolydian; short words on the beat, a chorus the whole room can shout, and a fiddle break played like a dance tune",
        guitar: Strum,
        band: Band {
            harmony_guitar: true,
            violin: true,
            choir: true,
            harmonies: true,
            doubles: true,
            ..kit(Full)
        },
        forms: &[FormId::VcBreaks, FormId::ChorusFirst, FormId::Vc],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Clipped },
    },
    Style {
        id: StyleId::Welsh,
        label: "Welsh hymn tune",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 60, 78), (Three4, 66, 84)],
        modes: &[Major],
        idiom: "four-part hymn harmony, cadences on I, a lift to vi",
        guitar: Arpeggio,
        band: Band { harp: true, choir: true, harmonies: true, ..kit(DrumKit::None) },
        forms: &[FormId::Hymn],
        lead: BreakLead::Guitar,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Legato, endings: Endings::Released },
    },
    Style {
        id: StyleId::Breton,
        label: "Breton dance song",
        meters: &[Four4, Six8],
        tempo: &[(Four4, 104, 128), (Six8, 68, 84)],
        modes: &[Dorian, Minor],
        idiom: "modal drone harmony, call and response in the lines",
        guitar: Strum,
        band: Band { violin: true, harmonies: true, ..kit(Soft) },
        forms: &[FormId::Refrain, FormId::VcBreaks],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Blues,
        label: "Delta and Piedmont blues",
        meters: &[Four4],
        tempo: &[(Four4, 70, 100)],
        modes: &[Mixolydian, Major],
        idiom: "dominant-seventh harmony, I7, IV7, V7",
        guitar: Travis,
        band: Band { harmony_guitar: true, ..kit(Brushes) },
        forms: &[FormId::Blues12],
        lead: BreakLead::Guitar,
        duet: DuetFit::Rare,
        phrasing: Phrasing { delivery: Delivery::Parlando, endings: Endings::Released },
    },
    Style {
        id: StyleId::Gospel,
        label: "Gospel",
        meters: &[Four4, Six8],
        tempo: &[(Four4, 72, 104), (Six8, 50, 66)],
        modes: &[Major],
        idiom: "I, IV, V, vi with passing sevenths; a call and a response",
        guitar: Arpeggio,
        band: Band { choir: true, harmonies: true, doubles: true, ..kit(Soft) },
        forms: &[FormId::ChorusFirst, FormId::Vc, FormId::Prechorus],
        lead: BreakLead::Guitar,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Legato, endings: Endings::Held },
    },
    Style {
        id: StyleId::Revival,
        label: "1960s folk revival",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 92, 124), (Three4, 92, 116)],
        modes: &[Major, Minor],
        idiom: "three chords and the truth; a duo harmony on the refrain",
        guitar: Fingerpick,
        band: Band { bass: false, harmonies: true, ..kit(DrumKit::None) },
        forms: &[FormId::Refrain, FormId::Vc, FormId::Strophic],
        lead: BreakLead::Guitar,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Held },
    },
    Style {
        id: StyleId::Laurel,
        label: "Laurel Canyon",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 72, 100), (Three4, 80, 104)],
        modes: &[Major, Mixolydian],
        idiom: "open tunings and maj7, sus2, add9 colors; stacked harmony",
        guitar: Arpeggio,
        band: Band { harmony_guitar: true, harmonies: true, doubles: true, ..kit(Soft) },
        forms: &[FormId::Vc, FormId::Prechorus, FormId::Aaba],
        lead: BreakLead::Guitar,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Nashville,
        label: "Nashville country waltz",
        meters: &[Three4],
        tempo: &[(Three4, 84, 112)],
        modes: &[Major],
        idiom: "I, IV, V with a II7 and a walk-up; fiddle fills",
        guitar: Strum,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, ..kit(Brushes) },
        forms: &[FormId::WaltzBreaks, FormId::Vc],
        lead: BreakLead::Violin,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Held },
    },
    Style {
        id: StyleId::Americana,
        label: "Present-day Americana",
        meters: &[Four4, Three4],
        tempo: &[(Four4, 72, 112), (Three4, 84, 110)],
        modes: &[Major, Minor, Mixolydian],
        idiom: "open, ringing harmony; a vi or a bVII where it hurts or lifts",
        guitar: Strum,
        band: Band { harmony_guitar: true, violin: true, harmonies: true, doubles: true, ..kit(Soft) },
        forms: &[FormId::Vc, FormId::Prechorus, FormId::ChorusFirst, FormId::Aaba],
        lead: BreakLead::Both,
        duet: DuetFit::Welcome,
        phrasing: Phrasing { delivery: Delivery::Flowing, endings: Endings::Released },
    },
    Style {
        id: StyleId::Shanty,
        label: "Sea shanty",
        meters: &[Four4, Six8],
        tempo: &[(Four4, 96, 124), (Six8, 60, 76)],
        modes: &[Major, Dorian],
        idiom: "call and response: the shantyman sings a line, the crew answers with a short refrain line; I and V",
        guitar: Strum,
        band: Band {
            bass: false,
            violin: true,
            choir: true,
            harmonies: true,
            doubles: true,
            ..kit(DrumKit::None)
        },
        forms: &[FormId::Refrain, FormId::ChorusFirst],
        lead: BreakLead::Violin,
        duet: DuetFit::Occasional,
        phrasing: Phrasing { delivery: Delivery::Detached, endings: Endings::Clipped },
    },
];

/// World-folk flavours that may season a song (CLAUDE.md: world folk only
/// as seasoning).
pub const WORLD_FLAVOURS: &[&str] = &[
    "fado",
    "Cape Verdean coladeira",
    "Mexican son jarocho",
    "Tex-Mex conjunto",
    "French chanson",
    "klezmer",
    "Malian desert blues",
];

/// Probability that a song gets a world flavour.
pub const WORLD_FLAVOUR_P: f64 = 0.12;

/// A uniformly chosen element of `items`, or `fallback` when it is empty.
/// The one pick over `Rng` in this crate.
pub(crate) fn pick<T: Copy>(rng: &mut Rng, items: &[T], fallback: T) -> T {
    rng.pick(items).copied().unwrap_or(fallback)
}

/// The creative direction for one song: a style and the choices made
/// within it.
#[derive(Clone, Debug)]
pub struct Direction {
    pub style: StyleId,
    pub label: &'static str,
    pub idiom: &'static str,
    pub mode: Mode,
    pub meter: Meter,
    pub tempo_lo: u32,
    pub tempo_hi: u32,
    pub form: FormId,
    pub guitar: GuitarPattern,
    /// Band parts and drum kit.
    pub band: Band,
    pub lead: BreakLead,
    pub world: Option<&'static str>,
    /// How welcome a duet is in this style (design 4.7).
    pub duet: DuetFit,
    /// The style's default articulation (design 5.1).
    pub phrasing: Phrasing,
}

/// Chooses the direction: `style`, or a uniformly random style when
/// `None`; then meter, form and mode uniformly from the style's lists, and
/// with probability `WORLD_FLAVOUR_P` one world flavour.
pub fn style_direction(style: Option<StyleId>, rng: &mut Rng) -> Direction {
    let id = style.unwrap_or_else(|| pick(rng, StyleId::ALL, StyleId::Americana));
    let s = id.style();
    let meter = pick(rng, s.meters, Meter::Four4);
    let (tempo_lo, tempo_hi) = s.tempo_for(meter).unwrap_or((60, 120));
    let form = pick(rng, s.forms, FormId::Vc);
    let mode = pick(rng, s.modes, Mode::Major);
    let world = if rng.uniform() < WORLD_FLAVOUR_P {
        rng.pick(WORLD_FLAVOURS).copied()
    } else {
        None
    };
    Direction {
        style: id,
        label: s.label,
        idiom: s.idiom,
        mode,
        meter,
        tempo_lo,
        tempo_hi,
        form,
        guitar: s.guitar,
        band: s.band,
        lead: s.lead,
        world,
        duet: s.duet,
        phrasing: s.phrasing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song_in(meter: &str) -> Song {
        let raw = serde_json::json!({
            "meter": meter,
            "tempo": 100,
            "sections": [{"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]
        });
        song::normalize_value(&raw).expect("test song normalises").0
    }

    #[test]
    fn tables_are_indexed_by_id() {
        for (i, id) in StyleId::ALL.iter().enumerate() {
            assert_eq!(STYLES[i].id, *id);
            assert_eq!(id.as_str().parse::<StyleId>(), Ok(*id));
        }
        for (i, id) in FormId::ALL.iter().enumerate() {
            assert_eq!(FORMS[i].id, *id);
            assert_eq!(id.as_str().parse::<FormId>(), Ok(*id));
        }
    }

    #[test]
    fn every_style_is_complete() {
        for s in &STYLES {
            assert!(
                !s.meters.is_empty() && !s.modes.is_empty() && !s.forms.is_empty(),
                "{}",
                s.id
            );
            for &m in s.meters {
                let (lo, hi) = s.tempo_for(m).unwrap_or_else(|| panic!("{} {m}", s.id));
                assert!(lo < hi, "{} {m}", s.id);
            }
        }
        for f in &FORMS {
            assert!(!f.steps.is_empty(), "{}", f.id);
        }
    }

    #[test]
    fn plan_text_renders_roles() {
        assert_eq!(
            FormId::VcBreaks.form().plan_text(),
            "1. intro: instrumental break (the lead instrument plays the tune), 4 chord entries, no lines\n\
             2. verse 1: 4 lines, 2 chord entries per line\n\
             3. chorus 1: 4 lines, 2 chord entries per line\n\
             4. interlude: instrumental break (the lead instrument plays the tune), 8 chord entries, no lines\n\
             5. verse 2: 4 lines, 2 chord entries per line\n\
             6. chorus: repeat, written {\"type\":\"chorus\",\"same\":true}\n\
             7. interlude: instrumental break (the lead instrument plays the tune), 4 chord entries, no lines\n\
             8. chorus: repeat, written {\"type\":\"chorus\",\"same\":true}\n\
             9. outro: a tag of 1 to 2 lyric lines (a closing echo of the hook), 2 chord entries per line"
        );
        assert_eq!(
            FormId::Blues12.form().plan_text(),
            "1. intro: instrumental, 4 chord entries, no lines\n\
             2. verse 1: 3 lines, 4 chord entries per line\n\
             3. verse 2: 3 lines, 4 chord entries per line\n\
             4. interlude: instrumental break (the lead instrument plays the tune), 8 chord entries, no lines\n\
             5. verse 3: 3 lines, 4 chord entries per line\n\
             6. verse 4: 3 lines, 4 chord entries per line\n\
             7. outro: instrumental, 4 chord entries, no lines"
        );
        assert!(FormId::Hymn
            .form()
            .plan_text()
            .ends_with("7. outro: a tag of 1 lyric line (a closing echo of the hook), 2 chord entries per line"));
    }

    #[test]
    fn roles_are_exposed() {
        let steps = FormId::VcBreaks.form().steps;
        assert_eq!(
            steps
                .iter()
                .filter(|s| s.role == SectionRole::Break)
                .count(),
            3
        );
        assert_eq!(steps.last().map(|s| s.role), Some(SectionRole::Tag));
    }

    #[test]
    fn direction_stays_inside_the_style() {
        for seed in 0..200u64 {
            let mut rng = Rng::from_seed(seed);
            let d = style_direction(None, &mut rng);
            let s = d.style.style();
            assert!(
                s.meters.contains(&d.meter)
                    && s.modes.contains(&d.mode)
                    && s.forms.contains(&d.form)
            );
            assert_eq!(s.tempo_for(d.meter), Some((d.tempo_lo, d.tempo_hi)));
            let mut rng = Rng::from_seed(seed);
            assert_eq!(
                style_direction(Some(StyleId::Blues), &mut rng).form,
                FormId::Blues12
            );
        }
    }

    // Cowboy in 3/4 plays 80-104 bpm; the clamp is 72-114.4, rounded.
    #[test]
    fn apply_clamps_and_rounds_the_tempo() {
        for (t, want) in [(120.0, 114.0), (60.0, 72.0), (100.0, 100.0)] {
            let mut s = song_in("3/4");
            s.tempo_bpm = t;
            let rep = apply_style("cowboy", &mut s).expect("cowboy exists");
            assert_eq!(s.tempo_bpm, want, "tempo {t}");
            assert_eq!(rep.len(), usize::from(t != want));
            assert_eq!(s.style.as_deref(), Some("cowboy"));
            assert_eq!(s.break_lead, BreakLead::Violin);
            assert_eq!(s.guitar, GuitarPattern::Travis);
            assert_eq!(s.band.drums, DrumKit::Brushes);
        }
    }

    #[test]
    fn unknown_style_is_an_error() {
        let mut s = song_in("4/4");
        assert_eq!(
            apply_style("polka", &mut s),
            Err(UnknownStyle("polka".into()))
        );
        assert!(s.style.is_none());
        assert!(style("Cowboy").is_err());
        assert!("vcbreaks".parse::<FormId>().is_err());
    }
}

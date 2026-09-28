//! Ports src/styles.js: FORMS, STYLES, formText, styleDirection, and the
//! arrangement data applyStyle needs (without depending on the compose crate).
//!
//! Randomness for creative choices comes from a caller-supplied `&mut dyn FnMut() -> f64`
//! returning a value in [0, 1), matching `Math.random()` call sites in the JS 1-for-1.
//! This crate does not need JS bit parity for these draws (see crate docs), but the
//! call order and pick-from-list logic (`p=a=>a[Math.floor(Math.random()*a.length)]`)
//! is ported faithfully so behavior matches in distribution.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use song::{Band, BreakLead, DrumKit, GuitarPattern, Meter, Mode, Repair, Song};

/// One step of a form's plan, ports a `FORMS[k].steps` entry (`['type', {opts}]`).
#[derive(Clone, Debug)]
pub struct FormStep {
    pub kind: &'static str,
    /// instrumental step: number of chord entries, no lyric lines
    pub inst: Option<u32>,
    /// number of lyric lines (may be a fixed count string like "2 to 4")
    pub n: Option<&'static str>,
    /// step is a literal repeat of the previous same-type section
    pub same: bool,
    /// instrumental break: the lead instrument plays the tune
    pub brk: bool,
    /// outro tag: a short closing echo of the hook
    pub tag: bool,
}

impl FormStep {
    fn new(kind: &'static str) -> Self {
        FormStep { kind, inst: None, n: None, same: false, brk: false, tag: false }
    }
    fn inst(mut self, n: u32) -> Self {
        self.inst = Some(n);
        self
    }
    fn n(mut self, n: &'static str) -> Self {
        self.n = Some(n);
        self
    }
    fn same(mut self) -> Self {
        self.same = true;
        self
    }
    fn brk(mut self) -> Self {
        self.brk = true;
        self
    }
    fn tag(mut self) -> Self {
        self.tag = true;
        self
    }
}

/// Ports `FORMS[k]`.
#[derive(Clone, Debug)]
pub struct Form {
    pub label: &'static str,
    pub note: &'static str,
    pub bars: u32,
    pub steps: Vec<FormStep>,
}

macro_rules! form {
    ($label:expr, $note:expr, $bars:expr, [$($step:expr),* $(,)?]) => {
        Form { label: $label, note: $note, bars: $bars, steps: vec![$($step),*] }
    };
}

/// Ports FORMS. Keys match the JS object's keys exactly, and entries are kept in JS
/// declaration order (a Vec, not a sorted map) so a random pick over `Object.keys(FORMS)`
/// (there is none today, but `styleDirection`'s STYLES pick relies on the same ordering
/// discipline) matches JS insertion order rather than alphabetical order.
pub fn forms() -> &'static Vec<(&'static str, Form)> {
    static FORMS: LazyLock<Vec<(&'static str, Form)>> = LazyLock::new(build_forms);
    &FORMS
}

fn forms_get(fk: &str) -> Option<&'static Form> {
    forms().iter().find(|(k, _)| *k == fk).map(|(_, f)| f)
}

fn build_forms() -> Vec<(&'static str, Form)> {
    let mut m: Vec<(&'static str, Form)> = Vec::new();
    m.push((
        "vc",
        form!(
            "verse and chorus",
            "",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").same(),
                FormStep::new("bridge").n("2 to 4"),
                FormStep::new("chorus").same(),
                FormStep::new("outro").inst(3),
            ]
        ),
    ));
    m.push((
        "vcBreaks",
        form!(
            "verse and chorus with instrumental breaks",
            "",
            2,
            [
                FormStep::new("intro").inst(4).brk(),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").n("4"),
                FormStep::new("interlude").inst(8).brk(),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").same(),
                FormStep::new("interlude").inst(4).brk(),
                FormStep::new("chorus").same(),
                FormStep::new("outro").n("1 to 2").tag(),
            ]
        ),
    ));
    m.push((
        "strophic",
        form!(
            "strophic ballad",
            "No chorus. The story runs across the verses; each verse moves it forward.",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("interlude").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("outro").inst(2),
            ]
        ),
    ));
    m.push((
        "refrain",
        form!(
            "verses with a refrain line",
            "No chorus. Every verse ends with the same refrain line, word for word; the verses change what it means.",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("interlude").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("outro").inst(2),
            ]
        ),
    ));
    m.push((
        "aaba",
        form!(
            "AABA (32-bar song form)",
            "The A sections share one melody; the B section (bridge) contrasts in harmony and angle.",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("bridge").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("interlude").inst(4),
                FormStep::new("bridge").same(),
                FormStep::new("verse").n("4"),
                FormStep::new("outro").inst(2),
            ]
        ),
    ));
    m.push((
        "chorusFirst",
        form!(
            "chorus first",
            "Open cold on the chorus, then tell the story.",
            2,
            [
                FormStep::new("chorus").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").same(),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").same(),
                FormStep::new("chorus").same(),
                FormStep::new("outro").inst(3),
            ]
        ),
    ));
    m.push((
        "prechorus",
        form!(
            "verse, pre-chorus, chorus",
            "",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("prechorus").n("2"),
                FormStep::new("chorus").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("prechorus").same(),
                FormStep::new("chorus").same(),
                FormStep::new("bridge").n("2 to 4"),
                FormStep::new("chorus").same(),
                FormStep::new("outro").inst(3),
            ]
        ),
    ));
    m.push((
        "waltzBreaks",
        form!(
            "verse and chorus with a break",
            "",
            2,
            [
                FormStep::new("intro").inst(4).brk(),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").n("4"),
                FormStep::new("interlude").inst(4).brk(),
                FormStep::new("verse").n("4"),
                FormStep::new("chorus").same(),
                FormStep::new("chorus").same(),
                FormStep::new("outro").inst(3),
            ]
        ),
    ));
    m.push((
        "blues12",
        form!(
            "12-bar blues",
            "Each verse is three lines, and each of those lines spans FOUR chord entries (4 bars), making 12 bars: I7 I7 I7 I7 | IV7 IV7 I7 I7 | V7 IV7 I7 V7 (variants welcome). Line two repeats line one, perhaps with a small change; line three answers or twists it.",
            4,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("3"),
                FormStep::new("verse").n("3"),
                FormStep::new("interlude").inst(8).brk(),
                FormStep::new("verse").n("3"),
                FormStep::new("verse").n("3"),
                FormStep::new("outro").inst(4),
            ]
        ),
    ));
    m.push((
        "hymn",
        form!(
            "hymn stanzas",
            "Stanzas in a hymn meter (8.6.8.6 or 8.7.8.7 syllables per line), plainly sung; the tag is one closing line.",
            2,
            [
                FormStep::new("intro").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("interlude").inst(4),
                FormStep::new("verse").n("4"),
                FormStep::new("verse").n("4"),
                FormStep::new("outro").n("1").tag(),
            ]
        ),
    ));
    m
}

/// Ports one entry of STYLES.
#[derive(Clone, Debug)]
pub struct Style {
    pub label: &'static str,
    pub meters: &'static [Meter],
    /// Tempo range per meter: (meter, lo, hi) in beats per minute.
    pub tempo: &'static [(Meter, u32, u32)],
    pub modes: &'static [Mode],
    pub idiom: &'static str,
    pub guitar: GuitarPattern,
    /// Band parts and drum kit.
    pub band: Band,
    pub forms: &'static [&'static str],
    /// Lead instrument of instrumental breaks.
    pub lead: BreakLead,
}

impl Style {
    /// The style's tempo range for `meter`, if it plays in that meter.
    pub fn tempo_for(&self, meter: Meter) -> Option<(u32, u32)> {
        self.tempo.iter().find(|(m, _, _)| *m == meter).map(|(_, lo, hi)| (*lo, *hi))
    }

    /// Imposes the style's arrangement on `song`: guitar pattern, break
    /// lead, band parts and drum kit, and the tempo clamped to the style's
    /// range for the song's meter widened by 10% each way, then rounded.
    /// Returns a `ClampedTempo` repair when the tempo moves.
    pub fn apply(&self, song: &mut Song) -> Vec<Repair> {
        song.guitar = self.guitar;
        song.break_lead = self.lead;
        song.band = self.band;
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

/// A style id that names no style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStyle(pub String);

impl std::fmt::Display for UnknownStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown style {:?}", self.0)
    }
}

impl std::error::Error for UnknownStyle {}

/// The style with id `key`.
pub fn style(key: &str) -> Result<&'static Style, UnknownStyle> {
    styles_get(key).ok_or_else(|| UnknownStyle(key.to_string()))
}

/// Applies style `key` to `song` (see `Style::apply`) and records the id in
/// `song.style`.
pub fn apply_style(key: &str, song: &mut Song) -> Result<Vec<Repair>, UnknownStyle> {
    let s = style(key)?;
    let rep = s.apply(song);
    song.style = Some(key.to_string());
    Ok(rep)
}

/// Ports STYLES. Keys match the JS object's keys exactly.
pub fn styles() -> &'static Vec<(&'static str, Style)> {
    static STYLES: LazyLock<Vec<(&'static str, Style)>> = LazyLock::new(build_styles);
    &STYLES
}

fn styles_get(k: &str) -> Option<&'static Style> {
    styles().iter().find(|(sk, _)| *sk == k).map(|(_, s)| s)
}

fn build_styles() -> Vec<(&'static str, Style)> {
    let mut m: Vec<(&'static str, Style)> = Vec::new();
    // A style's band: the kit, the bass on, every other part off unless named.
    macro_rules! b {
        ($drums:expr $(; $($field:ident: $val:expr),* $(,)?)?) => {{
            #[allow(unused_mut)]
            let mut band = Band {
                drums: $drums,
                bass: true,
                harmony_guitar: false,
                harp: false,
                violin: false,
                choir: false,
                harmonies: false,
                doubles: false,
            };
            $($(band.$field = $val;)*)?
            band
        }};
    }
    m.push((
        "appalachian",
        Style {
            label: "Appalachian ballad",
            meters: &[Meter::Three4, Meter::Four4],
            tempo: &[(Meter::Three4, 72, 96), (Meter::Four4, 66, 88)],
            modes: &[Mode::Mixolydian, Mode::Dorian, Mode::Minor, Mode::Major],
            idiom: "modal and spare: two or three chords (I, bVII, IV or i, bVII, v), a long story sung plainly",
            guitar: GuitarPattern::Fingerpick,
            band: b!(DrumKit::None; bass: false, violin: true),
            forms: &["strophic", "refrain"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "oldtime",
        Style {
            label: "Old-time string band",
            meters: &[Meter::Four4],
            tempo: &[(Meter::Four4, 100, 128)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "I, IV, V with drive; the fiddle takes the breaks",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::None; harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vcBreaks", "refrain"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "bluegrass",
        Style {
            label: "Bluegrass",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 108, 140), (Meter::Three4, 100, 132)],
            modes: &[Mode::Major],
            idiom: "I, IV, V with the occasional II major; high lonesome harmony on the chorus; fiddle and guitar trade breaks",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::None; harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vcBreaks"],
            lead: BreakLead::Both,
        },
    ));
    m.push((
        "cowboy",
        Style {
            label: "Western and cowboy song",
            meters: &[Meter::Three4, Meter::Four4],
            tempo: &[(Meter::Three4, 80, 104), (Meter::Four4, 76, 100)],
            modes: &[Mode::Major],
            idiom: "open-range major harmony, I, IV, V and the II7 on the way home",
            guitar: GuitarPattern::Travis,
            band: b!(DrumKit::Brushes; harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vc", "waltzBreaks", "strophic"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "bakersfield",
        Style {
            label: "Bakersfield country",
            meters: &[Meter::Four4],
            tempo: &[(Meter::Four4, 112, 144)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "shuffle-driven I, IV, V with a bVII; a twangy guitar break",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Full; harmony_guitar: true, violin: true, harmonies: true, doubles: true),
            forms: &["vcBreaks", "vc", "chorusFirst"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "texas",
        Style {
            label: "Texas songwriter",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 78, 104), (Meter::Three4, 84, 108)],
            modes: &[Mode::Major, Mode::Minor],
            idiom: "plain chords, long verses, detail-heavy storytelling",
            guitar: GuitarPattern::Travis,
            band: b!(DrumKit::Brushes; harmony_guitar: true),
            forms: &["refrain", "strophic", "aaba"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "cajun",
        Style {
            label: "Cajun waltz",
            meters: &[Meter::Three4],
            tempo: &[(Meter::Three4, 104, 138)],
            modes: &[Mode::Major],
            idiom: "two or three chords (I, V, IV); the fiddle answers the voice",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Soft; violin: true, harmonies: true),
            forms: &["waltzBreaks"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "zydeco",
        Style {
            label: "Creole and zydeco two-step",
            meters: &[Meter::Four4],
            tempo: &[(Meter::Four4, 108, 132)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "I, IV, V with a push; a call you can shout back",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Full; harmony_guitar: true, violin: true, choir: true, harmonies: true, doubles: true),
            forms: &["chorusFirst", "vcBreaks"],
            lead: BreakLead::Both,
        },
    ));
    m.push((
        "acadian",
        Style {
            label: "Acadian fiddle song",
            meters: &[Meter::Six8, Meter::Three4],
            tempo: &[(Meter::Six8, 62, 80), (Meter::Three4, 96, 120)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "kitchen-party harmony, I, IV, V; the fiddle leads the dance",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Soft; violin: true, harmonies: true),
            forms: &["vcBreaks", "refrain"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "broadside",
        Style {
            label: "English broadside ballad",
            meters: &[Meter::Six8, Meter::Three4, Meter::Four4],
            tempo: &[(Meter::Six8, 52, 68), (Meter::Three4, 80, 104), (Meter::Four4, 76, 98)],
            modes: &[Mode::Major, Mode::Dorian, Mode::Mixolydian],
            idiom: "a tune for a news story or a scandal, simple diatonic harmony",
            guitar: GuitarPattern::Fingerpick,
            band: b!(DrumKit::None; bass: false, violin: true),
            forms: &["strophic", "refrain"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "scottish",
        Style {
            label: "Scottish ballad",
            meters: &[Meter::Three4, Meter::Four4],
            tempo: &[(Meter::Three4, 66, 88), (Meter::Four4, 60, 80)],
            modes: &[Mode::Dorian, Mode::Mixolydian, Mode::Minor],
            idiom: "modal and dignified: i, bVII, bVI or I, bVII",
            guitar: GuitarPattern::Arpeggio,
            band: b!(DrumKit::None; harp: true, violin: true),
            forms: &["strophic", "refrain", "aaba"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "irishair",
        Style {
            label: "Irish air",
            meters: &[Meter::Three4, Meter::Six8],
            tempo: &[(Meter::Three4, 60, 80), (Meter::Six8, 44, 60)],
            modes: &[Mode::Major, Mode::Dorian, Mode::Mixolydian],
            idiom: "a long-breathed tune over few chords; ornament rather than harmony",
            guitar: GuitarPattern::Arpeggio,
            band: b!(DrumKit::None; harp: true, violin: true),
            forms: &["strophic", "aaba"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "irishpub",
        Style {
            label: "Irish drinking song",
            meters: &[Meter::Six8, Meter::Four4],
            tempo: &[(Meter::Six8, 68, 84), (Meter::Four4, 104, 128)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "a chorus the whole room can shout, I, IV, V",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Soft; violin: true, choir: true, harmonies: true, doubles: true),
            forms: &["vc", "chorusFirst", "vcBreaks"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "welsh",
        Style {
            label: "Welsh hymn tune",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 60, 78), (Meter::Three4, 66, 84)],
            modes: &[Mode::Major],
            idiom: "four-part hymn harmony, cadences on I, a lift to vi",
            guitar: GuitarPattern::Arpeggio,
            band: b!(DrumKit::None; harp: true, choir: true, harmonies: true),
            forms: &["hymn"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "breton",
        Style {
            label: "Breton dance song",
            meters: &[Meter::Four4, Meter::Six8],
            tempo: &[(Meter::Four4, 104, 128), (Meter::Six8, 68, 84)],
            modes: &[Mode::Dorian, Mode::Minor],
            idiom: "modal drone harmony, call and response in the lines",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Soft; violin: true, harmonies: true),
            forms: &["refrain", "vcBreaks"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "blues",
        Style {
            label: "Delta and Piedmont blues",
            meters: &[Meter::Four4],
            tempo: &[(Meter::Four4, 70, 100)],
            modes: &[Mode::Mixolydian, Mode::Major],
            idiom: "dominant-seventh harmony, I7, IV7, V7",
            guitar: GuitarPattern::Travis,
            band: b!(DrumKit::Brushes; harmony_guitar: true),
            forms: &["blues12"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "gospel",
        Style {
            label: "Gospel",
            meters: &[Meter::Four4, Meter::Six8],
            tempo: &[(Meter::Four4, 72, 104), (Meter::Six8, 50, 66)],
            modes: &[Mode::Major],
            idiom: "I, IV, V, vi with passing sevenths; a call and a response",
            guitar: GuitarPattern::Arpeggio,
            band: b!(DrumKit::Soft; choir: true, harmonies: true, doubles: true),
            forms: &["chorusFirst", "vc", "prechorus"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "revival",
        Style {
            label: "1960s folk revival",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 92, 124), (Meter::Three4, 92, 116)],
            modes: &[Mode::Major, Mode::Minor],
            idiom: "three chords and the truth; a duo harmony on the refrain",
            guitar: GuitarPattern::Fingerpick,
            band: b!(DrumKit::None; bass: false, harmonies: true),
            forms: &["refrain", "vc", "strophic"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "laurel",
        Style {
            label: "Laurel Canyon",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 72, 100), (Meter::Three4, 80, 104)],
            modes: &[Mode::Major, Mode::Mixolydian],
            idiom: "open tunings and maj7, sus2, add9 colors; stacked harmony",
            guitar: GuitarPattern::Arpeggio,
            band: b!(DrumKit::Soft; harmony_guitar: true, harmonies: true, doubles: true),
            forms: &["vc", "prechorus", "aaba"],
            lead: BreakLead::Guitar,
        },
    ));
    m.push((
        "nashville",
        Style {
            label: "Nashville country waltz",
            meters: &[Meter::Three4],
            tempo: &[(Meter::Three4, 84, 112)],
            modes: &[Mode::Major],
            idiom: "I, IV, V with a II7 and a walk-up; fiddle fills",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Brushes; harmony_guitar: true, violin: true, harmonies: true),
            forms: &["waltzBreaks", "vc"],
            lead: BreakLead::Violin,
        },
    ));
    m.push((
        "americana",
        Style {
            label: "Present-day Americana",
            meters: &[Meter::Four4, Meter::Three4],
            tempo: &[(Meter::Four4, 72, 112), (Meter::Three4, 84, 110)],
            modes: &[Mode::Major, Mode::Minor, Mode::Mixolydian],
            idiom: "open, ringing harmony; a vi or a bVII where it hurts or lifts",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::Soft; harmony_guitar: true, violin: true, harmonies: true, doubles: true),
            forms: &["vc", "prechorus", "chorusFirst", "aaba"],
            lead: BreakLead::Both,
        },
    ));
    m.push((
        "shanty",
        Style {
            label: "Sea shanty",
            meters: &[Meter::Four4, Meter::Six8],
            tempo: &[(Meter::Four4, 96, 124), (Meter::Six8, 60, 76)],
            modes: &[Mode::Major, Mode::Dorian],
            idiom: "call and response: the shantyman sings a line, the crew answers with a short refrain line; I and V",
            guitar: GuitarPattern::Strum,
            band: b!(DrumKit::None; bass: false, choir: true, harmonies: true, doubles: true, violin: true),
            forms: &["refrain", "chorusFirst"],
            lead: BreakLead::Violin,
        },
    ));
    m
}

/// Ports `formText(fk)`: renders a numbered plan for a form key.
pub fn form_text(fk: &str) -> FormTextResult {
    let f = forms_get(fk).unwrap_or_else(|| panic!("unknown form: {}", fk));
    let bars = f.bars;
    let mut cnt: BTreeMap<&str, u32> = BTreeMap::new();
    let mut lines = Vec::new();
    for (i, step) in f.steps.iter().enumerate() {
        let i = i + 1;
        let c = cnt.entry(step.kind).or_insert(0);
        *c += 1;
        let nm = step.kind; // JS: t==='interlude'?'interlude':t -- identity either way
        let line = if let Some(inst) = step.inst {
            format!(
                "{}. {}: instrumental{}, {} chord entries, no lines",
                i,
                nm,
                if step.brk { " break (the lead instrument plays the tune)" } else { "" },
                inst
            )
        } else if step.same {
            format!("{}. {}: repeat, written {{\"type\":\"{}\",\"same\":true}}", i, nm, step.kind)
        } else if step.tag {
            let n = step.n.unwrap_or("");
            format!(
                "{}. outro: a tag of {} lyric line{} (a closing echo of the hook), {} chord entries per line",
                i,
                n,
                if n == "1" { "" } else { "s" },
                bars
            )
        } else {
            format!("{}. {} {}: {} lines, {} chord entries per line", i, nm, c, step.n.unwrap_or(""), bars)
        };
        lines.push(line);
    }
    FormTextResult { label: f.label, note: f.note, text: lines.join("\n"), bars }
}

#[derive(Clone, Debug)]
pub struct FormTextResult {
    pub label: &'static str,
    pub note: &'static str,
    pub text: String,
    pub bars: u32,
}

/// Ports `styleDirection(key)`. `rand` returns a value in [0, 1), one call per JS `Math.random()`
/// use site, in the same order: meter pick, form pick, mode pick, then the world-flavor roll
/// (and its pick, when the roll succeeds). If `key` names an unknown style, JS falls back to a
/// key picked at random from `Object.keys(STYLES)` -- ported the same way (one extra `rand()` call
/// in that fallback path only, matching `p(Object.keys(STYLES))`).
pub fn style_direction(key: Option<&str>, rand: &mut dyn FnMut() -> f64) -> Direction {
    let k: String = match key {
        Some(k) if styles_get(k).is_some() => k.to_string(),
        _ => {
            // JS: `p(Object.keys(STYLES))` -- Object.keys on an object literal with
            // string keys yields insertion (declaration) order, matched here by
            // picking over `styles()`'s Vec in that same order.
            let keys: Vec<&str> = styles().iter().map(|(k, _)| *k).collect();
            let idx = (rand() * keys.len() as f64).floor() as usize;
            keys[idx.min(keys.len() - 1)].to_string()
        }
    };
    let s = styles_get(k.as_str()).expect("style key resolved above must exist");
    let meter = pick(s.meters, rand);
    // Every style lists a tempo range for each of its meters.
    let (lo, hi) = s.tempo_for(meter).unwrap_or((60, 120));
    let form = pick(s.forms, rand);
    let mode = pick(s.modes, rand);
    let world = if rand() < 0.12 {
        Some(pick(
            &[
                "fado",
                "Cape Verdean coladeira",
                "Mexican son jarocho",
                "Tex-Mex conjunto",
                "French chanson",
                "klezmer",
                "Malian desert blues",
            ],
            rand,
        ))
    } else {
        None
    };
    Direction {
        style: k,
        label: s.label,
        idiom: s.idiom,
        mode,
        meter,
        tempo_lo: lo,
        tempo_hi: hi,
        form,
        guitar: s.guitar,
        band: s.band,
        lead: s.lead,
        world,
    }
}

fn pick<'a, T: Copy>(a: &'a [T], rand: &mut dyn FnMut() -> f64) -> T {
    let idx = (rand() * a.len() as f64).floor() as usize;
    a[idx.min(a.len() - 1)]
}

/// Result of `styleDirection`.
#[derive(Clone, Debug)]
pub struct Direction {
    pub style: String,
    pub label: &'static str,
    pub idiom: &'static str,
    pub mode: Mode,
    pub meter: Meter,
    pub tempo_lo: u32,
    pub tempo_hi: u32,
    pub form: &'static str,
    pub guitar: GuitarPattern,
    /// Band parts and drum kit.
    pub band: Band,
    pub lead: BreakLead,
    pub world: Option<&'static str>,
}


#[cfg(test)]
mod parity_tests {
    use super::*;

    #[test]
    fn form_text_matches_js_reference() {
        let r = form_text("vcBreaks");
        assert_eq!(r.text, "1. intro: instrumental break (the lead instrument plays the tune), 4 chord entries, no lines\n2. verse 1: 4 lines, 2 chord entries per line\n3. chorus 1: 4 lines, 2 chord entries per line\n4. interlude: instrumental break (the lead instrument plays the tune), 8 chord entries, no lines\n5. verse 2: 4 lines, 2 chord entries per line\n6. chorus: repeat, written {\"type\":\"chorus\",\"same\":true}\n7. interlude: instrumental break (the lead instrument plays the tune), 4 chord entries, no lines\n8. chorus: repeat, written {\"type\":\"chorus\",\"same\":true}\n9. outro: a tag of 1 to 2 lyric lines (a closing echo of the hook), 2 chord entries per line");

        let r2 = form_text("blues12");
        assert_eq!(r2.text, "1. intro: instrumental, 4 chord entries, no lines\n2. verse 1: 3 lines, 4 chord entries per line\n3. verse 2: 3 lines, 4 chord entries per line\n4. interlude: instrumental break (the lead instrument plays the tune), 8 chord entries, no lines\n5. verse 3: 3 lines, 4 chord entries per line\n6. verse 4: 3 lines, 4 chord entries per line\n7. outro: instrumental, 4 chord entries, no lines");

        let r3 = form_text("hymn");
        assert_eq!(r3.text, "1. intro: instrumental, 4 chord entries, no lines\n2. verse 1: 4 lines, 2 chord entries per line\n3. verse 2: 4 lines, 2 chord entries per line\n4. interlude: instrumental, 4 chord entries, no lines\n5. verse 3: 4 lines, 2 chord entries per line\n6. verse 4: 4 lines, 2 chord entries per line\n7. outro: a tag of 1 lyric line (a closing echo of the hook), 2 chord entries per line");
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
    fn every_style_meter_has_a_tempo_range() {
        for (k, s) in styles() {
            for &m in s.meters {
                assert!(s.tempo_for(m).is_some(), "{k} {m}");
            }
            for f in s.forms {
                assert!(forms_get(f).is_some(), "{k} form {f}");
            }
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
        assert_eq!(apply_style("polka", &mut s), Err(UnknownStyle("polka".into())));
        assert!(s.style.is_none());
    }
}

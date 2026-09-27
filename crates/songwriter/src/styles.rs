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

/// Ports `B(o)`: the band-toggle default object, keys in JS insertion order.
#[derive(Clone, Debug, Copy)]
pub struct Band {
    pub bass: bool,
    pub harmony_guitar: bool,
    pub harp: bool,
    pub violin: bool,
    pub choir: bool,
    pub harmonies: bool,
    pub doubles: bool,
}

impl Default for Band {
    fn default() -> Self {
        Band {
            bass: true,
            harmony_guitar: false,
            harp: false,
            violin: false,
            choir: false,
            harmonies: false,
            doubles: false,
        }
    }
}

/// Ports one entry of STYLES.
#[derive(Clone, Debug)]
pub struct Style {
    pub label: &'static str,
    pub meters: &'static [&'static str],
    /// tempo range per meter: (meter, lo, hi), JS `tempo:{'3/4':[lo,hi],...}`
    pub tempo: &'static [(&'static str, u32, u32)],
    pub modes: &'static [&'static str],
    pub idiom: &'static str,
    pub guitar: &'static str,
    pub drums: &'static str,
    pub band: Band,
    pub forms: &'static [&'static str],
    /// break-instrument lead: "violin", "guitar", or "both"
    pub lead: &'static str,
}

impl Style {
    fn tempo_for(&self, meter: &str) -> Option<(u32, u32)> {
        self.tempo.iter().find(|(m, _, _)| *m == meter).map(|(_, lo, hi)| (*lo, *hi))
    }
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
    macro_rules! b {
        () => {
            Band::default()
        };
        ($($field:ident: $val:expr),* $(,)?) => {{
            let mut band = Band::default();
            $(band.$field = $val;)*
            band
        }};
    }
    m.push((
        "appalachian",
        Style {
            label: "Appalachian ballad",
            meters: &["3/4", "4/4"],
            tempo: &[("3/4", 72, 96), ("4/4", 66, 88)],
            modes: &["mixolydian", "dorian", "minor", "major"],
            idiom: "modal and spare: two or three chords (I, bVII, IV or i, bVII, v), a long story sung plainly",
            guitar: "fingerpick",
            drums: "none",
            band: b!(bass: false, violin: true),
            forms: &["strophic", "refrain"],
            lead: "violin",
        },
    ));
    m.push((
        "oldtime",
        Style {
            label: "Old-time string band",
            meters: &["4/4"],
            tempo: &[("4/4", 100, 128)],
            modes: &["major", "mixolydian"],
            idiom: "I, IV, V with drive; the fiddle takes the breaks",
            guitar: "strum",
            drums: "none",
            band: b!(harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vcBreaks", "refrain"],
            lead: "violin",
        },
    ));
    m.push((
        "bluegrass",
        Style {
            label: "Bluegrass",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 108, 140), ("3/4", 100, 132)],
            modes: &["major"],
            idiom: "I, IV, V with the occasional II major; high lonesome harmony on the chorus; fiddle and guitar trade breaks",
            guitar: "strum",
            drums: "none",
            band: b!(harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vcBreaks"],
            lead: "both",
        },
    ));
    m.push((
        "cowboy",
        Style {
            label: "Western and cowboy song",
            meters: &["3/4", "4/4"],
            tempo: &[("3/4", 80, 104), ("4/4", 76, 100)],
            modes: &["major"],
            idiom: "open-range major harmony, I, IV, V and the II7 on the way home",
            guitar: "travis",
            drums: "brushes",
            band: b!(harmony_guitar: true, violin: true, harmonies: true),
            forms: &["vc", "waltzBreaks", "strophic"],
            lead: "violin",
        },
    ));
    m.push((
        "bakersfield",
        Style {
            label: "Bakersfield country",
            meters: &["4/4"],
            tempo: &[("4/4", 112, 144)],
            modes: &["major", "mixolydian"],
            idiom: "shuffle-driven I, IV, V with a bVII; a twangy guitar break",
            guitar: "strum",
            drums: "full",
            band: b!(harmony_guitar: true, violin: true, harmonies: true, doubles: true),
            forms: &["vcBreaks", "vc", "chorusFirst"],
            lead: "guitar",
        },
    ));
    m.push((
        "texas",
        Style {
            label: "Texas songwriter",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 78, 104), ("3/4", 84, 108)],
            modes: &["major", "minor"],
            idiom: "plain chords, long verses, detail-heavy storytelling",
            guitar: "travis",
            drums: "brushes",
            band: b!(harmony_guitar: true),
            forms: &["refrain", "strophic", "aaba"],
            lead: "guitar",
        },
    ));
    m.push((
        "cajun",
        Style {
            label: "Cajun waltz",
            meters: &["3/4"],
            tempo: &[("3/4", 104, 138)],
            modes: &["major"],
            idiom: "two or three chords (I, V, IV); the fiddle answers the voice",
            guitar: "strum",
            drums: "soft",
            band: b!(violin: true, harmonies: true),
            forms: &["waltzBreaks"],
            lead: "violin",
        },
    ));
    m.push((
        "zydeco",
        Style {
            label: "Creole and zydeco two-step",
            meters: &["4/4"],
            tempo: &[("4/4", 108, 132)],
            modes: &["major", "mixolydian"],
            idiom: "I, IV, V with a push; a call you can shout back",
            guitar: "strum",
            drums: "full",
            band: b!(harmony_guitar: true, violin: true, choir: true, harmonies: true, doubles: true),
            forms: &["chorusFirst", "vcBreaks"],
            lead: "both",
        },
    ));
    m.push((
        "acadian",
        Style {
            label: "Acadian fiddle song",
            meters: &["6/8", "3/4"],
            tempo: &[("6/8", 62, 80), ("3/4", 96, 120)],
            modes: &["major", "mixolydian"],
            idiom: "kitchen-party harmony, I, IV, V; the fiddle leads the dance",
            guitar: "strum",
            drums: "soft",
            band: b!(violin: true, harmonies: true),
            forms: &["vcBreaks", "refrain"],
            lead: "violin",
        },
    ));
    m.push((
        "broadside",
        Style {
            label: "English broadside ballad",
            meters: &["6/8", "3/4", "4/4"],
            tempo: &[("6/8", 52, 68), ("3/4", 80, 104), ("4/4", 76, 98)],
            modes: &["major", "dorian", "mixolydian"],
            idiom: "a tune for a news story or a scandal, simple diatonic harmony",
            guitar: "fingerpick",
            drums: "none",
            band: b!(bass: false, violin: true),
            forms: &["strophic", "refrain"],
            lead: "violin",
        },
    ));
    m.push((
        "scottish",
        Style {
            label: "Scottish ballad",
            meters: &["3/4", "4/4"],
            tempo: &[("3/4", 66, 88), ("4/4", 60, 80)],
            modes: &["dorian", "mixolydian", "minor"],
            idiom: "modal and dignified: i, bVII, bVI or I, bVII",
            guitar: "arpeggio",
            drums: "none",
            band: b!(harp: true, violin: true),
            forms: &["strophic", "refrain", "aaba"],
            lead: "violin",
        },
    ));
    m.push((
        "irishair",
        Style {
            label: "Irish air",
            meters: &["3/4", "6/8"],
            tempo: &[("3/4", 60, 80), ("6/8", 44, 60)],
            modes: &["major", "dorian", "mixolydian"],
            idiom: "a long-breathed tune over few chords; ornament rather than harmony",
            guitar: "arpeggio",
            drums: "none",
            band: b!(harp: true, violin: true),
            forms: &["strophic", "aaba"],
            lead: "violin",
        },
    ));
    m.push((
        "irishpub",
        Style {
            label: "Irish drinking song",
            meters: &["6/8", "4/4"],
            tempo: &[("6/8", 68, 84), ("4/4", 104, 128)],
            modes: &["major", "mixolydian"],
            idiom: "a chorus the whole room can shout, I, IV, V",
            guitar: "strum",
            drums: "soft",
            band: b!(violin: true, choir: true, harmonies: true, doubles: true),
            forms: &["vc", "chorusFirst", "vcBreaks"],
            lead: "violin",
        },
    ));
    m.push((
        "welsh",
        Style {
            label: "Welsh hymn tune",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 60, 78), ("3/4", 66, 84)],
            modes: &["major"],
            idiom: "four-part hymn harmony, cadences on I, a lift to vi",
            guitar: "arpeggio",
            drums: "none",
            band: b!(harp: true, choir: true, harmonies: true),
            forms: &["hymn"],
            lead: "guitar",
        },
    ));
    m.push((
        "breton",
        Style {
            label: "Breton dance song",
            meters: &["4/4", "6/8"],
            tempo: &[("4/4", 104, 128), ("6/8", 68, 84)],
            modes: &["dorian", "minor"],
            idiom: "modal drone harmony, call and response in the lines",
            guitar: "strum",
            drums: "soft",
            band: b!(violin: true, harmonies: true),
            forms: &["refrain", "vcBreaks"],
            lead: "violin",
        },
    ));
    m.push((
        "blues",
        Style {
            label: "Delta and Piedmont blues",
            meters: &["4/4"],
            tempo: &[("4/4", 70, 100)],
            modes: &["mixolydian", "major"],
            idiom: "dominant-seventh harmony, I7, IV7, V7",
            guitar: "travis",
            drums: "brushes",
            band: b!(harmony_guitar: true),
            forms: &["blues12"],
            lead: "guitar",
        },
    ));
    m.push((
        "gospel",
        Style {
            label: "Gospel",
            meters: &["4/4", "6/8"],
            tempo: &[("4/4", 72, 104), ("6/8", 50, 66)],
            modes: &["major"],
            idiom: "I, IV, V, vi with passing sevenths; a call and a response",
            guitar: "arpeggio",
            drums: "soft",
            band: b!(choir: true, harmonies: true, doubles: true),
            forms: &["chorusFirst", "vc", "prechorus"],
            lead: "guitar",
        },
    ));
    m.push((
        "revival",
        Style {
            label: "1960s folk revival",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 92, 124), ("3/4", 92, 116)],
            modes: &["major", "minor"],
            idiom: "three chords and the truth; a duo harmony on the refrain",
            guitar: "fingerpick",
            drums: "none",
            band: b!(bass: false, harmonies: true),
            forms: &["refrain", "vc", "strophic"],
            lead: "guitar",
        },
    ));
    m.push((
        "laurel",
        Style {
            label: "Laurel Canyon",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 72, 100), ("3/4", 80, 104)],
            modes: &["major", "mixolydian"],
            idiom: "open tunings and maj7, sus2, add9 colors; stacked harmony",
            guitar: "arpeggio",
            drums: "soft",
            band: b!(harmony_guitar: true, harmonies: true, doubles: true),
            forms: &["vc", "prechorus", "aaba"],
            lead: "guitar",
        },
    ));
    m.push((
        "nashville",
        Style {
            label: "Nashville country waltz",
            meters: &["3/4"],
            tempo: &[("3/4", 84, 112)],
            modes: &["major"],
            idiom: "I, IV, V with a II7 and a walk-up; fiddle fills",
            guitar: "strum",
            drums: "brushes",
            band: b!(harmony_guitar: true, violin: true, harmonies: true),
            forms: &["waltzBreaks", "vc"],
            lead: "violin",
        },
    ));
    m.push((
        "americana",
        Style {
            label: "Present-day Americana",
            meters: &["4/4", "3/4"],
            tempo: &[("4/4", 72, 112), ("3/4", 84, 110)],
            modes: &["major", "minor", "mixolydian"],
            idiom: "open, ringing harmony; a vi or a bVII where it hurts or lifts",
            guitar: "strum",
            drums: "soft",
            band: b!(harmony_guitar: true, violin: true, harmonies: true, doubles: true),
            forms: &["vc", "prechorus", "chorusFirst", "aaba"],
            lead: "both",
        },
    ));
    m.push((
        "shanty",
        Style {
            label: "Sea shanty",
            meters: &["4/4", "6/8"],
            tempo: &[("4/4", 96, 124), ("6/8", 60, 76)],
            modes: &["major", "dorian"],
            idiom: "call and response: the shantyman sings a line, the crew answers with a short refrain line; I and V",
            guitar: "strum",
            drums: "none",
            band: b!(bass: false, choir: true, harmonies: true, doubles: true, violin: true),
            forms: &["refrain", "chorusFirst"],
            lead: "violin",
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
    let (lo, hi) = s.tempo_for(meter).expect("style tempo missing for meter");
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
        drums: s.drums,
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
    pub mode: &'static str,
    pub meter: &'static str,
    pub tempo_lo: u32,
    pub tempo_hi: u32,
    pub form: &'static str,
    pub guitar: &'static str,
    pub drums: &'static str,
    pub band: Band,
    pub lead: &'static str,
    pub world: Option<&'static str>,
}

/// The arrangement data `applyStyle(song,key)` imposes on a normalized song: guitar
/// pattern, break-lead instrument, and the full band (with drums folded in), plus the
/// tempo clamp range. The caller applies these to its own song representation; this
/// crate does not depend on the compose crate's song type.
#[derive(Clone, Debug)]
pub struct StyleArrangement {
    pub guitar: &'static str,
    pub break_lead: &'static str,
    pub band: Band,
    pub drums: &'static str,
    /// JS: `clamp(song.tempo, r[0]*0.9, r[1]*1.1)` for the given meter, if the style
    /// declares a tempo range for it.
    pub tempo_clamp: Option<(f64, f64)>,
}

/// Ports the data half of `applyStyle(song,key)`: guitar, breakLead, band (with drums),
/// and the tempo clamp bounds for `meter`. JS parity: `applyStyle` returns `song`
/// unchanged (not this struct) when `key` is not a known style; here that is `None`.
pub fn apply_style(key: &str, meter: &str) -> Option<StyleArrangement> {
    let s = styles_get(key)?;
    let tempo_clamp = s.tempo_for(meter).map(|(lo, hi)| (lo as f64 * 0.9, hi as f64 * 1.1));
    Some(StyleArrangement {
        guitar: s.guitar,
        break_lead: s.lead,
        band: s.band,
        drums: s.drums,
        tempo_clamp,
    })
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

//! The arranger pass (design section 8a): an optional second arranging
//! step, run by a model outside the engine. The rule-based arranger
//! (`arrange_song`) arranges first; a reader (Claude, through
//! `songwriter::arranger`) then sees a compact text view of the result
//! (`view`), and returns edits (`edit_schema` is their JSON schema). This
//! module holds the pure parts: the view, the edit reader, the validation
//! and `apply`, which changes a `Performance` in place. It makes no model
//! call and draws no random number; the same performance and the same edits
//! always give the same new performance. The saved performance file is the
//! recording's source.
//!
//! Units. The view and the edits count in beats, as `compose::timeline`
//! does: a beat is a quarter note in 4/4 and 3/4 and a dotted quarter in
//! 6/8. Bars are the bars of the played form (`Form::bars`; a stretched song
//! has twice the written bars). An event belongs to the bar of its onset,
//! the onset taken to the nearest 1/24 beat, so a drum hit that jitter
//! moved 4 ms before a downbeat still belongs to the bar that begins there.
//! `apply` converts beats to seconds with the song's `Timeline`, so rubato
//! and the closing ritard are kept.
//!
//! Editable parts: drums, bass, harp, violin and the harmony guitar's lead
//! notes (notes over bar ranges); the accompaniment guitar (a stroke pattern
//! per bar range); the lead vocal (pitch, onset and length of its notes,
//! never its words); and, per section, the harmony, the doubles and the
//! choir. The harmony guitar's arpeggio is not editable.
//!
//! Order of work. Lead edits change the composed lead (`Prepared::comp`),
//! and the vocal parts are planned again from it (`arrange::vocals::plan`),
//! with the section-level overrides; the guitar is planned again with the
//! edited bars' strokes (`arrange::guitar::plan_with`). Every other part is
//! a direct replacement of its events.
//!
//! Repairs. A reply that is wrong in a small way is repaired and each repair
//! is a line of text in `Applied::repairs`: bars outside the song are
//! dropped or clamped, velocity is clamped to 0..1, a pitch outside the
//! instrument's range is moved by octaves into it, a note with a missing
//! length or velocity takes a default. Anything that cannot be read is
//! dropped, never guessed.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use arrange::guitar::{self, Dir, GuitarStroke, Strings};
use arrange::vocals::{ChoirMode, HarmonyInterval, Overrides, SectionHarmony};
use compose::melody::LeadNote;
use compose::prepare::Prepared;
use compose::timeline::Timeline;
use serde_json::{json, Map, Value};
use song::events::{BowNote, DrumHit, DrumKind, PluckNote};
use song::{ChoirVoicing, DrumKit, Song, MELISMA_MAX_NOTES};

use crate::mixset::{DUCK_DB_MAX, DUCK_DB_MIN, GAIN_DB_MAX, GAIN_DB_MIN};
use crate::render::Performance;
use crate::track::TrackId;

/// Onsets and lengths are taken to this many steps per beat (16ths,
/// triplets and 32nds all land on a step).
const STEPS: f64 = 24.0;
/// Shortest note, beats.
const MIN_LEN: f64 = 1.0 / STEPS;
/// Longest note, beats.
const MAX_LEN: f64 = 64.0;
/// Velocity a note takes when the reply gives none.
const DEFAULT_VEL: f64 = 0.7;

/// The parts edited as notes over a bar range, by the names the edits use.
pub const PARTS: [&str; 5] = ["drums", "bass", "violin", "harmony_guitar", "harp"];
/// Every name an edit's `part` may take.
pub const EDIT_PARTS: [&str; 10] = [
    "drums",
    "bass",
    "violin",
    "harmony_guitar",
    "harp",
    "guitar",
    "lead",
    "harmony",
    "doubles",
    "choir",
];
/// The choir modes of a choir edit.
pub const CHOIR_MODES: [&str; 4] = ["off", "pad", "unison", "block"];
/// Drum kind names, as `song::events::DrumKind` spells them.
pub const DRUM_KINDS: [&str; 9] = [
    "Kick", "Snare", "Rim", "Tap", "Swish", "Hat", "Shaker", "Tom", "Ride",
];

/// MIDI range of each pitched part: bass E1 to C4, violin G3 to E7, the
/// harmony guitar E2 to E6.
const BASS_RANGE: (f64, f64) = (28.0, 60.0);
const VIOLIN_RANGE: (f64, f64) = (55.0, 100.0);
const HG_RANGE: (f64, f64) = (40.0, 88.0);
const HARP_RANGE: (f64, f64) = (36.0, 96.0);

/// A part whose events an edit replaces note by note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Drums,
    Bass,
    Violin,
    HarmonyGuitar,
    Harp,
}

impl Part {
    fn parse(s: &str) -> Option<Part> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace(['-', ' '], "_")
            .as_str()
        {
            "drums" => Some(Part::Drums),
            "bass" => Some(Part::Bass),
            "violin" => Some(Part::Violin),
            "harmony_guitar" | "harmonyguitar" | "hguitar" => Some(Part::HarmonyGuitar),
            "harp" => Some(Part::Harp),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Part::Drums => "drums",
            Part::Bass => "bass",
            Part::Violin => "violin",
            Part::HarmonyGuitar => "harmony_guitar",
            Part::Harp => "harp",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            Part::Bass => BASS_RANGE,
            Part::Violin => VIOLIN_RANGE,
            Part::Harp => HARP_RANGE,
            _ => HG_RANGE,
        }
    }
}

/// The sections an edit of the vocal parts names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SectionPart {
    Harmony,
    Doubles,
    Choir,
}

/// What an edit's `part` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Notes(Part),
    Guitar,
    Lead,
    Sections(SectionPart),
}

impl Target {
    fn parse(s: &str) -> Option<Target> {
        if let Some(p) = Part::parse(s) {
            return Some(Target::Notes(p));
        }
        match s.trim().to_ascii_lowercase().as_str() {
            "guitar" | "rhythm_guitar" => Some(Target::Guitar),
            "lead" | "vocal" | "lead_vocal" => Some(Target::Lead),
            "harmony" => Some(Target::Sections(SectionPart::Harmony)),
            "doubles" => Some(Target::Sections(SectionPart::Doubles)),
            "choir" => Some(Target::Sections(SectionPart::Choir)),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ view

/// Formats `x` with at most 3 decimals and no trailing zeros.
fn num(x: f64) -> String {
    let s = format!("{x:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "" | "-0" => "0".to_string(),
        s => s.to_string(),
    }
}

/// `x` rounded to the nearest step.
fn quant(x: f64) -> f64 {
    (x * STEPS).round() / STEPS
}

/// The bar of the event at `t` seconds and its beat inside that bar, both
/// after quantising the onset. The bar is negative before the song starts.
fn locate(tl: &Timeline, bpb: usize, t: f64) -> (i64, f64) {
    let beat = quant(tl.to_beat(t));
    let bar = (beat / bpb as f64).floor();
    (bar as i64, beat - bar * bpb as f64)
}

/// Length in beats of a note from `t0` to `t1` seconds for the view: to the
/// nearest twelfth of a beat (the planners trim notes by a breath, and a
/// length like 1.458 would only mislead), at least one twelfth.
fn len_beats(tl: &Timeline, t0: f64, t1: f64) -> f64 {
    ((tl.to_beat(t1) - tl.to_beat(t0)) * 12.0).round().max(1.0) / 12.0
}

fn kind_text(k: DrumKind, tl: &Timeline, t: f64) -> String {
    match k {
        DrumKind::Kick => "Kick".into(),
        DrumKind::Snare => "Snare".into(),
        DrumKind::Rim => "Rim".into(),
        DrumKind::Tap => "Tap".into(),
        DrumKind::Hat => "Hat".into(),
        DrumKind::Shaker => "Shaker".into(),
        DrumKind::Ride => "Ride".into(),
        DrumKind::Tom { hz } => format!("Tom({})", num(hz as f64)),
        DrumKind::Swish { dur } => format!("Swish({})", num(len_beats(tl, t, t + dur as f64))),
    }
}

/// The text the model reads. See `songwriter::arranger` for the prompt that
/// explains it.
pub fn view(song: &Song, prepared: &Prepared, perf: &Performance) -> String {
    let form = &prepared.form;
    let tl = &prepared.timeline;
    let bpb = form.bpb() as usize;
    let nbars = form.bars.len();
    let arr = &perf.arrangement;
    let flats = song.mode.prefers_flats(song::Pc::new(prepared.tonic));

    let mut o = String::new();
    let _ = writeln!(o, "SONG: {}", song.title);
    let unit = if song.meter == song::Meter::Six8 {
        "a dotted quarter note"
    } else {
        "a quarter note"
    };
    let _ = writeln!(
        o,
        "meter {}: {bpb} beats per bar, a beat is {unit}; tempo {} bpm (the felt beat); {nbars} bars, numbered from 0.",
        song.meter,
        num(song.tempo_bpm),
    );
    let _ = writeln!(
        o,
        "key after transposition for the singer: {} {} (tonic pitch class {}; its MIDI note nearest above middle C is {}). All MIDI numbers below are in this key.",
        song::Pc::new(prepared.tonic).name(flats),
        song.mode,
        prepared.tonic,
        60 + prepared.tonic,
    );
    let band = &song.band;
    let on = |b: bool| if b { "on" } else { "off" };
    let kit = match arr.drums {
        Some(_) => band.drums.to_string(),
        None => "none".to_string(),
    };
    let _ = writeln!(
        o,
        "band: drums {kit}, bass {}, violin {}, harmony_guitar {}, harp {}, harmony singer {}, doubles {}, choir {}. The rhythm guitar and the lead vocal always play. A part that is off cannot be edited.",
        on(band.bass),
        on(band.violin),
        on(band.harmony_guitar),
        on(band.harp),
        on(band.harmonies),
        on(band.doubles),
        on(band.choir),
    );

    let _ = writeln!(
        o,
        "\nSECTIONS (energy is how hard the band plays: quiet, low, mid, high)"
    );
    for (i, s) in form.sections.iter().enumerate() {
        let mut line = format!(
            "s{i} {} bars {}-{} ({} bars) energy {}",
            s.kind,
            s.start_bar,
            s.start_bar + s.n_bars.max(1) - 1,
            s.n_bars,
            ["quiet", "low", "mid", "high"][s.intensity.level() as usize],
        );
        if s.role != song::SectionRole::Plain {
            let _ = write!(line, ", role {}", s.role);
        }
        if s.is_lift() {
            line.push_str(", lifted (carries the hook)");
        }
        let _ = write!(line, ", key {} {}", s.key.0.name(flats), s.key.1);
        let st = section_state(prepared, i);
        if band.harmonies {
            let _ = write!(line, ", harmony {}", on(st.harmony));
        }
        if band.doubles {
            let _ = write!(line, ", doubles {}", on(st.doubles));
        }
        if band.choir {
            let _ = write!(line, ", choir {}", st.choir);
        }
        let _ = writeln!(o, "{line}");
    }

    // Events by bar, for each part.
    let mut lead: Vec<Vec<String>> = vec![Vec::new(); nbars];
    let lidx = lead_index(prepared);
    for (k, n) in prepared.comp.lead.iter().enumerate() {
        let Some(sn) = lidx.syl_of[k] else { continue };
        let (b, beat) = locate(tl, bpb, n.t0);
        if let Some(v) = usize::try_from(b).ok().and_then(|b| lead.get_mut(b)) {
            let singer = if prepared.voice_b.is_some() && n.singer == song::SingerId::B {
                " [B]"
            } else {
                ""
            };
            let at = format!(
                "{}+{} {}{singer}",
                num(beat),
                num(n.dur),
                num(n.midi as f64)
            );
            v.push(if n.syl.is_continuation() {
                format!("#{sn}~ {at}")
            } else {
                format!("#{sn} \"{}\" {at}", n.syl.text)
            });
        }
    }
    let pitched = |notes: &mut dyn Iterator<Item = (f64, f64, f32, f32)>| {
        let mut v: Vec<Vec<String>> = vec![Vec::new(); nbars];
        for (t0, t1, midi, vel) in notes {
            let (b, beat) = locate(tl, bpb, t0);
            if let Some(slot) = usize::try_from(b).ok().and_then(|b| v.get_mut(b)) {
                slot.push(format!(
                    "{}+{} {} {}",
                    num(beat),
                    num(len_beats(tl, t0, t1)),
                    num(midi as f64),
                    num(round2(vel as f64)),
                ));
            }
        }
        v
    };
    // A part the band leaves out is planned but never heard: not shown.
    let bass = pitched(
        &mut arr
            .bass
            .iter()
            .filter(|_| band.bass)
            .map(|n| (n.t0, n.t1, n.midi, n.vel)),
    );
    let violin = pitched(
        &mut arr
            .violin
            .iter()
            .filter(|_| band.violin)
            .map(|n| (n.t0, n.t1, n.midi, n.vel)),
    );
    let hg = pitched(
        &mut arr
            .harmony_guitar
            .lead
            .iter()
            .filter(|_| band.harmony_guitar)
            .map(|n| (n.t0, n.t1, n.midi, n.vel)),
    );
    // The harp rings: its length is the instrument's, so none is shown.
    let mut harp: Vec<Vec<String>> = vec![Vec::new(); nbars];
    for n in arr.harp.iter().filter(|_| band.harp) {
        let (b, beat) = locate(tl, bpb, n.t0);
        if let Some(slot) = usize::try_from(b).ok().and_then(|b| harp.get_mut(b)) {
            slot.push(format!(
                "{} {} {}",
                num(beat),
                num(n.midi as f64),
                num(round2(n.vel as f64))
            ));
        }
    }
    let mut drums: Vec<Vec<String>> = vec![Vec::new(); nbars];
    for h in arr.drums.iter().flatten() {
        let (b, beat) = locate(tl, bpb, h.t);
        if let Some(slot) = usize::try_from(b).ok().and_then(|b| drums.get_mut(b)) {
            slot.push(format!(
                "{} {} {}",
                num(beat),
                kind_text(h.kind, tl, h.t),
                num(round2(h.vel as f64))
            ));
        }
    }

    let _ = writeln!(
        o,
        "\nBARS. Notes are listed as BEAT+LENGTH MIDI VELOCITY (beats counted from the start of the bar, from 0); harp notes as BEAT MIDI VELOCITY (they ring); drum hits as BEAT KIND VELOCITY, where Tom(N) is N Hz and Swish(N) lasts N beats; guitar strokes as BEAT DIRECTION STRINGS VELOCITY (\"damp\" after a muted chop). \"vocal\" is the lead melody: #N is the syllable's number in the song (a melisma's later notes read #N~), then the syllable, BEAT+LENGTH and MIDI."
    );
    for (i, bar) in form.bars.iter().enumerate() {
        let chords: Vec<&str> = bar
            .chords
            .as_slice()
            .iter()
            .map(|&id| form.chord(id).symbol.as_str())
            .collect();
        let _ = writeln!(o, "\nbar {i} (s{}) chords: {}", bar.sec, chords.join(" "));
        for l in form.lines.iter().filter(|l| l.start_bar == i) {
            let _ = writeln!(o, "  lyric: {}", l.text);
        }
        for (name, v) in [
            ("vocal", &lead),
            ("drums", &drums),
            ("bass", &bass),
            ("harp", &harp),
            ("violin", &violin),
            ("harmony_guitar", &hg),
        ] {
            if !v[i].is_empty() {
                let _ = writeln!(o, "  {name}: {}", v[i].join("; "));
            }
        }
        let strokes = guitar::bar_strokes(song, form, i);
        if !strokes.is_empty() {
            if i > 0 && guitar::bar_strokes(song, form, i - 1) == strokes {
                let _ = writeln!(o, "  guitar: as bar {}", i - 1);
            } else {
                let text: Vec<String> = strokes
                    .iter()
                    .map(|k| {
                        format!(
                            "{} {} {} {}{}",
                            num(k.beat),
                            k.dir.name(),
                            k.strings.name(),
                            num(round2(k.vel)),
                            if k.damp { " damp" } else { "" }
                        )
                    })
                    .collect();
                let _ = writeln!(o, "  guitar: {}", text.join("; "));
            }
        }
    }
    o
}

/// What the rules do in a section, for the view.
struct SectionState {
    harmony: bool,
    doubles: bool,
    choir: String,
}

fn section_state(p: &Prepared, si: usize) -> SectionState {
    let form = &p.form;
    let sec = &form.sections[si];
    let mine = |n: &&LeadNote| form.lines[n.line_idx].sec == si;
    let choir_line = sec.lines.iter().find_map(|&li| match form.lines[li].part {
        song::Part::Choir(v) => Some(v),
        _ => None,
    });
    SectionState {
        harmony: p
            .comp
            .lead
            .iter()
            .filter(mine)
            .any(|n| n.lift && matches!(form.lines[n.line_idx].part, song::Part::Solo(_))),
        doubles: sec.is_repeat_lift()
            && p.comp
                .lead
                .iter()
                .filter(mine)
                .any(|n| n.lift && !form.lines[n.line_idx].part.is_choir()),
        choir: match choir_line {
            Some(v) => format!("words ({})", v.as_str()),
            None if arrange::choir::sings_here(sec) => "pad".to_string(),
            None => "off".to_string(),
        },
    }
}

/// The lead's notes grouped into syllables. Numbers count the syllables of
/// the lead's own lines (the choir's written lines have none) from 0 in
/// song order; a melisma's continuation notes share their syllable's
/// number.
struct LeadIndex {
    /// Per note of `Comp::lead`: its syllable's number.
    syl_of: Vec<Option<usize>>,
    /// Per syllable: the index of its first note and how many notes it has.
    groups: Vec<(usize, usize)>,
}

fn lead_index(p: &Prepared) -> LeadIndex {
    let mut syl_of = Vec::with_capacity(p.comp.lead.len());
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut prev_line = usize::MAX;
    for (k, n) in p.comp.lead.iter().enumerate() {
        if p.form.lines[n.line_idx].part.is_choir() {
            syl_of.push(None);
            continue;
        }
        if n.syl.is_continuation() && n.line_idx == prev_line && !groups.is_empty() {
            let g = groups.len() - 1;
            groups[g].1 += 1;
            syl_of.push(Some(g));
        } else {
            groups.push((k, 1));
            syl_of.push(Some(groups.len() - 1));
        }
        prev_line = n.line_idx;
    }
    LeadIndex { syl_of, groups }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ----------------------------------------------------------------- edits

/// The JSON schema of the model's reply (closed objects throughout, usable
/// as `--json-schema` and as `output_config.format.schema`). An edit is one
/// of eight kinds (`anyOf`), each a closed object: notes over a bar range
/// for the drums, the harp and the pitched parts, strokes over a bar range
/// for the guitar, the lead's notes over a bar range, and per-section
/// switches for the harmony, the doubles and the choir.
pub fn edit_schema() -> Value {
    let pitched = json!({
        "type": "object",
        "description": "A pitched note: beat from the start of from_bar, length in beats, MIDI note, velocity 0 to 1. Violin only: vibrato (default: on for notes of a beat or more).",
        "properties": {
            "beat": {"type": "number"},
            "len": {"type": "number"},
            "midi": {"type": "number"},
            "vel": {"type": "number"},
            "vibrato": {"type": "boolean"}
        },
        "required": ["beat", "len", "midi", "vel"],
        "additionalProperties": false
    });
    let harp_note = json!({
        "type": "object",
        "description": "A harp note (it rings; the instrument sets its length): beat from the start of from_bar, MIDI note 36 to 96, velocity 0 to 1.",
        "properties": {
            "beat": {"type": "number"},
            "midi": {"type": "number"},
            "vel": {"type": "number"}
        },
        "required": ["beat", "midi", "vel"],
        "additionalProperties": false
    });
    let drum = json!({
        "type": "object",
        "description": "A drum hit: beat from the start of from_bar, the drum, velocity 0 to 1. Tom needs hz; Swish needs len (beats).",
        "properties": {
            "beat": {"type": "number"},
            "drum": {"type": "string", "enum": DRUM_KINDS},
            "vel": {"type": "number"},
            "hz": {"type": "number"},
            "len": {"type": "number"}
        },
        "required": ["beat", "drum", "vel"],
        "additionalProperties": false
    });
    let stroke = json!({
        "type": "object",
        "description": "One guitar stroke of the bar's pattern, beat from the start of each bar (0 up to the bar's length). strings: all, bass (the low three strings of the chord's voicing), treble (the high three), or one picked note: low (the bass note), alt (the string above it), g, b, e. damp: a muted chop that stops at once. vel 0 to 1.",
        "properties": {
            "beat": {"type": "number"},
            "dir": {"type": "string", "enum": ["down", "up"]},
            "strings": {"type": "string", "enum": Strings::ALL.map(Strings::name)},
            "damp": {"type": "boolean"},
            "vel": {"type": "number"}
        },
        "required": ["beat", "dir", "strings", "vel"],
        "additionalProperties": false
    });
    let lead_note = json!({
        "type": "object",
        "description": "A sung note: the syllable's number as the view shows it (#N), beat from the start of from_bar, length in beats, MIDI note. A syllable sung over several notes (a melisma) is listed once per note, the notes in a row.",
        "properties": {
            "syllable": {"type": "integer"},
            "beat": {"type": "number"},
            "len": {"type": "number"},
            "midi": {"type": "number"}
        },
        "required": ["syllable", "beat", "len", "midi"],
        "additionalProperties": false
    });
    let ranged = |parts: &[&str], key: &str, item: Value, description: &str| {
        json!({
            "type": "object",
            "description": description,
            "properties": {
                "part": {"type": "string", "enum": parts},
                "from_bar": {"type": "integer"},
                "to_bar": {"type": "integer"},
                key: {"type": "array", "items": item}
            },
            "required": ["part", "from_bar", "to_bar", key],
            "additionalProperties": false
        })
    };
    let sections = json!({
        "type": "array",
        "description": "Section numbers as the view lists them (s0, s1, ...), written as integers.",
        "items": {"type": "integer"}
    });
    let intervals: Vec<&str> = HarmonyInterval::ALL.iter().map(|i| i.name()).collect();
    let edit_kinds = vec![
        ranged(
            &["bass", "violin", "harmony_guitar"],
            "notes",
            pitched,
            "Replaces the part's events in bars from_bar up to, not including, to_bar with exactly the notes given.",
        ),
        ranged(
            &["harp"],
            "notes",
            harp_note,
            "Replaces the harp's notes in bars from_bar up to, not including, to_bar.",
        ),
        ranged(
            &["drums"],
            "notes",
            drum,
            "Replaces the drum hits in bars from_bar up to, not including, to_bar.",
        ),
        ranged(
            &["guitar"],
            "strokes",
            stroke,
            "Sets the rhythm guitar's strum pattern for every bar from from_bar up to, not including, to_bar: the same strokes in each bar, voiced on that bar's chord.",
        ),
        ranged(
            &["lead"],
            "notes",
            lead_note,
            "Changes the pitch, onset and length of the lead vocal's notes in bars from_bar up to, not including, to_bar. List every syllable that begins in those bars, in order, each at least once; never add, drop or reorder words.",
        ),
        json!({
            "type": "object",
            "description": "Turns the harmony singer on or off in sections and, when on, sets its interval above or below the lead (default: the rules' own choice).",
            "properties": {
                "part": {"type": "string", "enum": ["harmony"]},
                "sections": sections,
                "on": {"type": "boolean"},
                "interval": {"type": "string", "enum": intervals}
            },
            "required": ["part", "sections", "on"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "description": "Turns the two doubled takes of the lead on or off in sections.",
            "properties": {
                "part": {"type": "string", "enum": ["doubles"]},
                "sections": sections,
                "on": {"type": "boolean"}
            },
            "required": ["part", "sections", "on"],
            "additionalProperties": false
        }),
        json!({
            "type": "object",
            "description": "Sets what the choir sings in sections: nothing (off), the /aa/ pad, or the section's words in unison or in four-part block harmony.",
            "properties": {
                "part": {"type": "string", "enum": ["choir"]},
                "sections": sections,
                "mode": {"type": "string", "enum": CHOIR_MODES}
            },
            "required": ["part", "sections", "mode"],
            "additionalProperties": false
        }),
    ];
    let mut tracks = Map::new();
    for id in TrackId::ALL {
        tracks.insert(
            id.name().to_string(),
            json!({
                "type": "object",
                "properties": {"gain_db": {"type": "number"}},
                "required": ["gain_db"],
                "additionalProperties": false
            }),
        );
    }
    json!({
        "type": "object",
        "properties": {
            "edits": {
                "type": "array",
                "description": "Each edit changes one part. Bars and sections you leave out keep the rule-based arrangement.",
                "items": {"anyOf": edit_kinds}
            },
            "mix": {
                "type": "object",
                "description": "Optional mix changes: a fader offset in dB per track (name: {gain_db}) and the depth in dB by which the band ducks under the lead (default 5).",
                "properties": {
                    "tracks": {
                        "type": "object",
                        "properties": Value::Object(tracks),
                        "additionalProperties": false
                    },
                    "duck_db": {"type": "number"}
                },
                "additionalProperties": false
            },
            "summary": {"type": "string", "description": "One or two plain sentences: what you changed and why."}
        },
        "required": ["edits", "summary"],
        "additionalProperties": false
    })
}

/// A reply read but not yet validated. Each edit stays a JSON value so one
/// unreadable edit is a repair, not a failure of the whole reply.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrangerEdits {
    pub edits: Vec<Value>,
    pub mix: Option<Value>,
    pub summary: Option<String>,
}

impl ArrangerEdits {
    /// Reads the reply's JSON object. Only a non-object is an error; a
    /// missing or mistyped `edits` is an empty list.
    pub fn from_value(v: &Value) -> Result<ArrangerEdits, String> {
        let o = v
            .as_object()
            .ok_or_else(|| "the arranger's reply is not a JSON object".to_string())?;
        Ok(ArrangerEdits {
            edits: o
                .get("edits")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            mix: o.get("mix").filter(|m| m.is_object()).cloned(),
            summary: o
                .get("summary")
                .and_then(Value::as_str)
                .map(|s| s.trim().chars().take(600).collect::<String>())
                .filter(|s| !s.is_empty()),
        })
    }
}

/// The result of `apply`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    /// One line per repair or drop, in edit order.
    pub repairs: Vec<String>,
    /// Edits that changed the performance.
    pub applied: usize,
    /// The validated mix sidecar (`MixSettings::from_json` reads it), when
    /// the reply had mix edits that survived validation.
    pub mix: Option<Value>,
    pub summary: Option<String>,
}

/// One note after validation, in beats from the start of the edit's first bar.
struct Note {
    beat: f64,
    len: f64,
    midi: f64,
    vel: f32,
    vibrato: Option<bool>,
    kind: Option<DrumKind>,
}

fn get_f64(o: &Map<String, Value>, k: &str) -> Option<f64> {
    o.get(k).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn get_int(o: &Map<String, Value>, k: &str) -> Option<i64> {
    let x = o.get(k)?;
    x.as_i64().or_else(|| {
        x.as_f64()
            .filter(|f| f.is_finite() && f.fract() == 0.0)
            .map(|f| f as i64)
    })
}

/// Moves `m` by octaves into `lo..=hi` (a range of at least an octave).
fn fold_into(mut m: f64, (lo, hi): (f64, f64)) -> f64 {
    while m < lo {
        m += 12.0;
    }
    while m > hi {
        m -= 12.0;
    }
    m
}

/// Validates one note of edit `ei`; `span` is the edit's length in beats.
#[allow(clippy::too_many_arguments)]
fn read_note(
    v: &Value,
    part: Part,
    span: f64,
    ei: usize,
    ni: usize,
    rep: &mut Vec<String>,
) -> Option<Note> {
    let at = format!("edit {ei} note {ni}");
    let Some(o) = v.as_object() else {
        rep.push(format!("{at}: not an object, dropped"));
        return None;
    };
    let Some(beat) = get_f64(o, "beat") else {
        rep.push(format!("{at}: no beat, dropped"));
        return None;
    };
    let beat = quant(beat);
    if beat < 0.0 || beat >= span {
        rep.push(format!(
            "{at}: beat {} is outside the {} beats of the edit, dropped",
            num(beat),
            num(span)
        ));
        return None;
    }
    let vel = match get_f64(o, "vel") {
        Some(x) if (0.0..=1.0).contains(&x) => x,
        Some(x) => {
            rep.push(format!("{at}: velocity {} clamped to 0..1", num(x)));
            x.clamp(0.0, 1.0)
        }
        None => {
            rep.push(format!("{at}: no velocity, {DEFAULT_VEL} used"));
            DEFAULT_VEL
        }
    } as f32;
    let len_of = |default: f64, rep: &mut Vec<String>| match get_f64(o, "len") {
        Some(x) if (MIN_LEN..=MAX_LEN).contains(&x) => x,
        Some(x) => {
            let c = x.clamp(MIN_LEN, MAX_LEN);
            rep.push(format!("{at}: length {} clamped to {}", num(x), num(c)));
            c
        }
        None => {
            rep.push(format!("{at}: no length, {} used", num(default)));
            default
        }
    };
    if part == Part::Drums {
        let name = o.get("drum").and_then(Value::as_str).unwrap_or("");
        let mut len = 0.0;
        let kind = match name.trim().to_ascii_lowercase().as_str() {
            "kick" => DrumKind::Kick,
            "snare" => DrumKind::Snare,
            "rim" => DrumKind::Rim,
            "tap" => DrumKind::Tap,
            "hat" => DrumKind::Hat,
            "shaker" => DrumKind::Shaker,
            "ride" => DrumKind::Ride,
            "tom" => {
                let hz = match get_f64(o, "hz") {
                    Some(h) if (40.0..=400.0).contains(&h) => h,
                    Some(h) => {
                        let c = h.clamp(40.0, 400.0);
                        rep.push(format!("{at}: tom {} Hz clamped to {}", num(h), num(c)));
                        c
                    }
                    None => {
                        rep.push(format!("{at}: tom with no hz, 120 used"));
                        120.0
                    }
                };
                DrumKind::Tom { hz: hz as f32 }
            }
            "swish" => {
                len = len_of(1.0, rep);
                DrumKind::Swish { dur: 0.0 }
            }
            _ => {
                rep.push(format!("{at}: unknown drum {name:?}, dropped"));
                return None;
            }
        };
        return Some(Note {
            beat,
            len,
            midi: 0.0,
            vel,
            vibrato: None,
            kind: Some(kind),
        });
    }
    let Some(midi) = get_f64(o, "midi") else {
        rep.push(format!("{at}: no midi, dropped"));
        return None;
    };
    let folded = fold_into(midi, part.range());
    if folded != midi {
        rep.push(format!(
            "{at}: midi {} is outside the {} range, moved to {}",
            num(midi),
            part.name(),
            num(folded)
        ));
    }
    // The harp rings: its length is the instrument's.
    let len = if part == Part::Harp {
        0.0
    } else {
        len_of(1.0, rep)
    };
    Some(Note {
        beat,
        len,
        midi: folded,
        vel,
        vibrato: o.get("vibrato").and_then(Value::as_bool),
        kind: None,
    })
}

/// Keeps the events of `items` whose onset bar is outside `lo..hi`.
fn keep_outside<T>(
    items: &mut Vec<T>,
    onset: impl Fn(&T) -> f64,
    tl: &Timeline,
    bpb: usize,
    (lo, hi): (usize, usize),
) {
    items.retain(|x| {
        let (b, _) = locate(tl, bpb, onset(x));
        b < lo as i64 || b >= hi as i64
    });
}

fn sort_by_onset<T>(items: &mut [T], onset: impl Fn(&T) -> f64) {
    items.sort_by(|a, b| onset(a).total_cmp(&onset(b)));
}

/// Default pan of a drum voice (the rule-based arranger's).
fn drum_pan(k: DrumKind) -> f32 {
    match k {
        DrumKind::Kick | DrumKind::Tom { .. } => 0.0,
        DrumKind::Snare | DrumKind::Rim => -0.12,
        DrumKind::Tap => -0.15,
        DrumKind::Swish { .. } => 0.35,
        DrumKind::Hat | DrumKind::Shaker => 0.45,
        DrumKind::Ride => 0.4,
    }
}

/// The bars an edit names, after repair.
struct Span {
    lo: usize,
    hi: usize,
    /// Beats cut off the front by clamping the start to bar 0.
    shift: f64,
}

/// Reads and repairs `from_bar` and `to_bar` of edit `ei`.
fn read_span(
    o: &Map<String, Value>,
    ei: usize,
    nbars: usize,
    bpb: usize,
    rep: &mut Vec<String>,
) -> Option<Span> {
    let (Some(from), Some(to)) = (get_int(o, "from_bar"), get_int(o, "to_bar")) else {
        rep.push(format!("edit {ei}: from_bar or to_bar missing, dropped"));
        return None;
    };
    if from >= nbars as i64 || to <= 0 || from >= to {
        rep.push(format!(
            "edit {ei}: bars {from}..{to} are outside the song's {nbars} bars (or empty), dropped"
        ));
        return None;
    }
    let (lo, hi) = (from.max(0) as usize, to.min(nbars as i64) as usize);
    if (lo as i64, hi as i64) != (from, to) {
        rep.push(format!(
            "edit {ei}: bars {from}..{to} clamped to {lo}..{hi}"
        ));
    }
    // Beats are relative to the reply's from_bar; a clamped start moves
    // them. Shift by the bars cut off the front.
    let shift = ((lo as i64 - from) * bpb as i64) as f64;
    Some(Span { lo, hi, shift })
}

/// Validates `edits` against `prepared` and applies them, in order, to
/// `perf`. Returns the repairs, the validated mix and the summary.
///
/// Note edits (drums, bass, harp, violin, harmony guitar) replace events
/// directly. Guitar, lead and section edits are collected, and once every
/// edit has been read the guitar and the vocals are planned again from the
/// edited `prepared` (`arrange::guitar::plan_with`, `arrange::vocals::plan`
/// with overrides): so a lead edit carries through to the harmony, the
/// doubles and the choir. `prepared` changes only when a lead edit applies
/// (its composed lead, its lines' syllables and the second singer's notes);
/// with no lead, guitar or section edit, `perf` is left exactly as it was.
pub fn apply(
    song: &Song,
    prepared: &mut Prepared,
    perf: &mut Performance,
    edits: &ArrangerEdits,
) -> Applied {
    let bpb = prepared.form.bpb() as usize;
    let nbars = prepared.form.bars.len();
    let mut out = Applied {
        summary: edits.summary.clone(),
        ..Applied::default()
    };
    let mut guitar_ov: BTreeMap<usize, Vec<GuitarStroke>> = BTreeMap::new();
    let mut vocal_ov = Overrides::default();
    let mut lead_changed = false;

    for (ei, e) in edits.edits.iter().enumerate() {
        let Some(o) = e.as_object() else {
            out.repairs
                .push(format!("edit {ei}: not an object, dropped"));
            continue;
        };
        let part_text = o.get("part").and_then(Value::as_str).unwrap_or("");
        let Some(target) = Target::parse(part_text) else {
            out.repairs.push(format!(
                "edit {ei}: part {part_text:?} is not editable (want one of {}), dropped",
                EDIT_PARTS.join(", ")
            ));
            continue;
        };
        if let Target::Sections(kind) = target {
            if section_edit(
                kind,
                o,
                ei,
                prepared,
                &perf.band,
                &mut vocal_ov,
                &mut out.repairs,
            ) {
                out.applied += 1;
            }
            continue;
        }
        let Some(sp) = read_span(o, ei, nbars, bpb, &mut out.repairs) else {
            continue;
        };
        let applied = match target {
            Target::Notes(part) => note_edit(part, o, ei, &sp, prepared, perf, &mut out.repairs),
            Target::Guitar => guitar_edit(o, ei, &sp, bpb, &mut guitar_ov, &mut out.repairs),
            Target::Lead => {
                let ok = lead_edit(o, ei, &sp, prepared, &mut out.repairs);
                lead_changed |= ok;
                ok
            }
            Target::Sections(_) => false,
        };
        if applied {
            out.applied += 1;
        }
    }

    if lead_changed {
        // As `prepare_voices` times them: the lead, then the other singer's
        // notes derived from it.
        compose::prepare::time_notes(&mut prepared.comp.lead, &prepared.timeline);
        let second = compose::prepare::compose_second(
            &prepared.comp,
            &prepared.form,
            &prepared.timeline,
            prepared.voice,
            prepared.voice_b,
        );
        prepared.comp.second = second;
        compose::prepare::time_notes(&mut prepared.comp.second, &prepared.timeline);
    }
    if !guitar_ov.is_empty() {
        perf.arrangement.guitar = guitar::plan_with(
            song,
            &prepared.form,
            &prepared.timeline,
            perf.seed,
            &guitar_ov,
        );
    }
    if lead_changed || !vocal_ov.is_empty() {
        perf.arrangement.vocals = arrange::vocals::plan(song, prepared, perf.seed, Some(&vocal_ov));
        let words: Vec<usize> = vocal_ov
            .choir
            .iter()
            .filter(|(_, m)| matches!(m, ChoirMode::Words(_)))
            .map(|(&s, _)| s)
            .collect();
        perf.choir_key = crate::render::choir_line_spans(prepared, &words);
    }

    if let Some(m) = &edits.mix {
        out.mix = read_mix(m, &mut out.repairs);
    }
    out
}

/// Applies a note edit (drums, bass, harp, violin, harmony guitar) to
/// `perf`. Returns whether it changed anything.
fn note_edit(
    part: Part,
    o: &Map<String, Value>,
    ei: usize,
    sp: &Span,
    prepared: &Prepared,
    perf: &mut Performance,
    rep: &mut Vec<String>,
) -> bool {
    let tl = &prepared.timeline;
    let bpb = prepared.form.bpb() as usize;
    let (lo, hi) = (sp.lo, sp.hi);
    let band = &perf.band;
    let part_on = match part {
        Part::Drums => perf.arrangement.drums.is_some() && band.drums != DrumKit::None,
        Part::Bass => band.bass,
        Part::Violin => band.violin,
        Part::HarmonyGuitar => band.harmony_guitar,
        Part::Harp => band.harp,
    };
    if !part_on {
        rep.push(format!(
            "edit {ei}: {} is off in this song's band, dropped",
            part.name()
        ));
        return false;
    }
    let Some(raw) = o.get("notes").and_then(Value::as_array) else {
        rep.push(format!("edit {ei}: no notes list, dropped"));
        return false;
    };
    let span = ((hi - lo) * bpb) as f64;
    let mut notes: Vec<Note> = Vec::new();
    for (ni, v) in raw.iter().enumerate() {
        let mut n = v.clone();
        if let Some(b) = v.get("beat").and_then(Value::as_f64) {
            if sp.shift != 0.0 {
                n["beat"] = json!(b - sp.shift);
            }
        }
        if let Some(note) = read_note(&n, part, span, ei, ni, rep) {
            notes.push(note);
        }
    }
    let base = (lo * bpb) as f64;
    let time = |beat: f64| tl.to_time(base + beat);
    let arr = &mut perf.arrangement;
    match part {
        Part::Drums => {
            let Some(d) = arr.drums.as_mut() else {
                return false;
            };
            keep_outside(d, |h: &DrumHit| h.t, tl, bpb, (lo, hi));
            d.extend(notes.iter().filter_map(|n| {
                let kind = match n.kind? {
                    DrumKind::Swish { .. } => DrumKind::Swish {
                        dur: (time(n.beat + n.len) - time(n.beat)).clamp(0.05, 4.0) as f32,
                    },
                    k => k,
                };
                Some(DrumHit {
                    t: time(n.beat),
                    kind,
                    vel: n.vel,
                    pan: drum_pan(kind),
                })
            }));
            sort_by_onset(d, |h| h.t);
        }
        Part::Bass | Part::HarmonyGuitar | Part::Harp => {
            let list = match part {
                Part::Bass => &mut arr.bass,
                Part::Harp => &mut arr.harp,
                _ => &mut arr.harmony_guitar.lead,
            };
            keep_outside(list, |n: &PluckNote| n.t0, tl, bpb, (lo, hi));
            list.extend(notes.iter().map(|n| PluckNote {
                t0: time(n.beat),
                t1: if part == Part::Harp {
                    time(n.beat)
                } else {
                    time(n.beat + n.len)
                },
                midi: n.midi as f32,
                vel: n.vel,
            }));
            sort_by_onset(list, |n| n.t0);
        }
        Part::Violin => {
            keep_outside(&mut arr.violin, |n: &BowNote| n.t0, tl, bpb, (lo, hi));
            arr.violin.extend(notes.iter().map(|n| BowNote {
                t0: time(n.beat),
                t1: time(n.beat + n.len),
                midi: n.midi as f32,
                vel: n.vel,
                vibrato: n.vibrato.unwrap_or(n.len >= 1.0),
            }));
            sort_by_onset(&mut arr.violin, |n| n.t0);
        }
    }
    true
}

/// Reads a guitar edit: one bar's strokes, set for every bar of the span
/// (beats from the start of each bar). The guitar is planned again after
/// all edits (`apply`).
fn guitar_edit(
    o: &Map<String, Value>,
    ei: usize,
    sp: &Span,
    bpb: usize,
    guitar_ov: &mut BTreeMap<usize, Vec<GuitarStroke>>,
    rep: &mut Vec<String>,
) -> bool {
    let Some(raw) = o.get("strokes").and_then(Value::as_array) else {
        rep.push(format!("edit {ei}: no strokes list, dropped"));
        return false;
    };
    let mut strokes: Vec<GuitarStroke> = Vec::new();
    for (ni, v) in raw.iter().enumerate() {
        let at = format!("edit {ei} stroke {ni}");
        let Some(k) = v.as_object() else {
            rep.push(format!("{at}: not an object, dropped"));
            continue;
        };
        let Some(beat) = get_f64(k, "beat") else {
            rep.push(format!("{at}: no beat, dropped"));
            continue;
        };
        let beat = quant(beat);
        if beat < 0.0 || beat >= bpb as f64 {
            rep.push(format!(
                "{at}: beat {} is outside the bar's {bpb} beats (a stroke's beat counts from the start of each bar), dropped",
                num(beat)
            ));
            continue;
        }
        let dir_text = k.get("dir").and_then(Value::as_str).unwrap_or("");
        let Some(dir) = Dir::parse(dir_text) else {
            rep.push(format!("{at}: dir {dir_text:?} is not down or up, dropped"));
            continue;
        };
        let strings = match k.get("strings").and_then(Value::as_str) {
            None => {
                rep.push(format!("{at}: no strings, all used"));
                Strings::All
            }
            Some(t) => match Strings::parse(t) {
                Some(x) => x,
                None => {
                    rep.push(format!(
                        "{at}: strings {t:?} is unknown (want all, bass, treble, low, alt, g, b or e), dropped"
                    ));
                    continue;
                }
            },
        };
        let vel = match get_f64(k, "vel") {
            Some(x) if (0.0..=1.0).contains(&x) => x,
            Some(x) => {
                rep.push(format!("{at}: velocity {} clamped to 0..1", num(x)));
                x.clamp(0.0, 1.0)
            }
            None => {
                rep.push(format!("{at}: no velocity, {DEFAULT_VEL} used"));
                DEFAULT_VEL
            }
        };
        strokes.push(GuitarStroke {
            beat,
            dir,
            strings,
            damp: k.get("damp").and_then(Value::as_bool).unwrap_or(false),
            vel,
        });
    }
    strokes.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    for bar in sp.lo..sp.hi {
        guitar_ov.insert(bar, strokes.clone());
    }
    true
}

/// One note of a lead edit, after reading.
struct LeadSpec {
    syl: usize,
    /// Beats from the start of the edit's first bar.
    beat: f64,
    len: f64,
    midi: f64,
}

/// Applies a lead edit to `prepared.comp.lead` (and the lines' syllables):
/// the pitch, onset and length of the notes of the syllables that begin in
/// the bars. Every such syllable must be listed, in order, each over one to
/// `MELISMA_MAX_NOTES` notes in a row; the onsets must rise. Otherwise the
/// whole edit is rejected with a repair line and nothing changes. The
/// syllable's phonemes are kept: a split syllable takes the melisma's
/// phoneme layout (`compose::form::expand_melismas`: the onset on the first
/// note, the vowel held, the coda on the last), a merged one the whole
/// syllable on its one note. Times are set again by `apply` afterwards.
fn lead_edit(
    o: &Map<String, Value>,
    ei: usize,
    sp: &Span,
    prepared: &mut Prepared,
    rep: &mut Vec<String>,
) -> bool {
    let bpb = prepared.form.bpb() as usize;
    let idx = lead_index(prepared);
    let mut expected: Vec<usize> = Vec::new();
    for (g, &(first, _)) in idx.groups.iter().enumerate() {
        // By beat, not time: an earlier edit of the reply has not retimed
        // its notes yet.
        let b = (quant(prepared.comp.lead[first].beat) / bpb as f64).floor() as i64;
        if b >= sp.lo as i64 && b < sp.hi as i64 {
            expected.push(g);
        }
    }
    if expected.is_empty() {
        rep.push(format!(
            "edit {ei}: no sung syllable begins in bars {}..{}, dropped",
            sp.lo, sp.hi
        ));
        return false;
    }
    let Some(raw) = o.get("notes").and_then(Value::as_array) else {
        rep.push(format!("edit {ei}: no notes list, dropped"));
        return false;
    };
    let span = ((sp.hi - sp.lo) * bpb) as f64;
    let mut specs: Vec<LeadSpec> = Vec::new();
    for (ni, v) in raw.iter().enumerate() {
        let at = format!("edit {ei} note {ni}");
        let Some(n) = v.as_object() else {
            rep.push(format!("{at}: not an object, dropped"));
            continue;
        };
        let syl = ["syllable", "syllable_index", "index"]
            .iter()
            .find_map(|k| get_int(n, k))
            .filter(|&x| x >= 0);
        let Some(syl) = syl else {
            rep.push(format!("{at}: no syllable number, dropped"));
            continue;
        };
        let (Some(beat), Some(midi)) = (get_f64(n, "beat"), get_f64(n, "midi")) else {
            rep.push(format!("{at}: no beat or no midi, dropped"));
            continue;
        };
        let beat = quant(beat - sp.shift);
        if beat < 0.0 || beat >= span {
            rep.push(format!(
                "{at}: beat {} is outside the {} beats of the edit, dropped",
                num(beat),
                num(span)
            ));
            continue;
        }
        let len = match get_f64(n, "len") {
            Some(x) if (MIN_LEN..=MAX_LEN).contains(&x) => x,
            Some(x) => {
                let c = x.clamp(MIN_LEN, MAX_LEN);
                rep.push(format!("{at}: length {} clamped to {}", num(x), num(c)));
                c
            }
            None => {
                rep.push(format!("{at}: no length, 1 used"));
                1.0
            }
        };
        specs.push(LeadSpec {
            syl: syl as usize,
            beat,
            len: quant(len).max(MIN_LEN),
            midi,
        });
    }

    // Group the notes by syllable: the groups must be exactly the syllables
    // that begin in the bars, in order.
    let mut groups: Vec<(usize, Vec<LeadSpec>)> = Vec::new();
    for x in specs {
        match groups.last_mut() {
            Some((g, v)) if *g == x.syl => v.push(x),
            _ => groups.push((x.syl, vec![x])),
        }
    }
    let got: Vec<usize> = groups.iter().map(|g| g.0).collect();
    if got != expected {
        let list = |v: Vec<usize>| {
            v.iter()
                .map(|x| format!("#{x}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let missing: Vec<usize> = expected
            .iter()
            .copied()
            .filter(|x| !got.contains(x))
            .collect();
        let extra: Vec<usize> = got
            .iter()
            .copied()
            .filter(|x| !expected.contains(x))
            .collect();
        let mut why = Vec::new();
        if !missing.is_empty() {
            why.push(format!("missing {}", list(missing)));
        }
        if !extra.is_empty() {
            why.push(format!("not in these bars: {}", list(extra)));
        }
        if why.is_empty() {
            why.push("a syllable is repeated apart or out of order".to_string());
        }
        rep.push(format!(
            "edit {ei}: the lead edit must list every syllable that begins in bars {}..{} ({}) once, in order, and no other; {}. Words are never dropped, added or reordered: edit rejected, the lead keeps its notes",
            sp.lo,
            sp.hi,
            list(expected),
            why.join("; ")
        ));
        return false;
    }
    let flat: Vec<f64> = groups
        .iter()
        .flat_map(|g| g.1.iter().map(|x| x.beat))
        .collect();
    if flat.windows(2).any(|w| w[1] <= w[0]) {
        rep.push(format!(
            "edit {ei}: the notes' beats must rise from one note to the next; edit rejected, the lead keeps its notes"
        ));
        return false;
    }
    for (g, v) in groups.iter_mut() {
        if v.len() > MELISMA_MAX_NOTES as usize {
            rep.push(format!(
                "edit {ei}: syllable #{g} has {} notes, more than {MELISMA_MAX_NOTES}; the extra notes dropped",
                v.len()
            ));
            v.truncate(MELISMA_MAX_NOTES as usize);
        }
    }

    // Build the new notes of each syllable.
    let base = (sp.lo * bpb) as f64;
    let mut replace: BTreeMap<usize, (usize, Vec<LeadNote>)> = BTreeMap::new();
    let mut lines: Vec<usize> = Vec::new();
    for (g, v) in &groups {
        let (first, count) = idx.groups[*g];
        let old = &prepared.comp.lead[first..first + count];
        let (head, last) = (&old[0], &old[count - 1]);
        let voice = match (head.singer, prepared.voice_b) {
            (song::SingerId::B, Some(vb)) => vb,
            _ => prepared.voice,
        };
        let r = voice.range();
        let fold = (r.lo as f64 - 3.0, r.hi as f64 + 3.0);
        // The whole syllable, then laid over the new number of notes.
        let mut syl = head.syl.clone();
        if count > 1 {
            let pos = syl
                .phones
                .iter()
                .position(|p| p.is_vowel())
                .unwrap_or(syl.phones.len());
            let mut ph = syl.phones[..pos].to_vec();
            ph.extend_from_slice(&last.syl.phones);
            syl.phones = ph;
            syl.word_end = last.syl.word_end;
        }
        syl.notes = v.len() as u8;
        let expanded = compose::form::expand_melismas(std::slice::from_ref(&syl));
        let mut notes = Vec::with_capacity(v.len());
        for (k, (x, sy)) in v.iter().zip(expanded).enumerate() {
            let folded = fold_into(x.midi, fold);
            if folded != x.midi {
                rep.push(format!(
                    "edit {ei}: syllable #{g}: midi {} is outside the singer's range, moved to {}",
                    num(x.midi),
                    num(folded)
                ));
            }
            let mut n = head.clone();
            n.stress = sy.stress;
            n.syl = sy;
            n.beat = base + x.beat;
            n.dur = x.len;
            n.midi = folded.round() as i32;
            n.phrase_start = head.phrase_start && k == 0;
            n.phrase_end = last.phrase_end && k + 1 == v.len();
            if k > 0 {
                n.grace = None;
            }
            notes.push(n);
        }
        if !lines.contains(&head.line_idx) {
            lines.push(head.line_idx);
        }
        replace.insert(first, (count, notes));
    }

    let old = std::mem::take(&mut prepared.comp.lead);
    let mut lead: Vec<LeadNote> = Vec::with_capacity(old.len());
    let mut is_new: Vec<bool> = Vec::with_capacity(old.len());
    let mut k = 0;
    while k < old.len() {
        if let Some((count, notes)) = replace.remove(&k) {
            is_new.extend(std::iter::repeat_n(true, notes.len()));
            lead.extend(notes);
            k += count;
        } else {
            is_new.push(false);
            lead.push(old[k].clone());
            k += 1;
        }
    }
    // A new note ends where the next note begins at the latest.
    for j in 0..lead.len().saturating_sub(1) {
        if is_new[j] {
            let gap = lead[j + 1].beat - lead[j].beat;
            if lead[j].dur > gap {
                lead[j].dur = gap.max(MIN_LEN);
            }
        }
    }
    // The lines keep their syllables' texts; their note lists follow.
    for li in lines {
        let mut syls = Vec::new();
        for n in lead.iter_mut().filter(|n| n.line_idx == li) {
            n.i = syls.len();
            syls.push(n.syl.clone());
        }
        prepared.form.lines[li].syls = syls;
    }
    prepared.comp.lead = lead;
    true
}

/// Reads a section-level edit (harmony, doubles, choir) into `ov`.
fn section_edit(
    kind: SectionPart,
    o: &Map<String, Value>,
    ei: usize,
    prepared: &Prepared,
    band: &song::Band,
    ov: &mut Overrides,
    rep: &mut Vec<String>,
) -> bool {
    let (on_in_band, what) = match kind {
        SectionPart::Harmony => (band.harmonies, "harmony"),
        SectionPart::Doubles => (band.doubles, "doubles"),
        SectionPart::Choir => (band.choir, "choir"),
    };
    if !on_in_band {
        rep.push(format!(
            "edit {ei}: {what} is off in this song's band, dropped"
        ));
        return false;
    }
    let form = &prepared.form;
    let nsec = form.sections.len();
    let Some(list) = o.get("sections").and_then(Value::as_array) else {
        rep.push(format!("edit {ei}: no sections list, dropped"));
        return false;
    };
    let mut secs: Vec<usize> = Vec::new();
    for v in list {
        let id = v.as_i64().or_else(|| {
            v.as_str()
                .and_then(|t| t.trim().trim_start_matches(['s', 'S']).parse().ok())
        });
        match id {
            Some(i) if (0..nsec as i64).contains(&i) => {
                if !secs.contains(&(i as usize)) {
                    secs.push(i as usize);
                }
            }
            _ => rep.push(format!(
                "edit {ei}: section {v} does not exist (the song has {nsec}, numbered from 0), dropped"
            )),
        }
    }
    if secs.is_empty() {
        rep.push(format!("edit {ei}: no valid section, dropped"));
        return false;
    }
    match kind {
        SectionPart::Harmony | SectionPart::Doubles => {
            let Some(on) = o.get("on").and_then(Value::as_bool) else {
                rep.push(format!("edit {ei}: no on (true or false), dropped"));
                return false;
            };
            let interval = match o.get("interval").and_then(Value::as_str) {
                Some(t) if kind == SectionPart::Harmony && on => {
                    let i = HarmonyInterval::parse(t);
                    if i.is_none() {
                        rep.push(format!(
                            "edit {ei}: interval {t:?} is unknown, the rules' own interval used"
                        ));
                    }
                    i
                }
                _ => None,
            };
            for s in secs {
                if kind == SectionPart::Harmony {
                    ov.harmony.insert(s, SectionHarmony { on, interval });
                } else {
                    ov.doubles.insert(s, on);
                }
            }
            true
        }
        SectionPart::Choir => {
            let mode_text = o.get("mode").and_then(Value::as_str).unwrap_or("");
            let mode = match mode_text.trim().to_ascii_lowercase().as_str() {
                "off" => ChoirMode::Off,
                "pad" => ChoirMode::Pad,
                "unison" => ChoirMode::Words(ChoirVoicing::Unison),
                "block" => ChoirMode::Words(ChoirVoicing::Block),
                _ => {
                    rep.push(format!(
                        "edit {ei}: choir mode {mode_text:?} is unknown (want {}), dropped",
                        CHOIR_MODES.join(", ")
                    ));
                    return false;
                }
            };
            let mut any = false;
            for s in secs {
                let sec = &form.sections[s];
                let written = sec.lines.iter().any(|&li| form.lines[li].part.is_choir());
                if written && !matches!(mode, ChoirMode::Words(_)) {
                    rep.push(format!(
                        "edit {ei}: section {s} has the writer's own choir line; only unison or block may change it, skipped"
                    ));
                    continue;
                }
                if matches!(mode, ChoirMode::Words(_)) && sec.lines.is_empty() {
                    rep.push(format!(
                        "edit {ei}: section {s} has no words for the choir to sing, skipped"
                    ));
                    continue;
                }
                ov.choir.insert(s, mode);
                any = true;
            }
            any
        }
    }
}

/// Validates the mix edit: known track names, `gain_db` and `duck_db`
/// clamped to the mixer's ranges. `None` when nothing is left.
fn read_mix(m: &Value, rep: &mut Vec<String>) -> Option<Value> {
    let mut tracks = Map::new();
    if let Some(t) = m.get("tracks").and_then(Value::as_object) {
        for (name, v) in t {
            let Some(id) = TrackId::ALL.into_iter().find(|id| id.name() == name) else {
                rep.push(format!("mix: unknown track {name:?}, dropped"));
                continue;
            };
            let Some(g) = v
                .get("gain_db")
                .and_then(Value::as_f64)
                .filter(|g| g.is_finite())
            else {
                rep.push(format!("mix: {name} has no gain_db, dropped"));
                continue;
            };
            let c = g.clamp(GAIN_DB_MIN as f64, GAIN_DB_MAX as f64);
            if c != g {
                rep.push(format!(
                    "mix: {name} gain {} dB clamped to {}",
                    num(g),
                    num(c)
                ));
            }
            tracks.insert(id.name().to_string(), json!({"gain_db": c}));
        }
    }
    let duck = m
        .get("duck_db")
        .and_then(Value::as_f64)
        .filter(|d| d.is_finite())
        .map(|d| {
            let c = d.clamp(DUCK_DB_MIN as f64, DUCK_DB_MAX as f64);
            if c != d {
                rep.push(format!("mix: duck_db {} clamped to {}", num(d), num(c)));
            }
            c
        });
    if tracks.is_empty() && duck.is_none() {
        return None;
    }
    let mut o = Map::new();
    o.insert("version".into(), json!(1));
    if !tracks.is_empty() {
        o.insert("tracks".into(), Value::Object(tracks));
    }
    if let Some(d) = duck {
        o.insert("duck_db".into(), json!(d));
    }
    Some(Value::Object(o))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::arrange_song;
    use compose::prepare::VoiceChoice;
    use song::events::StringNote;

    /// A small 4/4 song: 4 bars of verse, full kit, bass and violin on.
    fn small() -> Song {
        let v = json!({
            "schema_version": 3,
            "title": "Small", "note": "", "key": "G", "mode": "major", "meter": "4/4", "tempo": 120,
            "guitar": "strum", "voice": "baritone",
            "band": {"drums": "full", "bass": true, "harmonyGuitar": true, "harp": false,
                     "violin": true, "choir": false, "harmonies": false, "doubles": false},
            "sections": [
                {"type": "verse", "energy": "mid", "lines": [
                    {"syl": "*one *two *three *four", "chords": ["G", "C"]},
                    {"syl": "*five *six *seven *eight", "chords": ["D", "G"]}]}
            ]
        });
        song::normalize_value(&v).unwrap().0
    }

    fn arranged() -> (Song, Prepared, Performance) {
        let s = small();
        let (p, perf) = arrange_song(&s, 7, VoiceChoice::default());
        (s, p, perf)
    }

    fn edits(v: Value) -> ArrangerEdits {
        ArrangerEdits::from_value(&v).unwrap()
    }

    #[test]
    fn view_lists_meter_key_sections_and_bars_in_beats() {
        let (s, p, perf) = arranged();
        let text = view(&s, &p, &perf);
        assert!(text.contains("meter 4/4: 4 beats per bar"), "{text}");
        assert!(text.contains("key after transposition"));
        assert!(text.contains("s0 verse bars 0-"));
        assert!(text.contains("energy mid"));
        assert!(text.contains("bar 0 (s0) chords:"));
        assert!(text.contains("  vocal: "));
        assert!(text.contains("  drums: 0 Kick "), "{text}");
        assert!(text.contains("  bass: 0+"));
        // Every bar of the form has its heading.
        for i in 0..p.form.bars.len() {
            assert!(text.contains(&format!("bar {i} (s0)")), "bar {i}");
        }
    }

    #[test]
    fn no_edits_leave_the_performance_unchanged() {
        let (s, mut p, mut perf) = arranged();
        let before = perf.clone();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(json!({"edits": [], "summary": ""})),
        );
        assert_eq!(perf, before);
        assert!(a.repairs.is_empty() && a.applied == 0 && a.mix.is_none());
        assert!(a.summary.is_none());
    }

    #[test]
    fn a_kick_on_every_beat_lands_at_the_timeline_seconds() {
        let (s, mut p, mut perf) = arranged();
        let bpb = p.form.bpb() as usize;
        let notes: Vec<Value> = (0..bpb * 2)
            .map(|b| json!({"beat": b, "drum": "Kick", "vel": 0.9}))
            .collect();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(
                json!({"edits": [{"part": "drums", "from_bar": 1, "to_bar": 3, "notes": notes}],
                          "summary": "stomp"}),
            ),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 1);
        assert_eq!(a.summary.as_deref(), Some("stomp"));
        let d = perf.arrangement.drums.as_ref().unwrap();
        let tl = &p.timeline;
        // Jitter moves a hit by at most 4 ms: 10 ms clears it.
        let lo = tl.to_time((bpb) as f64) - 0.01;
        let hi = tl.to_time((3 * bpb) as f64) - 0.01;
        let inside: Vec<_> = d.iter().filter(|h| h.t >= lo && h.t < hi).collect();
        assert_eq!(inside.len(), bpb * 2, "only the new hits are in bars 1..3");
        for (k, h) in inside.iter().enumerate() {
            assert_eq!(h.kind, DrumKind::Kick);
            assert_eq!(h.vel, 0.9);
            assert_eq!(h.t, tl.to_time((bpb + k) as f64));
        }
        assert!(d.windows(2).all(|w| w[0].t <= w[1].t), "sorted");
        // Bar 0 is as the rules made it.
        let (_, _, before) = arranged();
        let b0 = |v: &[DrumHit]| v.iter().filter(|h| h.t < lo).count();
        assert_eq!(
            b0(d),
            b0(before.arrangement.drums.as_ref().unwrap()),
            "bars outside the edit keep their hits"
        );
    }

    #[test]
    fn a_replaced_violin_range_converts_beats_and_keeps_the_rest() {
        let (s, mut p, mut perf) = arranged();
        let bpb = p.form.bpb() as f64;
        let before = perf.arrangement.violin.clone();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(
                json!({"edits": [{"part": "violin", "from_bar": 2, "to_bar": 4, "notes": [
                {"beat": 0, "len": 1, "midi": 74, "vel": 0.6},
                {"beat": 1.5, "len": 0.5, "midi": 76, "vel": 0.7, "vibrato": false},
                {"beat": 4, "len": 4, "midi": 71, "vel": 0.5}]}]}),
            ),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        let tl = &p.timeline;
        let t2 = tl.to_time(2.0 * bpb);
        let t4 = tl.to_time(4.0 * bpb);
        let inside: Vec<_> = perf
            .arrangement
            .violin
            .iter()
            .filter(|n| n.t0 >= t2 - 0.01 && n.t0 < t4 - 0.01)
            .collect();
        assert_eq!(inside.len(), 3);
        assert_eq!(inside[0].t0, tl.to_time(2.0 * bpb));
        assert_eq!(inside[0].t1, tl.to_time(2.0 * bpb + 1.0));
        assert!(inside[0].vibrato);
        assert_eq!(inside[1].t0, tl.to_time(2.0 * bpb + 1.5));
        assert!(!inside[1].vibrato);
        assert_eq!(inside[2].midi, 71.0);
        assert_eq!(inside[2].t0, tl.to_time(3.0 * bpb));
        let outside = |v: &[BowNote]| v.iter().filter(|n| n.t0 < t2 - 0.01).count();
        assert_eq!(outside(&perf.arrangement.violin), outside(&before));
    }

    #[test]
    fn a_mix_edit_is_validated_into_a_sidecar_value() {
        let (s, mut p, mut perf) = arranged();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(json!({"edits": [], "summary": "s", "mix": {
                "tracks": {"drums": {"gain_db": 4}, "bass": {"gain_db": 60}, "kazoo": {"gain_db": 1}},
                "duck_db": 2}})),
        );
        let m = a.mix.expect("a mix");
        assert_eq!(m["version"], 1);
        assert_eq!(m["tracks"]["drums"]["gain_db"], 4.0);
        assert_eq!(m["tracks"]["bass"]["gain_db"], GAIN_DB_MAX as f64);
        assert!(m["tracks"].get("kazoo").is_none());
        assert_eq!(m["duck_db"], 2.0);
        assert!(a.repairs.iter().any(|r| r.contains("kazoo")));
        assert!(a.repairs.iter().any(|r| r.contains("bass gain")));
        // The sidecar reader accepts it without a warning.
        let stems = crate::render::Stems {
            len: 0,
            tracks: Default::default(),
        };
        let d = crate::MixSettings::default_for(&stems);
        let (mx, warn) = crate::MixSettings::from_json(&m, &d);
        assert!(warn.is_empty(), "{warn:?}");
        assert_eq!(mx.duck_db, 2.0);
        assert_eq!(mx.tracks[TrackId::Drums.index()].gain_db, 4.0);
    }

    #[test]
    fn invalid_edits_are_repaired_or_dropped_with_a_report() {
        let (s, mut p, mut perf) = arranged();
        let nbars = p.form.bars.len() as i64;
        let before = perf.clone();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(json!({"edits": [
                {"part": "harp", "from_bar": 0, "to_bar": 1, "notes": []},
                {"part": "bass", "from_bar": 900, "to_bar": 901, "notes": []},
                {"part": "bass", "from_bar": 1, "to_bar": 1, "notes": []},
                {"part": "drums", "from_bar": 0, "to_bar": 1, "notes": [
                    {"beat": 0, "drum": "Cowbell", "vel": 1},
                    {"beat": 9, "drum": "Kick", "vel": 1},
                    {"beat": -1, "drum": "Kick", "vel": 1},
                    {"beat": 1, "drum": "Kick", "vel": 7}]},
                {"part": "bass", "from_bar": -2, "to_bar": 1, "notes": [
                    {"beat": 9, "len": 1, "midi": 10, "vel": -1}]},
                "junk"
            ], "summary": ""})),
        );
        let text = a.repairs.join("\n");
        for needle in [
            "harp",
            "outside the song",
            "Cowbell",
            "beat 9 is outside",
            "beat -1 is outside",
            "velocity 7 clamped",
            "velocity -1 clamped",
            "bars -2..1 clamped to 0..1",
            "midi 10 is outside",
            "not an object",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
        // The clamped bass edit (bars -2..1 is 0..1) is applied. Its beats
        // count from bar -2, so beat 9 is beat 1 of bar 0.
        assert_eq!(a.applied, 2);
        let d = perf.arrangement.drums.as_ref().unwrap();
        let kick1 = d.iter().find(|h| h.vel == 1.0 && h.kind == DrumKind::Kick);
        assert!(kick1.is_some(), "velocity clamped to 1.0");
        let t_end = p.timeline.to_time(4.0) - 0.01;
        let bar0: Vec<_> = perf
            .arrangement
            .bass
            .iter()
            .filter(|n| n.t0 < t_end)
            .collect();
        assert_eq!(bar0.len(), 1, "bar 0 of the bass is the one new note");
        assert_eq!(bar0[0].midi, 34.0);
        assert_eq!(bar0[0].vel, 0.0);
        assert_eq!(bar0[0].t0, p.timeline.to_time(1.0));
        assert!(nbars > 1 && before.arrangement.bass.len() > 1);
    }

    #[test]
    fn pitches_outside_the_range_fold_by_octaves() {
        let (s, mut p, mut perf) = arranged();
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(
                json!({"edits": [{"part": "bass", "from_bar": 0, "to_bar": 1, "notes": [
                {"beat": 0, "len": 1, "midi": 70, "vel": 0.8},
                {"beat": 1, "len": 1, "midi": 20, "vel": 0.8}]}], "summary": ""}),
            ),
        );
        assert_eq!(a.repairs.len(), 2, "{:?}", a.repairs);
        let t0 = p.timeline.to_time(0.0);
        let t1 = p.timeline.to_time(4.0);
        let m: Vec<f32> = perf
            .arrangement
            .bass
            .iter()
            .filter(|n| n.t0 >= t0 - 0.01 && n.t0 < t1 - 0.01)
            .map(|n| n.midi)
            .collect();
        assert_eq!(m, vec![58.0, 32.0]);
    }

    #[test]
    fn an_edit_to_a_part_that_is_off_is_dropped() {
        let v = json!({
            "schema_version": 3, "title": "t", "note": "", "key": "C", "mode": "major",
            "meter": "4/4", "tempo": 100, "guitar": "strum", "voice": "baritone",
            "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false,
                     "violin": false, "choir": false, "harmonies": false, "doubles": false},
            "sections": [{"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]
        });
        let s = song::normalize_value(&v).unwrap().0;
        let (mut p, mut perf) = arrange_song(&s, 1, VoiceChoice::default());
        let before = perf.clone();
        let text = view(&s, &p, &perf);
        assert!(text.contains("violin off") && text.contains("drums none"));
        assert!(!text.contains("  violin:") && !text.contains("  drums:"));
        let a = apply(
            &s,
            &mut p,
            &mut perf,
            &edits(json!({"edits": [
                {"part": "drums", "from_bar": 0, "to_bar": 1, "notes": [{"beat": 0, "drum": "Kick", "vel": 1}]},
                {"part": "violin", "from_bar": 0, "to_bar": 1, "notes": []}], "summary": ""})),
        );
        assert_eq!(perf, before);
        assert_eq!(a.repairs.len(), 2);
        assert!(a
            .repairs
            .iter()
            .all(|r| r.contains("off in this song's band")));
    }

    #[test]
    fn the_edited_performance_survives_a_json_round_trip() {
        let (s, mut p, mut perf) = arranged();
        apply(
            &s,
            &mut p,
            &mut perf,
            &edits(
                json!({"edits": [{"part": "drums", "from_bar": 0, "to_bar": 2, "notes": [
                {"beat": 0, "drum": "Swish", "vel": 0.5, "len": 2},
                {"beat": 2, "drum": "Tom", "vel": 0.5, "hz": 150}]}], "summary": ""}),
            ),
        );
        let text = serde_json::to_string(&perf).unwrap();
        let back: Performance = serde_json::from_str(&text).unwrap();
        assert_eq!(back, perf);
    }

    #[test]
    fn the_arranging_note_does_not_change_the_rule_based_arrangement() {
        let plain = small();
        let mut noted = plain.clone();
        noted.arranging = Some("Drive it hard. Kick on every beat.".into());
        let (_, a) = arrange_song(&plain, 7, VoiceChoice::default());
        let (_, b) = arrange_song(&noted, 7, VoiceChoice::default());
        assert_eq!(a, b);
    }

    #[test]
    fn a_mock_arranger_pass_end_to_end() {
        use songwriter::arranger::{arrange, ArrangeRequest};
        use songwriter::claude::{Claude, ClaudeError, Effort, Reply, Request};
        use std::cell::RefCell;

        struct Mock {
            reply: String,
            prompt: RefCell<String>,
        }
        impl Claude for Mock {
            fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
                *self.prompt.borrow_mut() = req.prompt.clone();
                assert_eq!(req.json_schema, Some(edit_schema()));
                Ok(Reply::text_only(self.reply.clone()))
            }
        }

        let (s, mut p, mut perf) = arranged();
        let text = view(&s, &p, &perf);
        let bpb = p.form.bpb() as usize;
        let kicks: Vec<Value> = (0..bpb)
            .map(|b| json!({"beat": b, "drum": "Kick", "vel": 0.95}))
            .collect();
        let mock = Mock {
            reply: json!({
                "edits": [{"part": "drums", "from_bar": 0, "to_bar": 1, "notes": kicks}],
                "mix": {"tracks": {"drums": {"gain_db": 4}, "bass": {"gain_db": 6}}, "duck_db": 2},
                "summary": "A kick on every beat; drums and bass up; less ducking."
            })
            .to_string(),
            prompt: RefCell::new(String::new()),
        };
        let got = arrange(
            &mock,
            &ArrangeRequest {
                view: &text,
                style: songwriter::styles::style("oldtime").ok(),
                writer_note: Some("Barn dance, hard and fast."),
                extra_note: None,
                schema: edit_schema(),
                model: None,
                effort: Effort::Medium,
            },
        )
        .unwrap();
        let prompt = mock.prompt.borrow();
        assert!(prompt.contains(&text) && prompt.contains("Barn dance, hard and fast."));
        let e = ArrangerEdits::from_value(&got.raw).unwrap();
        let a = apply(&s, &mut p, &mut perf, &e);
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 1);
        let t1 = p.timeline.to_time(bpb as f64) - 0.01;
        let d = perf.arrangement.drums.as_ref().unwrap();
        let bar0: Vec<_> = d.iter().filter(|h| h.t < t1).collect();
        assert_eq!(bar0.len(), bpb);
        assert!(bar0
            .iter()
            .all(|h| h.kind == DrumKind::Kick && h.vel == 0.95));
        let m = a.mix.unwrap();
        assert_eq!(m["tracks"]["bass"]["gain_db"], 6.0);
        assert_eq!(m["duck_db"], 2.0);
    }

    #[test]
    fn the_schema_is_closed_and_names_every_part() {
        let s = edit_schema();
        assert_eq!(s["additionalProperties"], false);
        let kinds = s["properties"]["edits"]["items"]["anyOf"]
            .as_array()
            .unwrap();
        assert_eq!(kinds.len(), 8);
        let mut parts: Vec<String> = Vec::new();
        for k in kinds {
            assert_eq!(k["additionalProperties"], false);
            let names: Vec<&str> = k["properties"]["part"]["enum"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            parts.extend(names.iter().map(|x| x.to_string()));
            // Every property is listed and the required ones exist.
            for r in k["required"].as_array().unwrap() {
                assert!(k["properties"].get(r.as_str().unwrap()).is_some());
            }
            for key in ["notes", "strokes"] {
                if let Some(arr) = k["properties"].get(key) {
                    assert_eq!(arr["items"]["additionalProperties"], false);
                }
            }
        }
        parts.sort();
        let mut want: Vec<String> = EDIT_PARTS.iter().map(|x| x.to_string()).collect();
        want.sort();
        assert_eq!(parts, want);
        let tracks = &s["properties"]["mix"]["properties"]["tracks"];
        assert_eq!(tracks["additionalProperties"], false);
        for id in TrackId::ALL {
            assert!(tracks["properties"].get(id.name()).is_some());
        }
    }

    // ---------------------------------------------------- every part

    /// A 4/4 song with every band part on, a melisma in the first verse,
    /// lifted choruses (the second is a repeat) and an outro.
    fn full() -> Song {
        let v = json!({
            "schema_version": 2,
            "title": "Full", "note": "", "key": "G", "mode": "major", "meter": "4/4", "tempo": 100,
            "guitar": "strum", "voice": "baritone",
            "band": {"drums": "full", "bass": true, "harmonyGuitar": true, "harp": true,
                     "violin": true, "choir": true, "harmonies": true, "doubles": true},
            "sections": [
                {"type": "verse", "lines": [
                    {"syl": "*glo~2-ry *walk the *long *road", "chords": ["G", "C"]},
                    {"syl": "*home *to the *one *I *know", "chords": ["D", "G"]}]},
                {"type": "chorus", "lines": [
                    {"syl": "*hold *on, *hold *on", "chords": ["G", "D"]},
                    {"syl": "*sing *it *out *loud", "chords": ["C", "G"]}]},
                {"type": "verse", "same": true},
                {"type": "chorus", "same": true},
                {"type": "outro", "chords": ["G", "D", "G"]}
            ]
        });
        song::normalize_value(&v).unwrap().0
    }

    fn full_arranged() -> (Song, Prepared, Performance) {
        let s = full();
        let (p, perf) = arrange_song(&s, 11, VoiceChoice::default());
        (s, p, perf)
    }

    fn run(s: &Song, p: &mut Prepared, perf: &mut Performance, e: Value) -> Applied {
        apply(s, p, perf, &edits(e))
    }

    /// Seconds at the start of `bar`.
    fn at(p: &Prepared, bar: usize) -> f64 {
        p.timeline.to_time((bar * p.form.bpb() as usize) as f64)
    }

    /// The lead edit's notes for the syllables beginning in bars `lo..hi`,
    /// each moved by `dm` semitones.
    fn lead_notes(p: &Prepared, lo: usize, hi: usize, dm: i32) -> Vec<Value> {
        let bpb = p.form.bpb() as usize;
        let idx = lead_index(p);
        let mut out = Vec::new();
        for &(first, count) in &idx.groups {
            let bar = (p.comp.lead[first].beat / bpb as f64).floor() as usize;
            if bar < lo || bar >= hi {
                continue;
            }
            for n in &p.comp.lead[first..first + count] {
                out.push(json!({
                    "syllable": idx.syl_of[first].unwrap(),
                    "beat": n.beat - (lo * bpb) as f64,
                    "len": n.dur,
                    "midi": n.midi + dm,
                }));
            }
        }
        out
    }

    #[test]
    fn the_view_shows_every_part() {
        let (s, p, perf) = full_arranged();
        let text = view(&s, &p, &perf);
        assert!(text.contains("harp on") && text.contains("harmony singer on"));
        assert!(text.contains("harmony off") && text.contains("harmony on"));
        assert!(text.contains("doubles on") && text.contains("choir pad"));
        assert!(text.contains("  harp: "), "{text}");
        assert!(text.contains("  guitar: 0 down all "), "{text}");
        assert!(text.contains("  guitar: as bar 0") || text.contains("  guitar: as bar 1"));
        // Lead notes carry the syllable number and text; a melisma's second
        // note repeats the number with a tilde.
        assert!(text.contains("#0 \"glo\" 0+"), "{text}");
        assert!(text.contains("#0~ "), "{text}");
        assert!(text.contains("#1 \"ry\" "), "{text}");
    }

    #[test]
    fn harp_notes_replace_like_bass_and_fold_by_octaves() {
        let (s, mut p, mut perf) = full_arranged();
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "harp", "from_bar": 0, "to_bar": 2, "notes": [
                {"beat": 0, "midi": 72, "vel": 0.5},
                {"beat": 2.5, "midi": 20, "vel": 0.4}]}], "summary": ""}),
        );
        assert_eq!(a.applied, 1);
        assert_eq!(a.repairs.len(), 1, "{:?}", a.repairs);
        assert!(a.repairs[0].contains("midi 20 is outside the harp range, moved to 44"));
        let (t0, t1) = (at(&p, 0), at(&p, 2));
        let h: Vec<&PluckNote> = perf
            .arrangement
            .harp
            .iter()
            .filter(|n| n.t0 >= t0 - 0.01 && n.t0 < t1 - 0.01)
            .collect();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].t0, t0);
        assert_eq!(h[1].t0, p.timeline.to_time(2.5));
        assert_eq!((h[1].midi, h[1].t1), (44.0, h[1].t0), "a harp note rings");
        assert!(perf.arrangement.harp.iter().any(|n| n.t0 >= t1));
    }

    #[test]
    fn a_guitar_pattern_is_voiced_on_each_bars_chord() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.arrangement.guitar.clone();
        let st = |beat: f64, strings: &str, damp: bool| json!({"beat": beat, "dir": "down", "strings": strings, "damp": damp, "vel": 0.8});
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "guitar", "from_bar": 1, "to_bar": 3, "strokes": [
                st(0.0, "bass", false), st(1.0, "treble", false),
                st(2.0, "bass", false), st(3.0, "treble", true)]}], "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 1);
        let g = &perf.arrangement.guitar;
        let (t1, t3) = (at(&p, 1), at(&p, 3));
        let win = |v: &[Vec<StringNote>; 6]| -> Vec<StringNote> {
            v.iter()
                .flatten()
                .filter(|n| n.t >= t1 - 0.05 && n.t < t3 - 0.05)
                .copied()
                .collect()
        };
        let inside = win(g);
        assert!(!inside.is_empty());
        // Boom on beats 0 and 2 (strings 0-2), chuck on 1 and 3 (3-5), and
        // the chuck on 3 is a muted chop.
        for n in &inside {
            let beat = (p.timeline.to_beat(n.t) - 4.0).rem_euclid(4.0);
            let beat = if beat > 3.9 { 0.0 } else { beat };
            let which = beat.round() as usize;
            if which.is_multiple_of(2) {
                assert!(n.string <= 2, "beat {beat} string {}", n.string);
            } else {
                assert!(n.string >= 3, "beat {beat} string {}", n.string);
            }
            if which == 3 {
                assert!(n.stop - n.t <= guitar::CHOP_TIME + 1e-9);
            }
        }
        // Bar 0 and the bars from 3 on are as the rules made them.
        let key = |v: &[Vec<StringNote>; 6], lo: f64, hi: f64| -> Vec<(u8, u8, u64)> {
            v.iter()
                .flatten()
                .filter(|n| n.t >= lo && n.t < hi)
                .map(|n| (n.string, n.midi, n.t.to_bits()))
                .collect()
        };
        assert_eq!(key(g, 0.0, t1 - 0.05), key(&before, 0.0, t1 - 0.05));
        assert_eq!(key(g, t3 + 0.05, 1e9), key(&before, t3 + 0.05, 1e9));
    }

    #[test]
    fn guitar_strokes_are_validated() {
        let (s, mut p, mut perf) = full_arranged();
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "guitar", "from_bar": 0, "to_bar": 1, "strokes": [
                {"beat": 4, "dir": "down", "strings": "all", "vel": 0.8},
                {"beat": 0, "dir": "sideways", "strings": "all", "vel": 0.8},
                {"beat": 1, "dir": "up", "strings": "banjo", "vel": 0.8},
                {"beat": 2, "dir": "up", "strings": "treble", "vel": 3}]}], "summary": ""}),
        );
        let text = a.repairs.join("\n");
        for needle in [
            "beat 4 is outside",
            "sideways",
            "banjo",
            "velocity 3 clamped",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
        assert_eq!(a.applied, 1);
    }

    #[test]
    fn harmony_edits_turn_sections_on_and_off_and_name_the_interval() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.arrangement.vocals.clone();
        assert!(!before.harmony.notes.is_empty());
        // Off in both lifted choruses (sections 1 and 3), on a third below in
        // the first verse (section 0).
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [
                {"part": "harmony", "sections": [1, 3], "on": false},
                {"part": "harmony", "sections": [0], "on": true, "interval": "third_below"}],
                "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 2);
        let v = &perf.arrangement.vocals;
        let h = &v.harmony.notes;
        assert!(!h.is_empty());
        let sec1 = p.form.sections[1].start_bar;
        let sec2 = p.form.sections[2].start_bar;
        assert!(
            h.iter()
                .all(|n| n.t0 < at(&p, sec1) || n.t0 >= at(&p, sec2)),
            "no harmony in the choruses"
        );
        // Every harmony note sits a third (3 or 4 semitones) under its lead note.
        for n in h {
            let l = v
                .lead
                .notes
                .iter()
                .find(|l| (l.t0 - n.t0).abs() < 1e-9)
                .expect("a lead note at its onset");
            let d = l.midi - n.midi;
            assert!(d == 3.0 || d == 4.0, "interval {d}");
        }
        // The lead and the doubles are untouched.
        assert_eq!(v.lead, before.lead);
        assert_eq!(v.doubles, before.doubles);
    }

    #[test]
    fn doubles_edits_switch_sections() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.arrangement.vocals.doubles[0].notes.len();
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [
                {"part": "doubles", "sections": [3], "on": false},
                {"part": "doubles", "sections": [0], "on": true}], "summary": ""}),
        );
        assert_eq!(a.applied, 2, "{:?}", a.repairs);
        let d = &perf.arrangement.vocals.doubles[0].notes;
        let (v0, v1) = (at(&p, 0), at(&p, p.form.sections[1].start_bar));
        assert!(
            d.iter().any(|n| n.t0 >= v0 && n.t0 < v1),
            "doubles in verse"
        );
        let c3 = at(&p, p.form.sections[3].start_bar);
        let c4 = at(&p, p.form.sections[4].start_bar);
        assert!(
            !d.iter().any(|n| n.t0 >= c3 && n.t0 < c4),
            "none in chorus 3"
        );
        assert_ne!(d.len(), before);
        // The second take follows the first.
        assert_eq!(
            perf.arrangement.vocals.doubles[1].notes.len(),
            d.len(),
            "both takes"
        );
    }

    #[test]
    fn choir_modes_off_pad_and_words() {
        let (s, mut p, mut perf) = full_arranged();
        let win = |p: &Prepared, sec: usize| {
            let f = &p.form.sections[sec];
            (at(p, f.start_bar), at(p, f.start_bar + f.n_bars))
        };
        let count = |perf: &Performance, (lo, hi): (f64, f64)| -> usize {
            perf.arrangement
                .vocals
                .choir
                .iter()
                .flatten()
                .flat_map(|s| s.notes.iter())
                .filter(|n| n.t0 >= lo + 0.03 && n.t0 < hi - 0.03)
                .count()
        };
        let default_pad = count(&perf, win(&p, 3));
        assert!(default_pad > 0, "the repeat chorus has the pad");
        assert_eq!(count(&perf, win(&p, 0)), 0);
        assert!(perf.choir_key.is_empty());
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [
                {"part": "choir", "sections": [3], "mode": "off"},
                {"part": "choir", "sections": [0], "mode": "pad"},
                {"part": "choir", "sections": [1], "mode": "unison"},
                {"part": "choir", "sections": [2], "mode": "block"}], "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 4);
        assert_eq!(count(&perf, win(&p, 3)), 0, "off");
        let pad = count(&perf, win(&p, 0));
        assert!(pad > 0, "pad in the verse");
        let aa = |sec: usize| {
            let (lo, hi) = win(&p, sec);
            perf.arrangement
                .vocals
                .choir
                .iter()
                .flatten()
                .flat_map(|s| s.notes.iter())
                .filter(|n| n.t0 >= lo + 0.03 && n.t0 < hi - 0.03)
                .all(|n| n.phones == [song::Phoneme::Aa])
        };
        assert!(aa(0), "the pad is /aa/");
        assert!(count(&perf, win(&p, 1)) > 0 && !aa(1), "unison sings words");
        assert!(count(&perf, win(&p, 2)) > 0 && !aa(2), "block sings words");
        // The choir's word sections key the ducker.
        assert_eq!(perf.choir_key.len(), 2 * p.form.sections[1].lines.len());
    }

    #[test]
    fn section_edits_are_validated() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.clone();
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [
                {"part": "harmony", "sections": [99], "on": true},
                {"part": "harmony", "sections": [0], "on": true, "interval": "tritone"},
                {"part": "choir", "sections": [0], "mode": "shout"},
                {"part": "choir", "sections": [4], "mode": "unison"},
                {"part": "doubles", "sections": ["s1"]}], "summary": ""}),
        );
        let text = a.repairs.join("\n");
        for needle in [
            "section 99 does not exist",
            "tritone",
            "shout",
            "no words",
            "no on",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in\n{text}");
        }
        // The tritone edit still applies, with the rules' interval.
        assert_eq!(a.applied, 1);
        assert_ne!(
            perf.arrangement.vocals.harmony,
            before.arrangement.vocals.harmony
        );
        // A song whose band has no choir drops a choir edit.
        let mut s2 = full();
        s2.band.choir = false;
        let (mut p2, mut perf2) = arrange_song(&s2, 11, VoiceChoice::default());
        let a = run(
            &s2,
            &mut p2,
            &mut perf2,
            json!({"edits": [{"part": "choir", "sections": [1], "mode": "pad"}], "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(a.repairs[0].contains("choir is off in this song's band"));
    }

    #[test]
    fn a_lead_edit_moves_notes_and_keeps_the_words() {
        let (s, mut p, mut perf) = full_arranged();
        let before = p.comp.lead.clone();
        let l0 = &p.form.lines[0];
        let (lo, hi) = (l0.start_bar, l0.start_bar + l0.n_bars);
        let notes = lead_notes(&p, lo, hi, 2);
        assert!(notes.len() >= 6);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": notes}],
                   "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 1);
        assert_eq!(p.comp.lead.len(), before.len());
        for (n, o) in p.comp.lead.iter().zip(&before) {
            assert_eq!(n.syl, o.syl, "words and phonemes are kept");
            if n.line_idx == 0 {
                assert_eq!(n.midi, o.midi + 2);
                assert_eq!(n.t0, o.t0);
            } else {
                assert_eq!(n.midi, o.midi);
            }
        }
        // The sung lead has the new pitches.
        let lead = &perf.arrangement.vocals.lead.notes;
        assert_eq!(lead.len(), before.len());
        for (n, o) in lead.iter().zip(&before).filter(|(_, o)| o.line_idx == 0) {
            assert_eq!(n.midi, (o.midi + 2) as f32);
        }
        // A second edit, of another line, reads the first's notes too.
        let l1 = &p.form.lines[1];
        let (lo1, hi1) = (l1.start_bar, l1.start_bar + l1.n_bars);
        let notes = lead_notes(&p, lo1, hi1, -1);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo1, "to_bar": hi1, "notes": notes}],
                   "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
    }

    #[test]
    fn a_lead_edit_carries_through_to_the_harmony_and_the_sheet() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.arrangement.vocals.clone();
        // The first chorus, lifted: harmony and doubles derive from it.
        let c = &p.form.sections[1];
        let (lo, hi) = (c.start_bar, c.start_bar + c.n_bars);
        let notes = lead_notes(&p, lo, hi, 2);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": notes}],
                   "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        let v = &perf.arrangement.vocals;
        assert_ne!(v.harmony.notes, before.harmony.notes);
        // The sheet follows the edited lead.
        let sheet = crate::sheet::sheet_from(&s, 11, &p);
        assert!(!sheet.sections.is_empty());
    }

    #[test]
    fn a_lead_edit_may_split_a_syllable_and_merge_a_melisma() {
        let (s, mut p, mut perf) = full_arranged();
        let before = p.comp.lead.clone();
        let l0 = &p.form.lines[0];
        let (lo, hi) = (l0.start_bar, l0.start_bar + l0.n_bars);
        let mut notes = lead_notes(&p, lo, hi, 0);
        // Syllable #2 is "walk" (after "glo~2" and "ry"): sing it over three
        // notes, and merge "glo" (#0) into one note.
        let walk_at = notes
            .iter()
            .position(|n| n["syllable"] == 2)
            .expect("syllable 2");
        let w = notes[walk_at].clone();
        let (b, m) = (w["beat"].as_f64().unwrap(), w["midi"].as_f64().unwrap());
        let len = w["len"].as_f64().unwrap();
        notes[walk_at] = json!({"syllable": 2, "beat": b, "len": len / 3.0, "midi": m});
        notes.insert(
            walk_at + 1,
            json!({"syllable": 2, "beat": b + len / 3.0, "len": len / 3.0, "midi": m + 2.0}),
        );
        notes.insert(
            walk_at + 2,
            json!({"syllable": 2, "beat": b + 2.0 * len / 3.0, "len": len / 3.0, "midi": m + 4.0}),
        );
        // Merge: drop the second note of syllable 0.
        let second = notes
            .iter()
            .position(|n| n["syllable"] == 0)
            .map(|i| i + 1)
            .unwrap();
        assert_eq!(notes[second]["syllable"], 0);
        let first_len =
            notes[second - 1]["len"].as_f64().unwrap() + notes[second]["len"].as_f64().unwrap();
        notes.remove(second);
        notes[second - 1]["len"] = json!(first_len);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": notes}],
                   "summary": ""}),
        );
        assert!(a.repairs.is_empty(), "{:?}", a.repairs);
        assert_eq!(a.applied, 1);
        // One note fewer for "glo", two more for "walk".
        assert_eq!(p.comp.lead.len(), before.len() + 1);
        let line0: Vec<&LeadNote> = p.comp.lead.iter().filter(|n| n.line_idx == 0).collect();
        assert_eq!(p.form.lines[0].syls.len(), line0.len());
        for (k, n) in line0.iter().enumerate() {
            assert_eq!(n.i, k);
            assert_eq!(p.form.lines[0].syls[k], n.syl);
        }
        // "glo" is one plain note with its whole phoneme string.
        let glo = &line0[0];
        assert_eq!(glo.syl.notes, 1);
        let orig_glo: Vec<song::Phoneme> = before[0]
            .syl
            .phones
            .iter()
            .copied()
            .chain(before[1].syl.phones.iter().copied())
            .collect();
        let full_glo: Vec<song::Phoneme> = glo.syl.phones.clone();
        assert_eq!(full_glo.last(), orig_glo.last());
        assert!(full_glo.first().is_some_and(|p| p.is_consonant()));
        // "walk": onset on the first note, the vowel alone between, the coda
        // on the last; the later two are continuations.
        let w = line0.iter().position(|n| n.syl.text == "walk").unwrap();
        assert_eq!(line0[w].syl.notes, 3);
        assert!(line0[w]
            .syl
            .phones
            .first()
            .is_some_and(|p| p.is_consonant()));
        assert!(line0[w + 1].syl.is_continuation() && line0[w + 2].syl.is_continuation());
        assert_eq!(line0[w + 1].syl.phones.len(), 1);
        assert!(line0[w + 1].syl.phones[0].is_vowel());
        assert!(line0[w + 2]
            .syl
            .phones
            .last()
            .is_some_and(|p| p.is_consonant()));
        assert_eq!(line0[w].midi, m as i32);
        assert_eq!(line0[w + 2].midi, m as i32 + 4);
        let sung = &perf.arrangement.vocals.lead.notes;
        assert_eq!(sung.len(), p.comp.lead.len());
        let legato: Vec<bool> = sung.iter().take(line0.len()).map(|n| n.legato).collect();
        assert_eq!(legato.iter().filter(|&&l| l).count(), 2, "{legato:?}");
        assert!(sung
            .iter()
            .take(line0.len())
            .enumerate()
            .all(|(k, n)| !n.legato || (k > 0 && !n.phrase_start)));
    }

    #[test]
    fn invalid_lead_edits_are_rejected_with_a_repair() {
        let (s, mut p, mut perf) = full_arranged();
        let before_p = p.comp.lead.clone();
        let before_perf = perf.clone();
        let l0 = &p.form.lines[0];
        let (lo, hi) = (l0.start_bar, l0.start_bar + l0.n_bars);
        let good = lead_notes(&p, lo, hi, 0);
        let check = |p: &Prepared, perf: &Performance| {
            assert_eq!(p.comp.lead.len(), before_p.len());
            for (a, b) in p.comp.lead.iter().zip(&before_p) {
                assert_eq!((a.midi, a.beat, a.i), (b.midi, b.beat, b.i));
            }
            assert_eq!(*perf, before_perf);
        };
        // A dropped syllable.
        let mut dropped = good.clone();
        dropped.retain(|n| n["syllable"] != 3);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": dropped}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(
            a.repairs.len() == 1 && a.repairs[0].contains("missing #3"),
            "{:?}",
            a.repairs
        );
        assert!(a.repairs[0].contains("never dropped, added or reordered"));
        check(&p, &perf);
        // Two syllables swapped.
        let mut swapped = good.clone();
        let (i, j) = (
            swapped.iter().position(|n| n["syllable"] == 3).unwrap(),
            swapped.iter().position(|n| n["syllable"] == 4).unwrap(),
        );
        swapped.swap(i, j);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": swapped}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(a.repairs[0].contains("out of order"), "{:?}", a.repairs);
        check(&p, &perf);
        // A syllable from another bar.
        let mut extra = good.clone();
        extra.push(json!({"syllable": 99, "beat": 7.5, "len": 0.5, "midi": 60}));
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": extra}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(
            a.repairs[0].contains("not in these bars: #99"),
            "{:?}",
            a.repairs
        );
        check(&p, &perf);
        // Notes that do not rise in time.
        let mut loop_back = good.clone();
        loop_back[1]["beat"] = json!(0.0);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": loop_back}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(a.repairs[0].contains("beats must rise"), "{:?}", a.repairs);
        check(&p, &perf);
        // A range with no sung syllable.
        let last = p.form.bars.len() - 1;
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": last, "to_bar": last + 1, "notes": []}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 0);
        assert!(
            a.repairs[0].contains("no sung syllable begins"),
            "{:?}",
            a.repairs
        );
        check(&p, &perf);
    }

    #[test]
    fn lead_pitches_outside_the_singers_range_fold_by_octaves() {
        let (s, mut p, mut perf) = full_arranged();
        let l0 = &p.form.lines[0];
        let (lo, hi) = (l0.start_bar, l0.start_bar + l0.n_bars);
        let mut notes = lead_notes(&p, lo, hi, 0);
        notes[0]["midi"] = json!(100);
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [{"part": "lead", "from_bar": lo, "to_bar": hi, "notes": notes}],
                   "summary": ""}),
        );
        assert_eq!(a.applied, 1);
        assert_eq!(a.repairs.len(), 1, "{:?}", a.repairs);
        assert!(a.repairs[0].contains("outside the singer's range"));
        assert!(p.comp.lead[0].midi < 70);
    }

    #[test]
    fn no_edit_of_a_new_kind_leaves_the_performance_and_the_lead_alone() {
        let (s, mut p, mut perf) = full_arranged();
        let before = perf.clone();
        let lead = p.comp.lead.clone();
        let a = run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [], "mix": {"duck_db": 3}, "summary": "x"}),
        );
        assert_eq!(perf, before);
        assert!(a.mix.is_some());
        assert_eq!(p.comp.lead.len(), lead.len());
    }

    #[test]
    fn the_rule_based_vocals_equal_the_plan_with_empty_overrides() {
        let (s, p, perf) = full_arranged();
        let again = arrange::vocals::plan(&s, &p, 11, Some(&Overrides::default()));
        assert_eq!(again, perf.arrangement.vocals);
    }

    #[test]
    fn an_edited_full_performance_survives_a_json_round_trip() {
        let (s, mut p, mut perf) = full_arranged();
        run(
            &s,
            &mut p,
            &mut perf,
            json!({"edits": [
                {"part": "guitar", "from_bar": 0, "to_bar": 2, "strokes": [
                    {"beat": 0, "dir": "down", "strings": "low", "vel": 0.9}]},
                {"part": "choir", "sections": [1], "mode": "unison"}], "summary": ""}),
        );
        let text = serde_json::to_string(&perf).unwrap();
        let back: Performance = serde_json::from_str(&text).unwrap();
        assert_eq!(back, perf);
    }
}

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
//! Editable parts: drums, bass, violin and the harmony guitar's lead notes.
//! The vocals (shown for reference), the accompaniment guitar, the harp, the
//! harmony guitar's arpeggio and the choir are not editable in this
//! version.
//!
//! Repairs. A reply that is wrong in a small way is repaired and each repair
//! is a line of text in `Applied::repairs`: bars outside the song are
//! dropped or clamped, velocity is clamped to 0..1, a pitch outside the
//! instrument's range is moved by octaves into it, a note with a missing
//! length or velocity takes a default. Anything that cannot be read is
//! dropped, never guessed.

use std::fmt::Write as _;

use compose::prepare::Prepared;
use compose::timeline::Timeline;
use serde_json::{json, Map, Value};
use song::events::{BowNote, DrumHit, DrumKind, PluckNote};
use song::{DrumKit, Song};

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

/// The editable parts, by the names the edits use.
pub const PARTS: [&str; 4] = ["drums", "bass", "violin", "harmony_guitar"];
/// Drum kind names, as `song::events::DrumKind` spells them.
pub const DRUM_KINDS: [&str; 9] = [
    "Kick", "Snare", "Rim", "Tap", "Swish", "Hat", "Shaker", "Tom", "Ride",
];

/// MIDI range of each pitched part: bass E1 to C4, violin G3 to E7, the
/// harmony guitar E2 to E6.
const BASS_RANGE: (f64, f64) = (28.0, 60.0);
const VIOLIN_RANGE: (f64, f64) = (55.0, 100.0);
const HG_RANGE: (f64, f64) = (40.0, 88.0);

/// A pitched part the edits can replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Drums,
    Bass,
    Violin,
    HarmonyGuitar,
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
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Part::Drums => "drums",
            Part::Bass => "bass",
            Part::Violin => "violin",
            Part::HarmonyGuitar => "harmony_guitar",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            Part::Bass => BASS_RANGE,
            Part::Violin => VIOLIN_RANGE,
            _ => HG_RANGE,
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
        "band: drums {kit}, bass {}, violin {}, harmony_guitar {}. A part that is off cannot be edited.",
        on(band.bass),
        on(band.violin),
        on(band.harmony_guitar),
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
        let _ = writeln!(o, "{line}");
    }

    // Events by bar, for each part.
    let mut lead: Vec<Vec<String>> = vec![Vec::new(); nbars];
    for n in &arr.vocals.lead.notes {
        let (b, beat) = locate(tl, bpb, n.t0);
        if let Some(v) = usize::try_from(b).ok().and_then(|b| lead.get_mut(b)) {
            v.push(format!(
                "{}+{} {}",
                num(beat),
                num(len_beats(tl, n.t0, n.t1)),
                num(n.midi as f64)
            ));
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
        "\nBARS. Notes are listed as BEAT+LENGTH MIDI VELOCITY (beats counted from the start of the bar, from 0); drum hits as BEAT KIND VELOCITY, where Tom(N) is N Hz and Swish(N) lasts N beats. \"vocal\" is the sung melody, for reference: it cannot be edited."
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
            ("violin", &violin),
            ("harmony_guitar", &hg),
        ] {
            if !v[i].is_empty() {
                let _ = writeln!(o, "  {name}: {}", v[i].join("; "));
            }
        }
    }
    o
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ----------------------------------------------------------------- edits

/// The JSON schema of the model's reply (closed objects throughout, usable
/// as `--json-schema` and as `output_config.format.schema`).
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
                "description": "Each edit replaces one part's events in bars from_bar up to, not including, to_bar with exactly the notes given. Bars you leave out keep the rule-based arrangement.",
                "items": {
                    "type": "object",
                    "properties": {
                        "part": {"type": "string", "enum": PARTS},
                        "from_bar": {"type": "integer"},
                        "to_bar": {"type": "integer"},
                        "notes": {"type": "array", "items": {"anyOf": [pitched, drum]}}
                    },
                    "required": ["part", "from_bar", "to_bar", "notes"],
                    "additionalProperties": false
                }
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
    let len = len_of(1.0, rep);
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

/// Validates `edits` against `prepared` and applies them, in order, to
/// `perf`. Returns the repairs, the validated mix and the summary.
pub fn apply(prepared: &Prepared, perf: &mut Performance, edits: &ArrangerEdits) -> Applied {
    let tl = &prepared.timeline;
    let bpb = prepared.form.bpb() as usize;
    let nbars = prepared.form.bars.len();
    let mut out = Applied {
        summary: edits.summary.clone(),
        ..Applied::default()
    };
    let rep = &mut out.repairs;

    for (ei, e) in edits.edits.iter().enumerate() {
        let Some(o) = e.as_object() else {
            rep.push(format!("edit {ei}: not an object, dropped"));
            continue;
        };
        let part_text = o.get("part").and_then(Value::as_str).unwrap_or("");
        let Some(part) = Part::parse(part_text) else {
            rep.push(format!(
                "edit {ei}: part {part_text:?} is not editable (want one of {}), dropped",
                PARTS.join(", ")
            ));
            continue;
        };
        let (Some(from), Some(to)) = (get_int(o, "from_bar"), get_int(o, "to_bar")) else {
            rep.push(format!("edit {ei}: from_bar or to_bar missing, dropped"));
            continue;
        };
        if from >= nbars as i64 || to <= 0 || from >= to {
            rep.push(format!(
                "edit {ei}: bars {from}..{to} are outside the song's {nbars} bars (or empty), dropped"
            ));
            continue;
        }
        let (lo, hi) = (from.max(0) as usize, to.min(nbars as i64) as usize);
        if (lo as i64, hi as i64) != (from, to) {
            rep.push(format!(
                "edit {ei}: bars {from}..{to} clamped to {lo}..{hi}"
            ));
        }
        let band = &perf.band;
        let part_on = match part {
            Part::Drums => perf.arrangement.drums.is_some() && band.drums != DrumKit::None,
            Part::Bass => band.bass,
            Part::Violin => band.violin,
            Part::HarmonyGuitar => band.harmony_guitar,
        };
        if !part_on {
            rep.push(format!(
                "edit {ei}: {} is off in this song's band, dropped",
                part.name()
            ));
            continue;
        }
        let Some(raw) = o.get("notes").and_then(Value::as_array) else {
            rep.push(format!("edit {ei}: no notes list, dropped"));
            continue;
        };
        let span = ((hi - lo) * bpb) as f64;
        // Beats are relative to the reply's from_bar; a clamped start moves
        // them. Shift by the bars cut off the front.
        let shift = ((lo as i64 - from) * bpb as i64) as f64;
        let mut notes: Vec<Note> = Vec::new();
        for (ni, v) in raw.iter().enumerate() {
            let mut n = v.clone();
            if let Some(b) = v.get("beat").and_then(Value::as_f64) {
                if shift != 0.0 {
                    n["beat"] = json!(b - shift);
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
                    continue;
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
            Part::Bass | Part::HarmonyGuitar => {
                let list = if part == Part::Bass {
                    &mut arr.bass
                } else {
                    &mut arr.harmony_guitar.lead
                };
                keep_outside(list, |n: &PluckNote| n.t0, tl, bpb, (lo, hi));
                list.extend(notes.iter().map(|n| PluckNote {
                    t0: time(n.beat),
                    t1: time(n.beat + n.len),
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
        out.applied += 1;
    }

    if let Some(m) = &edits.mix {
        out.mix = read_mix(m, &mut out.repairs);
    }
    out
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
        let (_, p, mut perf) = arranged();
        let before = perf.clone();
        let a = apply(&p, &mut perf, &edits(json!({"edits": [], "summary": ""})));
        assert_eq!(perf, before);
        assert!(a.repairs.is_empty() && a.applied == 0 && a.mix.is_none());
        assert!(a.summary.is_none());
    }

    #[test]
    fn a_kick_on_every_beat_lands_at_the_timeline_seconds() {
        let (_, p, mut perf) = arranged();
        let bpb = p.form.bpb() as usize;
        let notes: Vec<Value> = (0..bpb * 2)
            .map(|b| json!({"beat": b, "drum": "Kick", "vel": 0.9}))
            .collect();
        let a = apply(
            &p,
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
        let (_, p, mut perf) = arranged();
        let bpb = p.form.bpb() as f64;
        let before = perf.arrangement.violin.clone();
        let a = apply(
            &p,
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
        let (_, p, mut perf) = arranged();
        let a = apply(
            &p,
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
        let (_, p, mut perf) = arranged();
        let nbars = p.form.bars.len() as i64;
        let before = perf.clone();
        let a = apply(
            &p,
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
        let (_, p, mut perf) = arranged();
        let a = apply(
            &p,
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
        let (p, mut perf) = arrange_song(&s, 1, VoiceChoice::default());
        let before = perf.clone();
        let text = view(&s, &p, &perf);
        assert!(text.contains("violin off") && text.contains("drums none"));
        assert!(!text.contains("  violin:") && !text.contains("  drums:"));
        let a = apply(
            &p,
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
        let (_, p, mut perf) = arranged();
        apply(
            &p,
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

        let (s, p, mut perf) = arranged();
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
        let a = apply(&p, &mut perf, &e);
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
        let item = &s["properties"]["edits"]["items"];
        assert_eq!(item["additionalProperties"], false);
        assert_eq!(item["properties"]["part"]["enum"], json!(PARTS));
        for alt in item["properties"]["notes"]["items"]["anyOf"]
            .as_array()
            .unwrap()
        {
            assert_eq!(alt["additionalProperties"], false);
        }
        let tracks = &s["properties"]["mix"]["properties"]["tracks"];
        assert_eq!(tracks["additionalProperties"], false);
        for id in TrackId::ALL {
            assert!(tracks["properties"].get(id.name()).is_some());
        }
    }
}

//! The full multipart score model: one staff per sounding part of the
//! arrangement, quantised to the same bar grid so every staff's bars line
//! up. See `docs/features-2.md` section 6.
//!
//! Time to beats is `compose::timeline::Timeline::to_beat`; beats to grid
//! units reuse the lead sheet's `crate::score::Grid` (sixteenth units, or
//! eighths in 6/8). Onsets round to the nearest unit (this is also the
//! chord-merge rule: two onsets within half a unit round to the same unit
//! and so land in the same `Ev`, becoming one chord's heads). A note's end
//! rounds the same way, is pushed out to at least one unit after the onset,
//! and is cut at the next onset quantised in the same voice of the same
//! staff. A harp `PluckNote` with `t1 <= t0` has no written release: it
//! rings to that next onset, capped at the end of its bar. Rests fill the
//! remaining gaps and a whole silent bar is one whole-bar rest, both by
//! `Grid::split` (the lead sheet's rule for notatable values and ties).
//!
//! Pitch spelling reuses `crate::score::{key_fifths, key_alterations,
//! spell}` in the transposed key. Drum positions and noteheads follow
//! Weinberg, "Guide to Standardized Drumset Notation" (Percussive Arts
//! Society, 1998): see `drum_head`.

use compose::melody::LeadNote;
use compose::prepare::Prepared;
use compose::timeline::Timeline;
use song::events::DrumKind;
use song::{Mode, Pc, SectionKind, SingerId, Song, Voice};

use crate::score::{key_alterations, key_fifths, spell, Grid};

/// One sounding part of the score, in the order the score is printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartId {
    Lead,
    /// The duet's second lead, from `Vocals::lead_b`; absent (no staff) in
    /// a solo song, where that field is `None`.
    LeadB,
    Harmony,
    Doubles,
    ChoirS,
    ChoirA,
    ChoirT,
    ChoirB,
    Violin,
    Guitar,
    HarmonyGuitar,
    HarpUpper,
    HarpLower,
    Bass,
    Drums,
}

/// All parts, in score order.
const ALL_PARTS: [PartId; 15] = [
    PartId::Lead,
    PartId::LeadB,
    PartId::Harmony,
    PartId::Doubles,
    PartId::ChoirS,
    PartId::ChoirA,
    PartId::ChoirT,
    PartId::ChoirB,
    PartId::Violin,
    PartId::Guitar,
    PartId::HarmonyGuitar,
    PartId::HarpUpper,
    PartId::HarpLower,
    PartId::Bass,
    PartId::Drums,
];

/// Clef of a staff. `Bass8vb` (bass sounding an octave below what is
/// written) is for the bass guitar part only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clef {
    Treble,
    Treble8vb,
    Bass,
    Bass8vb,
    Percussion,
}

/// The bracket (or, for `Harp`, brace) a staff is drawn under.
///
/// The bass guitar is grouped with the other band instruments (`Band`):
/// the harp's brace covers only its own two staves, and drums are bracketed
/// on their own, so `Harp` and `Drums` each name only the staff or staves
/// that share that one bracket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Vocals,
    Choir,
    Band,
    Harp,
    Drums,
}

/// One staff of the score.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaffDef {
    pub part: PartId,
    /// Full name, shown on the first system. For `Lead` in a duet and for
    /// `LeadB`, this names the singer's actual voice type ("Voice A
    /// (Baritone)"), the full score's counterpart to the lead sheet's "A
    /// (Baritone)" / "B (Alto)" system labels (design 4.8); a solo song's
    /// `Lead` staff keeps the plain "Voice (A)" it always had.
    pub name: String,
    /// Abbreviation, shown on every system after the first.
    pub abbrev: String,
    pub clef: Clef,
    pub group: Group,
}

/// A notehead shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notehead {
    Normal,
    X,
    Slash,
}

/// One pitch of a chord, already spelled (or, for drums, positioned).
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    /// Diatonic staff position: C4 = 28, as `crate::score::NoteEv::step`.
    pub step: i32,
    pub accidental: Option<i32>,
    pub notehead: Notehead,
}

/// A chord (one or more heads sounding together) at one grid position.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteChord {
    pub heads: Vec<Head>,
    pub tie_in: bool,
    pub tie_out: bool,
    /// The Lead staff only.
    pub lyric: Option<String>,
    pub hyphen: bool,
    pub t0: f64,
    pub t1: f64,
    /// Drum swish only ("sw.").
    pub text: Option<String>,
}

/// A note or a rest in one voice of one bar; `s` and `d` in grid units from
/// the bar start.
#[derive(Clone, Debug, PartialEq)]
pub struct Ev {
    pub s: i64,
    pub d: i64,
    pub chord: Option<NoteChord>,
}

/// One staff's content for one bar: one voice, or two for drums (up, then
/// down/kick).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub voices: Vec<Vec<Ev>>,
}

/// One bar, common to every staff (bars align across staves).
#[derive(Clone, Debug, PartialEq)]
pub struct BarCol {
    pub bar: usize,
    /// Index into the form's sections.
    pub sec: usize,
    /// Break group: bars of one lyric line, or a run of bars without lyrics
    /// in one section (the lead sheet's chunking rule). Wave 1 starts a
    /// system per chunk.
    pub chunk: usize,
    pub label: Option<String>,
    /// Chord symbols, at unit `u` of the bar.
    pub chords: Vec<(i64, String)>,
    pub t0: f64,
    pub t1: f64,
    /// One cell per staff, same order as `FullScore::staves`.
    pub cells: Vec<Cell>,
}

/// The full multipart score: every sounding part, quantised to one common
/// bar grid. Build with `FullScore::new`.
#[derive(Clone, Debug, PartialEq)]
pub struct FullScore {
    pub title: String,
    pub caption: String,
    pub width: f64,
    pub(crate) meter: song::Meter,
    pub(crate) tempo: f64,
    pub(crate) fifths: i32,
    pub staves: Vec<StaffDef>,
    pub bars: Vec<BarCol>,
}

/// One part's own view: its staff and its bars, with runs of two or more
/// silent bars inside one section folded into `MultiRest` (the full score
/// itself never does this: bars must align across staves there).
#[derive(Clone, Debug, PartialEq)]
pub struct PartScore {
    pub staff: StaffDef,
    pub bars: Vec<PartBar>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PartBar {
    Bar {
        bar: usize,
        sec: usize,
        chunk: usize,
        label: Option<String>,
        chords: Vec<(i64, String)>,
        t0: f64,
        t1: f64,
        cell: Cell,
    },
    MultiRest {
        bars: usize,
    },
}

// ---------------------------------------------------------------------
// Raw events, before quantisation.
// ---------------------------------------------------------------------

/// A source note before quantisation, one voice of one staff.
struct RawNote {
    t0: f64,
    t1: f64,
    /// Harp only: no written release; rings to the next onset, capped at
    /// the bar end.
    ring: bool,
    midi: f64,
    /// Skips spelling: the drum staff's fixed staff position.
    step_override: Option<i32>,
    notehead: Notehead,
    lyric: Option<String>,
    hyphen: bool,
    text: Option<String>,
    /// 0, or for drums 0 (up: everything but the kick) and 1 (down: kick).
    voice: usize,
}

impl RawNote {
    fn pitched(t0: f64, t1: f64, midi: f64) -> RawNote {
        RawNote {
            t0,
            t1,
            ring: false,
            midi,
            step_override: None,
            notehead: Notehead::Normal,
            lyric: None,
            hyphen: false,
            text: None,
            voice: 0,
        }
    }
}

fn vocal_clef(v: Voice) -> Clef {
    match v {
        Voice::Bass | Voice::Baritone | Voice::Tenor => Clef::Treble8vb,
        Voice::Alto | Voice::Soprano => Clef::Treble,
    }
}

/// Static part text: (full name, abbreviation). `Lead` in a duet and
/// `LeadB` get the singer's actual voice type spliced into the full name
/// at the call site below (`StaffDef::name`'s own doc comment); this text
/// is what a solo song's `Lead` staff keeps unchanged.
fn name_of(part: PartId) -> (&'static str, &'static str) {
    match part {
        PartId::Lead => ("Voice (A)", "Voc."),
        PartId::LeadB => ("Voice B", "Voc. B"),
        PartId::Harmony => ("Harmony", "Hrm."),
        PartId::Doubles => ("Doubles", "Dbl."),
        PartId::ChoirS => ("Soprano", "S."),
        PartId::ChoirA => ("Alto", "A."),
        PartId::ChoirT => ("Tenor", "T."),
        PartId::ChoirB => ("Bass", "B."),
        PartId::Violin => ("Violin", "Vln."),
        PartId::Guitar => ("Guitar", "Gtr."),
        PartId::HarmonyGuitar => ("Harmony guitar", "H.Gtr."),
        PartId::HarpUpper | PartId::HarpLower => ("Harp", "Hp."),
        PartId::Bass => ("Bass", "Bs."),
        PartId::Drums => ("Drums", "Dr."),
    }
}

fn group_of(part: PartId) -> Group {
    match part {
        PartId::Lead | PartId::LeadB | PartId::Harmony | PartId::Doubles => Group::Vocals,
        PartId::ChoirS | PartId::ChoirA | PartId::ChoirT | PartId::ChoirB => Group::Choir,
        PartId::Violin | PartId::Guitar | PartId::HarmonyGuitar | PartId::Bass => Group::Band,
        PartId::HarpUpper | PartId::HarpLower => Group::Harp,
        PartId::Drums => Group::Drums,
    }
}

/// Diatonic staff step (C4 = 28) of a natural note: `letter` 0=C..6=B,
/// `octave` a MIDI-style octave (C4 is octave 4).
fn nat_step(letter: i32, octave: i32) -> i32 {
    letter + 7 * (octave - 4) + 28
}

/// Drum staff position and notehead (Weinberg, PAS 1998): step (C4 = 28),
/// notehead, and swish text.
fn drum_head(kind: DrumKind) -> (i32, Notehead, Option<&'static str>) {
    match kind {
        DrumKind::Kick => (nat_step(3, 4), Notehead::Normal, None), // F4, bottom space
        DrumKind::Snare | DrumKind::Tap => (nat_step(0, 5), Notehead::Normal, None), // C5, third space
        DrumKind::Rim => (nat_step(0, 5), Notehead::X, None),
        DrumKind::Swish { .. } => (nat_step(0, 5), Notehead::Slash, Some("sw.")),
        DrumKind::Hat => (nat_step(4, 5), Notehead::X, None), // G5, above the staff
        DrumKind::Ride => (nat_step(3, 5), Notehead::X, None), // F5, top line
        DrumKind::Tom { hz } => {
            let step = if hz >= 150.0 {
                nat_step(2, 5)
            } else {
                nat_step(5, 4)
            }; // E5 or A4
            (step, Notehead::Normal, None)
        }
        DrumKind::Shaker => (nat_step(5, 5), Notehead::X, None), // A5, line above the staff
    }
}

fn drum_voice(kind: DrumKind) -> usize {
    usize::from(matches!(kind, DrumKind::Kick))
}

/// Raw notes for one part, or `None` when the part is off or has no
/// arrangement.
fn raw_notes_of(
    part: PartId,
    song: &Song,
    prep: &Prepared,
    arr: &arrange::Arrangement,
) -> Option<Vec<RawNote>> {
    use song::events::{BowNote, PluckNote, StringNote, VocalNote};

    let vocal = |notes: &[VocalNote]| -> Vec<RawNote> {
        notes
            .iter()
            .map(|n| RawNote::pitched(n.t0, n.t1, n.midi as f64))
            .collect()
    };
    let bow = |notes: &[BowNote]| -> Vec<RawNote> {
        notes
            .iter()
            .map(|n| RawNote::pitched(n.t0, n.t1, n.midi as f64))
            .collect()
    };
    let pluck = |notes: &[PluckNote], ring: bool| -> Vec<RawNote> {
        notes
            .iter()
            .map(|n| RawNote {
                ring: ring && n.t1 <= n.t0,
                ..RawNote::pitched(n.t0, n.t1, n.midi as f64)
            })
            .collect()
    };
    let strings = |lists: &[Vec<StringNote>; 6]| -> Vec<RawNote> {
        lists
            .iter()
            .flatten()
            .map(|n| RawNote::pitched(n.t, n.stop, n.midi as f64))
            .collect()
    };

    Some(match part {
        PartId::Lead => prep
            .comp
            .lead
            .iter()
            .map(|n| RawNote {
                lyric: Some(n.syl.text.clone()),
                hyphen: !n.syl.word_end,
                ..RawNote::pitched(n.t0, n.t1, n.midi as f64)
            })
            .collect(),
        PartId::LeadB => {
            let b = arr.vocals.lead_b.as_ref()?;
            // `b.notes` (`song::events::VocalNote`) carries no syllable
            // text: it is built from `singer_notes(SingerId::B, ...)`
            // (`arrange::vocals::singer_notes`) via a 1:1, order-preserving
            // map (`compose::prepare::vocal_notes` then `vocals::event`,
            // neither of which filters, merges or reorders), so the same
            // filter-and-sort over `prep.comp.lead`/`comp.second`
            // (`compose::melody::LeadNote`, which does carry the lyric)
            // reproduces singer B's notes in the same order and count and
            // supplies the lyric `b.notes` itself cannot.
            let mut lyric_notes: Vec<&LeadNote> = prep
                .comp
                .lead
                .iter()
                .chain(prep.comp.second.iter())
                .filter(|n| n.singer == SingerId::B)
                .collect();
            lyric_notes.sort_by(|a, b| a.t0.partial_cmp(&b.t0).expect("finite t0"));
            debug_assert_eq!(
                b.notes.len(),
                lyric_notes.len(),
                "LeadB voice and lyric note counts disagree"
            );
            b.notes
                .iter()
                .zip(lyric_notes)
                .map(|(n, ln)| RawNote {
                    lyric: Some(ln.syl.text.clone()),
                    hyphen: !ln.syl.word_end,
                    ..RawNote::pitched(n.t0, n.t1, n.midi as f64)
                })
                .collect()
        }
        PartId::Harmony => {
            if !song.band.harmonies {
                return None;
            }
            vocal(&arr.vocals.harmony.notes)
        }
        PartId::Doubles => {
            if !song.band.doubles {
                return None;
            }
            vocal(&arr.vocals.doubles[0].notes)
        }
        PartId::ChoirS | PartId::ChoirA | PartId::ChoirT | PartId::ChoirB => {
            if !song.band.choir {
                return None;
            }
            let idx = match part {
                PartId::ChoirB => 0,
                PartId::ChoirT => 1,
                PartId::ChoirA => 2,
                _ => 3,
            };
            let form = &prep.form;
            let tl = &prep.timeline;
            arrange::choir::voicings(form, tl, arrange::choir::sings_here)
                .iter()
                .map(|v| {
                    let sg = &tl.segs[v.seg];
                    RawNote::pitched(tl.to_time(sg.b0), tl.to_time(sg.b1), v.notes[idx] as f64)
                })
                .collect()
        }
        PartId::Violin => {
            if !song.band.violin {
                return None;
            }
            bow(&arr.violin)
        }
        PartId::Guitar => strings(&arr.guitar),
        PartId::HarmonyGuitar => {
            if !song.band.harmony_guitar {
                return None;
            }
            let mut v = pluck(&arr.harmony_guitar.lead, false);
            v.extend(pluck(&arr.harmony_guitar.arp, false));
            v
        }
        PartId::HarpUpper | PartId::HarpLower => {
            if !song.band.harp {
                return None;
            }
            pluck(&arr.harp, true)
                .into_iter()
                .filter(|n| (n.midi >= 60.0) == (part == PartId::HarpUpper))
                .collect()
        }
        PartId::Bass => {
            if !song.band.bass {
                return None;
            }
            pluck(&arr.bass, false)
        }
        PartId::Drums => {
            let Some(hits) = &arr.drums else { return None };
            hits.iter()
                .map(|h| {
                    let (step, notehead, text) = drum_head(h.kind);
                    let t1 = if let DrumKind::Swish { dur } = h.kind {
                        h.t + dur as f64
                    } else {
                        h.t
                    };
                    RawNote {
                        t0: h.t,
                        t1,
                        ring: false,
                        midi: 0.0,
                        step_override: Some(step),
                        notehead,
                        lyric: None,
                        hyphen: false,
                        text: text.map(str::to_string),
                        voice: drum_voice(h.kind),
                    }
                })
                .collect()
        }
    })
}

// ---------------------------------------------------------------------
// Quantisation.
// ---------------------------------------------------------------------

#[derive(Clone)]
struct Cluster {
    gu0: i64,
    end_gu: Option<i64>,
    heads: Vec<(f64, Option<i32>, Notehead)>,
    lyric: Option<String>,
    hyphen: bool,
    text: Option<String>,
}

/// Beats-to-units, rounded to the nearest unit: this rounding is also the
/// chord-merge rule (onsets within half a unit land on the same unit).
fn unit_of(tl: &Timeline, grid: &Grid, sec: f64) -> i64 {
    let beat = tl.to_beat(sec);
    (beat * grid.beat_u as f64).round() as i64
}

/// One voice's raw notes into per-bar `Ev` lists (rests fill the gaps, a
/// silent bar is one whole rest), spelling pitched heads in `fifths` with
/// key alterations `key_alt`, resetting the spelling state each bar.
fn quantize_voice(
    notes: &[&RawNote],
    tl: &Timeline,
    grid: &Grid,
    n_bars: usize,
    fifths: i32,
    key_alt: &[i32; 7],
    written: i32,
) -> Vec<Vec<Ev>> {
    let bar_u = grid.bar_u;
    let total = n_bars as i64 * bar_u;
    if total <= 0 {
        return vec![Vec::new(); n_bars];
    }

    let mut sorted: Vec<&RawNote> = notes.to_vec();
    sorted.sort_by(|a, b| a.t0.partial_cmp(&b.t0).unwrap_or(std::cmp::Ordering::Equal));

    let mut items: Vec<Cluster> = Vec::new();
    for n in sorted {
        let gu0 = unit_of(tl, grid, n.t0).clamp(0, total - 1);
        let end_gu = if n.ring {
            None
        } else {
            Some(unit_of(tl, grid, n.t1).max(gu0 + 1).min(total))
        };
        let head = (n.midi, n.step_override, n.notehead);
        match items.last_mut().filter(|c| c.gu0 == gu0) {
            Some(c) => {
                c.heads.push(head);
                if let Some(g1) = end_gu {
                    c.end_gu = Some(c.end_gu.map_or(g1, |e| e.min(g1)));
                }
                c.text = c.text.take().or_else(|| n.text.clone());
            }
            None => items.push(Cluster {
                gu0,
                end_gu,
                heads: vec![head],
                lyric: n.lyric.clone(),
                hyphen: n.hyphen,
                text: n.text.clone(),
            }),
        }
    }

    // Cap each cluster's end at the next cluster's onset. A ring note (harp:
    // no measured release, end_gu is None going in) additionally stops at its
    // own bar end, since it has no sustain to carry across the barline. Any
    // other note keeps its measured end and may cross into later bars; the
    // per-bar split below then emits tie_out/tie_in across the barline.
    let n_items = items.len();
    for i in 0..n_items {
        let is_ring = items[i].end_gu.is_none();
        let next = items.get(i + 1).map_or(total, |c| c.gu0);
        let cap = if is_ring {
            let bar_end = (items[i].gu0.div_euclid(bar_u) + 1) * bar_u;
            next.min(bar_end).min(total)
        } else {
            next.min(total)
        };
        let raw_end = items[i].end_gu.unwrap_or(cap);
        items[i].end_gu = Some(raw_end.min(cap).max(items[i].gu0 + 1));
    }

    let mut per_bar: Vec<Vec<Ev>> = vec![Vec::new(); n_bars];
    for (bar, out) in per_bar.iter_mut().enumerate() {
        let b0 = bar as i64 * bar_u;
        let b1 = b0 + bar_u;
        let mut c = 0i64;
        let mut state: Vec<(i32, i32)> = Vec::new();
        for it in &items {
            let end = it.end_gu.unwrap();
            if end <= b0 || it.gu0 >= b1 {
                continue;
            }
            let s = it.gu0.max(b0);
            let e = end.min(b1);
            if s - b0 > c {
                for (rs, rd) in grid.split(c, s - b0 - c, true) {
                    out.push(Ev {
                        s: rs,
                        d: rd,
                        chord: None,
                    });
                }
            }
            let starts_here = it.gu0 >= b0;
            let parts = grid.split(s - b0, e - s, false);
            let np = parts.len();
            for (j, (ps, pd)) in parts.into_iter().enumerate() {
                let tie_in = !starts_here || j > 0;
                let tie_out = e < end || j + 1 < np;
                let heads = it
                    .heads
                    .iter()
                    .map(|&(midi, step_override, notehead)| match step_override {
                        Some(st) => Head {
                            step: st,
                            accidental: None,
                            notehead,
                        },
                        None => {
                            let (l, a, step) =
                                spell(midi.round() as i32 + written, fifths, key_alt);
                            let cur = state
                                .iter()
                                .find(|x| x.0 == step)
                                .map_or(key_alt[l], |x| x.1);
                            let accidental = if a != cur && !tie_in {
                                state.retain(|x| x.0 != step);
                                state.push((step, a));
                                Some(a)
                            } else {
                                None
                            };
                            Head {
                                step,
                                accidental,
                                notehead,
                            }
                        }
                    })
                    .collect();
                let g0 = b0 + ps;
                let g1 = g0 + pd;
                out.push(Ev {
                    s: ps,
                    d: pd,
                    chord: Some(NoteChord {
                        heads,
                        tie_in,
                        tie_out,
                        lyric: (j == 0 && starts_here).then(|| it.lyric.clone()).flatten(),
                        hyphen: j == 0 && starts_here && it.hyphen,
                        t0: tl.to_time(g0 as f64 * grid.unit()),
                        t1: tl.to_time(g1 as f64 * grid.unit()),
                        text: (j == 0).then(|| it.text.clone()).flatten(),
                    }),
                });
            }
            c = e - b0;
        }
        if c < bar_u {
            for (rs, rd) in grid.split(c, bar_u - c, true) {
                out.push(Ev {
                    s: rs,
                    d: rd,
                    chord: None,
                });
            }
        }
        out.sort_by_key(|e| e.s);
    }
    per_bar
}

/// Builds one staff's `Cell`s (one per bar) from its raw notes. `written`
/// is added to each pitch before spelling: 12 semitones for a clef whose
/// staff is written an octave from where it sounds (`Treble8vb`,
/// `Bass8vb`), 0 otherwise.
fn build_cells(
    raw: &[RawNote],
    tl: &Timeline,
    grid: &Grid,
    n_bars: usize,
    fifths: i32,
    key_alt: &[i32; 7],
    written: i32,
    drums: bool,
) -> Vec<Cell> {
    // The drum staff always carries two voices (hands up, feet down), even
    // when the kit plays no kick.
    let max_voice = raw
        .iter()
        .map(|r| r.voice)
        .max()
        .unwrap_or(0)
        .max(usize::from(drums));
    let per_voice: Vec<Vec<Vec<Ev>>> = (0..=max_voice)
        .map(|v| {
            let notes: Vec<&RawNote> = raw.iter().filter(|r| r.voice == v).collect();
            quantize_voice(&notes, tl, grid, n_bars, fifths, key_alt, written)
        })
        .collect();
    (0..n_bars)
        .map(|bar| Cell {
            voices: per_voice.iter().map(|pv| pv[bar].clone()).collect(),
        })
        .collect()
}

/// Section label: "Verse 2", "Chorus", "Break" (mirrors the lead sheet's
/// `section_label`; duplicated here since that one stays private to
/// `score.rs`).
fn section_label(kind: SectionKind, role: song::SectionRole, occ: usize, verses: usize) -> String {
    use song::SectionRole;
    match role {
        SectionRole::Break => return "Break".into(),
        SectionRole::Tag => return "Tag".into(),
        SectionRole::Plain => {}
    }
    let name = match kind {
        SectionKind::Intro => "Intro",
        SectionKind::Verse => "Verse",
        SectionKind::Prechorus => "Pre-chorus",
        SectionKind::Chorus => "Chorus",
        SectionKind::Bridge => "Bridge",
        SectionKind::Interlude => "Interlude",
        SectionKind::Outro => "Outro",
    };
    if kind == SectionKind::Verse && verses > 1 {
        format!("{name} {}", occ + 1)
    } else {
        name.to_string()
    }
}

impl FullScore {
    /// The full score of `song`, from its composition `prep` and its
    /// arrangement `arr`. A staff is included when its `song.band` flag is
    /// on (lead and guitar always) and it has at least one event.
    pub fn new(song: &Song, prep: &Prepared, arr: &arrange::Arrangement) -> FullScore {
        let form = &prep.form;
        let tl = &prep.timeline;
        let grid = Grid::of(song.meter);
        let n_bars = form.bars.len();
        let fifths = key_fifths(prep.tonic, song.mode);
        let key_alt = key_alterations(fifths);

        let verses = form
            .sections
            .iter()
            .filter(|x| x.kind == SectionKind::Verse)
            .count();
        let mut chunk = 0usize;
        let mut prev_key: Option<(usize, Option<usize>)> = None;
        let mut chords: Vec<Vec<(i64, String)>> = vec![Vec::new(); n_bars];
        let u = grid.unit();
        for seg in &tl.segs {
            let pos = (seg.b0 / u).round() as i64;
            let b = pos.div_euclid(grid.bar_u);
            if let Some(v) = chords.get_mut(b as usize) {
                v.push((pos - b * grid.bar_u, form.chord(seg.chord).symbol.clone()));
            }
        }
        let mut bar_meta = Vec::with_capacity(n_bars);
        for (bar, bi) in form.bars.iter().enumerate() {
            let key = (bi.sec, bi.line);
            if prev_key.is_some_and(|p| p != key) {
                chunk += 1;
            }
            prev_key = Some(key);
            let sec = bi.sec;
            let label = form
                .sections
                .get(sec)
                .filter(|s| bar == s.start_bar)
                .map(|s| section_label(s.kind, s.role, s.occ, verses));
            let b0 = (bar as i64 * grid.bar_u) as f64 * u;
            let b1 = ((bar + 1) as i64 * grid.bar_u) as f64 * u;
            bar_meta.push((
                bar,
                sec,
                chunk,
                label,
                std::mem::take(&mut chords[bar]),
                tl.to_time(b0),
                tl.to_time(b1),
            ));
        }

        let mut staves = Vec::new();
        let mut all_cells: Vec<Vec<Cell>> = Vec::new();
        for &part in &ALL_PARTS {
            let Some(raw) = raw_notes_of(part, song, prep, arr) else {
                continue;
            };
            if raw.is_empty() {
                continue;
            }
            let clef = match part {
                PartId::Lead => vocal_clef(prep.voice),
                PartId::LeadB => {
                    vocal_clef(arr.vocals.lead_b.as_ref().map_or(prep.voice, |b| b.voice))
                }
                PartId::Harmony => vocal_clef(arr.vocals.harmony.voice),
                PartId::Doubles => vocal_clef(arr.vocals.doubles[0].voice),
                PartId::ChoirS | PartId::ChoirA => Clef::Treble,
                PartId::ChoirT => Clef::Treble8vb,
                PartId::ChoirB => Clef::Bass,
                PartId::Violin => Clef::Treble,
                PartId::Guitar | PartId::HarmonyGuitar => Clef::Treble8vb,
                PartId::HarpUpper => Clef::Treble,
                PartId::HarpLower => Clef::Bass,
                PartId::Bass => Clef::Bass8vb,
                PartId::Drums => Clef::Percussion,
            };
            let (base_name, abbrev) = name_of(part);
            let name = match part {
                PartId::Lead if song.is_duet() => format!("Voice A ({})", prep.voice.label()),
                PartId::LeadB => {
                    let vb = arr.vocals.lead_b.as_ref().map_or(prep.voice, |b| b.voice);
                    format!("Voice B ({})", vb.label())
                }
                _ => base_name.to_string(),
            };
            let pitched = part != PartId::Drums;
            let written = if matches!(clef, Clef::Treble8vb | Clef::Bass8vb) {
                12
            } else {
                0
            };
            let cells = build_cells(
                &raw,
                tl,
                &grid,
                n_bars,
                if pitched { fifths } else { 0 },
                &key_alt,
                written,
                !pitched,
            );
            all_cells.push(cells);
            staves.push(StaffDef {
                part,
                name,
                abbrev: abbrev.to_string(),
                clef,
                group: group_of(part),
            });
        }

        let bars = bar_meta
            .into_iter()
            .enumerate()
            .map(|(bar, (b, sec, chunk, label, chs, t0, t1))| {
                debug_assert_eq!(bar, b);
                BarCol {
                    bar,
                    sec,
                    chunk,
                    label,
                    chords: chs,
                    t0,
                    t1,
                    cells: all_cells.iter().map(|c| c[bar].clone()).collect(),
                }
            })
            .collect();

        let flats = fifths < 0;
        let mode = match song.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
            Mode::Dorian => "Dorian",
            Mode::Mixolydian => "Mixolydian",
        };
        let caption = format!(
            "{}, {} {}",
            prep.voice.label(),
            Pc::new(prep.tonic).name(flats),
            mode
        );
        FullScore {
            title: song.title.clone(),
            caption,
            width: crate::score::DEFAULT_WIDTH,
            meter: song.meter,
            tempo: song.tempo_bpm,
            fifths,
            staves,
            bars,
        }
    }

    /// The same score laid out for a page `width` px wide (at least 300).
    pub fn with_width(mut self, width: f64) -> FullScore {
        self.width = if width.is_finite() {
            width.max(300.0)
        } else {
            crate::score::DEFAULT_WIDTH
        };
        self
    }

    /// One part's own staff and bars, silent runs of two or more bars
    /// inside one section folded into `MultiRest`.
    pub fn part(&self, part: PartId) -> Option<PartScore> {
        let idx = self.staves.iter().position(|s| s.part == part)?;
        let staff = self.staves[idx].clone();
        let is_silent = |c: &Cell| c.voices.iter().all(|v| v.iter().all(|e| e.chord.is_none()));

        let mut bars: Vec<PartBar> = Vec::new();
        let mut i = 0usize;
        while i < self.bars.len() {
            let b = &self.bars[i];
            let cell = &b.cells[idx];
            if is_silent(cell) {
                let mut j = i + 1;
                while j < self.bars.len()
                    && self.bars[j].sec == b.sec
                    && is_silent(&self.bars[j].cells[idx])
                {
                    j += 1;
                }
                let run = j - i;
                if run >= 2 {
                    bars.push(PartBar::MultiRest { bars: run });
                    i = j;
                    continue;
                }
            }
            bars.push(PartBar::Bar {
                bar: b.bar,
                sec: b.sec,
                chunk: b.chunk,
                label: b.label.clone(),
                chords: b.chords.clone(),
                t0: b.t0,
                t1: b.t1,
                cell: cell.clone(),
            });
            i += 1;
        }
        Some(PartScore { staff, bars })
    }
}

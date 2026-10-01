//! Melody: the per-song melody profile, cadence and contour choice, and the
//! composer of the sung lines and the instrumental lead lines. Each line is
//! set by `compose_line`: text setting (`rhythm::set_text`), then pitch
//! (`pitch::pitch_line`) over the harmony at each onset.

use std::collections::HashMap;

use sfcore::random::{tag, Rng, Tag};
use song::{Mode, Pc, PcSet, SectionKind, SingerId, Song, Syllable, Voice};

pub use crate::contour::{Contour, ContourKind};
pub use crate::pitch::{Cadence, PitchStyle};
pub use crate::rhythm::RhythmStyle;

use crate::form::Form;
use crate::pitch::{fit_hints, pitch_line, PitchProblem};
use crate::rhythm::{set_text, RhythmResult};
use crate::theory::local_scale;
use crate::timeline::Timeline;

/// Stream of the melody profile draws.
const PROFILE: Tag = tag("melody.profile");
/// Per-line text-setting streams, keyed by (section kind, line, 0).
const RHYTHM: Tag = tag("rhythm");
/// Per-line pitch streams, keyed by (section kind, line, occurrence).
const PITCH: Tag = tag("pitch");
/// Per-line grace-note streams, keyed by the line's first bar.
const GRACE: Tag = tag("grace");
/// Instrumental lead lines: note count and rhythm, keyed by (section, chunk).
const INST_RHYTHM: Tag = tag("inst.rhythm");
/// Instrumental lead lines: pitch, keyed by (section, chunk).
const INST_PITCH: Tag = tag("inst.pitch");

/// An event index from three small integers (21 bits each).
fn event_key(a: usize, b: usize, c: usize) -> u64 {
    const M: u64 = (1 << 21) - 1;
    ((a as u64 & M) << 42) | ((b as u64 & M) << 21) | (c as u64 & M)
}

/// Tessitura of a section: contour axis `c` and amplitude `a`, semitones
/// above the register pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionTess {
    pub c: f64,
    pub a: f64,
}

/// Tessitura of instrumental lead lines.
pub const INST_TESS: SectionTess = SectionTess { c: 7.0, a: 3.5 };

/// A pair of contour choices, for even and odd lines of a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapePair(pub ContourKind, pub ContourKind);

impl ShapePair {
    /// The contour of line `li`.
    pub fn for_li(&self, li: usize) -> ContourKind {
        if li % 2 == 1 {
            self.1
        } else {
            self.0
        }
    }
}

/// The per-song melody profile: melodic and rhythmic character, and the
/// tessitura and contours per section kind (tables indexed by `SectionKind`).
#[derive(Clone, Copy, Debug)]
pub struct MelodyProfile {
    pub pitch: PitchStyle,
    pub rhythm: RhythmStyle,
    /// Tessitura per section kind; intro, interlude and outro take the verse's.
    pub tess: [SectionTess; SectionKind::ALL.len()],
    /// Contour pair per section kind; `None` (a plain arch) for intro,
    /// interlude and outro.
    pub shape: [Option<ShapePair>; SectionKind::ALL.len()],
    /// Interval of the first chorus's first two notes; 0 for none.
    pub hook: i32,
}

impl MelodyProfile {
    pub fn tess(&self, kind: SectionKind) -> SectionTess {
        self.tess[kind as usize]
    }

    pub fn shape(&self, kind: SectionKind) -> Option<ShapePair> {
        self.shape[kind as usize]
    }
}

/// Candidate hook intervals (0: no hook).
const HOOKS: [i32; 8] = [5, 7, -5, 9, 12, 3, -7, 0];

/// Draws the melody profile for `seed` and the song's title.
///
/// Ranges: verse axis -1..3 semitones above the register; the chorus
/// `lift` 2-7 above the verse, the prechorus half-way, the bridge 1-6 above
/// the verse; amplitudes 2-5 semitones. Leap 0-0.85, repetition 0-0.8,
/// pitch noise 0.9-2.2. Rhythm: dot and even 0-1, syncopation 0.3-0.8 in
/// 35% of songs, rhythm noise 0.5-1.4. Prechorus contours lean upward.
pub fn melody_profile(seed: u64, song: &Song) -> MelodyProfile {
    let mut r = Rng::event(seed, PROFILE, tag(&song.title).0);
    let keys = ContourKind::KEYS;
    let pick = |r: &mut Rng, opts: &[ContourKind]| opts[r.below(opts.len() as u32) as usize];

    let vc = (r.uniform() * 4.0).round() as i32 - 1;
    let lift = 2 + (r.uniform() * 5.0).round() as i32;

    let leap = r.uniform() * 0.85;
    let rep = r.uniform() * 0.8;
    let noise = 0.9 + r.uniform() * 1.3;

    let verse = SectionTess {
        c: vc as f64,
        a: 2.0 + r.uniform() * 3.0,
    };
    let prechorus = SectionTess {
        c: (vc + (lift as f64 / 2.0).round() as i32) as f64,
        a: 2.0 + r.uniform() * 2.5,
    };
    let chorus = SectionTess {
        c: (vc + lift) as f64,
        a: 2.5 + r.uniform() * 3.0,
    };
    let bridge = SectionTess {
        c: (vc + 1 + (r.uniform() * 5.0).round() as i32) as f64,
        a: 2.0 + r.uniform() * 3.0,
    };

    let s_verse = ShapePair(pick(&mut r, &keys), pick(&mut r, &keys));
    let s_pre = ShapePair(
        pick(
            &mut r,
            &[ContourKind::Rise, ContourKind::PeakLate, ContourKind::Arch],
        ),
        ContourKind::Rise,
    );
    let s_chorus = ShapePair(pick(&mut r, &keys), pick(&mut r, &keys));
    let s_bridge = ShapePair(pick(&mut r, &keys), pick(&mut r, &keys));

    let hook = HOOKS[r.below(HOOKS.len() as u32) as usize];

    let dot = r.uniform();
    let even = r.uniform();
    let sync = if r.uniform() < 0.35 {
        0.3 + r.uniform() * 0.5
    } else {
        0.0
    };
    let rnoise = 0.5 + r.uniform() * 0.9;

    let mut tess = [verse; SectionKind::ALL.len()];
    tess[SectionKind::Prechorus as usize] = prechorus;
    tess[SectionKind::Chorus as usize] = chorus;
    tess[SectionKind::Bridge as usize] = bridge;
    let mut shape = [None; SectionKind::ALL.len()];
    shape[SectionKind::Verse as usize] = Some(s_verse);
    shape[SectionKind::Prechorus as usize] = Some(s_pre);
    shape[SectionKind::Chorus as usize] = Some(s_chorus);
    shape[SectionKind::Bridge as usize] = Some(s_bridge);

    MelodyProfile {
        pitch: PitchStyle { leap, rep, noise },
        rhythm: RhythmStyle {
            dot,
            even,
            sync,
            noise: rnoise,
        },
        tess,
        shape,
        hook,
    }
}

/// Cadence of line `li` of `nl` in a section of `kind`: the last line
/// closes on the tonic (open in a bridge or prechorus), odd lines are open.
pub fn cadence_for(kind: SectionKind, li: usize, nl: usize) -> Cadence {
    if li + 1 == nl {
        if matches!(kind, SectionKind::Bridge | SectionKind::Prechorus) {
            Cadence::Open
        } else {
            Cadence::Tonic
        }
    } else if li % 2 == 1 {
        Cadence::Open
    } else {
        Cadence::None
    }
}

/// One lead-vocal note. `t0`/`t1` (seconds) are set by `prepare`.
#[derive(Clone, Debug)]
pub struct LeadNote {
    pub beat: f64,
    pub dur: f64,
    pub midi: i32,
    pub syl: Syllable,
    /// Index into `Form::lines`.
    pub line_idx: usize,
    /// Syllable index within the line.
    pub i: usize,
    pub stress: bool,
    pub phrase_start: bool,
    pub phrase_end: bool,
    pub grace: Option<i32>,
    /// The note's section is lifted.
    pub lift: bool,
    /// The singer this note belongs to: the melody singer for a note in
    /// `Comp::lead`, the other singer for a note in `Comp::second`.
    pub singer: SingerId,
    pub t0: f64,
    pub t1: f64,
}

/// One instrumental-lead note.
#[derive(Clone, Debug)]
pub struct InstNote {
    pub beat: f64,
    pub dur: f64,
    pub midi: i32,
    pub lift: bool,
    /// The note comes from a written break tune (`written`): its pitch is
    /// kept, and the violin plays it as written. Until `prepare` places the
    /// tune, `midi` is `60 +` semitones above the tonic (`written::place`).
    pub written: bool,
}

/// The composed melody.
#[derive(Clone, Debug)]
pub struct Comp {
    pub lead: Vec<LeadNote>,
    pub inst: Vec<InstNote>,
    /// The other singer's notes on shared lines (design 4.5): same onsets,
    /// durations, syllables, phrase flags and lift as the melody, no grace
    /// notes. Filled by `prepare::compose_second`, after the transposition;
    /// empty from `compose_melody` and in a solo song.
    pub second: Vec<LeadNote>,
    /// Register pitch: the tonic near middle C (MIDI 54-66).
    pub t: i32,
    /// Tonic pitch class, 0-11.
    pub tonic: i32,
    /// Repairs made while composing (`Repair::TuneMoved`).
    pub repairs: Vec<song::Repair>,
}

/// The register pitch of a tonic: the tonic between F#3 and F#4 (MIDI 54-66).
pub fn register_of(tonic: i32) -> i32 {
    60 + tonic - if tonic > 6 { 12 } else { 0 }
}

/// The MIDI pitch each note of a tune hints: `do` is the tonic (pitch class
/// `tonic`) nearest `register`, then the degree and octave marks. A free
/// note (`.`) has no hint. Not yet fitted to the voice (`pitch::fit_hints`).
pub fn tune_hints(tune: &[song::TuneNote], tonic: Pc, register: i32) -> Vec<Option<i32>> {
    let d = (tonic.get() as i32 - register).rem_euclid(12);
    let base = register + if d > 6 { d - 12 } else { d };
    tune.iter()
        .map(|n| {
            n.pitch
                .map(|p| base + p.semis as i32 + 12 * p.octave as i32)
        })
        .collect()
}

/// The harmonic context of a composition: the form and timeline give the
/// chord at each beat; the tonic and mode give the local scale.
#[derive(Clone, Copy)]
pub struct Harmony<'a> {
    pub form: &'a Form,
    pub tl: &'a Timeline,
    pub tonic: Pc,
    pub mode: Mode,
    /// Register pitch (`register_of`).
    pub register: i32,
}

impl Harmony<'_> {
    /// Chord tones and local scale at each onset of a line starting at `beat0`.
    fn at(&self, beat0: f64, onsets: &[f64]) -> (Vec<PcSet>, Vec<PcSet>) {
        onsets
            .iter()
            .map(|&o| {
                let c = self.tl.chord_at(self.form, beat0 + o + 0.01);
                (c.tones, local_scale(self.tonic, self.mode, c))
            })
            .unzip()
    }
}

/// Everything that defines one line, vocal or instrumental.
#[derive(Clone, Copy, Debug)]
pub struct LineSpec<'a> {
    /// Stress of each syllable (or note).
    pub stresses: &'a [bool],
    pub n_bars: usize,
    /// First beat of the line in the song.
    pub beat0: f64,
    pub rhythm: RhythmStyle,
    pub pitch: PitchStyle,
    pub cadence: Cadence,
    pub contour: Contour,
    /// Contour axis, semitones above the register pitch.
    pub center: f64,
    /// A line to echo (MIDI).
    pub reference: Option<&'a [i32]>,
    /// Last pitch of the previous line.
    pub prev_end: Option<i32>,
    /// Preferred first interval; 0 for none.
    pub hook: i32,
    /// Notes that continue a melisma (see `PitchProblem::conts`); empty for
    /// a line with none.
    pub conts: &'a [bool],
    /// The MIDI pitch the writer hinted for each note (see
    /// `PitchProblem::hints`); empty for a line with none.
    pub hints: &'a [Option<i32>],
}

/// A composed line.
#[derive(Clone, Debug)]
pub struct LineMelody {
    pub rh: RhythmResult,
    pub pitches: Vec<i32>,
    /// Local scale at each onset.
    pub scales: Vec<PcSet>,
}

/// Composes one line: text setting on `rhythm_rng`, then pitch on
/// `pitch_rng`, over the harmony at each onset.
pub fn compose_line(
    spec: &LineSpec,
    h: &Harmony,
    rhythm_rng: &mut Rng,
    pitch_rng: &mut Rng,
) -> LineMelody {
    let grid = h.form.grid();
    let rh = set_text(spec.stresses, spec.n_bars, grid, &spec.rhythm, rhythm_rng);
    let (chord_pcs, scales) = h.at(spec.beat0, &rh.onsets);
    let problem = PitchProblem {
        onsets: &rh.onsets,
        durs: &rh.durs,
        weights: &rh.weights,
        chord_pcs: &chord_pcs,
        scales: &scales,
        register: h.register,
        tonic: h.tonic,
        center: spec.center,
        contour: spec.contour,
        cadence: spec.cadence,
        reference: spec.reference,
        prev_end: spec.prev_end,
        line_beats: rh.line_beats,
        style: spec.pitch,
        hook: spec.hook,
        conts: spec.conts,
        hints: spec.hints,
    };
    let pitches = pitch_line(&problem, pitch_rng);
    LineMelody {
        rh,
        pitches,
        scales,
    }
}

/// Cache key of a sung line: kind, line index, text, the chords under it as
/// (root, intervals), one list per bar (so the bar count and the bar
/// boundaries are part of the key), the melody singer (design 4.5, so a
/// line sung by A and the same line sung by B, whose register centre
/// differs, compose separately), and the section's key. Equal keys compose to equal lines, and the
/// key transposes with the song, so the cache hits the same lines in any key.
type LineKey = (
    SectionKind,
    usize,
    String,
    Vec<Vec<(u8, &'static [u8])>>,
    SingerId,
    // The section's key: a chorus repeated in a new key composes anew.
    (u8, Mode),
    // The writer's tune: a hinted line composes apart from a free one.
    Option<Vec<song::TuneNote>>,
);

fn line_key(form: &Form, li_idx: usize, kind: SectionKind) -> LineKey {
    let l = &form.lines[li_idx];
    let chords = form.bars[l.start_bar..l.start_bar + l.n_bars]
        .iter()
        .map(|b| {
            b.chords
                .as_slice()
                .iter()
                .map(|&id| {
                    let c = form.chord(id);
                    (c.root.get(), c.intervals)
                })
                .collect()
        })
        .collect();
    // A melisma changes the notes of the line, so it is part of the key.
    let text = if l.syls.iter().any(|s| s.notes > 1) {
        l.syls
            .iter()
            .filter(|s| !s.is_continuation())
            .map(|s| format!("{}~{}", s.text, s.notes))
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        l.text.clone()
    };
    let (tonic, mode) = form.sections[l.sec].key;
    (
        kind,
        l.li,
        text,
        chords,
        l.part.melody(),
        (tonic.get(), mode),
        l.tune.clone(),
    )
}

/// Grace notes: on a long (>= 1.5 beats) last note, with probability 0.55,
/// an upper neighbour from the scale (1-3 semitones); on a long stressed
/// note that falls, with probability 0.18, the previous pitch.
const GRACE_END_P: f64 = 0.55;
const GRACE_FALL_P: f64 = 0.18;

/// Composes every sung line (lines with the same kind, index, text, chords
/// and melody singer are composed once and repeat exactly) and the
/// instrumental lead lines of instrumental sections. Sets
/// `form.lines[*].pitches` and `.rh`. `voice_a` is singer A's chosen voice;
/// `voice_b`, `Some` in a duet, is singer B's: the duet register fit
/// (design 4.5, `voices::duet_register`) then offsets singer B's line
/// centre by `d` while composing and shifts B's melody notes by `12 * o`
/// afterward, so a shared line composed for A and the same line for B
/// (different centres) are cached separately (`LineKey`'s melody singer).
pub fn compose_melody(
    song: &Song,
    form: &mut Form,
    tl: &Timeline,
    seed: u64,
    voice_a: Voice,
    voice_b: Option<Voice>,
) -> Comp {
    let bpb = form.bpb();
    let tonic = (song.key.get() as i32 + form.transpose).rem_euclid(12);
    let t = register_of(tonic);
    let prof = melody_profile(seed, song);
    let (o, d) = match voice_b {
        Some(vb) => crate::voices::duet_register(voice_a, vb),
        None => (0, 0.0),
    };

    let mut cache: HashMap<LineKey, LineMelody> = HashMap::new();
    let mut first_occ: [Vec<Option<Vec<i32>>>; SectionKind::ALL.len()] = Default::default();
    let mut lead: Vec<LeadNote> = Vec::new();
    let mut prev_end: Option<i32> = None;
    let mut repairs: Vec<song::Repair> = Vec::new();
    let mut composed: Vec<Option<(RhythmResult, Vec<i32>)>> = vec![None; form.lines.len()];

    {
        let form_r: &Form = form;
        for li_idx in 0..form_r.lines.len() {
            let line = &form_r.lines[li_idx];
            let sec = &form_r.sections[line.sec];
            let (kind, li, nl) = (sec.kind, line.li, sec.lines.len());
            let line_beat = (line.start_bar as i32 * bpb) as f64;
            let key = line_key(form_r, li_idx, kind);
            let (sec_tonic, sec_mode) = sec.key;
            let h = Harmony {
                form: form_r,
                tl,
                tonic: sec_tonic,
                mode: sec_mode,
                register: t,
            };
            let singer = line.part.melody();

            if !cache.contains_key(&key) {
                let stresses: Vec<bool> = line.syls.iter().map(|s| s.stress).collect();
                let conts: Vec<bool> = line.syls.iter().map(Syllable::is_continuation).collect();
                let cadence = cadence_for(kind, li, nl);
                // A tune shorter or longer than the notes cannot come from
                // `normalize`; a mismatch means no hints.
                let hints: Vec<Option<i32>> = match &line.tune {
                    Some(tn) if tn.len() == stresses.len() => {
                        let (h, moved) = fit_hints(&tune_hints(tn, sec_tonic, t), t);
                        if moved != 0 {
                            repairs.push(song::Repair::TuneMoved {
                                section: line.sec,
                                line: li,
                                octaves: moved,
                            });
                        }
                        h
                    }
                    _ => Vec::new(),
                };
                let mut ts = prof.tess(kind);
                if singer == SingerId::B {
                    ts.c += d;
                }
                // Reference: the same line of the first section of this
                // kind, else the line two back in this section.
                let first = if sec.occ > 0 {
                    first_occ[kind as usize].get(li).and_then(Option::as_ref)
                } else {
                    None
                };
                let two_back = || {
                    (li >= 2)
                        .then(|| sec.lines[li - 2])
                        .and_then(|j| composed[j].as_ref())
                        .map(|(_, p)| p)
                };
                let reference = first.or_else(two_back).map(Vec::as_slice);
                let first_lift = matches!(sec.lift, Some(l) if l.index == 0);
                let spec = LineSpec {
                    stresses: &stresses,
                    n_bars: line.n_bars,
                    beat0: line_beat,
                    rhythm: prof.rhythm,
                    pitch: prof.pitch,
                    cadence,
                    contour: Contour::for_phrase(
                        li,
                        cadence == Cadence::Tonic,
                        ts.a,
                        prof.shape(kind).map(|p| p.for_li(li)),
                    ),
                    center: ts.c,
                    reference,
                    prev_end,
                    hook: if first_lift && li == 0 { prof.hook } else { 0 },
                    conts: &conts,
                    hints: &hints,
                };
                let mut rr = Rng::event(seed, RHYTHM, event_key(kind as usize, li, 0));
                let mut pr = Rng::event(seed, PITCH, event_key(kind as usize, li, sec.occ));
                let m = compose_line(&spec, &h, &mut rr, &mut pr);
                cache.insert(key.clone(), m);
            }
            let m = &cache[&key];
            // Singer B's melody notes move by a whole octave after
            // composing (design 4.5): applied here, and to every use of
            // this line's pitches below, so `form.lines[*].pitches` and
            // the cached reference and lead notes all agree.
            let octave_shift = if singer == SingerId::B { 12 * o } else { 0 };
            let shifted: Vec<i32> = m.pitches.iter().map(|&p| p + octave_shift).collect();
            composed[li_idx] = Some((m.rh.clone(), shifted.clone()));

            let occ = &mut first_occ[kind as usize];
            if occ.len() <= li {
                occ.resize(li + 1, None);
            }
            if sec.occ == 0 {
                occ[li] = Some(shifted.clone());
            }

            let n = m.pitches.len();
            let mut gr = Rng::event(seed, GRACE, line.start_bar as u64);
            for i in 0..n {
                let dur = m.rh.durs[i];
                let midi = m.pitches[i];
                let syl = &line.syls[i];
                let mut grace = None;
                if syl.is_continuation() {
                    // A grace note never lands on a continuation.
                } else if i + 1 == n && dur >= 1.5 && gr.uniform() < GRACE_END_P {
                    grace = (1..=3)
                        .map(|d| midi + d)
                        .find(|&g| m.scales[i].contains(Pc::new(g)));
                } else if i > 0
                    && dur >= 1.0
                    && syl.stress
                    && gr.uniform() < GRACE_FALL_P
                    && midi < m.pitches[i - 1]
                {
                    grace = Some(m.pitches[i - 1]);
                }
                lead.push(LeadNote {
                    beat: line_beat + m.rh.onsets[i],
                    dur,
                    midi: midi + octave_shift,
                    syl: syl.clone(),
                    line_idx: li_idx,
                    i,
                    stress: syl.stress,
                    phrase_start: i == 0,
                    phrase_end: i + 1 == n,
                    grace: grace.map(|g| g + octave_shift),
                    lift: sec.is_lift(),
                    singer,
                    t0: 0.0,
                    t1: 0.0,
                });
            }
            if n > 0 {
                prev_end = Some(m.pitches[n - 1]);
            }
        }
    }
    for (l, c) in form.lines.iter_mut().zip(composed) {
        if let Some((rh, p)) = c {
            l.rh = Some(rh);
            l.pitches = Some(p);
        }
    }

    let inst = compose_instrumental(form, tl, seed, t);
    Comp {
        lead,
        inst,
        second: Vec::new(),
        t,
        tonic,
        repairs,
    }
}

/// Instrumental lead lines for sections without lyrics: one line per two
/// written bars, 4-6 notes (times 1.5 when stretched), echoing the chorus
/// lines; the last chunk closes open, or on the tonic in an outro. A section
/// with a written tune (`Sec::break_tune`) plays it instead (`written`).
fn compose_instrumental(form: &Form, tl: &Timeline, seed: u64, t: i32) -> Vec<InstNote> {
    let bpb = form.bpb();
    let chorus = form
        .sections
        .iter()
        .position(|s| s.kind == SectionKind::Chorus && s.is_sung())
        .or_else(|| form.sections.iter().position(|s| s.is_sung()));
    let cb = 2 * form.stretch.max(1) as usize;
    let mut inst = Vec::new();
    for (si, s) in form.sections.iter().enumerate() {
        if s.is_sung() {
            continue;
        }
        if let Some(bt) = &s.break_tune {
            inst.extend(crate::written::compose(form, si, bt));
            continue;
        }
        if s.n_bars < cb {
            continue;
        }
        let h = Harmony {
            form,
            tl,
            tonic: s.key.0,
            mode: s.key.1,
            register: t,
        };
        let chunks = s.n_bars / cb;
        for k in 0..chunks {
            let mut rr = Rng::event(seed, INST_RHYTHM, event_key(si, k, 0));
            let mut pr = Rng::event(seed, INST_PITCH, event_key(si, k, 0));
            let base = 4 + rr.below(3) as usize;
            let n = if form.stretch == 2 {
                base * 3 / 2
            } else {
                base
            };
            let stresses: Vec<bool> = (0..n).map(|i| i % 2 == 0 || i + 1 == n).collect();
            let reference = chorus.and_then(|ci| {
                let cl = &form.sections[ci].lines;
                (!cl.is_empty())
                    .then(|| cl[k % cl.len()])
                    .and_then(|j| form.lines[j].pitches.as_deref())
            });
            let cadence = if k + 1 == chunks {
                if s.kind == SectionKind::Outro {
                    Cadence::Tonic
                } else {
                    Cadence::Open
                }
            } else {
                Cadence::None
            };
            let b0 = ((s.start_bar + cb * k) as i32 * bpb) as f64;
            let spec = LineSpec {
                stresses: &stresses,
                n_bars: cb,
                beat0: b0,
                rhythm: RhythmStyle::default(),
                pitch: PitchStyle::default(),
                cadence,
                contour: Contour::for_phrase(k, cadence == Cadence::Tonic, INST_TESS.a, None),
                center: INST_TESS.c,
                reference,
                prev_end: None,
                hook: 0,
                conts: &[],
                hints: &[],
            };
            let m = compose_line(&spec, &h, &mut rr, &mut pr);
            for i in 0..m.pitches.len() {
                inst.push(InstNote {
                    beat: b0 + m.rh.onsets[i],
                    dur: m.rh.durs[i],
                    midi: m.pitches[i],
                    lift: s.is_lift(),
                    written: false,
                });
            }
        }
    }
    inst
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(extra: serde_json::Value) -> Song {
        let mut base = json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,"title":"t",
            "sections":[
                {"type":"intro","chords":["C","G"]},
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":["Am F"]}]},
                {"type":"chorus","same":true}
            ]
        });
        for (k, v) in extra.as_object().unwrap() {
            base.as_object_mut().unwrap().insert(k.clone(), v.clone());
        }
        song::normalize_value(&base).unwrap().0
    }

    #[test]
    fn cadences() {
        assert_eq!(cadence_for(SectionKind::Verse, 2, 3), Cadence::Tonic);
        assert_eq!(cadence_for(SectionKind::Bridge, 2, 3), Cadence::Open);
        assert_eq!(cadence_for(SectionKind::Verse, 1, 4), Cadence::Open);
        assert_eq!(cadence_for(SectionKind::Verse, 0, 4), Cadence::None);
    }

    #[test]
    fn melody_profile_is_deterministic() {
        let s = song(json!({}));
        let p1 = melody_profile(42, &s);
        let p2 = melody_profile(42, &s);
        assert_eq!(p1.pitch, p2.pitch);
        assert_eq!(p1.hook, p2.hook);
        assert!(p1.shape(SectionKind::Intro).is_none());
        assert!(p1.shape(SectionKind::Chorus).is_some());
    }

    #[test]
    fn compose_melody_fills_pitches_and_caches() {
        let s = song(json!({}));
        let mut form = crate::form::build_form(&s, 0);
        let tl = Timeline::new(&form, s.tempo_bpm);
        let comp = compose_melody(&s, &mut form, &tl, 7, Voice::Baritone, None);
        assert!(!comp.lead.is_empty());
        assert!(!comp.inst.is_empty());
        for l in &form.lines {
            assert!(l.pitches.is_some() && l.rh.is_some());
        }
        let chorus: Vec<_> = form
            .lines
            .iter()
            .filter(|l| form.sections[l.sec].kind == SectionKind::Chorus)
            .collect();
        assert_eq!(chorus.len(), 2);
        assert_eq!(chorus[0].pitches, chorus[1].pitches);
    }

    #[test]
    fn line_key_separates_bar_boundaries() {
        // Same kind, line index and text: 'C G' in one bar, then 'C' | 'G'.
        let s = song(json!({"sections":[
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C","G"]}]},
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]}
        ]}));
        let form = crate::form::build_form(&s, 0);
        assert_eq!(form.lines.len(), 3);
        let k: Vec<LineKey> = (0..3)
            .map(|i| line_key(&form, i, SectionKind::Verse))
            .collect();
        assert_eq!(form.lines[0].n_bars, 1);
        assert_eq!(form.lines[1].n_bars, 2);
        assert_ne!(k[0], k[1]);
        assert_eq!(k[0], k[2]);
    }

    /// Continuation notes of a melisma move by at most two scale steps
    /// (four semitones) from the note before, carry no grace note, and the
    /// composed line has one note per expanded syllable.
    #[test]
    fn melisma_continuations_step() {
        let s = crate::form::tests::melisma_song();
        let mut seen = 0;
        for seed in 0..24u64 {
            let mut form = crate::form::build_form(&s, 0);
            let tl = Timeline::new(&form, s.tempo_bpm);
            let comp = compose_melody(&s, &mut form, &tl, seed, Voice::Tenor, None);
            let n_notes: usize = form.lines.iter().map(|l| l.syls.len()).sum();
            assert_eq!(comp.lead.len(), n_notes);
            for w in comp.lead.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                assert!(!b.syl.is_continuation() || b.grace.is_none(), "seed {seed}");
                if b.syl.is_continuation() {
                    seen += 1;
                    assert_eq!(a.line_idx, b.line_idx);
                    let d = (b.midi - a.midi).abs();
                    assert!((1..=4).contains(&d), "seed {seed}: step of {d} semitones");
                }
            }
        }
        assert!(seen > 24 * 5);
    }

    /// A song of one verse whose line sings `tune` (schema 3).
    fn tune_song(tune: &str) -> Song {
        song(json!({"schema_version":3,"sections":[
            {"type":"verse","lines":[
                {"syl":"*one *two three *four","chords":["C G"],"tune":tune}]}
        ]}))
    }

    fn compose_leads(s: &Song, transpose: i32, seed: u64) -> (Vec<i32>, Vec<song::Repair>) {
        let mut form = crate::form::build_form(s, transpose);
        let tl = Timeline::new(&form, s.tempo_bpm);
        let comp = compose_melody(s, &mut form, &tl, seed, Voice::Tenor, None);
        (comp.lead.iter().map(|n| n.midi).collect(), comp.repairs)
    }

    /// A line whose every note is hinted sounds exactly as hinted, in any
    /// key, for any seed.
    #[test]
    fn a_fully_hinted_line_is_as_hinted() {
        let s = tune_song("s m r d");
        for (transpose, reg) in [(0, 60), (5, 65), (9, 57)] {
            for seed in 0..12u64 {
                let (midi, rep) = compose_leads(&s, transpose, seed);
                assert_eq!(midi, [reg + 7, reg + 4, reg + 2, reg], "seed {seed}");
                assert!(rep.is_empty());
            }
        }
        // Chromatic and off-chord notes stay as written.
        let s = tune_song("d ri fi te");
        let (midi, _) = compose_leads(&s, 0, 3);
        assert_eq!(midi, [60, 63, 66, 70]);
    }

    /// Free notes are composed; hinted notes stay.
    #[test]
    fn a_partly_hinted_line_keeps_its_hints() {
        let s = tune_song("s . . d");
        for seed in 0..12u64 {
            let (midi, _) = compose_leads(&s, 0, seed);
            assert_eq!((midi[0], midi[3]), (67, 60));
        }
    }

    /// A tune outside the voice moves by whole octaves, as a whole, with a
    /// repair.
    #[test]
    fn a_tune_out_of_range_moves_by_octaves() {
        let s = tune_song("d'' m'' s'' d''");
        let (midi, rep) = compose_leads(&s, 0, 1);
        assert_eq!(midi, [60, 64, 67, 60]);
        assert_eq!(
            rep,
            vec![song::Repair::TuneMoved {
                section: 0,
                line: 0,
                octaves: -2
            }]
        );
    }

    /// Without a tune a version-3 song composes as the same song of version 2.
    #[test]
    fn a_song_without_a_tune_composes_as_before() {
        let with = |v: u32| {
            song(json!({"schema_version":v,"sections":[
                {"type":"verse","lines":[{"syl":"*one *two three *four","chords":["C G"]}]}
            ]}))
        };
        for seed in 0..6u64 {
            assert_eq!(
                compose_leads(&with(2), 0, seed),
                compose_leads(&with(3), 0, seed)
            );
        }
    }

    #[test]
    fn line_key_separates_tunes() {
        let a = tune_song("s m r d");
        let b = tune_song("d m r d");
        let (fa, fb) = (
            crate::form::build_form(&a, 0),
            crate::form::build_form(&b, 0),
        );
        assert_ne!(
            line_key(&fa, 0, SectionKind::Verse),
            line_key(&fb, 0, SectionKind::Verse)
        );
    }

    #[test]
    fn line_key_separates_melismas() {
        let s = song(json!({"schema_version":2,"sections":[
            {"type":"verse","lines":[{"syl":"*one *two three *four","chords":["C G"]}]},
            {"type":"verse","lines":[{"syl":"*one~ *two three *four","chords":["C G"]}]}
        ]}));
        let form = crate::form::build_form(&s, 0);
        assert_ne!(
            line_key(&form, 0, SectionKind::Verse),
            line_key(&form, 1, SectionKind::Verse)
        );
    }

    /// A key change moves the chorus to D major (chords written in D) and a
    /// copied verse to E major (chords moved by the section's key).
    fn modulating_song() -> Song {
        song(json!({"schema_version":2,"sections":[
            {"type":"verse","lines":[
                {"syl":"one *two three *four","chords":["C G"]},
                {"syl":"*five *six *seven *eight","chords":["F C"]}]},
            {"type":"chorus","key":"D","lines":[
                {"syl":"*five *six *seven *eight","chords":["D A"]},
                {"syl":"one *two three *four","chords":["G D"]}]},
            {"type":"verse","same":true,"key":"E"}
        ]}))
    }

    #[test]
    fn a_section_in_a_new_key_uses_its_scale() {
        let s = modulating_song();
        assert!(s.modulates());
        let mut sharp_fourth = 0;
        for seed in 0..12u64 {
            let mut form = crate::form::build_form(&s, 0);
            let keys: Vec<(u8, Mode)> = form
                .sections
                .iter()
                .map(|x| (x.key.0.get(), x.key.1))
                .collect();
            assert_eq!(
                keys,
                vec![(0, Mode::Major), (2, Mode::Major), (4, Mode::Major)]
            );
            let tl = Timeline::new(&form, s.tempo_bpm);
            let comp = compose_melody(&s, &mut form, &tl, seed, Voice::Baritone, None);
            for n in &comp.lead {
                let sec = &form.sections[form.lines[n.line_idx].sec];
                let scale = local_scale(sec.key.0, sec.key.1, tl.chord_at(&form, n.beat + 0.01));
                assert!(
                    scale.contains(Pc::new(n.midi)),
                    "seed {seed}: note {} in a {:?} section is off its scale",
                    n.midi,
                    sec.key
                );
                // F# is the chorus's own note: in D major only.
                if sec.key.0.get() == 2 && n.midi.rem_euclid(12) == 6 {
                    sharp_fourth += 1;
                }
            }
        }
        assert!(sharp_fourth > 0, "the D major chorus never sings F#");
    }

    #[test]
    fn a_song_without_key_change_reads_one_key() {
        let s = song(json!({}));
        assert!(!s.modulates());
        let form = crate::form::build_form(&s, 3);
        assert!(form.sections.iter().all(|x| x.key == form.sections[0].key));
        assert_eq!(form.sections[0].key, (Pc::new(3), Mode::Major));
    }

    #[test]
    fn line_key_separates_keys() {
        // The same text and chord symbols in two keys compose apart.
        let s = song(json!({"schema_version":2,"sections":[
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
            {"type":"verse","key":"D","lines":[{"syl":"one *two three *four","chords":["C G"]}]}
        ]}));
        let form = crate::form::build_form(&s, 0);
        assert_ne!(
            line_key(&form, 0, SectionKind::Verse),
            line_key(&form, 1, SectionKind::Verse)
        );
    }

    #[test]
    fn event_keys_are_distinct() {
        assert_ne!(event_key(1, 0, 0), event_key(0, 1, 0));
        assert_ne!(event_key(0, 1, 0), event_key(0, 0, 1));
    }
}

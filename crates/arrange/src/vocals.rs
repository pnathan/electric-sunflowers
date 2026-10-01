//! The singers: lead, harmony, two doubles and the choir, as
//! `song::events::Singer` values. How each is sung is its `SingStyle`; the
//! voice renders it.
//!
//! - Lead: the composed melody at level 1 (unstressed syllables 0.86,
//!   lifted sections 1.08), the song's voice, the preset style, centre.
//! - Harmony: a third to a sixth above the lifted-section melody (below for
//!   a soprano lead), at 0.9, 8 ms late; the voice type (not the lead's)
//!   whose range centre is nearest the line's median; vibrato 0.8, breath
//!   1.2.
//! - Doubles: the melody of the repeated lifted sections at 0.8, twice:
//!   left (-0.6) 13 ms late, tract 0.97, +7 cents, rate 0.94; right (+0.6)
//!   21 ms late, tract 1.03, -6 cents, rate 1.07; vibrato 0.7, breath +0.05,
//!   no breaths between phrases.
//! - Choir: `CHOIR_SINGERS` singers per part (bass, tenor, alto, soprano;
//!   the `choir::voicings` notes) on /aa/ over repeated lifted sections,
//!   bridges and outros. Singer i of part p draws from
//!   `Rng::event(seed, CHOIR_SINGER, p * 16 + i)`: detune +-11 cents,
//!   lateness 12-47 ms, vibrato 0.55-1.0, rate 0.85-1.15, tract 0.95-1.05,
//!   F1 0.97-1.03, breath +0.04-0.12, Rd x 1.1-1.25, then per note an onset
//!   jitter of +-15 ms, a release 10-30 ms early (100-180 ms at a section
//!   end) and a level of 0.88-1.08 (0.8, 0.65 in a bridge). Jitter x 1.6,
//!   shimmer x 1.4, two high resonances, 50 ms voicing smoother, 50 ms
//!   glide, no scoop, no breaths. Pan: -0.5, -0.2, 0.25, 0.55 per part,
//!   singers spread 0.35 apart, within +-0.9.

use std::collections::BTreeMap;

use compose::melody::LeadNote;
use compose::prepare::{harmony_line, vocal_notes, Prepared, VocalNote as ComposedNote};
use compose::theory::local_scale;
use sfcore::random::{tag, Rng, Tag};
use song::events::{SingStyle, Singer, VocalNote};
use song::{ChoirVoicing, Part, Pc, Phoneme, Phrasing, SectionKind, SingerId, Song, Voice};

use crate::choir;

const CHOIR_SINGER: Tag = tag("choir.singer");

/// Singers per choir part.
pub const CHOIR_SINGERS: usize = 3;
/// Choir part voices, low to high (the order of `ChoirVoicing::notes`).
pub const CHOIR_VOICES: [Voice; 4] = [Voice::Bass, Voice::Tenor, Voice::Alto, Voice::Soprano];
/// Choir part pans, low to high.
const CHOIR_PANS: [f64; 4] = [-0.5, -0.2, 0.25, 0.55];
/// Pan distance between singers of one part.
const CHOIR_SPREAD: f64 = 0.35;
/// The choir's vowel.
const CHOIR_VOWEL: Phoneme = Phoneme::Aa;
/// A gap longer than this between choir notes separates phrases, seconds.
const CHOIR_PHRASE_GAP: f64 = 0.1;

/// An interval the harmony can sing against the melody. Each names the
/// semitone distances tried, nearest to the classical size first: a chord
/// tone at one of them wins, then a tone of the chord's local scale, else
/// the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarmonyInterval {
    ThirdAbove,
    ThirdBelow,
    SixthAbove,
    SixthBelow,
    FifthAbove,
    FifthBelow,
}

impl HarmonyInterval {
    pub const ALL: [HarmonyInterval; 6] = [
        HarmonyInterval::ThirdAbove,
        HarmonyInterval::ThirdBelow,
        HarmonyInterval::SixthAbove,
        HarmonyInterval::SixthBelow,
        HarmonyInterval::FifthAbove,
        HarmonyInterval::FifthBelow,
    ];

    /// The name the arranger edits use.
    pub fn name(self) -> &'static str {
        match self {
            HarmonyInterval::ThirdAbove => "third_above",
            HarmonyInterval::ThirdBelow => "third_below",
            HarmonyInterval::SixthAbove => "sixth_above",
            HarmonyInterval::SixthBelow => "sixth_below",
            HarmonyInterval::FifthAbove => "fifth_above",
            HarmonyInterval::FifthBelow => "fifth_below",
        }
    }

    pub fn parse(s: &str) -> Option<HarmonyInterval> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        HarmonyInterval::ALL.into_iter().find(|i| i.name() == s)
    }

    fn up(self) -> bool {
        matches!(
            self,
            HarmonyInterval::ThirdAbove | HarmonyInterval::SixthAbove | HarmonyInterval::FifthAbove
        )
    }

    fn distances(self) -> [i32; 2] {
        match self {
            HarmonyInterval::ThirdAbove | HarmonyInterval::ThirdBelow => [4, 3],
            HarmonyInterval::SixthAbove | HarmonyInterval::SixthBelow => [9, 8],
            HarmonyInterval::FifthAbove | HarmonyInterval::FifthBelow => [7, 6],
        }
    }
}

/// `lead` with every note moved by `interval` to a chord tone of its chord
/// (else a tone of the chord's local scale, else the first distance), grace
/// notes dropped: the counterpart of `compose::prepare::harmony_line` for an
/// explicit interval.
pub fn harmony_interval(
    lead: &[LeadNote],
    form: &compose::form::Form,
    tl: &compose::timeline::Timeline,
    interval: HarmonyInterval,
) -> Vec<LeadNote> {
    lead.iter()
        .map(|n| {
            let ch = tl.chord_at(form, n.beat + 0.01);
            let (tonic, mode) = form.sections[form.lines[n.line_idx].sec].key;
            let sc = local_scale(tonic, mode, ch);
            let sign = if interval.up() { 1 } else { -1 };
            let cand = interval.distances().map(|d| n.midi + sign * d);
            let pick = cand
                .iter()
                .find(|&&m| ch.tones.contains(Pc::new(m)))
                .or_else(|| cand.iter().find(|&&m| sc.contains(Pc::new(m))))
                .unwrap_or(&cand[0]);
            let mut nn = n.clone();
            nn.midi = *pick;
            nn.grace = None;
            nn
        })
        .collect()
}

/// What the choir sings in a section (an arranger override).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoirMode {
    /// Nothing.
    Off,
    /// The /aa/ pad over the chords.
    Pad,
    /// The section's words, in this voicing.
    Words(ChoirVoicing),
}

/// The harmony in a section (an arranger override).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionHarmony {
    pub on: bool,
    /// `None`: the rule-based interval (`harmony_line`).
    pub interval: Option<HarmonyInterval>,
}

/// Section-level changes to the vocals, keyed by an index of
/// `Form::sections`. A section not listed keeps the rule-based choice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overrides {
    pub harmony: BTreeMap<usize, SectionHarmony>,
    pub doubles: BTreeMap<usize, bool>,
    pub choir: BTreeMap<usize, ChoirMode>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        self.harmony.is_empty() && self.doubles.is_empty() && self.choir.is_empty()
    }

    /// The choir words of the overrides, for `choir::line_notes_with`.
    pub fn choir_words(&self) -> BTreeMap<usize, Option<ChoirVoicing>> {
        self.choir
            .iter()
            .map(|(&s, m)| match m {
                ChoirMode::Words(k) => (s, Some(*k)),
                ChoirMode::Off | ChoirMode::Pad => (s, None),
            })
            .collect()
    }
}

/// Every singer of a song.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Vocals {
    pub lead: Singer,
    /// Singer B of a duet; `None` in a solo song.
    pub lead_b: Option<Singer>,
    /// Notes may be empty (no lifted section).
    pub harmony: Singer,
    /// Left and right, singer A's two takes first, then singer B's (a
    /// duet only): 2 entries solo, 4 in a duet.
    pub doubles: Vec<Singer>,
    /// Per part, low to high (bass, tenor, alto, soprano); a singer with no
    /// notes is left out.
    pub choir: [Vec<Singer>; 4],
}

fn event(n: &ComposedNote, offset: f64) -> VocalNote {
    VocalNote {
        t0: n.t0 + offset,
        t1: n.t1 + offset,
        midi: n.midi as f32,
        phones: n.ph.clone(),
        amp: n.amp as f32,
        stress: n.stress,
        phrase_start: n.phrase_start,
        phrase_end: n.phrase_end,
        grace: n.grace.map(|g| g as f32),
        legato: n.cont,
    }
}

fn notes(lead: &[LeadNote], amp: f64) -> Vec<VocalNote> {
    vocal_notes(lead, amp)
        .iter()
        .map(|n| event(n, 0.0))
        .collect()
}

/// The voice type not in `exclude` whose range centre is nearest `median`
/// (first of baritone, tenor, alto, soprano on a tie). In a duet `exclude`
/// holds both singers' voices, so the harmony's voice type differs from
/// either.
fn harmony_voice(exclude: &[Voice], median: i32) -> Voice {
    let mut best: Option<Voice> = None;
    let mut best_d = f64::INFINITY;
    for v in [Voice::Baritone, Voice::Tenor, Voice::Alto, Voice::Soprano] {
        if exclude.contains(&v) {
            continue;
        }
        let d = (v.range().centre() - median as f64).abs();
        if d < best_d {
            best_d = d;
            best = Some(v);
        }
    }
    best.unwrap_or(Voice::Tenor)
}

/// The notes belonging to singer `id` (design 4.6): its melody notes from
/// `lead` plus, on a shared line where the other singer carries the
/// melody, its `Comp.second` notes for that line; sorted by onset. Lines
/// never overlap, so a stable sort by `t0` keeps every singer's notes in
/// order (and is a no-op in a solo song, whose one singer's notes are
/// already in order).
fn singer_notes(
    id: SingerId,
    lead: &[LeadNote],
    second: &[LeadNote],
    form: &compose::form::Form,
) -> Vec<LeadNote> {
    let mut v: Vec<LeadNote> = lead
        .iter()
        .filter(|n| n.singer == id && !form.lines[n.line_idx].part.is_choir())
        .cloned()
        .chain(second.iter().filter(|n| n.singer == id).cloned())
        .collect();
    v.sort_by(|a, b| a.t0.partial_cmp(&b.t0).expect("finite t0"));
    v
}

/// Two doubled takes of `notes` (already the melody at doubling level) in
/// `voice` with `phrasing`: left (-0.6) 13 ms late, tract 0.97, +7 cents,
/// rate 0.94; right (+0.6) 21 ms late, tract 1.03, -6 cents, rate 1.07.
fn double_takes(voice: Voice, phrasing: Phrasing, notes: &[VocalNote]) -> [Singer; 2] {
    let make = |pan: f32, offset: f64, formant: f32, cents: f32, rate: f32| Singer {
        voice,
        style: SingStyle {
            detune_cents: cents,
            vibrato_scale: 0.7,
            vibrato_rate_scale: rate,
            formant_scale: formant,
            breath_add: 0.05,
            breath_pauses: false,
            phrasing,
            ..SingStyle::LEAD
        },
        notes: notes.to_vec(),
        pan,
        offset,
    };
    [
        make(-0.6, 0.013, 0.97, 7.0, 0.94),
        make(0.6, 0.021, 1.03, -6.0, 1.07),
    ]
}

/// Every singer of the song. `overrides` `None` is the rule-based plan.
pub fn plan(song: &Song, prepared: &Prepared, seed: u64, overrides: Option<&Overrides>) -> Vocals {
    let p = prepared;
    let no = Overrides::default();
    let ov = overrides.unwrap_or(&no);
    let form = &p.form;
    let lead_notes = &p.comp.lead;
    let second = &p.comp.second;
    let sec_of = |n: &LeadNote| &form.sections[form.lines[n.line_idx].sec];

    let phrasing = song.phrasing_of(SingerId::A);
    let notes_a = singer_notes(SingerId::A, lead_notes, second, form);
    let lead_style = SingStyle {
        phrasing,
        ..SingStyle::LEAD
    };
    let lead = Singer {
        voice: p.voice,
        style: lead_style,
        notes: notes(&notes_a, 1.0),
        pan: 0.0,
        offset: 0.0,
    };

    let lead_b = p.voice_b.map(|vb| {
        let phrasing_b = song.phrasing_of(SingerId::B);
        let notes_b = singer_notes(SingerId::B, lead_notes, second, form);
        Singer {
            voice: vb,
            style: SingStyle {
                phrasing: phrasing_b,
                ..SingStyle::LEAD
            },
            notes: notes(&notes_b, 1.0),
            pan: 0.0,
            offset: 0.0,
        }
    });

    // Harmony: only solo lines (never a shared `Both` line), so a duet's
    // harmony never doubles up on a line the two leads already share.
    // An override turns a section's harmony on or off (the rule is: lifted
    // sections) and may name its interval.
    let harmony_here = |n: &LeadNote| match ov.harmony.get(&form.lines[n.line_idx].sec) {
        Some(h) => h.on,
        None => n.lift,
    };
    let interval_of = |n: &LeadNote| {
        ov.harmony
            .get(&form.lines[n.line_idx].sec)
            .and_then(|h| h.interval)
    };
    let lifted_solo: Vec<&LeadNote> = lead_notes
        .iter()
        .filter(|n| harmony_here(n) && matches!(form.lines[n.line_idx].part, Part::Solo(_)))
        .collect();
    let mut hl: Vec<LeadNote> = Vec::new();
    let harmonise = |singer: SingerId, voice: Voice, hl: &mut Vec<LeadNote>| {
        let mine: Vec<LeadNote> = lifted_solo
            .iter()
            .filter(|n| n.singer == singer)
            .map(|&n| n.clone())
            .collect();
        // Notes with a named interval, then the rule-based rest.
        let (named, rest): (Vec<LeadNote>, Vec<LeadNote>) =
            mine.into_iter().partition(|n| interval_of(n).is_some());
        if !rest.is_empty() {
            hl.extend(harmony_line(
                &rest,
                form,
                &p.timeline,
                voice != Voice::Soprano,
            ));
        }
        for n in named {
            let iv = interval_of(&n).expect("partitioned on it");
            hl.extend(harmony_interval(&[n], form, &p.timeline, iv));
        }
    };
    harmonise(SingerId::A, p.voice, &mut hl);
    if let Some(vb) = p.voice_b {
        harmonise(SingerId::B, vb, &mut hl);
    }
    hl.sort_by(|a, b| a.t0.partial_cmp(&b.t0).expect("finite t0"));
    let median = {
        let mut ms: Vec<i32> = hl.iter().map(|n| n.midi).collect();
        ms.sort_unstable();
        ms.get(ms.len() / 2).copied().unwrap_or(60)
    };
    let mut exclude = vec![p.voice];
    if let Some(vb) = p.voice_b {
        exclude.push(vb);
    }
    let harmony = Singer {
        voice: harmony_voice(&exclude, median),
        style: SingStyle {
            vibrato_scale: 0.8,
            breath_scale: 1.2,
            phrasing,
            ..SingStyle::LEAD
        },
        notes: notes(&hl, 0.9),
        pan: 0.0,
        offset: 0.008,
    };

    // Doubles: each singer's own melody notes (not a shared line's second
    // voice) in repeated lifted sections; A's two takes first, then B's.
    let doubled = |n: &LeadNote| match ov.doubles.get(&form.lines[n.line_idx].sec) {
        Some(on) => *on,
        None => n.lift && sec_of(n).is_repeat_lift(),
    };
    let repeated_a: Vec<LeadNote> = lead_notes
        .iter()
        .filter(|n| {
            n.singer == SingerId::A && doubled(n) && !form.lines[n.line_idx].part.is_choir()
        })
        .cloned()
        .collect();
    let mut doubles: Vec<Singer> = double_takes(p.voice, phrasing, &notes(&repeated_a, 0.8)).into();
    if let Some(vb) = p.voice_b {
        let phrasing_b = song.phrasing_of(SingerId::B);
        let repeated_b: Vec<LeadNote> = lead_notes
            .iter()
            .filter(|n| {
                n.singer == SingerId::B && doubled(n) && !form.lines[n.line_idx].part.is_choir()
            })
            .cloned()
            .collect();
        doubles.extend(double_takes(vb, phrasing_b, &notes(&repeated_b, 0.8)));
    }

    Vocals {
        lead,
        lead_b,
        harmony,
        doubles,
        choir: choir_singers(p, seed, overrides),
    }
}

/// The choir singers (see the module docs). `overrides` `None` is the
/// rule-based choir; with overrides a section's mode replaces the rule
/// (`Off` and `Words` silence its /aa/ pad, `Pad` sings it even where the
/// rules would not, `Words` sings the section's lead words).
pub fn choir_singers(p: &Prepared, seed: u64, overrides: Option<&Overrides>) -> [Vec<Singer>; 4] {
    let form = &p.form;
    let tl = &p.timeline;
    let no = Overrides::default();
    let ov = overrides.unwrap_or(&no);
    let vs = choir::voicings_clear(form, tl, |s| {
        let idx = form.sections.iter().position(|x| std::ptr::eq(x, s));
        match idx.and_then(|i| ov.choir.get(&i)) {
            Some(ChoirMode::Pad) => true,
            Some(_) => false,
            None => choir::sings_here(s),
        }
    });
    let words = choir::line_notes_with(p, &ov.choir_words());
    let mut parts: [Vec<Singer>; 4] = Default::default();
    if vs.is_empty() && words.is_empty() {
        return parts;
    }
    for (part, singers) in parts.iter_mut().enumerate() {
        for i in 0..CHOIR_SINGERS {
            let mut r = Rng::event(seed, CHOIR_SINGER, (part * 16 + i) as u64);
            let style = SingStyle {
                detune_cents: (11.0 * r.bipolar()) as f32,
                lateness: r.range(0.012, 0.047) as f32,
                vibrato_scale: r.range(0.55, 1.0) as f32,
                vibrato_rate_scale: r.range(0.85, 1.15) as f32,
                formant_scale: r.range(0.95, 1.05) as f32,
                f1_scale: r.range(0.97, 1.03) as f32,
                breath_scale: 1.0,
                breath_add: r.range(0.04, 0.12) as f32,
                rd_scale: r.range(1.1, 1.25) as f32,
                jitter_scale: 1.6,
                shimmer_scale: 1.4,
                n_high: 2,
                av_tau: 0.05,
                glide: 0.05,
                scoop: false,
                breath_pauses: false,
                phrasing: Phrasing::default(),
            };
            let mut notes: Vec<VocalNote> = Vec::with_capacity(vs.len());
            for cv in &vs {
                let sg = &tl.segs[cv.seg];
                let sec = &form.sections[sg.sec];
                let t0 = tl.to_time(sg.b0) + 0.015 * r.bipolar();
                let sec_end = sg.b1 == sec.beats(&form.meter).end;
                let early = if sec_end {
                    r.range(0.1, 0.18)
                } else {
                    r.range(0.01, 0.03)
                };
                let t1 = tl.to_time(sg.b1) - early;
                let level = if sec.kind == SectionKind::Bridge {
                    0.65
                } else {
                    0.8
                };
                let amp = level * r.range(0.88, 1.08);
                // Phrase breaks use the sung onset, lateness included.
                let phrase_start = notes
                    .last()
                    .is_none_or(|pn| t0 + style.lateness as f64 - pn.t1 > CHOIR_PHRASE_GAP);
                notes.push(VocalNote {
                    t0,
                    t1,
                    midi: cv.notes[part] as f32,
                    phones: vec![CHOIR_VOWEL],
                    amp: amp as f32,
                    stress: false,
                    phrase_start,
                    phrase_end: false,
                    grace: None,
                    legato: false,
                });
            }
            if !words.is_empty() {
                // The choir lines' words, after the pad's draws (so a song
                // without choir lines draws exactly as before). Onsets
                // jitter +-8 ms; level 0.9 x 0.88-1.08.
                let tune: Vec<LeadNote> = words
                    .iter()
                    .map(|w| LeadNote {
                        midi: w.midi[part],
                        grace: None,
                        ..w.note.clone()
                    })
                    .collect();
                let mut tagged: Vec<(VocalNote, bool)> =
                    notes.drain(..).map(|n| (n, false)).collect();
                for (n, w) in vocal_notes(&tune, 0.9).iter().zip(&words) {
                    let mut vn = event(n, 0.0);
                    let j = 0.008 * r.bipolar();
                    vn.t0 = (vn.t0 + j).min(vn.t1 - 0.05).max(w.note.t0 - 0.008);
                    vn.amp *= r.range(0.88, 1.08) as f32;
                    tagged.push((vn, w.note.phrase_start));
                }
                tagged.sort_by(|a, b| a.0.t0.partial_cmp(&b.0.t0).expect("finite t0"));
                let mut prev_t1 = f64::NEG_INFINITY;
                for (mut vn, line_start) in tagged {
                    vn.phrase_start =
                        line_start || vn.t0 + style.lateness as f64 - prev_t1 > CHOIR_PHRASE_GAP;
                    prev_t1 = vn.t1;
                    notes.push(vn);
                }
            }
            let n = notes.len();
            for k in 0..n {
                notes[k].phrase_end = k + 1 == n
                    || notes[k + 1].t0 + style.lateness as f64 - notes[k].t1 > CHOIR_PHRASE_GAP;
            }
            let pan = (CHOIR_PANS[part]
                + (i as f64 - (CHOIR_SINGERS as f64 - 1.0) / 2.0) * CHOIR_SPREAD)
                .clamp(-0.9, 0.9);
            singers.push(Singer {
                voice: CHOIR_VOICES[part],
                style,
                notes,
                pan: pan as f32,
                offset: 0.0,
            });
        }
    }
    parts
}

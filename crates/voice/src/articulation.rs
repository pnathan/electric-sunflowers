//! Articulation: sung notes to a timed plan of typed segments, by synthesis
//! by rule with targets and transitions (Holmes, Mattingly and Shearme
//! 1964; Klatt 1987). `crate::controls` turns the plan into frame tracks.
//!
//! Rules, in the order `plan_segments` applies them:
//!
//! - Syllable split: onset consonants, nucleus (first through last vowel),
//!   coda consonants. A note with no vowel sings /aa/, the choir vowel, and
//!   all its phonemes are coda.
//! - Durations: `cons_dur` times the voice's consonant scale (bass 1.15,
//!   baritone 1.2, tenor 1.25, alto 1.35, soprano 1.4).
//! - Flapping: a single /t/ or /d/ onset of an unstressed, non-phrase-initial
//!   note, after a note with no coda and less than 150 ms of gap, becomes
//!   the flap /dx/ (20 ms closure, not scaled).
//! - Onset compression: onset consonants start before the note and take at
//!   most 45% of the inter-onset interval (300 ms for the first note).
//! - Coda: ends at the note end, or when the next onset starts less than 30
//!   ms after it, at that onset (not before t0 + 60 ms); takes at most 40%
//!   of the note.
//! - CV transition: after a stop or affricate onset, F1-F3 move from the
//!   locus to the vowel over min(50 ms, 40% of the vowel) in 5 steps, eased
//!   1 - (1 - a)^1.6, with voicing rising from 0.7 to 1 (locus theory,
//!   Delattre, Liberman and Cooper 1955).
//! - Diphthongs and sonorant nuclei: the first target holds, the later
//!   targets share a tail of 30% of the vowel, clamped to 50-180 ms.
//! - Stops: closure (voicing 0.1 when voiced), a 12 ms burst at 0.7 onset
//!   or 0.4 coda times `BURST_GAIN` with 0.12 aspiration, and for voiceless
//!   onsets aspiration 0.4 with F1 damping (b1x 320 Hz) to the end of the
//!   consonant. Closure and aspiration scale with the consonant; the burst
//!   does not. Burst and closure formants sit 60% of the way to the locus.
//! - Affricates: closure, an 8 ms burst at 0.6, then frication (voicing 0.3
//!   when voiced); all three scale.
//! - Sonorants and nasals in onset or coda: their targets pulled 25% toward
//!   the vowel.
//! - Pauses: silence carrying the next nucleus's formants; before a phrase
//!   after more than 400 ms, a 240 ms breath ending 40 ms before the onset.

use std::ops::Range;

use sfcore::math::clamp;
use song::events::VocalNote;
use song::Phoneme;

use crate::params::VoiceParams;
use crate::phoneme::{consonant, vowel_formants, ConsClass, Consonant, Locus};
use crate::phrasing::PhrasingParams;
use crate::synth::VoiceSettings;
use crate::tuning::BURST_GAIN;

/// F1-F3 in Hz.
pub type Formants3 = [f64; 3];

/// A time span in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub t0: f64,
    pub t1: f64,
}

impl Span {
    pub fn len(&self) -> f64 {
        self.t1 - self.t0
    }
}

/// One articulation target. Every kind carries its F1-F3 (voice-scaled),
/// since the tract keeps filtering through noise and silence. Levels are
/// absolute (note amplitude folded in).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Voiced vowel (also each step of a CV transition).
    Vowel { f: Formants3, av: f64 },
    /// Liquid or glide.
    Sonorant { f: Formants3, av: f64 },
    /// Nasal murmur: voicing with widened bandwidths.
    Nasal { f: Formants3, av: f64 },
    /// Frication noise at centre `ff`, bandwidth `fbw`, over optional voicing.
    Fricative { f: Formants3, av: f64, af: f64, ff: f64, fbw: f64 },
    /// Aspiration noise through the tract; `b1x` Hz of extra F1 bandwidth.
    Aspiration { f: Formants3, ah: f64, b1x: f64 },
    /// Stop or affricate release: frication band plus aspiration.
    Burst { f: Formants3, af: f64, ff: f64, fbw: f64, ah: f64 },
    /// Oral closure; `av` is the voicing that leaks through (voiced stops).
    Closure { f: Formants3, av: f64 },
    /// Audible in-breath before a phrase.
    Breath { f: Formants3 },
    /// Nothing sounds; the formants move toward the next nucleus.
    Silence { f: Formants3 },
}

impl Segment {
    pub fn formants(&self) -> Formants3 {
        match *self {
            Segment::Vowel { f, .. }
            | Segment::Sonorant { f, .. }
            | Segment::Nasal { f, .. }
            | Segment::Fricative { f, .. }
            | Segment::Aspiration { f, .. }
            | Segment::Burst { f, .. }
            | Segment::Closure { f, .. }
            | Segment::Breath { f }
            | Segment::Silence { f } => f,
        }
    }
}

/// Default nucleus of a note without a vowel: /aa/, the choir vowel.
pub const DEFAULT_NUCLEUS: Phoneme = Phoneme::Aa;
/// Onset room of the first note, s.
pub const FIRST_ONSET: f64 = 0.3;
/// Share of the note the coda may take.
pub const CODA_SHARE: f64 = 0.4;
/// Longest CV transition, s (locus to vowel).
pub const CV_TRANSITION: f64 = 0.05;
/// Steps of a CV transition.
const CV_STEPS: usize = 5;
/// Stop burst length, s (not scaled per voice).
pub const STOP_BURST: f64 = 0.012;
/// Aspiration after a voiceless onset stop, s (before voice scaling).
pub const STOP_ASPIRATION: f64 = 0.024;
/// Affricate burst length, s.
pub const AFFRICATE_BURST: f64 = 0.008;
/// Coda nasal length, s.
pub const CODA_NASAL: f64 = 0.085;
/// Longest gap after which a /t d/ onset still flaps, s.
const FLAP_GAP: f64 = 0.15;
/// Breath: starts this long before the next onset and ends `BREATH_END`
/// before it, s; only after a pause longer than `BREATH_PAUSE`.
const BREATH_START: f64 = 0.28;
const BREATH_END: f64 = 0.04;
const BREATH_PAUSE: f64 = 0.4;
/// Tract formants of the breath (an open, neutral tract), Hz.
const BREATH_FORMANTS: Formants3 = [620.0, 1200.0, 2400.0];
/// Segments shorter than this are dropped, s.
const MIN_SEGMENT: f64 = 1e-4;

/// Kind of a nucleus target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NucKind {
    Vowel,
    Sonorant,
    Nasal,
}

/// One formant target of a nucleus (unscaled).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NucTarget {
    pub f: Formants3,
    pub kind: NucKind,
    /// Relative voicing: 1 for vowels, the table value for sonorants and
    /// nasals.
    pub av: f64,
}

impl NucTarget {
    fn segment(&self, f: Formants3, amp: f64) -> Segment {
        let av = amp * self.av;
        match self.kind {
            NucKind::Vowel => Segment::Vowel { f, av },
            NucKind::Sonorant => Segment::Sonorant { f, av },
            NucKind::Nasal => Segment::Nasal { f, av },
        }
    }
}

const DEFAULT_TARGET: NucTarget = NucTarget {
    f: match vowel_formants(DEFAULT_NUCLEUS) {
        Some(f) => f,
        None => [730.0, 1090.0, 2440.0],
    },
    kind: NucKind::Vowel,
    av: 1.0,
};

/// The formant targets of a nucleus: a diphthong gives its two vowel
/// targets, a sonorant or nasal inside the nucleus its own; other
/// consonants none. Never empty: the fallback is /aa/.
pub fn nuc_targets(nu: &[Phoneme]) -> Vec<NucTarget> {
    let mut out = Vec::with_capacity(nu.len().max(1) + 1);
    for &p in nu {
        if let Some(pair) = p.diphthong_targets() {
            out.extend(pair.iter().filter_map(|&v| vowel_formants(v)).map(|f| NucTarget {
                f,
                kind: NucKind::Vowel,
                av: 1.0,
            }));
        } else if let Some(f) = vowel_formants(p) {
            out.push(NucTarget { f, kind: NucKind::Vowel, av: 1.0 });
        } else if let Some(c) = consonant(p) {
            let kind = match c.class {
                ConsClass::Sonorant => NucKind::Sonorant,
                ConsClass::Nasal => NucKind::Nasal,
                _ => continue,
            };
            out.push(NucTarget { f: c.formants, kind, av: c.av });
        }
    }
    if out.is_empty() {
        out.push(DEFAULT_TARGET);
    }
    out
}

/// A consonant's nominal duration in seconds, before voice scaling: stops
/// are closure plus the 12 ms burst plus 24 ms aspiration when voiceless in
/// an onset; affricates closure plus 8 ms burst plus frication; a coda
/// nasal 85 ms; others their table duration. Vowels have none.
pub fn cons_dur(p: Phoneme, coda: bool) -> f64 {
    let Some(c) = consonant(p) else {
        return 0.0;
    };
    match c.class {
        ConsClass::Stop => c.closure + STOP_BURST + if coda || c.voiced { 0.0 } else { STOP_ASPIRATION },
        ConsClass::Affricate => c.closure + AFFRICATE_BURST + c.fric_dur,
        ConsClass::Nasal if coda => CODA_NASAL,
        _ => c.dur,
    }
}

/// One note's syllable after splitting, flapping and onset timing.
#[derive(Clone, Debug)]
pub struct Syllable {
    pub onset: Vec<Phoneme>,
    pub nucleus: Vec<Phoneme>,
    pub coda: Vec<Phoneme>,
    /// Onset consonant durations, s (voice-scaled, before compression).
    pub onset_dur: Vec<f64>,
    /// Coda consonant durations, s (voice-scaled, before compression).
    pub coda_dur: Vec<f64>,
    /// Onset compression factor, <= 1.
    pub onset_scale: f64,
    /// Time the first onset consonant starts, s (<= `vowel_start`).
    pub onset_start: f64,
    /// Time the nucleus starts, s (the note's `t0` with `lead_in` 1.0;
    /// later with a smaller `lead_in`, which pushes part of the onset
    /// consonants' scaled span past the written onset).
    pub vowel_start: f64,
    /// Nucleus formant targets (unscaled), never empty.
    pub targets: Vec<NucTarget>,
}

/// Split `ph` into onset consonants, nucleus (first vowel through last
/// vowel, with anything between) and coda consonants. With no vowel the
/// nucleus is /aa/ and every phoneme is coda.
pub fn split_ph(ph: &[Phoneme]) -> (Vec<Phoneme>, Vec<Phoneme>, Vec<Phoneme>) {
    let first = ph.iter().position(|p| p.is_vowel());
    let last = ph.iter().rposition(|p| p.is_vowel());
    match (first, last) {
        (Some(i0), Some(i1)) => (ph[..i0].to_vec(), ph[i0..=i1].to_vec(), ph[i1 + 1..].to_vec()),
        _ => (Vec::new(), vec![DEFAULT_NUCLEUS], ph.to_vec()),
    }
}

/// Per-note syllables of `notes` for voice `p`: split, durations, flapping,
/// onset compression against `ph.onset_share`, and the onset/vowel span
/// placed by `ph.lead_in`.
pub fn syllables(notes: &[VocalNote], p: &VoiceParams, ph: &PhrasingParams) -> Vec<Syllable> {
    let cs = p.cons_scale;
    let mut syl: Vec<Syllable> = notes
        .iter()
        .map(|n| {
            let (onset, nucleus, coda) = split_ph(&n.phones);
            let onset_dur = onset.iter().map(|&ph| cons_dur(ph, false) * cs).collect();
            let coda_dur = coda.iter().map(|&ph| cons_dur(ph, true) * cs).collect();
            let targets = nuc_targets(&nucleus);
            Syllable { onset, nucleus, coda, onset_dur, coda_dur, onset_scale: 1.0, onset_start: n.t0, vowel_start: n.t0, targets }
        })
        .collect();

    for k in 1..notes.len() {
        let (n, prev) = (&notes[k], &notes[k - 1]);
        let single_td = matches!(syl[k].onset.as_slice(), [Phoneme::T | Phoneme::D]);
        if single_td && !n.stress && !n.phrase_start && syl[k - 1].coda.is_empty() && n.t0 - prev.t1 < FLAP_GAP {
            syl[k].onset = vec![Phoneme::Dx];
            syl[k].onset_dur = vec![cons_dur(Phoneme::Dx, false)];
        }
    }

    for k in 0..notes.len() {
        let n = &notes[k];
        let d: f64 = syl[k].onset_dur.iter().sum();
        let avail = if k > 0 { (n.t0 - notes[k - 1].t0) * ph.onset_share } else { FIRST_ONSET };
        let s = if d > avail && d > 0.0 { avail / d } else { 1.0 };
        syl[k].onset_scale = s;
        // Onset consonants of scaled length `dd` span [t0 - lead_in dd,
        // t0 + (1 - lead_in) dd]; the vowel starts at the span end.
        let dd = d * s;
        syl[k].onset_start = n.t0 - ph.lead_in * dd;
        syl[k].vowel_start = n.t0 + (1.0 - ph.lead_in) * dd;
        // The vowel may not be pushed past half the note: on a short note
        // (or the first note's long onset) it would start after the note
        // ends. The span keeps length `dd`. Inactive at lead_in 1.0.
        let shift = syl[k].vowel_start - n.t0;
        let cap = 0.5 * (n.t1 - n.t0).max(0.0);
        if shift > cap {
            syl[k].vowel_start = n.t0 + cap;
            syl[k].onset_start = syl[k].vowel_start - dd;
        }
    }
    syl
}

/// Voice scaling of formants: F1 by `f1s`, F2 and F3 by `fs`.
#[derive(Clone, Copy)]
struct Scale {
    f1s: f64,
    fs: f64,
}

impl Scale {
    fn of(&self, f: Formants3) -> Formants3 {
        [f[0] * self.f1s, f[1] * self.fs, f[2] * self.fs]
    }

    /// Where the CV transition of stop or affricate `c` starts, before
    /// vowel `vf` (scaled). Velar: F1 250 Hz, F2 1.1 vowel F2 up to 2300
    /// Hz, F3 the vowel's.
    fn locus(&self, c: &Consonant, vf: Formants3) -> Formants3 {
        match c.locus {
            Locus::At(loc) => self.of(loc),
            Locus::Velar => [250.0 * self.f1s, 2300.0f64.min(vf[1] * 1.1), vf[2]],
        }
    }
}

/// Segment list under construction; drops spans shorter than 0.1 ms.
struct Plan<'a> {
    segs: &'a mut Vec<(Span, Segment)>,
}

impl Plan<'_> {
    fn put(&mut self, t0: f64, t1: f64, seg: Segment) {
        if t1 > t0 + MIN_SEGMENT {
            self.segs.push((Span { t0, t1 }, seg));
        }
    }
}

/// The timed segments of `notes` sung by voice `p` (settings already
/// applied), sorted by start time. See the module doc for the rules.
pub fn plan_segments(notes: &[VocalNote], p: &VoiceParams, settings: &VoiceSettings) -> Vec<(Span, Segment)> {
    let mut out = Vec::with_capacity(notes.len() * 8);
    plan_syllables(notes, &syllables(notes, p, &settings.phrasing), 0..notes.len(), p, settings, &mut out);
    out
}

/// The segments of notes `range` (the pause after the last one reaches
/// the next note's onset, which may lie outside `range`) into `out`,
/// cleared first. `syl` from `syllables` for all of `notes`.
pub fn plan_syllables(
    notes: &[VocalNote],
    syl: &[Syllable],
    range: Range<usize>,
    p: &VoiceParams,
    settings: &VoiceSettings,
    out: &mut Vec<(Span, Segment)>,
) {
    out.clear();
    let sc = Scale { f1s: p.f1s, fs: p.fs };
    let mut plan = Plan { segs: out };
    let range = range.start.min(notes.len())..range.end.min(notes.len()).min(syl.len());

    for k in range {
        let (n, s) = (&notes[k], &syl[k]);
        let amp = n.amp as f64;
        let vt = &s.targets;
        let vf0 = sc.of(vt[0].f);
        let vfl = sc.of(vt[vt.len() - 1].f);
        let next = notes.get(k + 1).zip(syl.get(k + 1));

        // Onset consonants.
        let mut t = s.onset_start;
        for (&ph, &d) in s.onset.iter().zip(&s.onset_dur) {
            let d = d * s.onset_scale;
            consonant_segments(&mut plan, &sc, ph, t, t + d, vf0, amp, false);
            t += d;
        }
        let locus = s
            .onset
            .last()
            .and_then(|&ph| consonant(ph))
            .filter(|c| matches!(c.class, ConsClass::Stop | ConsClass::Affricate))
            .map(|c| sc.locus(c, vf0));

        // Coda timing.
        let mut coda_end = n.t1;
        if let Some((_, ns)) = next {
            if ns.onset_start < n.t1 + 0.03 {
                coda_end = ns.onset_start.min((s.vowel_start + 0.06).max(n.t1));
            }
        }
        let d2: f64 = s.coda_dur.iter().sum();
        let lim = (coda_end - s.vowel_start) * CODA_SHARE;
        let s2 = if d2 > lim && d2 > 0.0 { lim / d2 } else { 1.0 };
        let coda_start = coda_end - d2 * s2;

        // CV transition from the locus.
        let vlen = coda_start - s.vowel_start;
        let mut n_start = s.vowel_start;
        if let (Some(loc), NucKind::Vowel) = (locus, vt[0].kind) {
            let tt = CV_TRANSITION.min(vlen * 0.4);
            for j in 0..CV_STEPS {
                let a = (j as f64 + 0.5) / CV_STEPS as f64;
                let e = 1.0 - (1.0 - a).powf(1.6);
                let f = [0, 1, 2].map(|q| loc[q] + (vf0[q] - loc[q]) * e);
                let t0 = s.vowel_start + tt * j as f64 / CV_STEPS as f64;
                let t1 = s.vowel_start + tt * (j as f64 + 1.0) / CV_STEPS as f64;
                plan.put(t0, t1, Segment::Vowel { f, av: amp * (0.7 + 0.3 * a) });
            }
            n_start = s.vowel_start + tt;
        }

        // Nucleus: one target, or a hold and a glide through the rest.
        if vt.len() == 1 {
            plan.put(n_start, coda_start, vt[0].segment(sc.of(vt[0].f), amp));
        } else {
            let tail = clamp(vlen * 0.3, 0.05, 0.18);
            let each = tail / (vt.len() - 1) as f64;
            plan.put(n_start, coda_start - tail, Segment::Vowel { f: sc.of(vt[0].f), av: amp });
            for (j, v) in vt.iter().enumerate().skip(1) {
                let a = coda_start - tail + (j - 1) as f64 * each;
                plan.put(a, a + each, v.segment(sc.of(v.f), amp));
            }
        }

        // Coda consonants.
        t = coda_start;
        for (&ph, &d) in s.coda.iter().zip(&s.coda_dur) {
            let d = d * s2;
            consonant_segments(&mut plan, &sc, ph, t, t + d, vfl, amp, true);
            t += d;
        }

        // Pause until the next onset: silence, or a breath before a phrase.
        let next_on = next.map_or(coda_end + 0.5, |(_, ns)| ns.onset_start);
        if next_on > coda_end + 0.01 {
            let nf = next.map_or(vfl, |(_, ns)| sc.of(ns.targets[0].f));
            let phrase = next.is_some_and(|(nn, _)| nn.phrase_start);
            if phrase && next_on - coda_end > BREATH_PAUSE && settings.breath_pauses {
                plan.put(coda_end, next_on - BREATH_START, Segment::Silence { f: nf });
                plan.put(next_on - BREATH_START, next_on - BREATH_END, Segment::Breath { f: sc.of(BREATH_FORMANTS) });
                plan.put(next_on - BREATH_END, next_on, Segment::Silence { f: nf });
            } else {
                plan.put(coda_end, next_on, Segment::Silence { f: nf });
            }
        }
    }

    // Stable: equal starts keep emission order.
    plan.segs.sort_by(|a, b| a.0.t0.total_cmp(&b.0.t0));
}

/// Segments of consonant `ph` over t0..t1 next to vowel `vf` (scaled).
#[allow(clippy::too_many_arguments)]
fn consonant_segments(
    plan: &mut Plan,
    sc: &Scale,
    ph: Phoneme,
    t0: f64,
    t1: f64,
    vf: Formants3,
    amp: f64,
    coda: bool,
) {
    let Some(c) = consonant(ph) else {
        return;
    };
    match c.class {
        ConsClass::Sonorant | ConsClass::Nasal => {
            let t = sc.of(c.formants);
            let f = [0, 1, 2].map(|i| t[i] * 0.75 + vf[i] * 0.25);
            let av = amp * c.av;
            let seg = if c.class == ConsClass::Nasal { Segment::Nasal { f, av } } else { Segment::Sonorant { f, av } };
            plan.put(t0, t1, seg);
        }
        ConsClass::Fricative => {
            let av = if c.voiced { amp * c.vv } else { 0.0 };
            plan.put(t0, t1, Segment::Fricative { f: vf, av, af: c.af * amp, ff: c.ff, fbw: c.bw });
        }
        ConsClass::Aspirate => plan.put(t0, t1, Segment::Aspiration { f: vf, ah: 0.5 * amp, b1x: 0.0 }),
        ConsClass::Stop | ConsClass::Affricate => {
            let loc = sc.locus(c, vf);
            let cf = [0, 1, 2].map(|i| loc[i] * 0.6 + vf[i] * 0.4);
            // Closure and (affricate) burst keep their nominal share of the
            // consonant after voice scaling and compression.
            let k = (t1 - t0) / cons_dur(ph, coda);
            let cl = t0 + c.closure * k;
            let av = if c.voiced { amp * 0.1 } else { 0.0 };
            plan.put(t0, cl, Segment::Closure { f: cf, av });
            if c.class == ConsClass::Stop {
                // Velar bursts follow the vowel: high for front vowels, low
                // for back.
                let ff = match c.locus {
                    Locus::At(_) => c.ff,
                    Locus::Velar if vf[1] > 1500.0 => 3000.0,
                    Locus::Velar => 1800.0,
                };
                let af = if coda { 0.4 } else { 0.7 } * amp * BURST_GAIN;
                plan.put(cl, cl + STOP_BURST, Segment::Burst { f: cf, af, ff, fbw: c.bw, ah: 0.12 * amp });
                if !coda && !c.voiced {
                    plan.put(cl + STOP_BURST, t1, Segment::Aspiration { f: vf, ah: 0.4 * amp, b1x: 320.0 });
                }
            } else {
                let b1 = cl + AFFRICATE_BURST * k;
                plan.put(cl, b1, Segment::Burst { f: cf, af: 0.6 * amp, ff: c.ff, fbw: c.bw, ah: 0.0 });
                let av = if c.voiced { amp * 0.3 } else { 0.0 };
                plan.put(b1, t1, Segment::Fricative { f: vf, av, af: c.af * amp, ff: c.ff, fbw: c.bw });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::voice_params;
    use song::Voice;

    fn s(v: &str) -> Phoneme {
        Phoneme::from_symbol(v).expect("known symbol")
    }
    fn ph(v: &[&str]) -> Vec<Phoneme> {
        v.iter().map(|x| s(x)).collect()
    }
    fn note(t0: f64, t1: f64, phones: &[&str], stress: bool) -> VocalNote {
        VocalNote {
            t0,
            t1,
            midi: 52.0,
            phones: ph(phones),
            amp: 1.0,
            stress,
            phrase_start: false,
            phrase_end: false,
            grace: None,
        }
    }

    #[test]
    fn split_basic_cvc() {
        let (on, nu, co) = split_ph(&ph(&["k", "ae", "t"]));
        assert_eq!((on, nu, co), (ph(&["k"]), ph(&["ae"]), ph(&["t"])));
    }

    #[test]
    fn split_without_vowel_sings_aa() {
        let (on, nu, co) = split_ph(&ph(&["s", "t"]));
        assert!(on.is_empty());
        assert_eq!(nu, vec![Phoneme::Aa]);
        assert_eq!(co, ph(&["s", "t"]));
        let (_, nu, _) = split_ph(&[]);
        assert_eq!(nu, vec![Phoneme::Aa]);
    }

    #[test]
    fn split_vowel_run_and_cluster() {
        let (on, nu, co) = split_ph(&ph(&["s", "t", "aa", "ih", "n"]));
        assert_eq!((on, nu, co), (ph(&["s", "t"]), ph(&["aa", "ih"]), ph(&["n"])));
    }

    #[test]
    fn cons_dur_rules() {
        assert!((cons_dur(Phoneme::T, false) - (0.045 + 0.012 + 0.024)).abs() < 1e-12);
        assert!((cons_dur(Phoneme::T, true) - (0.045 + 0.012)).abs() < 1e-12);
        assert!((cons_dur(Phoneme::D, false) - (0.05 + 0.012)).abs() < 1e-12);
        assert!((cons_dur(Phoneme::Ch, false) - (0.04 + 0.008 + 0.07)).abs() < 1e-12);
        assert_eq!(cons_dur(Phoneme::N, true), 0.085);
        assert_ne!(cons_dur(Phoneme::N, false), 0.085);
        assert_eq!(cons_dur(Phoneme::Aa, false), 0.0);
    }

    #[test]
    fn nucleus_targets() {
        let t = nuc_targets(&ph(&["ae"]));
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].kind, NucKind::Vowel);
        let t = nuc_targets(&ph(&["ay"]));
        assert_eq!(t.len(), 2);
        assert!(t.iter().all(|x| x.kind == NucKind::Vowel));
        let t = nuc_targets(&ph(&["ey"]));
        assert_eq!(t.iter().map(|x| x.f).collect::<Vec<_>>(), vec![[450.0, 2020.0, 2600.0], [340.0, 2210.0, 2780.0]]);
        let t = nuc_targets(&[]);
        assert_eq!(Some(t[0].f), vowel_formants(Phoneme::Aa));
    }

    /// Spans of the segments inside [t0, t1).
    fn inside(plan: &[(Span, Segment)], t0: f64, t1: f64) -> Vec<(Span, Segment)> {
        plan.iter().filter(|(sp, _)| sp.t0 >= t0 - 1e-9 && sp.t0 < t1).cloned().collect()
    }

    /// "the RIV-er ci-ty": the unstressed /t/ of "ty" after an open "ci"
    /// flaps to a 20 ms voiced closure, unscaled, with a burst and no
    /// aspiration.
    #[test]
    fn unstressed_intervocalic_t_flaps() {
        let settings = VoiceSettings::default();
        for voice in [Voice::Bass, Voice::Baritone, Voice::Soprano] {
            let p = voice_params(voice);
            let notes = [
                note(0.5, 0.9, &["dh", "ax"], false),
                note(1.0, 1.4, &["r", "ih"], true),
                note(1.5, 1.9, &["v", "er"], false),
                note(2.0, 2.4, &["s", "ih"], true),
                note(2.5, 2.9, &["t", "iy"], false),
            ];
            let syl = syllables(&notes, &p, &PhrasingParams::default());
            assert_eq!(syl[4].onset, vec![Phoneme::Dx], "{voice:?}");
            let plan = plan_segments(&notes, &p, &settings);
            let on = inside(&plan, syl[4].onset_start, notes[4].t0);
            let clos: Vec<_> = on.iter().filter(|(_, g)| matches!(g, Segment::Closure { .. })).collect();
            assert_eq!(clos.len(), 1);
            assert!((clos[0].0.len() - 0.02).abs() < 1e-9, "{voice:?} closure {}", clos[0].0.len());
            assert!(matches!(clos[0].1, Segment::Closure { av, .. } if av > 0.0), "flap closure is voiced");
            assert!(on.iter().any(|(_, g)| matches!(g, Segment::Burst { .. })));
            assert!(!on.iter().any(|(_, g)| matches!(g, Segment::Aspiration { .. })));

            // Stressed, the same /t/ is a full voiceless stop.
            let mut stressed = notes.clone();
            stressed[4].stress = true;
            let syl = syllables(&stressed, &p, &PhrasingParams::default());
            assert_eq!(syl[4].onset, vec![Phoneme::T]);
            assert_eq!(syl[4].onset_scale, 1.0);
            let plan = plan_segments(&stressed, &p, &settings);
            let on = inside(&plan, syl[4].onset_start, stressed[4].t0);
            let cs = p.cons_scale;
            let len = |pred: fn(&Segment) -> bool| -> f64 { on.iter().filter(|(_, g)| pred(g)).map(|(sp, _)| sp.len()).sum() };
            let clos = len(|g| matches!(g, Segment::Closure { av, .. } if *av == 0.0));
            let burst = len(|g| matches!(g, Segment::Burst { .. }));
            let asp = len(|g| matches!(g, Segment::Aspiration { b1x, .. } if *b1x > 0.0));
            assert!((clos - 0.045 * cs).abs() < 1e-9, "{voice:?} closure {clos}");
            assert!((burst - STOP_BURST).abs() < 1e-9, "{voice:?} burst {burst}");
            let want = (0.045 + STOP_BURST + STOP_ASPIRATION) * cs - 0.045 * cs - STOP_BURST;
            assert!((asp - want).abs() < 1e-9, "{voice:?} aspiration {asp} want {want}");
            // The CV transition follows the release.
            let cv = inside(&plan, stressed[4].t0, stressed[4].t0 + CV_TRANSITION);
            assert_eq!(cv.iter().filter(|(_, g)| matches!(g, Segment::Vowel { .. })).count(), CV_STEPS);
        }
    }

    /// A /t/ after a coda consonant, or across a long gap, does not flap.
    #[test]
    fn t_after_coda_or_gap_does_not_flap() {
        let p = voice_params(Voice::Baritone);
        let notes = [note(0.5, 0.9, &["s", "ih", "n"], true), note(1.0, 1.4, &["t", "iy"], false)];
        assert_eq!(syllables(&notes, &p, &PhrasingParams::default())[1].onset, vec![Phoneme::T]);
        let notes = [note(0.5, 0.9, &["s", "ih"], true), note(1.2, 1.6, &["t", "iy"], false)];
        assert_eq!(syllables(&notes, &p, &PhrasingParams::default())[1].onset, vec![Phoneme::T]);
    }

    /// Onsets take at most 45% of the inter-onset interval.
    #[test]
    fn onset_compression() {
        let p = voice_params(Voice::Soprano);
        let notes = [note(0.5, 0.6, &["aa"], true), note(0.62, 0.8, &["s", "t", "r", "aa"], true)];
        let syl = syllables(&notes, &p, &PhrasingParams::default());
        let used = notes[1].t0 - syl[1].onset_start;
        assert!(syl[1].onset_scale < 1.0);
        assert!((used - 0.12 * PhrasingParams::default().onset_share).abs() < 1e-12, "{used}");
    }

    /// `lead_in` moves the vowel start by `(1 - lead_in) * D`, where `D` is
    /// the onset consonants' scaled span (uncompressed here); `onset_start`
    /// moves the other way, by `lead_in * D`, so the span keeps length `D`.
    #[test]
    fn lead_in_moves_the_vowel_start() {
        let p = voice_params(Voice::Tenor);
        let notes = [note(1.0, 1.6, &["s", "t", "aa"], true), note(2.0, 2.6, &["b", "aa"], true)];
        let default = PhrasingParams::default();
        let syl0 = syllables(&notes, &p, &default);
        assert_eq!(syl0[0].vowel_start, notes[0].t0);
        let d: f64 = syl0[0].onset_dur.iter().sum::<f64>() * syl0[0].onset_scale;

        for lead_in in [0.6, 0.0, 1.0] {
            let ph = PhrasingParams { lead_in, ..default };
            let syl = syllables(&notes, &p, &ph);
            let dd: f64 = syl[0].onset_dur.iter().sum::<f64>() * syl[0].onset_scale;
            assert!((dd - d).abs() < 1e-9, "onset compression must not depend on lead_in");
            assert!((syl[0].vowel_start - (notes[0].t0 + (1.0 - lead_in) * dd)).abs() < 1e-9, "lead_in {lead_in}");
            assert!((syl[0].onset_start - (notes[0].t0 - lead_in * dd)).abs() < 1e-9, "lead_in {lead_in}");
        }
    }

    /// A breath precedes a phrase after a long pause, only when enabled.
    #[test]
    fn breath_before_phrase() {
        let p = voice_params(Voice::Baritone);
        let mut b = note(3.0, 3.5, &["aa"], true);
        b.phrase_start = true;
        let notes = [note(0.5, 1.0, &["aa"], true), b];
        let on = plan_segments(&notes, &p, &VoiceSettings::default());
        assert!(on.iter().any(|(sp, g)| matches!(g, Segment::Breath { .. }) && (sp.t1 - (3.0 - BREATH_END)).abs() < 1e-9));
        let off = plan_segments(&notes, &p, &VoiceSettings { breath_pauses: false, ..VoiceSettings::default() });
        assert!(!off.iter().any(|(_, g)| matches!(g, Segment::Breath { .. })));
    }

    /// Segments are sorted, finite and non-empty for odd input.
    #[test]
    fn plan_is_sorted_and_finite() {
        let p = voice_params(Voice::Alto);
        let notes = [
            note(0.0, 0.05, &["s", "t", "r", "ng", "k", "s"], false),
            note(0.05, 0.06, &[], false),
            note(0.06, 2.0, &["ch", "ay", "l", "d", "z"], true),
            note(2.0, 2.0, &["jh", "ey", "m"], false),
        ];
        let plan = plan_segments(&notes, &p, &VoiceSettings::default());
        assert!(!plan.is_empty());
        assert!(plan.windows(2).all(|w| w[0].0.t0 <= w[1].0.t0));
        for (sp, g) in &plan {
            assert!(sp.t0.is_finite() && sp.t1 > sp.t0);
            assert!(g.formants().iter().all(|f| f.is_finite() && *f > 0.0));
        }
    }
}

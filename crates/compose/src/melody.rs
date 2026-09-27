//! Melody: tessitura/contour tables, per-song melody profile, cadence and
//! shape functions, and the melody/instrumental-lead composer.
//! Ports `TESS`, `CONTOURS`, `melodyProfile`, `shapeFor`, `cadenceFor` and
//! `composeMelody` (engine.js lines ~315-386).

use std::collections::HashMap;
use std::rc::Rc;

use sfcore::js::{pow, round, sin};
use sfcore::rng::{rng_for, Rng};

use crate::form::Form;
use crate::pitch::{pitch_line, PitchOpts, PitchProf};
use crate::rhythm::{place_rhythm, PrOpts, RhythmResult};
use crate::song::Song;
use crate::theory::{local_scale, Chord};
use crate::timeline::Timeline;

const PI: f64 = std::f64::consts::PI;

/// One `TESS`/`prof.tess` entry: melodic center and amplitude for a section type.
#[derive(Clone, Copy, Debug)]
pub struct SectionTess {
    pub c: f64,
    pub a: f64,
}

/// `TESS`/the shape of `prof.tess`: center/amplitude per section type.
#[derive(Clone, Copy, Debug)]
pub struct TessSet {
    pub verse: SectionTess,
    pub prechorus: SectionTess,
    pub chorus: SectionTess,
    pub bridge: SectionTess,
    pub inst: SectionTess,
}

impl TessSet {
    /// `prof.tess[sec.type]||prof.tess.verse` (verse is the fallback).
    pub fn get(&self, sec_type: &str) -> SectionTess {
        match sec_type {
            "prechorus" => self.prechorus,
            "chorus" => self.chorus,
            "bridge" => self.bridge,
            "inst" => self.inst,
            _ => self.verse,
        }
    }
}

/// `TESS`: the fixed global tessitura table (used directly for instrumental
/// lead lines; `melodyProfile` builds the per-song `prof.tess` separately).
pub fn tess() -> TessSet {
    TessSet {
        verse: SectionTess { c: 2.0, a: 3.5 },
        prechorus: SectionTess { c: 4.0, a: 3.0 },
        chorus: SectionTess { c: 6.0, a: 4.0 },
        bridge: SectionTess { c: 5.0, a: 3.5 },
        inst: SectionTess { c: 7.0, a: 3.5 },
    }
}

/// `CONTOURS`: the seven named melodic contour shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourKind {
    Arch,
    Descent,
    Rise,
    Valley,
    Wave,
    PeakLate,
    PeakEarly,
}

impl ContourKind {
    /// `Object.keys(CONTOURS)`: all string keys, so plain insertion order.
    pub const KEYS: [ContourKind; 7] = [
        ContourKind::Arch,
        ContourKind::Descent,
        ContourKind::Rise,
        ContourKind::Valley,
        ContourKind::Wave,
        ContourKind::PeakLate,
        ContourKind::PeakEarly,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ContourKind::Arch => "arch",
            ContourKind::Descent => "descent",
            ContourKind::Rise => "rise",
            ContourKind::Valley => "valley",
            ContourKind::Wave => "wave",
            ContourKind::PeakLate => "peakLate",
            ContourKind::PeakEarly => "peakEarly",
        }
    }

    /// CONTOURS[name](x,a)
    pub fn apply(self, x: f64, a: f64) -> f64 {
        match self {
            ContourKind::Arch => a * sin(PI * x),
            ContourKind::Descent => a * (0.9 - 1.8 * x),
            ContourKind::Rise => a * (-0.7 + 1.5 * x),
            ContourKind::Valley => -a * 0.8 * sin(PI * x) + a * 0.3,
            ContourKind::Wave => a * 0.8 * sin(2.0 * PI * x),
            ContourKind::PeakLate => a * sin(PI * pow(x, 0.55)),
            ContourKind::PeakEarly => a * sin(PI * pow(x, 1.8)),
        }
    }
}

/// A pair of contour choices, one per `li%2` parity (`prof.shape[type]`).
#[derive(Clone, Copy, Debug)]
pub struct ShapePair(pub ContourKind, pub ContourKind);

impl ShapePair {
    /// `prof.shape[type][li%2]`
    pub fn for_li(&self, li: usize) -> ContourKind {
        if li % 2 == 1 {
            self.1
        } else {
            self.0
        }
    }
}

/// `prof.shape`: contour choice pair per section type.
#[derive(Clone, Copy, Debug)]
pub struct ShapeSet {
    pub verse: ShapePair,
    pub prechorus: ShapePair,
    pub chorus: ShapePair,
    pub bridge: ShapePair,
    pub inst: ShapePair,
}

impl ShapeSet {
    pub fn get(&self, sec_type: &str) -> Option<ShapePair> {
        match sec_type {
            "verse" => Some(self.verse),
            "prechorus" => Some(self.prechorus),
            "chorus" => Some(self.chorus),
            "bridge" => Some(self.bridge),
            "inst" => Some(self.inst),
            _ => None,
        }
    }
}

/// `melodyProfile`'s return value (`p` in JS).
#[derive(Clone, Copy, Debug)]
pub struct MelodyProfile {
    pub leap: f64,
    pub rep: f64,
    pub noise: f64,
    pub tess: TessSet,
    pub shape: ShapeSet,
    pub hook: i32,
    pub rh: PrOpts,
}

/// melodyProfile(seed,song): draws, in JS source order, are vc, lift, then
/// leap/rep/noise, then tess.{verse.a,prechorus.a,chorus.a,bridge.c,bridge.a},
/// then shape.{verse x2, prechorus x1, chorus x2, bridge x2, inst x2}, then
/// hook, then rh.{dot,even,sync-cond,sync-extra?,rnoise}.
pub fn melody_profile(seed: u32, song: &Song) -> MelodyProfile {
    let mut r = rng_for(seed, &format!("profile|{}", song.title));
    let keys = ContourKind::KEYS;
    let pick = |r: &mut Rng, opts: &[ContourKind]| -> ContourKind {
        opts[(r.next() * opts.len() as f64).floor() as usize]
    };

    let vc = round(r.next() * 4.0) as i32 - 1;
    let lift = 2 + round(r.next() * 5.0) as i32;

    let leap = r.next() * 0.85;
    let rep = r.next() * 0.8;
    let noise = 0.9 + r.next() * 1.3;

    let tess = TessSet {
        verse: SectionTess { c: vc as f64, a: 2.0 + r.next() * 3.0 },
        prechorus: SectionTess {
            c: (vc + round(lift as f64 / 2.0) as i32) as f64,
            a: 2.0 + r.next() * 2.5,
        },
        chorus: SectionTess { c: (vc + lift) as f64, a: 2.5 + r.next() * 3.0 },
        bridge: SectionTess {
            c: (vc + 1 + round(r.next() * 5.0) as i32) as f64,
            a: 2.0 + r.next() * 3.0,
        },
        inst: SectionTess { c: (vc + lift + 1) as f64, a: 3.0 },
    };

    let shape = ShapeSet {
        verse: ShapePair(pick(&mut r, &keys), pick(&mut r, &keys)),
        prechorus: ShapePair(
            pick(&mut r, &[ContourKind::Rise, ContourKind::PeakLate, ContourKind::Arch]),
            ContourKind::Rise,
        ),
        chorus: ShapePair(pick(&mut r, &keys), pick(&mut r, &keys)),
        bridge: ShapePair(pick(&mut r, &keys), pick(&mut r, &keys)),
        inst: ShapePair(pick(&mut r, &keys), pick(&mut r, &keys)),
    };

    let hook_opts = [5i32, 7, -5, 9, 12, 3, -7, 0];
    let hook = hook_opts[(r.next() * hook_opts.len() as f64).floor() as usize];

    let dot = r.next();
    let even = r.next();
    let sync_cond = r.next();
    let sync = if sync_cond < 0.35 { 0.3 + r.next() * 0.5 } else { 0.0 };
    let rnoise = 0.5 + r.next() * 0.9;

    MelodyProfile {
        leap,
        rep,
        noise,
        tess,
        shape,
        hook,
        rh: PrOpts { dot, even, sync, rnoise },
    }
}

/// shapeFor(li,nl,cad,a,prof,type): the per-section-contour shape function.
/// `prof` and `sec_type` select the contour from `melodyProfile`'s `shape`
/// table; the instrumental-lead call site passes no profile, which falls
/// back to the fixed arch/descent shape below (matches JS when prof/type
/// are absent).
pub fn shape_for(
    li: usize,
    _nl: usize,
    cad: &str,
    a: f64,
    prof: Option<&MelodyProfile>,
    sec_type: &str,
) -> Box<dyn Fn(f64) -> f64> {
    if cad == "tonic" {
        let base = prof.and_then(|p| p.shape.get(sec_type)).map(|sp| sp.for_li(li));
        return Box::new(move |x| {
            (match base {
                Some(k) => k.apply(x, a) * 0.6,
                None => a * 0.8 * sin(PI * x * 0.7),
            }) - a * 1.2 * x
        });
    }
    if let (Some(p), true) = (prof, prof.and_then(|p| p.shape.get(sec_type)).is_some()) {
        let k = p.shape.get(sec_type).unwrap().for_li(li);
        return if li % 2 == 1 {
            Box::new(move |x| k.apply(x, a) + 0.8 * x)
        } else {
            Box::new(move |x| k.apply(x, a))
        };
    }
    if li % 2 == 1 {
        return Box::new(move |x| a * sin(PI * x) + 0.8 * x);
    }
    Box::new(move |x| a * sin(PI * x))
}

/// cadenceFor(type,li,nl)
pub fn cadence_for(sec_type: &str, li: usize, nl: usize) -> &'static str {
    if li == nl - 1 {
        if sec_type == "bridge" || sec_type == "prechorus" {
            "open"
        } else {
            "tonic"
        }
    } else if li % 2 == 1 {
        "open"
    } else {
        "none"
    }
}

/// One lead-vocal note (`lead.push({...})` in JS). `t0`/`t1` are filled in by
/// `prepare` and are 0.0 here.
#[derive(Clone, Debug)]
pub struct LeadNote {
    pub beat: f64,
    pub dur: f64,
    pub midi: i32,
    pub syl: crate::song::Syllable,
    /// index into `Form::lines`
    pub line_idx: usize,
    /// syllable index within the line
    pub i: usize,
    pub stress: bool,
    pub phrase_start: bool,
    pub phrase_end: bool,
    pub grace: Option<i32>,
    /// `n.sec.lift`, copied at push time (JS holds a reference to `sec`).
    pub lift: bool,
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
}

/// composeMelody's return value.
#[derive(Clone, Debug)]
pub struct Comp {
    pub lead: Vec<LeadNote>,
    pub inst: Vec<InstNote>,
    /// `T` in JS.
    pub t: i32,
    pub tonic: i32,
}

struct CacheEntry {
    rh: RhythmResult,
    pitches: Vec<i32>,
    scales: Vec<Vec<i32>>,
}

/// composeMelody(song,form,tl,seed). Mutates `form.lines[*].pitches`/`.rh`,
/// as JS mutates `L.pitches`/`L.rh` in place.
pub fn compose_melody(song: &Song, form: &mut Form, tl: &Timeline, seed: u32) -> Comp {
    let bpb = form.mi.bpb;
    let tonic = (song.key_pc + form.transpose + 120).rem_euclid(12);
    let mode = song.mode.clone();
    let t = 60 + tonic - if tonic > 6 { 12 } else { 0 };
    let prof = melody_profile(seed, song);

    let mut cache: HashMap<String, Rc<CacheEntry>> = HashMap::new();
    let mut first_occ: HashMap<String, Vec<Option<Vec<i32>>>> = HashMap::new();
    let mut lead: Vec<LeadNote> = Vec::new();
    let mut prev_end: Option<i32> = None;

    let n_lines = form.lines.len();
    for li_idx in 0..n_lines {
        let sec_idx = form.lines[li_idx].sec;
        let li = form.lines[li_idx].li;
        let n_bars = form.lines[li_idx].n_bars;
        let start_bar = form.lines[li_idx].start_bar;
        let text = form.lines[li_idx].text.clone();
        let syls = form.lines[li_idx].syls.clone();
        let sec_type = form.sections[sec_idx].type_.clone();
        let nl = form.sections[sec_idx].lines.len();
        let sec_occ = form.sections[sec_idx].occ;
        let sec_lift = form.sections[sec_idx].lift;
        let sec_lift_idx = form.sections[sec_idx].lift_idx;
        let ck = format!("{}|{}|{}", sec_type, li, text);
        let line_beat = start_bar as f64 * bpb as f64;

        let entry: Rc<CacheEntry> = match cache.get(&ck) {
            Some(e) => e.clone(),
            None => {
                let stresses: Vec<bool> = syls.iter().map(|s| s.stress).collect();
                let mut rr = rng_for(seed, &format!("r|{}|{}", sec_type, li));
                let rh = place_rhythm(&stresses, n_bars, &form.mi, &mut rr, prof.rh);
                let chords: Vec<Chord> = rh
                    .onsets
                    .iter()
                    .map(|&o| tl.chord_at(form, line_beat + o + 0.01).clone())
                    .collect();
                let scales: Vec<Vec<i32>> = chords.iter().map(|c| local_scale(tonic, &mode, c)).collect();
                let cad = cadence_for(&sec_type, li, nl);
                let ts = prof.tess.get(&sec_type);

                // JS parity: `if(sec.occ>0&&firstOcc[t]&&firstOcc[t][li])ref=firstOcc[t][li];
                // else if(li>=2&&sec.lines[li-2].pitches)ref=sec.lines[li-2].pitches;` -- the
                // else-if is reached whenever the first branch's condition is false, which
                // includes an occ>0 section whose line li has no first-occurrence entry yet
                // (a later verse with more lines than the first). Do not nest the li>=2 check
                // inside the occ==0 case.
                let mut reference: Option<Vec<i32>> = None;
                let mut have_first_occ = false;
                if sec_occ > 0 {
                    if let Some(v) = first_occ.get(&sec_type) {
                        if let Some(Some(p)) = v.get(li) {
                            reference = Some(p.clone());
                            have_first_occ = true;
                        }
                    }
                }
                if !have_first_occ && li >= 2 {
                    let prev_line_idx = form.sections[sec_idx].lines[li - 2];
                    if let Some(p) = &form.lines[prev_line_idx].pitches {
                        reference = Some(p.clone());
                    }
                }

                let shape = shape_for(li, nl, cad, ts.a, Some(&prof), &sec_type);
                let hook = if sec_lift && sec_lift_idx == 0 && li == 0 && prof.hook != 0 {
                    prof.hook
                } else {
                    0
                };
                let chord_pcs: Vec<Vec<i32>> = chords.iter().map(|c| c.pcs.clone()).collect();
                let mut prng = rng_for(seed, &format!("p|{}|{}|{}", sec_type, li, sec_occ));
                let mut opts = PitchOpts {
                    n: syls.len(),
                    onsets: &rh.onsets,
                    durs: &rh.durs,
                    weights: Some(&rh.weights),
                    chord_pcs: &chord_pcs,
                    scales: &scales,
                    t,
                    tonic,
                    center: t as f64 + ts.c,
                    shape: &*shape,
                    cadence: cad,
                    reference: reference.as_deref(),
                    rng: &mut prng,
                    prev_end,
                    line_beats: rh.line_beats,
                    prof: Some(PitchProf { leap: prof.leap, rep: prof.rep, noise: prof.noise }),
                    hook,
                };
                let pitches = pitch_line(&mut opts);
                let e = Rc::new(CacheEntry { rh, pitches, scales });
                cache.insert(ck.clone(), e.clone());
                e
            }
        };

        form.lines[li_idx].pitches = Some(entry.pitches.clone());
        form.lines[li_idx].rh = Some(entry.rh.clone());

        {
            let occ_vec = first_occ.entry(sec_type.clone()).or_default();
            if occ_vec.len() <= li {
                occ_vec.resize(li + 1, None);
            }
            if sec_occ == 0 {
                occ_vec[li] = Some(entry.pitches.clone());
            }
        }

        let n = syls.len();
        let mut gr = rng_for(seed, &format!("g|{}", start_bar));
        for i in 0..n {
            let beat = line_beat + entry.rh.onsets[i];
            let dur = entry.rh.durs[i];
            let midi = entry.pitches[i];
            let mut grace: Option<i32> = None;
            if i == n - 1 && dur >= 1.5 && gr.next() < 0.55 {
                let sc = &entry.scales[i];
                for d in 1..=3 {
                    if sc.contains(&(midi + d).rem_euclid(12)) {
                        grace = Some(midi + d);
                        break;
                    }
                }
            } else if i > 0 && dur >= 1.0 && syls[i].stress && gr.next() < 0.18 && midi < entry.pitches[i - 1] {
                grace = Some(entry.pitches[i - 1]);
            }
            lead.push(LeadNote {
                beat,
                dur,
                midi,
                syl: syls[i].clone(),
                line_idx: li_idx,
                i,
                stress: syls[i].stress,
                phrase_start: i == 0,
                phrase_end: i == n - 1,
                grace,
                lift: sec_lift,
                t0: 0.0,
                t1: 0.0,
            });
        }
        prev_end = Some(entry.pitches[n - 1]);
    }

    // instrumental lead lines for intro / interlude / outro
    let inst_chorus_idx = form
        .sections
        .iter()
        .position(|s| s.type_ == "chorus" && !s.lines.is_empty())
        .or_else(|| form.sections.iter().position(|s| !s.lines.is_empty()));
    let tess_c = tess();
    let mut inst: Vec<InstNote> = Vec::new();
    for sec_idx2 in 0..form.sections.len() {
        let (lines_len, n_bars, start_bar, idx, sec_type, lift) = {
            let s = &form.sections[sec_idx2];
            (s.lines.len(), s.n_bars, s.start_bar, s.idx, s.type_.clone(), s.lift)
        };
        let cb = 2 * form.stretch;
        if lines_len != 0 || (n_bars as i32) < cb {
            continue;
        }
        let chunks = n_bars / cb as usize;
        for k in 0..chunks {
            let mut rr = rng_for(seed, &format!("ir|{}|{}", idx, k));
            let n = (sfcore::js::to_i32(
                (4.0 + (rr.next() * 3.0).floor()) * (if form.stretch == 2 { 1.5 } else { 1.0 }),
            )) as usize;
            let stresses: Vec<bool> = (0..n).map(|i| i % 2 == 0 || i == n - 1).collect();
            let rh = place_rhythm(&stresses, cb as usize, &form.mi, &mut rr, PrOpts::default());
            let b0 = (start_bar + cb as usize * k) as f64 * bpb as f64;
            let chords: Vec<Chord> = rh
                .onsets
                .iter()
                .map(|&o| tl.chord_at(form, b0 + o + 0.01).clone())
                .collect();
            let scales: Vec<Vec<i32>> = chords.iter().map(|c| local_scale(tonic, &mode, c)).collect();
            let reference: Option<Vec<i32>> = inst_chorus_idx.and_then(|ci| {
                let cl = &form.sections[ci].lines;
                if cl.is_empty() {
                    None
                } else {
                    form.lines[cl[k % cl.len()]].pitches.clone()
                }
            });
            let cad = if k == chunks - 1 {
                if sec_type == "outro" { "tonic" } else { "open" }
            } else {
                "none"
            };
            let shape = shape_for(k, chunks, cad, tess_c.inst.a, None, "");
            let chord_pcs: Vec<Vec<i32>> = chords.iter().map(|c| c.pcs.clone()).collect();
            let mut prng = rng_for(seed, &format!("ip|{}|{}", idx, k));
            let mut opts = PitchOpts {
                n,
                onsets: &rh.onsets,
                durs: &rh.durs,
                weights: Some(&rh.weights),
                chord_pcs: &chord_pcs,
                scales: &scales,
                t,
                tonic,
                center: t as f64 + tess_c.inst.c,
                shape: &*shape,
                cadence: cad,
                reference: reference.as_deref(),
                rng: &mut prng,
                prev_end: None,
                line_beats: rh.line_beats,
                prof: None,
                hook: 0,
            };
            let pitches = pitch_line(&mut opts);
            for i in 0..n {
                inst.push(InstNote { beat: b0 + rh.onsets[i], dur: rh.durs[i], midi: pitches[i], lift });
            }
        }
    }

    Comp { lead, inst, t, tonic }
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
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":"C G"}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":"Am F"}]},
                {"type":"chorus","same":true}
            ]
        });
        for (k, v) in extra.as_object().unwrap() {
            base.as_object_mut().unwrap().insert(k.clone(), v.clone());
        }
        crate::song::normalize_song(&base).unwrap()
    }

    #[test]
    fn cadence_for_matches_js() {
        assert_eq!(cadence_for("verse", 2, 3), "tonic");
        assert_eq!(cadence_for("bridge", 2, 3), "open");
        assert_eq!(cadence_for("verse", 1, 4), "open");
        assert_eq!(cadence_for("verse", 0, 4), "none");
    }

    #[test]
    fn contour_keys_order() {
        let names: Vec<&str> = ContourKind::KEYS.iter().map(|c| c.name()).collect();
        assert_eq!(names, ["arch", "descent", "rise", "valley", "wave", "peakLate", "peakEarly"]);
    }

    #[test]
    fn melody_profile_is_deterministic() {
        let s = song(json!({}));
        let p1 = melody_profile(42, &s);
        let p2 = melody_profile(42, &s);
        assert_eq!(p1.leap, p2.leap);
        assert_eq!(p1.hook, p2.hook);
    }

    #[test]
    fn compose_melody_fills_pitches_and_caches() {
        let s = song(json!({}));
        let mut form = crate::form::build_form(&s, 0);
        let tl = Timeline::new(&form, s.tempo);
        let comp = compose_melody(&s, &mut form, &tl, 7);
        assert!(!comp.lead.is_empty());
        for l in &form.lines {
            assert!(l.pitches.is_some());
        }
        // the two "same" choruses share text/type/li, so they hit the cache
        // and must produce identical pitches.
        let chorus_lines: Vec<&crate::form::FormLine> =
            form.lines.iter().filter(|l| form.sections[l.sec].type_ == "chorus").collect();
        assert_eq!(chorus_lines.len(), 2);
        assert_eq!(chorus_lines[0].pitches, chorus_lines[1].pitches);
    }
}

//! splitPh, nucTargets, consDur, voiceControls.
//! Ports engine.js lines ~425-537.

use sfcore::js::{self, clamp, f32r};
use sfcore::rng::Rng;
use sfcore::tuning::Tuning;

use compose::phonetics::{cons_map_cached, diph_map_cached, is_vowel, vowel_map_cached, Cons};
use compose::prepare::VocalNote;

/// renderVoice's / voiceControls's per-note input. JS: `{t0,t1,midi,ph,nu,amp,
/// phraseStart,phraseEnd,grace,stress}`. `ph` is the phoneme list for a lead/
/// harmony/double note; `nu` is a vowel-nucleus list used instead of `ph` for
/// choir notes (`ph` is then null/None).
#[derive(Clone, Debug)]
pub struct VoiceNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: i32,
    pub ph: Option<Vec<String>>,
    pub nu: Option<Vec<String>>,
    pub amp: f64,
    pub phrase_start: bool,
    pub phrase_end: bool,
    pub grace: Option<i32>,
    pub stress: bool,
}

impl From<&VocalNote> for VoiceNote {
    fn from(n: &VocalNote) -> Self {
        VoiceNote {
            t0: n.t0,
            t1: n.t1,
            midi: n.midi,
            ph: Some(n.ph.clone()),
            nu: None,
            amp: n.amp,
            phrase_start: n.phrase_start,
            phrase_end: n.phrase_end,
            grace: n.grace,
            stress: n.stress,
        }
    }
}

/// The opts bag renderVoice/voiceControls read. JS reads some fields with
/// `o.x||d` (0/NaN/undefined all mean "use default") and some with
/// `o.x==null?d:o.x` (only null/undefined mean "use default"); each field
/// below says which. `rng` and `seed` are threaded in by the caller (JS:
/// `renderVoice` assigns `opts.rng = opts.rng||rngFor(opts.seed||1,'v')`
/// before calling `voiceControls`), so by the time `voice_controls` runs,
/// `rng` must already be set.
pub struct VoiceOpts {
    /// opts.seed. `||1` default (renderVoice), separately `>>>0||12345`
    /// inside synthVoice's own local LCG (not read here). This carries the
    /// bits of the JS value after `ToUint32`/`ToInt32`; a value that was
    /// negative in JS arrives here as the same bit pattern reinterpreted as
    /// `u32` (e.g. JS `-1` is `u32::MAX`), matching `>>>0`.
    pub seed: Option<u32>,
    /// opts.rng: the shared draw stream. `None` until resolved. JS:
    /// `renderVoice` does `opts.rng = opts.rng || rngFor(opts.seed||1,'v')`
    /// before calling `voiceControls`/`synthVoice`; `render_voice` here does
    /// the same via `resolve_rng`. `voice_controls` and `synth_voice` both
    /// require it already resolved (`.expect(...)`), matching that JS never
    /// calls them with `opts.rng` unset.
    pub rng: Option<Rng>,
    /// opts.vibScale. `==null?1:` default: only None means 1.0.
    pub vib_scale: Option<f64>,
    /// opts.breathScale. Read by synthVoice, not voiceControls; kept here
    /// since it is part of the same opts bag. `||1` default.
    pub breath_scale: Option<f64>,
    /// opts.detune. `||0` default.
    pub detune: Option<f64>,
    /// opts.rateScale. `||1` default.
    pub rate_scale: Option<f64>,
    /// opts.noBreath. JS truthiness on an optional bool; default false.
    pub no_breath: bool,
    /// opts.rdScale. Read by synthVoice. `||1` default.
    pub rd_scale: Option<f64>,
    /// opts.hfGain. Read by synthVoice.
    pub hf_gain: Option<f64>,
    /// opts.nHigh. Read by synthVoice (`==null?9:5+min(4,nHigh)`).
    pub n_high: Option<f64>,
    /// opts.avTau. `||.009` default.
    pub av_tau: Option<f64>,
    /// opts.noScoop. Default false.
    pub no_scoop: bool,
    /// opts.glide. `||.028` default.
    pub glide: Option<f64>,
    /// opts.breathAmt. `||1` default. No call site in engine.js ever sets
    /// this, so it is always the default in practice; kept for parity.
    pub breath_amt: Option<f64>,
    /// opts.tiltScale. Read by synthVoice only.
    pub tilt_scale: Option<f64>,
}

impl Default for VoiceOpts {
    fn default() -> Self {
        VoiceOpts {
            seed: None,
            rng: None,
            vib_scale: None,
            breath_scale: None,
            detune: None,
            rate_scale: None,
            no_breath: false,
            rd_scale: None,
            hf_gain: None,
            n_high: None,
            av_tau: None,
            no_scoop: false,
            glide: None,
            breath_amt: None,
            tilt_scale: None,
        }
    }
}

/// `o.x||d`: 0, NaN and None (undefined) all mean "use default".
pub(crate) fn or_falsy(x: Option<f64>, d: f64) -> f64 {
    match x {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => d,
    }
}

/// `o.x==null?d:o.x`: only None (null/undefined) means "use default".
pub(crate) fn or_null(x: Option<f64>, d: f64) -> f64 {
    x.unwrap_or(d)
}

/// `opts.seed||1` (JS falsy: 0 counts as unset, same as None).
fn resolved_seed(seed: Option<u32>) -> u32 {
    match seed {
        Some(s) if s != 0 => s,
        _ => 1,
    }
}

/// `opts.rng = opts.rng||rngFor(opts.seed||1,'v')`. Idempotent: leaves an
/// already-set `rng` untouched. Called by `render_voice`; a caller of
/// `voice_controls`/`synth_voice` directly (as the parity tests do) must set
/// `opts.rng = Some(..)` itself first.
pub fn resolve_rng(opts: &mut VoiceOpts) {
    if opts.rng.is_none() {
        opts.rng = Some(sfcore::rng::rng_for(resolved_seed(opts.seed), "v"));
    }
}

/// splitPh(ph) result.
#[derive(Clone, Debug, Default)]
pub struct SplitPh {
    pub on: Vec<String>,
    pub nu: Vec<String>,
    pub co: Vec<String>,
}

/// splitPh(ph): onset consonants, the vowel-nucleus run (first vowel through
/// last vowel, inclusive of anything in between), and coda consonants.
pub fn split_ph(ph: &[String]) -> SplitPh {
    let mut i0: i32 = -1;
    let mut i1: i32 = -1;
    for (i, p) in ph.iter().enumerate() {
        if is_vowel(p) {
            if i0 < 0 {
                i0 = i as i32;
            }
            i1 = i as i32;
        }
    }
    if i0 < 0 {
        return SplitPh { on: vec![], nu: vec!["ah".to_string()], co: ph.to_vec() };
    }
    let i0 = i0 as usize;
    let i1 = i1 as usize;
    SplitPh {
        on: ph[..i0].to_vec(),
        nu: ph[i0..=i1].to_vec(),
        co: ph[i1 + 1..].to_vec(),
    }
}

/// One nucTargets() output entry.
#[derive(Clone, Copy, Debug)]
pub struct NucTarget {
    pub f: [f64; 3],
    pub k: &'static str,
    pub av: Option<f64>,
}

/// nucTargets(nu): expands a vowel-nucleus phoneme list into formant/kind
/// targets, splitting any diphthong into its two vowel halves and allowing a
/// nasal/sonorant coda-ish phoneme embedded in the nucleus run through.
pub fn nuc_targets(nu: &[String]) -> Vec<NucTarget> {
    let vowels = vowel_map_cached();
    let diphs = diph_map_cached();
    let conses = cons_map_cached();
    let mut out = Vec::new();
    for p in nu {
        if let Some(pair) = diphs.get(p.as_str()) {
            for v in pair {
                out.push(NucTarget { f: vowels[v], k: "vow", av: None });
            }
        } else if let Some(f) = vowels.get(p.as_str()) {
            out.push(NucTarget { f: *f, k: "vow", av: None });
        } else if let Some(c) = conses.get(p.as_str()) {
            if c.t == "son" || c.t == "nas" {
                out.push(NucTarget { f: c.f.unwrap_or([0.0, 0.0, 0.0]), k: cons_kind(c.t), av: c.av });
            }
        }
    }
    if out.is_empty() {
        out.push(NucTarget { f: vowels["ah"], k: "vow", av: None });
    }
    out
}

fn cons_kind(t: &str) -> &'static str {
    match t {
        "son" => "son",
        "nas" => "nas",
        other => panic!("cons_kind: unexpected consonant type {other}"),
    }
}

/// consDur(p,coda): a consonant's nominal duration.
pub fn cons_dur(p: &str, coda: bool) -> f64 {
    let conses = cons_map_cached();
    let c = match conses.get(p) {
        Some(c) => c,
        None => return 0.0,
    };
    if c.t == "stop" {
        let cl = c.cl.unwrap_or(0.0);
        return cl + 0.012 + if coda { 0.0 } else if c.v == Some(0) { 0.024 } else { 0.0 };
    }
    if c.t == "aff" {
        return c.cl.unwrap_or(0.0) + 0.008 + c.fr.unwrap_or(0.0);
    }
    if coda && c.t == "nas" {
        return 0.085;
    }
    c.d
}

/// gauss(r): Box-Muller normal deviate, ported here since sfcore::rng::Rng
/// already exposes it as a method (`r.gauss()`), matching this file's uses.
fn gauss(r: &mut Rng) -> f64 {
    r.gauss()
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// voiceControls's per-frame control tracks. One frame per HOP samples;
/// stored as Float32Array parity (f32, each write rounded through f32r).
pub struct VoiceControls {
    pub av: Vec<f32>,
    pub ah: Vec<f32>,
    pub af: Vec<f32>,
    pub ff: Vec<f32>,
    pub fbw: Vec<f32>,
    pub f1: Vec<f32>,
    pub f2: Vec<f32>,
    pub f3: Vec<f32>,
    pub nas: Vec<f32>,
    pub m: Vec<f32>,
    pub vb: Vec<f32>,
    pub b1x: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegKind {
    Vow,
    Son,
    Nas,
    Fric,
    Asp,
    Aspr,
    Burst,
    Clos,
    Sil,
    Breath,
}

#[derive(Clone, Debug)]
struct Seg {
    t0: f64,
    t1: f64,
    k: SegKind,
    f: [f64; 3],
    av: Option<f64>,
    af: Option<f64>,
    ff: Option<f64>,
    fbw: Option<f64>,
    ah: Option<f64>,
    b1x: Option<f64>,
    vb: Option<f64>,
    nas: Option<f64>,
}

fn seg_kind(k: &str) -> SegKind {
    match k {
        "vow" => SegKind::Vow,
        "son" => SegKind::Son,
        "nas" => SegKind::Nas,
        "fric" => SegKind::Fric,
        "asp" => SegKind::Asp,
        "aspr" => SegKind::Aspr,
        "burst" => SegKind::Burst,
        "clos" => SegKind::Clos,
        "sil" => SegKind::Sil,
        "breath" => SegKind::Breath,
        other => panic!("seg_kind: unknown segment kind {other}"),
    }
}

/// Per-note onset/coda timing info computed in voiceControls's "pass 1".
struct NoteInfo {
    sp: SplitPh,
    on: Vec<f64>,
    co: Vec<f64>,
    on_s: f64,
    on_start: f64,
}

/// voiceControls(notes,P,nF,opts): builds per-frame articulation/formant
/// control tracks for one voice line.
pub fn voice_controls(
    notes: &[VoiceNote],
    p: &compose::voices::VoiceParams,
    n_f: usize,
    opts: &mut VoiceOpts,
    tuning: &Tuning,
) -> VoiceControls {
    let fr = sfcore::SR_F / sfcore::HOP as f64;

    let mut av = vec![0.0f32; n_f];
    let mut ah = vec![0.0f32; n_f];
    let mut af = vec![0.0f32; n_f];
    let mut ff = vec![4000.0f32; n_f];
    let mut fbw = vec![3000.0f32; n_f];
    let mut f1 = vec![500.0f32; n_f];
    let mut f2 = vec![1500.0f32; n_f];
    let mut f3 = vec![2500.0f32; n_f];
    let mut nas = vec![0.0f32; n_f];
    let mut m = vec![0.0f32; n_f];
    let mut vb = vec![0.0f32; n_f];
    let mut b1x = vec![0.0f32; n_f];

    let sc = |f: [f64; 3]| -> [f64; 3] { [f[0] * p.f1s, f[1] * p.fs, f[2] * p.fs] };

    let mut segs: Vec<Seg> = Vec::new();
    let put = |segs: &mut Vec<Seg>, t0: f64, t1: f64, seg: Seg| {
        if t1 > t0 + 1e-4 {
            let mut s = seg;
            s.t0 = t0;
            s.t1 = t1;
            segs.push(s);
        }
    };
    // Convenience constructor: most fields default to None.
    let blank = |k: &str, f: [f64; 3]| Seg {
        t0: 0.0,
        t1: 0.0,
        k: seg_kind(k),
        f,
        av: None,
        af: None,
        ff: None,
        fbw: None,
        ah: None,
        b1x: None,
        vb: None,
        nas: None,
    };

    // pass 1: onset timing
    let mut info: Vec<NoteInfo> = notes
        .iter()
        .map(|n| {
            let mut sp = if let Some(ph) = &n.ph {
                split_ph(ph)
            } else {
                SplitPh { on: vec![], nu: n.nu.clone().unwrap_or_else(|| vec!["uw".to_string()]), co: vec![] }
            };
            if let Some(nu) = &n.nu {
                sp.nu = nu.clone();
            }
            let cs = or_falsy(Some(p.cons_scale), 1.0);
            let on: Vec<f64> = sp.on.iter().map(|ph| cons_dur(ph, false) * cs).collect();
            let co: Vec<f64> = sp.co.iter().map(|ph| cons_dur(ph, true) * cs).collect();
            NoteInfo { sp, on, co, on_s: 1.0, on_start: 0.0 }
        })
        .collect();

    for k in 1..notes.len() {
        let n = &notes[k];
        let pv = &notes[k - 1];
        let (single_td, pv_co_empty) = {
            let i = &info[k];
            let pi = &info[k - 1];
            let single_td = i.sp.on.len() == 1 && (i.sp.on[0] == "t" || i.sp.on[0] == "d");
            (single_td, pi.sp.co.is_empty())
        };
        if single_td && !n.stress && !n.phrase_start && pv_co_empty && n.t0 - pv.t1 < 0.15 {
            info[k].sp.on = vec!["dx".to_string()];
            info[k].on = vec![cons_dur("dx", false)];
        }
    }

    for k in 0..notes.len() {
        let n = &notes[k];
        let d: f64 = info[k].on.iter().sum();
        let avail = if k > 0 { (n.t0 - notes[k - 1].t0) * 0.45 } else { 0.3 };
        let s = if d > avail && d > 0.0 { avail / d } else { 1.0 };
        info[k].on_s = s;
        info[k].on_start = n.t0 - d * s;
    }

    let cons_scale_cur = or_falsy(Some(p.cons_scale), 1.0);
    let _ = cons_scale_cur; // used above per-note; kept for clarity only

    for k in 0..notes.len() {
        let n = &notes[k];
        let vt = nuc_targets(&info[k].sp.nu);
        let vf0 = sc(vt[0].f);
        let vfl = sc(vt[vt.len() - 1].f);

        // onset consonants
        let mut t = info[k].on_start;
        let on_ph = &info[k].sp.on;
        let on_dur = &info[k].on;
        let on_s = info[k].on_s;
        for (j, ph) in on_ph.iter().enumerate() {
            let d = on_dur[j] * on_s;
            emit_cons(&mut segs, &put, ph, t, t + d, vf0, n.amp, false, p, tuning, &sc);
            t += d;
        }
        let last_on = info[k].sp.on.last().cloned();
        let conses = cons_map_cached();
        let lc = last_on.as_ref().and_then(|ph| conses.get(ph.as_str()));
        let loc_f: Option<[f64; 3]> = lc.filter(|c| c.t == "stop" || c.t == "aff").map(|c| match &c.loc {
            Some(Some(loc)) => sc(*loc),
            _ => [250.0 * p.f1s, (2300.0f64).min(vf0[1] * 1.1), vf0[2]],
        });

        // coda
        let nx = notes.get(k + 1);
        let mut coda_end = n.t1;
        if let Some(nx) = nx {
            if info[k + 1].on_start < n.t1 + 0.03 {
                coda_end = info[k + 1].on_start.min((n.t0 + 0.06).max(n.t1));
            }
            let _ = nx;
        }
        let mut d2: f64 = info[k].co.iter().sum();
        let lim = (coda_end - n.t0) * 0.4;
        let s2 = if d2 > lim && d2 > 0.0 { lim / d2 } else { 1.0 };
        d2 *= s2;
        let coda_start = coda_end - d2;

        // nucleus
        let vlen = coda_start - n.t0;
        let mut n_start = n.t0;
        if tuning.vf.trans != 0.0 {
            if let Some(loc_f) = loc_f {
                if vt[0].k == "vow" {
                    let tt = (0.05f64).min((coda_start - n.t0) * 0.4);
                    let s = 5;
                    for j in 0..s {
                        let a = (j as f64 + 0.5) / s as f64;
                        let e = 1.0 - js::pow(1.0 - a, 1.6);
                        let f: [f64; 3] = [0, 1, 2].map(|q| loc_f[q] + (vf0[q] - loc_f[q]) * e);
                        put(
                            &mut segs,
                            n.t0 + tt * j as f64 / s as f64,
                            n.t0 + tt * (j as f64 + 1.0) / s as f64,
                            Seg { av: Some(n.amp * (0.7 + 0.3 * a)), ..blank("vow", f) },
                        );
                    }
                    n_start = n.t0 + tt;
                }
            }
        }
        if vt.len() == 1 {
            put(
                &mut segs,
                n_start,
                coda_start,
                Seg {
                    av: Some(n.amp * vt[0].av.unwrap_or(1.0)),
                    nas: Some(if vt[0].k == "nas" { 1.0 } else { 0.0 }),
                    ..blank(vt[0].k, sc(vt[0].f))
                },
            );
        } else {
            let tail = clamp(vlen * 0.3, 0.05, 0.18);
            let each = tail / (vt.len() - 1) as f64;
            put(
                &mut segs,
                n_start,
                coda_start - tail,
                Seg { av: Some(n.amp), ..blank("vow", sc(vt[0].f)) },
            );
            for j in 1..vt.len() {
                let a = coda_start - tail + (j - 1) as f64 * each;
                put(
                    &mut segs,
                    a,
                    a + each,
                    Seg {
                        av: Some(n.amp * vt[j].av.unwrap_or(1.0)),
                        nas: Some(if vt[j].k == "nas" { 1.0 } else { 0.0 }),
                        ..blank(vt[j].k, sc(vt[j].f))
                    },
                );
            }
        }
        t = coda_start;
        let co_ph = &info[k].sp.co;
        let co_dur = &info[k].co;
        for (j, ph) in co_ph.iter().enumerate() {
            let d = co_dur[j] * s2;
            emit_cons(&mut segs, &put, ph, t, t + d, vfl, n.amp, true, p, tuning, &sc);
            t += d;
        }

        // gap until next onset
        let next_on = if let Some(_nx) = nx { info[k + 1].on_start } else { coda_end + 0.5 };
        if next_on > coda_end + 0.01 {
            let nf = if let Some(_nx) = nx {
                sc(nuc_targets(&info[k + 1].sp.nu)[0].f)
            } else {
                vfl
            };
            if nx.map(|nx| nx.phrase_start).unwrap_or(false) && next_on - coda_end > 0.4 && !opts.no_breath {
                put(&mut segs, coda_end, next_on - 0.28, Seg { ..blank("sil", nf) });
                put(
                    &mut segs,
                    next_on - 0.28,
                    next_on - 0.04,
                    Seg { ah: None, ..blank("breath", sc([620.0, 1200.0, 2400.0])) },
                );
                put(&mut segs, next_on - 0.04, next_on, Seg { ..blank("sil", nf) });
            } else {
                put(&mut segs, coda_end, next_on, Seg { ..blank("sil", nf) });
            }
        }
    }

    segs.sort_by(|a, b| a.t0.partial_cmp(&b.t0).unwrap());
    for s in &segs {
        let i0 = (js::round(s.t0 * fr) as isize).max(0) as usize;
        let i1 = ((js::round(s.t1 * fr) as isize).max(0) as usize).min(n_f);
        for i in i0..i1 {
            f1[i] = f32r(s.f[0]) as f32;
            f2[i] = f32r(s.f[1]) as f32;
            f3[i] = f32r(s.f[2]) as f32;
            match s.k {
                SegKind::Vow | SegKind::Son | SegKind::Nas => {
                    av[i] = f32r(s.av.unwrap_or(0.0)) as f32;
                    nas[i] = f32r(s.nas.unwrap_or(0.0)) as f32;
                }
                SegKind::Fric => {
                    av[i] = f32r(s.av.unwrap_or(0.0)) as f32;
                    af[i] = f32r(s.af.unwrap_or(0.0)) as f32;
                    ff[i] = f32r(s.ff.unwrap_or(0.0)) as f32;
                    fbw[i] = f32r(s.fbw.unwrap_or(0.0)) as f32;
                }
                SegKind::Asp | SegKind::Aspr => {
                    ah[i] = f32r(s.ah.unwrap_or(0.0)) as f32;
                    b1x[i] = f32r(s.b1x.unwrap_or(0.0)) as f32;
                }
                SegKind::Burst => {
                    af[i] = f32r(s.af.unwrap_or(0.0)) as f32;
                    ff[i] = f32r(s.ff.unwrap_or(0.0)) as f32;
                    fbw[i] = f32r(s.fbw.unwrap_or(0.0)) as f32;
                    ah[i] = f32r(s.ah.unwrap_or(0.0)) as f32;
                    b1x[i] = f32r(s.b1x.unwrap_or(0.0)) as f32;
                }
                SegKind::Clos => {
                    av[i] = f32r(s.av.unwrap_or(0.0)) as f32;
                    vb[i] = f32r(s.vb.unwrap_or(0.0)) as f32;
                }
                SegKind::Breath => {
                    ah[i] = f32r(0.045 * or_falsy(opts.breath_amt, 1.0)) as f32;
                }
                SegKind::Sil => {}
            }
        }
    }

    // dynamics per note: swell and phrase-end fade; pitch track
    for k in 0..notes.len() {
        let n = &notes[k];
        let i0 = js::round(n.t0 * fr) as isize;
        let i1 = js::round(n.t1 * fr) as isize;
        let len = (1isize).max(i1 - i0) as f64;
        let dur = n.t1 - n.t0;
        let i0u = i0.max(0) as usize;
        let i1u = i1.max(0) as usize;
        for i in i0u..i1u.min(n_f) {
            let x = (i as f64 - i0 as f64) / len;
            let mut e = if dur > 0.5 { 0.9 + 0.16 * js::sin(std::f64::consts::PI * (x * 1.1).min(1.0)) } else { 1.0 };
            if n.phrase_end {
                e *= 1.0 - 0.4 * smoothstep(0.55, 1.0, x);
            }
            av[i] = f32r(av[i] as f64 * e) as f32;
        }
        let s0 = (js::round(info[k].on_start * fr) as isize).max(0) as usize;
        let e0 = if let Some(_nx) = notes.get(k + 1) {
            js::round(info[k + 1].on_start * fr) as isize
        } else {
            (n_f as isize).min(i1 + js::round(0.3 * fr) as isize)
        };
        let e0 = e0.max(0) as usize;
        for i in s0..e0.min(n_f) {
            m[i] = n.midi as f32;
        }
        if n.phrase_start && !opts.no_scoop {
            let s_end = (n_f).min(i0u + js::round(0.07 * fr) as usize);
            for i in s0..s_end {
                m[i] = (n.midi as f64 - 1.1) as f32;
            }
        }
        if let Some(grace) = n.grace {
            let g_end = n_f.min(i0u + js::round((0.11f64).min(dur * 0.25) * fr) as usize);
            for i in i0u..g_end {
                m[i] = grace as f32;
            }
        }
    }

    // backfill any leading zeros in M
    let mut first_m = 0.0f32;
    for &v in &m {
        if v != 0.0 {
            first_m = v;
            break;
        }
    }
    let mut last_m = first_m;
    for i in 0..n_f {
        if m[i] != 0.0 {
            last_m = m[i];
        } else {
            m[i] = last_m;
        }
    }

    let smooth = |a: &mut [f32], tau: f64| {
        let al = 1.0 - js::exp(-1.0 / (fr * tau));
        if a.is_empty() {
            return;
        }
        let mut y = a[0] as f64;
        for v in a.iter_mut() {
            y += al * (*v as f64 - y);
            *v = f32r(y) as f32;
        }
        let mut y = a[a.len() - 1] as f64;
        for v in a.iter_mut().rev() {
            y += al * (*v as f64 - y);
            *v = f32r(y) as f32;
        }
    };
    smooth(&mut f1, 0.016);
    smooth(&mut f2, 0.018);
    smooth(&mut f3, 0.02);
    smooth(&mut nas, 0.02);
    smooth(&mut av, or_falsy(opts.av_tau, 0.009));
    smooth(&mut ah, 0.006);
    smooth(&mut af, 0.002);
    smooth(&mut vb, 0.008);
    smooth(&mut b1x, 0.006);
    smooth(&mut ff, 0.004);
    smooth(&mut fbw, 0.004);

    // pitch glide (one-sided so the note arrives, then settles)
    {
        let al = 1.0 - js::exp(-1.0 / (fr * or_falsy(opts.glide, 0.028)));
        if !m.is_empty() {
            let mut y = m[0] as f64;
            for v in m.iter_mut() {
                y += al * (*v as f64 - y);
                *v = f32r(y) as f32;
            }
        }
    }

    // vibrato + drift
    let r = opts.rng.as_mut().expect("voice_controls: opts.rng must be resolved (see resolve_rng)");
    let mut vph = r.next() * 6.28;
    let mut drift = 0.0f64;
    let mut dv = 0.0f64;
    let vd = p.vib_depth * or_null(opts.vib_scale, 1.0);
    let mut vib = vec![0.0f32; n_f];
    for k in 0..notes.len() {
        let n = &notes[k];
        let dur = n.t1 - n.t0;
        if dur < 0.4 {
            continue;
        }
        let a = js::round((n.t0 + 0.22) * fr) as isize;
        let b = (n_f as isize).min(js::round(n.t1 * fr) as isize);
        let au = a.max(0) as usize;
        let bu = b.max(0) as usize;
        for i in au..bu.min(n_f) {
            vib[i] = f32r(vd * smoothstep(0.0, 0.38 * fr, i as f64 - a as f64) * if n.phrase_end { 1.15 } else { 1.0 }) as f32;
        }
    }
    smooth(&mut vib, 0.05);
    let rate = p.vib_rate * or_falsy(opts.rate_scale, 1.0);
    for i in 0..n_f {
        vph += 2.0 * std::f64::consts::PI * rate * (1.0 + 0.06 * js::sin(i as f64 / fr * 0.7)) / fr;
        dv += gauss(r) * 0.004;
        dv *= 0.985;
        drift += dv;
        drift *= 0.998;
        let v = m[i] as f64 + vib[i] as f64 * js::sin(vph) + clamp(drift, -0.12, 0.12) + or_falsy(opts.detune, 0.0);
        m[i] = f32r(v) as f32;
    }

    VoiceControls { av, ah, af, ff, fbw, f1, f2, f3, nas, m, vb, b1x }
}

#[allow(clippy::too_many_arguments)]
fn emit_cons(
    segs: &mut Vec<Seg>,
    put: &dyn Fn(&mut Vec<Seg>, f64, f64, Seg),
    ph: &str,
    t0: f64,
    t1: f64,
    vf: [f64; 3],
    amp: f64,
    coda: bool,
    p: &compose::voices::VoiceParams,
    tuning: &Tuning,
    sc: &dyn Fn([f64; 3]) -> [f64; 3],
) {
    let conses = cons_map_cached();
    let c: &Cons = match conses.get(ph) {
        Some(c) => c,
        None => return,
    };
    let blank = |k: &str, f: [f64; 3]| Seg {
        t0: 0.0,
        t1: 0.0,
        k: seg_kind(k),
        f,
        av: None,
        af: None,
        ff: None,
        fbw: None,
        ah: None,
        b1x: None,
        vb: None,
        nas: None,
    };

    if c.t == "son" || c.t == "nas" {
        let f = sc(c.f.unwrap_or([0.0, 0.0, 0.0]));
        let f = [0, 1, 2].map(|i| f[i] * 0.75 + vf[i] * 0.25);
        put(
            segs,
            t0,
            t1,
            Seg { av: Some(amp * c.av.unwrap_or(0.0)), nas: Some(if c.t == "nas" { 1.0 } else { 0.0 }), ..blank(c.t, f) },
        );
        return;
    }
    if c.t == "fric" {
        put(
            segs,
            t0,
            t1,
            Seg {
                av: Some(if c.v == Some(1) { amp * c.vv.unwrap_or(0.35) } else { 0.0 }),
                af: Some(c.af.unwrap_or(0.0) * amp),
                ff: c.ff,
                fbw: c.bw,
                ..blank("fric", vf)
            },
        );
        return;
    }
    if c.t == "asp" {
        put(segs, t0, t1, Seg { ah: Some(0.5 * amp), ..blank("asp", vf) });
        return;
    }
    let loc = match &c.loc {
        Some(Some(loc)) => sc(*loc),
        _ => [250.0 * p.f1s, (2300.0f64).min(vf[1] * 1.1), vf[2]],
    };
    let cf = [0, 1, 2].map(|i| loc[i] * 0.6 + vf[i] * 0.4);
    // JS: `c.loc?c.ff:...` -- c.loc is null (falsy) for k/g even though the
    // JS object literal still has the key; only Some(Some(_)) is truthy.
    let ff = if matches!(c.loc, Some(Some(_))) { c.ff.unwrap_or(0.0) } else if vf[1] > 1500.0 { 3000.0 } else { 1800.0 };

    if c.t == "stop" && tuning.vf.legacy != 0.0 {
        let cl = (t1 - t0) * (c.cl.unwrap_or(0.0) / cons_dur(ph, coda));
        put(segs, t0, t0 + cl, Seg { av: Some(if c.v == Some(1) { amp * 0.1 } else { 0.0 }), ..blank("clos", cf) });
        let b = t0 + cl;
        put(
            segs,
            b,
            b + 0.012,
            Seg {
                af: Some(if coda { 0.4 } else { 0.7 } * amp * tuning.vf.burst),
                ff: Some(ff),
                fbw: c.bw,
                ah: Some(0.12 * amp),
                ..blank("burst", cf)
            },
        );
        if !coda && c.v != Some(1) {
            put(
                segs,
                b + 0.012,
                t1,
                Seg { ah: Some(0.4 * amp * tuning.vf.asp), b1x: Some(320.0), ..blank("aspr", vf) },
            );
        }
        return;
    }
    if c.t == "stop" {
        let cl = (t1 - t0) * (c.cl.unwrap_or(0.0) / cons_dur(ph, coda));
        put(
            segs,
            t0,
            t0 + cl,
            Seg {
                vb: Some(if c.v == Some(1) { amp * tuning.vbg * if c.flap == Some(1) { 1.3 } else { 1.0 } } else { 0.0 }),
                ..blank("clos", cf)
            },
        );
        let b = t0 + cl;
        let bd = if c.v == Some(1) { 0.005 } else { 0.007 };
        let af_val = if coda {
            0.12
        } else if c.flap == Some(1) {
            0.05
        } else if c.v == Some(1) {
            tuning.bd_v
        } else {
            tuning.bd_t
        };
        put(
            segs,
            b,
            b + bd,
            Seg {
                af: Some(af_val * amp * tuning.bst),
                ff: Some(ff),
                fbw: c.bw.map(|x| x * 0.7),
                ah: Some(if c.v == Some(1) { 0.03 } else { 0.05 } * amp),
                b1x: Some(250.0),
                ..blank("burst", cf)
            },
        );
        if !coda && c.v != Some(1) {
            let f = [0, 1, 2].map(|i| if i == 0 { vf[i] } else { vf[i] * 0.97 });
            put(segs, b + bd, t1, Seg { ah: Some(0.6 * amp * tuning.aspg), b1x: Some(320.0), ..blank("aspr", f) });
        } else if !coda && c.v == Some(1) && t1 > b + bd {
            let f = [0, 1, 2].map(|i| cf[i] + (vf[i] - cf[i]) * 0.3);
            put(segs, b + bd, t1, Seg { av: Some(amp * 0.55), ..blank("vow", f) });
        }
        return;
    }
    if c.t == "aff" {
        let tot = t1 - t0;
        let cl = tot * c.cl.unwrap_or(0.0) / cons_dur(ph, coda);
        put(segs, t0, t0 + cl, Seg { av: Some(if c.v == Some(1) { amp * 0.1 } else { 0.0 }), ..blank("clos", cf) });
        let burst_dur = tot * 0.008 / cons_dur(ph, coda);
        put(
            segs,
            t0 + cl,
            t0 + cl + burst_dur,
            Seg { af: Some(0.6 * amp), ff: c.ff, fbw: c.bw, ..blank("burst", cf) },
        );
        put(
            segs,
            t0 + cl + burst_dur,
            t1,
            Seg {
                af: Some(c.af.unwrap_or(0.0) * amp),
                ff: c.ff,
                fbw: c.bw,
                av: Some(if c.v == Some(1) { amp * 0.3 } else { 0.0 }),
                ..blank("fric", vf)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }
    fn ph(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn split_ph_basic_cvc() {
        let sp = split_ph(&ph(&["k", "ae", "t"]));
        assert_eq!(sp.on, vec![s("k")]);
        assert_eq!(sp.nu, vec![s("ae")]);
        assert_eq!(sp.co, vec![s("t")]);
    }

    #[test]
    fn split_ph_no_vowel_falls_back_to_ah() {
        let sp = split_ph(&ph(&["s", "t"]));
        assert!(sp.on.is_empty());
        assert_eq!(sp.nu, vec![s("ah")]);
        assert_eq!(sp.co, vec![s("s"), s("t")]);
    }

    #[test]
    fn split_ph_diphthong_and_onset_cluster() {
        let sp = split_ph(&ph(&["s", "t", "aa", "ih", "n"]));
        assert_eq!(sp.on, vec![s("s"), s("t")]);
        // "aa" and "ih" are both vowels; run spans i0..=i1.
        assert_eq!(sp.nu, vec![s("aa"), s("ih")]);
        assert_eq!(sp.co, vec![s("n")]);
    }

    #[test]
    fn cons_dur_stop_voiceless_onset_adds_aspiration_margin() {
        // "t": stop, voiceless (v=0), cl=0.045 -> onset (coda=false): +0.024
        let d = cons_dur("t", false);
        assert!((d - (0.045 + 0.012 + 0.024)).abs() < 1e-12, "{d}");
    }

    #[test]
    fn cons_dur_stop_voiced_no_aspiration_margin() {
        // "d": voiced (v=1) -> the (coda? 0 : v?0:0.024) term is 0 either way
        let d = cons_dur("d", false);
        assert!((d - (0.05 + 0.012)).abs() < 1e-12, "{d}");
    }

    #[test]
    fn cons_dur_stop_coda_drops_aspiration_margin() {
        let on = cons_dur("t", false);
        let co = cons_dur("t", true);
        assert!(co < on);
        assert!((co - (0.045 + 0.012)).abs() < 1e-12);
    }

    #[test]
    fn cons_dur_affricate() {
        // "ch": cl=0.04, fr=0.07 -> cl+0.008+fr
        let d = cons_dur("ch", false);
        assert!((d - (0.04 + 0.008 + 0.07)).abs() < 1e-12, "{d}");
    }

    #[test]
    fn cons_dur_nasal_coda_is_fixed() {
        assert_eq!(cons_dur("n", true), 0.085);
        assert_ne!(cons_dur("n", false), 0.085);
    }

    #[test]
    fn cons_dur_unknown_phoneme_is_zero() {
        assert_eq!(cons_dur("zzz", false), 0.0);
    }

    #[test]
    fn nuc_targets_plain_vowel() {
        let t = nuc_targets(&ph(&["ae"]));
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].k, "vow");
    }

    #[test]
    fn nuc_targets_diphthong_expands_to_two() {
        let t = nuc_targets(&ph(&["ay"]));
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].k, "vow");
        assert_eq!(t[1].k, "vow");
    }

    #[test]
    fn nuc_targets_empty_falls_back_to_ah() {
        let t = nuc_targets(&[]);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].f, vowel_map_cached()["ah"]);
    }

    #[test]
    fn or_falsy_treats_zero_as_default() {
        assert_eq!(or_falsy(Some(0.0), 5.0), 5.0);
        assert_eq!(or_falsy(None, 5.0), 5.0);
        assert_eq!(or_falsy(Some(2.0), 5.0), 2.0);
    }

    #[test]
    fn or_null_keeps_zero() {
        assert_eq!(or_null(Some(0.0), 5.0), 0.0);
        assert_eq!(or_null(None, 5.0), 5.0);
    }
}

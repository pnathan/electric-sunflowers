//! `TRACKS`, `EQ`, `BODY_OF`, `activeRms`, `processTrack`, `mixSong` (engine.js
//! lines ~1179-1216): the per-track EQ/gain/compression cache and the final mix
//! (pan, sends, reverb, bus compression, peak normalisation).

use crate::body::Body;
use crate::dynamics::{compress, db_of, stereo_compress};
use crate::filter::{bq, run_bq, FilterType};
use crate::reverb::fdn_reverb;
use rayon::prelude::*;
use sfcore::js;
use sfcore::{SR, SR_F};

/// Number of `TRACKS` entries; also the length of `Render`'s fixed arrays.
const N_TRACKS: usize = 10;

/// Position of a track key in `TRACKS`, used to index `Render`'s fixed
/// arrays instead of a string-keyed map (every key is known at compile
/// time, so this never allocates or hashes).
fn track_index(key: &str) -> usize {
    TRACKS.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("unknown track {key}"))
}

/// One entry of `TRACKS` in engine.js.
#[derive(Clone, Copy, Debug)]
pub struct TrackSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub gain: f64,
    pub pan: f64,
    pub send: f64,
    pub always: bool,
    pub band: Option<&'static str>,
}

/// `TRACKS` in engine.js, in order (mix order matters: pan/send accumulate in
/// this order, and `k` in the progress callback counts through it).
pub const TRACKS: [TrackSpec; 10] = [
    TrackSpec { key: "lead", label: "Lead vocal", gain: 1.0, pan: 0.0, send: 0.2, always: true, band: None },
    TrackSpec { key: "doubles", label: "Melody doubles", gain: 0.3, pan: 0.0, send: 0.34, always: false, band: Some("doubles") },
    TrackSpec { key: "harmony", label: "Harmony vocal", gain: 0.4, pan: 0.28, send: 0.32, always: false, band: Some("harmonies") },
    TrackSpec { key: "choir", label: "Backing choir", gain: 0.36, pan: 0.0, send: 0.5, always: false, band: Some("choir") },
    TrackSpec { key: "guitar", label: "Guitar", gain: 0.62, pan: -0.2, send: 0.16, always: true, band: None },
    TrackSpec { key: "hg", label: "Harmony guitar", gain: 0.36, pan: 0.45, send: 0.28, always: false, band: Some("harmonyGuitar") },
    TrackSpec { key: "bass", label: "Bass", gain: 0.5, pan: 0.0, send: 0.04, always: false, band: Some("bass") },
    TrackSpec { key: "drums", label: "Drums", gain: 0.42, pan: 0.0, send: 0.14, always: false, band: Some("drums") },
    TrackSpec { key: "harp", label: "Harp", gain: 0.36, pan: 0.34, send: 0.4, always: false, band: Some("harp") },
    TrackSpec { key: "violin", label: "Violin", gain: 0.34, pan: -0.4, send: 0.42, always: false, band: Some("violin") },
];

/// One `[type, freq, Q, gainDb]` entry of an `EQ[key]` chain.
pub type EqBand = (FilterType, f64, f64, f64);

/// `EQ` in engine.js.
pub fn eq_for(key: &str) -> &'static [EqBand] {
    use FilterType::*;
    match key {
        "lead" => &[(Hp, 90.0, 0.7, 0.0), (Pk, 250.0, 1.0, -1.5), (Pk, 2900.0, 1.0, 1.5)],
        "doubles" => &[(Hp, 140.0, 0.7, 0.0), (Hs, 6000.0, 0.7, -6.0)],
        "harmony" => &[(Hp, 130.0, 0.7, 0.0), (Hs, 6500.0, 0.7, -5.0)],
        "choir" => &[(Hp, 120.0, 0.7, 0.0), (Lp, 6500.0, 0.7, 0.0)],
        "guitar" => &[(Hp, 70.0, 0.7, 0.0), (Pk, 115.0, 0.9, 3.0), (Hs, 9500.0, 0.7, -2.0)],
        "hg" => &[(Hp, 120.0, 0.7, 0.0), (Hs, 9000.0, 0.7, -2.0)],
        "bass" => &[(Lp, 2000.0, 0.7, 0.0), (Pk, 85.0, 1.0, 2.0)],
        "drums" => &[(Hp, 30.0, 0.7, 0.0)],
        "harp" => &[(Hp, 75.0, 0.7, 0.0), (Pk, 180.0, 0.8, 1.5)],
        "violin" => &[(Hp, 190.0, 0.7, 0.0), (Hs, 2800.0, 0.7, 7.0), (Hs, 7000.0, 0.7, 5.0)],
        _ => &[],
    }
}

/// `BODY_OF` in engine.js: `[bodyCurveName, seedOffset, refPeak]` per track key
/// that gets convolved with a measured body impulse response.
pub fn body_of(key: &str) -> Option<(Body, f64, f64)> {
    match key {
        "guitar" => Some((Body::Guitar, 0.0, 5.4)),
        "hg" => Some((Body::Guitar, 17.0, 5.4)),
        "harp" => Some((Body::Harp, 0.0, 7.3)),
        "violin" => Some((Body::Violin, 0.0, 2.4)),
        _ => None,
    }
}

/// `activeRms(a)`: RMS over 2048-sample blocks whose level exceeds 5% of the
/// loudest block, ignoring silence/lead-in.
pub fn active_rms(a: &[f32]) -> f64 {
    const B: usize = 2048;
    let mut mx = 0.0f64;
    let mut rs: Vec<f64> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        let e = (i + B).min(a.len());
        let mut s = 0.0f64;
        for j in i..e {
            let v = a[j] as f64;
            s += v * v;
        }
        let v = (s / (e - i) as f64).sqrt();
        rs.push(v);
        if v > mx {
            mx = v;
        }
        i += B;
    }
    let act: Vec<f64> = rs.into_iter().filter(|&v| v > mx * 0.05).collect();
    if act.is_empty() {
        return 0.0;
    }
    (act.iter().map(|v| v * v).sum::<f64>() / act.len() as f64).sqrt()
}

/// A cache slot for a processed track: not yet computed, computed and
/// silent (dropped, like JS's `render.proc[key]=null`), or computed and
/// present.
#[derive(Default)]
enum ProcSlot {
    #[default]
    Empty,
    Silent,
    Ready(Vec<Vec<f32>>),
}

impl ProcSlot {
    fn as_ready(&self) -> Option<&Vec<Vec<f32>>> {
        match self {
            ProcSlot::Ready(chs) => Some(chs),
            _ => None,
        }
    }
}

/// The render state `processTrack`/`mixSong` operate on: raw tracks (taken
/// out of the array as each is processed, so nothing is ever copied), the
/// per-track processed cache (indexed the same way, by position in
/// `TRACKS`), and the lead slapback delay computed once when the lead
/// track is processed. A re-mix with a different `enabled` set reads the
/// same cache, so it costs no extra copies either.
#[derive(Default)]
pub struct Render {
    pub len: usize,
    tracks: [Option<Vec<Vec<f32>>>; N_TRACKS],
    proc: [ProcSlot; N_TRACKS],
    pub lead_delay: Option<Vec<f32>>,
}

impl Render {
    pub fn new(len: usize) -> Self {
        Render { len, tracks: Default::default(), proc: Default::default(), lead_delay: None }
    }

    /// Sets a raw track's channels (1 or 2, as the real tracks have).
    pub fn set_track(&mut self, key: &str, chs: Vec<Vec<f32>>) {
        self.tracks[track_index(key)] = Some(chs);
    }
}

/// The pure, per-track half of `processTrack`: EQ, gain-to-target and
/// (lead/harmony) compression, plus the lead's slapback tail. Depends only
/// on its own input channels, so this is what the threaded mix path runs
/// in parallel; `process_track`/`process_tracks_threaded` below are the
/// two ways of getting a track's raw channels into this function and its
/// result back into `Render`'s cache.
fn compute_track(key: &str, mut chs: Vec<Vec<f32>>) -> (ProcSlot, Option<Vec<f32>>) {
    for &(ty, f, q, g) in eq_for(key) {
        let co = bq(ty, f, q, g);
        for c in chs.iter_mut() {
            run_bq(c, &co);
        }
    }

    let mut rms = 0.0f64;
    for c in &chs {
        rms = js::max(rms, active_rms(c));
    }
    if rms < 1e-6 {
        return (ProcSlot::Silent, None);
    }
    let g0 = 0.1 / rms;
    for c in chs.iter_mut() {
        for v in c.iter_mut() {
            *v = js::f32r(*v as f64 * g0) as f32;
        }
    }

    if key == "lead" || key == "harmony" {
        for c in chs.iter_mut() {
            compress(c, db_of(0.1) + 1.0, 3.0, 0.008, 0.15, Some(6.0));
        }
    }

    let mut lead_delay = None;
    if key == "lead" {
        // slapback into the reverb and bus, computed once
        let x = &chs[0];
        let dl = js::round(0.34 * SR_F) as usize;
        let mut o = vec![0.0f32; x.len()];
        let lp = bq(FilterType::Lp, 3200.0, 0.7, 0.0);
        let (mut x1, mut x2, mut y1, mut y2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let mut i = dl;
        while i < x.len() {
            let v = x[i - dl] as f64 + 0.22 * o[i - dl] as f64;
            let y = lp.b0 * v + lp.b1 * x1 + lp.b2 * x2 - lp.a1 * y1 - lp.a2 * y2;
            x2 = x1;
            x1 = v;
            y2 = y1;
            y1 = y;
            o[i] = js::f32r(y) as f32;
            i += 1;
        }
        for v in o.iter_mut() {
            *v = js::f32r(*v as f64 * 0.07) as f32;
        }
        lead_delay = Some(o);
    }

    (ProcSlot::Ready(chs), lead_delay)
}

/// `processTrack(render,T)`: per-track EQ, gain-to-target and (lead/harmony)
/// compression, cached on `render.proc`; the raw track is taken (removed)
/// from `render.tracks` and processed in place, never copied.
pub fn process_track(render: &mut Render, t: &TrackSpec) {
    let idx = track_index(t.key);
    if !matches!(render.proc[idx], ProcSlot::Empty) {
        return;
    }
    let chs = match render.tracks[idx].take() {
        Some(c) => c,
        None => {
            render.proc[idx] = ProcSlot::Silent;
            return;
        }
    };
    let (slot, lead_delay) = compute_track(t.key, chs);
    if let Some(ld) = lead_delay {
        render.lead_delay = Some(ld);
    }
    render.proc[idx] = slot;
}

/// Runs `compute_track` for every one of `specs` whose slot is still
/// `Empty`, in parallel (rayon), then writes each result back into
/// `render`'s cache sequentially. `process_track`/`mix_song`'s later
/// per-track loop then finds every slot already filled and does no
/// further work, so this is purely an ordering change: which tracks get
/// computed is identical to the sequential path, only when they run
/// differs. Taking each raw track out of `render.tracks` (a plain `Vec`
/// move, no computation) happens up front, sequentially, since `Render`'s
/// fixed arrays cannot be indexed from multiple threads at once; only the
/// EQ/gain/compression work itself — the expensive part — runs in
/// parallel.
fn process_tracks_threaded(render: &mut Render, specs: &[TrackSpec]) {
    let mut work: Vec<(usize, &'static str, Vec<Vec<f32>>)> = Vec::new();
    for t in specs {
        let idx = track_index(t.key);
        if !matches!(render.proc[idx], ProcSlot::Empty) {
            continue;
        }
        match render.tracks[idx].take() {
            Some(c) => work.push((idx, t.key, c)),
            None => render.proc[idx] = ProcSlot::Silent,
        }
    }

    let results: Vec<(usize, ProcSlot, Option<Vec<f32>>)> = work
        .into_par_iter()
        .map(|(idx, key, chs)| {
            let (slot, ld) = compute_track(key, chs);
            (idx, slot, ld)
        })
        .collect();

    for (idx, slot, ld) in results {
        if let Some(ld) = ld {
            render.lead_delay = Some(ld);
        }
        render.proc[idx] = slot;
    }
}

/// `panInto(bL,bR,chs,pan,gain,sL,sR,send)`.
pub fn pan_into(b_l: &mut [f32], b_r: &mut [f32], chs: &[Vec<f32>], pan: f64, gain: f64, s_l: &mut [f32], s_r: &mut [f32], send: f64) {
    let n = b_l.len();
    if chs.len() == 1 {
        let gl = js::cos((pan + 1.0) * std::f64::consts::PI / 4.0) * gain;
        let gr = js::sin((pan + 1.0) * std::f64::consts::PI / 4.0) * gain;
        let x = &chs[0];
        for i in 0..n {
            let v = x[i] as f64;
            b_l[i] = js::f32r(b_l[i] as f64 + v * gl) as f32;
            b_r[i] = js::f32r(b_r[i] as f64 + v * gr) as f32;
            s_l[i] = js::f32r(s_l[i] as f64 + v * gl * send) as f32;
            s_r[i] = js::f32r(s_r[i] as f64 + v * gr * send) as f32;
        }
        return;
    }
    let (l, r) = (&chs[0], &chs[1]);
    let (a, b, c, d);
    if pan <= 0.0 {
        let x = (pan + 1.0) * std::f64::consts::PI / 2.0;
        a = 1.0;
        b = js::cos(x);
        c = 0.0;
        d = js::sin(x);
    } else {
        let x = pan * std::f64::consts::PI / 2.0;
        a = js::cos(x);
        b = 0.0;
        c = js::sin(x);
        d = 1.0;
    }
    for i in 0..n {
        let lv = l[i] as f64;
        let rv = r[i] as f64;
        let ol = (a * lv + b * rv) * gain;
        let or_ = (c * lv + d * rv) * gain;
        b_l[i] = js::f32r(b_l[i] as f64 + ol) as f32;
        b_r[i] = js::f32r(b_r[i] as f64 + or_) as f32;
        s_l[i] = js::f32r(s_l[i] as f64 + ol * send) as f32;
        s_r[i] = js::f32r(s_r[i] as f64 + or_ * send) as f32;
    }
}

/// The final stereo mix, as `mixSong` returns it (`{L,R,duration,sampleRate}`).
pub struct MixResult {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub duration: f64,
    pub sample_rate: usize,
}

/// `mixSong(render,enabled,seed,progress)`. `enabled` decides which `TRACKS`
/// entries mix in; `progress(label, frac)` replaces the JS async progress
/// callback (there are no async ticks here). Sequential path: every
/// enabled track is EQ'd/gained/compressed one at a time, in `TRACKS`
/// order, exactly as `mix_song_threaded` sums them, so the two are
/// bit-identical.
pub fn mix_song(render: &mut Render, enabled: impl Fn(&TrackSpec) -> bool, seed: i64, progress: Option<&mut dyn FnMut(&str, f64)>) -> MixResult {
    mix_song_impl(render, enabled, seed, progress, false)
}

/// Same as `mix_song`, but every enabled track's `process_track` work (EQ,
/// gain, compression, the lead's slapback tail) runs in parallel first
/// (`process_tracks_threaded`), since each track's processing depends only
/// on its own raw channels. The pan/send accumulation and the lead-delay
/// add-in afterwards are unchanged and still run in `TRACKS` order, so the
/// summed mix is bit-identical to `mix_song`'s. No progress callback, for
/// the same reason `render_song_threaded` drops one.
pub fn mix_song_threaded(render: &mut Render, enabled: impl Fn(&TrackSpec) -> bool, seed: i64) -> MixResult {
    mix_song_impl(render, enabled, seed, None, true)
}

fn mix_song_impl(
    render: &mut Render,
    enabled: impl Fn(&TrackSpec) -> bool,
    seed: i64,
    mut progress: Option<&mut dyn FnMut(&str, f64)>,
    threaded: bool,
) -> MixResult {
    let len = render.len;
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    let mut s_l = vec![0.0f32; len];
    let mut s_r = vec![0.0f32; len];

    if threaded {
        let specs: Vec<TrackSpec> = TRACKS.iter().copied().filter(|t| enabled(t)).collect();
        process_tracks_threaded(render, &specs);
    }

    let mut k = 0usize;
    for t in TRACKS.iter() {
        if !enabled(t) {
            continue;
        }
        process_track(render, t);
        k += 1;
        if let Some(p) = progress.as_deref_mut() {
            p("Mixing", 0.95 + 0.03 * k as f64 / TRACKS.len() as f64);
        }
        let idx = track_index(t.key);
        let Some(chs) = render.proc[idx].as_ready() else { continue };
        pan_into(&mut l, &mut r, chs, t.pan, t.gain, &mut s_l, &mut s_r, t.send);
        if t.key == "lead" {
            if let Some(o) = render.lead_delay.as_deref() {
                for i in 0..len {
                    l[i] = js::f32r(l[i] as f64 + o[i] as f64) as f32;
                    r[i] = js::f32r(r[i] as f64 + o[i] as f64) as f32;
                    s_l[i] = js::f32r(s_l[i] as f64 + o[i] as f64) as f32;
                    s_r[i] = js::f32r(s_r[i] as f64 + o[i] as f64) as f32;
                }
            }
        }
    }

    fdn_reverb(&s_l, &s_r, &mut l, &mut r, 0.55, seed);

    let mut e = 0.0f64;
    let mut c = 0usize;
    let mut i = 0usize;
    while i < len {
        let v = (l[i] as f64).abs() + (r[i] as f64).abs();
        if v > 1e-4 {
            e += l[i] as f64 * l[i] as f64 + r[i] as f64 * r[i] as f64;
            c += 2;
        }
        i += 4;
    }
    let bus_rms = (e / c.max(1) as f64).sqrt();
    stereo_compress(&mut l, &mut r, db_of(bus_rms) + 5.0, 2.2, 0.02, 0.3);

    let mut pk = 1e-9f64;
    for i in 0..len {
        let a = js::max((l[i] as f64).abs(), (r[i] as f64).abs());
        if a > pk {
            pk = a;
        }
    }
    let g = 0.89 / pk;
    for i in 0..len {
        l[i] = js::f32r(l[i] as f64 * g) as f32;
        r[i] = js::f32r(r[i] as f64 * g) as f32;
    }

    MixResult { l, r, duration: len as f64 / SR_F, sample_rate: SR }
}

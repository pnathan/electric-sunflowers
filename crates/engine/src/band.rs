//! renderSong's instrumental tracks: guitar, bass, drums, harp, violin and
//! harmony guitar (engine.js ~913-944), plus the body-convolution loop
//! (engine.js ~945-949).

use compose::form::Form;
use compose::melody::LeadNote;
use compose::prepare::Prepared;
use compose::timeline::Timeline;
use song::{BreakLead, Pc, SectionKind, Song};
use dsp::pluck::{pluck, PluckOpts};
use sfcore::js::pow;
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use sfcore::SR_F;

use arrange::bass::gen_bass;
use arrange::drums::gen_drums;
use arrange::guitar::gen_guitar;
use arrange::harp::gen_harp;
use arrange::lines::{counter_line, fills_for};

use dsp::violin::{render_violin, ViolinNote};

/// `mtof(m)` from engine.js. `guitar.rs`'s copy is `pub(crate)` to `arrange`
/// and not reachable from here, so this is a second, identical copy for the
/// harmony-guitar pluck calls `renderSong` makes directly (engine.js
/// ~930-944), not through `genGuitar`.
fn mtof(m: f64) -> f64 {
    440.0 * pow(2.0, (m - 69.0) / 12.0)
}

/// `tracks.guitar=[genGuitar(song,form,tl,seed)]`.
pub fn render_guitar(song: &Song, form: &Form, tl: &Timeline, seed: u32, tuning: &Tuning) -> Vec<Vec<f32>> {
    vec![gen_guitar(song, form, tl, seed, tuning)]
}

/// `tracks.bass=[genBass(song,form,tl,seed)]`.
pub fn render_bass(song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<Vec<f32>> {
    vec![gen_bass(song, form, tl, seed)]
}

/// `tracks.drums=genDrums(song,form,tl,seed)` (already stereo in JS).
pub fn render_drums(song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<Vec<f32>> {
    let [l, r] = gen_drums(song, form, tl, seed);
    vec![l, r]
}

/// `tracks.harp=[genHarp(song,form,tl,seed)]`.
pub fn render_harp(song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<Vec<f32>> {
    vec![gen_harp(song, form, tl, seed)]
}

/// The violin and harmony-guitar tracks, each a raw mono channel.
/// `break_lead` picks who plays the instrumental lead lines: the violin,
/// the harmony guitar, or both.
pub fn render_violin_and_harmony_guitar(
    prepared: &Prepared,
    song: &Song,
    seed: u32,
    len: usize,
    break_lead: BreakLead,
) -> (Vec<f32>, Vec<f32>) {
    let form = &prepared.form;
    let tl = &prepared.timeline;
    let lead: &[LeadNote] = &prepared.comp.lead;

    // violin: intro/outro/interlude lead, chorus counter-line, bridge long
    // tones, later-verse fills.
    let inst_v: Vec<ViolinNote> = if break_lead == BreakLead::Guitar {
        Vec::new()
    } else {
        prepared
            .comp
            .inst
            .iter()
            .map(|n| {
                let mut m = n.midi;
                while m < 64 {
                    m += 12;
                }
                while m > 86 {
                    m -= 12;
                }
                ViolinNote { t0: tl.to_time(n.beat), t1: tl.to_time(n.beat + n.dur) - 0.02, m: m as f64, v: 0.65, vib: None }
            })
            .collect()
    };
    let ctr = counter_line(form, tl, lead, 67, 86, |s| s.is_lift(), seed, false);
    let br = counter_line(form, tl, lead, 62, 79, |s| s.kind == SectionKind::Bridge, seed, true);
    let fl = fills_for(form, tl, lead, 69, 88, |s| s.kind == SectionKind::Verse && s.occ > 0, song, seed);

    let mut vnotes: Vec<ViolinNote> = inst_v;
    for n in ctr.iter().chain(br.iter()).chain(fl.iter()) {
        vnotes.push(ViolinNote { t0: n.t0, t1: n.t1, m: n.m as f64, v: n.v, vib: None });
    }
    let violin = render_violin(&vnotes, len, seed);

    // harmony guitar: intro/outro lead an octave below, verse fills, later-
    // chorus arpeggios.
    let mut hr = rng_for(seed, "hg");
    let mut hg_out = vec![0.0f32; len];

    let inst_g: Vec<(f64, f64, i32, f64)> = if break_lead == BreakLead::Violin {
        Vec::new()
    } else {
        prepared
            .comp
            .inst
            .iter()
            .map(|n| {
                let mut m = n.midi;
                while m < 55 {
                    m += 12;
                }
                while m > 76 {
                    m -= 12;
                }
                (tl.to_time(n.beat), tl.to_time(n.beat + n.dur), m, 0.55)
            })
            .collect()
    };
    let fl_g = fills_for(form, tl, lead, 59, 79, |s| s.kind == SectionKind::Verse, song, seed + 1);

    for &(t0, t1, m, v) in &inst_g {
        pluck_hg(&mut hg_out, t0, t1, m, v, &mut hr);
    }
    for n in &fl_g {
        pluck_hg(&mut hg_out, n.t0, n.t1, n.m, n.v, &mut hr);
    }

    for sg in &tl.segs {
        let sec = &form.sections[sg.sec];
        if !sec.is_repeat_lift() {
            continue;
        }
        let pcs = form.chord(sg.chord).tones;
        let mut tones: Vec<i32> = Vec::new();
        let mut m = 64i32;
        while m <= 83 && tones.len() < 3 {
            if pcs.contains(Pc::new(m)) {
                tones.push(m);
            }
            m += 1;
        }
        let pat = [0usize, 1, 2, 1];
        let sub = form.sub();
        let mut b = sg.b0;
        let mut k = 0usize;
        while b < sg.b1 - 1e-6 {
            let m = tones[pat[k % 4] % tones.len()];
            let t = tl.to_time(b) + (hr.next() - 0.5) * 0.008;
            let f = mtof(m as f64);
            let o = PluckOpts {
                amp: 0.32,
                bright: Some(0.65),
                damp: Some(0.08),
                t60: 3.0,
                pick: Some(0.2),
                noise: Some(0.05),
                detune: None,
                rel: 0.15,
                rel_t: 0.2,
                glide: 0.0,
                atk_noise: 0.0,
            };
            let start = sfcore::js::round(t * SR_F) as i64;
            let l = sfcore::js::round(1.4 * SR_F) as i64;
            pluck(&mut hg_out, start, f, l, &o, &mut hr);
            b += 1.0 / sub as f64;
            k += 1;
        }
    }

    (violin, hg_out)
}

/// One `pluck(hgOut,Math.round(t0*SR),mtof(m),Math.round((t1-t0+0.4)*SR),{...})`
/// call shared by `instG` and `flG` (engine.js ~938-940).
fn pluck_hg(out: &mut [f32], t0: f64, t1: f64, m: i32, v: f64, hr: &mut sfcore::rng::Rng) {
    let f = mtof(m as f64);
    let start = sfcore::js::round(t0 * SR_F) as i64;
    let l = sfcore::js::round((t1 - t0 + 0.4) * SR_F) as i64;
    let o = PluckOpts {
        amp: v,
        bright: Some(0.7),
        damp: Some(0.08),
        t60: 5.0 * pow(110.0 / f, 0.4),
        pick: Some(0.12),
        noise: Some(0.05),
        detune: None,
        rel: 0.08,
        rel_t: 0.1,
        glide: 0.0,
        atk_noise: 0.0,
    };
    pluck(out, start, f, l, &o, hr);
}

/// `for(const k in BODY_OF){...}` (engine.js ~945-949): if `key`'s raw track
/// is a mono, non-silent buffer, convolve it with that instrument's measured
/// body impulse response and replace it with the stereo result. Returns
/// `None` when the track has no body (not one of guitar/hg/harp/violin) or
/// is silent (every 64th sample is exactly zero, as JS's `nz` check reads),
/// in which case the caller keeps the original mono track.
///
/// Deviation from JS: JS divides the body IR by `bk[2]` in f64 and keeps a
/// plain f64 array (`d[0].map(v=>v/bk[2])`) for `convStereo`. This crate's
/// `dsp::fft::conv_stereo` (owned by the `dsp` crate, out of this step's
/// scope) only accepts `&[f32]` impulse responses, so the divided IR is
/// rounded to f32 here before the FFT convolution (itself done in f64)
/// runs. The rounding is one f32 ULP on the impulse response tail; report
/// the resulting track error against measurement, not assumed zero.
pub fn apply_body(key: &str, seed: u32, len: usize, x: &[f32]) -> Option<[Vec<f32>; 2]> {
    let (body, seed_off, scale) = dsp::mix::body_of(key)?;
    let mut nz = false;
    let mut i = 0usize;
    while i < x.len() {
        if x[i] != 0.0 {
            nz = true;
            break;
        }
        i += 64;
    }
    if !nz {
        return None;
    }
    let body_seed = seed.wrapping_add(seed_off as u32);
    let d = dsp::body::body_ir_data(body, body_seed);
    let h_l: Vec<f32> = d[0].iter().map(|v| (*v as f64 / scale) as f32).collect();
    let h_r: Vec<f32> = d[1].iter().map(|v| (*v as f64 / scale) as f32).collect();
    let (y_l, y_r) = dsp::fft::conv_stereo(x, &h_l, &h_r, len);
    Some([y_l, y_r])
}

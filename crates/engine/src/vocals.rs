//! renderSong's four vocal tracks: lead, harmony, doubles, choir
//! (engine.js ~870-919, right after `prepare` in `renderSong`).
//!
//! Every function returns the track's raw channel buffers exactly as
//! `renderSong` stores them into `tracks[key]` (one `Float32Array` for a
//! mono track, two for a stereo one), so a caller can feed the result
//! straight to `dsp::mix::Render::set_track`.

use compose::form::{Form, Sec};
use compose::melody::LeadNote;
use compose::prepare::{harmony_line, vocal_notes, Prepared};
use compose::song::Song;
use compose::voices::{voice_params, Voice, VoiceParams};
use dsp::pan::add_pan;
use rayon::prelude::*;
use sfcore::js::clamp;
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use voice::controls::{VoiceNote, VoiceOpts};
use voice::synth::render_voice;

use arrange::choir::choir_voicings;

/// The section a lead note belongs to (`n.sec` in JS: the JS note holds a
/// reference to its section object directly; here a `LeadNote` only keeps
/// `line_idx`, so the section is looked up through `Form::lines`).
fn sec_of<'a>(form: &'a Form, n: &LeadNote) -> &'a Sec {
    &form.sections[form.lines[n.line_idx].sec]
}

/// `renderSong`'s lead-vocal track (engine.js ~883-884):
/// `tracks.lead=[renderVoice(vocalNotes(lead,1),VP,len,{seed:seed^11,rng:rngFor(seed,'lead')})]`.
pub fn render_lead(p: &Prepared, seed: u32, len: usize, tuning: &Tuning) -> Vec<Vec<f32>> {
    let vp = voice_params(p.voice);
    let vn = vocal_notes(&p.comp.lead, 1.0);
    let notes: Vec<VoiceNote> = vn.iter().map(VoiceNote::from).collect();
    let mut opts = VoiceOpts { seed: Some(seed ^ 11), rng: Some(rng_for(seed, "lead")), ..Default::default() };
    let audio = render_voice(&notes, &vp, len, &mut opts, tuning);
    vec![audio]
}

/// `renderSong`'s harmony-vocal track (engine.js ~886-892).
pub fn render_harmony(p: &Prepared, song: &Song, seed: u32, len: usize, tuning: &Tuning) -> Vec<Vec<f32>> {
    let up = p.voice != Voice::Soprano;
    let cl: Vec<LeadNote> = p.comp.lead.iter().filter(|n| n.lift).cloned().collect();
    let hl = harmony_line(&cl, &p.form, &p.timeline, song, p.tonic, up);

    let med: i32 = if hl.is_empty() {
        60
    } else {
        let mut ms: Vec<i32> = hl.iter().map(|n| n.midi).collect();
        ms.sort_unstable();
        ms[ms.len() >> 1]
    };

    // `for(const k of ['baritone','tenor','alto','soprano']){if(k===P0.voice)continue; ...}`:
    // strict less-than keeps the first (lowest-index) tie, matching JS's
    // fixed iteration order over the same list.
    let mut hv = Voice::Tenor;
    let mut bd = 1e9f64;
    for &k in &[Voice::Baritone, Voice::Tenor, Voice::Alto, Voice::Soprano] {
        if k == p.voice {
            continue;
        }
        let kp = voice_params(k);
        let c = (kp.lo + kp.hi) as f64 / 2.0;
        let d = (c - med as f64).abs();
        if d < bd {
            bd = d;
            hv = k;
        }
    }

    let hn: Vec<VoiceNote> = vocal_notes(&hl, 0.9)
        .iter()
        .map(|n| {
            let mut v = VoiceNote::from(n);
            v.t0 += 0.008;
            v.t1 += 0.008;
            v
        })
        .collect();

    if hn.is_empty() {
        return vec![vec![0.0f32; len]];
    }
    let vp = voice_params(hv);
    let mut opts = VoiceOpts {
        seed: Some(seed ^ 23),
        rng: Some(rng_for(seed, "harm")),
        vib_scale: Some(0.8),
        breath_scale: Some(1.2),
        ..Default::default()
    };
    vec![render_voice(&hn, &vp, len, &mut opts, tuning)]
}

/// `renderSong`'s doubled-melody track (engine.js ~894-900).
pub fn render_doubles(p: &Prepared, seed: u32, len: usize, tuning: &Tuning) -> Vec<Vec<f32>> {
    let vp = voice_params(p.voice);
    let dl: Vec<LeadNote> = p
        .comp
        .lead
        .iter()
        .filter(|n| n.lift && sec_of(&p.form, n).lift_idx > 0)
        .cloned()
        .collect();

    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];

    if !dl.is_empty() {
        // JS: `for(const [k,pan,off,det] of [[0,-.6,.013,.07],[1,.6,.021,-.06]])`.
        for &(k, pan, off, det) in &[(0u32, -0.6f64, 0.013f64, 0.07f64), (1u32, 0.6, 0.021, -0.06)] {
            let mut pd = vp;
            pd.fs = vp.fs * if k != 0 { 1.03 } else { 0.97 };
            pd.breath = vp.breath + 0.05;

            let dn: Vec<VoiceNote> = vocal_notes(&dl, 0.8)
                .iter()
                .map(|n| {
                    let mut v = VoiceNote::from(n);
                    v.t0 += off;
                    v.t1 += off;
                    v
                })
                .collect();

            let mut opts = VoiceOpts {
                seed: Some(seed ^ (31 + k)),
                rng: Some(rng_for(seed, &format!("dbl{k}"))),
                detune: Some(det),
                vib_scale: Some(0.7),
                rate_scale: Some(if k != 0 { 1.07 } else { 0.94 }),
                no_breath: true,
                ..Default::default()
            };
            let v = render_voice(&dn, &pd, len, &mut opts, tuning);
            add_pan(&mut l, &mut r, 0, &v, pan, 1.0);
        }
    }
    vec![l, r]
}

/// One choir singer's fully-resolved render job: everything the shared
/// `'choirv'` rng stream determines for that singer (notes, voice params,
/// opts including the singer's own independent `rng`) plus its fixed pan.
/// Building this list is the only part of the choir that must run
/// sequentially (it is the only place that touches the shared stream);
/// `render_voice`-ing each job is independent of every other job (each
/// carries its own `rngFor(seed,'ch{pp}{d}')` stream) and safe to run in
/// parallel, as long as the results are summed back in this list's order.
pub struct ChoirJob {
    notes: Vec<VoiceNote>,
    params: VoiceParams,
    opts: VoiceOpts,
    pan: f64,
}

/// The sequential half of `renderSong`'s backing-choir track (engine.js
/// ~902-920): draws every singer's parameters and every note's timing from
/// the single shared `rngFor(seed,'choirv')` stream, in JS's exact order —
/// per singer, six draws (tune, late, vibScale, rate, fs-scale, breath) up
/// front, then three draws per voiced segment (a t0 jitter, a t1
/// jitter/tail draw, an amp draw), then one draw for the singer's F1 scale
/// and one for `rdScale` (both skipped, like JS's early `continue`, when
/// the singer ends up with no voiced segments).
pub fn choir_plan(p: &Prepared, seed: u32, tuning: &Tuning) -> Vec<ChoirJob> {
    let filt = |s: &Sec| (s.lift && s.lift_idx > 0) || s.type_ == "bridge" || s.type_ == "outro";
    let vs = choir_voicings(&p.form, &p.timeline, filt);

    let presets = [Voice::Bass, Voice::Tenor, Voice::Alto, Voice::Soprano];
    let pans = [-0.5f64, -0.2, 0.25, 0.55];
    let mut cr = rng_for(seed, "choirv");
    let mut jobs: Vec<ChoirJob> = Vec::new();

    for pp in 0..4usize {
        for d in 0..tuning.choir_n {
            let tune = (cr.next() - 0.5) * 0.22;
            let late = 0.012 + cr.next() * 0.035;
            let vib_scale = 0.55 + cr.next() * 0.45;
            let rate = 0.85 + cr.next() * 0.3;
            let fsx = 0.95 + cr.next() * 0.1;
            let br = 0.04 + cr.next() * 0.08;

            let mut notes: Vec<VoiceNote> = Vec::new();
            for cv in &vs {
                let sg = &p.timeline.segs[cv.seg_idx];
                let sec = &p.form.sections[sg.sec];
                let nu = vec![tuning.choir_vowel.to_string()];

                let t0 = p.timeline.to_time(sg.b0) + late + (cr.next() - 0.5) * 0.03;
                // JS: `sg.b1===sec.startBar*form.mi.bpb+sec.nBars*form.mi.bpb`
                // (this segment runs to the section's very last beat).
                let sec_end = (sec.start_bar as i32 * p.form.mi.bpb + sec.n_bars as i32 * p.form.mi.bpb) as f64;
                let is_last = sg.b1 == sec_end;
                let t1 = p.timeline.to_time(sg.b1)
                    - if is_last { 0.1 + cr.next() * 0.08 } else { 0.01 + cr.next() * 0.02 };

                let prev_t1 = notes.last().map(|n: &VoiceNote| n.t1);
                let amp = (if sec.type_ == "bridge" { 0.65 } else { 0.8 }) * (0.88 + cr.next() * 0.2);
                let phrase_start = match prev_t1 {
                    None => true,
                    Some(pt1) => t0 - pt1 > 0.1,
                };

                notes.push(VoiceNote {
                    t0,
                    t1,
                    midi: cv.v[pp],
                    ph: None,
                    nu: Some(nu),
                    amp,
                    phrase_start,
                    phrase_end: false,
                    grace: None,
                    stress: false,
                });
            }
            let n = notes.len();
            for i in 0..n {
                let ends_phrase = if i + 1 < n { notes[i + 1].t0 - notes[i].t1 > 0.1 } else { true };
                if ends_phrase {
                    notes[i].phrase_end = true;
                }
            }
            if notes.is_empty() {
                continue;
            }

            let b = voice_params(presets[pp]);
            let mut pc = b;
            pc.breath = b.breath + br;
            pc.fs = b.fs * fsx;
            pc.f1s = b.f1s * (0.97 + cr.next() * 0.06);
            pc.jitter = b.jitter * 1.6;
            pc.shimmer = b.shimmer * 1.4;

            let opts = VoiceOpts {
                seed: Some(seed ^ (101 + pp as u32 * 7 + d as u32)),
                rng: Some(rng_for(seed, &format!("ch{pp}{d}"))),
                rd_scale: Some(1.1 + cr.next() * 0.15),
                hf_gain: Some(0.0),
                n_high: Some(tuning.choir_nhigh),
                av_tau: Some(0.05),
                vib_scale: Some(vib_scale),
                rate_scale: Some(rate),
                detune: Some(tune),
                no_scoop: true,
                no_breath: true,
                glide: Some(0.05),
                ..Default::default()
            };
            let pan = pans[pp] + (d as f64 - (tuning.choir_n as f64 - 1.0) / 2.0) * 0.35;
            jobs.push(ChoirJob { notes, params: pc, opts, pan: clamp(pan, -0.9, 0.9) });
        }
    }
    jobs
}

/// `renderSong`'s backing-choir track (engine.js ~902-920), sequential
/// path: plans every singer (the only part that must run in order, see
/// `choir_plan`), then renders and sums them one at a time in that order.
pub fn render_choir(p: &Prepared, seed: u32, len: usize, tuning: &Tuning) -> Vec<Vec<f32>> {
    let mut jobs = choir_plan(p, seed, tuning);
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    for job in jobs.iter_mut() {
        let v = render_voice(&job.notes, &job.params, len, &mut job.opts, tuning);
        add_pan(&mut l, &mut r, 0, &v, job.pan, 1.0);
    }
    vec![l, r]
}

/// Same as `render_choir`, but renders every singer's `render_voice` call in
/// parallel (each draws only from its own independent `rngFor(seed,
/// 'ch{pp}{d}')` stream, so this is safe) and then sums the results back
/// into the stereo buffer sequentially, in the original singer order —
/// float addition order changes the result, so the sum itself is never
/// parallelized.
pub fn render_choir_threaded(p: &Prepared, seed: u32, len: usize, tuning: &Tuning) -> Vec<Vec<f32>> {
    let mut jobs = choir_plan(p, seed, tuning);
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];

    // Render and sum in fixed-size groups instead of collecting every
    // singer's full-length buffer (up to 4 presets * tuning.choir_n, e.g.
    // 12 at the default 3) before summing: with 12 singers at the demo's
    // length that collect held ~390 MB of mono f32 alive at once, on top of
    // whatever the other threaded tracks (guitar/bass/drums/...) were doing
    // in parallel. `CHUNK` bounds that to at most `CHUNK` buffers regardless
    // of choir_n, trading a little of the singer-level parallelism for
    // memory. Chunking by original order and summing each chunk in order
    // before starting the next is exactly the same arithmetic as summing
    // one big `audios` vector in order, so the result is unchanged.
    const CHUNK: usize = 4;
    for group in jobs.chunks_mut(CHUNK) {
        let audios: Vec<Vec<f32>> =
            group.par_iter_mut().map(|job| render_voice(&job.notes, &job.params, len, &mut job.opts, tuning)).collect();
        for (job, v) in group.iter().zip(audios.iter()) {
            add_pan(&mut l, &mut r, 0, v, job.pan, 1.0);
        }
    }
    vec![l, r]
}

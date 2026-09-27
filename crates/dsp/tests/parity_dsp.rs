//! Parity tests against tests/parity/dsp.js reference dumps in ref/parity/dsp.
//! Run `tests/parity/gen.sh` first. Each test reports max absolute error divided
//! by the reference peak. Target below 1e-6; the math functions in sfcore::js
//! are still up to 1 ulp off V8 on a small fraction of inputs, and recurrences
//! (reverb, violin, the pluck/body loops) can amplify that, so small nonzero
//! errors are expected and reported rather than chased.

use dsp::body::{body_ir_data, Body};
use dsp::dynamics::{compress, stereo_compress};
use dsp::fft::conv_stereo;
use dsp::filter::{bq, run_bq, FilterType};
use dsp::mix::{mix_song, Render, TRACKS};
use dsp::pluck::{ks_pluck, pluck, PluckOpts};
use dsp::reverb::fdn_reverb;
use dsp::violin::{render_violin, ViolinNote};
use serde_json::Value;
use sfcore::rng::rng_for;
use sfcore::{js, SR, SR_F};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/dsp"))
}

fn read_bin(name: &str) -> Vec<f32> {
    let p = dir().join(format!("{name}.bin"));
    let bytes = std::fs::read(&p).unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn read_index() -> Value {
    let p = dir().join("index.json");
    let s = std::fs::read_to_string(&p).unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn case<'a>(idx: &'a Value, name: &str) -> &'a Value {
    idx["cases"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap()
}

/// max|a-b| / max|ref|, the metric every case reports.
fn err_metric(rust: &[f32], reference: &[f32]) -> f64 {
    assert_eq!(rust.len(), reference.len(), "length mismatch");
    let mut peak = 0.0f64;
    let mut maxerr = 0.0f64;
    for i in 0..rust.len() {
        let r = reference[i] as f64;
        let a = (r).abs();
        if a > peak {
            peak = a;
        }
        let e = (rust[i] as f64 - r).abs();
        if e > maxerr {
            maxerr = e;
        }
    }
    if peak == 0.0 {
        maxerr
    } else {
        maxerr / peak
    }
}

fn parse_ft(s: &str) -> FilterType {
    match s {
        "lp" => FilterType::Lp,
        "hp" => FilterType::Hp,
        "bp" => FilterType::Bp,
        "hs" => FilterType::Hs,
        "ls" => FilterType::Ls,
        "pk" => FilterType::Pk,
        _ => panic!("unknown filter type {s}"),
    }
}

#[test]
fn run_bq_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "run_bq");
    let input_len = c["input_len"].as_u64().unwrap() as usize;
    let all = read_bin("run_bq");
    let input = &all[0..input_len];
    let cases = c["cases"].as_array().unwrap();
    let mut worst = 0.0f64;
    for (i, cc) in cases.iter().enumerate() {
        let ty = parse_ft(cc["type"].as_str().unwrap());
        let f = cc["freq"].as_f64().unwrap();
        let q = cc["q"].as_f64().unwrap();
        let g = cc["gainDb"].as_f64().unwrap();
        let n = cc["n"].as_u64().unwrap() as usize;
        let co = bq(ty, f, q, g);
        let mut x: Vec<f32> = input.to_vec();
        run_bq(&mut x, &co);
        let start = input_len + i * n;
        let reference = &all[start..start + n];
        let e = err_metric(&x, reference);
        worst = js::max(worst, e);
    }
    println!("run_bq: worst relative max-abs-error over {} cases = {:.3e}", cases.len(), worst);
    // Measured: exact (worst=0.000e0). f32r stores match JS Float32Array bit for bit.
    assert_eq!(worst, 0.0, "run_bq worst error {worst:.3e}, expected exact");
}

#[test]
fn pluck_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "pluck");
    let len = c["len"].as_u64().unwrap() as usize;
    let opt_sets = c["opt_sets"].as_array().unwrap();
    let all = read_bin("pluck");
    let cases = c["cases"].as_array().unwrap();
    let mut worst = 0.0f64;
    for (i, cc) in cases.iter().enumerate() {
        let f = cc["f"].as_f64().unwrap();
        let opt_idx = cc["optIdx"].as_u64().unwrap() as usize;
        let seed = cc["seed"].as_u64().unwrap();
        let o = &opt_sets[opt_idx];
        let opts = PluckOpts {
            amp: o["amp"].as_f64().unwrap_or(1.0),
            t60: o["t60"].as_f64().unwrap_or(1.0),
            pick: o.get("pick").and_then(|v| v.as_f64()),
            bright: o.get("bright").and_then(|v| v.as_f64()),
            damp: o.get("damp").and_then(|v| v.as_f64()),
            noise: o.get("noise").and_then(|v| v.as_f64()),
            detune: o.get("detune").and_then(|v| v.as_f64()),
            rel: o.get("rel").and_then(|v| v.as_f64()).unwrap_or(0.0),
            rel_t: o.get("relT").and_then(|v| v.as_f64()).unwrap_or(0.0),
            glide: o.get("glide").and_then(|v| v.as_f64()).unwrap_or(0.0),
            atk_noise: o.get("atkNoise").and_then(|v| v.as_f64()).unwrap_or(0.0),
        };
        let mut r = rng_for(seed as u32, &format!("pluck{seed}"));
        let mut out = vec![0.0f32; len];
        pluck(&mut out, 0, f, len as i64, &opts, &mut r);
        let start = i * len;
        let reference = &all[start..start + len];
        let e = err_metric(&out, reference);
        worst = js::max(worst, e);
    }
    println!("pluck: worst relative max-abs-error over {} cases = {:.3e}", cases.len(), worst);
    // Measured: exact (worst=0.000e0).
    assert_eq!(worst, 0.0, "pluck worst error {worst:.3e}, expected exact");
}

#[test]
fn ks_pluck_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "ks_pluck");
    let len = c["len"].as_u64().unwrap() as usize;
    let f = c["f"].as_f64().unwrap();
    let seed = c["seed"].as_u64().unwrap();
    let tag = c["tag"].as_str().unwrap();
    let o = &c["opts"];
    let opts = PluckOpts { amp: o["amp"].as_f64().unwrap(), t60: o["t60"].as_f64().unwrap(), ..Default::default() };
    let mut r = rng_for(seed as u32, tag);
    let mut out = vec![0.0f32; len];
    ks_pluck(&mut out, 0, f, len as i64, &opts, &mut r);
    let reference = read_bin("ks_pluck");
    let e = err_metric(&out, &reference);
    println!("ks_pluck: relative max-abs-error = {e:.3e}");
    // Measured: exact (0.000e0).
    assert_eq!(e, 0.0, "ks_pluck error {e:.3e}, expected exact");
}

#[test]
fn body_ir_data_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "body_ir");
    let all = read_bin("body_ir");
    let cases = c["cases"].as_array().unwrap();
    let mut off = 0usize;
    let mut worst = 0.0f64;
    for cc in cases {
        let name = cc["name"].as_str().unwrap();
        let seed = cc["seed"].as_u64().unwrap();
        let n = cc["n"].as_u64().unwrap() as usize;
        let ch = body_ir_data(Body::from_name(name), seed as u32);
        let ref_l = &all[off..off + n];
        off += n;
        let ref_r = &all[off..off + n];
        off += n;
        let el = err_metric(&ch[0], ref_l);
        let er = err_metric(&ch[1], ref_r);
        worst = js::max(worst, js::max(el, er));
    }
    println!("body_ir_data: worst relative max-abs-error over {} cases = {:.3e}", cases.len(), worst);
    // Measured: exact (worst=0.000e0).
    assert_eq!(worst, 0.0, "body_ir_data worst error {worst:.3e}, expected exact");
}

#[test]
fn conv_stereo_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "conv_stereo");
    let x_len = c["xLen"].as_u64().unwrap() as usize;
    let h_len = c["hLen"].as_u64().unwrap() as usize;
    let out_len = c["outLen"].as_u64().unwrap() as usize;
    let all = read_bin("conv_stereo");
    let mut o = 0usize;
    let x = &all[o..o + x_len];
    o += x_len;
    let h_l = &all[o..o + h_len];
    o += h_len;
    let h_r = &all[o..o + h_len];
    o += h_len;
    let y_l_ref = &all[o..o + out_len];
    o += out_len;
    let y_r_ref = &all[o..o + out_len];

    let (y_l, y_r) = conv_stereo(x, h_l, h_r, out_len);
    let el = err_metric(&y_l, y_l_ref);
    let er = err_metric(&y_r, y_r_ref);
    println!("conv_stereo: relative max-abs-error L={el:.3e} R={er:.3e}");
    // Measured: exact (L=0.000e0, R=0.000e0).
    assert_eq!((el, er), (0.0, 0.0), "conv_stereo error L={el:.3e} R={er:.3e}, expected exact");
}

#[test]
fn compress_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "compress");
    let n = c["n"].as_u64().unwrap() as usize;
    let all = read_bin("compress");
    let input = &all[0..n];
    let reference = &all[n..2 * n];
    let mut x = input.to_vec();
    compress(&mut x, c["thrDb"].as_f64().unwrap(), c["ratio"].as_f64().unwrap(), c["atk"].as_f64().unwrap(), c["rel"].as_f64().unwrap(), c["knee"].as_f64());
    let e = err_metric(&x, reference);
    println!("compress: relative max-abs-error = {e:.3e}");
    // Measured: exact (0.000e0).
    assert_eq!(e, 0.0, "compress error {e:.3e}, expected exact");
}

#[test]
fn stereo_compress_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "stereo_compress");
    let n = c["n"].as_u64().unwrap() as usize;
    let all = read_bin("stereo_compress");
    let in_l = all[0..n].to_vec();
    let in_r = all[n..2 * n].to_vec();
    let ref_l = &all[2 * n..3 * n];
    let ref_r = &all[3 * n..4 * n];
    let mut l = in_l;
    let mut r = in_r;
    stereo_compress(&mut l, &mut r, c["thrDb"].as_f64().unwrap(), c["ratio"].as_f64().unwrap(), c["atk"].as_f64().unwrap(), c["rel"].as_f64().unwrap());
    let el = err_metric(&l, ref_l);
    let er = err_metric(&r, ref_r);
    println!("stereo_compress: relative max-abs-error L={el:.3e} R={er:.3e}");
    // Measured: exact (L=0.000e0, R=0.000e0).
    assert_eq!((el, er), (0.0, 0.0), "stereo_compress error L={el:.3e} R={er:.3e}, expected exact");
}

#[test]
fn fdn_reverb_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "fdn_reverb");
    let n = c["n"].as_u64().unwrap() as usize;
    let all = read_bin("fdn_reverb");
    let in_l = &all[0..n];
    let in_r = &all[n..2 * n];
    let ref_out_l = &all[2 * n..3 * n];
    let ref_out_r = &all[3 * n..4 * n];
    let mut out_l = vec![0.0f32; n];
    let mut out_r = vec![0.0f32; n];
    fdn_reverb(in_l, in_r, &mut out_l, &mut out_r, c["wet"].as_f64().unwrap(), c["seed"].as_i64().unwrap());
    let el = err_metric(&out_l, ref_out_l);
    let er = err_metric(&out_r, ref_out_r);
    println!("fdn_reverb: relative max-abs-error L={el:.3e} R={er:.3e} (a recurrence: small input errors amplify)");
    // Measured: exact (L=0.000e0, R=0.000e0) now the FDN state buffers are f32,
    // matching the JS Float32Array store points exactly.
    assert_eq!((el, er), (0.0, 0.0), "fdn_reverb error L={el:.3e} R={er:.3e}, expected exact");
}

#[test]
fn render_violin_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "render_violin");
    let len = c["len"].as_u64().unwrap() as usize;
    let seed = c["seed"].as_u64().unwrap() as u32;
    let notes: Vec<ViolinNote> = c["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| ViolinNote {
            t0: n["t0"].as_f64().unwrap(),
            t1: n["t1"].as_f64().unwrap(),
            m: n["m"].as_f64().unwrap(),
            v: n["v"].as_f64().unwrap(),
            vib: n.get("vib").and_then(|v| v.as_f64()),
        })
        .collect();
    let out = render_violin(&notes, len, seed);
    let reference = read_bin("render_violin");
    let e = err_metric(&out, &reference);
    println!("render_violin: relative max-abs-error = {e:.3e} (a waveguide recurrence: small errors amplify)");
    // Measured: exact (0.000e0) after fixing del() to clamp only the input
    // delay (not the post-wrap index), which had been corrupting in-range reads.
    assert_eq!(e, 0.0, "render_violin error {e:.3e}, expected exact");
}

/// Wider `renderViolin` coverage: a note >=3s (the split path and its rng
/// draw), several gaps >=0.06s (multiple phrases), a phrase cut off by
/// `len`, and a second seed.
#[test]
fn render_violin_ex_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "render_violin_ex");
    let len = c["len"].as_u64().unwrap() as usize;
    let seed = c["seed"].as_u64().unwrap() as u32;
    let notes: Vec<ViolinNote> = c["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| ViolinNote {
            t0: n["t0"].as_f64().unwrap(),
            t1: n["t1"].as_f64().unwrap(),
            m: n["m"].as_f64().unwrap(),
            v: n["v"].as_f64().unwrap(),
            vib: n.get("vib").and_then(|v| v.as_f64()),
        })
        .collect();
    let out = render_violin(&notes, len, seed);
    let reference = read_bin("render_violin_ex");
    let e = err_metric(&out, &reference);
    println!("render_violin_ex: relative max-abs-error = {e:.3e}");
    // Measured: exact (0.000e0).
    assert_eq!(e, 0.0, "render_violin_ex error {e:.3e}, expected exact");
}

/// `burstTrack`/`buildRender` from tests/parity/dsp.js: deterministic noise
/// bursts, one rng stream per channel (so a stereo track's channels are
/// identical, matching the JS test harness).
fn burst_track(seed: u32, chans: usize, len: usize) -> Vec<Vec<f32>> {
    let tag = format!("mixburst{chans}");
    (0..chans)
        .map(|_| {
            let mut r = rng_for(seed, &tag);
            let mut a = vec![0.0f32; len];
            for &start in &[0.2f64, 1.6, 3.0] {
                let s0 = js::round(start * SR_F) as usize;
                let n = js::round(0.3 * SR_F) as usize;
                for i in 0..n {
                    if s0 + i >= len {
                        break;
                    }
                    let v = (r.next() * 2.0 - 1.0) * 0.3 * js::exp(-(i as f64) / (0.05 * SR_F));
                    a[s0 + i] = js::f32r(v) as f32;
                }
            }
            a
        })
        .collect()
}

fn build_render(len: usize) -> Render {
    let mut render = Render::new(len);
    for t in TRACKS.iter() {
        let chans = if t.key == "lead" {
            1
        } else if matches!(t.key, "doubles" | "harmony" | "choir" | "hg" | "harp" | "violin") {
            2
        } else {
            1
        };
        let seed = 200 + t.key.len() as u32;
        render.set_track(t.key, burst_track(seed, chans, len));
    }
    render.set_track("drums", burst_track(999, 2, len));
    render
}

#[test]
fn mix_song_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "mix_song");
    let len = c["len"].as_u64().unwrap() as usize;
    let seed = c["seed"].as_i64().unwrap();
    let all = read_bin("mix_song");
    let ref_all_l = &all[0..len];
    let ref_all_r = &all[len..2 * len];
    let ref_nh_l = &all[2 * len..3 * len];
    let ref_nh_r = &all[3 * len..4 * len];

    let mut r1 = build_render(len);
    let m1 = mix_song(&mut r1, |_| true, seed, None);
    let mut r2 = build_render(len);
    let m2 = mix_song(&mut r2, |t| t.key != "harp", seed, None);

    assert_eq!(m1.sample_rate, SR);

    let e_all_l = err_metric(&m1.l, ref_all_l);
    let e_all_r = err_metric(&m1.r, ref_all_r);
    let e_nh_l = err_metric(&m2.l, ref_nh_l);
    let e_nh_r = err_metric(&m2.r, ref_nh_r);
    println!("mix_song: relative max-abs-error all=({e_all_l:.3e},{e_all_r:.3e}) no_harp=({e_nh_l:.3e},{e_nh_r:.3e})");
    let worst = [e_all_l, e_all_r, e_nh_l, e_nh_r].iter().cloned().fold(0.0, js::max);
    // Measured: exact (all worst=0.000e0) now processTrack/mixSong keep every
    // intermediate in f32 and the reverb state is f32 too.
    assert_eq!(worst, 0.0, "mix_song worst error {worst:.3e}, expected exact");
}

/// `burstTrackLR`/`buildRenderEx` from tests/parity/dsp.js: per-channel rng
/// streams so a stereo track's L and R genuinely differ.
fn burst_track_lr(seed: u32, side: usize, len: usize) -> Vec<f32> {
    let tag = format!("mixburstlr{side}");
    let mut r = rng_for(seed, &tag);
    let mut a = vec![0.0f32; len];
    let starts: [f64; 3] = if side == 0 { [0.15, 1.2, 2.3] } else { [0.3, 1.5, 2.6] };
    for &start in &starts {
        let s0 = js::round(start * SR_F) as usize;
        let n = js::round(0.25 * SR_F) as usize;
        for i in 0..n {
            if s0 + i >= len {
                break;
            }
            let v = (r.next() * 2.0 - 1.0) * 0.3 * js::exp(-(i as f64) / (0.05 * SR_F));
            a[s0 + i] = js::f32r(v) as f32;
        }
    }
    a
}

/// Wider `mixSong` coverage: stereo tracks whose L and R genuinely differ,
/// an all-zero track (bass), a guitar track run through the exact
/// `bodyIRData`/`BODY_OF`/`convStereo` steps `renderSong` applies
/// (engine.js ~line 948), and two mixes of the *same* `Render` with
/// different enabled sets, which must reuse `processTrack`'s cache rather
/// than recomputing (and must not copy the cached buffers).
#[test]
fn mix_song_ex_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let idx = read_index();
    let c = case(&idx, "mix_song_ex");
    let len = c["len"].as_u64().unwrap() as usize;
    let seed = c["seed"].as_i64().unwrap();
    let all = read_bin("mix_song_ex");
    let ref_all_l = &all[0..len];
    let ref_all_r = &all[len..2 * len];
    let ref_nb_l = &all[2 * len..3 * len];
    let ref_nb_r = &all[3 * len..4 * len];

    let mut render = Render::new(len);
    for t in TRACKS.iter() {
        let stereo = matches!(t.key, "doubles" | "harmony" | "choir" | "hg" | "harp" | "violin");
        let seed0 = 300 + t.key.len() as u32;
        if t.key == "bass" {
            render.set_track(t.key, vec![vec![0.0f32; len]]); // all-zero track
        } else if stereo {
            render.set_track(t.key, vec![burst_track_lr(seed0, 0, len), burst_track_lr(seed0, 1, len)]);
        } else {
            render.set_track(t.key, vec![burst_track_lr(seed0, 0, len)]);
        }
    }
    // guitar: run through the exact body-convolution step renderSong applies.
    {
        let seed2 = 555u32;
        let (body, off, scale) = dsp::mix::body_of("guitar").unwrap();
        let x = burst_track_lr(300 + "guitar".len() as u32, 0, len);
        let d = dsp::body::body_ir_data(body, (seed2 as f64 + off) as u32);
        let hl: Vec<f32> = d[0].iter().map(|&v| (v as f64 / scale) as f32).collect();
        let hr: Vec<f32> = d[1].iter().map(|&v| (v as f64 / scale) as f32).collect();
        let (yl, yr) = conv_stereo(&x, &hl, &hr, len);
        render.set_track("guitar", vec![yl, yr]);
    }

    let m1 = mix_song(&mut render, |_| true, seed, None);
    let e_all_l = err_metric(&m1.l, ref_all_l);
    let e_all_r = err_metric(&m1.r, ref_all_r);

    let m2 = mix_song(&mut render, |t| t.key != "bass", seed, None); // same render: cache reuse
    let e_nb_l = err_metric(&m2.l, ref_nb_l);
    let e_nb_r = err_metric(&m2.r, ref_nb_r);

    println!("mix_song_ex: relative max-abs-error all=({e_all_l:.3e},{e_all_r:.3e}) no_bass_cached=({e_nb_l:.3e},{e_nb_r:.3e})");
    let worst = [e_all_l, e_all_r, e_nb_l, e_nb_r].iter().cloned().fold(0.0, js::max);
    // Measured: exact (0.000e0).
    assert_eq!(worst, 0.0, "mix_song_ex worst error {worst:.3e}, expected exact");
}

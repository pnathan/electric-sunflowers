//! lfTable, synthVoice, renderVoice. Ports engine.js lines ~1081-1099 (lfTable)
//! and ~539-596 (synthVoice, renderVoice).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use sfcore::js::{self, clamp, f32r};
use sfcore::tuning::Tuning;
use sfcore::HOP;

use dsp::filter::{bq, BqCoeffs, FilterType};

use crate::params::VoiceParams;

use crate::controls::{or_falsy, resolve_rng, voice_controls, VoiceControls, VoiceNote, VoiceOpts};

/// `mtof(m)` from engine.js: MIDI-ish note number to frequency.
fn mtof(m: f64) -> f64 {
    440.0 * js::pow(2.0, (m - 69.0) / 12.0)
}

/// lfTable's cached result: `{D,G,te,tp}` in JS. `te`/`tp` are never read
/// back anywhere in engine.js (only `D` and `G` are destructured at the call
/// site); kept here anyway since they are part of the JS return value.
pub struct LfTable {
    pub d: Vec<f32>,
    pub g: Vec<f32>,
    #[allow(dead_code)]
    pub te: f64,
    #[allow(dead_code)]
    pub tp: f64,
}

type LfCache = Mutex<HashMap<String, Arc<LfTable>>>;
static LF_CACHE: OnceLock<LfCache> = OnceLock::new();

/// `lfTable(Rd,TL)`: Liljencrants-Fant glottal-flow-derivative table (Fant
/// 1995), keyed and cached by `Rd.toFixed(3)` as JS does (a `Map` keyed by
/// string). Returns the differentiated waveform `D` (RMS-normalised) and its
/// running integral `G` (0..1, the glottal-area-like curve), both length
/// `TL+1`.
pub fn lf_table(rd: f64, tl: usize) -> Arc<LfTable> {
    let key = format!("{:.3}", rd);
    let cache = LF_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = cache.lock().unwrap().get(&key) {
        return v.clone();
    }

    let ra = (-1.0 + 4.8 * rd) / 100.0;
    let rk = (22.4 + 11.8 * rd) / 100.0;
    let rg = rk / (4.0 * (0.11 * rd / (0.5 + 1.2 * rk) - ra));
    let tp = 1.0 / (2.0 * rg);
    let te = tp * (1.0 + rk);
    let ta = ra;
    let mut eps = 1.0 / ta;
    for _ in 0..40 {
        eps = (1.0 - js::exp(-eps * (1.0 - te))) / ta;
    }
    let wg = std::f64::consts::PI / tp;

    let build = |al: f64| -> Vec<f64> {
        let mut d = vec![0.0f64; tl + 1];
        let e0 = -1.0 / (js::exp(al * te) * js::sin(wg * te));
        for i in 0..=tl {
            let t = i as f64 / tl as f64;
            d[i] = if t <= te {
                e0 * js::exp(al * t) * js::sin(wg * t)
            } else {
                -(js::exp(-eps * (t - te)) - js::exp(-eps * (1.0 - te))) / (eps * ta)
            };
        }
        d
    };
    let area = |al: f64| -> f64 {
        let d = build(al);
        let mut s = 0.0;
        for i in 0..tl {
            s += d[i];
        }
        s
    };

    let mut lo = -10.0f64;
    let mut hi = 80.0f64;
    let mut alo = area(lo);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        let am = area(mid);
        if (am > 0.0) == (alo > 0.0) {
            lo = mid;
            alo = am;
        } else {
            hi = mid;
        }
    }
    let al = (lo + hi) / 2.0;

    let d = build(al);
    let mut g = vec![0.0f32; tl + 1];
    let mut s = 0.0f64;
    let mut gm = 1e-9f64;
    for i in 0..=tl {
        s += d[i];
        g[i] = f32r(s) as f32;
        if s > gm {
            gm = s;
        }
    }
    let mut e = 0.0f64;
    for i in 0..tl {
        e += d[i] * d[i];
    }
    let rms = (e / tl as f64).sqrt();
    let mut df = vec![0.0f32; tl + 1];
    for i in 0..=tl {
        df[i] = f32r(d[i] / rms) as f32;
        let gv = (g[i] as f64 / gm).max(0.0);
        g[i] = f32r(gv) as f32;
    }

    let res = Arc::new(LfTable { d: df, g, te, tp });
    cache.lock().unwrap().insert(key, res.clone());
    res
}

/// `synthVoice(ctl,P,len,opts)`: source-filter synthesis over the per-frame
/// control tracks from `voiceControls`. The parallel high-frequency branch
/// (`HP`/`PK`, engine.js's `if(false){...}` block) is unreachable in JS and
/// contributes nothing to the output (it only feeds `hq`, which is never
/// read, and its state variables `hx1/hx2/hy1/hy2/px1/px2/py1/py2` are
/// otherwise untouched); it is omitted here rather than ported dead.
pub fn synth_voice(ctl: &VoiceControls, p: &VoiceParams, len: usize, opts: &mut VoiceOpts, tuning: &Tuning) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    let n_f = ctl.av.len();
    let mut vbl1 = 0.0f64;
    let mut vbl2 = 0.0f64;

    const TL: usize = 2048;
    let rd = p.rd * or_falsy(opts.rd_scale, 1.0);
    let la = lf_table(js::round((rd + 0.35) * 40.0) / 40.0, TL);
    let lt = lf_table(js::round(js::max(0.6, rd - 0.28) * 40.0) / 40.0, TL);
    let da = &la.d;
    let dt = &lt.d;
    let ga_arr = &la.g;
    let gt_arr = &lt.g;

    let mut w_a = 0.0f64;
    let mut w_b = 0.0f64;
    let mut v_a = 0.0f64;
    let mut v_b = 0.0f64;

    // synthVoice's own local xorshift32, independent of opts.rng.
    let mut seed: u32 = opts.seed.unwrap_or(0);
    if seed == 0 {
        seed = 12345;
    }
    let mut rnd = move || -> f64 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed as f64 / 2147483648.0) - 1.0
    };

    let jr = opts.rng.as_mut().expect("synth_voice: opts.rng must be resolved (see resolve_rng)");
    let mut ph = 0.0f64;
    let mut jit = 0.0f64;
    let mut shim = 1.0f64;
    let mut tilt_y = 0.0f64;
    let mut tilt_y2 = 0.0f64;
    let mut nlp = 0.0f64;
    let nla = 1.0 - js::exp(-2.0 * std::f64::consts::PI * 2600.0 / sfcore::SR_F);
    let tilt_a = 1.0 - js::exp(-2.0 * std::f64::consts::PI * (p.tilt * tuning.tls * or_falsy(opts.tilt_scale, 1.0)) / sfcore::SR_F);

    let nr: usize = match opts.n_high {
        None => 9,
        Some(nh) => 5 + js::min(4.0, nh) as usize,
    };
    let mut ry1 = vec![0.0f64; nr];
    let mut ry2 = vec![0.0f64; nr];
    let mut r_a = vec![0.0f64; nr];
    let mut r_b = vec![0.0f64; nr];
    let mut r_c = vec![0.0f64; nr];

    let mut fx1 = 0.0f64;
    let mut fx2 = 0.0f64;
    let mut fy1 = 0.0f64;
    let mut fy2 = 0.0f64;
    let mut dcx = 0.0f64;
    let mut dcy = 0.0f64;
    // Always 0.0: see the doc comment above (the branch that would write it is dead).
    let hy1 = 0.0f64;

    let breath = p.breath * or_falsy(opts.breath_scale, 1.0);
    let sf = p.sf;
    let fs = p.fs;
    let f4 = (3350.0 - sf * 350.0) * fs;
    let f5 = (3950.0 - sf * 500.0) * fs;
    let b4 = 250.0 - sf * 110.0;
    let b5 = 300.0 - sf * 120.0;
    let hf: [f64; 4] = [5500.0 * fs, 6600.0 * fs, 7700.0 * fs, 8800.0 * fs];
    let hb: [f64; 4] = [420.0 + 0.05 * hf[0], 420.0 + 0.05 * hf[1], 420.0 + 0.05 * hf[2], 420.0 + 0.05 * hf[3]];
    let hs: BqCoeffs = bq(FilterType::Hs, 5200.0, 0.7, tuning.shg);
    let mut sx1 = 0.0f64;
    let mut sx2 = 0.0f64;
    let mut sy1 = 0.0f64;
    let mut sy2 = 0.0f64;

    let res = |k: usize, f: f64, b: f64, r_a: &mut [f64], r_b: &mut [f64], r_c: &mut [f64]| {
        let r = js::exp(-std::f64::consts::PI * b / sfcore::SR_F);
        r_c[k] = -r * r;
        r_b[k] = 2.0 * r * js::cos(2.0 * std::f64::consts::PI * js::min(f, sfcore::SR_F * 0.45) / sfcore::SR_F);
        r_a[k] = 1.0 - r_b[k] - r_c[k];
    };

    let hop_f = HOP as f64;
    let vf = &tuning.vf;
    let aspg = tuning.aspg;
    let frg = tuning.frg;

    let mut m = 0usize;
    while m < n_f.saturating_sub(1) {
        let s0 = m * HOP;
        if s0 >= len {
            break;
        }
        let av0 = ctl.av[m] as f64;
        let av1 = ctl.av[m + 1] as f64;
        let ah0 = ctl.ah[m] as f64;
        let ah1 = ctl.ah[m + 1] as f64;
        let af0 = ctl.af[m] as f64;
        let af1 = ctl.af[m + 1] as f64;
        let vb0 = ctl.vb[m] as f64;
        let vb1 = ctl.vb[m + 1] as f64;

        if av0 < 1e-5
            && av1 < 1e-5
            && ah0 < 1e-5
            && ah1 < 1e-5
            && af0 < 1e-5
            && af1 < 1e-5
            && vb0 < 1e-5
            && vb1 < 1e-5
            && vbl2.abs() < 1e-7
            && ry1[nr - 1].abs() < 1e-7
            && hy1.abs() < 1e-7
            && fy1.abs() < 1e-7
        {
            for v in ry1.iter_mut() {
                *v = 0.0;
            }
            for v in ry2.iter_mut() {
                *v = 0.0;
            }
            fy1 = 0.0;
            fy2 = 0.0;
            fx1 = 0.0;
            fx2 = 0.0;
            m += 1;
            continue;
        }

        let f0 = mtof(ctl.m[m] as f64);
        let f0b = mtof(ctl.m[m + 1] as f64);
        let nas = ctl.nas[m] as f64;
        let f1_local = js::max(ctl.f1[m] as f64, f0 * 1.06);

        v_a += jr.gauss() * 0.02;
        v_a *= 0.97;
        w_a = clamp(w_a + v_a * 0.01, -0.02, 0.02);
        v_b += jr.gauss() * 0.02;
        v_b *= 0.97;
        w_b = clamp(w_b + v_b * 0.01, -0.025, 0.025);

        res(0, f1_local, 60.0 + breath * 80.0 + nas * 50.0 + ctl.b1x[m] as f64 * vf.b1x, &mut r_a, &mut r_b, &mut r_c);
        res(1, ctl.f2[m] as f64 * (1.0 + w_a), 90.0 + nas * 170.0, &mut r_a, &mut r_b, &mut r_c);
        res(2, ctl.f3[m] as f64 * (1.0 + w_b), 130.0 + nas * 220.0, &mut r_a, &mut r_b, &mut r_c);
        res(3, f4, b4, &mut r_a, &mut r_b, &mut r_c);
        res(4, f5, b5, &mut r_a, &mut r_b, &mut r_c);
        for h in 5..nr {
            res(h, hf[h - 5], hb[h - 5], &mut r_a, &mut r_b, &mut r_c);
        }

        let ff_m = ctl.ff[m] as f64;
        let fbw_m = ctl.fbw[m] as f64;
        let w0 = 2.0 * std::f64::consts::PI * js::min(ff_m, sfcore::SR_F * 0.42) / sfcore::SR_F;
        let q = js::max(0.5, ff_m / fbw_m);
        let al = js::sin(w0) / (2.0 * q);
        let a0 = 1.0 + al;
        let fb0 = al / a0;
        let fb2 = -al / a0;
        let fa1 = -2.0 * js::cos(w0) / a0;
        let fa2 = (1.0 - al) / a0;

        let end = js::min(hop_f, (len - s0) as f64) as usize;
        let f_on = af0 > 1e-6 || af1 > 1e-6 || fy1.abs() > 1e-7 || fy2.abs() > 1e-7;
        if !f_on {
            fy1 = 0.0;
            fy2 = 0.0;
            fx1 = 0.0;
            fx2 = 0.0;
        }
        let v_on = vb0 > 1e-6 || vb1 > 1e-6 || vbl2.abs() > 1e-8;
        if !v_on {
            vbl1 = 0.0;
            vbl2 = 0.0;
        }

        for j in 0..end {
            let t = j as f64 / hop_f;
            let av = av0 + (av1 - av0) * t;
            let ah = ah0 + (ah1 - ah0) * t;
            let af = af0 + (af1 - af0) * t;
            let f = f0 + (f0b - f0) * t;
            ph += f / sfcore::SR_F * (1.0 + jit);
            // JS checks `if(ph>=1)` once per sample (a single subtraction),
            // never guards ph<0, and then indexes DA/DT/GA/GT at `ph*TL|0`
            // and `+1` with no bounds check: at extreme pitch (one hop's f
            // large enough that ph overshoots 1 by more than 1) or a large
            // negative jitter driving `1+jit` negative, ph can land outside
            // [0,1) and JS reads `undefined` (silently propagating NaN into
            // the sample) where Rust would index out of bounds and panic.
            // Deviation from JS: wrap ph fully (not just once) so it always
            // lands in [0,1), and clamp `ii` so `ii+1` stays in range. This
            // does not change any in-range case: the loop runs 0 or 1 times
            // exactly like the JS `if` whenever ph was already in range.
            while ph >= 1.0 {
                ph -= 1.0;
                jit = jr.gauss() * p.jitter;
                shim = 1.0 + jr.gauss() * p.shimmer;
            }
            while ph < 0.0 {
                ph += 1.0;
            }
            let xi = ph * TL as f64;
            let ii = (xi as usize).min(TL - 1);
            let frac = xi - ii as f64;
            let wt = clamp((av - 0.42) / 0.6, 0.0, 1.0);
            let d_a = da[ii] as f64 + (da[ii + 1] as f64 - da[ii] as f64) * frac;
            let d_t = dt[ii] as f64 + (dt[ii + 1] as f64 - dt[ii] as f64) * frac;
            let g_a = ga_arr[ii] as f64 + (ga_arr[ii + 1] as f64 - ga_arr[ii] as f64) * frac;
            let g_t = gt_arr[ii] as f64 + (gt_arr[ii + 1] as f64 - gt_arr[ii] as f64) * frac;
            let dg = (d_a + (d_t - d_a) * wt) * 0.176;
            let g = g_a + (g_t - g_a) * wt;
            tilt_y += tilt_a * (dg - tilt_y);
            tilt_y2 += tilt_a * (tilt_y - tilt_y2);
            let nr_ = rnd();
            nlp += nla * (nr_ - nlp);
            let n1 = nlp * 1.9;
            let vs = tilt_y2 * av * shim * 1.6;
            let mut x = vs + (nr_ * ah * 0.9 * aspg + n1 * breath * av * (0.18 + 0.9 * g)) * 0.55;

            for k in 0..nr {
                let y = r_a[k] * x + r_b[k] * ry1[k] + r_c[k] * ry2[k];
                ry2[k] = ry1[k];
                ry1[k] = y;
                x = y;
            }
            {
                let y = hs.b0 * x + hs.b1 * sx1 + hs.b2 * sx2 - hs.a1 * sy1 - hs.a2 * sy2;
                sx2 = sx1;
                sx1 = x;
                sy2 = sy1;
                sy1 = y;
                x = y;
            }
            if v_on {
                let vb = vb0 + (vb1 - vb0) * t;
                let a = 0.042;
                vbl1 += a * (tilt_y2 * shim * 1.6 * vb - vbl1);
                vbl2 += a * (vbl1 - vbl2);
            }
            let mut fy = 0.0f64;
            if f_on {
                let n2 = rnd() * af;
                fy = fb0 * n2 + fb2 * fx2 - fa1 * fy1 - fa2 * fy2;
                fx2 = fx1;
                fx1 = n2;
                fy2 = fy1;
                fy1 = fy;
            }
            let o = x + fy * frg + vbl2 * 2.2;
            let d = o - dcx + 0.995 * dcy;
            dcx = o;
            dcy = d;
            out[s0 + j] = f32r(d) as f32;
        }
        m += 1;
    }
    out
}

/// `renderVoice(notes,P,len,opts)`.
pub fn render_voice(notes: &[VoiceNote], p: &VoiceParams, len: usize, opts: &mut VoiceOpts, tuning: &Tuning) -> Vec<f32> {
    let n_f = (len as f64 / HOP as f64).ceil() as usize + 2;
    // JS: `opts.rng=opts.rng||rngFor(opts.seed||1,'v')`.
    resolve_rng(opts);
    let ctl = voice_controls(notes, p, n_f, opts, tuning);
    synth_voice(&ctl, p, len, opts, tuning)
}

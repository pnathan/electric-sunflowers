//! Bowed string: `VB_POLE`/`VB_BETA`/... and `renderViolin` (engine.js lines
//! ~1054-1079). A digital waveguide bowed string (McIntyre, Schumacher,
//! Woodhouse 1983; Smith; STK Bowed): two delay lines split at the bow point,
//! nut reflection -1, bridge reflection -lowpass, friction reflection coefficient
//! `(|v*s+o|+0.75)^-4` clipped to 1. The body (measured, see `body.rs`) is
//! applied in the mix, not here.

use sfcore::js;
use sfcore::rng::{rng_for, Rng};
use sfcore::SR_F;

const VB_POLE: f64 = 0.2;
const VB_BETA: f64 = 0.13;
const VB_P: f64 = 0.6;
const VN_S: f64 = 0.1;
const VN_P: f64 = 0.12;
const VN_B: f64 = 0.1;
const VN_R: f64 = 0.5;
const VN_N: f64 = 1.5;

/// `mtof(m)` from engine.js.
fn mtof_f(m: f64) -> f64 {
    440.0 * js::pow(2.0, (m - 69.0) / 12.0)
}

/// One input note to `render_violin`: `{t0,t1,m,v,vib}` in engine.js. `vib`
/// is `Some(0.0)` only when the JS explicitly sets `vib:0` to silence vibrato;
/// `None` matches an absent/non-zero field and uses the normal depth formula.
#[derive(Clone, Copy, Debug)]
pub struct ViolinNote {
    pub t0: f64,
    pub t1: f64,
    pub m: f64,
    pub v: f64,
    pub vib: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
struct WNote {
    t0: f64,
    t1: f64,
    m: f64,
    v: f64,
    vib: Option<f64>,
    force_bow: bool,
    rate: f64,
    depth: f64,
    vdel: f64,
    f: f64,
    bow: bool,
    slide: bool,
    dir: f64,
}

/// `renderViolin(notes,len,seed)`.
#[allow(unused_assignments)]
pub fn render_violin(notes: &[ViolinNote], len: usize, seed: u32) -> Vec<f32> {
    let mut r: Rng = rng_for(seed, "vln");
    let mut out = vec![0.0f32; len];

    let mut src: Vec<ViolinNote> = notes.to_vec();
    src.sort_by(|a, b| a.t0.total_cmp(&b.t0));

    let mut work: Vec<WNote> = Vec::new();
    for n in &src {
        let d = n.t1 - n.t0;
        if d <= 2.4 {
            work.push(WNote {
                t0: n.t0,
                t1: n.t1,
                m: n.m,
                v: n.v,
                vib: n.vib,
                force_bow: false,
                rate: 0.0,
                depth: 0.0,
                vdel: 0.0,
                f: 0.0,
                bow: false,
                slide: false,
                dir: 0.0,
            });
            continue;
        }
        let k = (d / (1.6 + r.next() * 0.6)).ceil() as i64;
        for j in 0..k {
            work.push(WNote {
                t0: n.t0 + d * j as f64 / k as f64,
                t1: n.t0 + d * (j + 1) as f64 / k as f64,
                m: n.m,
                v: n.v,
                vib: n.vib,
                force_bow: j > 0,
                rate: 0.0,
                depth: 0.0,
                vdel: 0.0,
                f: 0.0,
                bow: false,
                slide: false,
                dir: 0.0,
            });
        }
    }

    // group into phrases (gap < 0.06s)
    let mut phrases: Vec<Vec<WNote>> = Vec::new();
    for n in work {
        let start_new = match phrases.last() {
            Some(p) => n.t0 - p.last().unwrap().t1 >= 0.06,
            None => true,
        };
        if start_new {
            phrases.push(vec![n]);
        } else {
            phrases.last_mut().unwrap().push(n);
        }
    }

    let maxd = (SR_F / 150.0).ceil() as usize + 8;
    let cr = 16usize;
    let cr_f = cr as f64;

    for ph in phrases.iter_mut() {
        let t0 = ph[0].t0 - 0.03;
        let t1 = ph[ph.len() - 1].t1;
        let s0 = js::max(0.0, js::round(t0 * SR_F)) as i64;
        let l_len = (len as i64 - s0).min(js::round((t1 - t0 + 1.2) * SR_F) as i64).max(0);

        for k in 0..ph.len() {
            let prev_m = if k > 0 { Some(ph[k - 1].m) } else { None };
            let n = &mut ph[k];
            n.rate = 5.4 + r.next() * 1.0;
            n.depth = if matches!(n.vib, Some(v) if v == 0.0) { 0.0 } else { 0.18 + r.next() * 0.12 };
            n.vdel = if n.force_bow { 0.0 } else { 0.15 + r.next() * 0.2 };
            n.f = mtof_f(n.m);
            n.bow = k == 0 || n.force_bow || (n.t1 - n.t0) > 0.85 || r.next() < 0.4;
            n.slide = k > 0 && (n.m - prev_m.unwrap()).abs() >= 5.0 && r.next() < 0.5;
        }
        let mut dir = if r.next() < 0.5 { 1.0 } else { -1.0 };
        for k in 0..ph.len() {
            if k > 0 && ph[k].bow {
                dir = -dir;
            }
            ph[k].dir = dir;
        }

        let mut nd = vec![0.0f64; maxd];
        let mut bd = vec![0.0f64; maxd];
        let mut np = 0usize;
        let mut bp = 0usize;
        let b_j = VB_BETA + r.next() * 0.02;
        let mut db = SR_F / ph[0].f * b_j;
        let mut p_hi = 0.0f64;
        let pole = VB_POLE;
        let mut sg = 0.985f64;
        let mut lp = 0.0f64;

        let mut k_idx = 0usize;
        let mut f = ph[0].f;
        let mut vph = r.next() * 6.28;
        let mut dir_s = ph[0].dir;
        let mut slope = 3.5f64;
        let mut wander = 0.0f64;
        let mut dw = 0.0f64;
        let mut vd = 0.0f64;
        let mut dn = SR_F / f - db;
        let mut speed = 0.0f64;
        let mut lift = 1.0f64;
        let mut nz = 0.0f64;
        let mut nzs = 0.0f64;
        let mut sn = 0.0f64;
        let mut pn = 0.0f64;
        let mut bn = 0.0f64;
        let mut rn = 0.0f64;

        let ou_a = cr_f / SR_F;
        let ou = |x: f64, tau: f64, r: &mut Rng| -> f64 { x - x * ou_a / tau + (2.0 * ou_a / tau).sqrt() * r.gauss() };
        let del = |buf: &[f64], p: usize, d: f64| -> f64 {
            // Deviation from JS: a delay `d` at or beyond MAXD makes `x` stay
            // negative below (or land outside the buffer after wrap), which JS
            // reads as `undefined` and turns into NaN through the whole render.
            // Clamp `d` itself to MAXD-2 first, so the normal in-range math below
            // is untouched (x still lands in [0,MAXD) exactly as JS computes it)
            // and only a pathological note (below the delay line's frequency
            // floor) gets a bounded, non-NaN read instead of one that ruins the mix.
            let d = if d > maxd as f64 - 2.0 { maxd as f64 - 2.0 } else { d };
            let mut x = p as f64 - d;
            if x < 0.0 {
                x += maxd as f64;
            }
            let i = js::to_i32(x) as usize;
            let fr = x - i as f64;
            let a = buf[i];
            let b = buf[if i + 1 == maxd { 0 } else { i + 1 }];
            a + (b - a) * fr
        };

        let mut i = 0i64;
        while i < l_len {
            if (i as usize) % cr == 0 {
                let t = t0 + i as f64 / SR_F;
                while k_idx < ph.len() - 1 && t >= ph[k_idx + 1].t0 {
                    k_idx += 1;
                }
                let n = ph[k_idx];
                let since = t - n.t0;
                f += (n.f - f) * (1.0 - js::exp(-cr_f / (SR_F * (if n.slide { 0.07 } else { 0.01 }))));
                let nx_ = js::clamp(since / js::max(0.2, n.t1 - n.t0), 0.0, 1.0);
                vd = n.depth * smoothstep(n.vdel, n.vdel + 0.3, since) * (0.7 + 0.4 * nx_) * (if t > t1 { 0.0 } else { 1.0 });
                vph += 2.0 * std::f64::consts::PI * n.rate * (1.0 + 0.07 * rn) * cr_f / SR_F;
                rn = ou(rn, 0.25, &mut r);
                dw += r.gauss() * 0.0004;
                dw *= 0.97;
                wander = js::clamp(wander + dw * 0.05, -0.0015, 0.0015);
                let fi = f * js::exp(vd * js::sin(vph) * std::f64::consts::LN_2 / 12.0 + wander);
                let w0 = 2.0 * std::f64::consts::PI * fi / SR_F;
                let tau = js::atan2(pole * js::sin(w0), 1.0 - pole * js::cos(w0)) / w0;
                sn = ou(sn, 0.18, &mut r);
                pn = ou(pn, 0.12, &mut r);
                bn = ou(bn, 0.35, &mut r);
                db += (SR_F / f * b_j * (1.0 + VN_B * bn) - db) * (1.0 - js::exp(-cr_f / (SR_F * 0.06)));
                p_hi = 0.15 * js::clamp((12.0 * js::log2(f / 440.0) + 69.0 - 72.0) / 10.0, 0.0, 1.0);
                dn = js::max(2.0, SR_F / fi - tau - db);
                let att = smoothstep(0.0, if ph[0].t1 - ph[0].t0 > 1.0 { 0.16 } else { 0.06 }, t - t0);
                lift = if t > t1 { 1.0 - smoothstep(0.0, 0.05, t - t1) } else { 1.0 };
                sg = 0.985 - 0.06 * smoothstep(0.25, 0.45, t - t1);
                let dur = js::max(0.25, n.t1 - n.t0);
                let x_ = js::clamp(since / dur, 0.0, 1.0);
                let swell = if dur > 0.8 { 0.82 + 0.24 * js::sin(std::f64::consts::PI * js::min(1.0, x_ * 1.05)) } else { 0.96 };
                speed = (0.1 + 0.2 * n.v * swell) * att * (1.0 + VN_S * sn);
                dir_s += (n.dir - dir_s) * (1.0 - js::exp(-cr_f / (SR_F * 0.012)));
                let press = VB_P
                    + p_hi
                    + 0.22 * js::exp(-(t - t0) / 0.05)
                    + (if k_idx > 0 && n.bow { 0.15 * js::exp(-since / 0.04) } else { 0.0 })
                    + VN_P * pn;
                slope = js::clamp(5.0 - 4.0 * press, 1.2, 4.6);
            }
            nz += 0.3 * ((r.next() * 2.0 - 1.0) - nz);
            let vel = speed * dir_s * (1.0 + nz * VN_R);
            let bridge = del(&bd, bp, db);
            let nut = del(&nd, np, dn);
            lp = (1.0 - pole) * bridge + pole * lp;
            let b_r = -sg * lp;
            let n_r = -nut;
            let sv = b_r + n_r;
            let dv = vel - sv;
            let q = (dv * slope + 0.001).abs() + 0.75;
            let q2 = q * q;
            let mut rf = 1.0 / (q2 * q2);
            if rf > 1.0 {
                rf = 1.0;
            }
            let nv = dv * rf * lift;
            nd[np] = b_r + nv;
            bd[bp] = n_r + nv;
            np += 1;
            if np == maxd {
                np = 0;
            }
            bp += 1;
            if bp == maxd {
                bp = 0;
            }
            nzs += 0.25 * (nz - nzs);
            let idx = (s0 + i) as usize;
            let add = bridge * 1.6 + (nz - nzs) * speed * VN_N * lift;
            out[idx] = js::f32r(out[idx] as f64 + add) as f32;
            i += 1;
        }
    }
    out
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = js::clamp((x - a) / (b - a), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

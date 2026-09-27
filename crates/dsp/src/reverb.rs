//! `fdnReverb` (engine.js lines ~1163-1178): feedback delay network, 8 lines,
//! Householder-like feedback, per-line two-band absorption, input diffusion.

use crate::filter::{bq, FilterType};
use sfcore::js;
use sfcore::SR_F;

/// `fdnReverb(inL,inR,outL,outR,wet,seed)`: reads mono-summed `inL`/`inR`, adds
/// the wet signal into `outL`/`outR` (both Float32Array in JS).
pub fn fdn_reverb(in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32], wet: f64, seed: i64) {
    let n = in_l.len();
    let d_base = [1433i64, 1601, 1867, 2053, 2251, 2399, 2617, 2903];
    let d: Vec<usize> = d_base.iter().map(|&x| (x + (seed.wrapping_mul(x % 7)) % 31) as usize).collect();
    let t60_lo = 2.2f64;
    let t60_hi = 0.8f64;
    let mut lines: Vec<Vec<f64>> = d.iter().map(|&dd| vec![0.0f64; dd]).collect();
    let mut pos = vec![0usize; 8];
    let mut z = vec![0.0f64; 8];
    let a: Vec<(f64, f64)> = d
        .iter()
        .map(|&dd| {
            let gd = js::pow(10.0, -3.0 * dd as f64 / (t60_lo * SR_F));
            let gn = js::pow(10.0, -3.0 * dd as f64 / (t60_hi * SR_F));
            let al = (gd - gn) / (gd + gn);
            (gd * (1.0 - al), al)
        })
        .collect();

    let mut a0 = vec![0.0f64; 142];
    let mut a1 = vec![0.0f64; 379];
    let mut a2 = vec![0.0f64; 107];
    let mut a3 = vec![0.0f64; 277];
    let (mut ia0, mut ia1, mut ia2, mut ia3) = (0usize, 0usize, 0usize, 0usize);

    let pre = js::round(0.016 * SR_F) as usize;
    let mut pb = vec![0.0f64; pre.max(1)];
    let mut pp = 0usize;
    let hp = bq(FilterType::Hp, 220.0, 0.7, 0.0);
    let (mut hx1, mut hx2, mut hy1, mut hy2) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let mut o = vec![0.0f64; 8];

    for i in 0..n {
        let mut x = (in_l[i] as f64 + in_r[i] as f64) * 0.5;
        let y = hp.b0 * x + hp.b1 * hx1 + hp.b2 * hx2 - hp.a1 * hy1 - hp.a2 * hy2;
        hx2 = hx1;
        hx1 = x;
        hy2 = hy1;
        hy1 = y;
        x = y;

        let dl = pb[pp];
        pb[pp] = x;
        pp += 1;
        if pp == pre.max(1) {
            pp = 0;
        }
        x = dl;

        {
            let v = a0[ia0];
            let w = x + 0.65 * v;
            a0[ia0] = w;
            ia0 += 1;
            if ia0 == 142 {
                ia0 = 0;
            }
            x = v - 0.65 * w;
            let v = a1[ia1];
            let w = x + 0.62 * v;
            a1[ia1] = w;
            ia1 += 1;
            if ia1 == 379 {
                ia1 = 0;
            }
            x = v - 0.62 * w;
            let v = a2[ia2];
            let w = x + 0.6 * v;
            a2[ia2] = w;
            ia2 += 1;
            if ia2 == 107 {
                ia2 = 0;
            }
            x = v - 0.6 * w;
            let v = a3[ia3];
            let w = x + 0.58 * v;
            a3[ia3] = w;
            ia3 += 1;
            if ia3 == 277 {
                ia3 = 0;
            }
            x = v - 0.58 * w;
        }

        let mut sum = 0.0f64;
        for k in 0..8 {
            let v = lines[k][pos[k]];
            z[k] = a[k].0 * v + a[k].1 * z[k];
            o[k] = z[k];
            sum += z[k];
        }
        let h = sum * 0.25;
        let side = (in_l[i] as f64 - in_r[i] as f64) * 0.15;
        for k in 0..8 {
            let ln = &mut lines[k];
            let val = o[k] - h + x * (if k & 1 != 0 { 0.5 } else { 0.5 }) + (if k & 2 != 0 { side } else { -side });
            ln[pos[k]] = val;
            pos[k] += 1;
            if pos[k] == ln.len() {
                pos[k] = 0;
            }
        }
        let add_l = wet * (o[0] - o[2] + o[4] - o[6] + 0.5 * (o[1] + o[7]));
        let add_r = wet * (o[1] - o[3] + o[5] - o[7] + 0.5 * (o[2] + o[6]));
        out_l[i] = js::f32r(out_l[i] as f64 + add_l) as f32;
        out_r[i] = js::f32r(out_r[i] as f64 + add_r) as f32;
    }
}

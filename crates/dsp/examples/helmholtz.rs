//! Helmholtz-motion sweep of the bowed string (port of tests/sweep2.js).
//!
//! For MIDI 55..=90, velocity 0.4/0.6/0.85 and two seeds, renders one note
//! (t0 0.1 s, t1 1.2 s, 1.6 s long, seed m*13 + sd*101 + round(v*10)) and
//! measures Goertzel magnitudes (Goertzel 1958) over 0.45-1.0 s at f0, 2 f0,
//! f0/2 and 1.5 f0. A note holds Helmholtz motion when
//! a(f0) > 0.35 a(2 f0), a(f0/2) < 0.1 a(f0) and a(1.5 f0) < 0.1 a(f0).
//! Prints `stable N/216` and the failing notes as `midi:velocity`.

use dsp::violin::{render_violin, ViolinNote};
use rayon::prelude::*;
use sfcore::SR_F;

/// Goertzel magnitude of `x` at `f` Hz.
fn goertzel(x: &[f32], f: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * f / SR_F;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt()
}

fn main() {
    let mut cases = Vec::new();
    for m in 55..=90u32 {
        for &v in &[0.4f64, 0.6, 0.85] {
            for sd in [1u32, 2] {
                cases.push((m, v, sd));
            }
        }
    }
    let len = (1.6 * SR_F).round() as usize;
    let (a, b) = ((0.45 * SR_F).round() as usize, (1.0 * SR_F).round() as usize);
    let res: Vec<bool> = cases
        .par_iter()
        .map(|&(m, v, sd)| {
            let seed = m * 13 + sd * 101 + (v * 10.0).round() as u32;
            let note = ViolinNote { t0: 0.1, t1: 1.2, m: m as f64, v, vib: None };
            let x = render_violin(&[note], len, seed);
            let f = 440.0 * 2f64.powf((m as f64 - 69.0) / 12.0);
            let seg = &x[a..b];
            let a1 = goertzel(seg, f);
            let a2 = goertzel(seg, 2.0 * f);
            let ah = goertzel(seg, f / 2.0);
            let a15 = goertzel(seg, 1.5 * f);
            a1 > 0.35 * a2 && ah < 0.1 * a1 && a15 < 0.1 * a1
        })
        .collect();
    let ok = res.iter().filter(|&&s| s).count();
    let bad: Vec<String> =
        cases.iter().zip(&res).filter(|(_, &s)| !s).map(|(&(m, v, _), _)| format!("{m}:{v}")).collect();
    println!("stable {ok}/{} {}", cases.len(), bad.join(" "));
}

//! `addPan` (engine.js line ~610).

use sfcore::js;

/// `addPan(L,R,start,sig,pan,gain)`: equal-power pan-and-add of a mono signal
/// into stereo buffers at a sample offset.
pub fn add_pan(l: &mut [f32], r: &mut [f32], start: i64, sig: &[f32], pan: f64, gain: f64) {
    let gl = js::cos((pan + 1.0) * std::f64::consts::PI / 4.0) * gain;
    let gr = js::sin((pan + 1.0) * std::f64::consts::PI / 4.0) * gain;
    let n = (sig.len() as i64).min(l.len() as i64 - start);
    let lo = (-start).max(0);
    let mut i = lo;
    while i < n {
        let idx = (start + i) as usize;
        let s = sig[i as usize] as f64;
        l[idx] = js::f32r(l[idx] as f64 + s * gl) as f32;
        r[idx] = js::f32r(r[idx] as f64 + s * gr) as f32;
        i += 1;
    }
}

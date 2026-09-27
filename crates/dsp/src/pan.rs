//! Pan laws and pan-and-add (design section 5.13).
//!
//! - `equal_power`: mono source, sine/cosine law. theta = (pan + 1) pi / 4,
//!   gains (cos theta, sin theta); L^2 + R^2 = 1 at every pan, -3.01 dB each
//!   at centre. pan -1 is hard left, +1 hard right.
//! - `balance`: stereo source. The near channel stays at unity; the far
//!   channel is split by a cos/sin law, the cos part staying in place and the
//!   sin part folded across. For pan <= 0 with x = (pan + 1) pi / 2:
//!   L' = L + cos(x) R, R' = sin(x) R. For pan > 0 with x = pan pi / 2:
//!   L' = cos(x) L, R' = sin(x) L + R. Centre (pan 0) is the identity.

use std::f64::consts::PI;

/// Equal-power gains [left, right] for a mono source.
pub fn equal_power(pan: f64) -> [f32; 2] {
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    let th = (pan + 1.0) * PI / 4.0;
    [th.cos() as f32, th.sin() as f32]
}

/// Balance matrix for a stereo source: `[[ll, rl], [lr, rr]]`, rows are the
/// output channels, so out_l = m[0][0] L + m[0][1] R and out_r = m[1][0] L + m[1][1] R.
pub fn balance(pan: f64) -> [[f32; 2]; 2] {
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    if pan <= 0.0 {
        let x = (pan + 1.0) * PI / 2.0;
        [[1.0, x.cos() as f32], [0.0, x.sin() as f32]]
    } else {
        let x = pan * PI / 2.0;
        [[x.cos() as f32, 0.0], [x.sin() as f32, 1.0]]
    }
}

/// Add `src * gains` into `dst_l`/`dst_r` starting at frame `start`. Frames
/// before 0 and past the end of the destination are dropped.
pub fn add_mono(dst_l: &mut [f32], dst_r: &mut [f32], start: isize, src: &[f32], gains: [f32; 2]) {
    let skip = start.min(0).unsigned_abs();
    if skip >= src.len() {
        return;
    }
    let at = start.max(0) as usize;
    let end = dst_l.len().min(dst_r.len());
    if at >= end {
        return;
    }
    let n = (src.len() - skip).min(end - at);
    let [gl, gr] = gains;
    for ((l, r), s) in dst_l[at..at + n].iter_mut().zip(dst_r[at..at + n].iter_mut()).zip(&src[skip..skip + n]) {
        *l += s * gl;
        *r += s * gr;
    }
}

/// shim: deleted in wave 5. Equal-power pan-and-add with a gain.
#[doc(hidden)]
pub fn add_pan(l: &mut [f32], r: &mut [f32], start: i64, sig: &[f32], pan: f64, gain: f64) {
    let [gl, gr] = equal_power(pan);
    let g = gain as f32;
    let start = isize::try_from(start).unwrap_or(if start < 0 { isize::MIN } else { isize::MAX });
    add_mono(l, r, start, sig, [gl * g, gr * g]);
}

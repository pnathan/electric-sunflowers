//! `compress`, `stereoCompress`, `dbOf` (engine.js lines ~1154-1162).

use crate::or_default;
use sfcore::js;
use sfcore::SR_F;

/// `dbOf(v)`.
pub fn db_of(v: f64) -> f64 {
    20.0 * js::log10(v + 1e-12)
}

/// `compress(x,thrDb,ratio,atk,rel,knee)`: mono soft-knee compressor, in place
/// on a Float32Array. Gain is updated every 16th sample (JS `(i&15)===0`).
pub fn compress(x: &mut [f32], thr_db: f64, ratio: f64, atk: f64, rel: f64, knee: Option<f64>) {
    let ga = js::exp(-1.0 / (atk * SR_F));
    let gr = js::exp(-1.0 / (rel * SR_F));
    // JS parity: `const kn=knee||6` — knee is a plain f64 falsy read, not an
    // Option; `None` here plays the role of an absent/0/NaN JS argument.
    let kn = or_default(knee.unwrap_or(0.0), 6.0);
    let mut env = 0.0f64;
    let mut g = 1.0f64;
    for i in 0..x.len() {
        let a = (x[i] as f64).abs();
        env = if a > env { ga * env + (1.0 - ga) * a } else { gr * env + (1.0 - gr) * a };
        if i & 15 == 0 {
            let lv = 20.0 * js::log10(env + 1e-9);
            let o = lv - thr_db;
            let red = if o > kn / 2.0 {
                o * (1.0 - 1.0 / ratio)
            } else if o > -kn / 2.0 {
                let q = o + kn / 2.0;
                (1.0 - 1.0 / ratio) * q * q / (2.0 * kn)
            } else {
                0.0
            };
            g = js::pow(10.0, -red / 20.0);
        }
        x[i] = js::f32r(x[i] as f64 * g) as f32;
    }
}

/// `stereoCompress(L,R,thrDb,ratio,atk,rel)`: linked-detector stereo compressor
/// with a fixed 5 dB knee, in place on two Float32Arrays.
pub fn stereo_compress(l: &mut [f32], r: &mut [f32], thr_db: f64, ratio: f64, atk: f64, rel: f64) {
    let ga = js::exp(-1.0 / (atk * SR_F));
    let gr = js::exp(-1.0 / (rel * SR_F));
    let mut env = 0.0f64;
    let mut g = 1.0f64;
    for i in 0..l.len() {
        let a = js::max((l[i] as f64).abs(), (r[i] as f64).abs());
        env = if a > env { ga * env + (1.0 - ga) * a } else { gr * env + (1.0 - gr) * a };
        if i & 15 == 0 {
            let o = 20.0 * js::log10(env + 1e-9) - thr_db;
            let red = if o > 5.0 {
                o * (1.0 - 1.0 / ratio)
            } else if o > -5.0 {
                let q = o + 5.0;
                (1.0 - 1.0 / ratio) * q * q / 20.0
            } else {
                0.0
            };
            g = js::pow(10.0, -red / 20.0);
        }
        l[i] = js::f32r(l[i] as f64 * g) as f32;
        r[i] = js::f32r(r[i] as f64 * g) as f32;
    }
}

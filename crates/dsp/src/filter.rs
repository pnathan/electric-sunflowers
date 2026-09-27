//! `bq` and `runBq` (engine.js lines ~597-598): biquad coefficient design and the
//! Direct Form I run loop.

use sfcore::js;
use sfcore::SR_F;

/// Biquad coefficients, normalised by a0 (`bq` in engine.js).
#[derive(Clone, Copy, Debug)]
pub struct BqCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

/// Filter type tag matching the JS string literals passed to `bq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    Lp,
    Hp,
    Bp,
    Hs,
    Ls,
    /// The JS `else` branch (peaking / 'pk').
    Pk,
}

/// `bq(type,f,Q,g)`: RBJ cookbook biquad design, normalised by a0.
pub fn bq(ty: FilterType, f: f64, q: f64, g: f64) -> BqCoeffs {
    let w = 2.0 * std::f64::consts::PI * js::min(f, SR_F * 0.45) / SR_F;
    let c = js::cos(w);
    let s = js::sin(w);
    let al = s / (2.0 * q);
    let a = js::pow(10.0, g / 40.0);
    let (b0, b1, b2, a0, a1, a2);
    match ty {
        FilterType::Lp => {
            b0 = (1.0 - c) / 2.0;
            b1 = 1.0 - c;
            b2 = (1.0 - c) / 2.0;
            a0 = 1.0 + al;
            a1 = -2.0 * c;
            a2 = 1.0 - al;
        }
        FilterType::Hp => {
            b0 = (1.0 + c) / 2.0;
            b1 = -(1.0 + c);
            b2 = (1.0 + c) / 2.0;
            a0 = 1.0 + al;
            a1 = -2.0 * c;
            a2 = 1.0 - al;
        }
        FilterType::Bp => {
            b0 = al;
            b1 = 0.0;
            b2 = -al;
            a0 = 1.0 + al;
            a1 = -2.0 * c;
            a2 = 1.0 - al;
        }
        FilterType::Hs | FilterType::Ls => {
            // engine.js: 2*Math.sqrt(A)*al. IEEE sqrt is exact and equals
            // Math.sqrt, so use f64::sqrt directly rather than js::pow's
            // general (imprecise) pow(x,0.5) path.
            let sq = 2.0 * a.sqrt() * al;
            let sg = if ty == FilterType::Hs { 1.0 } else { -1.0 };
            b0 = a * ((a + 1.0) + sg * (a - 1.0) * c + sq);
            b1 = -2.0 * sg * a * ((a - 1.0) + sg * (a + 1.0) * c);
            b2 = a * ((a + 1.0) + sg * (a - 1.0) * c - sq);
            a0 = (a + 1.0) - sg * (a - 1.0) * c + sq;
            a1 = 2.0 * sg * ((a - 1.0) - sg * (a + 1.0) * c);
            a2 = (a + 1.0) - sg * (a - 1.0) * c - sq;
        }
        FilterType::Pk => {
            b0 = 1.0 + al * a;
            b1 = -2.0 * c;
            b2 = 1.0 - al * a;
            a0 = 1.0 + al / a;
            a1 = -2.0 * c;
            a2 = 1.0 - al / a;
        }
    }
    BqCoeffs { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
}

/// `runBq(x,c,out)` in place: `x` is a Float32Array in JS, so each store rounds to f32.
/// State (x1,x2,y1,y2) resets to 0 every call; there is no cross-call state.
pub fn run_bq(x: &mut [f32], c: &BqCoeffs) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for v in x.iter_mut() {
        let vv = *v as f64;
        let y = c.b0 * vv + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
        x2 = x1;
        x1 = vv;
        y2 = y1;
        y1 = y;
        *v = js::f32r(y) as f32;
    }
}

/// `runBq(x,c,out)` writing to a separate output buffer.
pub fn run_bq_into(x: &[f32], c: &BqCoeffs, out: &mut [f32]) {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for i in 0..x.len() {
        let vv = x[i] as f64;
        let y = c.b0 * vv + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2;
        x2 = x1;
        x1 = vv;
        y2 = y1;
        y1 = y;
        out[i] = js::f32r(y) as f32;
    }
}

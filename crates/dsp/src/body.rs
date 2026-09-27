//! `BODY_CURVES`, `bodyLevel`, `BODY_SPEC`, `bodyIRData` (engine.js lines ~953-985):
//! measured-body resonators. `bodyIRData` builds a stereo modal impulse response
//! whose modal density, decay and energy follow a measured curve.

use sfcore::js;
use sfcore::rng::{rng_for, Rng};
use sfcore::SR_F;

/// The three instrument bodies the engine models. Replaces JS's plain string
/// keys ("guitar"/"harp"/"violin"); an unknown name is a programmer error in
/// JS too (undefined lookups propagate NaN there), so this still panics on
/// `from_name`, but only once at the boundary instead of on every curve/spec
/// lookup, and with no per-call `format!` allocation for the rng tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Body {
    Guitar,
    Harp,
    Violin,
}

impl Body {
    pub fn from_name(name: &str) -> Self {
        match name {
            "guitar" => Body::Guitar,
            "harp" => Body::Harp,
            "violin" => Body::Violin,
            _ => panic!("unknown body {name}"),
        }
    }

    /// The JS body-curve/rng-tag string, e.g. `"body"+name` in `bodyIRData`.
    pub fn name(self) -> &'static str {
        match self {
            Body::Guitar => "guitar",
            Body::Harp => "harp",
            Body::Violin => "violin",
        }
    }

    fn rng_tag(self) -> &'static str {
        match self {
            Body::Guitar => "bodyguitar",
            Body::Harp => "bodyharp",
            Body::Violin => "bodyviolin",
        }
    }
}

/// `BODY_CURVES`: body transfer magnitude (dB, 1/12 octave from `lo` Hz), extracted
/// from anechoic recordings (Univ. of Iowa MIS): partial amplitudes of every note
/// divided by the ideal string-force spectrum 1/k.
pub struct BodyCurves;

impl BodyCurves {
    pub const LO: f64 = 80.0;
    pub const PER_OCT: f64 = 12.0;

    pub fn curve(body: Body) -> &'static [f64] {
        match body {
            Body::Violin => &VIOLIN,
            Body::Guitar => &GUITAR,
            Body::Harp => &HARP,
        }
    }
}

static VIOLIN: [f64; 86] = [
    -24.2, -23.5, -22.8, -22.2, -21.5, -20.8, -20.2, -19.5, -18.8, -18.2, -17.5, -16.8, -16.2, -15.5, -14.8, -14.2,
    -13.5, -12.8, -12.2, -11.5, -11.0, -8.2, -5.5, -3.8, -3.2, -3.5, -4.0, -4.5, -4.9, -5.1, -6.1, -7.6, -9.0, -9.4,
    -9.3, -10.0, -12.0, -14.0, -12.6, -10.2, -8.0, -8.3, -9.6, -10.6, -9.8, -7.9, -9.0, -12.0, -14.9, -14.7, -14.4,
    -14.0, -13.6, -10.1, -6.7, -1.9, 0.0, -0.1, -2.0, -5.9, -8.1, -10.1, -9.5, -9.8, -9.2, -9.7, -9.3, -9.8, -10.1,
    -11.3, -14.1, -18.0, -21.3, -22.8, -23.5, -24.9, -27.0, -28.6, -30.6, -32.6, -35.5, -37.6, -39.0, -40.8, -43.6,
    -46.5,
];
static GUITAR: [f64; 86] = [
    -36.8, -36.8, -36.8, -31.3, -25.8, -20.2, -19.1, -18.0, -17.4, -18.5, -19.9, -20.7, -18.9, -12.9, -6.6, -1.4,
    -0.2, 0.0, -1.2, -4.9, -10.3, -15.3, -16.2, -14.6, -13.1, -13.5, -11.7, -12.1, -13.1, -16.9, -18.6, -20.4, -22.0,
    -23.1, -25.1, -24.8, -26.4, -24.1, -24.1, -22.3, -22.4, -22.1, -20.7, -19.4, -18.1, -20.1, -23.6, -25.7, -26.8,
    -25.1, -27.3, -28.1, -30.9, -31.1, -30.9, -30.2, -29.4, -29.0, -28.6, -29.9, -31.9, -35.3, -37.9, -40.4, -41.8,
    -42.6, -42.7, -42.4, -42.6, -42.9, -43.3, -43.2, -42.7, -41.8, -40.8, -39.6, -38.7, -37.9, -37.7, -37.6, -37.6,
    -37.8, -38.5, -39.0, -39.1, -38.8,
];
static HARP: [f64; 86] = [
    -35.3, -34.2, -32.0, -28.6, -25.1, -21.3, -18.6, -17.1, -17.0, -17.3, -17.5, -16.6, -14.3, -10.6, -6.5, -2.7,
    -0.3, 0.0, -1.8, -4.8, -8.0, -10.7, -12.4, -13.0, -12.3, -11.4, -11.1, -11.9, -12.9, -14.7, -16.7, -18.7, -20.3,
    -21.6, -22.8, -23.4, -23.8, -23.5, -23.3, -22.8, -22.5, -21.9, -21.4, -21.2, -22.0, -23.5, -25.6, -27.6, -29.6,
    -31.1, -32.7, -34.1, -35.9, -37.0, -37.9, -38.1, -38.2, -38.6, -39.5, -41.3, -43.6, -46.6, -49.5, -52.2, -54.3,
    -55.8, -56.8, -57.6, -58.3, -59.0, -59.6, -60.1, -60.2, -60.1, -59.8, -59.4, -59.1, -58.9, -58.9, -59.0, -59.3,
    -59.9, -60.4, -60.9, -61.4, -61.7,
];

/// `bodyLevel(name,f)`: interpolate/extrapolate the measured curve at frequency `f`.
pub fn body_level(body: Body, f: f64) -> f64 {
    let a = BodyCurves::curve(body);
    let x = js::log2(f / BodyCurves::LO) * BodyCurves::PER_OCT;
    if x <= 0.0 {
        return a[0] + 6.0 * x / BodyCurves::PER_OCT * 2.0;
    }
    if x >= a.len() as f64 - 1.0 {
        return a[a.len() - 1] - (x - a.len() as f64 + 1.0) / BodyCurves::PER_OCT * 12.0;
    }
    let i = x as usize;
    let t = x - i as f64;
    a[i] + (a[i + 1] - a[i]) * t
}

/// `BODY_SPEC`: per-instrument modal-synthesis parameters for `bodyIRData`.
#[derive(Clone, Copy)]
pub struct BodySpec {
    pub sec: f64,
    pub q_lo: f64,
    pub q_hi: f64,
    pub f_min: f64,
}

pub fn body_spec(body: Body) -> BodySpec {
    match body {
        Body::Guitar => BodySpec { sec: 0.36, q_lo: 26.0, q_hi: 48.0, f_min: 70.0 },
        Body::Harp => BodySpec { sec: 0.4, q_lo: 18.0, q_hi: 34.0, f_min: 60.0 },
        Body::Violin => BodySpec { sec: 0.32, q_lo: 30.0, q_hi: 40.0, f_min: 180.0 },
    }
}

/// `bodyIRData(name,seed)`: a stereo modal impulse response whose modal density,
/// decay and energy follow the measured `bodyLevel` curve for `name`.
pub fn body_ir_data(body: Body, seed: u32) -> [Vec<f32>; 2] {
    let s = body_spec(body);
    let mut r: Rng = rng_for(seed, body.rng_tag());
    let n = js::round(s.sec * SR_F) as usize;
    let mut ch: [Vec<f32>; 2] = [vec![0.0; n], vec![0.0; n]];
    let mut f = s.f_min;
    while f < 13000.0 {
        let df = js::max(3.0, f * 0.011) * (0.55 + 0.9 * r.next());
        f += df;
        let q = s.q_lo + (s.q_hi - s.q_lo) * js::clamp(js::log2(f / 200.0) / 6.0, 0.0, 1.0) * (0.7 + 0.6 * r.next());
        let tau = q / (std::f64::consts::PI * f);
        let t = js::pow(10.0, body_level(body, f) / 10.0);
        let a0 = (t * df / tau).sqrt();
        let w = 2.0 * std::f64::consts::PI * f / SR_F;
        let rd = js::exp(-1.0 / (tau * SR_F));
        let c2 = 2.0 * rd * js::cos(w);
        let r2 = rd * rd;
        let m = n.min((tau * SR_F * 9.5).ceil() as usize);
        for c in 0..2 {
            let a = a0 * r.gauss();
            let ph = r.next() * 2.0 * std::f64::consts::PI;
            let d = &mut ch[c];
            let mut s1 = a * js::sin(ph);
            let mut s2 = a * js::sin(ph - w) / rd;
            for i in 0..m {
                let y = c2 * s1 - r2 * s2;
                d[i] = js::f32r(d[i] as f64 + s1) as f32;
                s2 = s1;
                s1 = y;
            }
        }
    }
    // normalise to unit energy per channel, short fade at the tail
    let fl = js::round(n as f64 * 0.1) as usize;
    for c in 0..2 {
        let d = &mut ch[c];
        let mut e = 0.0f64;
        for i in 0..n {
            e += d[i] as f64 * d[i] as f64;
        }
        let g = 1.0 / (if e == 0.0 { 1.0 } else { e }).sqrt();
        for i in 0..n {
            let fade = if i > n.saturating_sub(fl) { (n - i) as f64 / fl as f64 } else { 1.0 };
            d[i] = js::f32r(d[i] as f64 * g * fade) as f32;
        }
    }
    ch
}

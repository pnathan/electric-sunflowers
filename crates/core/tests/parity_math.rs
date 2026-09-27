//! Checks sfcore's transcendental math against node's V8, using
//! ref/parity/math.bin (see tests/parity/math.js). Run tests/parity/gen.sh
//! first.
//!
//! Under `--features sfcore/v8`: bit-exact (v8math.rs is a line-for-line
//! fdlibm port). In the default build: max 2 ulp, which is what the fast
//! std/libm math actually measures at (see examples/mathbench.rs); the
//! owner's rule is that this math has to be right for sound, not bit-exact
//! with V8, so the default build is not held to bit-exactness.

use std::io::Read;

fn load() -> Vec<u8> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ref/parity/math.bin");
    let mut raw = Vec::new();
    std::fs::File::open(path)
        .expect("run tests/parity/gen.sh first")
        .read_to_end(&mut raw)
        .unwrap();
    raw
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.b[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn f64(&mut self) -> f64 {
        let v = f64::from_le_bytes(self.b[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        v
    }
}

fn bits_eq(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

/// Distance in ULPs between two finite, same-signed-domain floats, as an
/// integer difference of their bit patterns. NaN vs NaN is 0; NaN vs
/// non-NaN is treated as infinitely far.
fn ulp_diff(a: f64, b: f64) -> f64 {
    if a.is_nan() && b.is_nan() {
        return 0.0;
    }
    if a.is_nan() || b.is_nan() {
        return f64::INFINITY;
    }
    if a == b {
        return 0.0;
    }
    ((a.to_bits() as i64) - (b.to_bits() as i64)).unsigned_abs() as f64
}

/// Max ULP error allowed in the default (non-v8) build: the fast std/libm
/// math measured at <=2 ulp against node's V8 output (see mathbench.rs and
/// the manual probe that produced this bound); 0 under the v8 feature,
/// where the port must be bit-exact.
#[cfg(feature = "v8")]
const MAX_ULP: f64 = 0.0;
#[cfg(not(feature = "v8"))]
const MAX_ULP: f64 = 2.0;

fn check_unary(r: &mut Reader, name: &str, f: impl Fn(f64) -> f64) {
    let n = r.u32();
    let mut mism = 0u32;
    let mut worst = 0.0f64;
    let mut first = None;
    for _ in 0..n {
        let x = r.f64();
        let y = r.f64();
        let got = f(x);
        let u = ulp_diff(got, y);
        if u > worst {
            worst = u;
        }
        if u > MAX_ULP {
            mism += 1;
            if first.is_none() {
                first = Some((x, y, got, u));
            }
        }
    }
    assert_eq!(
        mism, 0,
        "{name}: {mism} of {n} exceeded {MAX_ULP} ulp (max seen {worst}), first {first:?}"
    );
}

fn check_binary(r: &mut Reader, name: &str, f: impl Fn(f64, f64) -> f64) {
    let n = r.u32();
    let mut mism = 0u32;
    let mut worst = 0.0f64;
    let mut first = None;
    for _ in 0..n {
        let a = r.f64();
        let b = r.f64();
        let y = r.f64();
        let got = f(a, b);
        let u = ulp_diff(got, y);
        if u > worst {
            worst = u;
        }
        if u > MAX_ULP {
            mism += 1;
            if first.is_none() {
                first = Some((a, b, y, got, u));
            }
        }
    }
    assert_eq!(
        mism, 0,
        "{name}: {mism} of {n} exceeded {MAX_ULP} ulp (max seen {worst}), first {first:?}"
    );
}

/// pow and sign are exact JS semantics (a special case plus std powf, and a
/// pure sign-bit rule), not approximated math, so both builds hold them to
/// bit-exactness rather than the ULP bound.
fn check_exact_unary(r: &mut Reader, name: &str, f: impl Fn(f64) -> f64) {
    let n = r.u32();
    let mut mism = 0u32;
    let mut first = None;
    for _ in 0..n {
        let x = r.f64();
        let y = r.f64();
        let got = f(x);
        if !bits_eq(got, y) {
            mism += 1;
            if first.is_none() {
                first = Some((x, y, got));
            }
        }
    }
    assert_eq!(mism, 0, "{name}: {mism} of {n} mismatched, first {first:?}");
}

fn check_exact_binary(r: &mut Reader, name: &str, f: impl Fn(f64, f64) -> f64) {
    let n = r.u32();
    let mut mism = 0u32;
    let mut first = None;
    for _ in 0..n {
        let a = r.f64();
        let b = r.f64();
        let y = r.f64();
        let got = f(a, b);
        if !bits_eq(got, y) {
            mism += 1;
            if first.is_none() {
                first = Some((a, b, y, got));
            }
        }
    }
    assert_eq!(mism, 0, "{name}: {mism} of {n} mismatched, first {first:?}");
}

#[test]
fn v8math_parity() {
    let raw = load();
    let mut r = Reader { b: &raw, pos: 0 };
    check_unary(&mut r, "sin", sfcore::js::sin);
    check_unary(&mut r, "cos", sfcore::js::cos);
    check_unary(&mut r, "log", sfcore::js::log);
    check_unary(&mut r, "log2", sfcore::js::log2);
    check_unary(&mut r, "log10", sfcore::js::log10);
    check_exact_binary(&mut r, "pow", sfcore::js::pow);
    check_binary(&mut r, "atan2", sfcore::js::atan2);
    check_exact_unary(&mut r, "sign", sfcore::js::sign);
}

//! Asserts sfcore's transcendental math is bit-exact with node's V8, using
//! ref/parity/math.bin (see tests/parity/math.js). Run tests/parity/gen.sh
//! first.

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

fn check_unary(r: &mut Reader, name: &str, f: impl Fn(f64) -> f64) {
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

fn check_binary(r: &mut Reader, name: &str, f: impl Fn(f64, f64) -> f64) {
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
fn v8math_bit_exact() {
    let raw = load();
    let mut r = Reader { b: &raw, pos: 0 };
    check_unary(&mut r, "sin", sfcore::js::sin);
    check_unary(&mut r, "cos", sfcore::js::cos);
    check_unary(&mut r, "log", sfcore::js::log);
    check_unary(&mut r, "log2", sfcore::js::log2);
    check_unary(&mut r, "log10", sfcore::js::log10);
    check_binary(&mut r, "pow", sfcore::js::pow);
    check_binary(&mut r, "atan2", sfcore::js::atan2);
    check_unary(&mut r, "sign", sfcore::js::sign);
}

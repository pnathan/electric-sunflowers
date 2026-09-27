//! Reads ref/parity/math.bin (see tests/parity/math.js) and reports, per
//! function, how many of node's outputs sfcore's v8math port fails to
//! reproduce bit-exactly. Run: cargo run -p sfcore --example mathprobe
//! (after tests/parity/gen.sh has produced ref/parity/math.bin).

use std::io::Read;

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
    if a.is_nan() && b.is_nan() {
        return true;
    }
    a.to_bits() == b.to_bits()
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ref/parity/math.bin".to_string());
    let mut raw = Vec::new();
    std::fs::File::open(&path)
        .unwrap_or_else(|e| panic!("open {}: {} (run tests/parity/gen.sh first)", path, e))
        .read_to_end(&mut raw)
        .unwrap();
    let mut r = Reader { b: &raw, pos: 0 };

    for (name, f) in [
        ("sin", sfcore::v8math::sin as fn(f64) -> f64),
        ("cos", sfcore::v8math::cos as fn(f64) -> f64),
    ] {
        let n = r.u32();
        let mut mism = 0u32;
        for _ in 0..n {
            let x = r.f64();
            let y = r.f64();
            if !bits_eq(f(x), y) {
                mism += 1;
            }
        }
        println!("{:6} mismatches {:7} of {}", name, mism, n);
    }
    for (name, f) in [
        ("log", sfcore::v8math::log as fn(f64) -> f64),
        ("log2", sfcore::v8math::log2 as fn(f64) -> f64),
        ("log10", sfcore::v8math::log10 as fn(f64) -> f64),
    ] {
        let n = r.u32();
        let mut mism = 0u32;
        for _ in 0..n {
            let x = r.f64();
            let y = r.f64();
            if !bits_eq(f(x), y) {
                mism += 1;
            }
        }
        println!("{:6} mismatches {:7} of {}", name, mism, n);
    }
    // pow: currently std::f64::powf (via sfcore::js::pow); report but don't
    // route through v8math (no port written; std/glibc already bit-exact
    // per the original 200k probe).
    {
        let n = r.u32();
        let mut mism = 0u32;
        for _ in 0..n {
            let a = r.f64();
            let b = r.f64();
            let y = r.f64();
            if !bits_eq(sfcore::js::pow(a, b), y) {
                mism += 1;
            }
        }
        println!("{:6} mismatches {:7} of {}", "pow", mism, n);
    }
    {
        let n = r.u32();
        let mut mism = 0u32;
        for _ in 0..n {
            let a = r.f64();
            let b = r.f64();
            let y = r.f64();
            if !bits_eq(sfcore::js::atan2(a, b), y) {
                mism += 1;
            }
        }
        println!("{:6} mismatches {:7} of {}", "atan2", mism, n);
    }
}

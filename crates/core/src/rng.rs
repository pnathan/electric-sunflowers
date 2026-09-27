//! Seeded random numbers, bit-identical to engine.js (`mulberry32`, `hashStr`, `rngFor`, `gauss`).
//!
//! Every stream is keyed by a seed and a tag string. A port matches the JS output
//! only if it draws from the same streams in the same order, call for call.

use crate::js;

/// `mulberry32`: a 32-bit generator returning values in [0, 1).
#[derive(Clone, Debug)]
pub struct Rng {
    a: i32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Rng { a: seed as i32 }
    }

    /// One draw, as `r()` in JS.
    #[inline]
    pub fn next(&mut self) -> f64 {
        self.a = self.a.wrapping_add(0x6D2B79F5);
        let a = self.a;
        let mut t = js::imul(a ^ ((a as u32) >> 15) as i32, 1 | a);
        t = t.wrapping_add(js::imul(t ^ ((t as u32) >> 7) as i32, 61 | t)) ^ t;
        let r = (t ^ ((t as u32) >> 14) as i32) as u32;
        r as f64 / 4294967296.0
    }

    /// `gauss(r)`: Box-Muller, one normal deviate per call (draws twice or more).
    pub fn gauss(&mut self) -> f64 {
        let mut u = 0.0;
        while u == 0.0 {
            u = self.next();
        }
        (-2.0 * js::log(u)).sqrt() * js::cos(2.0 * std::f64::consts::PI * self.next())
    }
}

/// `hashStr`: FNV-1a over UTF-16 code units.
pub fn hash_str(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for c in s.encode_utf16() {
        h ^= c as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// `rngFor(seed, tag)`. `seed` carries the JS value's ToInt32 bits.
pub fn rng_for(seed: u32, tag: &str) -> Rng {
    Rng::new(seed ^ hash_str(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Reference values from node: rngFor(1234,'lead') first three draws, hashStr('r|verse|0'),
    // and two gauss() values from rngFor(5,'g').
    #[test]
    fn matches_js_reference() {
        assert_eq!(hash_str("r|verse|0"), REF_HASH);
        let mut r = rng_for(1234, "lead");
        for v in REF_DRAWS {
            assert_eq!(r.next(), v);
        }
        let mut g = rng_for(5, "g");
        assert_eq!(g.gauss(), 1.116724585254541);
        assert_eq!(g.gauss(), 0.8334645124301873);
    }

    const REF_HASH: u32 = 2818696708;
    const REF_DRAWS: [f64; 3] = [0.09561664168722928, 0.41913972585462034, 0.4711336656473577];
}

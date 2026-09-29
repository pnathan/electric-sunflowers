//! Random numbers: xoshiro128++ (Blackman and Vigna 2018) seeded by SplitMix64 (Steele, Lea, Flood 2014), with compile-time FNV-1a stream tags.
//!
//! Every stream is a pure function of (song seed, tag, event index), so an
//! event's draws do not depend on render order, thread count or how many
//! draws any other event made. Tags are declared next to their use:
//!
//! ```
//! use sfcore::random::{tag, Rng, Tag};
//! const GUITAR_STRUM: Tag = tag("guitar.strum");
//! let mut r = Rng::event(1234, GUITAR_STRUM, 7);
//! let x = r.bipolar();
//! assert!((-1.0..1.0).contains(&x));
//! ```
//!
//! References:
//! - D. Blackman, S. Vigna, "Scrambled linear pseudorandom number
//!   generators", 2018 (xoshiro128++: 128-bit state, period 2^128 - 1).
//! - G. Steele, D. Lea, C. Flood, "Fast splittable pseudorandom number
//!   generators", OOPSLA 2014 (SplitMix64, used here as a seed mixer).
//! - G. Fowler, L. Noll, K.-P. Vo, FNV-1a hash (64-bit offset basis
//!   0xcbf29ce484222325, prime 0x100000001b3).
//! - G. Marsaglia, T. Bray, "A convenient method for generating normal
//!   variables", SIAM Review 6(3), 1964 (polar method).
//! - D. Lemire, "Fast random integer generation in an interval", ACM TOMACS
//!   29(1), 2019 (unbiased `below`).

/// A stream tag: a 64-bit FNV-1a hash of a name, built at compile time.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Tag(pub u64);

/// FNV-1a 64 over the UTF-8 bytes of `name`. `const`, so tags are
/// constants: `const DRUM_HIT: Tag = tag("drums.hit");`.
pub const fn tag(name: &str) -> Tag {
    let b = name.as_bytes();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    Tag(h)
}

/// SplitMix64 output function (Steele, Lea, Flood 2014; the finaliser of
/// MurmurHash3 variant 13): a bijective 64-bit mix.
#[inline]
const fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// SplitMix64 increment (the golden-ratio constant 2^64 / phi).
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

/// Domain separator for the index step of `Rng::event`, so that
/// `event(s, t, i)` never reproduces `stream(s, t)` by construction.
const EVENT_DOMAIN: u64 = 0x6576_656e_745f_6978; // "event_ix"

/// xoshiro128++ generator (Blackman and Vigna 2018): four 32-bit words of
/// state, output `rotl(s0 + s3, 7) + s0`. Plus a cached second deviate for
/// `gauss`. Not `Copy`, so a stream is never duplicated by accident.
#[derive(Clone, Debug)]
pub struct Rng {
    s: [u32; 4],
    spare: Option<f64>,
}

impl Rng {
    /// Seeds the state from `seed` by two SplitMix64 steps (Steele, Lea,
    /// Flood 2014), as Blackman and Vigna recommend. The all-zero state,
    /// which xoshiro cannot leave, is replaced by a fixed nonzero state.
    pub fn from_seed(seed: u64) -> Rng {
        let a = mix64(seed.wrapping_add(GAMMA));
        let b = mix64(seed.wrapping_add(GAMMA.wrapping_mul(2)));
        let mut s = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32];
        if s == [0; 4] {
            s = [0x9e37_79b9, 0x7f4a_7c15, 0xf39c_c060, 0x5ced_c834];
        }
        Rng { s, spare: None }
    }

    /// The stream for (`seed`, `tag`): `mix64(mix64(seed) ^ tag)` seeds the
    /// generator. Use for draws that belong to one track as a whole.
    pub fn stream(seed: u64, tag: Tag) -> Rng {
        Rng::from_seed(mix64(mix64(seed) ^ tag.0))
    }

    /// The stream for event `index` of (`seed`, `tag`), e.g. guitar stroke
    /// k or choir singer i: the `stream` key mixed again with
    /// `mix64(index ^ EVENT_DOMAIN)`. Each event is independent of every
    /// other event's draw count.
    pub fn event(seed: u64, tag: Tag, index: u64) -> Rng {
        let k = mix64(mix64(seed) ^ tag.0);
        Rng::from_seed(mix64(k ^ mix64(index ^ EVENT_DOMAIN)))
    }

    /// Next 32 random bits (xoshiro128++ step).
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let s = &mut self.s;
        let out = s[0].wrapping_add(s[3]).rotate_left(7).wrapping_add(s[0]);
        let t = s[1] << 9;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(11);
        out
    }

    /// Uniform in [0, 1) from one 32-bit draw: `u32 * 2^-32`. The grid step
    /// is 2.3e-10, far finer than any audio parameter needs; the value is
    /// exact in `f64` and never reaches 1.
    #[inline]
    pub fn uniform(&mut self) -> f64 {
        self.next_u32() as f64 * (1.0 / 4_294_967_296.0)
    }

    /// Uniform in [-1, 1): `2 uniform() - 1`.
    #[inline]
    pub fn bipolar(&mut self) -> f64 {
        2.0 * self.uniform() - 1.0
    }

    /// Uniform in [`lo`, `hi`).
    #[inline]
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.uniform()
    }

    /// Standard normal deviate (mean 0, variance 1) by the Marsaglia polar
    /// method: draw (u, v) uniform in the square until 0 < s = u^2 + v^2 < 1,
    /// then `u m` and `v m` with `m = sqrt(-2 ln s / s)` are two independent
    /// normals. The second is cached and returned by the next call.
    pub fn gauss(&mut self) -> f64 {
        if let Some(g) = self.spare.take() {
            return g;
        }
        loop {
            let u = self.bipolar();
            let v = self.bipolar();
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                let m = (-2.0 * s.ln() / s).sqrt();
                self.spare = Some(v * m);
                return u * m;
            }
        }
    }

    /// Fills `out` with uniform values in [-1, 1): the top 24 bits of each
    /// draw scaled by 2^-23, minus 1. Every value is exact in `f32`.
    pub fn fill_bipolar(&mut self, out: &mut [f32]) {
        for x in out {
            *x = (self.next_u32() >> 8) as f32 * (1.0 / 8_388_608.0) - 1.0;
        }
    }

    /// Unbiased integer in [0, n) by Lemire's multiply-and-reject method.
    /// `n == 0` gives 0.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut m = self.next_u32() as u64 * n as u64;
        if (m as u32) < n {
            let thresh = n.wrapping_neg() % n;
            while (m as u32) < thresh {
                m = self.next_u32() as u64 * n as u64;
            }
        }
        (m >> 32) as u32
    }

    /// A uniformly chosen element of `items`; `None` when empty. Slices
    /// longer than `u32::MAX` pick only from the first `u32::MAX` elements.
    #[inline]
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        let n = u32::try_from(items.len()).unwrap_or(u32::MAX);
        if n == 0 {
            return None;
        }
        items.get(self.below(n) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// First ten outputs of the reference xoshiro128plusplus.c (Blackman
    /// and Vigna) for state {1, 2, 3, 4}; the same vector is in the
    /// rand_xoshiro crate's tests.
    #[test]
    fn xoshiro128pp_reference_vector() {
        let mut r = Rng {
            s: [1, 2, 3, 4],
            spare: None,
        };
        let want = [
            641, 1573767, 3222811527, 3517856514, 836907274, 4247214768, 3867114732, 1355841295,
            495546011, 621204420,
        ];
        for w in want {
            assert_eq!(r.next_u32(), w);
        }
    }
}

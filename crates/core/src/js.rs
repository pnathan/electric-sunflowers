//! JavaScript number semantics, so the port reproduces the JS engine's arithmetic.
//!
//! JS computes in f64 and rounds to f32 only when it stores into a Float32Array.
//! Transcendental functions must go through this module, never `f64::sin` and friends
//! directly.
//!
//! Two modes, picked by the `v8` cargo feature:
//!
//! - `--features sfcore/v8` (off by default): bit-exact with node 24's V8, via
//!   the ported fdlibm/V8 algorithm in v8math.rs for sin/cos/log/log2/log10,
//!   libm for atan2 and exp, and std powf for pow. Measured against node on
//!   200k inputs plus hard cases (crates/core/tests/parity_math.rs, data from
//!   tests/parity/math.js). This mode exists only to verify the port; it is
//!   not needed for correct sound.
//! - default (no feature): std's sin/cos/log/log2/log10/atan2/exp/powf, which
//!   crates/core/examples/mathbench.rs measured as the fastest option on this
//!   platform for every one of these functions (std beat both libm and the
//!   v8math port on realistic engine.js argument ranges: sin/cos phases up to
//!   1e6 rad, exp of negative arguments, log of positive arguments spanning
//!   1e-9..1e4, atan2 near the origin, pow with bases 2/10/~1). Accuracy
//!   checked against the same node reference data is within 2 ulp (see
//!   parity_math.rs's non-v8 assertion), which is what "right for sound"
//!   needs; V8 exactness is not a sound requirement. exp is kept on libm
//!   under the v8 feature regardless (see `exp` below): it feeds IIR
//!   feedback coefficients where a few ulp of drift compounds sample by
//!   sample, and crates/arrange's full-render parity tests caught the
//!   divergence when std was tried there unconditionally.

/// `Math.round`: halves round toward +infinity.
/// JS parity: Math.round preserves the sign of the input on a zero result
/// (round(-0.4) is -0), so copy the sign onto the returned zero.
#[inline]
pub fn round(x: f64) -> f64 {
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    if r == 0.0 {
        0.0_f64.copysign(x)
    } else {
        r
    }
}

/// `Math.sign`: NaN for NaN, x itself for +0/-0, else +-1.0.
#[inline]
pub fn sign(x: f64) -> f64 {
    if x.is_nan() || x == 0.0 {
        x
    } else if x > 0.0 {
        1.0
    } else {
        -1.0
    }
}

/// `ToInt32`, as in `x | 0`.
#[inline]
pub fn to_i32(x: f64) -> i32 {
    to_u32(x) as i32
}

/// `ToUint32`, as in `x >>> 0`.
#[inline]
pub fn to_u32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    if t.abs() < 4294967296.0 {
        return (t as i64) as u32;
    }
    t.rem_euclid(4294967296.0) as u32
}

/// `Math.imul`.
#[inline]
pub fn imul(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// Store an f64 as a Float32Array element would.
#[inline]
pub fn f32r(x: f64) -> f64 {
    x as f32 as f64
}

#[cfg(feature = "v8")]
#[inline]
pub fn sin(x: f64) -> f64 {
    crate::v8math::sin(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn sin(x: f64) -> f64 {
    x.sin()
}

#[cfg(feature = "v8")]
#[inline]
pub fn cos(x: f64) -> f64 {
    crate::v8math::cos(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn cos(x: f64) -> f64 {
    x.cos()
}

#[cfg(feature = "v8")]
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

#[cfg(feature = "v8")]
#[inline]
pub fn exp(x: f64) -> f64 {
    // Bit-exact with node's Math.exp (verified by the original 200k probe).
    // exp feeds IIR feedback coefficients (resonant filters, pluck decay);
    // a few ulp of drift here compounds sample by sample, so this path
    // must stay on the exact implementation under the v8 feature even
    // though std measured indistinguishably fast and close in isolation
    // (see mathbench.rs and crates/arrange's parity tests, which caught
    // the divergence when std was tried here unconditionally).
    libm::exp(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn exp(x: f64) -> f64 {
    x.exp()
}

#[cfg(feature = "v8")]
#[inline]
pub fn log(x: f64) -> f64 {
    crate::v8math::log(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn log(x: f64) -> f64 {
    x.ln()
}

#[cfg(feature = "v8")]
#[inline]
pub fn log2(x: f64) -> f64 {
    crate::v8math::log2(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn log2(x: f64) -> f64 {
    x.log2()
}

#[cfg(feature = "v8")]
#[inline]
pub fn log10(x: f64) -> f64 {
    crate::v8math::log10(x)
}
#[cfg(not(feature = "v8"))]
#[inline]
pub fn log10(x: f64) -> f64 {
    x.log10()
}
/// `Math.pow`. JS parity: the ECMAScript spec (Number::exponentiate) special-
/// cases base +-1 raised to an infinite exponent to NaN; IEEE 754 `pow` (and
/// so Rust's `f64::powf`) returns 1.0 there instead.
#[inline]
pub fn pow(x: f64, y: f64) -> f64 {
    if (x == 1.0 || x == -1.0) && y.is_infinite() {
        return f64::NAN;
    }
    x.powf(y)
}
/// `Math.max` over two values, with the JS NaN rule.
/// JS parity: max(0, -0) is +0 (0 > -0 is false under IEEE, but Math.max
/// special-cases +0 over -0), so a plain `a > b` comparison is not enough.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() {
            b
        } else {
            a
        }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min` over two values, with the JS NaN rule.
/// JS parity: min(0, -0) is -0.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == 0.0 && b == 0.0 {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `clamp` from engine.js: `x<a?a:x>b?b:x`.
#[inline]
pub fn clamp(x: f64, a: f64, b: f64) -> f64 {
    if x < a {
        a
    } else if x > b {
        b
    } else {
        x
    }
}

/// `((m % 12) + 12) % 12` for an integer-valued pitch.
#[inline]
pub fn pc(m: i32) -> i32 {
    m.rem_euclid(12)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_matches_js() {
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -2.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-0.4), 0.0);
        assert!(round(-0.4).is_sign_negative());
        assert!(round(-0.0).is_sign_negative());
        assert!(!round(0.0).is_sign_negative());
    }

    #[test]
    fn sign_matches_js() {
        assert!(sign(f64::NAN).is_nan());
        assert!(sign(0.0).is_sign_positive() && sign(0.0) == 0.0);
        assert!(sign(-0.0).is_sign_negative() && sign(-0.0) == 0.0);
        assert_eq!(sign(5.0), 1.0);
        assert_eq!(sign(-5.0), -1.0);
        assert_eq!(sign(f64::INFINITY), 1.0);
        assert_eq!(sign(f64::NEG_INFINITY), -1.0);
    }

    #[test]
    fn max_min_signed_zero_matches_js() {
        assert!(max(0.0, -0.0).is_sign_positive());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(min(-0.0, 0.0).is_sign_negative());
        assert!(max(f64::NAN, 1.0).is_nan());
        assert!(min(1.0, f64::NAN).is_nan());
    }

    #[test]
    fn int_conversions_match_js() {
        assert_eq!(to_i32(4294967296.0 + 5.0), 5);
        assert_eq!(to_i32(-1.5), -1);
        assert_eq!(to_u32(-1.0), 4294967295);
        assert_eq!(to_i32(2147483648.0), -2147483648);
    }
}

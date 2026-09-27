//! JavaScript number semantics, so the port reproduces the JS engine's arithmetic.
//!
//! JS computes in f64 and rounds to f32 only when it stores into a Float32Array.
//! Transcendental functions must go through this module, never `f64::sin` and friends
//! directly. Measured against node 24 on 200k inputs plus hard cases
//! (crates/core/tests/parity_math.rs, data from tests/parity/math.js): sin, cos, log,
//! log2, log10, atan2 are bit-exact via the ported fdlibm/V8 algorithm in v8math.rs;
//! exp and sqrt (libm) are bit-exact as-is; pow needed one JS-spec special case
//! (see `pow` below) and is then bit-exact.

/// `Math.round`: halves round toward +infinity.
#[inline]
pub fn round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
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

#[inline]
pub fn sin(x: f64) -> f64 {
    crate::v8math::sin(x)
}
#[inline]
pub fn cos(x: f64) -> f64 {
    crate::v8math::cos(x)
}
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}
#[inline]
pub fn log(x: f64) -> f64 {
    crate::v8math::log(x)
}
#[inline]
pub fn log2(x: f64) -> f64 {
    crate::v8math::log2(x)
}
#[inline]
pub fn log10(x: f64) -> f64 {
    crate::v8math::log10(x)
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
#[inline]
pub fn tanh(x: f64) -> f64 {
    x.tanh()
}
#[inline]
pub fn sinh(x: f64) -> f64 {
    libm::sinh(x)
}
#[inline]
pub fn cosh(x: f64) -> f64 {
    libm::cosh(x)
}
#[inline]
pub fn expm1(x: f64) -> f64 {
    libm::expm1(x)
}

/// `Math.max` over two values, with the JS NaN rule.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min` over two values, with the JS NaN rule.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
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
    }

    #[test]
    fn int_conversions_match_js() {
        assert_eq!(to_i32(4294967296.0 + 5.0), 5);
        assert_eq!(to_i32(-1.5), -1);
        assert_eq!(to_u32(-1.0), 4294967295);
        assert_eq!(to_i32(2147483648.0), -2147483648);
    }
}

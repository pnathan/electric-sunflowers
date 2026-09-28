//! Time: seconds to sample index, one rounding rule for the whole engine (`sample_at(t) = round(t * SR)`).
//!
//! Every conversion from seconds to a sample position goes through
//! `sample_at`, so two events at the same time land on the same sample.

use crate::SR_F;

/// Sample index of time `t` seconds: `round(t * SR)`, halves away from
/// zero (`f64::round`). Negative times give negative indices (the caller
/// clips). NaN gives 0; out-of-range values saturate.
#[inline]
pub fn sample_at(t: f64) -> isize {
    (t * SR_F).round() as isize
}

/// Buffer length in samples that holds everything up to `t_end` seconds:
/// `ceil(t_end * SR)`. Negative or NaN input gives 0.
#[inline]
pub fn len_samples(t_end: f64) -> usize {
    (t_end * SR_F).ceil() as usize
}

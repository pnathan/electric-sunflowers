//! Old filter entry points, kept as thin shims over `biquad` so callers
//! compile unchanged. New code uses `biquad::{BiquadCoeffs, Biquad, EqBand}`.

use crate::biquad::{Biquad, BiquadCoeffs, EqBand, EqKind};
use sfcore::SR_F;

/// shim: deleted in wave 5
#[doc(hidden)]
pub type BqCoeffs = BiquadCoeffs;

/// shim: deleted in wave 5
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterType {
    Lp,
    Hp,
    Bp,
    Hs,
    Ls,
    Pk,
}

impl From<FilterType> for EqKind {
    fn from(t: FilterType) -> Self {
        match t {
            FilterType::Lp => EqKind::Lowpass,
            FilterType::Hp => EqKind::Highpass,
            FilterType::Bp => EqKind::Bandpass,
            FilterType::Hs => EqKind::HighShelf,
            FilterType::Ls => EqKind::LowShelf,
            FilterType::Pk => EqKind::Peaking,
        }
    }
}

/// shim: deleted in wave 5. Cookbook design at `sfcore::SR_F`.
#[doc(hidden)]
pub fn bq(ty: FilterType, f: f64, q: f64, g: f64) -> BqCoeffs {
    EqBand { kind: ty.into(), f, q, db: g }.design(SR_F)
}

/// shim: deleted in wave 5. Filter in place from zero state.
#[doc(hidden)]
pub fn run_bq(x: &mut [f32], c: &BqCoeffs) {
    Biquad::new(*c).process(x);
}

/// shim: deleted in wave 5. Filter `x` into `out` from zero state; the
/// shorter of the two lengths is processed.
#[doc(hidden)]
pub fn run_bq_into(x: &[f32], c: &BqCoeffs, out: &mut [f32]) {
    let n = x.len().min(out.len());
    out[..n].copy_from_slice(&x[..n]);
    run_bq(&mut out[..n], c);
}

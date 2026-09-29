//! Melodic contours: the target line a phrase follows, as a pitch offset
//! over the phrase position x in [0, 1].

use std::f64::consts::PI;

/// Named contour shapes. Each maps x in [0, 1] and amplitude a (semitones)
/// to an offset from the section's centre pitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContourKind {
    /// a sin(pi x): up and back.
    Arch,
    /// a (0.9 - 1.8 x): falling.
    Descent,
    /// a (-0.7 + 1.5 x): rising.
    Rise,
    /// -0.8 a sin(pi x) + 0.3 a: down and back.
    Valley,
    /// 0.8 a sin(2 pi x): up, down, up.
    Wave,
    /// a sin(pi x^1.8): peak near x = 0.68.
    PeakLate,
    /// a sin(pi x^0.55): peak near x = 0.28.
    PeakEarly,
    /// a sin(0.7 pi x): rises to x = 0.71 and ends high; the default for
    /// a closing phrase without a profile shape.
    OpenArch,
}

impl ContourKind {
    /// The seven shapes a melody profile draws from, in a fixed order.
    pub const KEYS: [ContourKind; 7] = [
        ContourKind::Arch,
        ContourKind::Descent,
        ContourKind::Rise,
        ContourKind::Valley,
        ContourKind::Wave,
        ContourKind::PeakLate,
        ContourKind::PeakEarly,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ContourKind::Arch => "arch",
            ContourKind::Descent => "descent",
            ContourKind::Rise => "rise",
            ContourKind::Valley => "valley",
            ContourKind::Wave => "wave",
            ContourKind::PeakLate => "peakLate",
            ContourKind::PeakEarly => "peakEarly",
            ContourKind::OpenArch => "openArch",
        }
    }

    /// Offset at `x` for amplitude `a`.
    pub fn apply(self, x: f64, a: f64) -> f64 {
        match self {
            ContourKind::Arch => a * (PI * x).sin(),
            ContourKind::Descent => a * (0.9 - 1.8 * x),
            ContourKind::Rise => a * (-0.7 + 1.5 * x),
            ContourKind::Valley => -a * 0.8 * (PI * x).sin() + a * 0.3,
            ContourKind::Wave => a * 0.8 * (2.0 * PI * x).sin(),
            ContourKind::PeakLate => a * (PI * x.powf(1.8)).sin(),
            ContourKind::PeakEarly => a * (PI * x.powf(0.55)).sin(),
            ContourKind::OpenArch => a * (0.7 * PI * x).sin(),
        }
    }
}

/// A phrase contour: `gain * kind(x, amp) + slope * x` semitones.
/// The slope tilts the phrase: negative into a tonic cadence, positive on
/// answering (odd) lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contour {
    pub kind: ContourKind,
    pub amp: f64,
    pub gain: f64,
    pub slope: f64,
}

/// Gain of a profile shape under a tonic cadence.
const TONIC_GAIN: f64 = 0.6;
/// Gain of the default shape under a tonic cadence.
const TONIC_DEFAULT_GAIN: f64 = 0.8;
/// Fall into a tonic cadence, in units of the amplitude over the phrase.
const TONIC_FALL: f64 = 1.2;
/// Rise of an answering (odd) line over the phrase, semitones.
const ANSWER_RISE: f64 = 0.8;

impl Contour {
    /// The contour of phrase `li`: `shape` from the melody profile, or an
    /// arch when there is none. A tonic cadence damps the shape and falls
    /// `TONIC_FALL a` over the phrase; otherwise odd phrases rise
    /// `ANSWER_RISE` semitones.
    pub fn for_phrase(
        li: usize,
        tonic_cadence: bool,
        amp: f64,
        shape: Option<ContourKind>,
    ) -> Contour {
        if tonic_cadence {
            let (kind, gain) = match shape {
                Some(k) => (k, TONIC_GAIN),
                None => (ContourKind::OpenArch, TONIC_DEFAULT_GAIN),
            };
            return Contour {
                kind,
                amp,
                gain,
                slope: -TONIC_FALL * amp,
            };
        }
        let slope = if li % 2 == 1 { ANSWER_RISE } else { 0.0 };
        Contour {
            kind: shape.unwrap_or(ContourKind::Arch),
            amp,
            gain: 1.0,
            slope,
        }
    }

    /// Offset in semitones at phrase position `x` in [0, 1].
    #[inline]
    pub fn at(&self, x: f64) -> f64 {
        self.gain * self.kind.apply(x, self.amp) + self.slope * x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert!(ContourKind::Arch.apply(0.5, 2.0) > 1.99);
        assert!(ContourKind::Descent.apply(1.0, 1.0) < 0.0);
        assert!(ContourKind::PeakLate.apply(0.7, 1.0) > 0.99);
        assert!(ContourKind::PeakEarly.apply(0.3, 1.0) > 0.99);
        let c = Contour::for_phrase(0, true, 3.0, None);
        assert!(c.at(1.0) < c.at(0.0));
        let c = Contour::for_phrase(1, false, 3.0, Some(ContourKind::Rise));
        assert!((c.at(1.0) - (3.0 * 0.8 + 0.8)).abs() < 1e-12);
    }
}

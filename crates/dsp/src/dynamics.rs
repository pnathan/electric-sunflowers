//! Feed-forward compressor (design section 5.11).
//!
//! - `PeakDetector`: branching one-pole on |x| (Giannoulis, Massberg, Reiss,
//!   "Digital Dynamic Range Compressor Design - A Tutorial and Analysis",
//!   JAES 60(6), 2012, the "branching" peak detector, applied to the
//!   linear level). Coefficient exp(-1 / (t fs)) for attack and release.
//! - `GainComputer`: static curve with a quadratic soft knee (same paper,
//!   eq. 4), as gain reduction in dB for an input level in dB. Threshold,
//!   ratio and knee width set the curve.
//! - `Compressor`: the detector runs every sample; every `GAIN_PERIOD` (16)
//!   samples the gain computer turns the detector level into a gain, and the
//!   gain is interpolated linearly (in linear gain) across the period that
//!   produced it, so there are no zipper steps and no added lag. `Link`
//!   selects mono or stereo linking by max(|L|, |R|).
//!
//! Level in dB and gain in linear come from `sfcore::math::gain_to_db` and
//! `db_to_gain`; silence reads `DB_FLOOR` (-200 dB).

use sfcore::math::{db_to_gain, gain_to_db, one_pole_coeff_tau};

/// Samples between gain-computer updates.
pub const GAIN_PERIOD: usize = 16;

/// shim: deleted in wave 5 with its callers in mix.rs and the stems example.
/// Same as `sfcore::math::gain_to_db`.
#[doc(hidden)]
#[inline]
pub fn db_of(v: f64) -> f64 {
    gain_to_db(v)
}

/// Pole k = exp(-1 / (t fs)) = 1 - `one_pole_coeff_tau`; t <= 0 or
/// non-finite gives 0 (instant).
fn pole(t: f64, fs: f64) -> f64 {
    1.0 - one_pole_coeff_tau(if t.is_finite() { t } else { 0.0 }, fs)
}

/// Branching one-pole envelope follower on the rectified signal.
#[derive(Clone, Copy, Debug, Default)]
pub struct PeakDetector {
    pub attack_coef: f64,
    pub release_coef: f64,
    pub env: f64,
}

impl PeakDetector {
    /// Attack and release time constants in seconds.
    pub fn new(attack: f64, release: f64, fs: f64) -> Self {
        PeakDetector { attack_coef: pole(attack, fs), release_coef: pole(release, fs), env: 0.0 }
    }

    /// Feed |x| (already rectified); returns the envelope.
    #[inline(always)]
    pub fn tick(&mut self, a: f64) -> f64 {
        let k = if a > self.env { self.attack_coef } else { self.release_coef };
        self.env = k * self.env + (1.0 - k) * a;
        self.env
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
    }
}

/// Static compression curve with a quadratic soft knee.
#[derive(Clone, Copy, Debug)]
pub struct GainComputer {
    pub thr_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
}

impl GainComputer {
    /// Gain in dB (<= 0) for input level `level_db`. With o = level - thr
    /// and s = 1 - 1/ratio: 0 below the knee, -s (o + W/2)^2 / (2 W) inside
    /// it, -s o above. Ratio below 1 is treated as 1; knee <= 0 is hard.
    pub fn gain_db(&self, level_db: f64) -> f64 {
        let s = 1.0 - 1.0 / self.ratio.max(1.0);
        let w = self.knee_db.max(0.0);
        let o = level_db - self.thr_db;
        if 2.0 * o > w {
            -s * o
        } else if w > 0.0 && 2.0 * o > -w {
            let q = o + w / 2.0;
            -s * q * q / (2.0 * w)
        } else {
            0.0
        }
    }
}

/// Channel linking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// One channel, detector on |x|.
    Mono,
    /// Two channels, one detector on max(|L|, |R|), one gain for both.
    StereoMax,
}

/// Feed-forward compressor: detector, gain computer, interpolated gain.
#[derive(Clone, Copy, Debug)]
pub struct Compressor {
    pub det: PeakDetector,
    pub gc: GainComputer,
    pub link: Link,
    /// Linear gain at the end of the last period.
    gain: f64,
}

impl Compressor {
    pub fn new(det: PeakDetector, gc: GainComputer, link: Link) -> Self {
        Compressor { det, gc, link, gain: 1.0 }
    }

    /// Detector level of the last sample fed, in dB.
    pub fn level_db(&self) -> f64 {
        gain_to_db(self.det.env)
    }

    fn period_gain(&self) -> f64 {
        db_to_gain(self.gc.gain_db(self.level_db()))
    }

    pub fn reset(&mut self) {
        self.det.reset();
        self.gain = 1.0;
    }

    /// Compress one channel in place (`link` is ignored).
    pub fn process_mono(&mut self, x: &mut [f32]) {
        for chunk in x.chunks_mut(GAIN_PERIOD) {
            for v in chunk.iter() {
                self.det.tick((*v as f64).abs());
            }
            let (g0, g1) = (self.gain, self.period_gain());
            let d = (g1 - g0) / chunk.len() as f64;
            for (j, v) in chunk.iter_mut().enumerate() {
                *v = (*v as f64 * (g0 + d * (j + 1) as f64)) as f32;
            }
            self.gain = g1;
        }
    }

    /// Compress two channels in place with one linked gain (max of |L|, |R|).
    /// Lengths may differ; the shorter one bounds the linked part and the
    /// rest of the longer channel is left unchanged.
    pub fn process_stereo(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        for (cl, cr) in l[..n].chunks_mut(GAIN_PERIOD).zip(r[..n].chunks_mut(GAIN_PERIOD)) {
            for (a, b) in cl.iter().zip(cr.iter()) {
                self.det.tick((*a as f64).abs().max((*b as f64).abs()));
            }
            let (g0, g1) = (self.gain, self.period_gain());
            let d = (g1 - g0) / cl.len() as f64;
            for (j, (a, b)) in cl.iter_mut().zip(cr.iter_mut()).enumerate() {
                let g = g0 + d * (j + 1) as f64;
                *a = (*a as f64 * g) as f32;
                *b = (*b as f64 * g) as f32;
            }
            self.gain = g1;
        }
    }
}

/// shim: deleted in wave 5. Mono compressor at `sfcore::SR_F`; knee None
/// (or non-positive, or non-finite) means 6 dB.
#[doc(hidden)]
pub fn compress(x: &mut [f32], thr_db: f64, ratio: f64, atk: f64, rel: f64, knee: Option<f64>) {
    let knee_db = knee.filter(|k| k.is_finite() && *k > 0.0).unwrap_or(6.0);
    let det = PeakDetector::new(atk, rel, sfcore::SR_F);
    Compressor::new(det, GainComputer { thr_db, ratio, knee_db }, Link::Mono).process_mono(x);
}

/// shim: deleted in wave 5. Stereo-linked compressor at `sfcore::SR_F`, 10 dB knee.
#[doc(hidden)]
pub fn stereo_compress(l: &mut [f32], r: &mut [f32], thr_db: f64, ratio: f64, atk: f64, rel: f64) {
    let det = PeakDetector::new(atk, rel, sfcore::SR_F);
    Compressor::new(det, GainComputer { thr_db, ratio, knee_db: 10.0 }, Link::StereoMax).process_stereo(l, r);
}

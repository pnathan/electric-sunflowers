//! Long-term average spectrum in 1/3-octave bands, plus gated level
//! statistics, for one multichannel signal.
//!
//! Method:
//! - Activity gate: 2048-sample blocks; a block's level is the RMS of the
//!   mean channel power. A block is active when its level exceeds 5% of the
//!   loudest block (the rule of the engine's `active_rms`).
//! - Spectrum: Welch periodogram (Welch 1967), Hann window, 8192 points,
//!   50% overlap (hop 4096). A frame covers four blocks and enters the
//!   average when at least two of them are active. Channel powers are summed.
//! - Bands: the 29 base-10 1/3-octave bands of IEC 61260-1, exact centres
//!   1000 * 10^(n/10) for n = -16..12 (nominal 25 Hz to 16 kHz), edges at
//!   centre * 10^(+-1/20). A bin belongs to the band that holds its frequency.
//! - Each band is reported in dB relative to the total power of the averaged
//!   spectrum (all bins), floored at `FLOOR_DB`. `compare` clamps both
//!   sides at `compare::REL_FLOOR_DB` (-60 dB) before it takes a delta, so
//!   bands far below the file's power do not fail the gate.

use crate::fft::Fft;
use serde::{Deserialize, Serialize};

pub const BLOCK: usize = 2048;
pub const FRAME: usize = 8192;
pub const HOP: usize = FRAME / 2;
/// Band levels below this, relative to total active power, are reported as
/// this value: the measurement floor.
pub const FLOOR_DB: f64 = -90.0;
/// Level reported for a file with no active signal, in dBFS.
pub const SILENT_DBFS: f64 = -200.0;

/// Nominal IEC 61260 centre frequencies, 25 Hz to 16 kHz.
pub const NOMINAL_HZ: [f64; 29] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0,
    630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
    10000.0, 12500.0, 16000.0,
];

/// Exact base-10 centre of band `i` (index into `NOMINAL_HZ`).
pub fn exact_centre(i: usize) -> f64 {
    1000.0 * 10f64.powf((i as f64 - 16.0) / 10.0)
}

/// Result for one file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ltas {
    /// Band levels in dB relative to total active power, one per `NOMINAL_HZ`.
    pub bands_db: Vec<f64>,
    /// RMS over active blocks, dBFS (mean channel power).
    pub gated_rms_dbfs: f64,
    /// Active blocks / all blocks.
    pub active_fraction: f64,
    /// Largest absolute sample value over all channels.
    pub peak: f64,
    /// Count of NaN and infinite samples (analysed as zero).
    pub nonfinite: u64,
    pub channels: usize,
    pub frames: usize,
    pub sample_rate: u32,
}

/// Per-block mean channel power; non-finite samples count as zero.
fn block_powers(chs: &[Vec<f64>]) -> Vec<f64> {
    let n = chs.first().map_or(0, |c| c.len());
    let nch = chs.len().max(1) as f64;
    let mut out = Vec::with_capacity(n.div_ceil(BLOCK));
    let mut i = 0;
    while i < n {
        let e = (i + BLOCK).min(n);
        let mut s = 0.0;
        for c in chs {
            for &v in &c[i..e] {
                if v.is_finite() {
                    s += v * v;
                }
            }
        }
        out.push(s / ((e - i) as f64 * nch));
        i += BLOCK;
    }
    out
}

/// Analyses one signal. `chs` holds equal-length channels.
pub fn analyse(chs: &[Vec<f64>], sample_rate: u32) -> Ltas {
    let n = chs.first().map_or(0, |c| c.len());
    let mut peak = 0.0f64;
    let mut nonfinite = 0u64;
    for c in chs {
        for &v in c {
            if v.is_finite() {
                peak = peak.max(v.abs());
            } else {
                nonfinite += 1;
            }
        }
    }

    let bp = block_powers(chs);
    let loudest = bp.iter().fold(0.0f64, |m, &p| m.max(p.sqrt()));
    let active: Vec<bool> = bp
        .iter()
        .map(|&p| loudest > 0.0 && p.sqrt() > 0.05 * loudest)
        .collect();
    let n_active = active.iter().filter(|&&a| a).count();
    let active_fraction = if bp.is_empty() {
        0.0
    } else {
        n_active as f64 / bp.len() as f64
    };
    let gated_ms = if n_active == 0 {
        0.0
    } else {
        bp.iter()
            .zip(&active)
            .filter(|(_, &a)| a)
            .map(|(&p, _)| p)
            .sum::<f64>()
            / n_active as f64
    };
    let gated_rms_dbfs = if gated_ms > 0.0 {
        10.0 * gated_ms.log10()
    } else {
        SILENT_DBFS
    };

    // Welch average over admitted frames.
    let fft = Fft::new(FRAME).expect("FRAME is a power of two");
    let win: Vec<f64> = (0..FRAME)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / FRAME as f64).cos())
        .collect();
    let nb = FRAME / 2 + 1;
    let mut acc = vec![0.0f64; nb];
    let mut re = vec![0.0f64; FRAME];
    let mut im = vec![0.0f64; FRAME];
    let mut used = 0usize;
    let mut s = 0usize;
    loop {
        let b0 = s / BLOCK;
        let covered = &active[b0.min(active.len())..(b0 + FRAME / BLOCK).min(active.len())];
        if covered.iter().filter(|&&a| a).count() >= 2 {
            for c in chs {
                for i in 0..FRAME {
                    let v = c.get(s + i).copied().unwrap_or(0.0);
                    re[i] = if v.is_finite() { v * win[i] } else { 0.0 };
                    im[i] = 0.0;
                }
                fft.run(&mut re, &mut im, false);
                for k in 0..nb {
                    acc[k] += re[k] * re[k] + im[k] * im[k];
                }
            }
            used += 1;
        }
        if s + FRAME >= n {
            break;
        }
        s += HOP;
    }

    let total: f64 = acc.iter().sum();
    let bin_hz = sample_rate as f64 / FRAME as f64;
    let mut band_pow = vec![0.0f64; NOMINAL_HZ.len()];
    let edge = 10f64.powf(0.05);
    for (k, &p) in acc.iter().enumerate() {
        let f = k as f64 * bin_hz;
        for (i, bpow) in band_pow.iter_mut().enumerate() {
            let c = exact_centre(i);
            if f >= c / edge && f < c * edge {
                *bpow += p;
                break;
            }
        }
    }
    let bands_db = band_pow
        .iter()
        .map(|&p| {
            if used > 0 && total > 0.0 && p > 0.0 {
                (10.0 * (p / total).log10()).max(FLOOR_DB)
            } else {
                FLOOR_DB
            }
        })
        .collect();

    Ltas {
        bands_db,
        gated_rms_dbfs,
        active_fraction,
        peak,
        nonfinite,
        channels: chs.len(),
        frames: n,
        sample_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_1k_lands_in_1k_band() {
        let sr = 44100u32;
        let n = sr as usize * 3;
        let x: Vec<f64> = (0..n)
            .map(|i| 0.5 * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / sr as f64).sin())
            .collect();
        let l = analyse(&[x], sr);
        let i1k = NOMINAL_HZ.iter().position(|&f| f == 1000.0).unwrap();
        let frac = 10f64.powf(l.bands_db[i1k] / 10.0);
        assert!(frac > 0.99, "1 kHz band holds {frac}");
        assert!((l.active_fraction - 1.0).abs() < 1e-12);
        assert!((l.peak - 0.5).abs() < 1e-3);
        // RMS of a 0.5 sine: 0.5/sqrt(2), -9.03 dBFS
        assert!((l.gated_rms_dbfs - 20.0 * (0.5f64 / 2f64.sqrt()).log10()).abs() < 0.01);
    }

    #[test]
    fn silence_is_floored_not_nan() {
        let l = analyse(&[vec![0.0; 50000]], 44100);
        assert_eq!(l.active_fraction, 0.0);
        assert!(l.bands_db.iter().all(|&b| b == FLOOR_DB));
        assert_eq!(l.gated_rms_dbfs, SILENT_DBFS);
    }

    #[test]
    fn nonfinite_counted() {
        let mut x = vec![0.1; 10000];
        x[5] = f64::NAN;
        x[7] = f64::INFINITY;
        let l = analyse(&[x], 44100);
        assert_eq!(l.nonfinite, 2);
        assert!(l.bands_db.iter().all(|b| b.is_finite()));
    }
}

//! The channel strip (design sections 3.6 and 5.13): per channel, one pass
//! over the present stem blocks runs the EQ cascade (`dsp::biquad`, TDF-II,
//! `f64` state) and, in the same loop, the 2048-sample sums of squares for
//! the gated loudness. Then the lead and harmony compressor, then the
//! lead's slapback.
//!
//! - Gated loudness (`active_rms`): RMS over the 2048-sample blocks whose
//!   RMS exceeds 5% of the loudest block; the louder channel counts. Kept
//!   unweighted (design section 11). A track under 1e-6 is silent and
//!   yields no stem.
//! - Level: `TARGET_RMS / gated_rms`. It is not applied to the audio; the
//!   mixer folds it into the pan gain. The compressor therefore runs on the
//!   unscaled audio with its threshold moved by `-20 log10(level)`: the
//!   peak detector is linear and the gain computer works in dB, so
//!   compressing then scaling equals scaling then compressing.
//! - Slapback: feedback comb over the compressed lead,
//!   `y[n] = LP(x[n - d] + 0.22 y[n - d])` with d = round(0.34 s) and a
//!   3.2 kHz RBJ low-pass, stored times `0.07 level`.

use dsp::biquad::{Biquad, BiquadCoeffs};
use dsp::dynamics::{Compressor, GainComputer, Link, PeakDetector};
use sfcore::math::{db_to_gain, gain_to_db};
use sfcore::SR_F;

use crate::stem::{process_sparse, Process, SparseBuf, Stem, RING_FLOOR, STEM_BLOCK};
use crate::track::{CompSpec, Slapback, Strip, TARGET_RMS};

/// Samples per loudness block.
pub const RMS_BLOCK: usize = 2048;
/// A loudness block counts when its RMS exceeds this fraction of the loudest.
pub const RMS_GATE: f64 = 0.05;
/// Gated RMS below which a track is silent.
pub const SILENT_RMS: f64 = 1e-6;
/// Most EQ bands on one strip.
pub const MAX_EQ: usize = 4;

/// A track after its strip: EQ and compression applied; `level` is applied
/// by the mixer (and by anything that reads the stem at its mixed level).
/// `slap` is the strip's slapback, level and slap gain already applied, as
/// the mixer adds it to the buses (design section 3.3).
#[derive(Clone, Debug)]
pub struct ProcessedStem {
    pub audio: Stem,
    pub level: f32,
    pub slap: Option<SparseBuf>,
}

/// The strip's EQ as a fixed array of sections.
struct EqChain {
    st: [Biquad; MAX_EQ],
    n: usize,
}

impl EqChain {
    fn new(strip: &Strip) -> Self {
        let mut st = [Biquad::new(BiquadCoeffs::IDENTITY); MAX_EQ];
        let n = strip.eq.len().min(MAX_EQ);
        debug_assert!(strip.eq.len() <= MAX_EQ);
        for (s, band) in st.iter_mut().zip(&strip.eq[..n]) {
            *s = Biquad::new(band.design(SR_F));
        }
        EqChain { st, n }
    }
}

impl Process for EqChain {
    fn process(&mut self, buf: &mut [f32]) {
        for s in self.st[..self.n].iter_mut() {
            s.process(buf);
            s.flush_denormals();
        }
    }

    fn reset(&mut self) {
        self.st.iter_mut().for_each(Biquad::reset);
    }
}

/// EQ one channel in place; returns the sums of squares per `RMS_BLOCK`.
fn eq_channel(buf: &mut SparseBuf, strip: &Strip) -> Vec<f64> {
    let mut sums = vec![0.0f64; buf.len().div_ceil(RMS_BLOCK)];
    let mut eq = EqChain::new(strip);
    process_sparse(buf, &mut eq, |b, x| {
        let k0 = b * (STEM_BLOCK / RMS_BLOCK);
        for (j, c) in x.chunks(RMS_BLOCK).enumerate() {
            sums[k0 + j] += c.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>();
        }
    });
    sums
}

/// Gated RMS from per-block sums of squares over `len` samples: RMS of the
/// blocks above `RMS_GATE` of the loudest block; 0 when all are silent.
pub fn active_rms(sums: &[f64], len: usize) -> f64 {
    let block_rms = |k: usize| {
        let n = len.saturating_sub(k * RMS_BLOCK).clamp(1, RMS_BLOCK);
        (sums[k] / n as f64).sqrt()
    };
    let mx = (0..sums.len()).map(block_rms).fold(0.0, f64::max);
    let (mut e, mut c) = (0.0f64, 0usize);
    for k in 0..sums.len() {
        let v = block_rms(k);
        if v > mx * RMS_GATE {
            e += v * v;
            c += 1;
        }
    }
    if c == 0 {
        0.0
    } else {
        (e / c as f64).sqrt()
    }
}

/// Gated RMS of a dense buffer (for tests and tools).
pub fn active_rms_dense(x: &[f32]) -> f64 {
    let sums: Vec<f64> = x.chunks(RMS_BLOCK).map(|c| c.iter().map(|&v| (v as f64) * (v as f64)).sum()).collect();
    active_rms(&sums, x.len())
}

/// The compressor of `spec` for audio that the mixer scales by `level`.
pub fn compressor(spec: &CompSpec, level: f64) -> Compressor {
    let thr_db = gain_to_db(TARGET_RMS) + spec.above_target_db - gain_to_db(level);
    Compressor::new(
        PeakDetector::new(spec.attack, spec.release, SR_F),
        GainComputer { thr_db, ratio: spec.ratio, knee_db: spec.knee_db },
        Link::Mono,
    )
}

/// Compress one channel in place. Absent blocks after signal feed zeros to
/// the detector (so release and gain recovery run as on a dense buffer)
/// until the detector is 60 dB under the threshold; then the compressor is
/// reset (its gain is unity there) and absent blocks are skipped.
fn compress_channel(buf: &mut SparseBuf, mut comp: Compressor, zeros: &mut [f32]) {
    let floor = db_to_gain(comp.gc.thr_db - 60.0);
    let mut active = false;
    for b in 0..buf.block_count() {
        if let Some(x) = buf.block_mut_if_present(b) {
            comp.process_mono(x);
            active = true;
        } else if active {
            let n = buf.block_len(b);
            comp.process_mono(&mut zeros[..n]);
            if comp.det.env < floor {
                comp.reset();
                active = false;
            }
        }
    }
}

/// The slapback of mono `x` (already compressed, unscaled), times `gain`.
pub fn slapback(x: &SparseBuf, s: &Slapback, gain: f32) -> SparseBuf {
    let len = x.len();
    let d = (s.delay_s * SR_F).round() as usize;
    debug_assert!(d >= STEM_BLOCK, "the comb reads only finished blocks");
    let mut out = SparseBuf::new(len);
    let mut lp = Biquad::new(BiquadCoeffs::lowpass(SR_F, s.lp_hz, s.lp_q));
    let mut vx = vec![0.0f32; STEM_BLOCK];
    let mut vo = vec![0.0f32; STEM_BLOCK];
    let mut ringing = false;
    for b in 0..out.block_count() {
        let (start, n) = (b * STEM_BLOCK, out.block_len(b));
        if start + n <= d {
            continue;
        }
        let at = start as isize - d as isize;
        x.read_into(at, &mut vx[..n]);
        out.read_into(at, &mut vo[..n]);
        let quiet = vx[..n].iter().chain(&vo[..n]).all(|&v| v == 0.0);
        if quiet && !ringing {
            continue;
        }
        let fb = s.feedback;
        let y = out.block_mut(b);
        let mut ms = 0.0f64;
        for ((o, &a), &c) in y.iter_mut().zip(&vx[..n]).zip(&vo[..n]) {
            let v = lp.tick(a as f64 + fb * c as f64);
            ms += v * v;
            *o = v as f32;
        }
        lp.flush_denormals();
        ringing = !quiet || ms / n as f64 >= RING_FLOOR;
        if !ringing {
            lp.reset();
        }
    }
    out.scale(gain);
    out
}

/// Runs `strip` over `audio`. Returns `None` for a silent track, else the
/// processed stem, its `slap` buffer set for a strip with a slapback.
pub fn run_strip(strip: &Strip, mut audio: Stem) -> Option<ProcessedStem> {
    let len = audio.len();
    let rms = match &mut audio {
        Stem::Mono(x) => active_rms(&eq_channel(x, strip), len),
        Stem::Stereo([l, r]) => {
            let (sl, sr) = rayon::join(|| eq_channel(l, strip), || eq_channel(r, strip));
            active_rms(&sl, len).max(active_rms(&sr, len))
        }
    };
    if rms.is_nan() || rms < SILENT_RMS {
        return None;
    }
    let level = TARGET_RMS / rms;
    if let Some(spec) = &strip.comp {
        let mut zeros = vec![0.0f32; STEM_BLOCK];
        for ch in audio.channels_mut() {
            compress_channel(ch, compressor(spec, level), &mut zeros);
        }
    }
    let slap = match (&strip.slapback, &audio) {
        (Some(s), Stem::Mono(x)) => Some(slapback(x, s, s.level * level as f32)),
        _ => None,
    };
    Some(ProcessedStem { audio, level: level as f32, slap })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{TrackId, VOCAL_COMP};

    #[test]
    fn gated_rms_ignores_silence() {
        let mut x = vec![0.0f32; 20 * RMS_BLOCK];
        for (i, v) in x[..4 * RMS_BLOCK].iter_mut().enumerate() {
            *v = 0.5 * (i as f32 * 0.1).sin();
        }
        let r = active_rms_dense(&x);
        assert!((r - 0.5 / 2f64.sqrt()).abs() < 1e-3, "{r}");
    }

    #[test]
    fn compressor_threshold_shift_equals_scaling_first() {
        let level = 3.7f64;
        let mut x: Vec<f32> = (0..20_000).map(|i| (0.2 * (i as f32 * 0.03).sin()) * if i > 8000 { 1.0 } else { 0.1 }).collect();
        let mut y: Vec<f32> = x.iter().map(|&v| (v as f64 * level) as f32).collect();
        compressor(&VOCAL_COMP, level).process_mono(&mut x);
        compressor(&VOCAL_COMP, 1.0).process_mono(&mut y);
        let err = x.iter().zip(&y).map(|(a, b)| ((*a as f64 * level) - *b as f64).abs()).fold(0.0, f64::max);
        assert!(err < 1e-5, "{err}");
    }

    #[test]
    fn silent_track_has_no_stem() {
        let strip = TrackId::Bass.strip();
        assert!(run_strip(strip, Stem::Mono(SparseBuf::new(50_000))).is_none());
    }

    #[test]
    fn lead_strip_levels_and_slaps() {
        let n = 60_000;
        let x: Vec<f32> = (0..n).map(|i| if (5000..30_000).contains(&i) { 0.3 * (i as f32 * 0.05).sin() } else { 0.0 }).collect();
        let p = run_strip(TrackId::Lead.strip(), Stem::Mono(SparseBuf::from_dense(&x))).expect("not silent");
        let Stem::Mono(y) = &p.audio else { panic!("lead is mono") };
        let scaled: Vec<f32> = y.to_dense().iter().map(|v| v * p.level).collect();
        let r = active_rms_dense(&scaled);
        assert!(r > 0.05 && r < 0.12, "{r}");
        let s = p.slap.expect("lead has a slapback").to_dense();
        let d = (0.34 * SR_F).round() as usize;
        assert!(s[..d].iter().all(|&v| v == 0.0));
        assert!(s[d + 5000..d + 30_000].iter().any(|&v| v.abs() > 1e-3));
    }
}

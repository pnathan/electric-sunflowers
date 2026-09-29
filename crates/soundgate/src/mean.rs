//! Take-robust comparison: statistics over several seeds (takes) of the
//! same song, compared between two engines.
//!
//! Each seed renders a different take (other melody notes, other
//! arrangement detail), so a per-seed LTAS delta between two engines mixes
//! the engine change with the take change. Here each stem is summarised by
//! the mean over seeds of its 1/3-octave LTAS (each seed relative to its own
//! active power, clamped at `REL_FLOOR_DB`, averaged in dB), with the
//! seed-to-seed sample standard deviation per band, and two such summaries
//! are compared.
//!
//! Band rule (`BandTol`): a band passes when |mean_new - mean_base| is
//! within the fixed tolerance (3 dB for 100 Hz-10 kHz, 6 dB elsewhere, the
//! "about right" limits of docs/engine-design.md section 12) OR within
//! `k` (2) times the base engine's own seed-to-seed std in that band, the
//! latter capped at `cap_mid_db` (6 dB, 100 Hz-10 kHz) and `cap_edge_db`
//! (9 dB elsewhere): allowed = min(max(fixed, k sd), cap).
//! Reasoning for 2 sd: for one engine, the mean of N takes has standard
//! error s/sqrt(N), so the difference of two N-take means of the same
//! engine has std s*sqrt(2/N) (s/2 for N = 8). A 2s allowance is then about
//! 4 standard errors of the null difference; with ~300 band tests per run
//! the false alarm rate stays near 2%. It is also the natural reading of
//! "about right": a shift smaller than two takes of one engine already
//! differ by is not a change of character. The fixed tolerance keeps
//! stable bands (small s) from failing on sub-audible drift.
//! Reasoning for the cap: the std model assumes takes scatter about one
//! level. Where takes are bimodal (a take either has notes in the band or
//! not), s is large and 2s admits shifts no listener calls "about right".
//! Baseline-mean of wave 3 (8 seeds): harmony 160 Hz s = 9.6 dB (2s = 19.3;
//! takes at -29..-33 and -45..-53 dB), doubles 125 Hz 7.2, guitar 100 Hz
//! 6.5 (5 takes at -30, 3 at -41..-45). 19 of 210 stem bands in 100 Hz-10
//! kHz had 2s > 6 dB; no edge band had 2s > 9 dB (worst 8.3, bass 40 Hz),
//! so the edge cap only bounds future spreads. A robust spread (1.4826
//! times the median absolute deviation) was rejected: on the same bimodal
//! bands it is 14.1 dB on harmony 160 Hz (larger than s) and 0.6 dB on
//! guitar 100 Hz (the 5-take majority), so it neither caps nor measures the
//! take spread. The cap is 2x the fixed tolerance: the widest shift the
//! gate accepts in any band is 6 / 9 dB.
//!
//! Level rules: the mean gated RMS of each file within 1.5 dB (section 12;
//! the largest seed-to-seed std in baseline-mean is 0.48 dB, harmony_guitar, and
//! 0.32 dB on the mix, so the null difference of two 8-take means has std
//! at most 0.24 dB and 1.5 dB holds), the mean active fraction within 15
//! points, every seed's mix peak 0.89 +- 0.002, no NaN/inf. Pitch (YIN,
//! `pitch`): pooled over seeds, the fraction within 50 cents may drop by at
//! most 0.03, and octave errors per seed may rise by at most 1 on average
//! (the per-seed rule allowed +2 on one take; the mean of 8 takes has a
//! third of the take noise).

// See crates/soundgate/src/compare.rs: `!(x <= tol)` must fail the gate on
// a NaN measurement, which `x > tol` would not.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use crate::compare::{
    is_mid, LtasFile, MIX_PEAK, MIX_PEAK_TOL, MIX_RMS_DB, PITCH_FRACTION_DROP, REL_FLOOR_DB,
};
use crate::ltas::NOMINAL_HZ;
use crate::pitch::PitchReport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mean gated RMS limit per file, dB: the per-seed mix limit of section 12.
pub const MEAN_RMS_DB: f64 = MIX_RMS_DB;
/// Default cap of the spread allowance, 100 Hz-10 kHz, dB.
pub const CAP_MID_DB: f64 = 6.0;
/// Default cap of the spread allowance outside 100 Hz-10 kHz, dB.
pub const CAP_EDGE_DB: f64 = 9.0;
pub const MEAN_ACTIVE_POINTS: f64 = 15.0;
pub const OCTAVE_PER_SEED_RISE: f64 = 1.0;

/// Summary of one file (stem or mix) over seeds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileStats {
    pub seeds: usize,
    /// Mean band level, dB re the seed's active power, floored at `REL_FLOOR_DB`.
    pub mean_db: Vec<f64>,
    /// Sample std (N-1) of the band level over seeds, dB.
    pub std_db: Vec<f64>,
    pub gated_rms_mean: f64,
    pub gated_rms_std: f64,
    pub active_mean: f64,
    pub active_std: f64,
    pub peak_min: f64,
    pub peak_max: f64,
    pub nonfinite: u64,
}

/// Lead pitch pooled over seeds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PitchPool {
    pub seeds: usize,
    pub notes_analysed: usize,
    pub within_50c: usize,
    pub fraction_within_50c: f64,
    pub octave_errors: usize,
    pub octave_errors_per_seed: f64,
}

/// Contents of ltas-mean.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeanFile {
    pub centres_hz: Vec<f64>,
    pub seeds: Vec<String>,
    pub files: BTreeMap<String, FileStats>,
    pub pitch: Option<PitchPool>,
}

fn mean_std(x: &[f64]) -> (f64, f64) {
    let n = x.len() as f64;
    let m = x.iter().sum::<f64>() / n;
    let v = if x.len() > 1 {
        x.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (n - 1.0)
    } else {
        0.0
    };
    (m, v.sqrt())
}

/// Summarises per-seed LTAS files (and optional pitch reports, one per
/// seed). A file absent from some seed is summarised over the seeds that
/// hold it.
pub fn summarise(
    labels: Vec<String>,
    runs: &[LtasFile],
    pitch: &[PitchReport],
) -> Result<MeanFile, String> {
    let mut names: Vec<&String> = runs.iter().flat_map(|r| r.files.keys()).collect();
    names.sort();
    names.dedup();
    let mut files = BTreeMap::new();
    for name in names {
        let es: Vec<_> = runs.iter().filter_map(|r| r.files.get(name)).collect();
        if es.iter().any(|e| e.bands_db.len() != NOMINAL_HZ.len()) {
            return Err(format!("{name}: band count"));
        }
        let mut mean_db = Vec::with_capacity(NOMINAL_HZ.len());
        let mut std_db = Vec::with_capacity(NOMINAL_HZ.len());
        for i in 0..NOMINAL_HZ.len() {
            let v: Vec<f64> = es.iter().map(|e| e.bands_db[i].max(REL_FLOOR_DB)).collect();
            let (m, s) = mean_std(&v);
            mean_db.push(m);
            std_db.push(s);
        }
        let (gated_rms_mean, gated_rms_std) =
            mean_std(&es.iter().map(|e| e.gated_rms_dbfs).collect::<Vec<_>>());
        let (active_mean, active_std) =
            mean_std(&es.iter().map(|e| e.active_fraction).collect::<Vec<_>>());
        files.insert(
            name.clone(),
            FileStats {
                seeds: es.len(),
                mean_db,
                std_db,
                gated_rms_mean,
                gated_rms_std,
                active_mean,
                active_std,
                peak_min: es.iter().map(|e| e.peak).fold(f64::INFINITY, f64::min),
                peak_max: es.iter().map(|e| e.peak).fold(0.0, f64::max),
                nonfinite: es.iter().map(|e| e.nonfinite).sum(),
            },
        );
    }
    let pitch = if pitch.is_empty() {
        None
    } else {
        let na: usize = pitch.iter().map(|p| p.notes_analysed).sum();
        let w: usize = pitch.iter().map(|p| p.within_50c).sum();
        let oe: usize = pitch.iter().map(|p| p.octave_errors).sum();
        Some(PitchPool {
            seeds: pitch.len(),
            notes_analysed: na,
            within_50c: w,
            fraction_within_50c: if na > 0 { w as f64 / na as f64 } else { 0.0 },
            octave_errors: oe,
            octave_errors_per_seed: oe as f64 / pitch.len() as f64,
        })
    };
    Ok(MeanFile {
        centres_hz: NOMINAL_HZ.to_vec(),
        seeds: labels,
        files,
        pitch,
    })
}

/// Band tolerance: fixed limits, or `k` times the base seed-to-seed std
/// up to a cap.
#[derive(Clone, Copy, Debug)]
pub struct BandTol {
    pub mid_db: f64,
    pub edge_db: f64,
    pub k: f64,
    /// Cap of the allowance in 100 Hz-10 kHz, dB.
    pub cap_mid_db: f64,
    /// Cap of the allowance in the other bands, dB.
    pub cap_edge_db: f64,
}

impl BandTol {
    /// Allowed |delta| in band `i` given the base std there:
    /// `min(max(fixed, k sd), cap)`, never below the fixed tolerance.
    pub fn allowed(&self, i: usize, base_std: f64) -> f64 {
        let (fixed, cap) = if is_mid(i) {
            (self.mid_db, self.cap_mid_db)
        } else {
            (self.edge_db, self.cap_edge_db)
        };
        fixed.max((self.k * base_std).min(cap))
    }
}

/// Worst band of one file: the band with the largest delta / allowed.
#[derive(Clone, Debug)]
pub struct Worst {
    pub band: usize,
    pub delta: f64,
    pub base_std: f64,
    pub allowed: f64,
}

/// One printed row.
pub struct MeanRow {
    pub file: String,
    pub worst: Option<Worst>,
    pub failed_bands: Vec<usize>,
    pub rms_db: Option<f64>,
    pub active_pts: Option<f64>,
    pub fails: Vec<String>,
}

/// Compares two summaries. A file on one side only fails.
pub fn compare_means(base: &MeanFile, new: &MeanFile, tol: &BandTol) -> Vec<MeanRow> {
    let mut names: Vec<&String> = base.files.keys().chain(new.files.keys()).collect();
    names.sort();
    names.dedup();
    let mut rows = Vec::new();
    for name in names {
        let mut row = MeanRow {
            file: name.clone(),
            worst: None,
            failed_bands: vec![],
            rms_db: None,
            active_pts: None,
            fails: vec![],
        };
        match (base.files.get(name), new.files.get(name)) {
            (Some(b), Some(n)) => {
                if b.mean_db.len() != NOMINAL_HZ.len()
                    || n.mean_db.len() != NOMINAL_HZ.len()
                    || b.std_db.len() != NOMINAL_HZ.len()
                {
                    row.fails.push("band count".into());
                } else {
                    for i in 0..NOMINAL_HZ.len() {
                        let delta = n.mean_db[i] - b.mean_db[i];
                        let allowed = tol.allowed(i, b.std_db[i]);
                        let w = Worst {
                            band: i,
                            delta,
                            base_std: b.std_db[i],
                            allowed,
                        };
                        if row
                            .worst
                            .as_ref()
                            .is_none_or(|o| delta.abs() / allowed > o.delta.abs() / o.allowed)
                        {
                            row.worst = Some(w);
                        }
                        if !(delta.abs() <= allowed) {
                            row.failed_bands.push(i);
                        }
                    }
                    if !row.failed_bands.is_empty() {
                        let hz: Vec<String> = row
                            .failed_bands
                            .iter()
                            .map(|&i| format!("{}", NOMINAL_HZ[i]))
                            .collect();
                        row.fails.push(format!("bands {} Hz", hz.join(",")));
                    }
                }
                let dr = n.gated_rms_mean - b.gated_rms_mean;
                row.rms_db = Some(dr);
                if !(dr.abs() <= MEAN_RMS_DB) {
                    row.fails.push(format!("rms {dr:+.2}dB"));
                }
                let ap = 100.0 * (n.active_mean - b.active_mean);
                row.active_pts = Some(ap);
                if !(ap.abs() <= MEAN_ACTIVE_POINTS) {
                    row.fails.push(format!("active {ap:+.1}pts"));
                }
                if n.nonfinite > 0 {
                    row.fails.push(format!("{} NaN/inf", n.nonfinite));
                }
                if name == "mix"
                    && !((n.peak_min - MIX_PEAK).abs() <= MIX_PEAK_TOL
                        && (n.peak_max - MIX_PEAK).abs() <= MIX_PEAK_TOL)
                {
                    row.fails
                        .push(format!("peak {:.4}-{:.4}", n.peak_min, n.peak_max));
                }
            }
            (Some(_), None) => row.fails.push("missing in new".into()),
            (None, Some(_)) => row.fails.push("missing in base".into()),
            (None, None) => {}
        }
        rows.push(row);
    }
    rows
}

/// Pooled pitch check. Returns failures.
pub fn compare_pitch_pool(base: &PitchPool, new: &PitchPool) -> Vec<String> {
    let mut f = Vec::new();
    if !(new.fraction_within_50c >= base.fraction_within_50c - PITCH_FRACTION_DROP) {
        f.push(format!(
            "fraction {:.3} < {:.3}-{PITCH_FRACTION_DROP}",
            new.fraction_within_50c, base.fraction_within_50c
        ));
    }
    if !(new.octave_errors_per_seed <= base.octave_errors_per_seed + OCTAVE_PER_SEED_RISE) {
        f.push(format!(
            "octave errors/seed {:.2} > {:.2}+{OCTAVE_PER_SEED_RISE}",
            new.octave_errors_per_seed, base.octave_errors_per_seed
        ));
    }
    if new.notes_analysed == 0 {
        f.push("no notes analysed".into());
    }
    f
}

/// Formats the comparison table: per file the worst band (largest
/// |delta|/allowed) with the base spread there.
pub fn mean_table(rows: &[MeanRow]) -> String {
    let mut s = format!(
        "{:<16} {:>7} {:>8} {:>8} {:>8} {:>7} {:>8}  {}\n",
        "file", "worst", "delta", "base sd", "allowed", "rms dB", "act pts", "result"
    );
    let o = |v: Option<f64>, p: usize| v.map_or("-".to_string(), |x| format!("{x:+.p$}"));
    for r in rows {
        let (hz, d, sd, al) = match &r.worst {
            Some(w) => (
                format!("{}", NOMINAL_HZ[w.band]),
                format!("{:+.2}", w.delta),
                format!("{:.2}", w.base_std),
                format!("{:.2}", w.allowed),
            ),
            None => ("-".into(), "-".into(), "-".into(), "-".into()),
        };
        s += &format!(
            "{:<16} {:>7} {:>8} {:>8} {:>8} {:>7} {:>8}  {}\n",
            r.file,
            hz,
            d,
            sd,
            al,
            o(r.rms_db, 2),
            o(r.active_pts, 1),
            if r.fails.is_empty() {
                "PASS".to_string()
            } else {
                format!("FAIL {}", r.fails.join(", "))
            }
        );
    }
    s
}

/// Per-band detail for one file: base mean, base sd, new mean, new sd,
/// delta, allowed.
pub fn band_table(name: &str, b: &FileStats, n: &FileStats, tol: &BandTol) -> String {
    let mut s = format!(
        "-- {name}\n{:>7} {:>8} {:>7} {:>8} {:>7} {:>8} {:>8}\n",
        "Hz", "base", "base sd", "new", "new sd", "delta", "allowed"
    );
    for (i, hz) in NOMINAL_HZ.iter().enumerate() {
        let d = n.mean_db[i] - b.mean_db[i];
        let a = tol.allowed(i, b.std_db[i]);
        s += &format!(
            "{:>7} {:>8.2} {:>7.2} {:>8.2} {:>7.2} {:>+8.2} {:>8.2}{}\n",
            hz,
            b.mean_db[i],
            b.std_db[i],
            n.mean_db[i],
            n.std_db[i],
            d,
            a,
            if d.abs() <= a { "" } else { "  FAIL" }
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ltas::Ltas;

    fn run(level: f64, peak: f64) -> LtasFile {
        let mut files = BTreeMap::new();
        let l = Ltas {
            bands_db: vec![level; NOMINAL_HZ.len()],
            gated_rms_dbfs: -20.0,
            active_fraction: 0.5,
            peak,
            nonfinite: 0,
            channels: 2,
            frames: 1000,
            sample_rate: 44100,
        };
        files.insert("mix".to_string(), l.clone());
        files.insert("lead".to_string(), l);
        LtasFile {
            centres_hz: NOMINAL_HZ.to_vec(),
            files,
        }
    }

    const TOL: BandTol = BandTol {
        mid_db: 3.0,
        edge_db: 6.0,
        k: 2.0,
        cap_mid_db: 6.0,
        cap_edge_db: 9.0,
    };

    #[test]
    fn mean_std_and_floor() {
        let runs = [run(-20.0, 0.89), run(-24.0, 0.89), run(-95.0, 0.89)];
        let m = summarise(vec!["a".into(), "b".into(), "c".into()], &runs, &[]).unwrap();
        let f = &m.files["lead"];
        // -95 clamps to -60: mean of -20, -24, -60
        assert!((f.mean_db[10] - (-104.0 / 3.0)).abs() < 1e-9);
        let (_, s) = mean_std(&[-20.0, -24.0, -60.0]);
        assert!((f.std_db[10] - s).abs() < 1e-9);
        assert_eq!(f.seeds, 3);
    }

    #[test]
    fn spread_widens_the_band_tolerance() {
        // base: bands alternate -20/-30 over seeds, sd 5.77 dB (4 seeds)
        let base = summarise(
            vec![],
            &[
                run(-20.0, 0.89),
                run(-30.0, 0.89),
                run(-20.0, 0.89),
                run(-30.0, 0.89),
            ],
            &[],
        )
        .unwrap();
        // new: mean shifted by 5 dB (-20 vs -25): beyond 3 dB, within 2 sd
        // and under the 6 dB cap
        let new = summarise(vec![], &[run(-20.0, 0.89), run(-20.0, 0.89)], &[]).unwrap();
        assert!(compare_means(&base, &new, &TOL)
            .iter()
            .all(|r| r.fails.is_empty()));
        // stable base (sd 0): the fixed tolerance applies
        let base = summarise(vec![], &[run(-25.0, 0.89), run(-25.0, 0.89)], &[]).unwrap();
        let rows = compare_means(&base, &new, &TOL);
        assert!(rows.iter().all(|r| !r.fails.is_empty()));
        assert_eq!(rows[0].worst.as_ref().unwrap().delta, 5.0);
    }

    #[test]
    fn spread_allowance_is_capped() {
        // mid band 10 (1 kHz), edge band 0 (25 Hz)
        assert!(is_mid(10) && !is_mid(0));
        assert_eq!(TOL.allowed(10, 0.5), 3.0);
        assert_eq!(TOL.allowed(10, 2.0), 4.0);
        assert_eq!(TOL.allowed(10, 9.6), 6.0);
        assert_eq!(TOL.allowed(0, 2.0), 6.0);
        assert_eq!(TOL.allowed(0, 4.0), 8.0);
        assert_eq!(TOL.allowed(0, 20.0), 9.0);
        // a cap under the fixed tolerance never tightens it
        let t = BandTol {
            cap_mid_db: 1.0,
            cap_edge_db: 1.0,
            ..TOL
        };
        assert_eq!(t.allowed(10, 9.6), 3.0);
        // bimodal base: sd 10 dB, a 7 dB shift fails although within 2 sd
        let base = summarise(
            vec![],
            &[
                run(-10.0, 0.89),
                run(-30.0, 0.89),
                run(-10.0, 0.89),
                run(-30.0, 0.89),
            ],
            &[],
        )
        .unwrap();
        let new = summarise(vec![], &[run(-13.0, 0.89), run(-13.0, 0.89)], &[]).unwrap();
        let rows = compare_means(&base, &new, &TOL);
        let lead = rows.iter().find(|r| r.file == "lead").unwrap();
        assert!(!lead.failed_bands.is_empty() && lead.failed_bands.iter().all(|&i| is_mid(i)));
    }

    #[test]
    fn mean_rms_limit_is_1_5_db() {
        let base = summarise(vec![], &[run(-20.0, 0.89)], &[]).unwrap();
        let mut new = base.clone();
        new.files.get_mut("mix").unwrap().gated_rms_mean += 1.4;
        assert!(compare_means(&base, &new, &TOL)
            .iter()
            .all(|r| r.fails.is_empty()));
        new.files.get_mut("mix").unwrap().gated_rms_mean += 0.2;
        let rows = compare_means(&base, &new, &TOL);
        assert!(rows
            .iter()
            .find(|r| r.file == "mix")
            .unwrap()
            .fails
            .iter()
            .any(|f| f.starts_with("rms")));
    }

    #[test]
    fn mix_peak_and_missing_file_fail() {
        let base = summarise(vec![], &[run(-20.0, 0.89)], &[]).unwrap();
        let new = summarise(vec![], &[run(-20.0, 0.89), run(-20.0, 0.85)], &[]).unwrap();
        let rows = compare_means(&base, &new, &TOL);
        assert!(rows
            .iter()
            .find(|r| r.file == "mix")
            .unwrap()
            .fails
            .iter()
            .any(|f| f.starts_with("peak")));
        let mut new = base.clone();
        new.files.remove("lead");
        assert!(!compare_means(&base, &new, &TOL)
            .iter()
            .find(|r| r.file == "lead")
            .unwrap()
            .fails
            .is_empty());
    }

    #[test]
    fn pooled_pitch() {
        let p = |w, oe| PitchReport {
            notes_total: 100,
            notes_analysed: 100,
            within_50c: w,
            fraction_within_50c: w as f64 / 100.0,
            octave_errors: oe,
            median_abs_cents: None,
            notes: vec![],
        };
        let r = [run(-20.0, 0.89), run(-20.0, 0.89)];
        let base = summarise(vec![], &r, &[p(99, 1), p(97, 1)])
            .unwrap()
            .pitch
            .unwrap();
        assert!(
            (base.fraction_within_50c - 0.98).abs() < 1e-12 && base.octave_errors_per_seed == 1.0
        );
        let ok = summarise(vec![], &r, &[p(96, 2), p(96, 2)])
            .unwrap()
            .pitch
            .unwrap();
        assert!(compare_pitch_pool(&base, &ok).is_empty());
        let bad = summarise(vec![], &r, &[p(93, 3), p(96, 2)])
            .unwrap()
            .pitch
            .unwrap();
        assert_eq!(compare_pitch_pool(&base, &bad).len(), 2);
    }
}

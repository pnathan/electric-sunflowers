//! Comparison of two runs: LTAS tables (per file) and pitch reports.
//! Thresholds follow docs/engine-design.md section 12.

use crate::ltas::{Ltas, NOMINAL_HZ};
use crate::pitch::PitchReport;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Contents of ltas.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LtasFile {
    pub centres_hz: Vec<f64>,
    pub files: BTreeMap<String, Ltas>,
}

/// Mid bands span 100 Hz to 10 kHz inclusive; the rest are edge bands.
pub fn is_mid(i: usize) -> bool {
    (100.0..=10000.0).contains(&NOMINAL_HZ[i])
}

/// Compare floor, in dB relative to the file's total active power. Each band
/// enters the delta as max(level, REL_FLOOR_DB), so a band below the floor
/// on both sides counts as equal, and a band that crosses the floor counts
/// only the part above it. Content 60 dB under the file's power is
/// inaudible in the mix and sits near the measurement floor (`FLOOR_DB`),
/// where small numeric changes (flush-to-zero, filter precision) move it by
/// several dB.
pub const REL_FLOOR_DB: f64 = -60.0;

pub const ACTIVE_POINTS: f64 = 15.0;
pub const MIX_RMS_DB: f64 = 1.5;
pub const MIX_PEAK: f64 = 0.89;
pub const MIX_PEAK_TOL: f64 = 0.002;
pub const PITCH_FRACTION_DROP: f64 = 0.03;
pub const PITCH_OCTAVE_RISE: usize = 2;

pub struct Tolerances {
    pub mid_db: f64,
    pub edge_db: f64,
}

/// One printed row.
pub struct Row {
    pub file: String,
    pub mid: Option<f64>,
    pub edge: Option<f64>,
    pub active_pts: Option<f64>,
    pub rms_db: Option<f64>,
    pub peak: Option<f64>,
    pub nonfinite: Option<u64>,
    pub fails: Vec<String>,
}

/// Compares every file in `base` and `new`. A file on one side only fails.
pub fn compare_ltas(base: &LtasFile, new: &LtasFile, tol: &Tolerances) -> Vec<Row> {
    let mut names: Vec<&String> = base.files.keys().chain(new.files.keys()).collect();
    names.sort();
    names.dedup();
    let mut rows = Vec::new();
    for name in names {
        let mut row =
            Row { file: name.clone(), mid: None, edge: None, active_pts: None, rms_db: None, peak: None, nonfinite: None, fails: vec![] };
        match (base.files.get(name), new.files.get(name)) {
            (Some(b), Some(n)) => {
                if b.bands_db.len() != NOMINAL_HZ.len() || n.bands_db.len() != NOMINAL_HZ.len() {
                    row.fails.push("band count".into());
                } else {
                    let (mut mid, mut edge) = (0.0f64, 0.0f64);
                    for i in 0..NOMINAL_HZ.len() {
                        let d = (n.bands_db[i].max(REL_FLOOR_DB) - b.bands_db[i].max(REL_FLOOR_DB)).abs();
                        if is_mid(i) {
                            mid = mid.max(d);
                        } else {
                            edge = edge.max(d);
                        }
                    }
                    row.mid = Some(mid);
                    row.edge = Some(edge);
                    if !(mid <= tol.mid_db) {
                        row.fails.push(format!("mid {mid:.2}>{}", tol.mid_db));
                    }
                    if !(edge <= tol.edge_db) {
                        row.fails.push(format!("edge {edge:.2}>{}", tol.edge_db));
                    }
                }
                let ap = 100.0 * (n.active_fraction - b.active_fraction);
                row.active_pts = Some(ap);
                if !(ap.abs() <= ACTIVE_POINTS) {
                    row.fails.push(format!("active {ap:+.1}pts"));
                }
                row.nonfinite = Some(n.nonfinite);
                if n.nonfinite > 0 {
                    row.fails.push(format!("{} NaN/inf", n.nonfinite));
                }
                if name == "mix" {
                    let dr = n.gated_rms_dbfs - b.gated_rms_dbfs;
                    row.rms_db = Some(dr);
                    row.peak = Some(n.peak);
                    if !(dr.abs() <= MIX_RMS_DB) {
                        row.fails.push(format!("rms {dr:+.2}dB"));
                    }
                    if !((n.peak - MIX_PEAK).abs() <= MIX_PEAK_TOL) {
                        row.fails.push(format!("peak {:.4}", n.peak));
                    }
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

fn opt(v: Option<f64>, prec: usize) -> String {
    v.map_or("-".to_string(), |x| format!("{x:.prec$}"))
}

/// Formats the comparison table.
pub fn table(rows: &[Row]) -> String {
    let mut s = format!(
        "{:<16} {:>7} {:>7} {:>8} {:>7} {:>7} {:>5}  {}\n",
        "file", "mid dB", "edge dB", "act pts", "rms dB", "peak", "nan", "result"
    );
    for r in rows {
        s += &format!(
            "{:<16} {:>7} {:>7} {:>8} {:>7} {:>7} {:>5}  {}\n",
            r.file,
            opt(r.mid, 2),
            opt(r.edge, 2),
            opt(r.active_pts, 1),
            opt(r.rms_db, 2),
            opt(r.peak, 4),
            r.nonfinite.map_or("-".into(), |v| v.to_string()),
            if r.fails.is_empty() { "PASS".to_string() } else { format!("FAIL {}", r.fails.join(", ")) }
        );
    }
    s
}

/// Pitch check against a baseline report: fraction within 50 cents may drop
/// by at most 0.03; octave errors may rise by at most 2. Returns failures.
pub fn compare_pitch(base: &PitchReport, new: &PitchReport) -> Vec<String> {
    let mut f = Vec::new();
    if !(new.fraction_within_50c >= base.fraction_within_50c - PITCH_FRACTION_DROP) {
        f.push(format!("fraction {:.3} < {:.3}-{PITCH_FRACTION_DROP}", new.fraction_within_50c, base.fraction_within_50c));
    }
    if new.octave_errors > base.octave_errors + PITCH_OCTAVE_RISE {
        f.push(format!("octave errors {} > {}+{PITCH_OCTAVE_RISE}", new.octave_errors, base.octave_errors));
    }
    if new.notes_analysed == 0 {
        f.push("no notes analysed".into());
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(peak: f64) -> Ltas {
        Ltas {
            bands_db: vec![-20.0; NOMINAL_HZ.len()],
            gated_rms_dbfs: -18.0,
            active_fraction: 0.8,
            peak,
            nonfinite: 0,
            channels: 2,
            frames: 1000,
            sample_rate: 44100,
        }
    }

    #[test]
    fn self_compare_passes_and_shifts_fail() {
        let mut files = BTreeMap::new();
        files.insert("mix".to_string(), entry(0.89));
        files.insert("lead".to_string(), entry(0.5));
        let base = LtasFile { centres_hz: NOMINAL_HZ.to_vec(), files };
        let tol = Tolerances { mid_db: 0.5, edge_db: 1.0 };
        assert!(compare_ltas(&base, &base, &tol).iter().all(|r| r.fails.is_empty()));

        let mut new = base.clone();
        new.files.get_mut("lead").unwrap().bands_db[16] += 0.6; // 1 kHz, mid
        new.files.get_mut("mix").unwrap().peak = 0.87;
        new.files.remove("mix");
        let rows = compare_ltas(&base, &new, &tol);
        assert!(rows.iter().all(|r| !r.fails.is_empty()));
        assert!(is_mid(6) && is_mid(26) && !is_mid(5) && !is_mid(27));
    }

    #[test]
    fn bands_under_the_floor_are_clamped() {
        let mut files = BTreeMap::new();
        files.insert("violin".to_string(), entry(0.5));
        files.get_mut("violin").unwrap().bands_db[0] = -88.0;
        let base = LtasFile { centres_hz: NOMINAL_HZ.to_vec(), files };
        let tol = Tolerances { mid_db: 0.5, edge_db: 1.0 };
        let mut new = base.clone();
        new.files.get_mut("violin").unwrap().bands_db[0] = -70.0;
        assert!(compare_ltas(&base, &new, &tol)[0].fails.is_empty());
        new.files.get_mut("violin").unwrap().bands_db[0] = -58.5;
        let r = &compare_ltas(&base, &new, &tol)[0];
        assert!((r.edge.unwrap() - 1.5).abs() < 1e-9 && !r.fails.is_empty());
    }
}

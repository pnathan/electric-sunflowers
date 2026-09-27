//! Pitch: chooses a melodic line via Viterbi over (prev, cur) pitch pairs.
//! Ports `pitchLine` (engine.js).
//!
//! JS parity: in the JS source, `U` closes over `pf` although `const pf` is
//! declared textually after `U`'s definition. This is not a temporal-dead-zone
//! bug: `U` is only ever *called* (inside `cand.map` building `us`) after the
//! `pf` declaration has executed, so by call time `pf` already holds its
//! value. The port simply computes `pf` before calling `U`, which reproduces
//! the same behavior with no special-casing needed.

use sfcore::js::{clamp, round};
use sfcore::rng::Rng;

/// The `prof` bag pitchLine reads (`o.prof || {leap:0.3, rep:0.3}`); `noise`
/// defaults to 0.7 via `pf.noise||0.7` when absent (e.g. instrumental lines
/// that pass no `prof` at all).
#[derive(Clone, Copy, Debug)]
pub struct PitchProf {
    pub leap: f64,
    pub rep: f64,
    pub noise: f64,
}

impl Default for PitchProf {
    fn default() -> Self {
        PitchProf { leap: 0.3, rep: 0.3, noise: 0.7 }
    }
}

/// pitchLine's option bag.
pub struct PitchOpts<'a> {
    pub n: usize,
    pub onsets: &'a [f64],
    pub durs: &'a [f64],
    pub weights: Option<&'a [f64]>,
    pub chord_pcs: &'a [Vec<i32>],
    pub scales: &'a [Vec<i32>],
    pub t: i32,
    pub tonic: i32,
    pub center: f64,
    pub shape: &'a dyn Fn(f64) -> f64,
    pub cadence: &'a str,
    pub reference: Option<&'a [i32]>,
    pub rng: &'a mut Rng,
    pub prev_end: Option<i32>,
    pub line_beats: f64,
    pub prof: Option<PitchProf>,
    pub hook: i32,
}

/// pitchLine(o)
pub fn pitch_line(o: &mut PitchOpts) -> Vec<i32> {
    let n = o.n;
    let lo = o.t - 5;
    let hi = o.t + 14;
    let mut cand: Vec<Vec<i32>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut c = Vec::new();
        for m in lo..=hi {
            if o.scales[i].contains(&m.rem_euclid(12)) {
                c.push(m);
            }
        }
        cand.push(c);
    }

    let pf = o.prof.unwrap_or_default();
    let lp = pf.leap;
    let rp = pf.rep;

    let u = |i: usize, m: i32, o: &mut PitchOpts, pf: PitchProf| -> f64 {
        let pc = m.rem_euclid(12);
        let ct = o.chord_pcs[i].contains(&pc);
        let strong = o.weights.map(|w| w[i]).unwrap_or(0.5) >= 0.7 || o.durs[i] >= 1.5;
        let mut s = if strong {
            if ct { 2.4 } else { -2.4 }
        } else if ct {
            0.6
        } else {
            0.0
        };
        if o.durs[i] >= 1.0 && !ct {
            s -= 1.5;
        }
        let x = clamp(o.onsets[i] / o.line_beats, 0.0, 1.0);
        s -= 0.3 * (m as f64 - (o.center + (o.shape)(x))).abs();
        if let Some(r) = o.reference {
            if !r.is_empty() {
                let idx = (round(i as f64 * (r.len() as f64 - 1.0) / (1.0f64).max((n - 1) as f64)) as i64)
                    .min(r.len() as i64 - 1)
                    .max(0) as usize;
                let rv = r[idx];
                if m == rv {
                    s += 1.3;
                } else if (m - rv).abs() <= 2 {
                    s += 0.3;
                }
            }
        }
        if i == n - 1 {
            if o.cadence == "tonic" {
                s += if pc == o.tonic {
                    3.5
                } else if ct {
                    0.0
                } else {
                    -3.0
                };
            } else if o.cadence == "open" {
                if ct
                    && [
                        (o.tonic + 7).rem_euclid(12),
                        (o.tonic + 2).rem_euclid(12),
                        (o.tonic + 4).rem_euclid(12),
                        (o.tonic + 11).rem_euclid(12),
                    ]
                    .contains(&pc)
                {
                    s += 1.5;
                }
                if pc == o.tonic {
                    s -= 0.4;
                }
            } else if !ct {
                s -= 1.5;
            }
        }
        if i == 0 {
            if let Some(pe) = o.prev_end {
                s -= 0.12 * (m - pe).abs() as f64;
            }
        }
        s + (o.rng.next() - 0.5) * pf.noise
    };

    let p = |a: i32, b: i32| -> f64 {
        let d = (b - a).abs();
        if d == 0 {
            return -0.4 + 0.8 * rp;
        }
        if d <= 2 {
            return 0.8 - 0.45 * lp;
        }
        if d <= 4 {
            return 0.15 + 0.35 * lp;
        }
        if d == 5 {
            return -0.35 + 0.6 * lp;
        }
        if d == 7 {
            return -0.7 + 0.7 * lp;
        }
        if d == 6 {
            return -1.6;
        }
        if d == 8 || d == 9 {
            return -1.4 + 0.9 * lp;
        }
        if d == 12 {
            return -1.6 + 1.0 * lp;
        }
        -2.5 - (d - 7) as f64 * 0.4
    };

    let tr = |a: i32, b: i32, c: i32| -> f64 {
        let l = b - a;
        let m = c - b;
        let mut s = 0.0;
        let sign = |x: i32| -> i32 {
            if x > 0 {
                1
            } else if x < 0 {
                -1
            } else {
                0
            }
        };
        if l.abs() >= 5 {
            if sign(m) == -sign(l) && m.abs() <= 2 {
                s += 0.9;
            } else if sign(m) == sign(l) {
                s -= 1.0;
            }
        } else if l != 0 && c == a && l.abs() <= 2 {
            s -= 0.75;
        } else if l != 0 && sign(m) == sign(l) && l.abs() <= 2 && m.abs() <= 2 {
            s += 0.12;
        }
        if l == 0 && m == 0 {
            s -= 0.5;
        }
        s
    };

    // us[i][k] = U(i, cand[i][k]); computed in JS order (i ascending, k
    // ascending within cand[i]) so the rng draws inside U happen in that order.
    let mut us: Vec<Vec<f64>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = Vec::with_capacity(cand[i].len());
        for k in 0..cand[i].len() {
            let m = cand[i][k];
            row.push(u(i, m, o, pf));
        }
        us.push(row);
    }

    if n == 1 {
        let mut bi = 0usize;
        for k in 1..us[0].len() {
            if us[0][k] > us[0][bi] {
                bi = k;
            }
        }
        return vec![cand[0][bi]];
    }

    let mut dp: Vec<Vec<f64>>; // dp[a][b]
    {
        let a_list = &cand[0];
        let b_list = &cand[1];
        dp = a_list
            .iter()
            .enumerate()
            .map(|(a, &ma)| {
                b_list
                    .iter()
                    .enumerate()
                    .map(|(b, &mb)| {
                        us[0][a] + us[1][b] + p(ma, mb) + if o.hook != 0 && mb - ma == o.hook { 2.2 } else { 0.0 }
                    })
                    .collect::<Vec<f64>>()
            })
            .collect();
    }

    let mut bps: Vec<Vec<Vec<i32>>> = Vec::new(); // bps[i-2][b][c] = a
    for i in 2..n {
        let a_list = &cand[i - 2];
        let b_list = &cand[i - 1];
        let c_list = &cand[i];
        let mut nd: Vec<Vec<f64>> = Vec::with_capacity(b_list.len());
        let mut nbp: Vec<Vec<i32>> = Vec::with_capacity(b_list.len());
        for (b, &mb) in b_list.iter().enumerate() {
            let mut row = vec![0.0f64; c_list.len()];
            let mut brow = vec![0i32; c_list.len()];
            for (c, &mc) in c_list.iter().enumerate() {
                let mut best = -1e9f64;
                let mut arg = 0i32;
                for (a, &ma) in a_list.iter().enumerate() {
                    let v = dp[a][b] + tr(ma, mb, mc);
                    if v > best {
                        best = v;
                        arg = a as i32;
                    }
                }
                row[c] = best + p(mb, mc) + us[i][c];
                brow[c] = arg;
            }
            nd.push(row);
            nbp.push(brow);
        }
        dp = nd;
        bps.push(nbp);
    }

    let mut bb = 0usize;
    let mut bc = 0usize;
    let mut bv = -1e9f64;
    for b in 0..dp.len() {
        for c in 0..dp[b].len() {
            if dp[b][c] > bv {
                bv = dp[b][c];
                bb = b;
                bc = c;
            }
        }
    }
    let mut idx = vec![0usize; n];
    idx[n - 1] = bc;
    idx[n - 2] = bb;
    for i in (2..n).rev() {
        idx[i - 2] = bps[i - 2][idx[i - 1]][idx[i]] as usize;
    }
    idx.iter().enumerate().map(|(i, &k)| cand[i][k]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sfcore::rng::rng_for;

    #[test]
    fn pitch_line_single() {
        let scales = vec![vec![0, 2, 4, 5, 7, 9, 11]];
        let chords = vec![vec![0, 4, 7]];
        let onsets = vec![0.0];
        let durs = vec![1.0];
        let mut rng = rng_for(1, "t");
        let shape = |_x: f64| 0.0;
        let mut opts = PitchOpts {
            n: 1,
            onsets: &onsets,
            durs: &durs,
            weights: None,
            chord_pcs: &chords,
            scales: &scales,
            t: 60,
            tonic: 0,
            center: 60.0,
            shape: &shape,
            cadence: "tonic",
            reference: None,
            rng: &mut rng,
            prev_end: None,
            line_beats: 1.0,
            prof: None,
            hook: 0,
        };
        let out = pitch_line(&mut opts);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn pitch_line_multi() {
        let scale = vec![0, 2, 4, 5, 7, 9, 11];
        let scales = vec![scale.clone(); 4];
        let chords = vec![vec![0, 4, 7]; 4];
        let onsets = vec![0.0, 1.0, 2.0, 3.0];
        let durs = vec![1.0, 1.0, 1.0, 1.0];
        let mut rng = rng_for(2, "t2");
        let shape = |_x: f64| 0.0;
        let mut opts = PitchOpts {
            n: 4,
            onsets: &onsets,
            durs: &durs,
            weights: None,
            chord_pcs: &chords,
            scales: &scales,
            t: 60,
            tonic: 0,
            center: 60.0,
            shape: &shape,
            cadence: "tonic",
            reference: None,
            rng: &mut rng,
            prev_end: None,
            line_beats: 4.0,
            prof: None,
            hook: 0,
        };
        let out = pitch_line(&mut opts);
        assert_eq!(out.len(), 4);
    }
}

//! Rhythm: places syllables onto the metric grid by dynamic programming.
//! Ports `placeRhythm` (engine.js).

use crate::theory::Meter;
use sfcore::js::f32r;
use sfcore::rng::Rng;

/// The `pr` option bag; JS default `{dot:0.3,even:0.3,sync:0,rnoise:0.6}`.
#[derive(Clone, Copy, Debug)]
pub struct PrOpts {
    pub dot: f64,
    pub even: f64,
    pub sync: f64,
    pub rnoise: f64,
}

impl Default for PrOpts {
    fn default() -> Self {
        PrOpts {
            dot: 0.3,
            even: 0.3,
            sync: 0.0,
            rnoise: 0.6,
        }
    }
}

/// placeRhythm's return value.
#[derive(Clone, Debug)]
pub struct RhythmResult {
    pub onsets: Vec<f64>,
    pub durs: Vec<f64>,
    pub weights: Vec<f64>,
    pub line_beats: f64,
}

/// placeRhythm(syls, nBars, mi, rng, pr): `syls` gives just the per-syllable
/// stress flags the JS function reads (`s.stress`).
pub fn place_rhythm(stresses: &[bool], n_bars: usize, mi: &Meter, rng: &mut Rng, pr: PrOpts) -> RhythmResult {
    let n = stresses.len();
    let bar_slots = (mi.bpb * mi.sub) as i64;
    let res: i64 = if (n as f64) > n_bars as f64 * bar_slots as f64 * 0.68 { 2 } else { 1 };
    let s_total = n_bars as i64 * bar_slots * res;
    let beat_slots = mi.sub as i64 * res;
    let max_last = s_total - beat_slots;
    let st: Vec<i64> = stresses.iter().map(|&b| if b { 1 } else { 0 }).collect();

    let mut lead0 = 0usize;
    while lead0 < n && st[lead0] == 0 {
        lead0 += 1;
    }
    let o_off: i64 = if lead0 > 0 {
        (beat_slots).min(lead0 as i64 * res)
    } else {
        0
    };

    let g_size = (s_total + o_off) as usize;
    let mut w = vec![0.0f32; g_size];
    for g in 0..g_size as i64 {
        let s = g - o_off;
        if s < 0 {
            w[g as usize] = 0.12;
            continue;
        }
        if res == 2 && s % 2 != 0 {
            w[g as usize] = 0.06;
            continue;
        }
        let q = ((s / res) % bar_slots) as usize;
        let mut v = mi.w[q];
        if q == 0 && s > 0 {
            v *= 0.95;
        }
        w[g as usize] = f32r(v) as f32;
    }

    // dense-line early return: no rng draws happen before this check.
    if n as i64 > max_last + o_off + 1 {
        let step = (s_total as f64 - beat_slots as f64 * 0.5) / n as f64;
        let mut onsets = Vec::with_capacity(n);
        let mut durs = Vec::with_capacity(n);
        for i in 0..n {
            onsets.push(i as f64 * step / beat_slots as f64);
            durs.push(step / beat_slots as f64);
        }
        return RhythmResult {
            onsets,
            durs,
            weights: vec![0.5; n],
            line_beats: s_total as f64 / beat_slots as f64,
        };
    }

    let mut noise: Vec<Vec<f32>> = Vec::with_capacity(n);
    for _ in 0..n {
        let mut a = vec![0.0f32; g_size];
        for p in 0..g_size {
            a[p] = f32r((rng.next() - 0.5) * pr.rnoise) as f32;
        }
        noise.push(a);
    }

    let un = |i: usize, g: i64| -> f64 {
        let wg = w[g as usize] as f64;
        let base = if st[i] != 0 {
            3.0 * wg + if wg > 0.1 && wg < 0.5 { pr.sync * 1.6 } else { 0.0 }
        } else {
            -2.2 * wg
        };
        base + noise[i][g as usize] as f64
    };

    let gap_score = |g: f64, ps: bool| -> f64 {
        if g < 0.75 {
            return -0.55;
        }
        if ps {
            if g == 1.0 {
                return -0.2 + 0.4 * pr.even;
            }
            if g == 1.5 {
                return 0.05 + 0.55 * pr.dot;
            }
            if g == 2.0 {
                return 0.4 - 0.3 * pr.even;
            }
            if g == 3.0 {
                return (if mi.sub == 3 { 0.45 } else { 0.2 }) + 0.35 * pr.dot;
            }
            if g == 4.0 {
                return 0.05;
            }
            return -0.45 * (g - 4.0);
        }
        if g == 0.5 {
            return -0.3 + 0.45 * pr.dot;
        }
        if g == 1.0 {
            return 0.3 + 0.35 * pr.even;
        }
        if g == 1.5 {
            return -0.1 + 0.4 * pr.dot;
        }
        if g == 2.0 {
            return 0.0;
        }
        if g == 3.0 {
            return -0.35;
        }
        if g == 4.0 {
            return -0.85;
        }
        -1.2 - 0.3 * (g - 4.0)
    };

    const NEG: f64 = -1e9;
    let mut dp: Vec<Vec<f64>> = (0..n).map(|_| vec![NEG; g_size]).collect();
    let mut bp: Vec<Vec<i32>> = (0..n).map(|_| vec![-1i32; g_size]).collect();

    let first_max = o_off + (2 * beat_slots).min(max_last - (n as i64 - 1));
    for g in 0..=first_max {
        if g < 0 || g as usize >= g_size {
            continue;
        }
        if g < o_off && st[0] != 0 {
            continue;
        }
        dp[0][g as usize] = un(0, g) - 0.22 * (0.0f64).max((g - o_off) as f64) / res as f64;
    }

    for i in 1..n {
        let gmax = o_off + max_last - (n as i64 - 1 - i as i64);
        let g_start = i as i64;
        let mut g = g_start;
        while g <= gmax {
            if g < o_off && (st[i] != 0 || i >= lead0) {
                g += 1;
                continue;
            }
            let mut best = NEG;
            let mut arg: i32 = -1;
            let qmin = (g - 6 * beat_slots).max(0);
            let mut q = qmin;
            while q < g {
                if dp[i - 1][q as usize] > NEG / 2.0 {
                    let v = dp[i - 1][q as usize] + gap_score((g - q) as f64 / res as f64, st[i - 1] != 0);
                    if v > best {
                        best = v;
                        arg = q as i32;
                    }
                }
                q += 1;
            }
            if arg >= 0 {
                dp[i][g as usize] = best + un(i, g);
                bp[i][g as usize] = arg;
            }
            g += 1;
        }
    }

    let mut best_p: i64 = -1;
    let mut bv = NEG;
    for g in o_off..=(o_off + max_last) {
        if dp[n - 1][g as usize] <= NEG / 2.0 {
            continue;
        }
        let s = g - o_off;
        let wg = w[g as usize] as f64;
        let v = dp[n - 1][g as usize]
            + if s as f64 >= s_total as f64 / 2.0 - res as f64 {
                1.4 * wg + 0.6 + if wg > 0.9 { 0.8 } else { 0.0 }
            } else {
                -2.2
            };
        if v > bv {
            bv = v;
            best_p = g;
        }
    }

    // Deviation from JS: the JS backtrack has no guard here, so when no
    // complete DP path is found (bestP stays -1) it reads bp[i][-1], which in
    // JS is `undefined`, propagates as NaN through the arithmetic below, and
    // reaches the renderer as NaN onsets/durs. Rust cannot index with -1, so
    // fall back to the same even-spacing result the dense-line early return
    // above already produces, rather than crash or propagate NaN.
    if best_p < 0 {
        let step = (s_total as f64 - beat_slots as f64 * 0.5) / n as f64;
        let mut onsets = Vec::with_capacity(n);
        let mut durs = Vec::with_capacity(n);
        for i in 0..n {
            onsets.push(i as f64 * step / beat_slots as f64);
            durs.push(step / beat_slots as f64);
        }
        return RhythmResult {
            onsets,
            durs,
            weights: vec![0.5; n],
            line_beats: s_total as f64 / beat_slots as f64,
        };
    }

    let mut pos = vec![0i64; n];
    pos[n - 1] = best_p;
    for i in (1..n).rev() {
        pos[i - 1] = bp[i][pos[i] as usize] as i64;
    }

    let onsets: Vec<f64> = pos.iter().map(|&g| (g - o_off) as f64 / beat_slots as f64).collect();
    let mut durs = Vec::with_capacity(n);
    for i in 0..n {
        if i < n - 1 {
            durs.push(onsets[i + 1] - onsets[i]);
        } else {
            durs.push((0.5f64).max((s_total as f64 - beat_slots as f64 * 0.6) / beat_slots as f64 - onsets[i]));
        }
    }
    let weights: Vec<f64> = pos.iter().map(|&g| w[g as usize] as f64).collect();

    RhythmResult {
        onsets,
        durs,
        weights,
        line_beats: s_total as f64 / beat_slots as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theory::meter;
    use sfcore::rng::rng_for;

    #[test]
    fn place_rhythm_basic() {
        let mi = meter("4/4");
        let stresses = vec![true, false, true, false, true, false, true, false];
        let mut rng = rng_for(1, "test");
        let r = place_rhythm(&stresses, 2, &mi, &mut rng, PrOpts::default());
        assert_eq!(r.onsets.len(), 8);
        assert_eq!(r.durs.len(), 8);
        assert_eq!(r.weights.len(), 8);
        // onsets non-decreasing
        for i in 1..r.onsets.len() {
            assert!(r.onsets[i] >= r.onsets[i - 1]);
        }
    }

    #[test]
    fn place_rhythm_no_complete_path_falls_back() {
        // 4/4, 1 bar, 15 syllables (first unstressed, then alternating),
        // dense enough to squeeze past the early-return threshold but leave
        // the DP unable to complete a path to the last syllable: the JS
        // would yield NaN onsets here. Must not panic, and must fall back
        // to even spacing (weights all 0.5) as the dense-line branch does.
        let mi = meter("4/4");
        let mut stresses = vec![false];
        for i in 0..14 {
            stresses.push(i % 2 == 0);
        }
        let mut rng = rng_for(1, "test");
        let r = place_rhythm(&stresses, 1, &mi, &mut rng, PrOpts::default());
        assert_eq!(r.onsets.len(), 15);
        assert_eq!(r.durs.len(), 15);
        assert!(r.weights.iter().all(|&w| w == 0.5));
        for i in 1..r.onsets.len() {
            assert!(r.onsets[i] >= r.onsets[i - 1]);
        }
    }

    #[test]
    fn place_rhythm_dense_early_return() {
        let mi = meter("4/4");
        let stresses: Vec<bool> = (0..40).map(|i| i % 2 == 0).collect();
        let mut rng = rng_for(1, "test");
        let r = place_rhythm(&stresses, 1, &mi, &mut rng, PrOpts::default());
        assert_eq!(r.onsets.len(), 40);
        assert!(r.weights.iter().all(|&w| w == 0.5));
    }
}

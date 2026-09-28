//! Rhythm: sets a line's syllables on the metric grid.
//!
//! Algorithm: Viterbi over a monotone alignment of syllables to grid
//! positions. It is an explicit-duration (semi-Markov) model: the state of
//! syllable i is its onset position, and the transition from syllable i-1
//! is scored by the gap between the two onsets, so each syllable's
//! duration is scored directly. Emissions reward stressed syllables on
//! strong metric positions and unstressed ones on weak positions.
//!
//! Variety comes from perturb-and-MAP (Papandreou and Yuille, "Perturb-and-
//! MAP random fields", ICCV 2011): uniform noise of width `RhythmStyle::noise`
//! is added to every emission, and the exact MAP alignment of the
//! perturbed model is taken. Each take is a coherent optimum, not a
//! sequence of independent random choices.
//!
//! Units: a grid position is one metric slot (an eighth note in 4/4), or a
//! half slot when the line is dense (`res` = 2). Gaps are scored in
//! half-slot units, as integers.

use sfcore::random::Rng;
use song::MeterGrid;

/// The per-song rhythmic character (from the melody profile).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RhythmStyle {
    /// Preference for dotted gaps (1.5 and 3 slots), 0-1.
    pub dot: f64,
    /// Preference for even gaps (1 slot after an unstressed syllable, 1 or
    /// 2 after a stressed one), 0-1.
    pub even: f64,
    /// Bonus scale for stressed syllables on weak off-beat positions
    /// (syncopation), 0-1.
    pub sync: f64,
    /// Width of the uniform perturb-and-MAP noise on each emission.
    pub noise: f64,
}

impl Default for RhythmStyle {
    /// The neutral style used for instrumental lines.
    fn default() -> Self {
        RhythmStyle { dot: 0.3, even: 0.3, sync: 0.0, noise: 0.6 }
    }
}

/// Score weights of the text-setting model. Scores are log-domain; a
/// larger total is better.
#[derive(Clone, Copy, Debug)]
pub struct RhythmWeights {
    /// A line is set at half-slot resolution when it has more syllables
    /// than this fraction of its slots.
    pub dense_fraction: f64,
    /// Metric weight of a pickup position before the line's first beat.
    pub pickup_weight: f64,
    /// Metric weight of a half-slot position.
    pub half_slot_weight: f64,
    /// Scale of the downbeat weight of bars after the first.
    pub later_downbeat: f64,
    /// Emission of a stressed syllable per unit of metric weight.
    pub stress_accent: f64,
    /// Emission of an unstressed syllable per unit of metric weight.
    pub unstress_accent: f64,
    /// Syncopation bonus scale (times `RhythmStyle::sync`) for a stressed
    /// syllable on a position of weight strictly between the two bounds.
    pub sync_bonus: f64,
    pub sync_lo: f64,
    pub sync_hi: f64,
    /// Penalty per slot that the first syllable starts after the line.
    pub late_start: f64,
    /// Largest gap, in beats.
    pub max_gap_beats: usize,
    /// Latest start of the first syllable, in beats.
    pub max_start_beats: usize,
    /// Gap score for gaps under 0.75 slot.
    pub crowded: f64,
    /// End bonus for a last syllable in the line's second half: this times
    /// its metric weight, plus `end_bonus`, plus `end_strong` when the
    /// weight exceeds 0.9.
    pub end_accent: f64,
    pub end_bonus: f64,
    pub end_strong: f64,
    /// Score for a last syllable in the line's first half.
    pub end_early: f64,
    /// Offset of the last note's end before the line's end, in beats.
    pub end_release: f64,
    /// Shortest last note, in beats.
    pub min_last: f64,
}

/// The text-setting weights.
pub const RHYTHM_WEIGHTS: RhythmWeights = RhythmWeights {
    dense_fraction: 0.68,
    pickup_weight: 0.12,
    half_slot_weight: 0.06,
    later_downbeat: 0.95,
    stress_accent: 3.0,
    unstress_accent: -2.2,
    sync_bonus: 1.6,
    sync_lo: 0.1,
    sync_hi: 0.5,
    late_start: 0.22,
    max_gap_beats: 6,
    max_start_beats: 2,
    crowded: -0.55,
    end_accent: 1.4,
    end_bonus: 0.6,
    end_strong: 0.8,
    end_early: -2.2,
    end_release: 0.6,
    min_last: 0.5,
};

/// Score of a gap of `h` half slots after a stressed (`after_stress`) or
/// unstressed syllable. Stressed syllables prefer 1-2 slots (and dotted 1.5
/// and 3 by `dot`); unstressed ones prefer 1 slot. `sub3` marks compound
/// meters, where a 3-slot gap is a full beat.
fn gap_score(h: usize, after_stress: bool, style: &RhythmStyle, sub3: bool, w: &RhythmWeights) -> f64 {
    if h <= 1 {
        return w.crowded;
    }
    let (dot, even) = (style.dot, style.even);
    if after_stress {
        match h {
            2 => -0.2 + 0.4 * even,
            3 => 0.05 + 0.55 * dot,
            4 => 0.4 - 0.3 * even,
            6 => (if sub3 { 0.45 } else { 0.2 }) + 0.35 * dot,
            8 => 0.05,
            // 0.45 per slot away from 4 slots.
            _ => -0.225 * (h as f64 - 8.0).abs(),
        }
    } else {
        match h {
            2 => 0.3 + 0.35 * even,
            3 => -0.1 + 0.4 * dot,
            4 => 0.0,
            6 => -0.35,
            8 => -0.85,
            // -1.2 at 4 slots, 0.3 per further slot.
            _ => -1.2 - 0.15 * (h as f64 - 8.0),
        }
    }
}

/// Result of text setting, in beats from the line's first beat.
#[derive(Clone, Debug)]
pub struct RhythmResult {
    /// Onset of each syllable; negative for a pickup before the line.
    pub onsets: Vec<f64>,
    pub durs: Vec<f64>,
    /// Metric weight of each onset's position (1 on the downbeat).
    pub weights: Vec<f64>,
    /// Length of the line in beats.
    pub line_beats: f64,
}

/// The grid of one line: positions, their metric weights, and sizes.
struct LineGrid {
    /// Positions per slot: 1, or 2 for a dense line.
    res: usize,
    /// Positions in the line (without pickups).
    total: usize,
    /// Positions per beat.
    beat: usize,
    /// Pickup positions before the line.
    pickup: usize,
    /// Metric weight of each position, pickups first; `pickup + total` entries.
    w: Vec<f64>,
}

impl LineGrid {
    fn new(n: usize, n_bars: usize, grid: &MeterGrid, lead_unstressed: usize, wt: &RhythmWeights) -> LineGrid {
        let bar_slots = grid.slots();
        let res = if n as f64 > (n_bars * bar_slots) as f64 * wt.dense_fraction { 2 } else { 1 };
        let total = n_bars * bar_slots * res;
        let beat = grid.sub as usize * res;
        let pickup = if lead_unstressed > 0 { beat.min(lead_unstressed * res) } else { 0 };
        let w = (0..pickup + total)
            .map(|g| {
                if g < pickup {
                    return wt.pickup_weight;
                }
                let s = g - pickup;
                if s % res != 0 {
                    return wt.half_slot_weight;
                }
                let q = (s / res) % bar_slots;
                let v = grid.weights[q] as f64;
                if q == 0 && s > 0 {
                    v * wt.later_downbeat
                } else {
                    v
                }
            })
            .collect();
        LineGrid { res, total, beat, pickup, w }
    }

    fn line_beats(&self) -> f64 {
        self.total as f64 / self.beat as f64
    }

    /// End of the last note, in beats: `end_release` before the line's end,
    /// and at least `min_last` after its onset.
    fn last_dur(&self, onset: f64, wt: &RhythmWeights) -> f64 {
        wt.min_last.max(self.line_beats() - wt.end_release - onset)
    }

    /// Onsets (beats), durations and weights for syllables at positions `pos`.
    fn result(&self, pos: &[usize], wt: &RhythmWeights) -> RhythmResult {
        let beat = self.beat as f64;
        let onsets: Vec<f64> = pos.iter().map(|&g| (g as f64 - self.pickup as f64) / beat).collect();
        let durs = durations(&onsets, |o| self.last_dur(o, wt));
        let weights = pos.iter().map(|&g| self.w[g]).collect();
        RhythmResult { onsets, durs, weights, line_beats: self.line_beats() }
    }

    /// Fallback when no alignment exists (more syllables than positions):
    /// onsets evenly spread over the line less half a beat, quantised to
    /// the grid, subdivided by powers of two until the onsets are distinct.
    fn quantised(&self, n: usize, wt: &RhythmWeights) -> RhythmResult {
        let span = self.total as f64 - self.beat as f64 * 0.5;
        let step = span / n as f64;
        let mut q = 1usize;
        while step * (q as f64) < 1.0 && q < 64 {
            q *= 2;
        }
        let beat = self.beat as f64;
        let mut onsets = Vec::with_capacity(n);
        let mut weights = Vec::with_capacity(n);
        for i in 0..n {
            let k = (i as f64 * step * q as f64 + 0.5).floor() as usize;
            let (g, frac) = (k / q, k % q);
            onsets.push((k as f64 / q as f64) / beat);
            weights.push(if frac == 0 {
                self.w.get(self.pickup + g).copied().unwrap_or(wt.half_slot_weight)
            } else {
                wt.half_slot_weight
            });
        }
        let durs = durations(&onsets, |o| self.last_dur(o, wt));
        RhythmResult { onsets, durs, weights, line_beats: self.line_beats() }
    }
}

/// Inter-onset durations; the last from `last`.
fn durations(onsets: &[f64], last: impl Fn(f64) -> f64) -> Vec<f64> {
    let n = onsets.len();
    (0..n).map(|i| if i + 1 < n { onsets[i + 1] - onsets[i] } else { last(onsets[i]) }).collect()
}

/// Sets `stresses.len()` syllables over `n_bars` bars of `grid` (see the
/// module doc). The first syllables, when unstressed, may fall in a pickup
/// of up to one beat before the line. The last syllable must start at least
/// one beat before the line's end and is rewarded in the line's second
/// half on a strong position.
pub fn set_text(stresses: &[bool], n_bars: usize, grid: &MeterGrid, style: &RhythmStyle, rng: &mut Rng) -> RhythmResult {
    set_text_with(stresses, n_bars, grid, style, &RHYTHM_WEIGHTS, rng)
}

/// `set_text` with explicit weights.
pub fn set_text_with(
    stresses: &[bool],
    n_bars: usize,
    grid: &MeterGrid,
    style: &RhythmStyle,
    wt: &RhythmWeights,
    rng: &mut Rng,
) -> RhythmResult {
    let n = stresses.len();
    let n_bars = n_bars.max(1);
    let lead0 = stresses.iter().position(|&s| s).unwrap_or(n);
    let lg = LineGrid::new(n, n_bars, grid, lead0, wt);
    if n == 0 {
        return RhythmResult { onsets: vec![], durs: vec![], weights: vec![], line_beats: lg.line_beats() };
    }
    let size = lg.pickup + lg.total;
    // Latest position of the last syllable (line positions).
    let max_last = lg.total - lg.beat;
    if n > max_last + lg.pickup + 1 {
        return lg.quantised(n, wt);
    }

    // Perturbation, one draw per (syllable, position).
    let mut noise = vec![0.0f64; n * size];
    for x in noise.iter_mut() {
        *x = (rng.uniform() - 0.5) * style.noise;
    }
    let emit = |i: usize, g: usize| -> f64 {
        let wg = lg.w[g];
        let base = if stresses[i] {
            wt.stress_accent * wg
                + if wg > wt.sync_lo && wg < wt.sync_hi { style.sync * wt.sync_bonus } else { 0.0 }
        } else {
            wt.unstress_accent * wg
        };
        base + noise[i * size + g]
    };
    // Half slots per position.
    let half = 2 / lg.res;
    let sub3 = grid.sub == 3;

    const NEG: f64 = f64::NEG_INFINITY;
    const NONE: u32 = u32::MAX;
    let mut dp = vec![NEG; n * size];
    let mut bp = vec![NONE; n * size];

    // Latest first onset: `max_start_beats` into the line, and early enough
    // that the other syllables fit; a crowded line may start in the pickup.
    let room = max_last as isize - (n as isize - 1);
    let first_max = lg.pickup as isize + ((wt.max_start_beats * lg.beat) as isize).min(room);
    for g in 0..(first_max + 1).clamp(0, size as isize) as usize {
        if g < lg.pickup && stresses[0] {
            continue;
        }
        let late = g.saturating_sub(lg.pickup) as f64 / lg.res as f64;
        dp[g] = emit(0, g) - wt.late_start * late;
    }

    let max_gap = wt.max_gap_beats * lg.beat;
    for i in 1..n {
        let gmax = lg.pickup + max_last - (n - 1 - i);
        let after_stress = stresses[i - 1];
        let (prev, cur) = dp.split_at_mut(i * size);
        let prev = &prev[(i - 1) * size..];
        let cur = &mut cur[..size];
        let bcur = &mut bp[i * size..(i + 1) * size];
        for g in i..=gmax {
            if g < lg.pickup && (stresses[i] || i >= lead0) {
                continue;
            }
            let mut best = NEG;
            let mut arg = NONE;
            for q in g.saturating_sub(max_gap)..g {
                let d = prev[q];
                if d == NEG {
                    continue;
                }
                let v = d + gap_score((g - q) * half, after_stress, style, sub3, wt);
                if v > best {
                    best = v;
                    arg = q as u32;
                }
            }
            if arg != NONE {
                cur[g] = best + emit(i, g);
                bcur[g] = arg;
            }
        }
    }

    let last = &dp[(n - 1) * size..];
    let mut best_g = None;
    let mut best_v = NEG;
    for g in lg.pickup..=lg.pickup + max_last {
        if last[g] == NEG {
            continue;
        }
        let s = g - lg.pickup;
        let wg = lg.w[g];
        let end = if 2 * s + 2 * lg.res >= lg.total {
            wt.end_accent * wg + wt.end_bonus + if wg > 0.9 { wt.end_strong } else { 0.0 }
        } else {
            wt.end_early
        };
        let v = last[g] + end;
        if v > best_v {
            best_v = v;
            best_g = Some(g);
        }
    }
    let Some(g_last) = best_g else {
        return lg.quantised(n, wt);
    };

    let mut pos = vec![0usize; n];
    pos[n - 1] = g_last;
    for i in (1..n).rev() {
        pos[i - 1] = bp[i * size + pos[i]] as usize;
    }
    lg.result(&pos, wt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sfcore::random::tag;
    use song::Meter;

    fn rng(k: u64) -> Rng {
        Rng::event(1, tag("test.rhythm"), k)
    }

    #[test]
    fn set_text_basic() {
        let mi = Meter::Four4.grid();
        let stresses = vec![true, false, true, false, true, false, true, false];
        let r = set_text(&stresses, 2, mi, &RhythmStyle::default(), &mut rng(0));
        assert_eq!(r.onsets.len(), 8);
        assert_eq!(r.durs.len(), 8);
        assert_eq!(r.weights.len(), 8);
        for i in 1..r.onsets.len() {
            assert!(r.onsets[i] > r.onsets[i - 1]);
        }
        assert!(r.durs.iter().all(|&d| d > 0.0));
        assert_eq!(r.line_beats, 8.0);
    }

    #[test]
    fn stressed_syllables_take_strong_positions() {
        let mi = Meter::Four4.grid();
        let stresses = vec![false, true, false, true, false, true, false, true];
        let mut on_strong = 0;
        let mut total = 0;
        for k in 0..50 {
            let r = set_text(&stresses, 2, mi, &RhythmStyle::default(), &mut rng(k));
            for (s, w) in stresses.iter().zip(&r.weights) {
                if *s {
                    total += 1;
                    on_strong += usize::from(*w >= 0.5);
                }
            }
        }
        assert!(on_strong * 10 >= total * 9, "{on_strong}/{total}");
    }

    #[test]
    fn no_complete_path_falls_back_to_the_grid() {
        // 15 syllables in one bar with an unstressed pickup: no alignment
        // exists. The fallback must give distinct grid onsets.
        let mi = Meter::Four4.grid();
        let mut stresses = vec![false];
        for i in 0..14 {
            stresses.push(i % 2 == 0);
        }
        let r = set_text(&stresses, 1, mi, &RhythmStyle::default(), &mut rng(1));
        assert_eq!(r.onsets.len(), 15);
        for i in 1..r.onsets.len() {
            assert!(r.onsets[i] > r.onsets[i - 1]);
        }
        assert!(r.durs.iter().all(|&d| d > 0.0));
        // Half-slot grid in 4/4: onsets are multiples of 1/4 beat or finer
        // powers of two.
        for o in &r.onsets {
            let x = o * 16.0;
            assert_eq!(x, x.round(), "onset {o} off the grid");
        }
    }

    #[test]
    fn dense_line_falls_back_to_the_grid() {
        let mi = Meter::Four4.grid();
        let stresses: Vec<bool> = (0..40).map(|i| i % 2 == 0).collect();
        let r = set_text(&stresses, 1, mi, &RhythmStyle::default(), &mut rng(2));
        assert_eq!(r.onsets.len(), 40);
        for i in 1..r.onsets.len() {
            assert!(r.onsets[i] > r.onsets[i - 1]);
        }
    }

    #[test]
    fn empty_line() {
        let r = set_text(&[], 1, Meter::Three4.grid(), &RhythmStyle::default(), &mut rng(3));
        assert!(r.onsets.is_empty());
        assert_eq!(r.line_beats, 3.0);
    }
}

//! Instrumental lines shared by the violin and the harmony guitar:
//! counter-lines and fills, as notes in seconds. No rendering.
//!
//! Counter-line: a greedy walk (first-species style: one note per chord
//! segment, or per ~1.7 s of it) through the chord tones in `[lo, hi]`.
//! Cost per candidate: interval size (steps 0.35 per semitone; up to a
//! fourth 0.6 + 0.35 per semitone; leaps 2.4 + 0.7 per semitone), +4.5 for a
//! repeated note, +1.2 for returning to the note before last, -0.6 for a
//! leap of more than a major third against the previous direction (leap
//! recovery), +0.8 for a non-chord tone (the last sub-segment may take the
//! next chord's tones), 0.22 per semitone from an arch target (30% of the
//! range at the section edges, 70% in the middle), +6 within a whole step of
//! a sounding lead note and +1.5 at an octave of one, plus uniform noise
//! 0..0.7.
//!
//! Fill: a descending scalar run in the gap after a lyric line (the gap from
//! one beat after the last syllable to a quarter beat before the next
//! line's first onset, at least 1.25 beats), one note per grid step, two to
//! five notes, in the local scale, landing on a tone of the chord at the end
//! of the gap.

use compose::form::{Form, Sec};
use compose::melody::LeadNote;
use compose::theory::local_scale;
use compose::timeline::Timeline;
use sfcore::random::{Rng, Tag};
use song::{Pc, PcSet};

/// One note of a counter-line or a fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineNote {
    pub t0: f64,
    pub t1: f64,
    pub midi: i32,
    pub vel: f32,
}

/// Counter-line in `[lo, hi]` over the segments of the sections that pass
/// `filter`, avoiding the `lead` notes. `long` plays one note per segment.
/// Segment k draws from `Rng::event(seed, tag, k)`.
#[allow(clippy::too_many_arguments)]
pub fn counter_line(
    form: &Form,
    tl: &Timeline,
    lead: &[LeadNote],
    lo: i32,
    hi: i32,
    filter: impl Fn(&Sec) -> bool,
    seed: u64,
    tag: Tag,
    long: bool,
) -> Vec<LineNote> {
    let mut notes: Vec<LineNote> = Vec::new();
    let mut prev = (lo + hi) / 2 + (lo + hi) % 2;
    let mut prev_dir = 0i32;
    let segs = &tl.segs;
    let vel = if long { 0.68 } else { 0.58 };

    for (si, sg) in segs.iter().enumerate() {
        let sec = &form.sections[sg.sec];
        if !filter(sec) {
            continue;
        }
        let mut r = Rng::event(seed, tag, si as u64);
        let next = segs.get(si + 1);
        let (st0, st1) = (tl.to_time(sg.b0), tl.to_time(sg.b1));
        let n = if long {
            1
        } else {
            (((st1 - st0) / 1.7).round() as i32).max(1)
        };
        let span = sec.beats(&form.meter);
        let (sec_t0, sec_t1) = (tl.to_time(span.start), tl.to_time(span.end));
        let chord = form.chord(sg.chord);

        for j in 0..n {
            let t0 = st0 + (st1 - st0) * j as f64 / n as f64;
            let t1 = st0 + (st1 - st0) * (j + 1) as f64 / n as f64;
            // The last sub-segment may anticipate the next chord.
            let pcs: PcSet = match next {
                Some(nx) if j > 0 && j == n - 1 => chord.tones.union(form.chord(nx.chord).tones),
                _ => chord.tones,
            };
            let x = (((t0 + t1) / 2.0 - sec_t0) / (sec_t1 - sec_t0).max(1.0)).clamp(0.0, 1.0);
            let target =
                lo as f64 + (hi - lo) as f64 * (0.3 + 0.4 * (std::f64::consts::PI * x).sin());
            let before_last = notes.len().checked_sub(2).map(|i| notes[i].midi);

            let mut best: Option<i32> = None;
            let mut best_cost = f64::INFINITY;
            for m in lo..=hi {
                if !pcs.contains(Pc::new(m)) {
                    continue;
                }
                let iv = m - prev;
                let ai = iv.abs();
                let mut c = if ai <= 2 {
                    ai as f64 * 0.35
                } else if ai <= 5 {
                    0.6 + ai as f64 * 0.35
                } else {
                    2.4 + ai as f64 * 0.7
                };
                if m == prev {
                    c += 4.5;
                }
                if before_last == Some(m) {
                    c += 1.2;
                }
                if prev_dir != 0 && iv.signum() == -prev_dir && ai > 4 {
                    c -= 0.6;
                }
                if !chord.tones.contains(Pc::new(m)) {
                    c += 0.8;
                }
                c += (m as f64 - target).abs() * 0.22 + 0.7 * r.uniform();
                for v in lead.iter().filter(|v| v.t0 < t1 && v.t1 > t0) {
                    let d = (m - v.midi).abs();
                    if d <= 2 {
                        c += 6.0;
                    } else if d % 12 == 0 {
                        c += 1.5;
                    }
                }
                if c < best_cost {
                    best_cost = c;
                    best = Some(m);
                }
            }
            let Some(best) = best else { continue };
            let dir = (best - prev).signum();
            if dir != 0 {
                prev_dir = dir;
            }
            prev = best;
            notes.push(LineNote {
                t0,
                t1,
                midi: best,
                vel,
            });
        }
    }
    notes
}

/// Fills in `[lo, hi]` after the lyric lines of the sections that pass
/// `filter`. Line k draws from `Rng::event(seed, tag, k)`.
#[allow(clippy::too_many_arguments)]
pub fn fills(
    form: &Form,
    tl: &Timeline,
    lo: i32,
    hi: i32,
    filter: impl Fn(&Sec) -> bool,
    seed: u64,
    tag: Tag,
) -> Vec<LineNote> {
    let mut notes: Vec<LineNote> = Vec::new();
    let bpb = form.bpb();
    let sub = form.sub() as f64;

    for (k, line) in form.lines.iter().enumerate() {
        let sec = &form.sections[line.sec];
        if !filter(sec) {
            continue;
        }
        // An uncomposed or empty line gets no fill.
        let Some(rh) = line.rh.as_ref() else { continue };
        let Some(&last_onset) = rh.onsets.get(line.syls.len().wrapping_sub(1)) else {
            continue;
        };
        let last_on = (line.start_bar as i32 * bpb) as f64 + last_onset;
        let line_end = ((line.start_bar + line.n_bars) as i32 * bpb) as f64;
        let w0 = last_on + 1.0;
        let next_onset = sec
            .lines
            .get(line.li + 1)
            .and_then(|&idx| form.lines[idx].rh.as_ref())
            .and_then(|rh2| rh2.onsets.first().copied())
            .unwrap_or(0.5);
        let w1 = line_end + next_onset - 0.25;
        if w1 - w0 < 1.25 {
            continue;
        }
        let cnt = ((w1 - w0) * sub * 0.8).floor().min(5.0) as i32;
        if cnt < 2 {
            continue;
        }
        let mut r = Rng::event(seed, tag, k as u64);
        let ch_end = tl.chord_at(form, w1 + 0.3);
        let sc = local_scale(sec.key.0, sec.key.1, tl.chord_at(form, w0 + 0.01));
        let run: Vec<i32> = (lo..=hi).filter(|&m| sc.contains(Pc::new(m))).collect();
        if run.is_empty() {
            continue;
        }
        let target = run
            .iter()
            .position(|&m| ch_end.tones.contains(Pc::new(m)) && m >= lo + 4)
            .unwrap_or(2) as i32;
        let bump = i32::from(r.uniform() < 0.4);
        let last_idx = run.len() as i32 - 1;
        let start_idx = (target + cnt - 1 + bump).clamp(0, last_idx);

        for j in 0..cnt {
            let b = w0 + j as f64 / sub;
            let idx = (start_idx - j).clamp(0, last_idx) as usize;
            let d = if j == cnt - 1 {
                (w1 - b).max(0.5)
            } else {
                1.0 / sub
            };
            notes.push(LineNote {
                t0: tl.to_time(b) + 0.005 * r.bipolar(),
                t1: tl.to_time(b + d),
                midi: run[idx],
                vel: if j == 0 { 0.6 } else { 0.5 },
            });
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sfcore::random::tag;

    /// Fills follow the key of the section they sit in: a verse in C major,
    /// then one in E major, over diatonic chords, so every fill note lies in
    /// its own section's major scale.
    #[test]
    fn fills_use_the_scale_of_their_section() {
        let v = json!({
            "schema_version":2,"key":"C","mode":"major","meter":"4/4","tempo":90,
            "sections":[
                {"type":"verse","lines":[{"syl":"*walk the *line","chords":["C","G","C","G"]}]},
                {"type":"verse","key":"E","lines":[{"syl":"*walk the *line","chords":["E","B","E","B"]}]}
            ]
        });
        let s = song::normalize_value(&v).unwrap().0;
        let p = compose::prepare::prepare(&s, 5, None);
        let (form, tl) = (&p.form, &p.timeline);
        let starts: Vec<f64> = form
            .sections
            .iter()
            .map(|x| tl.to_time(x.beats(&form.meter).start))
            .collect();
        let mut seen = [0usize; 2];
        for seed in 0..6u64 {
            let fl = fills(form, tl, 59, 79, |_| true, seed, tag("test.fill"));
            for n in &fl {
                let si = usize::from(n.t0 >= starts[1] - 1e-6);
                let (tonic, mode) = form.sections[si].key;
                let scale = mode.scale().transpose(tonic.get() as i32);
                assert!(scale.contains(Pc::new(n.midi)), "seed {seed}: {}", n.midi);
                seen[si] += 1;
            }
        }
        assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
    }
}

//! Counter-lines and fills: note lists for the violin's counter-melodies
//! and fills and the harmony guitar's fills. No rendering.

use compose::form::{Form, Sec};
use compose::melody::LeadNote;
use compose::theory::local_scale;
use compose::timeline::Timeline;
use sfcore::js;
use sfcore::rng::rng_for;
use song::{Pc, PcSet, Song};

/// One note in a counter-line or fill.
#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub t0: f64,
    pub t1: f64,
    pub m: i32,
    pub v: f64,
}

/// A greedy walk that threads one note per (sub-)segment through the chord
/// tones in `[lo,hi]`, preferring steps, following an arch across the
/// section, and steering clear of the `lead` notes sounding at the time.
pub fn counter_line(
    form: &Form,
    tl: &Timeline,
    lead: &[LeadNote],
    lo: i32,
    hi: i32,
    filter: impl Fn(&Sec) -> bool,
    seed: u32,
    long: bool,
) -> Vec<Note> {
    let mut r = rng_for(seed, &format!("ctr{lo}"));
    let mut notes: Vec<Note> = Vec::new();
    let mut prev = js::round((lo + hi) as f64 / 2.0) as i32;
    let mut prev_dir: i32 = 0;
    let segs = &tl.segs;

    for si in 0..segs.len() {
        let sg = &segs[si];
        let sec = &form.sections[sg.sec];
        if !filter(sec) {
            continue;
        }
        let nx = segs.get(si + 1);
        let t0_ = tl.to_time(sg.b0);
        let t1_ = tl.to_time(sg.b1);
        let n: i32 = if long {
            1
        } else {
            (js::round((t1_ - t0_) / 1.7) as i32).max(1)
        };
        let span = sec.beats(&form.meter);
        let sec_t0 = tl.to_time(span.start);
        let sec_t1 = tl.to_time(span.end);
        let chord = form.chord(sg.chord);

        for j in 0..n {
            let t0 = t0_ + (t1_ - t0_) * j as f64 / n as f64;
            let t1 = t0_ + (t1_ - t0_) * (j + 1) as f64 / n as f64;
            // The last sub-segment may anticipate the next chord.
            let pcs: PcSet = match nx {
                Some(nxs) if j > 0 && j == n - 1 => chord.tones.union(form.chord(nxs.chord).tones),
                _ => chord.tones,
            };
            let voc: Vec<i32> = lead
                .iter()
                .filter(|v| v.t0 < t1 && v.t1 > t0)
                .map(|v| v.midi)
                .collect();
            let x = js::clamp(
                ((t0 + t1) / 2.0 - sec_t0) / js::max(1.0, sec_t1 - sec_t0),
                0.0,
                1.0,
            );
            let target =
                lo as f64 + (hi - lo) as f64 * (0.3 + 0.4 * js::sin(std::f64::consts::PI * x));

            let mut best: Option<i32> = None;
            let mut bs = 1e9f64;
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
                if notes.len() > 1 && m == notes[notes.len() - 2].m {
                    c += 1.2;
                }
                if prev_dir != 0 && js::sign(iv as f64) as i32 == -prev_dir && ai > 4 {
                    c -= 0.6;
                }
                if !chord.tones.contains(Pc::new(m)) {
                    c += 0.8;
                }
                c += (m as f64 - target).abs() * 0.22 + r.next() * 0.7;
                for &v in &voc {
                    let d = (m - v).abs();
                    if d <= 2 {
                        c += 6.0;
                    } else if d % 12 == 0 {
                        c += 1.5;
                    }
                }
                if c < bs {
                    bs = c;
                    best = Some(m);
                }
            }
            let best = match best {
                Some(b) => b,
                None => continue,
            };
            let dir = js::sign((best - prev) as f64) as i32;
            if dir != 0 {
                prev_dir = dir;
            }
            prev = best;
            notes.push(Note {
                t0,
                t1,
                m: best,
                v: if long { 0.68 } else { 0.58 },
            });
        }
    }
    notes
}

/// A short descending scalar run in the gap after each lyric line, landing
/// on a tone of the chord at the run's end. `_lead` is unused.
pub fn fills_for(
    form: &Form,
    tl: &Timeline,
    _lead: &[LeadNote],
    lo: i32,
    hi: i32,
    filter: impl Fn(&Sec) -> bool,
    song: &Song,
    seed: u32,
) -> Vec<Note> {
    let mut r = rng_for(seed, &format!("fill{lo}"));
    let mut notes: Vec<Note> = Vec::new();
    let tonic = song.key.transpose(form.transpose);
    let bpb = form.bpb();
    let sub = form.sub();

    for line in &form.lines {
        let sec = &form.sections[line.sec];
        if !filter(sec) {
            continue;
        }
        let n = line.syls.len();
        if n == 0 {
            // An empty line gets no fill but still takes the run-length
            // draw, keeping the stream in step.
            let _ = r.next() < 0.4;
            continue;
        }
        // Lines are composed by `prepare`; an uncomposed line gets no fill.
        let Some(rh) = line.rh.as_ref() else { continue };
        let last_on = (line.start_bar as i32 * bpb) as f64 + rh.onsets[n - 1];
        let line_end = ((line.start_bar + line.n_bars) as i32 * bpb) as f64;
        // JS: `bpb===3?1:1` -- both arms are 1, so the offset is always 1.
        let w0 = last_on + 1.0;
        let next_onset = sec
            .lines
            .get(line.li + 1)
            .and_then(|&idx| form.lines[idx].rh.as_ref())
            .map(|rh2| rh2.onsets[0])
            .unwrap_or(0.5);
        let w1 = line_end + next_onset - 0.25;
        if w1 - w0 < 1.25 {
            continue;
        }
        let cnt = js::min(5.0, ((w1 - w0) * sub as f64 * 0.8).floor()) as i32;
        if cnt < 2 {
            continue;
        }
        let ch_end = tl.chord_at(form, w1 + 0.3);
        let ch0 = tl.chord_at(form, w0 + 0.01);
        let sc = local_scale(tonic, song.mode, ch0);
        let run: Vec<i32> = (lo..=hi).filter(|&m| sc.contains(Pc::new(m))).collect();
        let mut target = run
            .iter()
            .position(|&m| ch_end.tones.contains(Pc::new(m)) && m >= lo + 4)
            .map(|p| p as i32)
            .unwrap_or(-1);
        if target < 0 {
            target = 2;
        }
        let bump = if r.next() < 0.4 { 1 } else { 0 };
        let last_idx = (run.len() as i32 - 1).max(0);
        let start_idx = js::clamp((target + cnt - 1 + bump) as f64, 0.0, last_idx as f64) as i32;

        for k in 0..cnt {
            // A draw with no effect, kept so the stream stays in step.
            let _ = r.next() < 0.25 && k > 0;
            let b = w0 + k as f64 / sub as f64;
            let idx = js::clamp((start_idx - k) as f64, 0.0, last_idx as f64) as usize;
            let d = if k == cnt - 1 {
                js::max(0.5, w1 - b)
            } else {
                1.0 / sub as f64
            };
            notes.push(Note {
                t0: tl.to_time(b) + (r.next() - 0.5) * 0.01,
                t1: tl.to_time(b + d),
                m: run[idx],
                v: 0.5 + if k == 0 { 0.1 } else { 0.0 },
            });
        }
    }
    notes
}

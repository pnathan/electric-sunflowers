//! Port of the bass part of engine.js (`genBass`, engine.js ~696-716).

use crate::guitar::mtof;
use compose::form::Form;
use compose::song::Song;
use compose::timeline::Timeline;
use dsp::pluck::{pluck, PluckOpts};
use sfcore::js::{clamp, round};
use sfcore::rng::rng_for;
use sfcore::SR_F;

struct Note {
    b: f64,
    d: f64,
    m: i32,
    v: f64,
}

/// `nearest(pc,c)` in JS: the note nearest `c` (a fractional MIDI centre)
/// whose pitch class is `pc`.
fn nearest(pc: i32, c: f64) -> i32 {
    let mut m = round(c) as i32 - 6;
    while (m.rem_euclid(12)) != pc {
        m += 1;
    }
    m
}

/// `genBass(song,form,tl,seed)`.
pub fn gen_bass(_song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<f32> {
    let mut r = rng_for(seed, "bass");
    let bpb = form.mi.bpb;
    let mut notes: Vec<Note> = Vec::new();
    let segs = &tl.segs;
    let mut prev = 40i32;

    for si in 0..segs.len() {
        let sg = &segs[si];
        let sec = &form.sections[sg.sec];
        if sec.type_ == "intro" {
            continue;
        }
        if sec.type_ == "verse" && sec.occ == 0 && sg.b0 < (sec.start_bar as f64 + sec.n_bars as f64 / 2.0) * bpb as f64 {
            continue;
        }
        let intensity = sec.intensity;
        let root = nearest(sg.chord.bass, clamp(prev as f64, 34.0, 46.0));
        prev = root;
        let nxt = segs.get(si + 1);
        let len = sg.b1 - sg.b0;
        let is_last = nxt.is_none();
        if is_last || intensity <= 1 || sec.type_ == "bridge" {
            notes.push(Note { b: sg.b0, d: len - 0.1, m: root, v: 0.85 });
            continue;
        }
        let step = if bpb == 2 { 1.0 } else if bpb == 3 { 3.0 } else { 2.0 };
        let mut b = sg.b0;
        while b < sg.b1 - 1e-6 {
            let rem = sg.b1 - b;
            let first = b == sg.b0;
            let mut m = if first {
                root
            } else if let Some(fifth) = sg.chord.fifth {
                nearest(fifth, root as f64 + 2.0)
            } else {
                root
            };
            if m > 48 {
                m -= 12;
            }
            let d = rem.min(if bpb == 3 { 3.0 } else { 2.0 }) - 0.08;
            if !first && nxt.is_some() && rem <= 2.0 && intensity >= 2 && bpb == 4 {
                notes.push(Note { b, d: 0.9, m, v: 0.75 });
                let tr = nearest(nxt.unwrap().chord.bass, root as f64);
                let draw = r.next();
                let ap = tr + if draw < 0.5 { -1 } else if tr > root { -2 } else { 2 };
                notes.push(Note { b: b + 1.0, d: 0.9, m: ap, v: 0.7 });
            } else {
                notes.push(Note { b, d, m, v: if first { 0.9 } else { 0.75 } });
            }
            b += step;
        }
    }

    let len = (tl.end * SR_F).ceil() as usize;
    let mut out = vec![0f32; len];
    for n in &notes {
        let t = tl.to_time(n.b) + (r.next() - 0.5) * 0.008;
        let t1 = tl.to_time(n.b + n.d);
        let f = mtof(n.m as f64);
        let s = round(t * SR_F) as i64;
        let l = round((t1 - t + 0.05) * SR_F) as i64;
        let o = PluckOpts {
            amp: n.v,
            bright: Some(0.12),
            damp: Some(0.5),
            t60: 2.2,
            pick: Some(0.2),
            noise: Some(0.03),
            detune: None,
            rel: 0.06,
            rel_t: 0.12,
            glide: 0.0,
            atk_noise: 0.0,
        };
        pluck(&mut out, s, f, l, &o, &mut r);
        let mut ph = 0.0f64;
        for i in 0..l {
            let idx = s + i;
            if idx < 0 || idx as usize >= len {
                continue;
            }
            let tt = i as f64 / SR_F;
            ph += 2.0 * std::f64::consts::PI * f / SR_F;
            let tail = if i > l - 2600 { (l - i) as f64 / 2600.0 } else { 1.0 };
            let env = (tt / 0.006).min(1.0) * (-tt / 0.7).exp() * tail;
            out[idx as usize] += (ph.sin() * 0.55 * n.v * env) as f32;
        }
    }
    out
}

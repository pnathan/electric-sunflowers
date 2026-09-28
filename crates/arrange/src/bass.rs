//! The bass: roots on chord changes, root-fifth patterns and approach notes
//! in busier sections, plucked with a sine sub layer.

use crate::guitar::mtof;
use compose::form::Form;
use compose::timeline::Timeline;
use song::{Pc, SectionKind, Song};
use dsp::pluck::{pluck, PluckOpts};
use sfcore::js::{clamp, exp, f32r, round, sin};
use sfcore::rng::rng_for;
use sfcore::SR_F;

struct Note {
    b: f64,
    d: f64,
    m: i32,
    v: f64,
}

/// The lowest note from `round(c) - 6` up whose pitch class is `pc`.
fn nearest(pc: Pc, c: f64) -> i32 {
    let mut m = round(c) as i32 - 6;
    while Pc::new(m) != pc {
        m += 1;
    }
    m
}

/// The bass track.
pub fn gen_bass(_song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<f32> {
    let mut r = rng_for(seed, "bass");
    let bpb = form.bpb();
    let mut notes: Vec<Note> = Vec::new();
    let segs = &tl.segs;
    let mut prev = 40i32;

    for si in 0..segs.len() {
        let sg = &segs[si];
        let sec = &form.sections[sg.sec];
        if sec.kind == SectionKind::Intro {
            continue;
        }
        if sec.kind == SectionKind::Verse
            && sec.occ == 0
            && sg.b0 < (sec.start_bar as f64 + sec.n_bars as f64 / 2.0) * bpb as f64
        {
            continue;
        }
        let intensity = sec.intensity.level();
        let chord = form.chord(sg.chord);
        let root = nearest(chord.bass, clamp(prev as f64, 34.0, 46.0));
        prev = root;
        let nxt = segs.get(si + 1);
        let len = sg.b1 - sg.b0;
        let is_last = nxt.is_none();
        if is_last || intensity <= 1 || sec.kind == SectionKind::Bridge {
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
            } else if let Some(fifth) = chord.fifth {
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
                let tr = nxt.map_or(root, |nx| nearest(form.chord(nx.chord).bass, root as f64));
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
        // JS parity: `for(i=0;i<L&&s+i<len;i++)` only bounds-checks the
        // upper edge (`s+i<len`); a negative index still satisfies that
        // condition, so the loop body runs (and `ph` still advances), but
        // `out[s+i]+=...` with a negative index is a silent no-op on a JS
        // typed array (continue past it). Once `s+i>=len` the loop
        // condition itself is false, so the body -- including the phase
        // advance -- never runs for that `i`: break before touching `ph`.
        for i in 0..l {
            let idx = s + i;
            if idx >= len as i64 {
                break;
            }
            let tt = i as f64 / SR_F;
            ph += 2.0 * std::f64::consts::PI * f / SR_F;
            if idx < 0 {
                continue;
            }
            let tail = if i > l - 2600 { (l - i) as f64 / 2600.0 } else { 1.0 };
            let env = (tt / 0.006).min(1.0) * exp(-tt / 0.7) * tail;
            let idx = idx as usize;
            // JS parity: `out[i]+=d` on a Float32Array is one rounding of
            // f64(out[i])+d to f32, not two (round d, then round the sum).
            out[idx] = f32r(out[idx] as f64 + sin(ph) * 0.55 * n.v * env) as f32;
        }
    }
    out
}

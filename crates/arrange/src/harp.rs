//! The harp: rolled chords per segment (sparse in verses and the intro),
//! echoes of the top tones in busy sections, and a glissando into the
//! bridge and the final lifted section.

use crate::guitar::mtof;
use compose::form::Form;
use compose::timeline::Timeline;
use song::{Pc, SectionKind, Song};
use dsp::pluck::{pluck, PluckOpts};
use sfcore::js::{pow, round};
use sfcore::rng::rng_for;
use sfcore::SR_F;

struct Note {
    t: f64,
    m: i32,
    v: f64,
}

/// The harp track.
pub fn gen_harp(song: &Song, form: &Form, tl: &Timeline, seed: u32) -> Vec<f32> {
    let mut r = rng_for(seed, "harp");
    let bpb = form.bpb();
    let mut notes: Vec<Note> = Vec::new();

    for sg in &tl.segs {
        let sec = &form.sections[sg.sec];
        let intensity = sec.intensity.level();
        let on = sec.is_lift()
            || match sec.kind {
                SectionKind::Chorus
                | SectionKind::Bridge
                | SectionKind::Outro
                | SectionKind::Interlude
                | SectionKind::Intro => true,
                SectionKind::Verse => sec.occ > 0,
                SectionKind::Prechorus => false,
            };
        if !on {
            continue;
        }
        let sparse = matches!(sec.kind, SectionKind::Verse | SectionKind::Intro);
        if sparse && (sg.bar - sec.start_bar) % 2 == 1 {
            continue;
        }
        let chord = form.chord(sg.chord);
        let want = if sparse { 4 } else { 6 };
        let mut tones: Vec<i32> = Vec::new();
        let mut m = 55i32;
        while m <= 88 && tones.len() < want {
            let pc = Pc::new(m);
            if chord.tones.contains(pc) && (!tones.is_empty() || pc == chord.root) {
                tones.push(m);
            }
            m += 1;
        }
        let t0 = tl.to_time(sg.b0);
        let stagger = if sparse { 0.09 } else { 0.065 };
        let n_tones = tones.len();
        for (k, &m) in tones.iter().enumerate() {
            let draw = r.next();
            notes.push(Note {
                t: t0 + k as f64 * stagger + (draw - 0.5) * 0.01,
                m,
                v: (0.55 + 0.1 * k as f64 / n_tones as f64) * (if sparse { 0.7 } else { 1.0 }),
            });
        }
        if intensity >= 2 && sg.b1 - sg.b0 >= bpb as f64 {
            let tm = tl.to_time(sg.b0 + if bpb == 4 { 2.0 } else if bpb == 3 { 2.0 } else { 1.0 });
            let last3: Vec<i32> = tones.iter().rev().take(3).rev().cloned().collect();
            for (k, &m) in last3.iter().enumerate() {
                notes.push(Note { t: tm + k as f64 * 0.05, m, v: 0.4 });
            }
        }
    }

    for sec in &form.sections {
        if !(sec.kind == SectionKind::Bridge || matches!(sec.lift, Some(l) if l.is_final)) {
            continue;
        }
        let b = sec.start_bar as f64 * bpb as f64;
        let t1 = tl.to_time(b) - 0.03;
        let t0 = t1 - 0.62;
        let tonic = tl.chord_at(form, b).root;
        let sc = song.mode.scale().transpose(tonic.get() as i32);
        let mut run: Vec<i32> = Vec::new();
        for m in 62..=88i32 {
            if sc.contains(Pc::new(m)) {
                run.push(m);
            }
        }
        let n = run.len();
        for (k, &m) in run.iter().enumerate() {
            notes.push(Note { t: t0 + (t1 - t0) * k as f64 / n as f64, m, v: 0.28 + 0.25 * k as f64 / n as f64 });
        }
    }

    let len = (tl.end * SR_F).ceil() as usize;
    let mut out = vec![0f32; len];
    for n in &notes {
        let f = mtof(n.m as f64);
        let start = round(n.t * SR_F) as i64;
        let l = round((7.0f64.min(3.0 + 400.0 / f)) * SR_F) as i64;
        let pick_draw = 0.3 + r.next() * 0.15;
        let detune_draw = 0.8 + r.next() * 0.8;
        let o = PluckOpts {
            amp: n.v,
            bright: Some(0.45),
            damp: Some(0.16),
            t60: 6.0 * pow(98.0 / f, 0.5),
            pick: Some(pick_draw),
            noise: Some(0.02),
            detune: Some(detune_draw),
            rel: 0.3,
            rel_t: 0.4,
            glide: 0.0,
            atk_noise: 0.0,
        };
        pluck(&mut out, start, f, l, &o, &mut r);
    }
    out
}

//! Port of the guitar part of engine.js (`GTUNE`, `guitarVoicing`, `GPAT`,
//! `genGuitar`, engine.js ~611-694).

use compose::form::Form;
use compose::song::Song;
use compose::theory::Chord;
use compose::timeline::Timeline;
use dsp::pluck::{pluck, PluckOpts};
use dsp::truthy;
use sfcore::js::{pow, round};
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use sfcore::SR_F;

/// `mtof(m)` from engine.js. Shared with bass.rs and harp.rs.
pub(crate) fn mtof(m: f64) -> f64 {
    440.0 * pow(2.0, (m - 69.0) / 12.0)
}

const GTUNE: [i32; 6] = [40, 45, 50, 55, 59, 64];

/// One string's fret at a given position search: `opts[s]` in JS.
fn opts_for(o: i32, pos: i32, pcs: &[i32]) -> Vec<i32> {
    let mut a = Vec::new();
    for f in [0, pos, pos + 1, pos + 2, pos + 3] {
        if f >= 0 && pcs.contains(&((o + f).rem_euclid(12))) && !a.contains(&f) {
            a.push(f);
        }
    }
    a
}

/// `rec(s)` in JS `guitarVoicing`: backtracks over strings `bs0+1..6`,
/// trying each fret option for that string, then "no note on this string",
/// in that order (order matters: the first candidate to strictly beat `bs`
/// wins on ties).
#[allow(clippy::too_many_arguments)]
fn rec(
    s: usize,
    cur: &mut [Option<i32>; 6],
    opts: &[Vec<i32>; 6],
    ch: &Chord,
    bs0: usize,
    pos: i32,
    best: &mut Option<[Option<i32>; 6]>,
    bs: &mut f64,
) {
    if s == 6 {
        let fr: Vec<i32> = cur.iter().filter_map(|x| x.filter(|&v| v > 0)).collect();
        if fr.len() > 4 {
            return;
        }
        let span = if fr.is_empty() { 0 } else { fr.iter().max().unwrap() - fr.iter().min().unwrap() };
        if span > 3 {
            return;
        }
        let notes: [Option<i32>; 6] = {
            let mut n = [None; 6];
            for i in 0..6 {
                n[i] = cur[i].map(|f| GTUNE[i] + f);
            }
            n
        };
        let have: u16 = notes.iter().filter_map(|x| x.map(|v| 1u16 << v.rem_euclid(12))).fold(0, |a, b| a | b);
        let mut sc = 0.0f64;
        for &e in &ch.ess {
            if have & (1 << e) == 0 {
                sc -= 6.0;
            }
        }
        if let Some(fifth) = ch.fifth {
            if have & (1 << fifth) == 0 {
                sc -= 0.6;
            }
        }
        if ch.bass != ch.root && have & (1 << ch.root) == 0 {
            sc -= 6.0;
        }
        sc -= 0.3 * pos as f64;
        sc += 0.45 * cur.iter().filter(|x| **x == Some(0)).count() as f64;
        for i in (bs0 + 1)..6 {
            if cur[i].is_none() {
                sc -= if i == 5 { 1.2 } else { 1.6 };
            }
        }
        sc -= 0.25 * bs0 as f64;
        if let Some(third) = ch.third {
            if notes.iter().filter(|x| x.map(|v| v.rem_euclid(12)) == Some(third)).count() > 1 {
                sc -= 0.5;
            }
        }
        if notes.iter().filter(|x| x.is_some()).count() < 4 {
            sc -= 2.0;
        }
        if sc > *bs {
            *bs = sc;
            *best = Some(notes);
        }
        return;
    }
    for &f in &opts[s] {
        cur[s] = Some(f);
        rec(s + 1, cur, opts, ch, bs0, pos, best, bs);
    }
    cur[s] = None;
    rec(s + 1, cur, opts, ch, bs0, pos, best, bs);
}

/// `guitarVoicing(ch)`. JS parity: the JS caches results by chord name;
/// here it is a plain deterministic function (no rng involved), so a cache
/// is not needed for parity.
pub fn guitar_voicing(ch: &Chord) -> [Option<i32>; 6] {
    let pcs: Vec<i32> = if ch.pcs.contains(&ch.bass) { ch.pcs.clone() } else { ch.pcs.iter().cloned().chain(std::iter::once(ch.bass)).collect() };
    let mut best: Option<[Option<i32>; 6]> = None;
    let mut bs = -1e9f64;
    for pos in 0..=9i32 {
        let opts: [Vec<i32>; 6] = {
            let mut o: [Vec<i32>; 6] = Default::default();
            for i in 0..6 {
                o[i] = opts_for(GTUNE[i], pos, &pcs);
            }
            o
        };
        for bs0 in 0..=2usize {
            for &bf in &opts[bs0] {
                if (GTUNE[bs0] + bf).rem_euclid(12) != ch.bass {
                    continue;
                }
                let mut cur = [None; 6];
                cur[bs0] = Some(bf);
                rec(bs0 + 1, &mut cur, &opts, ch, bs0, pos, &mut best, &mut bs);
            }
        }
    }
    match best {
        Some(b) => b,
        None => {
            let mut b = [None; 6];
            let r = 48 + (ch.root - 48).rem_euclid(12);
            b[2] = Some(r);
            b[3] = Some(r + if ch.third.is_some() { (ch.third.unwrap() - ch.root).rem_euclid(12) } else { 7 });
            b[4] = Some(r + 7);
            b
        }
    }
}

/// One event on a string (`ev[s]` entries in JS `genGuitar`).
struct GEvent {
    t: f64,
    m: Option<i32>,
    v: f64,
    stop: bool,
}

type Pattern = &'static [(f64, &'static str, f64)];

fn gpat(style: &str, meter: &str) -> Pattern {
    const STRUM_44: Pattern = &[(0.0, "D", 0.85), (2.0, "D", 0.7), (3.0, "U", 0.45), (5.0, "U", 0.5), (6.0, "D", 0.65), (7.0, "U", 0.45)];
    const STRUM_34: Pattern = &[(0.0, "B", 0.9), (2.0, "D", 0.6), (4.0, "D", 0.6), (5.0, "U", 0.4)];
    const STRUM_68: Pattern = &[(0.0, "D", 0.85), (2.0, "U", 0.4), (3.0, "D", 0.7), (4.0, "U", 0.4), (5.0, "U", 0.45)];
    const STRUMLITE_44: Pattern = &[(0.0, "B", 0.85), (2.0, "d", 0.55), (4.0, "B2", 0.75), (6.0, "d", 0.55), (7.0, "u", 0.35)];
    const STRUMLITE_34: Pattern = &[(0.0, "B", 0.85), (2.0, "d", 0.5), (4.0, "d", 0.5)];
    const STRUMLITE_68: Pattern = &[(0.0, "B", 0.85), (2.0, "d", 0.45), (3.0, "B2", 0.7), (5.0, "d", 0.45)];
    const FP_44: Pattern = &[(0.0, "B", 0.85), (1.0, "g", 0.5), (2.0, "b", 0.55), (3.0, "e", 0.6), (4.0, "B2", 0.75), (5.0, "b", 0.5), (6.0, "g", 0.5), (7.0, "b", 0.5)];
    const FP_34: Pattern = &[(0.0, "B", 0.85), (1.0, "g", 0.5), (2.0, "b", 0.55), (3.0, "e", 0.6), (4.0, "b", 0.5), (5.0, "g", 0.5)];
    const FP_68: Pattern = &[(0.0, "B", 0.85), (1.0, "g", 0.5), (2.0, "b", 0.55), (3.0, "e", 0.6), (4.0, "b", 0.5), (5.0, "g", 0.5)];
    const TRAVIS_44: Pattern = &[(0.0, "B", 0.85), (0.0, "e", 0.55), (1.0, "b", 0.45), (2.0, "B2", 0.75), (3.0, "g", 0.5), (4.0, "B", 0.8), (5.0, "e", 0.5), (6.0, "B2", 0.75), (7.0, "b", 0.45)];
    const TRAVIS_34: Pattern = &[(0.0, "B", 0.85), (0.0, "e", 0.55), (1.0, "b", 0.45), (2.0, "B2", 0.7), (3.0, "g", 0.5), (4.0, "B2", 0.7), (5.0, "b", 0.45)];
    const TRAVIS_68: Pattern = &[(0.0, "B", 0.85), (0.0, "e", 0.5), (1.0, "g", 0.45), (2.0, "b", 0.5), (3.0, "B2", 0.75), (4.0, "b", 0.45), (5.0, "g", 0.45)];
    // arpeggio == fingerpick in JS.
    match (style, meter) {
        ("strum", "4/4") => STRUM_44,
        ("strum", "3/4") => STRUM_34,
        ("strum", "6/8") => STRUM_68,
        ("strumLite", "4/4") => STRUMLITE_44,
        ("strumLite", "3/4") => STRUMLITE_34,
        ("strumLite", "6/8") => STRUMLITE_68,
        ("fingerpick", "4/4") | ("arpeggio", "4/4") => FP_44,
        ("fingerpick", "3/4") | ("arpeggio", "3/4") => FP_34,
        ("fingerpick", "6/8") | ("arpeggio", "6/8") => FP_68,
        ("travis", "4/4") => TRAVIS_44,
        ("travis", "3/4") => TRAVIS_34,
        ("travis", "6/8") => TRAVIS_68,
        _ => panic!("unknown guitar pattern {style} {meter}"),
    }
}

/// `genGuitar(song,form,tl,seed)`.
pub fn gen_guitar(song: &Song, form: &Form, tl: &Timeline, seed: u32, tuning: &Tuning) -> Vec<f32> {
    let mut r = rng_for(seed, "gtr");
    let bpb = form.mi.bpb;
    let sub = form.mi.sub;
    let mut ev: [Vec<GEvent>; 6] = Default::default();
    let nbars = form.bars.len();
    // JS parity: JS caches guitarVoicing results by chord name; guitar_voicing
    // itself is deterministic and side-effect free, so this is a plain
    // memoization for speed, not needed for output parity.
    let mut voicing_cache: std::collections::HashMap<String, [Option<i32>; 6]> = std::collections::HashMap::new();
    let voicing_of = |ch: &Chord, cache: &mut std::collections::HashMap<String, [Option<i32>; 6]>| -> [Option<i32>; 6] {
        *cache.entry(ch.name.clone()).or_insert_with(|| guitar_voicing(ch))
    };

    for bi in 0..nbars {
        let bar = &form.bars[bi];
        let sec = &form.sections[bar.sec];
        let intensity = sec.intensity;
        let mut style = song.guitar.clone();
        if style == "strum" && intensity <= 1 {
            style = "strumLite".to_string();
        }
        let cond_style = style == "fingerpick" || style == "arpeggio" || style == "travis";
        if cond_style && intensity >= 3 && song.band.drums != "none" {
            // JS parity: `r()<0` is always false (r() is in [0,1)), but the
            // draw is still consumed because JS evaluates it as part of the
            // `&&` chain.
            let draw = r.next();
            if draw < 0.0 {
                style = "strum".to_string();
            }
        }
        if sec.type_ == "bridge" && style == "strum" {
            style = "fingerpick".to_string();
        }
        let pat = gpat(&style, &song.meter_name);
        let last = bi == nbars - 1;
        let vel_s = 0.72 + 0.1 * intensity as f64;
        let events: Vec<(f64, &str, f64)> = if last { vec![(0.0, "D", 0.8)] } else { pat.to_vec() };

        for (slot, kind, vel) in events {
            let beat = bi as f64 * bpb as f64 + slot / sub as f64;
            let ch = tl.chord_at(form, beat + 0.01);
            let v = voicing_of(ch, &mut voicing_cache);
            let t = tl.to_time(beat) + (r.next() - 0.5) * 0.012;
            let vv = vel * vel_s * (0.92 + r.next() * 0.16);
            let bass_str: i32 = v.iter().position(|x| x.is_some()).map(|x| x as i32).unwrap_or(-1);
            let alt_str: i32 = {
                let mut found = bass_str;
                let hi = 3.min(bass_str + 2);
                let mut s = bass_str + 1;
                while s <= hi {
                    if s >= 0 && (s as usize) < 6 && v[s as usize].is_some() && s != bass_str {
                        found = s;
                        break;
                    }
                    s += 1;
                }
                found
            };
            let pick = |s: i32, ev: &mut [Vec<GEvent>; 6]| {
                if !(0..=5).contains(&s) {
                    return;
                }
                let mut ss = s;
                while ss >= 0 && v[ss as usize].is_none() {
                    ss -= 1;
                }
                if ss < 0 {
                    return;
                }
                ev[ss as usize].push(GEvent { t, m: v[ss as usize], v: vv, stop: false });
            };
            match kind {
                "B" => pick(bass_str, &mut ev),
                "B2" => pick(alt_str, &mut ev),
                "g" => pick(3, &mut ev),
                "b" => pick(4, &mut ev),
                "e" => pick(5, &mut ev),
                _ => {
                    let down = kind == "D" || kind == "d";
                    let lite = kind == "d" || kind == "u";
                    let mut strs: Vec<usize> = (0..6usize).filter(|&s| v[s].is_some() && (kind == "D" || s >= if lite { 3 } else { 2 })).collect();
                    if !down {
                        strs.reverse();
                    }
                    if kind == "U" || kind == "u" {
                        strs.truncate(4);
                    }
                    let spread = if last { 0.028 } else if down { 0.009 } else { 0.007 };
                    for (k, &s) in strs.iter().enumerate() {
                        let tt = t + k as f64 * spread * (0.8 + r.next() * 0.4);
                        let vvv = vv * (if down { 1.0 } else { 0.75 }) * (if k == 0 && down { 1.05 } else { 1.0 });
                        ev[s].push(GEvent { t: tt, m: v[s], v: vvv, stop: false });
                    }
                }
            }
        }
    }

    // chord-change stops: a fretted string stops when the chord changes and
    // its note is not in the new voicing.
    for sg in &tl.segs {
        let v = voicing_of(&sg.chord, &mut voicing_cache);
        let t = tl.to_time(sg.b0) - 0.015;
        for s in 0..6 {
            ev[s].push(GEvent { t, m: v[s], v: 0.0, stop: true });
        }
    }

    let len = (tl.end * SR_F).ceil() as usize;
    let mut out = vec![0f32; len];
    for s in 0..6 {
        let e = &mut ev[s];
        e.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        for k in 0..e.len() {
            if e[k].stop {
                continue;
            }
            let x_t = e[k].t;
            let x_m = e[k].m;
            let mut t_end = x_t + 6.0;
            for j in (k + 1)..e.len() {
                let y = &e[j];
                if !y.stop {
                    t_end = y.t + 0.004;
                    break;
                }
                if y.m != x_m {
                    t_end = y.t + 0.03;
                    break;
                }
            }
            let f = mtof(x_m.unwrap() as f64);
            let start = round(x_t * SR_F) as i64;
            let l = round((t_end - x_t) * SR_F) as i64;
            let pick_draw = 0.11 + r.next() * 0.07;
            let detune_draw = 1.0 + r.next() * 0.8;
            let o = PluckOpts {
                amp: e[k].v,
                bright: Some(0.6 + 0.25 * e[k].v),
                damp: Some(tuning.gt.damp),
                glide: tuning.gt.glide * e[k].v,
                atk_noise: tuning.gt.atk * e[k].v,
                t60: 7.0 * pow(82.0 / f, 0.45),
                pick: Some(pick_draw),
                noise: Some(0.06),
                detune: Some(detune_draw),
                rel: 0.02,
                rel_t: 0.08,
            };
            pluck(&mut out, start, f, l, &o, &mut r);
        }
    }

    if truthy(tuning.gt.symp) {
        let opens = [40, 45, 50, 55, 59, 64];
        let mut y = vec![0f32; len];
        for m in opens {
            let n_period = SR_F / mtof(m as f64);
            let l = n_period.floor() as usize;
            let fr = n_period - l as f64;
            let mut buf = vec![0f32; l + 2];
            let mut q = 0usize;
            let mut lp = 0f64;
            let g = pow(10.0, -3.0 / (3.5 * mtof(m as f64)));
            for i in 0..len {
                let a = buf[q] as f64;
                let b = buf[(q + 1) % (l + 2)] as f64;
                let d = a + (b - a) * fr;
                lp = 0.5 * d + 0.5 * lp;
                let v = out[i] as f64 * 0.012 * tuning.gt.symp + g * lp;
                buf[(q + l) % (l + 2)] = v as f32;
                // JS parity: `y[i]+=d` on a Float32Array is one rounding of
                // the f64 sum (y[i] as f64 + d), not f32(d) added to y[i].
                y[i] = (y[i] as f64 + d) as f32;
                q = (q + 1) % (l + 2);
            }
        }
        for i in 0..len {
            out[i] += y[i];
        }
    }

    out
}

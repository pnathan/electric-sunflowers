//! The accompaniment guitar: fretboard voicing search, strum and picking
//! patterns per meter, and the plucked-string rendering with sympathetic
//! open strings.

use compose::form::Form;
use compose::timeline::Timeline;
use dsp::pluck::{pluck, PluckOpts};
use dsp::truthy;
use sfcore::js::{pow, round};
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use sfcore::SR_F;
use song::{Chord, ChordId, DrumKit, GuitarPattern, Meter, Pc, PcSet, SectionKind, Song};

/// `mtof(m)` from engine.js. Shared with bass.rs and harp.rs.
pub(crate) fn mtof(m: f64) -> f64 {
    440.0 * pow(2.0, (m - 69.0) / 12.0)
}

const GTUNE: [i32; 6] = [40, 45, 50, 55, 59, 64];

/// Frets on the open string `o` that sound a chord tone at hand position
/// `pos`: open, or one of the four frets from `pos`.
fn opts_for(o: i32, pos: i32, pcs: PcSet) -> Vec<i32> {
    let mut a = Vec::new();
    for f in [0, pos, pos + 1, pos + 2, pos + 3] {
        if f >= 0 && pcs.contains(Pc::new(o + f)) && !a.contains(&f) {
            a.push(f);
        }
    }
    a
}

/// Backtracks over strings `bs0+1..6`, trying each fret option for the
/// string, then no note on it, in that order (the first candidate to beat
/// the best score strictly wins ties). Scores: -6 per missing essential
/// tone, -0.6 without the fifth, -6 for a slash chord without its root,
/// -0.3 per position, +0.45 per open string, -1.6 per muted inner string
/// (-1.2 the top one), -0.25 per bass-string index, -0.5 for a doubled
/// third, -2 under four notes; at most four fretted notes spanning three
/// frets.
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
        for e in ch.essential.iter() {
            if have & (1 << e.get()) == 0 {
                sc -= 6.0;
            }
        }
        if let Some(fifth) = ch.fifth {
            if have & (1 << fifth.get()) == 0 {
                sc -= 0.6;
            }
        }
        if ch.bass != ch.root && have & (1 << ch.root.get()) == 0 {
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
            if notes.iter().filter(|x| x.map(Pc::new) == Some(third)).count() > 1 {
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

/// The best open-position or barre voicing of `ch` over hand positions
/// 0-9 with the bass note on one of the three lowest strings: MIDI note per
/// string, `None` for a muted string. Without a playable shape: root, third
/// (or fifth) and fifth from C3 up on the middle strings.
pub fn guitar_voicing(ch: &Chord) -> [Option<i32>; 6] {
    let pcs = ch.tones.with(ch.bass);
    let mut best: Option<[Option<i32>; 6]> = None;
    let mut bs = -1e9f64;
    for pos in 0..=9i32 {
        let opts: [Vec<i32>; 6] = {
            let mut o: [Vec<i32>; 6] = Default::default();
            for i in 0..6 {
                o[i] = opts_for(GTUNE[i], pos, pcs);
            }
            o
        };
        for bs0 in 0..=2usize {
            for &bf in &opts[bs0] {
                if Pc::new(GTUNE[bs0] + bf) != ch.bass {
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
            let r = 48 + (ch.root.get() as i32 - 48).rem_euclid(12);
            b[2] = Some(r);
            b[3] = Some(r + ch.third.map_or(7, |t| ch.root.interval_to(t) as i32));
            b[4] = Some(r + 7);
            b
        }
    }
}

/// One event on a string: a note, or a stop (damping) at a chord change.
struct GEvent {
    t: f64,
    m: Option<i32>,
    v: f64,
    stop: bool,
}

/// One stroke of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stroke {
    /// Full down strum over every sounding string.
    Down,
    /// Up strum over the top four strings from string 2 up.
    Up,
    /// Light down strum from string 3 up.
    DownLite,
    /// Light up strum over the top four strings from string 3 up.
    UpLite,
    /// Bass note on the lowest sounding string.
    Bass,
    /// Alternate bass: the next sounding string up (at most string 3).
    AltBass,
    /// Pick string 3 (G), 4 (B) or 5 (high E), or the nearest sounding one below.
    G,
    B,
    E,
}

/// Pattern played in a bar: the song's pattern, or a lighter strum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pat {
    Strum,
    StrumLite,
    Fingerpick,
    Travis,
}

impl Pat {
    fn of(g: GuitarPattern) -> Pat {
        match g {
            GuitarPattern::Strum => Pat::Strum,
            GuitarPattern::Fingerpick | GuitarPattern::Arpeggio => Pat::Fingerpick,
            GuitarPattern::Travis => Pat::Travis,
        }
    }
}

/// (slot, stroke, velocity); slots are grid steps of the meter.
type Pattern = &'static [(f64, Stroke, f64)];

fn gpat(pat: Pat, meter: Meter) -> Pattern {
    use Stroke::{AltBass as B2, Bass as Bs, Down as D, DownLite as Dl, Up as U, UpLite as Ul, B, E, G};
    const STRUM_44: Pattern = &[(0.0, D, 0.85), (2.0, D, 0.7), (3.0, U, 0.45), (5.0, U, 0.5), (6.0, D, 0.65), (7.0, U, 0.45)];
    const STRUM_34: Pattern = &[(0.0, Bs, 0.9), (2.0, D, 0.6), (4.0, D, 0.6), (5.0, U, 0.4)];
    const STRUM_68: Pattern = &[(0.0, D, 0.85), (2.0, U, 0.4), (3.0, D, 0.7), (4.0, U, 0.4), (5.0, U, 0.45)];
    const STRUMLITE_44: Pattern = &[(0.0, Bs, 0.85), (2.0, Dl, 0.55), (4.0, B2, 0.75), (6.0, Dl, 0.55), (7.0, Ul, 0.35)];
    const STRUMLITE_34: Pattern = &[(0.0, Bs, 0.85), (2.0, Dl, 0.5), (4.0, Dl, 0.5)];
    const STRUMLITE_68: Pattern = &[(0.0, Bs, 0.85), (2.0, Dl, 0.45), (3.0, B2, 0.7), (5.0, Dl, 0.45)];
    const FP_44: Pattern =
        &[(0.0, Bs, 0.85), (1.0, G, 0.5), (2.0, B, 0.55), (3.0, E, 0.6), (4.0, B2, 0.75), (5.0, B, 0.5), (6.0, G, 0.5), (7.0, B, 0.5)];
    const FP_34: Pattern = &[(0.0, Bs, 0.85), (1.0, G, 0.5), (2.0, B, 0.55), (3.0, E, 0.6), (4.0, B, 0.5), (5.0, G, 0.5)];
    const FP_68: Pattern = &[(0.0, Bs, 0.85), (1.0, G, 0.5), (2.0, B, 0.55), (3.0, E, 0.6), (4.0, B, 0.5), (5.0, G, 0.5)];
    const TRAVIS_44: Pattern = &[
        (0.0, Bs, 0.85),
        (0.0, E, 0.55),
        (1.0, B, 0.45),
        (2.0, B2, 0.75),
        (3.0, G, 0.5),
        (4.0, Bs, 0.8),
        (5.0, E, 0.5),
        (6.0, B2, 0.75),
        (7.0, B, 0.45),
    ];
    const TRAVIS_34: Pattern =
        &[(0.0, Bs, 0.85), (0.0, E, 0.55), (1.0, B, 0.45), (2.0, B2, 0.7), (3.0, G, 0.5), (4.0, B2, 0.7), (5.0, B, 0.45)];
    const TRAVIS_68: Pattern =
        &[(0.0, Bs, 0.85), (0.0, E, 0.5), (1.0, G, 0.45), (2.0, B, 0.5), (3.0, B2, 0.75), (4.0, B, 0.45), (5.0, G, 0.45)];
    match (pat, meter) {
        (Pat::Strum, Meter::Four4) => STRUM_44,
        (Pat::Strum, Meter::Three4) => STRUM_34,
        (Pat::Strum, Meter::Six8) => STRUM_68,
        (Pat::StrumLite, Meter::Four4) => STRUMLITE_44,
        (Pat::StrumLite, Meter::Three4) => STRUMLITE_34,
        (Pat::StrumLite, Meter::Six8) => STRUMLITE_68,
        (Pat::Fingerpick, Meter::Four4) => FP_44,
        (Pat::Fingerpick, Meter::Three4) => FP_34,
        (Pat::Fingerpick, Meter::Six8) => FP_68,
        (Pat::Travis, Meter::Four4) => TRAVIS_44,
        (Pat::Travis, Meter::Three4) => TRAVIS_34,
        (Pat::Travis, Meter::Six8) => TRAVIS_68,
    }
}

/// The accompaniment guitar track: one voicing per chord, the song's
/// pattern per bar (lighter strums in quiet sections, fingerpicking in the
/// bridge), strings damped at chord changes, sympathetic open strings.
pub fn gen_guitar(song: &Song, form: &Form, tl: &Timeline, seed: u32, tuning: &Tuning) -> Vec<f32> {
    let mut r = rng_for(seed, "gtr");
    let bpb = form.bpb();
    let sub = form.sub();
    let mut ev: [Vec<GEvent>; 6] = Default::default();
    let nbars = form.bars.len();
    // Voicings memoised per chord (the search is deterministic).
    let mut voicing_cache: std::collections::HashMap<ChordId, [Option<i32>; 6]> = std::collections::HashMap::new();
    let voicing_of = |id: ChordId, cache: &mut std::collections::HashMap<ChordId, [Option<i32>; 6]>| -> [Option<i32>; 6] {
        *cache.entry(id).or_insert_with(|| guitar_voicing(form.chord(id)))
    };

    for bi in 0..nbars {
        let bar = &form.bars[bi];
        let sec = &form.sections[bar.sec];
        let intensity = sec.intensity.level();
        let mut style = Pat::of(song.guitar);
        if style == Pat::Strum && intensity <= 1 {
            style = Pat::StrumLite;
        }
        let cond_style = matches!(style, Pat::Fingerpick | Pat::Travis);
        if cond_style && intensity >= 3 && song.band.drums != DrumKit::None {
            // A draw that never switches to a strum (uniform draws are >= 0);
            // it only keeps the stream in step.
            let draw = r.next();
            if draw < 0.0 {
                style = Pat::Strum;
            }
        }
        if sec.kind == SectionKind::Bridge && style == Pat::Strum {
            style = Pat::Fingerpick;
        }
        let pat = gpat(style, form.meter);
        let last = bi == nbars - 1;
        let vel_s = 0.72 + 0.1 * intensity as f64;
        const LAST_BAR: Pattern = &[(0.0, Stroke::Down, 0.8)];
        let events: Pattern = if last { LAST_BAR } else { pat };

        for &(slot, kind, vel) in events {
            let beat = bi as f64 * bpb as f64 + slot / sub as f64;
            let v = voicing_of(tl.chord_id_at(form, beat + 0.01), &mut voicing_cache);
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
                Stroke::Bass => pick(bass_str, &mut ev),
                Stroke::AltBass => pick(alt_str, &mut ev),
                Stroke::G => pick(3, &mut ev),
                Stroke::B => pick(4, &mut ev),
                Stroke::E => pick(5, &mut ev),
                Stroke::Down | Stroke::Up | Stroke::DownLite | Stroke::UpLite => {
                    let down = matches!(kind, Stroke::Down | Stroke::DownLite);
                    let lite = matches!(kind, Stroke::DownLite | Stroke::UpLite);
                    let mut strs: Vec<usize> = (0..6usize)
                        .filter(|&s| v[s].is_some() && (kind == Stroke::Down || s >= if lite { 3 } else { 2 }))
                        .collect();
                    if !down {
                        strs.reverse();
                    }
                    if matches!(kind, Stroke::Up | Stroke::UpLite) {
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
        let v = voicing_of(sg.chord, &mut voicing_cache);
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

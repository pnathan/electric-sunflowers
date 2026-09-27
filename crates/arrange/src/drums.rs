//! Port of the drums part of engine.js: `drumHit` and `genDrums`
//! (engine.js lines ~782-822).

use compose::form::Form;
use compose::song::Song;
use compose::timeline::Timeline;
use dsp::filter::{bq, run_bq, FilterType};
use dsp::noise::noise_buf;
use dsp::pan::add_pan;
use sfcore::js;
use sfcore::rng::{rng_for, Rng};
use sfcore::SR_F;

/// `drumHit(type,vel,r,extra)`: one percussive one-shot, mono. `extra` is
/// `None` where JS omits the argument (0/NaN/undefined all fall back the
/// same way in the branches that use it, via `extra||default`).
pub fn drum_hit(kind: &str, vel: f64, r: &mut Rng, extra: Option<f64>) -> Vec<f32> {
    let mut s: Vec<f32>;
    match kind {
        "kick" => {
            let n = js::round(0.45 * SR_F) as usize;
            s = vec![0.0f32; n];
            let mut ph = 0.0f64;
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                ph += 2.0 * std::f64::consts::PI * (47.0 + 72.0 * js::exp(-t / 0.034)) / SR_F;
                let mut x = js::sin(ph) * js::exp(-t / 0.17);
                if t < 0.004 {
                    x += (r.next() * 2.0 - 1.0) * 0.25 * js::exp(-t / 0.0012);
                }
                *v = js::f32r(x) as f32;
            }
        }
        "snare" => {
            let n = js::round(0.3 * SR_F) as usize;
            let mut nz = noise_buf(n, r);
            run_bq(&mut nz, &bq(FilterType::Hp, 1400.0, 0.7, 0.0));
            s = vec![0.0f32; n];
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                let x = js::sin(2.0 * std::f64::consts::PI * 188.0 * t) * 0.45 * js::exp(-t / 0.05)
                    + nz[i] as f64 * 0.7 * js::exp(-t / 0.11);
                *v = js::f32r(x) as f32;
            }
        }
        "rim" => {
            let n = js::round(0.12 * SR_F) as usize;
            s = noise_buf(n, r);
            run_bq(&mut s, &bq(FilterType::Bp, 1800.0, 6.0, 0.0));
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                let x = *v as f64 * 3.0 * js::exp(-t / 0.02)
                    + js::sin(2.0 * std::f64::consts::PI * 520.0 * t) * 0.4 * js::exp(-t / 0.015);
                *v = js::f32r(x) as f32;
            }
        }
        "tap" => {
            let n = js::round(0.25 * SR_F) as usize;
            let mut nz = noise_buf(n, r);
            run_bq(&mut nz, &bq(FilterType::Bp, 3800.0, 0.9, 0.0));
            let mut hd = noise_buf(n, r);
            run_bq(&mut hd, &bq(FilterType::Bp, 190.0, 6.0, 0.0));
            let mut h2 = noise_buf(n, r);
            run_bq(&mut h2, &bq(FilterType::Bp, 330.0, 5.0, 0.0));
            s = vec![0.0f32; n];
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                let x = (js::sin(2.0 * std::f64::consts::PI * 186.0 * t) * 0.35
                    + hd[i] as f64 * 1.6
                    + h2[i] as f64 * 0.9)
                    * js::exp(-t / 0.07)
                    * js::min(1.0, t / 0.002)
                    + nz[i] as f64 * 0.75 * js::exp(-t / 0.1) * js::min(1.0, t / 0.004);
                *v = js::f32r(x) as f32;
            }
        }
        "swish" => {
            let dur = dsp::or_default(extra.unwrap_or(0.0), 0.5);
            let n = js::round(dur * SR_F) as usize;
            s = noise_buf(n, r);
            run_bq(&mut s, &bq(FilterType::Bp, 3600.0, 0.7, 0.0));
            run_bq(&mut s, &bq(FilterType::Hs, 7000.0, 0.7, -6.0));
            let mut hd = noise_buf(n, r);
            run_bq(&mut hd, &bq(FilterType::Bp, 200.0, 4.0, 0.0));
            for i in 0..n {
                let x = i as f64 / n as f64;
                let e = js::pow(js::sin(std::f64::consts::PI * x), 1.5)
                    * (0.6 + 0.4 * js::sin(2.0 * std::f64::consts::PI * x).abs());
                let y = (s[i] as f64 * 0.2 + hd[i] as f64 * 0.25) * e;
                s[i] = js::f32r(y) as f32;
            }
        }
        "hat" => {
            let n = js::round(0.12 * SR_F) as usize;
            s = noise_buf(n, r);
            run_bq(&mut s, &bq(FilterType::Hp, 7200.0, 0.7, 0.0));
            for (i, v) in s.iter_mut().enumerate() {
                let x = *v as f64 * 0.7 * js::exp(-(i as f64 / SR_F) / 0.032);
                *v = js::f32r(x) as f32;
            }
        }
        "shaker" => {
            let n = js::round(0.12 * SR_F) as usize;
            s = noise_buf(n, r);
            run_bq(&mut s, &bq(FilterType::Bp, 6200.0, 1.3, 0.0));
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                let x = *v as f64 * 1.4 * js::min(1.0, t / 0.012) * js::exp(-t / 0.045);
                *v = js::f32r(x) as f32;
            }
        }
        "tom" => {
            let f = dsp::or_default(extra.unwrap_or(0.0), 110.0);
            let n = js::round(0.5 * SR_F) as usize;
            s = vec![0.0f32; n];
            let mut ph = 0.0f64;
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                ph += 2.0 * std::f64::consts::PI * f * (1.0 + 0.35 * js::exp(-t / 0.04)) / SR_F;
                let x = js::sin(ph) * js::exp(-t / 0.28)
                    + (r.next() * 2.0 - 1.0) * 0.05 * js::exp(-t / 0.02);
                *v = js::f32r(x) as f32;
            }
        }
        "ride" => {
            let n = js::round(1.6 * SR_F) as usize;
            let fr: [f64; 6] = [421.0, 601.0, 793.0, 1033.0, 1285.0, 1559.0].map(|x| x * 1.9);
            let mut ph: [f64; 6] = [0.0; 6];
            for p in ph.iter_mut() {
                *p = r.next();
            }
            s = vec![0.0f32; n];
            for v in s.iter_mut() {
                let mut acc = 0.0f64;
                for k in 0..6 {
                    ph[k] += fr[k] / SR_F;
                    if ph[k] >= 1.0 {
                        ph[k] -= 1.0;
                    }
                    acc += if ph[k] < 0.5 { 1.0 } else { -1.0 };
                }
                *v = js::f32r(acc / 6.0) as f32;
            }
            run_bq(&mut s, &bq(FilterType::Hp, 3500.0, 0.7, 0.0));
            let mut ping = noise_buf(n, r);
            run_bq(&mut ping, &bq(FilterType::Bp, 5200.0, 4.0, 0.0));
            for (i, v) in s.iter_mut().enumerate() {
                let t = i as f64 / SR_F;
                let x = *v as f64 * 0.35 * js::exp(-t / 0.7) + ping[i] as f64 * 0.6 * js::exp(-t / 0.04);
                *v = js::f32r(x) as f32;
            }
        }
        other => panic!("unknown drum type {other}"),
    }
    for v in s.iter_mut() {
        *v = js::f32r(*v as f64 * vel) as f32;
    }
    s
}

/// One `hit(...)` call inside `genDrums`'s inner closure: draws the timing
/// jitter and velocity jitter from `r` (in that order, matching JS argument
/// evaluation order) before calling `drumHit`, then pans the result into
/// `l`/`rbuf`.
#[allow(clippy::too_many_arguments)]
fn hit(
    l: &mut [f32],
    rbuf: &mut [f32],
    tl: &Timeline,
    r: &mut Rng,
    kind: &str,
    beat: f64,
    vel: f64,
    pan: f64,
    extra: Option<f64>,
) {
    let t = tl.to_time(beat) + (r.next() - 0.5) * 0.008;
    let v = vel * (0.9 + r.next() * 0.2);
    let sig = drum_hit(kind, v, r, extra);
    add_pan(l, rbuf, js::round(t * SR_F) as i64, &sig, pan, 1.0);
}

/// `genDrums(song,form,tl,seed)`.
pub fn gen_drums(song: &Song, form: &Form, tl: &Timeline, seed: u32) -> [Vec<f32>; 2] {
    let style = song.band.drums.as_str();
    let len = (tl.end * SR_F).ceil() as usize;
    let mut l = vec![0.0f32; len];
    let mut rbuf = vec![0.0f32; len];
    if style == "none" {
        return [l, rbuf];
    }
    let mut r = rng_for(seed, "drm");
    let bpb = form.mi.bpb;
    let sub = form.mi.sub;
    let meter = song.meter_name.as_str();
    let nb = form.bars.len();

    for bi in 0..nb {
        let sec_idx = form.bars[bi].sec;
        let sec = &form.sections[sec_idx];
        let mut i_ = sec.intensity;
        if style != "full" {
            i_ -= 1;
        }
        if sec.type_ == "bridge" {
            i_ = i_.min(1);
        }
        if i_ < 1 {
            continue;
        }
        let b0 = (bi as i32 * bpb) as f64;
        let last = bi == nb - 1;
        if last {
            hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0, 0.7, 0.0, None);
            let ty = if style == "brushes" { "swish" } else { "ride" };
            hit(&mut l, &mut rbuf, tl, &mut r, ty, b0, 0.5, 0.35, Some(1.2));
            continue;
        }
        let sec_end = bi + 1 == sec.start_bar + sec.n_bars;
        let fill = sec_end
            && sec
                .next
                .map(|ni| form.sections[ni].intensity > sec.intensity)
                .unwrap_or(false)
            && i_ >= 1;

        if style == "brushes" {
            for b in 0..bpb {
                hit(
                    &mut l,
                    &mut rbuf,
                    tl,
                    &mut r,
                    "swish",
                    b0 + b as f64,
                    0.35 + 0.1 * i_ as f64,
                    -0.1,
                    Some(tl.beat_dur(b0 + b as f64) * 0.95),
                );
            }
            if meter == "4/4" {
                hit(&mut l, &mut rbuf, tl, &mut r, "tap", b0 + 1.0, 0.5, -0.15, None);
                hit(&mut l, &mut rbuf, tl, &mut r, "tap", b0 + 3.0, 0.5, -0.15, None);
                if i_ >= 2 {
                    hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0, 0.45, 0.0, None);
                    hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0 + 2.0, 0.35, 0.0, None);
                }
            } else if meter == "3/4" {
                hit(&mut l, &mut rbuf, tl, &mut r, "tap", b0 + 1.0, 0.35, -0.15, None);
                hit(&mut l, &mut rbuf, tl, &mut r, "tap", b0 + 2.0, 0.35, -0.15, None);
                if i_ >= 2 {
                    hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0, 0.45, 0.0, None);
                }
            } else {
                hit(&mut l, &mut rbuf, tl, &mut r, "tap", b0 + 1.0, 0.5, -0.15, None);
                if i_ >= 2 {
                    hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0, 0.45, 0.0, None);
                }
            }
        } else {
            let kick_p: Vec<f64> = if meter == "4/4" {
                if i_ >= 3 {
                    vec![0.0, 1.5, 2.0]
                } else {
                    vec![0.0, 2.0]
                }
            } else {
                vec![0.0]
            };
            let sn_p: Vec<f64> = if meter == "4/4" {
                vec![1.0, 3.0]
            } else if meter == "3/4" {
                vec![1.0, 2.0]
            } else {
                vec![1.0]
            };
            for &k in &kick_p {
                hit(&mut l, &mut rbuf, tl, &mut r, "kick", b0 + k, 0.75, 0.0, None);
            }
            let sn = if style == "soft" { "rim" } else { "snare" };
            let sn_vel = if meter == "3/4" { 0.45 } else { 0.6 };
            for &k in &sn_p {
                if fill && k >= (bpb - 1) as f64 {
                    continue;
                }
                hit(&mut l, &mut rbuf, tl, &mut r, sn, b0 + k, sn_vel, -0.12, None);
            }
            if i_ >= 2 {
                let kind = if style == "soft" { "shaker" } else { "hat" };
                for s in 0..(bpb * sub) {
                    let vel = if s % sub == 0 { 0.55 } else { 0.35 };
                    hit(
                        &mut l,
                        &mut rbuf,
                        tl,
                        &mut r,
                        kind,
                        b0 + s as f64 / sub as f64,
                        vel,
                        0.45,
                        None,
                    );
                }
            }
            if i_ >= 3 && style == "full" {
                for b in 0..bpb {
                    hit(&mut l, &mut rbuf, tl, &mut r, "ride", b0 + b as f64, 0.35, 0.4, None);
                }
            }
        }

        if fill {
            let fb = b0 + (bpb - 1) as f64;
            let steps = if sub == 3 { 3 } else { 4 };
            let freqs = [180.0, 150.0, 120.0, 95.0];
            for k in 0..steps {
                let f = freqs[k % 4];
                let kind = if style == "brushes" { "tap" } else { "tom" };
                hit(
                    &mut l,
                    &mut rbuf,
                    tl,
                    &mut r,
                    kind,
                    fb + k as f64 / steps as f64,
                    0.45 + k as f64 * 0.08,
                    -0.3 + k as f64 * 0.2,
                    Some(f),
                );
            }
        }
        if bi == sec.start_bar && sec.intensity >= 3 && style != "brushes" {
            hit(&mut l, &mut rbuf, tl, &mut r, "ride", b0, 0.55, 0.4, None);
        }
    }
    [l, rbuf]
}

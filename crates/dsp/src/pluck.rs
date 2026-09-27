//! `pluck`, `ksPluck` (engine.js lines ~987-1030): plucked string, two
//! polarisations, shaped excitation run in the velocity domain, frequency-dependent
//! loss (a Karplus-Strong loop with a tuned all-pass for fractional delay).

use sfcore::js;
use sfcore::rng::Rng;
use sfcore::SR_F;

/// The `o` option bag `pluck` destructures. Defaults match the JS
/// `o.field==null?default:o.field` / `o.field||default` reads.
#[derive(Clone, Copy, Debug)]
pub struct PluckOpts {
    pub amp: f64,
    pub t60: f64,
    /// `pick` in JS (string pluck position, 0..0.5); `None` -> 0.15.
    pub pick: Option<f64>,
    /// `None` -> 0.5.
    pub bright: Option<f64>,
    /// `None` -> `0.42 - 0.36*bright`.
    pub damp: Option<f64>,
    /// `None` -> 0.08.
    pub noise: Option<f64>,
    /// `None` -> 1.4.
    pub detune: Option<f64>,
    /// `0.0` (JS falsy) -> 0.03.
    pub rel: f64,
    /// `0.0` (JS falsy) -> 0.09.
    pub rel_t: f64,
    /// `0.0` (JS falsy) -> no glide.
    pub glide: f64,
    /// `0.0` (JS falsy) -> no attack noise burst.
    pub atk_noise: f64,
}

impl Default for PluckOpts {
    fn default() -> Self {
        PluckOpts {
            amp: 1.0,
            t60: 1.0,
            pick: None,
            bright: None,
            damp: None,
            noise: None,
            detune: None,
            rel: 0.0,
            rel_t: 0.0,
            glide: 0.0,
            atk_noise: 0.0,
        }
    }
}

/// `pluck(out,start,f,len,o)`: adds the plucked-string signal into `out` (a
/// Float32Array in JS) starting at sample `start`.
pub fn pluck(out: &mut [f32], start: i64, f: f64, len: i64, o: &PluckOpts, r: &mut Rng) {
    if start < 0 || start as usize >= out.len() || f < 20.0 {
        return;
    }
    let len = len.min(out.len() as i64 - start);
    let bright = o.bright.unwrap_or(0.5);
    let damp = o.damp.unwrap_or(0.42 - 0.36 * bright);
    let beta = js::clamp(o.pick.unwrap_or(0.15), 0.04, 0.5);
    let noise = o.noise.unwrap_or(0.08);
    let det = o.detune.unwrap_or(1.4);
    let rel = if o.rel != 0.0 { o.rel } else { 0.03 };
    let rel_t = if o.rel_t != 0.0 { o.rel_t } else { 0.09 };
    let rel_n = js::round(rel * SR_F) as i64;
    let rel_start = len - rel_n;

    for pol in 0..2i32 {
        let fp = f * js::pow(2.0, (if pol != 0 { det } else { -det * 0.25 }) / 1200.0);
        let n_period = SR_F / fp;
        let w0 = 2.0 * std::f64::consts::PI * fp / SR_F;
        let t60 = o.t60 * (if pol != 0 { 0.62 } else { 1.0 });
        let rho = js::pow(10.0, -3.0 / (t60 * fp));
        let hm = |p: f64| (1.0 - p) / (1.0 - 2.0 * p * js::cos(w0) + p * p).sqrt();
        let mut p = damp;
        let lim = rho.sqrt();
        let mut guard = 0;
        while p > 0.002 && hm(p) < lim && guard < 40 {
            p *= 0.85;
            guard += 1;
        }
        let g = js::min(0.99995, rho / hm(p));
        let tau = js::atan2(p * js::sin(w0), 1.0 - p * js::cos(w0)) / w0;
        let l_f = (n_period - tau - 0.5).floor();
        if l_f < 3.0 {
            return;
        }
        let l = l_f as usize;
        let d_frac = n_period - tau - l_f;
        let mut c_ap = (1.0 - d_frac) / (1.0 + d_frac);
        let gl = o.glide * (if pol != 0 { 0.8 } else { 1.0 });

        // excitation: plucked-string shape (triangle, apex at beta) smoothed by
        // finger/pick width, plus noise
        let mut buf = vec![0.0f32; l];
        let p_apex = (1usize).max(js::round(beta * l as f64) as usize);
        for i in 0..l {
            let tri = if i < p_apex { i as f64 / p_apex as f64 } else { (l - i) as f64 / (l - p_apex) as f64 };
            buf[i] = js::f32r(tri + (r.next() * 2.0 - 1.0) * noise) as f32;
        }
        let w = (1usize).max(js::round(l as f64 * 0.008 * (1.0 + 2.0 * (1.0 - bright))) as usize);
        if w > 1 {
            let tmp = buf.clone();
            let mut acc = 0.0f64;
            let li = l as i64;
            let wi = w as i64;
            for i in (-wi + 1)..=0 {
                acc += tmp[((i + li) % li) as usize] as f64;
            }
            for i in 0..l {
                buf[i] = js::f32r(acc / w as f64) as f32;
                let ii = i as i64;
                acc += tmp[((ii + 1) % li) as usize] as f64 - tmp[(((ii - wi + 1 + li) % li)) as usize] as f64;
            }
        }
        // run the loop in the velocity domain (bridge force ~ string slope): load
        // the derivative of the shape
        {
            let sh = buf.clone();
            for i in 0..l {
                let prev = sh[(i + l - 1) % l] as f64;
                buf[i] = js::f32r(sh[i] as f64 - prev) as f32;
            }
        }
        // release time of finger or pick: circular one-pole low-pass on the excitation
        {
            let fc = 1500.0 + 14000.0 * bright * bright;
            let a = 1.0 - js::exp(-2.0 * std::f64::consts::PI * fc / SR_F);
            let mut z = 0.0f64;
            for pass in 0..2 {
                for i in 0..l {
                    z += a * (buf[i] as f64 - z);
                    if pass == 1 {
                        buf[i] = js::f32r(z) as f32;
                    }
                }
            }
            let mut m = 0.0f64;
            for i in 0..l {
                m += buf[i] as f64;
            }
            m /= l as f64;
            for i in 0..l {
                buf[i] = js::f32r(buf[i] as f64 - m) as f32;
            }
        }
        let mut vpk = 1e-9f64;
        for i in 0..l {
            let v = (buf[i] as f64).abs();
            if v > vpk {
                vpk = v;
            }
        }
        let gain = o.amp * (if pol != 0 { 0.42 } else { 1.0 }) / vpk * js::min(1.0, l as f64 / 60.0);
        if o.atk_noise != 0.0 && pol == 0 {
            let an = js::round(0.004 * SR_F) as i64;
            let mut hpz = 0.0f64;
            let mut i = 0i64;
            while i < an && start + i < out.len() as i64 {
                let ww = r.next() * 2.0 - 1.0;
                let h = ww - hpz;
                hpz = ww;
                let idx = (start + i) as usize;
                let add = h * o.atk_noise * o.amp * 0.35 * js::exp(-(i as f64) / (0.0012 * SR_F));
                out[idx] = js::f32r(out[idx] as f64 + add) as f32;
                i += 1;
            }
        }
        let atk = (8i64).max(js::round(SR_F * (0.0004 + 0.0016 * (1.0 - bright))) as i64);

        let mut q = 0usize;
        let mut lp = buf[l - 1] as f64;
        let mut ax = g * buf[l - 1] as f64;
        let mut ay = g * buf[l - 1] as f64;
        let mut gg = g;
        let mut pp = p;
        let mut peak0 = -1.0f64;
        let mut pk = 0.0f64;
        let mut chk = 0i64;

        let mut nn = -(l as i64);
        while nn < len {
            if nn < 0 {
                let cur = buf[q] as f64;
                lp = (1.0 - pp) * cur + pp * lp;
                let v = gg * lp;
                let y = c_ap * v + ax - c_ap * ay;
                ax = v;
                ay = y;
                buf[q] = js::f32r(y) as f32;
                q += 1;
                if q == l {
                    q = 0;
                }
                nn += 1;
                continue;
            }
            if nn == rel_start {
                gg = js::pow(10.0, -3.0 / (rel_t * fp));
                pp = js::min(0.7, p + 0.3);
            }
            if gl != 0.0 && (nn & 31) == 0 {
                let e = gl * js::exp(-(nn as f64) / (0.07 * SR_F));
                let n_n = SR_F / (fp * js::pow(2.0, e / 1200.0));
                let dd = n_n - tau - l as f64;
                let dc = js::clamp(dd, 0.3, 1.7);
                c_ap = (1.0 - dc) / (1.0 + dc);
            }
            let cur = buf[q] as f64;
            lp = (1.0 - pp) * cur + pp * lp;
            let v = gg * lp;
            let y = c_ap * v + ax - c_ap * ay;
            ax = v;
            ay = y;
            buf[q] = js::f32r(y) as f32;
            q += 1;
            if q == l {
                q = 0;
            }
            let vel = cur;
            let e = (if nn > len - 64 { (len - nn) as f64 / 64.0 } else { 1.0 }) * (if nn < atk { nn as f64 / atk as f64 } else { 1.0 });
            let idx = (start + nn) as usize;
            out[idx] = js::f32r(out[idx] as f64 + vel * gain * e) as f32;
            let av = vel.abs();
            if av > pk {
                pk = av;
            }
            chk += 1;
            if chk == l as i64 {
                chk = 0;
                if peak0 < 0.0 {
                    peak0 = pk;
                } else if pk < peak0 * 2e-4 {
                    break;
                }
                pk = 0.0;
            }
            nn += 1;
        }
    }
}

/// `ksPluck(out,start,f,len,o)`: an alias for `pluck` in engine.js.
pub fn ks_pluck(out: &mut [f32], start: i64, f: f64, len: i64, o: &PluckOpts, r: &mut Rng) {
    pluck(out, start, f, len, o, r);
}

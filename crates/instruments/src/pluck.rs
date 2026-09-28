//! Plucked string: extended Karplus-Strong with a velocity-domain loop, two
//! polarisations, a shaped pick excitation and a loss filter designed for a
//! requested T60 (design section 5.4).
//!
//! Algorithm
//! - Karplus and Strong 1983, "Digital synthesis of plucked-string and drum
//!   timbres"; Jaffe and Smith 1983, "Extensions of the Karplus-Strong
//!   plucked-string algorithm" (loss filter, allpass tuning, pick position).
//! - The loop carries string velocity (bridge force follows string slope).
//!   A displacement loop differentiated at the output gave a spike every
//!   period, so the excitation is differentiated once, before the loop.
//! - Loss: one-pole low-pass `y = (1 - p) x + p y1` times a gain `g`. The
//!   pole starts at `damp` and is reduced by 15% steps until
//!   `|H(f0)| >= sqrt(rho)`, where `rho = 10^(-3 / (T60 f0))` is the per-period
//!   gain that makes the fundamental fall 60 dB in T60; then
//!   `g = min(0.99995, rho / |H(f0)|)` (loop-filter design after Valimaki,
//!   Huopaniemi, Karjalainen, Janosy 1996).
//! - Tuning: the loop delay `SR / f0` is the integer line length `L`, the
//!   one-pole phase delay at f0 (`dsp::delay::one_pole_phase_delay`), and a
//!   first-order Thiran allpass for the rest (Thiran 1971; Laakso, Valimaki,
//!   Karjalainen, Laine 1996). Without a glide the allpass delay delta is
//!   in [0.5, 1.5). With a glide, `L` is taken from the sharp starting
//!   period, so the allpass starts in [0.5, 1.5) and carries the whole glide
//!   (delta up to 1.5 plus the glide in samples, at most `DELTA_MAX`):
//!   `dsp::delay::Thiran1::with_max`. Its phase delay at D = 3 is 2.6
//!   samples at 0.5 rad/sample (3.5 kHz), which on the 538-sample loop of
//!   E2 moves the 42nd harmonic by 1.2 cents.
//! - Two polarisations, summed without coupling: the main one at
//!   `-detune / 4` cents with the full T60, the second at `+detune` cents,
//!   amplitude 0.42 and 0.62 T60. With the default detune 1.4 that is -0.35
//!   and +1.4 cents: a slow beat and a two-stage decay.
//! - Excitation, per polarisation, one period long: a triangle with its apex
//!   at the pick position plus white noise; a circular moving average of
//!   width `0.8% L (1 + 2 (1 - bright))` (finger or pick width);
//!   differentiated; a circular one-pole low-pass at
//!   `1500 + 14000 bright^2` Hz (pick release time), run twice round the
//!   period so it has no start transient; DC removed; normalised to unit
//!   peak; loaded by one warm-up pass through the loop.
//! - Options: first-difference high-passed pick noise over 4 ms with a 1.2 ms
//!   exponential decay (`attack_noise`); a tension pitch glide that starts
//!   `glide` cents sharp and relaxes with a 70 ms time constant (the second
//!   polarisation glides 0.8 as far); release damping over the last
//!   `release` seconds (loop T60 `release_t60`, pole raised by 0.3, at most
//!   0.7); a linear attack ramp of `0.4 + 1.6 (1 - bright)` ms, at least 8
//!   samples; a 64-sample fade at the end; an early stop when the peak of a
//!   period falls 74 dB below the first period's peak.
//! - DC: the loaded period is zero-mean and the filters start from zero
//!   state, so the loop output sums to about zero over a full decay (1e-8 of
//!   the peak as a mean). Within the note a slow DC component of up to about
//!   5e-4 of the peak rides on the sustain (the loop's DC mode, decaying at
//!   gain g per period) and cancels the onset. The attack ramp multiplies the velocity output, so it removes
//!   area from the onset: a one-time transient (net output sum up to about
//!   -40 peak-samples for the bass, 1.8e-4 of the peak as a mean over the
//!   note), not an offset. Ramping displacement instead removes it but adds
//!   a low-frequency onset pulse up to 2.2x the peak; the shipped velocity
//!   ramp is kept.
//! - High-note trim: the output gain is multiplied by `min(1, L / 60)`, which
//!   attenuates notes above SR / 60 = 735 Hz (6 dB per octave). Empirical,
//!   kept from the shipped sound.
//!
//! Structure: the note is cut into spans at the phase boundaries (warm-up,
//! attack, glide, sustain, release, fade) and at period ends (for the early
//! stop). Each span runs one inner loop with no branch per sample. The delay
//! line is `f32`; filter state is `f64`.

use dsp::delay::{one_pole_phase_delay, DelayLine, Thiran1};
use dsp::onepole::OnePole;
use sfcore::random::Rng;
use sfcore::SR_F;
use std::f64::consts::TAU;

use crate::guitar::TUNING;

/// Lowest fundamental rendered, Hz. Sets the delay-line capacity.
pub const F0_MIN: f64 = 20.0;

/// Longest integer loop length: `SR / F0_MIN`.
const L_MAX: usize = 2205;

/// Early stop: a period whose peak is below this fraction of the first
/// period's peak ends the note (-74 dB).
const STOP_RATIO: f64 = 2e-4;

/// End fade length in samples.
const FADE: usize = 64;

/// Glide control period in samples: the target delay is computed every
/// `GLIDE_CTRL` samples and ramped linearly per sample between.
const GLIDE_CTRL: usize = 32;

/// Glide time constant, seconds.
const GLIDE_TAU: f64 = 0.07;

/// The glide ends when its remaining offset falls below this many cents
/// (the step left at its end is far below the pitch JND of about 3 cents).
const GLIDE_FLOOR_CENTS: f64 = 0.02;

/// Largest allpass delay, samples. Leaves `GLIDE_MAX_SAMPLES` of glide
/// above the [0.5, 1.5) range of a note without glide.
const DELTA_MAX: f64 = 3.5;

/// Largest glide in samples of loop delay: a deeper glide is reduced to
/// this. 5 cents at E2 (the guitar's lowest note) is 1.55 samples.
const GLIDE_MAX_SAMPLES: f64 = DELTA_MAX - 1.5;

/// Second polarisation: amplitude, T60 factor, glide factor.
const POL2_AMP: f64 = 0.42;
const POL2_T60: f64 = 0.62;
const POL2_GLIDE: f64 = 0.8;

/// Largest loop gain; keeps the loop strictly lossy.
const G_MAX: f64 = 0.99995;

/// How a note's rendered length follows from the event (seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteLength {
    /// `t1 - t0 + tail`: the string is damped at the event's end.
    Held { tail: f64 },
    /// `min(max, base + per_hz / f0)`: the string rings regardless of `t1`
    /// (harp).
    Ring { base: f64, per_hz: f64, max: f64 },
    /// A fixed length (harmony-guitar arpeggio).
    Fixed { secs: f64 },
}

/// Plucked-string parameters.
///
/// Two groups of fields. `pluck_into` reads the first group as given. The
/// second group holds per-note laws that `note` applies once per event to
/// produce the resolved copy `pluck_into` renders: `amp` times velocity,
/// T60 by pitch, random pick position and detune, brightness, glide and pick
/// noise by velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluckParams {
    /// Output amplitude (peak of the main polarisation before the high-note trim).
    pub amp: f64,
    /// Fundamental T60, seconds (at `t60_ref_hz` when `t60_exp` is not 0).
    pub t60: f64,
    /// Pick position as a fraction of the string, clamped to [0.04, 0.5].
    pub pick: f64,
    /// Brightness 0..1: pick width, pick-release low-pass, attack time.
    pub bright: f64,
    /// Starting loss-filter pole (higher is darker and faster high decay).
    pub damp: f64,
    /// White noise added to the pick shape, relative to its peak.
    pub noise: f64,
    /// Polarisation detune, cents (second +detune, main -detune/4).
    pub detune: f64,
    /// Release damping length at the end of the note, seconds.
    pub release: f64,
    /// Loop T60 during the release, seconds.
    pub release_t60: f64,
    /// Tension glide depth, cents sharp at the onset.
    pub glide: f64,
    /// Pick-noise level (0 is none).
    pub attack_noise: f64,
    /// T60 law: `t60 * (t60_ref_hz / f0)^t60_exp`.
    pub t60_ref_hz: f64,
    pub t60_exp: f64,
    /// Pick position law: `pick + pick_spread * U[0, 1)`.
    pub pick_spread: f64,
    /// Detune law: `detune + detune_spread * U[0, 1)`.
    pub detune_spread: f64,
    /// Brightness law: `bright + bright_vel * vel`.
    pub bright_vel: f64,
    /// Rendered length per event.
    pub length: NoteLength,
}

impl Default for PluckParams {
    /// A neutral string: the defaults of the shipped model (pick 0.15,
    /// brightness 0.5, damp `0.42 - 0.36 bright`, noise 0.08, detune 1.4
    /// cents, release 30 ms at T60 90 ms), no glide, no pick noise, no
    /// per-note laws.
    fn default() -> Self {
        PluckParams {
            amp: 1.0,
            t60: 1.0,
            pick: 0.15,
            bright: 0.5,
            damp: 0.42 - 0.36 * 0.5,
            noise: 0.08,
            detune: 1.4,
            release: 0.03,
            release_t60: 0.09,
            glide: 0.0,
            attack_noise: 0.0,
            t60_ref_hz: 1.0,
            t60_exp: 0.0,
            pick_spread: 0.0,
            detune_spread: 0.0,
            bright_vel: 0.0,
            length: NoteLength::Held { tail: 0.0 },
        }
    }
}

impl PluckParams {
    /// Accompaniment guitar. Replaces the PluckOpts literal in
    /// arrange/src/guitar.rs: amp v, bright 0.6 + 0.25 v, damp GT.damp 0.18,
    /// glide GT.glide 5 * v, atk_noise GT.atk 1.0 * v, t60 7 (82 / f)^0.45,
    /// pick 0.11 + 0.07 U, noise 0.06, detune 1 + 0.8 U, rel 0.02, rel_t 0.08.
    /// Length comes from the string events (`guitar::render_strings`).
    pub const GUITAR: PluckParams = PluckParams {
        amp: 1.0,
        t60: 7.0,
        pick: 0.11,
        bright: 0.6,
        damp: TUNING.damping,
        noise: 0.06,
        detune: 1.0,
        release: 0.02,
        release_t60: 0.08,
        glide: TUNING.glide,
        attack_noise: TUNING.attack,
        t60_ref_hz: 82.0,
        t60_exp: 0.45,
        pick_spread: 0.07,
        detune_spread: 0.8,
        bright_vel: 0.25,
        length: NoteLength::Held { tail: 0.0 },
    };

    /// Bass pluck layer. Replaces the literal in arrange/src/bass.rs: amp v,
    /// bright 0.12, damp 0.5, t60 2.2, pick 0.2, noise 0.03, detune default
    /// 1.4, rel 0.06, rel_t 0.12; length `t1 - t0 + 0.05`. The sine sub layer
    /// is `guitar::render_bass`.
    pub const BASS: PluckParams = PluckParams {
        amp: 1.0,
        t60: 2.2,
        pick: 0.2,
        bright: 0.12,
        damp: 0.5,
        noise: 0.03,
        detune: 1.4,
        release: 0.06,
        release_t60: 0.12,
        glide: 0.0,
        attack_noise: 0.0,
        t60_ref_hz: 1.0,
        t60_exp: 0.0,
        pick_spread: 0.0,
        detune_spread: 0.0,
        bright_vel: 0.0,
        length: NoteLength::Held { tail: 0.05 },
    };

    /// Harp. Replaces the literal in arrange/src/harp.rs: amp v, bright 0.45,
    /// damp 0.16, t60 6 (98 / f)^0.5, pick 0.3 + 0.15 U, noise 0.02, detune
    /// 0.8 + 0.8 U, rel 0.3, rel_t 0.4; length `min(7, 3 + 400 / f)` s.
    pub const HARP: PluckParams = PluckParams {
        amp: 1.0,
        t60: 6.0,
        pick: 0.3,
        bright: 0.45,
        damp: 0.16,
        noise: 0.02,
        detune: 0.8,
        release: 0.3,
        release_t60: 0.4,
        glide: 0.0,
        attack_noise: 0.0,
        t60_ref_hz: 98.0,
        t60_exp: 0.5,
        pick_spread: 0.15,
        detune_spread: 0.8,
        bright_vel: 0.0,
        length: NoteLength::Ring { base: 3.0, per_hz: 400.0, max: 7.0 },
    };

    /// Harmony-guitar lead and fill notes. Replaces the literal in
    /// engine/src/band.rs `pluck_hg`: amp v, bright 0.7, damp 0.08,
    /// t60 5 (110 / f)^0.4, pick 0.12, noise 0.05, detune default 1.4,
    /// rel 0.08, rel_t 0.1; length `t1 - t0 + 0.4`.
    pub const HG_LEAD: PluckParams = PluckParams {
        amp: 1.0,
        t60: 5.0,
        pick: 0.12,
        bright: 0.7,
        damp: 0.08,
        noise: 0.05,
        detune: 1.4,
        release: 0.08,
        release_t60: 0.1,
        glide: 0.0,
        attack_noise: 0.0,
        t60_ref_hz: 110.0,
        t60_exp: 0.4,
        pick_spread: 0.0,
        detune_spread: 0.0,
        bright_vel: 0.0,
        length: NoteLength::Held { tail: 0.4 },
    };

    /// Harmony-guitar arpeggio. Replaces the literal in engine/src/band.rs:
    /// amp 0.32 (fixed: the planner gives velocity 1), bright 0.65, damp
    /// 0.08, t60 3, pick 0.2, noise 0.05, detune default 1.4, rel 0.15,
    /// rel_t 0.2; length 1.4 s.
    pub const HG_ARP: PluckParams = PluckParams {
        amp: 0.32,
        t60: 3.0,
        pick: 0.2,
        bright: 0.65,
        damp: 0.08,
        noise: 0.05,
        detune: 1.4,
        release: 0.15,
        release_t60: 0.2,
        glide: 0.0,
        attack_noise: 0.0,
        t60_ref_hz: 1.0,
        t60_exp: 0.0,
        pick_spread: 0.0,
        detune_spread: 0.0,
        bright_vel: 0.0,
        length: NoteLength::Fixed { secs: 1.4 },
    };

    /// Resolves the per-note laws for fundamental `f0` Hz and velocity
    /// `vel`: two uniform draws from `rng` (pick position, then detune).
    /// The result has neutral laws, so resolving it again changes only
    /// `amp`, `glide` and `attack_noise` (by `vel` again).
    pub fn note(&self, f0: f64, vel: f64, rng: &mut Rng) -> PluckParams {
        let pick = self.pick + self.pick_spread * rng.uniform();
        let detune = self.detune + self.detune_spread * rng.uniform();
        let t60 = if self.t60_exp == 0.0 { self.t60 } else { self.t60 * (self.t60_ref_hz / f0).powf(self.t60_exp) };
        PluckParams {
            amp: self.amp * vel,
            t60,
            pick,
            bright: self.bright + self.bright_vel * vel,
            detune,
            glide: self.glide * vel,
            attack_noise: self.attack_noise * vel,
            t60_exp: 0.0,
            pick_spread: 0.0,
            detune_spread: 0.0,
            bright_vel: 0.0,
            ..*self
        }
    }

    /// True when every field `pluck_into` reads is finite.
    pub fn is_finite(&self) -> bool {
        [self.amp, self.t60, self.pick, self.bright, self.damp, self.noise, self.detune, self.release, self.release_t60, self.glide, self.attack_noise]
            .iter()
            .all(|v| v.is_finite())
    }

    /// Rendered length in seconds of an event from `t0` to `t1` at `f0` Hz.
    pub fn length_secs(&self, f0: f64, t0: f64, t1: f64) -> f64 {
        match self.length {
            NoteLength::Held { tail } => t1 - t0 + tail,
            NoteLength::Ring { base, per_hz, max } => (base + per_hz / f0).min(max),
            NoteLength::Fixed { secs } => secs,
        }
    }
}

/// Buffers reused across notes: the excitation, a copy for the circular
/// filters, and the loop delay line (sized for `F0_MIN`).
pub struct PluckScratch {
    exc: Vec<f64>,
    tmp: Vec<f64>,
    line: DelayLine,
}

impl Default for PluckScratch {
    fn default() -> Self {
        Self::new()
    }
}

impl PluckScratch {
    pub fn new() -> Self {
        PluckScratch { exc: Vec::with_capacity(L_MAX + 1), tmp: Vec::with_capacity(L_MAX + 1), line: DelayLine::new(L_MAX + 2) }
    }
}

/// `|H(e^jw)|` of the loss filter `(1 - p) / (1 - p z^-1)`.
#[inline]
pub(crate) fn loss_mag(p: f64, w: f64) -> f64 {
    (1.0 - p) / (1.0 - 2.0 * p * w.cos() + p * p).sqrt()
}

/// One polarisation's loop: delay line, loss filter, gain, Thiran allpass.
struct Loop<'a> {
    line: &'a mut DelayLine,
    /// `read_int` tap for the oldest of the last L samples: L - 1.
    tap: usize,
    lp: OnePole,
    g: f64,
    ap: Thiran1,
}

impl Loop<'_> {
    /// One loop step: returns the velocity leaving the delay line and feeds
    /// it back through loss, gain and allpass.
    #[inline(always)]
    fn step(&mut self, tap: usize) -> f64 {
        let cur = self.line.read_int(tap) as f64;
        let v = self.g * self.lp.tick(cur);
        let y = self.ap.tick(v);
        self.line.push(y as f32);
        cur
    }

    /// Runs `out.len()` steps at the fixed tap and adds
    /// `cur * gain * e * h` to `out`, with `e` and `h` linear ramps (attack
    /// and fade). Returns the peak of `|cur|`.
    #[inline]
    fn run(&mut self, out: &mut [f32], gain: f64, mut e: f64, de: f64, mut h: f64, dh: f64) -> f64 {
        let tap = self.tap;
        let mut pk = 0.0f64;
        for o in out.iter_mut() {
            let cur = self.step(tap);
            *o += (cur * gain * e * h) as f32;
            pk = pk.max(cur.abs());
            e += de;
            h += dh;
        }
        pk
    }

    /// `run` with no ramps (sustain and release spans): adds `cur * gain`.
    #[inline]
    fn run_flat(&mut self, out: &mut [f32], gain: f64) -> f64 {
        let tap = self.tap;
        let mut pk = 0.0f64;
        for o in out.iter_mut() {
            let cur = self.step(tap);
            *o += (cur * gain) as f32;
            pk = pk.max(cur.abs());
        }
        pk
    }

    /// As `run`, with the loop delay `d` (line plus allpass, samples)
    /// ramped by `dd` per sample through the allpass alone. The line length
    /// is fixed and taken from the sharp starting period, so the allpass
    /// starts in [0.5, 1.5) and ends at the steady delta: a glide of `c`
    /// cents needs `N (1 - 2^(-c/1200))` samples for a period of `N`
    /// samples, all of it inside the allpass range. Moving the tap instead
    /// would repeat a sample at each step and inject DC into the loop.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    fn run_glide(&mut self, out: &mut [f32], gain: f64, mut e: f64, de: f64, mut h: f64, dh: f64, d: &mut f64, dd: f64) -> f64 {
        let tap = self.tap;
        let line = (tap + 1) as f64;
        let mut pk = 0.0f64;
        for o in out.iter_mut() {
            self.ap.set_delay(*d - line);
            let cur = self.step(tap);
            *o += (cur * gain * e * h) as f32;
            pk = pk.max(cur.abs());
            e += de;
            h += dh;
            *d += dd;
        }
        pk
    }
}

/// Adds one plucked note to `out` from sample `start` for `len` samples
/// (clipped to the buffer) at fundamental `f0` Hz, with resolved parameters
/// `p` (see `PluckParams::note`). Draws the excitation noise and pick noise
/// from `rng`. Does nothing for `f0` below 20 Hz, a non-finite `f0` or
/// parameter, or a start past the end.
pub fn pluck_into(out: &mut [f32], start: usize, f0: f64, len: usize, p: &PluckParams, rng: &mut Rng, scratch: &mut PluckScratch) {
    if start >= out.len() || !f0.is_finite() || f0 < F0_MIN || !p.is_finite() {
        return;
    }
    let len = len.min(out.len() - start);
    if len == 0 {
        return;
    }
    let out = &mut out[start..start + len];
    let bright = p.bright.clamp(0.0, 1.0);
    let beta = p.pick.clamp(0.04, 0.5);
    let damp = p.damp.clamp(0.0, 0.99);
    let t60 = p.t60.max(1e-3);
    let rel_n = (p.release.max(0.0) * SR_F).round() as usize;
    let rel0 = len.saturating_sub(rel_n);
    let fade0 = len.saturating_sub(FADE);
    let atk = ((SR_F * (0.0004 + 0.0016 * (1.0 - bright))).round() as usize).max(8);
    let n_atk = atk.min(len);

    for pol in 0..2 {
        let second = pol == 1;
        let cents = if second { p.detune } else { -0.25 * p.detune };
        let fp = f0 * (cents / 1200.0).exp2();
        let n_period = SR_F / fp;
        let w0 = TAU * fp / SR_F;
        let rho = 10f64.powf(-3.0 / (t60 * if second { POL2_T60 } else { 1.0 } * fp));

        // Loss filter: reduce the pole until |H(f0)| >= sqrt(rho).
        let lim = rho.sqrt();
        let mut pole = damp;
        let mut guard = 0;
        while pole > 0.002 && loss_mag(pole, w0) < lim && guard < 40 {
            pole *= 0.85;
            guard += 1;
        }
        let g = (rho / loss_mag(pole, w0)).min(G_MAX);
        let tau = one_pole_phase_delay(pole, w0);

        // Glide depth in cents, reduced so it spans at most
        // GLIDE_MAX_SAMPLES of loop delay.
        let gl_req = p.glide * if second { POL2_GLIDE } else { 1.0 };
        let glide_on = gl_req > GLIDE_FLOOR_CENTS && gl_req.is_finite();
        let gl = if glide_on {
            let room = (1.0 - GLIDE_MAX_SAMPLES / n_period).max(1e-9);
            gl_req.min(-1200.0 * room.log2())
        } else {
            0.0
        };
        // Line length from the sharp starting period (the steady period
        // without a glide): the allpass starts in [0.5, 1.5).
        let n_start = n_period * (-gl / 1200.0).exp2();
        let lf = (n_start - tau - 0.5).floor();
        if lf < 3.0 || lf > L_MAX as f64 {
            continue;
        }
        let l = lf as usize;
        let delta = n_period - tau - lf;

        let vpk = excite(scratch, l, beta, bright, p.noise, rng);
        let gain = p.amp * if second { POL2_AMP } else { 1.0 } / vpk * (l as f64 / 60.0).min(1.0);

        if !second && p.attack_noise > 0.0 {
            pick_noise(out, p.attack_noise * p.amp * 0.35, rng);
        }

        // Load one period. Loss filter and allpass start at zero: the line
        // holds a zero-mean period, so from zero states the loop output sums
        // to about zero over a full decay (a warm filter state would inject
        // a DC step that the loop then sustains at gain g). The warm-up pass
        // absorbs the start.
        let lp = OnePole { a: 1.0 - pole, z: 0.0 };
        let exc = &scratch.exc;
        let line = &mut scratch.line;
        for &x in &exc[..l] {
            line.push(x as f32);
        }
        let mut lpl = Loop { line, tap: l - 1, lp, g, ap: Thiran1::with_max(n_start - tau - lf, DELTA_MAX) };
        // Warm-up: one pass round the loop with no output.
        for _ in 0..l {
            lpl.step(l - 1);
        }

        // Glide setup: target loop delay every GLIDE_CTRL samples.
        let glide_on = gl > GLIDE_FLOOR_CENTS;
        let glide_decay = (-(GLIDE_CTRL as f64) / (GLIDE_TAU * SR_F)).exp();
        let glide_end = if glide_on {
            let n = GLIDE_TAU * SR_F * (gl / GLIDE_FLOOR_CENTS).ln();
            (((n / GLIDE_CTRL as f64).ceil() as usize) * GLIDE_CTRL).min(len)
        } else {
            0
        };
        let d_of = |cents: f64| SR_F / (fp * (cents / 1200.0).exp2()) - tau;
        let mut g_cents = gl;
        let mut d = d_of(g_cents);
        let mut dd = 0.0;

        // Span driver.
        let mut n = 0usize;
        let mut chk = 0usize;
        let mut pk = 0.0f64;
        let mut peak0 = -1.0f64;
        let mut released = false;
        while n < len {
            if !released && n >= rel0 {
                released = true;
                lpl.lp.a = 1.0 - (pole + 0.3).min(0.7);
                lpl.g = 10f64.powf(-3.0 / (p.release_t60.max(1e-3) * fp));
            }
            let gliding = n < glide_end;
            if gliding && n.is_multiple_of(GLIDE_CTRL) {
                g_cents *= glide_decay;
                dd = (d_of(g_cents) - d) / GLIDE_CTRL as f64;
            }
            let mut end = (n + (l - chk)).min(len);
            for b in [n_atk, rel0, fade0] {
                if b > n && b < end {
                    end = b;
                }
            }
            if gliding {
                let ctrl = (n / GLIDE_CTRL + 1) * GLIDE_CTRL;
                end = end.min(ctrl).min(glide_end);
            }
            let (e0, de) = if n < n_atk { (n as f64 / atk as f64, 1.0 / atk as f64) } else { (1.0, 0.0) };
            let (h0, dh) = if n >= fade0 { ((len - n) as f64 / FADE as f64, -1.0 / FADE as f64) } else { (1.0, 0.0) };
            let span = &mut out[n..end];
            let spk = if gliding {
                let r = lpl.run_glide(span, gain, e0, de, h0, dh, &mut d, dd);
                if end == glide_end {
                    lpl.ap.set_delay(delta);
                }
                r
            } else if n >= n_atk && n < fade0 {
                lpl.run_flat(span, gain)
            } else {
                lpl.run(span, gain, e0, de, h0, dh)
            };
            pk = pk.max(spk);
            chk += end - n;
            n = end;
            if chk == l {
                chk = 0;
                if peak0 < 0.0 {
                    peak0 = pk;
                } else if pk < peak0 * STOP_RATIO {
                    break;
                }
                pk = 0.0;
            }
        }
    }
}

/// Builds the excitation for loop length `l` in `scratch.exc` and returns
/// its peak (at least 1e-9).
fn excite(scratch: &mut PluckScratch, l: usize, beta: f64, bright: f64, noise: f64, rng: &mut Rng) -> f64 {
    let exc = &mut scratch.exc;
    let tmp = &mut scratch.tmp;
    exc.clear();
    exc.resize(l, 0.0);
    // Triangle with apex at the pick position, plus noise.
    let apex = ((beta * l as f64).round() as usize).clamp(1, l - 1);
    let up = 1.0 / apex as f64;
    let down = 1.0 / (l - apex) as f64;
    for (i, x) in exc.iter_mut().enumerate() {
        let tri = if i < apex { i as f64 * up } else { (l - i) as f64 * down };
        *x = tri + rng.bipolar() * noise;
    }
    // Circular moving average of width w (finger or pick width).
    let w = ((l as f64 * 0.008 * (1.0 + 2.0 * (1.0 - bright))).round() as usize).clamp(1, l);
    if w > 1 {
        tmp.clear();
        tmp.extend_from_slice(exc);
        let mut acc: f64 = (0..w).map(|k| tmp[(l - k) % l]).sum();
        let inv = 1.0 / w as f64;
        for (i, x) in exc.iter_mut().enumerate() {
            *x = acc * inv;
            let add = if i + 1 == l { 0 } else { i + 1 };
            let sub = (i + l + 1 - w) % l;
            acc += tmp[add] - tmp[sub];
        }
    }
    // Differentiate (displacement shape to velocity).
    tmp.clear();
    tmp.extend_from_slice(exc);
    let mut prev = tmp[l - 1];
    for (x, &s) in exc.iter_mut().zip(tmp.iter()) {
        *x = s - prev;
        prev = s;
    }
    // Circular one-pole low-pass (pick release): first pass settles the
    // state, second pass writes.
    let fc = 1500.0 + 14000.0 * bright * bright;
    let a = 1.0 - (-TAU * fc / SR_F).exp();
    let mut z = 0.0f64;
    for &x in exc.iter() {
        z += a * (x - z);
    }
    for x in exc.iter_mut() {
        z += a * (*x - z);
        *x = z;
    }
    // Remove DC, find the peak.
    let m = exc.iter().sum::<f64>() / l as f64;
    let mut vpk = 1e-9f64;
    for x in exc.iter_mut() {
        *x -= m;
        vpk = vpk.max(x.abs());
    }
    vpk
}

/// Pick noise: 4 ms of first-difference high-passed white noise at `level`,
/// decaying with a 1.2 ms time constant (one multiply per sample).
fn pick_noise(out: &mut [f32], level: f64, rng: &mut Rng) {
    let n = ((0.004 * SR_F).round() as usize).min(out.len());
    let decay = (-1.0 / (0.0012 * SR_F)).exp();
    let mut env = level;
    let mut prev = 0.0f64;
    for o in out[..n].iter_mut() {
        let w = rng.bipolar();
        *o += ((w - prev) * env) as f32;
        prev = w;
        env *= decay;
    }
}

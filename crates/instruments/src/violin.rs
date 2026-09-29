//! Bowed string: digital waveguide with the STK bow friction table (McIntyre, Schumacher, Woodhouse 1983; Smith 1986; Cook and Scavone).
//!
//! Three parts:
//!
//! - `BowedString`: the string. Two delay lines split at the bow point (nut
//!   side and bridge side), rigid nut (reflection -1), bridge reflection
//!   `-reflect_gain` times a one-pole low-pass (the loop loss), and the bow
//!   as a velocity-dependent reflection at the bow point (STK `BowTable`).
//! - `plan_strokes`: the stroke planner. Groups notes into legato phrases,
//!   splits long notes into several bows, places bow changes, slides and the
//!   bow direction, and draws each note's vibrato.
//! - `render_violin`: the player. Runs the bow controls at `CONTROL`-sample
//!   periods (bow position, force, speed, vibrato, pitch wander, four
//!   Ornstein-Uhlenbeck perturbations) and ramps every control value per
//!   sample across the period (`dsp::smoother::Ramp`).
//!
//! The body (measured, `instruments::body`) is applied by the engine, not here.
//!
//! Loop tuning. With `d_n` the nut-side delay, `d_b` the bridge-side delay
//! and `tau(w0)` the phase delay of the bridge low-pass at the note's
//! frequency (`dsp::delay::one_pole_phase_delay`), the round trip is
//! `d_n + d_b + tau(w0) = SR / f0`. Fractional delays are read by 4-point
//! Lagrange interpolation, whose gain is flat to within 0.1 dB below 4 kHz
//! for any fraction, so vibrato no longer modulates the loop loss.
//!
//! Loss re-tuning for the Lagrange reads (design 5.7). The linear reads of
//! the earlier model low-passed the loop twice per round trip by an amount
//! that depended on the fraction (|cos(w/2)| at fraction 0.5). Lagrange
//! removes that loss, so the same bridge filter gives a brighter loop and
//! more energy in the upper partials. `BRIDGE_POLE` and `BRIDGE_REFLECTION`
//! below are the values chosen against the earlier model's 1/3-octave
//! spectrum of a sustained A4 (tests/violin.rs) and the Helmholtz sweep
//! (examples/helmholtz.rs); see their doc comments.

use dsp::delay::{one_pole_phase_delay, DelayLine};
use dsp::onepole::OnePole;
use dsp::smoother::Ramp;
use dsp::stochastic::{OuProcess, RandomWalk};
use sfcore::math::{clamp, mtof, smoothstep};
use sfcore::random::{tag, Rng, Tag};
use sfcore::time::sample_at;
use sfcore::SR_F;
use song::events::BowNote;
use std::f64::consts::{LN_2, TAU};

/// Stream for the stroke planner (phrase grouping, bowing, vibrato draws).
const VIOLIN: Tag = tag("violin");
/// Per-phrase stream for the performance noise (OU processes, wander, bow
/// noise), indexed by phrase number. A phrase's sound does not depend on
/// how many draws any other phrase made.
const VIOLIN_BOW: Tag = tag("violin.bow");

/// Control period in samples (2756 Hz control rate).
pub const CONTROL: usize = 16;
const CONTROL_F: f64 = CONTROL as f64;
/// Control tick interval in seconds.
const CONTROL_DT: f64 = CONTROL_F / SR_F;

// ---------------------------------------------------------------- string

/// Pole of the bridge reflection low-pass `H(z) = (1 - p) / (1 - p z^-1)`.
/// The earlier model used 0.2 with linear delay reads. With Lagrange reads
/// and 0.2 the sustained A4 spectrum rose +1.1 dB at 3.2 kHz and +2.8 dB at
/// 8 kHz against the earlier model; 0.25 gave +1.9 dB and 0.3 +0.9 dB at
/// 8 kHz; 0.35 matches it within 0.9 dB in every band 200 Hz - 8 kHz and
/// holds 212/216 in the Helmholtz sweep (0.2: 213, 0.25: 210, 0.3: 209).
/// The pole's larger phase delay is part of the loop tuning.
pub const BRIDGE_POLE: f64 = 0.35;
/// Bridge reflection magnitude at DC (the sign is negative). Sets the
/// round-trip loss at low frequencies and so the ring-down after the bow
/// lifts. Unchanged from the earlier model: the extra brightness of the
/// Lagrange reads is at high frequencies, where the pole acts.
pub const BRIDGE_REFLECTION: f64 = 0.985;
/// Bridge reflection drops by this much after the phrase ends (the player
/// damps the string), over `DAMP_START`..`DAMP_END` seconds after release.
const RELEASE_DAMPING: f64 = 0.06;
const DAMP_START: f64 = 0.25;
const DAMP_END: f64 = 0.45;

/// Lowest note the delay lines hold, Hz. Lower notes are raised to it.
pub const MIN_F0: f64 = 150.0;
/// Highest note, Hz. Keeps both delay lines at 2 samples or more.
pub const MAX_F0: f64 = 3000.0;
/// Shortest one-way delay, samples (the Lagrange read needs d - 1 >= 1).
const MIN_DELAY: f64 = 2.0;
/// Longest one-way delay the lines are sized for, samples.
const MAX_DELAY: usize = (SR_F / MIN_F0) as usize + 8;

/// STK `BowTable` (Cook and Scavone, the Synthesis ToolKit): reflection
/// coefficient of the bow for relative velocity times slope `x`,
/// `(|x + 0.001| + 0.75)^-4`, clipped to 1. Near zero relative velocity the
/// hair sticks (coefficient 1); far from it the string slips.
#[inline(always)]
pub fn bow_table(x: f64) -> f64 {
    let q = (x + 0.001).abs() + 0.75;
    let q2 = q * q;
    (1.0 / (q2 * q2)).min(1.0)
}

/// Friction slope for bow pressure `p`: `5 - 4 p` (STK `Bowed`). Higher
/// pressure gives a flatter table and a longer stick phase.
#[inline]
pub fn slope_for_pressure(p: f64) -> f64 {
    5.0 - 4.0 * p
}

/// Bow state for one sample of `BowedString::tick`.
#[derive(Clone, Copy, Debug, Default)]
pub struct BowControl {
    /// Bow velocity (string velocity units; sign is the bow direction).
    pub velocity: f64,
    /// Friction table slope (`slope_for_pressure`).
    pub slope: f64,
    /// One-way delay from the bow to the bridge, samples.
    pub bow_delay: f64,
    /// One-way delay from the bow to the nut, samples.
    pub nut_delay: f64,
    /// Bow contact, 1 on the string, 0 lifted.
    pub lift: f64,
}

/// Bowed string waveguide (McIntyre, Schumacher, Woodhouse 1983; Smith 1986).
///
/// Velocity waves travel in two `f32` delay lines: `nut` carries the wave
/// leaving the bow towards the nut (`nut_delay` samples), `bridge` the wave
/// leaving the bow towards the bridge (`bow_delay` samples). The return
/// trips are folded into the reflections, so each line's delay is the
/// round trip on its side of the bow. Each sample:
///
/// ```text
/// b_in = bridge[n - d_b]          wave arriving at the bridge
/// n_in = nut[n - d_n]             wave arriving at the nut
/// b_r  = -g * lowpass(b_in)       bridge reflection, back towards the bow
/// n_r  = -n_in                    nut reflection
/// dv   = v_bow - (b_r + n_r)      bow velocity relative to the string
/// e    = dv * bow_table(dv * slope) * lift
/// nut.push(b_r + e); bridge.push(n_r + e)
/// ```
///
/// The output taps `b_in`, the velocity wave incident on the bridge: the
/// force that drives the body is proportional to it, so it is the signal a
/// bridge pickup (and the body convolution after it) sees.
#[derive(Clone, Debug)]
pub struct BowedString {
    nut: DelayLine,
    bridge: DelayLine,
    reflect: OnePole,
    reflect_gain: f64,
}

impl Default for BowedString {
    fn default() -> Self {
        BowedString::new()
    }
}

impl BowedString {
    /// A string at rest with lines sized for `MIN_F0`, bridge pole
    /// `BRIDGE_POLE` and reflection `BRIDGE_REFLECTION`.
    pub fn new() -> Self {
        BowedString {
            nut: DelayLine::new(MAX_DELAY),
            bridge: DelayLine::new(MAX_DELAY),
            reflect: OnePole {
                a: 1.0 - BRIDGE_POLE,
                z: 0.0,
            },
            reflect_gain: BRIDGE_REFLECTION,
        }
    }

    /// Bridge reflection magnitude at DC.
    #[inline]
    pub fn set_reflect_gain(&mut self, g: f64) {
        self.reflect_gain = g;
    }

    /// Phase delay of the bridge low-pass at `w` rad/sample (for tuning).
    pub fn bridge_phase_delay(w: f64) -> f64 {
        one_pole_phase_delay(BRIDGE_POLE, w)
    }

    /// Longest one-way delay a read may use, samples.
    pub fn max_delay() -> f64 {
        (MAX_DELAY - 1) as f64
    }

    /// Advance one sample; returns the wave arriving at the bridge.
    /// Delays are clamped to [`MIN_DELAY`, `max_delay()`]; a NaN delay
    /// reads at `MIN_DELAY`.
    #[inline]
    pub fn tick(&mut self, bow: &BowControl) -> f32 {
        let hi = Self::max_delay();
        // A sample pushed at step n is read back at step n + d. Reads come
        // before this step's push, so the line's delay-(d - 1) tap is used.
        let d_b = clamp(bow.bow_delay, MIN_DELAY, hi) - 1.0;
        let d_n = clamp(bow.nut_delay, MIN_DELAY, hi) - 1.0;
        let b_in = self.bridge.read_lagrange3(d_b);
        let n_in = self.nut.read_lagrange3(d_n);
        let b_r = -self.reflect_gain * self.reflect.tick(b_in);
        let n_r = -n_in;
        let dv = bow.velocity - (b_r + n_r);
        let e = dv * bow_table(dv * bow.slope) * bow.lift;
        self.nut.push((b_r + e) as f32);
        self.bridge.push((n_r + e) as f32);
        b_in as f32
    }

    /// Silence the string.
    pub fn reset(&mut self) {
        self.nut.clear();
        self.bridge.clear();
        self.reflect.reset();
        self.reflect_gain = BRIDGE_REFLECTION;
    }
}

// ---------------------------------------------------------------- planner

/// Notes longer than this (seconds) are split into several bow strokes.
const MAX_STROKE: f64 = 2.4;
/// A split stroke lasts `SPLIT_STROKE_MIN + SPLIT_STROKE_SPREAD * u` seconds.
const SPLIT_STROKE_MIN: f64 = 1.6;
const SPLIT_STROKE_SPREAD: f64 = 0.6;
/// A gap of this many seconds or more between notes ends the phrase (the
/// bow lifts and the string rings down).
const PHRASE_GAP: f64 = 0.06;
/// Notes longer than this (seconds) always take a new bow.
const LONG_NOTE_REBOW: f64 = 0.85;
/// Probability that a shorter note inside a phrase takes a new bow (the
/// rest are slurred).
const REBOW_PROB: f64 = 0.4;
/// Intervals of this many semitones or more may be taken as an audible
/// finger slide, with probability `SLIDE_PROB`.
const SLIDE_INTERVAL: f64 = 5.0;
const SLIDE_PROB: f64 = 0.5;
/// Vibrato rate, Hz: `VIBRATO_RATE_MIN + VIBRATO_RATE_SPREAD * u`.
const VIBRATO_RATE_MIN: f64 = 5.4;
const VIBRATO_RATE_SPREAD: f64 = 1.0;
/// Vibrato depth, semitones peak: `VIBRATO_DEPTH_MIN + VIBRATO_DEPTH_SPREAD * u`.
const VIBRATO_DEPTH_MIN: f64 = 0.18;
const VIBRATO_DEPTH_SPREAD: f64 = 0.12;
/// Vibrato onset delay after the note starts, seconds.
const VIBRATO_DELAY_MIN: f64 = 0.15;
const VIBRATO_DELAY_SPREAD: f64 = 0.2;
/// Bow-to-bridge distance as a fraction of the string length:
/// `BOW_BETA_MIN + BOW_BETA_SPREAD * u`, drawn per phrase.
const BOW_BETA_MIN: f64 = 0.13;
const BOW_BETA_SPREAD: f64 = 0.02;

/// One bow stroke (a note, or part of a split note) with its bowing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub t0: f64,
    pub t1: f64,
    /// Pitch, fractional MIDI.
    pub midi: f64,
    /// Bow velocity scale, 0..=1.
    pub vel: f64,
    /// Vibrato rate, Hz.
    pub vib_rate: f64,
    /// Vibrato depth, semitones peak (0: none).
    pub vib_depth: f64,
    /// Vibrato onset delay, seconds (0 on the continuation of a split note).
    pub vib_delay: f64,
    /// A new bow starts here (bow change with a re-bow accent).
    pub rebow: bool,
    /// The pitch slides into this note (70 ms glide instead of 10 ms).
    pub slide: bool,
    /// Bow direction, +1 down-bow, -1 up-bow.
    pub dir: f64,
}

/// Strokes played without lifting the bow, and the per-phrase bow position.
#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    pub strokes: Vec<Stroke>,
    /// Bow-to-bridge distance as a fraction of the vibrating length.
    pub beta: f64,
    /// Initial vibrato phase, radians.
    pub vib_phase: f64,
}

/// Plan the bowing of `notes`: sort by onset, drop non-finite and empty
/// notes, split notes longer than `MAX_STROKE` into strokes of about 1.6-2.2
/// s (each continuation re-bows with no vibrato delay), group strokes
/// separated by less than `PHRASE_GAP` into phrases, then per stroke draw
/// vibrato rate, depth and delay, decide re-bow (`LONG_NOTE_REBOW`,
/// `REBOW_PROB`) and slide (`SLIDE_INTERVAL`, `SLIDE_PROB`), and alternate
/// the bow direction at each re-bow from a random first direction.
pub fn plan_strokes(notes: &[BowNote], rng: &mut Rng) -> Vec<Phrase> {
    let mut src: Vec<&BowNote> = notes
        .iter()
        .filter(|n| {
            n.t0.is_finite()
                && n.t1.is_finite()
                && n.midi.is_finite()
                && n.vel.is_finite()
                && n.t1 > n.t0
        })
        .collect();
    src.sort_by(|a, b| a.t0.total_cmp(&b.t0));

    // Split long notes. `split` marks a continuation stroke.
    let mut work: Vec<(Stroke, bool)> = Vec::with_capacity(src.len());
    for n in src {
        let base = Stroke {
            t0: n.t0,
            t1: n.t1,
            midi: n.midi as f64,
            vel: clamp(n.vel as f64, 0.0, 1.0),
            vib_rate: 0.0,
            vib_depth: if n.vibrato { 1.0 } else { 0.0 },
            vib_delay: 0.0,
            rebow: false,
            slide: false,
            dir: 1.0,
        };
        let d = n.t1 - n.t0;
        if d <= MAX_STROKE {
            work.push((base, false));
            continue;
        }
        let k = (d / (SPLIT_STROKE_MIN + SPLIT_STROKE_SPREAD * rng.uniform()))
            .ceil()
            .max(1.0) as usize;
        for j in 0..k {
            let s = Stroke {
                t0: n.t0 + d * j as f64 / k as f64,
                t1: n.t0 + d * (j + 1) as f64 / k as f64,
                ..base
            };
            work.push((s, j > 0));
        }
    }

    // Group into phrases.
    let mut groups: Vec<Vec<(Stroke, bool)>> = Vec::new();
    for s in work {
        match groups.last_mut() {
            Some(g) if g.last().is_some_and(|p| s.0.t0 - p.0.t1 < PHRASE_GAP) => g.push(s),
            _ => groups.push(vec![s]),
        }
    }

    // Bowing and vibrato.
    groups
        .into_iter()
        .map(|g| {
            let mut strokes: Vec<Stroke> = Vec::with_capacity(g.len());
            for (k, (mut s, split)) in g.into_iter().enumerate() {
                s.vib_rate = VIBRATO_RATE_MIN + VIBRATO_RATE_SPREAD * rng.uniform();
                s.vib_depth *= VIBRATO_DEPTH_MIN + VIBRATO_DEPTH_SPREAD * rng.uniform();
                s.vib_delay = if split {
                    0.0
                } else {
                    VIBRATO_DELAY_MIN + VIBRATO_DELAY_SPREAD * rng.uniform()
                };
                s.rebow =
                    k == 0 || split || s.t1 - s.t0 > LONG_NOTE_REBOW || rng.uniform() < REBOW_PROB;
                s.slide = match strokes.last() {
                    Some(p) => {
                        (s.midi - p.midi).abs() >= SLIDE_INTERVAL && rng.uniform() < SLIDE_PROB
                    }
                    None => false,
                };
                strokes.push(s);
            }
            let mut dir = if rng.uniform() < 0.5 { 1.0 } else { -1.0 };
            for (k, s) in strokes.iter_mut().enumerate() {
                if k > 0 && s.rebow {
                    dir = -dir;
                }
                s.dir = dir;
            }
            let beta = BOW_BETA_MIN + BOW_BETA_SPREAD * rng.uniform();
            let vib_phase = TAU * rng.uniform();
            Phrase {
                strokes,
                beta,
                vib_phase,
            }
        })
        .collect()
}

// ---------------------------------------------------------------- player

/// Onset: the bow starts this many seconds before the first note.
const PRE_ROLL: f64 = 0.03;
/// Render this long after the phrase ends (release and ring-down), seconds.
const RING: f64 = 1.2;
/// Output gain on the bridge wave.
const OUTPUT_GAIN: f64 = 1.6;

/// Pitch glide time constant between notes, seconds, and on slides.
const GLIDE_TAU: f64 = 0.01;
const SLIDE_TAU: f64 = 0.07;
/// Bow position (bow-to-bridge delay) glide time constant, seconds.
const BOW_POSITION_TAU: f64 = 0.06;
/// Bow direction change time constant, seconds.
const BOW_DIRECTION_TAU: f64 = 0.012;
/// Bow lift after the last note, seconds (smoothstep).
const LIFT_TIME: f64 = 0.05;

/// Vibrato: fade-in length after the onset delay, seconds; depth grows
/// from `VIBRATO_START` to `VIBRATO_START + VIBRATO_GROWTH` of the drawn
/// depth over the note.
const VIBRATO_FADE: f64 = 0.3;
const VIBRATO_START: f64 = 0.7;
const VIBRATO_GROWTH: f64 = 0.4;
/// Relative vibrato rate modulation by its OU process.
const VIBRATO_RATE_JITTER: f64 = 0.07;

/// Bow speed: `SPEED_BASE + SPEED_PER_VEL * vel * swell`, times the attack
/// ramp and the speed OU process.
const SPEED_BASE: f64 = 0.1;
const SPEED_PER_VEL: f64 = 0.2;
/// Swell on notes longer than `SWELL_MIN_DUR`: `SWELL_BASE + SWELL_DEPTH *
/// sin(pi x)`; shorter notes use `FLAT_SWELL`.
const SWELL_MIN_DUR: f64 = 0.8;
const SWELL_BASE: f64 = 0.82;
const SWELL_DEPTH: f64 = 0.24;
const FLAT_SWELL: f64 = 0.96;
/// Bow speed attack, seconds: long first notes start slower.
const ATTACK_SHORT: f64 = 0.06;
const ATTACK_LONG: f64 = 0.16;

/// Bow force: `PRESSURE_BASE` plus up to `PRESSURE_HIGH` for notes from
/// MIDI `PRESSURE_HIGH_FROM` over `PRESSURE_HIGH_SPAN` semitones, plus the
/// onset accent `ONSET_ACCENT * exp(-t / ONSET_TAU)` and the re-bow accent
/// `REBOW_ACCENT * exp(-t / REBOW_TAU)`, plus the pressure OU process.
const PRESSURE_BASE: f64 = 0.6;
const PRESSURE_HIGH: f64 = 0.15;
const PRESSURE_HIGH_FROM: f64 = 72.0;
const PRESSURE_HIGH_SPAN: f64 = 10.0;
const ONSET_ACCENT: f64 = 0.22;
const ONSET_TAU: f64 = 0.05;
const REBOW_ACCENT: f64 = 0.15;
const REBOW_TAU: f64 = 0.04;
/// Friction slope limits.
const SLOPE_MIN: f64 = 1.2;
const SLOPE_MAX: f64 = 4.6;

/// Ornstein-Uhlenbeck perturbations (unit variance): time constants in
/// seconds and the relative depth each applies.
const SPEED_OU_TAU: f64 = 0.18;
const SPEED_JITTER: f64 = 0.1;
const PRESSURE_OU_TAU: f64 = 0.12;
const PRESSURE_JITTER: f64 = 0.12;
const POSITION_OU_TAU: f64 = 0.35;
const POSITION_JITTER: f64 = 0.1;
const VIBRATO_RATE_OU_TAU: f64 = 0.25;

/// Pitch wander: `RandomWalk::bounded` at the control rate, in natural-log
/// frequency (0.0015 is 2.6 cents).
const WANDER_STEP: f64 = 0.0004;
const WANDER_LEAK: f64 = 0.97;
const WANDER_GAIN: f64 = 0.05;
const WANDER_LIMIT: f64 = 0.0015;

/// Bow hair noise: a uniform source smoothed by a one-pole (coefficient
/// `HAIR_SMOOTH`) modulates the bow velocity by `HAIR_ROUGHNESS`; its
/// high-passed part (second one-pole, `HAIR_HP_SMOOTH`) is added to the
/// output at `HAIR_NOISE_LEVEL` times the bow speed.
const HAIR_SMOOTH: f64 = 0.3;
const HAIR_HP_SMOOTH: f64 = 0.25;
const HAIR_ROUGHNESS: f64 = 0.5;
const HAIR_NOISE_LEVEL: f64 = 1.5;

/// Coefficient of a one-pole smoother run at the control rate with time
/// constant `tau` seconds.
fn control_coeff(tau: f64) -> f64 {
    1.0 - (-CONTROL_DT / tau).exp()
}

/// Per-sample ramps of the values the string reads.
struct Ramps {
    velocity: Ramp,
    slope: Ramp,
    bow_delay: Ramp,
    nut_delay: Ramp,
    lift: Ramp,
    reflect: Ramp,
    /// Bow speed times lift: the level of the hair noise at the output.
    noise_level: Ramp,
}

/// Control targets for one period.
struct Targets {
    velocity: f64,
    slope: f64,
    bow_delay: f64,
    nut_delay: f64,
    lift: f64,
    reflect: f64,
    noise_level: f64,
}

impl Ramps {
    fn set(&mut self, t: &Targets, n: u32) {
        self.velocity.set_target(t.velocity, n);
        self.slope.set_target(t.slope, n);
        self.bow_delay.set_target(t.bow_delay, n);
        self.nut_delay.set_target(t.nut_delay, n);
        self.lift.set_target(t.lift, n);
        self.reflect.set_target(t.reflect, n);
        self.noise_level.set_target(t.noise_level, n);
    }
}

/// The player's slowly varying state across one phrase.
struct Bowing<'a> {
    ph: &'a Phrase,
    /// Phrase start (bow onset) and end, seconds.
    t_start: f64,
    t_end: f64,
    /// Current stroke.
    k: usize,
    /// Fingered pitch before vibrato, MIDI (glides between notes).
    pitch: f64,
    vib_phase: f64,
    /// Bow-to-bridge delay, samples (glides with the note).
    bow_delay: f64,
    dir: f64,
    speed_ou: OuProcess,
    pressure_ou: OuProcess,
    position_ou: OuProcess,
    rate_ou: OuProcess,
    wander: RandomWalk,
}

impl<'a> Bowing<'a> {
    fn new(ph: &'a Phrase, first: &Stroke) -> Self {
        let pitch = first.midi;
        Bowing {
            ph,
            t_start: first.t0 - PRE_ROLL,
            t_end: ph.strokes.last().map_or(first.t1, |s| s.t1),
            k: 0,
            pitch,
            vib_phase: ph.vib_phase,
            bow_delay: SR_F / note_hz(pitch) * ph.beta,
            dir: first.dir,
            speed_ou: OuProcess::unit(SPEED_OU_TAU, CONTROL_DT),
            pressure_ou: OuProcess::unit(PRESSURE_OU_TAU, CONTROL_DT),
            position_ou: OuProcess::unit(POSITION_OU_TAU, CONTROL_DT),
            rate_ou: OuProcess::unit(VIBRATO_RATE_OU_TAU, CONTROL_DT),
            wander: RandomWalk::bounded(WANDER_STEP, WANDER_LEAK, WANDER_GAIN, WANDER_LIMIT),
        }
    }

    /// Advance the controls to time `t` and return this period's targets.
    fn control(&mut self, t: f64, rng: &mut Rng) -> Targets {
        let strokes = &self.ph.strokes;
        while self.k + 1 < strokes.len() && t >= strokes[self.k + 1].t0 {
            self.k += 1;
        }
        let n = strokes[self.k];
        let since = t - n.t0;
        let from_start = t - self.t_start;
        let after_end = t - self.t_end;

        // Fingered pitch glides in log frequency.
        let glide = if n.slide { SLIDE_TAU } else { GLIDE_TAU };
        self.pitch += (n.midi - self.pitch) * control_coeff(glide);

        // Vibrato: delayed, faded in, growing over the note, off after release.
        let x_note = clamp(since / (n.t1 - n.t0).max(0.2), 0.0, 1.0);
        let depth = if after_end > 0.0 {
            0.0
        } else {
            n.vib_depth
                * smoothstep(n.vib_delay, n.vib_delay + VIBRATO_FADE, since)
                * (VIBRATO_START + VIBRATO_GROWTH * x_note)
        };
        let rate_n = self.rate_ou.step(rng);
        self.vib_phase += TAU * n.vib_rate * (1.0 + VIBRATO_RATE_JITTER * rate_n) * CONTROL_DT;
        if self.vib_phase > TAU {
            self.vib_phase -= TAU;
        }
        let wander = self.wander.step(rng);
        let f_played =
            note_hz(self.pitch) * (depth * self.vib_phase.sin() * LN_2 / 12.0 + wander).exp();
        let f_played = clamp(f_played, MIN_F0, MAX_F0);
        let f_finger = note_hz(self.pitch);

        // Bow position follows the fingered length.
        let speed_n = self.speed_ou.step(rng);
        let pressure_n = self.pressure_ou.step(rng);
        let position_n = self.position_ou.step(rng);
        let bow_target = SR_F / f_finger * self.ph.beta * (1.0 + POSITION_JITTER * position_n);
        self.bow_delay += (bow_target - self.bow_delay) * control_coeff(BOW_POSITION_TAU);
        let bow_delay = self.bow_delay.max(MIN_DELAY);
        let w0 = TAU * f_played / SR_F;
        let loop_delay = SR_F / f_played - BowedString::bridge_phase_delay(w0);
        let nut_delay = (loop_delay - bow_delay).max(MIN_DELAY);

        // Bow speed: attack, swell, jitter.
        let first_dur = strokes[0].t1 - strokes[0].t0;
        let attack = smoothstep(
            0.0,
            if first_dur > 1.0 {
                ATTACK_LONG
            } else {
                ATTACK_SHORT
            },
            from_start,
        );
        let dur = (n.t1 - n.t0).max(0.25);
        let x = clamp(since / dur, 0.0, 1.0);
        let swell = if dur > SWELL_MIN_DUR {
            SWELL_BASE + SWELL_DEPTH * (std::f64::consts::PI * (x * 1.05).min(1.0)).sin()
        } else {
            FLAT_SWELL
        };
        let speed =
            (SPEED_BASE + SPEED_PER_VEL * n.vel * swell) * attack * (1.0 + SPEED_JITTER * speed_n);
        self.dir += (n.dir - self.dir) * control_coeff(BOW_DIRECTION_TAU);

        // Bow force.
        let midi_finger = 69.0 + 12.0 * (f_finger / 440.0).log2();
        let high = PRESSURE_HIGH
            * clamp(
                (midi_finger - PRESSURE_HIGH_FROM) / PRESSURE_HIGH_SPAN,
                0.0,
                1.0,
            );
        let rebow = if self.k > 0 && n.rebow {
            REBOW_ACCENT * (-since / REBOW_TAU).exp()
        } else {
            0.0
        };
        let pressure = PRESSURE_BASE
            + high
            + ONSET_ACCENT * (-from_start / ONSET_TAU).exp()
            + rebow
            + PRESSURE_JITTER * pressure_n;
        let slope = clamp(slope_for_pressure(pressure), SLOPE_MIN, SLOPE_MAX);

        let lift = if after_end > 0.0 {
            1.0 - smoothstep(0.0, LIFT_TIME, after_end)
        } else {
            1.0
        };
        let reflect =
            BRIDGE_REFLECTION - RELEASE_DAMPING * smoothstep(DAMP_START, DAMP_END, after_end);

        Targets {
            velocity: speed * self.dir,
            slope,
            bow_delay,
            nut_delay,
            lift,
            reflect,
            noise_level: speed * lift,
        }
    }
}

/// Frequency of fractional MIDI `m`, Hz.
#[inline]
fn note_hz(m: f64) -> f64 {
    mtof(m)
}

/// Render one phrase into `out` (added). `string` is reset first.
fn render_phrase(ph: &Phrase, string: &mut BowedString, rng: &mut Rng, out: &mut [f32]) {
    let Some(first) = ph.strokes.first() else {
        return;
    };
    string.reset();
    let mut bowing = Bowing::new(ph, first);
    let s0 = sample_at(bowing.t_start);
    let s1 = sample_at(bowing.t_end + RING).min(out.len() as isize);
    if s1 <= s0.max(0) {
        return;
    }
    let mut ramps = Ramps {
        velocity: Ramp::new(0.0),
        slope: Ramp::new(0.0),
        bow_delay: Ramp::new(0.0),
        nut_delay: Ramp::new(0.0),
        lift: Ramp::new(0.0),
        reflect: Ramp::new(0.0),
        noise_level: Ramp::new(0.0),
    };
    let mut noise = [0.0f32; CONTROL];
    let (mut hair, mut hair_lp) = (0.0f64, 0.0f64);
    let mut n = s0;
    let mut first_tick = true;
    while n < s1 {
        let t = n as f64 / SR_F;
        let targets = bowing.control(t, rng);
        ramps.set(&targets, if first_tick { 0 } else { CONTROL as u32 });
        first_tick = false;
        rng.fill_bipolar(&mut noise);
        let end = (n + CONTROL as isize).min(s1);
        for (j, idx) in (n..end).enumerate() {
            let reflect = ramps.reflect.next();
            string.set_reflect_gain(reflect);
            hair += HAIR_SMOOTH * (noise[j] as f64 - hair);
            let bow = BowControl {
                velocity: ramps.velocity.next() * (1.0 + HAIR_ROUGHNESS * hair),
                slope: ramps.slope.next(),
                bow_delay: ramps.bow_delay.next(),
                nut_delay: ramps.nut_delay.next(),
                lift: ramps.lift.next(),
            };
            let y = string.tick(&bow) as f64;
            hair_lp += HAIR_HP_SMOOTH * (hair - hair_lp);
            let level = ramps.noise_level.next();
            if idx >= 0 {
                let o = &mut out[idx as usize];
                *o += (y * OUTPUT_GAIN + (hair - hair_lp) * level * HAIR_NOISE_LEVEL) as f32;
            }
        }
        n = end;
    }
}

/// Render bowed `notes` into a mono buffer of `len` samples. The stroke
/// plan draws from `Rng::stream(seed, tag("violin"))`; each phrase's
/// performance noise from `Rng::event(seed, tag("violin.bow"), phrase)`.
pub fn render_violin(notes: &[BowNote], len: usize, seed: u64) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    let mut rng = Rng::stream(seed, VIOLIN);
    let phrases = plan_strokes(notes, &mut rng);
    let mut string = BowedString::new();
    for (k, ph) in phrases.iter().enumerate() {
        let mut bow_rng = Rng::event(seed, VIOLIN_BOW, k as u64);
        render_phrase(ph, &mut string, &mut bow_rng, &mut out);
    }
    out
}

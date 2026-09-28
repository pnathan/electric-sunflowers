//! Guitar: six monophonic strings plus six open-string sympathetic
//! Karplus-Strong loops (E2 A2 D3 G3 B3 E4) excited by the guitar output;
//! the note renderers for the free plucked parts (bass, harp, harmony
//! guitar) and the bass sine sub layer (design sections 5.5, 5.9).
//!
//! Strings. Each of the six strings is monophonic. A note sounds from its
//! onset until its `stop` time (the planner folds chord-change damping into
//! it) or until the next note on the same string plus 4 ms, whichever is
//! first; a note shorter than one sample is skipped. The pluck model is
//! `pluck::pluck_into` with `PluckParams::GUITAR`; each note draws from its
//! own stream `Rng::event(seed, GUITAR_NOTE, string << 32 | index)`, so a
//! note does not depend on any other note.
//!
//! Sympathetic strings. Six open-string loops (Karplus and Strong 1983)
//! driven by `SYMP_EXCITE` = 1.2% of the guitar output times the
//! `sympathetic` level. Loop: integer ring buffer, one-pole low-pass with
//! pole 0.5, first-order Thiran allpass for the fractional delay (the one-pole
//! phase delay at f0 is subtracted), gain for a fundamental T60 of 3.5 s.
//! The loops' outputs are added to the guitar in one pass over the buffer;
//! blocks where the input is silent and every loop is below -120 dB are
//! skipped (the loops are cleared once on entering such a gap).
//!
//! Bass sub layer. A sine at the note's fundamental, level 0.55 v, 6 ms
//! linear attack, 0.7 s exponential decay, 2600-sample linear release ramp
//! at the end of the note. The sine is the two-term recursion
//! `s[n+1] = 2 cos(w) s[n] - s[n-1]` and the decay one multiply per sample.

use dsp::delay::{one_pole_phase_delay, Thiran1};
use dsp::onepole::OnePole;
use sfcore::math::mtof;
use sfcore::random::{tag, Rng, Tag};
use sfcore::time::sample_at;
use sfcore::SR_F;
use song::events::{PluckNote, StringNote};
use std::f64::consts::TAU;

use crate::pluck::{pluck_into, PluckParams, PluckScratch};

/// Guitar settings that shape the accompaniment guitar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuitarTuning {
    /// Loss-filter pole of the strings.
    pub damping: f64,
    /// Pick-noise level at velocity 1.
    pub attack: f64,
    /// Tension glide at velocity 1, cents.
    pub glide: f64,
    /// Sympathetic-string level (multiplies the 1.2% excitation).
    pub sympathetic: f64,
}

/// Shipped guitar settings (the GT values of the original engine).
pub const TUNING: GuitarTuning = GuitarTuning { damping: 0.18, attack: 1.0, glide: 5.0, sympathetic: 1.0 };

/// Open strings, MIDI: E2 A2 D3 G3 B3 E4.
pub const OPEN_STRINGS: [u8; 6] = [40, 45, 50, 55, 59, 64];

/// Stream tags.
pub const GUITAR_NOTE: Tag = tag("guitar.note");
pub const BASS_NOTE: Tag = tag("bass.note");

/// A new note on a string ends the previous one this long after its onset.
const RESTRIKE_S: f64 = 0.004;

/// Sympathetic excitation: fraction of the guitar output fed to each loop.
const SYMP_EXCITE: f64 = 0.012;
/// Sympathetic loop T60 at the fundamental, seconds.
const SYMP_T60: f64 = 3.5;
/// Sympathetic loss-filter pole.
const SYMP_POLE: f64 = 0.5;
/// Sympathetic block size for the silence skip.
const SYMP_BLOCK: usize = 256;
/// Input below this is silent (-140 dB).
const SYMP_IN_FLOOR: f32 = 1e-7;
/// A loop whose block peak is below this is asleep (-120 dB).
const SYMP_LOOP_FLOOR: f64 = 1e-6;

/// One open-string loop.
struct SympString {
    buf: Vec<f32>,
    q: usize,
    lp: OnePole,
    ap: Thiran1,
    g: f64,
}

impl SympString {
    fn new(midi: u8) -> Self {
        let f0 = mtof(midi as f64);
        let w0 = TAU * f0 / SR_F;
        let n = SR_F / f0;
        let tau = one_pole_phase_delay(SYMP_POLE, w0);
        let d = (n - tau - 0.5).floor().max(2.0);
        let delta = n - tau - d;
        let rho = 10f64.powf(-3.0 / (SYMP_T60 * f0));
        let mag = crate::pluck::loss_mag(SYMP_POLE, w0);
        SympString {
            buf: vec![0.0; d as usize],
            q: 0,
            lp: OnePole { a: 1.0 - SYMP_POLE, z: 0.0 },
            ap: Thiran1::new(delta),
            g: (rho / mag).min(0.99995),
        }
    }

    /// Feeds `x` in and returns the sample leaving the delay line.
    #[inline(always)]
    fn step(&mut self, x: f64) -> f64 {
        let cur = self.buf[self.q] as f64;
        let v = x + self.g * self.ap.tick(self.lp.tick(cur));
        self.buf[self.q] = v as f32;
        self.q += 1;
        if self.q == self.buf.len() {
            self.q = 0;
        }
        cur
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.q = 0;
        self.lp.reset();
        self.ap.reset();
    }
}

/// Six sympathetic open strings.
pub struct Sympathetic {
    strings: [SympString; 6],
    excite: f64,
    asleep: bool,
}

impl Sympathetic {
    /// Loops for `OPEN_STRINGS`; `level` multiplies the 1.2% excitation
    /// (`TUNING.sympathetic` is 1).
    pub fn new(level: f64) -> Self {
        Sympathetic { strings: OPEN_STRINGS.map(SympString::new), excite: SYMP_EXCITE * level, asleep: true }
    }

    /// Adds the sympathetic strings' response to `buf` in place. Each
    /// output sample is the input plus the six loop outputs; the loops hear
    /// the input only. State carries over between calls.
    pub fn process(&mut self, buf: &mut [f32]) {
        if self.excite == 0.0 || !self.excite.is_finite() {
            return;
        }
        for block in buf.chunks_mut(SYMP_BLOCK) {
            let silent = block.iter().all(|x| x.abs() < SYMP_IN_FLOOR);
            if silent && self.asleep {
                continue;
            }
            let mut pk = [0.0f64; 6];
            for y in block.iter_mut() {
                let x = *y as f64 * self.excite;
                let mut sum = 0.0f64;
                for (s, p) in self.strings.iter_mut().zip(pk.iter_mut()) {
                    let d = s.step(x);
                    sum += d;
                    *p = p.max(d.abs());
                }
                *y = (*y as f64 + sum) as f32;
            }
            let quiet = pk.iter().all(|&p| p < SYMP_LOOP_FLOOR);
            if silent && quiet {
                for s in self.strings.iter_mut() {
                    s.clear();
                }
                self.asleep = true;
            } else {
                self.asleep = false;
            }
        }
    }
}

/// Renders the six guitar strings into a new buffer of `len` samples with
/// `params` (normally `PluckParams::GUITAR`). No sympathetic strings.
pub fn render_strings(strings: &[Vec<StringNote>; 6], params: &PluckParams, seed: u64, len: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    let mut scratch = PluckScratch::new();
    let mut order: Vec<usize> = Vec::new();
    for (s, notes) in strings.iter().enumerate() {
        order.clear();
        order.extend(0..notes.len());
        order.sort_by(|&a, &b| notes[a].t.total_cmp(&notes[b].t));
        for (j, &k) in order.iter().enumerate() {
            let n = &notes[k];
            let mut end = n.stop;
            if let Some(&k2) = order.get(j + 1) {
                end = end.min(notes[k2].t + RESTRIKE_S);
            }
            let (Ok(start), Ok(n_len)) = (usize::try_from(sample_at(n.t)), usize::try_from(sample_at(end - n.t))) else {
                continue;
            };
            if !n.t.is_finite() || !end.is_finite() || n_len == 0 {
                continue;
            }
            let f0 = mtof(n.midi as f64);
            let mut rng = Rng::event(seed, GUITAR_NOTE, ((s as u64) << 32) | k as u64);
            let p = params.note(f0, n.vel as f64, &mut rng);
            pluck_into(&mut out, start, f0, n_len, &p, &mut rng, &mut scratch);
        }
    }
    out
}

/// The accompaniment guitar: `render_strings` with `PluckParams::GUITAR`,
/// then the sympathetic strings at `TUNING.sympathetic`.
pub fn render_guitar(strings: &[Vec<StringNote>; 6], seed: u64, len: usize) -> Vec<f32> {
    let mut out = render_strings(strings, &PluckParams::GUITAR, seed, len);
    Sympathetic::new(TUNING.sympathetic).process(&mut out);
    out
}

/// Renders free plucked notes (bass pluck layer, harp, harmony guitar) into
/// a new buffer of `len` samples. Note k draws from
/// `Rng::event(seed, tag, k)`; its length follows `params.length`.
pub fn render_plucks(notes: &[PluckNote], params: &PluckParams, seed: u64, tag: Tag, len: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    pluck_notes(&mut out, notes, params, seed, tag);
    out
}

fn pluck_notes(out: &mut [f32], notes: &[PluckNote], params: &PluckParams, seed: u64, tag: Tag) {
    let mut scratch = PluckScratch::new();
    for (k, n) in notes.iter().enumerate() {
        let f0 = mtof(n.midi as f64);
        let secs = params.length_secs(f0, n.t0, n.t1);
        let (Ok(start), Ok(n_len)) = (usize::try_from(sample_at(n.t0)), usize::try_from(sample_at(secs))) else {
            continue;
        };
        let mut rng = Rng::event(seed, tag, k as u64);
        let p = params.note(f0, n.vel as f64, &mut rng);
        pluck_into(out, start, f0, n_len, &p, &mut rng, &mut scratch);
    }
}

/// Bass sub layer settings.
const SUB_LEVEL: f64 = 0.55;
const SUB_ATTACK_S: f64 = 0.006;
const SUB_DECAY_S: f64 = 0.7;
const SUB_RELEASE: usize = 2600;

/// The bass: `PluckParams::BASS` notes (stream tag `BASS_NOTE`) plus the
/// sine sub layer over the same length.
pub fn render_bass(notes: &[PluckNote], seed: u64, len: usize) -> Vec<f32> {
    let params = PluckParams::BASS;
    let mut out = vec![0.0f32; len];
    pluck_notes(&mut out, notes, &params, seed, BASS_NOTE);
    for n in notes {
        let f0 = mtof(n.midi as f64);
        let secs = params.length_secs(f0, n.t0, n.t1);
        let (Ok(start), Ok(n_len)) = (usize::try_from(sample_at(n.t0)), usize::try_from(sample_at(secs))) else {
            continue;
        };
        if start >= len || !f0.is_finite() || !n.vel.is_finite() {
            continue;
        }
        let n_len = n_len.min(len - start);
        sub_into(&mut out[start..start + n_len], f0, SUB_LEVEL * n.vel as f64);
    }
    out
}

/// Adds the sub sine over all of `out`: `sin(w (i + 1))` times the attack
/// ramp, the decay and the release ramp.
fn sub_into(out: &mut [f32], f0: f64, level: f64) {
    let len = out.len();
    let w = TAU * f0 / SR_F;
    let c2 = 2.0 * w.cos();
    let (mut s0, mut s1) = (0.0f64, w.sin());
    let decay = (-1.0 / (SUB_DECAY_S * SR_F)).exp();
    let mut env = level;
    let atk = SUB_ATTACK_S * SR_F;
    let n_atk = (atk.ceil() as usize).min(len);
    let rel0 = len.saturating_sub(SUB_RELEASE);
    let mut n = 0;
    while n < len {
        let mut end = len;
        for b in [n_atk, rel0] {
            if b > n && b < end {
                end = b;
            }
        }
        let (mut e, de) = if n < n_atk { (n as f64 / atk, 1.0 / atk) } else { (1.0, 0.0) };
        let (mut h, dh) = if n >= rel0 { ((len - n) as f64 / SUB_RELEASE as f64, -1.0 / SUB_RELEASE as f64) } else { (1.0, 0.0) };
        for o in out[n..end].iter_mut() {
            *o += (s1 * env * e * h) as f32;
            let s2 = c2 * s1 - s0;
            s0 = s1;
            s1 = s2;
            env *= decay;
            e += de;
            h += dh;
        }
        n = end;
    }
}

//! Source-filter singing synthesis: the LF glottal source (`glottal`), the
//! formant cascade (`tract`) and the noise sources, driven by per-frame
//! control tracks (`controls`), rendered phrase by phrase.
//!
//! Per sample, with av, ah, af, f0 ramped linearly across each HOP frame:
//!
//! ```text
//! (pulse, flow) = source(f0, av)
//! n             = white noise in [-1, 1)
//! breath        = lowpass_2.6k(n) * 1.9 * breath * av * (0.18 + 0.9 flow)
//! x             = pulse * av + (n * ah * 0.9 * 0.8 + breath) * 0.55
//! out           = dc(cascade(x) + 2.2 bandpass(n' * af))
//! ```
//!
//! Aspiration is raw white noise into the cascade; only breath noise is
//! low-passed at 2.6 kHz. Low-passing aspiration as well is open work, to
//! be decided by ear (design section 11).
//!
//! Frame work. F1-F3 targets are designed once per frame and ramped per
//! sample to the next frame's design (design 3.4). F2 and F3 wobble by a
//! bounded random walk per frame (dsp::stochastic, step 0.02, leak 0.97,
//! gain 0.01, limits 2% and 2.5%). F1 is floored at 1.06 f0. Bandwidths:
//! B1 = 60 + 80 breath + 50 nasal + b1x, B2 = 90 + 170 nasal, B3 = 130 +
//! 220 nasal. A frame in which every level is below 1e-5 and the tract is
//! quiet is skipped and the resonators are reset. The frication filter
//! runs only in frames where it is active or still ringing; the choice is
//! made per frame by a monomorphised inner loop.
//!
//! Random streams (sfcore::random, per singer seed): `voice.source`
//! (jitter, shimmer), `voice.noise` (aspiration, breath, frication),
//! `voice.formant` (formant wobble), `voice.controls` (vibrato phase, pitch
//! drift).

use std::borrow::Cow;
use std::ops::Range;

use dsp::biquad::{Biquad, BiquadCoeffs};
use dsp::onepole::OnePole;
use dsp::stochastic::RandomWalk;
use sfcore::math::mtof;
use sfcore::random::{tag, Rng, Tag};
use sfcore::{HOP, SR_F};
use song::events::{SingStyle, VocalNote};
use song::Voice;

use crate::controls::{Articulation, ControlTracks, Window};
use crate::glottal::GlottalSource;
use crate::params::{voice_params, VoiceParams};
use crate::phrasing::PhrasingParams;
use crate::tract::{Formants, Tract, MAX_HIGH};
use crate::tuning::{
    ASPIRATION_GAIN, BREATH_LP_HZ, BW_SCALE, HF_BRANCH_GAIN, HF_BRANCH_HZ, TILT_SCALE,
};

const SOURCE: Tag = tag("voice.source");
const NOISE: Tag = tag("voice.noise");
const FORMANT: Tag = tag("voice.formant");
const CONTROLS: Tag = tag("voice.controls");

/// Extra F1 bandwidth per unit of the b1x track (F1 damping in aspiration).
const B1X_GAIN: f64 = 1.0;
/// Level below which a control counts as off.
const SILENT: f32 = 1e-5;
/// A gap of at least this many seconds between notes ends a phrase.
pub const PHRASE_GAP: f64 = 0.3;
/// Seconds rendered after a phrase's last note (release and ring).
pub const PHRASE_TAIL: f64 = 0.5;
/// Seconds rendered before a phrase's first note (onset consonants and
/// the breath before the phrase, which starts up to 0.58 s early).
pub const PHRASE_LEAD: f64 = 0.7;

/// How one singer departs from the voice type's preset. Plain values,
/// 1 = unchanged for scales. Build from `SingStyle` with `From`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceSettings {
    /// Constant pitch offset, semitones.
    pub detune: f64,
    /// Seconds added to each note's t0 (not t1).
    pub lateness: f64,
    pub vibrato_scale: f64,
    pub vibrato_rate_scale: f64,
    /// Scale on F2 and above (tract length).
    pub formant_scale: f64,
    /// Scale on F1.
    pub f1_scale: f64,
    pub breath_scale: f64,
    /// Added to the breath level after scaling.
    pub breath_add: f64,
    /// Scale on Rd (above 1: laxer, breathier).
    pub rd_scale: f64,
    /// Scale on the spectral tilt corner.
    pub tilt_scale: f64,
    pub jitter_scale: f64,
    pub shimmer_scale: f64,
    /// High resonances above F5, 0-4.
    pub n_high: u8,
    /// Time constant of the voicing smoother, s.
    pub av_tau: f64,
    /// Time constant of the pitch glide, s.
    pub glide: f64,
    /// Pitch scoop into phrase-initial notes.
    pub scoop: bool,
    /// Audible breaths in pauses before phrases.
    pub breath_pauses: bool,
    /// Delivery and endings (design 5.2); `vibrato` and `glide` are
    /// already folded into `vibrato_scale` and `glide` above, so read the
    /// other fields from here.
    pub phrasing: PhrasingParams,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings::from(&SingStyle::LEAD)
    }
}

impl From<&SingStyle> for VoiceSettings {
    fn from(s: &SingStyle) -> Self {
        let phrasing = PhrasingParams::of(s.phrasing);
        VoiceSettings {
            detune: s.detune_cents as f64 / 100.0,
            lateness: s.lateness as f64,
            vibrato_scale: s.vibrato_scale as f64 * phrasing.vibrato,
            vibrato_rate_scale: s.vibrato_rate_scale as f64,
            formant_scale: s.formant_scale as f64,
            f1_scale: s.f1_scale as f64,
            breath_scale: s.breath_scale as f64,
            breath_add: s.breath_add as f64,
            rd_scale: s.rd_scale as f64,
            tilt_scale: 1.0,
            jitter_scale: s.jitter_scale as f64,
            shimmer_scale: s.shimmer_scale as f64,
            n_high: s.n_high,
            av_tau: s.av_tau as f64,
            glide: s.glide as f64 * phrasing.glide,
            scoop: s.scoop,
            breath_pauses: s.breath_pauses,
            phrasing,
        }
    }
}

impl VoiceSettings {
    /// The preset `p` with this singer's scales folded in.
    pub fn apply(&self, p: VoiceParams) -> VoiceParams {
        VoiceParams {
            fs: p.fs * self.formant_scale,
            f1s: p.f1s * self.f1_scale,
            breath: p.breath * self.breath_scale + self.breath_add,
            rd: p.rd * self.rd_scale,
            tilt: p.tilt * self.tilt_scale,
            jitter: p.jitter * self.jitter_scale,
            shimmer: p.shimmer * self.shimmer_scale,
            ..p
        }
    }
}

/// One singer's synthesiser: source, tract, noise, and per-frame state.
pub struct VoiceSynth {
    source: GlottalSource,
    tract: Tract,
    /// Aspiration, breath and frication noise.
    noise: Rng,
    /// Jitter and shimmer draws.
    source_rng: Rng,
    /// Formant wobble draws.
    formant_rng: Rng,
    wobble: [RandomWalk; 2],
    breath_lp: OnePole,
    /// The high-frequency voiced branch's 4th-order high-pass (`tuning::HF_BRANCH_*`).
    hf: [Biquad; 2],
    breath: f64,
    /// Per-frame noise buffers.
    asp: [f32; HOP],
    fric: [f32; HOP],
}

/// Per-frame values the inner loop ramps.
struct Frame {
    av: (f64, f64),
    ah: (f64, f64),
    af: (f64, f64),
    f0: (f64, f64),
}

impl VoiceSynth {
    /// A synthesiser for resolved parameters `p` (settings already
    /// applied) with `n_high` high resonances; random streams from `seed`.
    pub fn new(p: &VoiceParams, n_high: usize, seed: u64) -> VoiceSynth {
        VoiceSynth {
            source: GlottalSource::new(p.rd, p.tilt * TILT_SCALE, p.jitter, p.shimmer),
            tract: Tract::new(p, n_high.min(MAX_HIGH)),
            noise: Rng::stream(seed, NOISE),
            source_rng: Rng::stream(seed, SOURCE),
            formant_rng: Rng::stream(seed, FORMANT),
            wobble: [
                RandomWalk::bounded(0.02, 0.97, 0.01, 0.02),
                RandomWalk::bounded(0.02, 0.97, 0.01, 0.025),
            ],
            breath_lp: OnePole::from_hz(BREATH_LP_HZ, SR_F),
            hf: [
                Biquad::new(BiquadCoeffs::highpass(SR_F, HF_BRANCH_HZ, 0.5412)),
                Biquad::new(BiquadCoeffs::highpass(SR_F, HF_BRANCH_HZ, 1.3066)),
            ],
            breath: p.breath,
            asp: [0.0; HOP],
            fric: [0.0; HOP],
        }
    }

    /// Clear all filter and oscillator state (random streams continue).
    pub fn reset(&mut self) {
        self.source.reset();
        self.tract.reset();
        self.breath_lp.reset();
        self.hf[0].reset();
        self.hf[1].reset();
    }

    /// Formants of frame `m` (fundamental `f0` Hz) with the current wobble.
    fn formants(&self, ctl: &ControlTracks, m: usize, f0: f64) -> Formants {
        let nas = ctl.nas[m] as f64;
        Formants {
            f: [
                (ctl.f1[m] as f64).max(f0 * 1.06),
                ctl.f2[m] as f64 * (1.0 + self.wobble[0].w),
                ctl.f3[m] as f64 * (1.0 + self.wobble[1].w),
            ],
            bw: [
                (60.0 + self.breath * 80.0 + nas * 50.0 + ctl.b1x[m] as f64 * B1X_GAIN) * BW_SCALE,
                (90.0 + nas * 170.0) * BW_SCALE,
                (130.0 + nas * 220.0) * BW_SCALE,
            ],
        }
    }

    /// Render frames `frames` of `ctl` into `out`; `out[0]` is the first
    /// sample of frame `frames.start` (indices into `ctl`). Writes
    /// min(out.len(), frames * HOP) samples; silent frames are written as
    /// zeros. Frames at or past the last control frame are not rendered
    /// (frame m ramps to m + 1).
    pub fn render_frames(&mut self, ctl: &ControlTracks, frames: Range<usize>, out: &mut [f32]) {
        let n_f = ctl.av.len().min(ctl.midi.len());
        let last = frames.end.min(n_f.saturating_sub(1));
        let mut m = frames.start;
        let mut f0_next: Option<(usize, f64)> = None;
        while m < last {
            let s0 = (m - frames.start) * HOP;
            if s0 >= out.len() {
                return;
            }
            let s1 = (s0 + HOP).min(out.len());
            let dst = &mut out[s0..s1];
            let lv = |t: &[f32]| (t[m], t[m + 1]);
            let (av, ah, af) = (lv(&ctl.av), lv(&ctl.ah), lv(&ctl.af));
            let off = |x: (f32, f32)| x.0 < SILENT && x.1 < SILENT;
            if off(av) && off(ah) && off(af) && self.tract.is_quiet() {
                self.tract.reset_resonators();
                dst.fill(0.0);
                m += 1;
                continue;
            }

            // f0 at both frame edges; the end of this hop is the start of
            // the next, so it is carried over.
            let f0a = match f0_next {
                Some((k, f)) if k == m => f,
                _ => mtof(ctl.midi[m] as f64),
            };
            let f0b = mtof(ctl.midi[m + 1] as f64);
            f0_next = Some((m + 1, f0b));

            // Formants: jump to frame m after a reset, ramp to frame m + 1.
            if self.tract.is_fresh() {
                let now = self.formants(ctl, m, f0a);
                self.tract.jump_formants(&now);
            }
            for w in &mut self.wobble {
                w.step(&mut self.formant_rng);
            }
            let next = self.formants(ctl, m + 1, f0b);
            self.tract.ramp_formants(&next);

            let fric_on = af.0 > 1e-6 || af.1 > 1e-6 || self.tract.frication_ringing();
            if fric_on {
                self.tract
                    .set_frication(ctl.ff[m] as f64, ctl.fbw[m] as f64);
            } else {
                self.tract.reset_frication();
            }

            let d = |x: (f32, f32)| (x.0 as f64, x.1 as f64);
            let fr = Frame {
                av: d(av),
                ah: d(ah),
                af: d(af),
                f0: (f0a, f0b),
            };
            let n = dst.len();
            self.noise.fill_bipolar(&mut self.asp[..n]);
            if fric_on {
                self.noise.fill_bipolar(&mut self.fric[..n]);
            }
            if fric_on {
                self.hop::<true>(&fr, dst);
            } else {
                self.hop::<false>(&fr, dst);
            }
            m += 1;
        }
        let done = (last.saturating_sub(frames.start) * HOP).min(out.len());
        out[done..].fill(0.0);
    }

    /// One frame of samples in three passes: source and noise into the
    /// excitation (and the frication input), the cascade over the block,
    /// then the output stage. `FRIC` selects the frication path at compile
    /// time.
    #[inline(always)]
    fn hop<const FRIC: bool>(&mut self, fr: &Frame, out: &mut [f32]) {
        let n = out.len().min(HOP);
        let k = 1.0 / HOP as f64;
        let ramp = |x: (f64, f64)| (x.0, (x.1 - x.0) * k);
        let (av, dav) = ramp(fr.av);
        let (ah, dah) = ramp(fr.ah);
        let (af, daf) = ramp(fr.af);
        let breath = self.breath;
        let mut lp = self.breath_lp;
        let mut exc = [0.0f64; HOP];
        let mut vh = [0.0f64; HOP];
        let mut fin = [0.0f64; HOP];
        let (asp, fric) = (&self.asp, &self.fric);
        self.source.run(
            n,
            ramp(fr.f0),
            (av, dav),
            &mut self.source_rng,
            |j, pulse, flow| {
                let t = j as f64;
                let a = av + dav * t;
                let nz = asp[j] as f64;
                let n1 = lp.tick(nz) * 1.9;
                // HF bed: noise shaped by the glottal flow (voiced hiss, not buzz).
                vh[j] = nz * a * (0.15 + 0.85 * flow);
                exc[j] = pulse * a
                    + (nz * (ah + dah * t) * 0.9 * ASPIRATION_GAIN
                        + n1 * breath * a * (0.18 + 0.9 * flow))
                        * 0.55;
                if FRIC {
                    fin[j] = fric[j] as f64 * (af + daf * t);
                }
            },
        );
        self.breath_lp = lp;
        self.tract.cascade_block(&mut exc[..n]);
        if HF_BRANCH_GAIN != 0.0 {
            for j in 0..n {
                let x = self.hf[0].tick(vh[j]);
                let y = self.hf[1].tick(x);
                exc[j] += y * HF_BRANCH_GAIN;
            }
        }
        self.tract
            .finish_block::<FRIC>(&exc[..n], &fin[..n], &mut out[..n]);
        self.tract.flush_denormals();
        self.hf[0].flush_denormals();
        self.hf[1].flush_denormals();
    }
}

/// Maximal runs of notes with no gap of `PHRASE_GAP` or more, as index
/// ranges into `notes` (assumed in time order).
pub fn phrases(notes: &[VocalNote]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut end_t = f64::NEG_INFINITY;
    for (k, n) in notes.iter().enumerate() {
        if k > start && n.t0 - end_t >= PHRASE_GAP {
            out.push(start..k);
            start = k;
        }
        end_t = if k == start { n.t1 } else { end_t.max(n.t1) };
    }
    if start < notes.len() {
        out.push(start..notes.len());
    }
    out
}

/// Frame span of each phrase of `notes` on a grid of `n_f` frames, as
/// (note range, frame range) pairs: `PHRASE_LEAD` before the first note to
/// `PHRASE_TAIL` after the last, clipped so spans never overlap. Where the
/// next phrase's lead reaches back before this phrase's last note ends,
/// the boundary moves to the middle of the pause between that end and the
/// next phrase's first onset: both windows plan that pause alike (silence,
/// or a breath), so the smoothers restart where the tracks are flat, not
/// inside a note.
pub(crate) fn phrase_spans(
    notes: &[VocalNote],
    art: &Articulation,
    n_f: usize,
) -> Vec<(Range<usize>, Range<usize>)> {
    let fr = SR_F / HOP as f64;
    let frame_at = |t: f64, up: bool| -> usize {
        let x = t * fr;
        let x = if up { x.ceil() } else { x.floor() };
        if x.is_finite() && x > 0.0 {
            (x as usize).min(n_f)
        } else {
            0
        }
    };
    let groups = phrases(notes);
    let mut spans = Vec::with_capacity(groups.len());
    let mut prev_end = 0usize;
    for (i, g) in groups.iter().enumerate() {
        let t0 = notes[g.start].t0;
        let t1 = notes[g.clone()].iter().map(|n| n.t1).fold(t0, f64::max);
        let start = frame_at(t0 - PHRASE_LEAD, false).max(prev_end);
        let mut end = frame_at(t1 + PHRASE_TAIL, true);
        if let Some(nx) = groups.get(i + 1) {
            let next_t0 = notes[nx.start].t0;
            let mut b = next_t0 - PHRASE_LEAD;
            if b < t1 {
                let on = art.onset_start(nx.start).unwrap_or(next_t0).min(next_t0);
                b = 0.5 * (t1 + on.max(t1));
            }
            end = end.min(frame_at(b, false).max(start));
        }
        if end > start {
            spans.push((g.clone(), start..end));
            prev_end = end;
        }
    }
    spans
}

/// Render one singer: `notes` sung by `voice` with `settings`, random
/// streams from `seed`, song length `len` samples. Each phrase (a maximal
/// run of notes without a gap of 0.3 s or more, plus 0.7 s before and 0.5 s
/// after) gets its own control tracks (`controls::Articulation`), is
/// rendered into a reused scratch buffer and passed to
/// `emit(start_sample, samples)`. Emitted ranges are disjoint, in time
/// order, and lie inside 0..len.
pub fn render_phrases(
    notes: &[VocalNote],
    voice: Voice,
    settings: &VoiceSettings,
    seed: u64,
    len: usize,
    emit: impl FnMut(usize, &[f32]),
) {
    let p = settings.apply(voice_params(voice));
    render_phrases_with(notes, &p, settings, seed, len, emit);
}

/// `render_phrases` with resolved parameters `p` (the settings' scales on
/// the preset are already applied; only the settings' articulation fields,
/// lateness and `n_high` are read here).
pub(crate) fn render_phrases_with(
    notes: &[VocalNote],
    p: &VoiceParams,
    settings: &VoiceSettings,
    seed: u64,
    len: usize,
    mut emit: impl FnMut(usize, &[f32]),
) {
    if notes.is_empty() || len == 0 {
        return;
    }
    let notes: Cow<[VocalNote]> = if settings.lateness != 0.0 {
        Cow::Owned(
            notes
                .iter()
                .map(|n| VocalNote {
                    t0: n.t0 + settings.lateness,
                    ..n.clone()
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(notes)
    };
    let n_f = len.div_ceil(HOP) + 2;
    let mut synth = VoiceSynth::new(p, settings.n_high as usize, seed);

    let mut art = Articulation::new(&notes, p, settings, Rng::stream(seed, CONTROLS));
    // `art.notes()` is `notes` with `phrasing::phrase_notes` applied (a
    // no-op copy for the default phrasing); phrase splitting and the
    // phrase spans use those shaped times, matching what `art` plans.
    let spans = phrase_spans(art.notes(), &art, n_f);

    // Control tracks per phrase: the span plus the frame its last frame
    // ramps to.
    let mut ctl = ControlTracks::default();
    let longest = spans.iter().map(|(_, s)| s.len()).max().unwrap_or(0) * HOP;
    let mut scratch = vec![0.0f32; longest];
    let mut prev_end = 0usize;
    for (g, s) in spans {
        if s.start > prev_end {
            synth.reset();
        }
        prev_end = s.end;
        let s0 = s.start * HOP;
        if s0 >= len {
            break;
        }
        art.phrase(
            g,
            Window {
                start: s.start,
                len: s.len() + 1,
            },
            &mut ctl,
        );
        let n = (s.len() * HOP).min(len - s0);
        let buf = &mut scratch[..n];
        synth.render_frames(&ctl, 0..s.len(), buf);
        emit(s0, buf);
    }
}

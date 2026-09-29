//! Frame-rate control tracks for the voice synthesiser, one frame per HOP
//! samples (689 Hz), from the segment plan of `crate::articulation`.
//!
//! Tracks are built for a `Window` of the song's frame grid, one phrase at
//! a time (`Articulation::phrase`), so the cost and memory follow the sung
//! frames, not the song length. All frame arithmetic is on absolute frames
//! and clipped to the window. Pipeline per window:
//!
//! 0. `plan_syllables` for the phrase's notes and every earlier note that
//!    still writes frames inside the window (at least the note before the
//!    phrase: its pause, and the breath in it, lead into the phrase).
//! 1. `rasterise`: each segment writes its fields into the frames it
//!    covers, in start order (a later segment overwrites only its own
//!    fields).
//! 2. `shape_dynamics`: per-note swell (notes over 0.5 s, 0.9 + 0.16 sin)
//!    and a 40% fade over the last 45% of a phrase-final note.
//! 3. Zero-phase one-pole smoothing (`dsp::onepole::zero_phase_smooth_lanes`) of
//!    every track except pitch; time constants in `SMOOTH_*` and the
//!    singer's `av_tau`.
//! 4. `pitch_track`: the note's MIDI pitch from its onset to the next
//!    onset, a 1.1 semitone scoop over 70 ms into phrase-initial notes, the
//!    grace pitch over min(110 ms, 25%) of a note, then a one-sided
//!    one-pole glide (time constant `glide`) so the pitch arrives late and
//!    settles.
//! 5. `add_vibrato`: sinusoidal vibrato on notes of 0.4 s or more, starting
//!    220 ms into the note and rising over 380 ms by smoothstep, 15% deeper
//!    on phrase-final notes, envelope smoothed (50 ms); rate wobbles 6% at
//!    0.11 Hz. Start phase uniform from the singer's control stream.
//! 6. `add_drift`: a leaky second-order random walk (dsp::stochastic,
//!    step 0.004, leak 0.985, position leak 0.998, limit 0.12 semitone)
//!    plus the singer's detune. The walk and the vibrato phase run on
//!    from one window to the next.

use std::borrow::Cow;
use std::f64::consts::{PI, TAU};
use std::ops::Range;

use dsp::onepole::{zero_phase_smooth, zero_phase_smooth_lanes};
use dsp::stochastic::RandomWalk;
use sfcore::math::{one_pole_coeff_tau, smoothstep};
use sfcore::random::Rng;
use sfcore::{HOP, SR_F};
use song::events::VocalNote;

use crate::articulation::{plan_syllables, syllables, Segment, Span, Syllable};
use crate::params::VoiceParams;
use crate::phrasing::{phrase_notes, PhrasingParams};
use crate::synth::VoiceSettings;

/// Control frames per second.
pub const FRAME_RATE: f64 = SR_F / HOP as f64;
/// Notes whose frames end this close before a window's start are planned
/// in it too, s (covers frame rounding).
const WINDOW_MARGIN: f64 = 0.05;
/// Pause planned after the song's last note, s (`plan_syllables`).
const LAST_PAUSE: f64 = 0.5;

/// Zero-phase smoothing time constants, s.
const SMOOTH_F1: f64 = 0.016;
const SMOOTH_F2: f64 = 0.018;
const SMOOTH_F3: f64 = 0.02;
const SMOOTH_NASAL: f64 = 0.02;
const SMOOTH_ASPIRATION: f64 = 0.006;
const SMOOTH_FRICATION: f64 = 0.002;
const SMOOTH_B1X: f64 = 0.006;
const SMOOTH_NOISE_BAND: f64 = 0.004;
const SMOOTH_VIBRATO: f64 = 0.05;

/// Breath noise level of a `Segment::Breath`.
const BREATH_AH: f32 = 0.045;
/// Pitch scoop into a phrase-initial note: semitones below, seconds.
const SCOOP_DEPTH: f32 = 1.1;
const SCOOP_TIME: f64 = 0.07;
/// Longest grace note, s, and its largest share of the note.
const GRACE_TIME: f64 = 0.11;
const GRACE_SHARE: f64 = 0.25;
/// Pitch held after the last note, s.
const LAST_HOLD: f64 = 0.3;
/// Vibrato: shortest note, onset delay, rise time (s), phrase-end depth.
const VIBRATO_MIN_NOTE: f64 = 0.4;
const VIBRATO_DELAY: f64 = 0.22;
const VIBRATO_RISE: f64 = 0.38;
const VIBRATO_PHRASE_END: f64 = 1.15;
/// Vibrato envelope below which no vibrato is added, semitones.
const VIBRATO_FLOOR: f32 = 1e-6;
/// Vibrato rate wobble: depth and angular rate (rad/s).
const RATE_WOBBLE: f64 = 0.06;
const RATE_WOBBLE_W: f64 = 0.7;
/// Pitch drift walk: step, velocity leak, position leak, limit (semitones).
const DRIFT: (f64, f64, f64, f64) = (0.004, 0.985, 0.998, 0.12);

/// Per-frame controls the synthesiser reads, all of one length.
#[derive(Clone, Debug, Default)]
pub struct ControlTracks {
    /// Voicing amplitude.
    pub av: Vec<f32>,
    /// Aspiration (and breath) noise amplitude.
    pub ah: Vec<f32>,
    /// Frication noise amplitude.
    pub af: Vec<f32>,
    /// Frication band centre, Hz.
    pub ff: Vec<f32>,
    /// Frication bandwidth, Hz.
    pub fbw: Vec<f32>,
    /// F1-F3 targets, Hz.
    pub f1: Vec<f32>,
    pub f2: Vec<f32>,
    pub f3: Vec<f32>,
    /// Nasality 0-1 (widens B1-B3).
    pub nas: Vec<f32>,
    /// Pitch, fractional MIDI.
    pub midi: Vec<f32>,
    /// Extra F1 bandwidth during aspiration, Hz.
    pub b1x: Vec<f32>,
}

impl ControlTracks {
    /// `frames` frames at rest: silent, neutral formants, pitch 0.
    pub fn neutral(frames: usize) -> ControlTracks {
        let mut c = ControlTracks::default();
        c.reset(frames);
        c
    }

    /// Set every track to `frames` frames at rest, reusing the buffers.
    pub fn reset(&mut self, frames: usize) {
        let set = |v: &mut Vec<f32>, x: f32| {
            v.clear();
            v.resize(frames, x);
        };
        set(&mut self.av, 0.0);
        set(&mut self.ah, 0.0);
        set(&mut self.af, 0.0);
        set(&mut self.ff, 4000.0);
        set(&mut self.fbw, 3000.0);
        set(&mut self.f1, 500.0);
        set(&mut self.f2, 1500.0);
        set(&mut self.f3, 2500.0);
        set(&mut self.nas, 0.0);
        set(&mut self.midi, 0.0);
        set(&mut self.b1x, 0.0);
    }

    pub fn len(&self) -> usize {
        self.av.len()
    }

    pub fn is_empty(&self) -> bool {
        self.av.is_empty()
    }
}

/// Frames `start..start + len` of the song's frame grid (frame i starts
/// at sample i HOP).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub start: usize,
    pub len: usize,
}

impl Window {
    /// The whole song from frame 0.
    pub fn song(len: usize) -> Window {
        Window { start: 0, len }
    }

    /// Nearest absolute frame of time `t` (saturating; NaN gives 0).
    #[inline]
    pub fn frame(t: f64) -> isize {
        (t * FRAME_RATE).round() as isize
    }

    /// Absolute frames `a..b` as indices into this window, clipped.
    #[inline]
    pub fn clip(&self, a: isize, b: isize) -> Range<usize> {
        let s = self.start as isize;
        let len = self.len as isize;
        let lo = a.saturating_sub(s).clamp(0, len);
        let hi = b.saturating_sub(s).clamp(lo, len);
        lo as usize..hi as usize
    }

    /// Absolute frame of window index `i`.
    #[inline]
    fn abs(&self, i: usize) -> isize {
        (self.start + i) as isize
    }
}

/// The segments of `plan` written into `out`, reset to `w.len` neutral
/// frames. Each kind writes F1-F3 and its own fields only; later segments
/// overwrite earlier ones. `breath` scales `BREATH_AH` (`PhrasingParams`).
pub fn rasterise(plan: &[(Span, Segment)], w: Window, out: &mut ControlTracks, breath: f64) {
    let c = out;
    c.reset(w.len);
    for (span, seg) in plan {
        let r = w.clip(Window::frame(span.t0).max(0), Window::frame(span.t1).max(0));
        if r.is_empty() {
            continue;
        }
        let f = seg.formants();
        c.f1[r.clone()].fill(f[0] as f32);
        c.f2[r.clone()].fill(f[1] as f32);
        c.f3[r.clone()].fill(f[2] as f32);
        match *seg {
            Segment::Vowel { av, .. } | Segment::Sonorant { av, .. } => {
                c.av[r.clone()].fill(av as f32);
                c.nas[r].fill(0.0);
            }
            Segment::Nasal { av, .. } => {
                c.av[r.clone()].fill(av as f32);
                c.nas[r].fill(1.0);
            }
            Segment::Fricative {
                av, af, ff, fbw, ..
            } => {
                c.av[r.clone()].fill(av as f32);
                c.af[r.clone()].fill(af as f32);
                c.ff[r.clone()].fill(ff as f32);
                c.fbw[r].fill(fbw as f32);
            }
            Segment::Aspiration { ah, b1x, .. } => {
                c.ah[r.clone()].fill(ah as f32);
                c.b1x[r].fill(b1x as f32);
            }
            Segment::Burst {
                af, ff, fbw, ah, ..
            } => {
                c.af[r.clone()].fill(af as f32);
                c.ff[r.clone()].fill(ff as f32);
                c.fbw[r.clone()].fill(fbw as f32);
                c.ah[r.clone()].fill(ah as f32);
                c.b1x[r].fill(0.0);
            }
            Segment::Closure { av, .. } => c.av[r].fill(av as f32),
            Segment::Breath { .. } => c.ah[r].fill(BREATH_AH * breath as f32),
            Segment::Silence { .. } => {}
        }
    }
}

/// Per-note swell and phrase-end fade on the voicing track `av` of window
/// `w`, for `notes`. `ph.swell`, `ph.fade_depth` and `ph.fade_from` replace
/// the 0.16, 0.4 and 0.55 of today's constants.
pub fn shape_dynamics(av: &mut [f32], notes: &[VocalNote], w: Window, ph: &PhrasingParams) {
    for n in notes {
        let dur = n.t1 - n.t0;
        if dur <= 0.5 && !n.phrase_end {
            continue;
        }
        let i0 = Window::frame(n.t0);
        let i1 = Window::frame(n.t1);
        let len = (i1 - i0).max(1) as f64;
        for i in w.clip(i0, i1) {
            let x = (w.abs(i) - i0) as f64 / len;
            let mut e = if dur > 0.5 {
                0.9 + ph.swell * (PI * (x * 1.1).min(1.0)).sin()
            } else {
                1.0
            };
            if n.phrase_end {
                e *= 1.0 - ph.fade_depth * smoothstep(ph.fade_from, 1.0, x);
            }
            av[i] = (av[i] as f64 * e) as f32;
        }
    }
}

/// Pitch of notes `range` into `out` (window `w`, one value per frame):
/// note pitch from its onset to the next note's onset (0.3 s past the end
/// for the song's last note), scoop, grace, frames before the first onset
/// set to its pitch and gaps holding the last, then the one-sided glide.
/// `syl` from `articulation::syllables` for all of `notes`.
pub fn pitch_track(
    notes: &[VocalNote],
    syl: &[Syllable],
    range: Range<usize>,
    settings: &VoiceSettings,
    w: Window,
    out: &mut [f32],
) {
    let m = out;
    m.fill(0.0);
    let range = range.start.min(notes.len())..range.end.min(notes.len()).min(syl.len());
    for k in range {
        let (n, s) = (&notes[k], &syl[k]);
        let i0 = Window::frame(n.t0).max(0);
        let start = Window::frame(s.onset_start).max(0);
        let end = match syl.get(k + 1) {
            Some(ns) => Window::frame(ns.onset_start),
            None => Window::frame(n.t1) + Window::frame(LAST_HOLD),
        };
        m[w.clip(start, end)].fill(n.midi);
        if n.phrase_start && settings.scoop {
            m[w.clip(start, i0 + Window::frame(SCOOP_TIME).max(0))].fill(n.midi - SCOOP_DEPTH);
        }
        if let Some(grace) = n.grace {
            let g = Window::frame(GRACE_TIME.min((n.t1 - n.t0) * GRACE_SHARE)).max(0);
            m[w.clip(i0, i0 + g)].fill(grace);
        }
    }

    let mut last = m.iter().copied().find(|&v| v != 0.0).unwrap_or(0.0);
    for v in m.iter_mut() {
        if *v != 0.0 {
            last = *v;
        } else {
            *v = last;
        }
    }

    // One-sided glide: the pitch arrives after the note starts.
    if let Some(&first) = m.first() {
        let a = one_pole_coeff_tau(settings.glide, FRAME_RATE);
        let mut y = first as f64;
        for v in m.iter_mut() {
            y += a * (*v as f64 - y);
            *v = y as f32;
        }
    }
}

/// Vibrato generator: its phase runs on across windows.
#[derive(Clone, Debug)]
pub struct Vibrato {
    /// Depth, semitones.
    pub depth: f64,
    /// Rate, Hz.
    pub rate: f64,
    /// Current phase, radians.
    pub phase: f64,
    /// Envelope scratch.
    env: Vec<f32>,
}

impl Vibrato {
    pub fn new(depth: f64, rate: f64, phase: f64) -> Vibrato {
        Vibrato {
            depth,
            rate,
            phase,
            env: Vec::new(),
        }
    }

    /// Add vibrato for `notes` to `midi` (window `w`). See the module doc
    /// for the envelope.
    pub fn add(&mut self, midi: &mut [f32], notes: &[VocalNote], w: Window) {
        let env = &mut self.env;
        env.clear();
        env.resize(midi.len(), 0.0);
        for n in notes {
            if n.t1 - n.t0 < VIBRATO_MIN_NOTE {
                continue;
            }
            let a = Window::frame(n.t0 + VIBRATO_DELAY);
            let d = self.depth
                * if n.phrase_end {
                    VIBRATO_PHRASE_END
                } else {
                    1.0
                };
            for i in w.clip(a.max(0), Window::frame(n.t1)) {
                env[i] =
                    (d * smoothstep(0.0, VIBRATO_RISE * FRAME_RATE, (w.abs(i) - a) as f64)) as f32;
            }
        }
        zero_phase_smooth(env, one_pole_coeff_tau(SMOOTH_VIBRATO, FRAME_RATE));
        // Phase advance per frame: rate (1 + 0.06 sin(w t)) / FRAME_RATE at
        // absolute frame time t; the wobble sine comes from a rotating
        // phasor, and sin(phase) is evaluated only where the envelope is
        // audible.
        let dph = TAU * self.rate / FRAME_RATE;
        let (sw, cw) = (RATE_WOBBLE_W / FRAME_RATE).sin_cos();
        let (mut ws, mut wc) = (w.start as f64 * RATE_WOBBLE_W / FRAME_RATE).sin_cos();
        let mut ph = self.phase;
        for (m, &e) in midi.iter_mut().zip(env.iter()) {
            ph += dph * (1.0 + RATE_WOBBLE * ws);
            (ws, wc) = (ws * cw + wc * sw, wc * cw - ws * sw);
            if e > VIBRATO_FLOOR {
                *m = (*m as f64 + e as f64 * ph.sin()) as f32;
            }
        }
        self.phase = ph % TAU;
    }
}

/// Add slow pitch drift (one step of `walk` per frame, deviates from
/// `rng`) and a constant `detune` in semitones to `midi`.
pub fn add_drift(midi: &mut [f32], walk: &mut RandomWalk, rng: &mut Rng, detune: f64) {
    for m in midi.iter_mut() {
        *m = (*m as f64 + walk.step(rng) + detune) as f32;
    }
}

/// The pitch drift walk of `add_drift`.
pub fn drift_walk() -> RandomWalk {
    let (step, leak, w_leak, limit) = DRIFT;
    RandomWalk::leaky(step, leak, w_leak, limit)
}

/// One singer's articulation state: the notes with `phrasing::phrase_notes`
/// applied, their syllables, the vibrato phase and drift walk that run
/// across phrases, and reused buffers.
pub struct Articulation<'a> {
    notes: Cow<'a, [VocalNote]>,
    syl: Vec<Syllable>,
    p: VoiceParams,
    settings: VoiceSettings,
    rng: Rng,
    vibrato: Vibrato,
    drift: RandomWalk,
    plan: Vec<(Span, Segment)>,
}

impl<'a> Articulation<'a> {
    /// For `notes` (in time order) sung with resolved parameters `p`
    /// (settings applied). Applies `phrasing::phrase_notes` first (a
    /// no-op copy for the default phrasing). `rng` is the singer's control
    /// stream: one uniform draw now (vibrato phase), then one normal draw
    /// per rendered frame (drift).
    pub fn new(
        notes: &'a [VocalNote],
        p: &VoiceParams,
        settings: &VoiceSettings,
        mut rng: Rng,
    ) -> Self {
        let phase = rng.uniform() * TAU;
        let notes = phrase_notes(notes, &settings.phrasing);
        let syl = syllables(&notes, p, &settings.phrasing);
        Articulation {
            notes,
            syl,
            p: *p,
            settings: *settings,
            rng,
            vibrato: Vibrato::new(
                p.vib_depth * settings.vibrato_scale,
                p.vib_rate * settings.vibrato_rate_scale,
                phase,
            ),
            drift: drift_walk(),
            plan: Vec::new(),
        }
    }

    /// The notes actually sung: `phrase_notes` applied to the notes given
    /// to `new`.
    pub fn notes(&self) -> &[VocalNote] {
        &self.notes
    }

    /// Time note `k` first sounds: its first onset consonant, s.
    pub fn onset_start(&self, k: usize) -> Option<f64> {
        self.syl.get(k).map(|s| s.onset_start)
    }

    /// Last time note `k` writes a control frame, s: its pause (and a
    /// breath in it) and its pitch run to the next note's onset.
    fn reach(&self, k: usize) -> f64 {
        let t1 = self.notes[k].t1;
        match self.syl.get(k + 1) {
            Some(ns) => t1.max(ns.onset_start),
            None => t1 + LAST_HOLD.max(LAST_PAUSE),
        }
    }

    /// Control tracks of window `w` for the phrase of notes `range` into
    /// `out`. Every earlier note that still writes frames at or after the
    /// window's start is planned too (at least the note before `range`,
    /// whose pause and breath lead into the phrase), so a window clipped
    /// to start inside the previous phrase matches the whole-song tracks.
    pub fn phrase(&mut self, range: Range<usize>, w: Window, out: &mut ControlTracks) {
        let end = range.end.min(self.notes.len()).min(self.syl.len());
        let t_start = w.start as f64 / FRAME_RATE - WINDOW_MARGIN;
        let mut lo = range.start.min(end).saturating_sub(1);
        while lo > 0 && self.reach(lo - 1) >= t_start {
            lo -= 1;
        }
        let range = lo..end;
        let notes = self.notes.get(range.clone()).unwrap_or(&[]);
        plan_syllables(
            &self.notes,
            &self.syl,
            range.clone(),
            &self.p,
            &self.settings,
            &mut self.plan,
        );
        rasterise(&self.plan, w, out, self.settings.phrasing.breath);
        shape_dynamics(&mut out.av, notes, w, &self.settings.phrasing);

        let taus = [
            SMOOTH_F1,
            SMOOTH_F2,
            SMOOTH_F3,
            SMOOTH_NASAL,
            self.settings.av_tau,
            SMOOTH_ASPIRATION,
            SMOOTH_FRICATION,
            SMOOTH_B1X,
            SMOOTH_NOISE_BAND,
            SMOOTH_NOISE_BAND,
        ];
        let c = &mut *out;
        let smoothed = zero_phase_smooth_lanes(
            [
                &mut c.f1, &mut c.f2, &mut c.f3, &mut c.nas, &mut c.av, &mut c.ah, &mut c.af,
                &mut c.b1x, &mut c.ff, &mut c.fbw,
            ],
            taus.map(|t| one_pole_coeff_tau(t, FRAME_RATE)),
        );
        // Every ControlTracks track has the window's frame count.
        debug_assert!(smoothed.is_ok(), "control tracks of unequal length");

        pitch_track(
            &self.notes,
            &self.syl,
            range,
            &self.settings,
            w,
            &mut out.midi,
        );
        self.vibrato.add(&mut out.midi, notes, w);
        add_drift(
            &mut out.midi,
            &mut self.drift,
            &mut self.rng,
            self.settings.detune,
        );
    }
}

/// Control tracks of `frames` frames from frame 0 for all of `notes`: one
/// window over the whole song (tests and probes; the synthesiser renders
/// phrase by phrase).
pub fn control_tracks(
    notes: &[VocalNote],
    p: &VoiceParams,
    settings: &VoiceSettings,
    frames: usize,
    rng: Rng,
) -> ControlTracks {
    let mut out = ControlTracks::default();
    Articulation::new(notes, p, settings, rng).phrase(
        0..notes.len(),
        Window::song(frames),
        &mut out,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::voice_params;
    use song::{Phoneme, Voice};

    fn note(t0: f64, t1: f64, midi: f32, phones: &[Phoneme]) -> VocalNote {
        VocalNote {
            t0,
            t1,
            midi,
            phones: phones.to_vec(),
            amp: 0.9,
            stress: true,
            phrase_start: false,
            phrase_end: false,
            grace: None,
        }
    }

    fn song() -> Vec<VocalNote> {
        use Phoneme::*;
        let mut v = vec![
            note(0.5, 1.4, 50.0, &[Dh, Ax]),
            note(1.5, 2.2, 52.0, &[R, Ih]),
            note(2.25, 2.4, 55.0, &[V, Er]),
            note(2.45, 3.6, 57.0, &[S, T, R, Ay, K]),
            note(4.5, 5.0, 53.0, &[Ch, Ey, N, Jh]),
            note(5.0, 6.2, 48.0, &[Hh, Ow, L, D, Z]),
        ];
        v[0].phrase_start = true;
        v[4].phrase_start = true;
        v[3].phrase_end = true;
        v[5].phrase_end = true;
        v[4].grace = Some(51.0);
        v
    }

    fn frame(t: f64) -> usize {
        Window::frame(t).max(0) as usize
    }

    fn frames_for(notes: &[VocalNote]) -> usize {
        frame(notes.last().map_or(0.0, |n| n.t1) + 1.0)
    }

    #[test]
    fn tracks_are_finite_for_every_voice() {
        let notes = song();
        let n = frames_for(&notes);
        for voice in [
            Voice::Bass,
            Voice::Baritone,
            Voice::Tenor,
            Voice::Alto,
            Voice::Soprano,
        ] {
            let settings = VoiceSettings::default();
            let p = settings.apply(voice_params(voice));
            let c = control_tracks(&notes, &p, &settings, n, Rng::from_seed(7));
            for t in [
                &c.av, &c.ah, &c.af, &c.ff, &c.fbw, &c.f1, &c.f2, &c.f3, &c.nas, &c.midi, &c.b1x,
            ] {
                assert_eq!(t.len(), n);
                assert!(t.iter().all(|x| x.is_finite()), "{voice:?}");
            }
            assert!(c.av.iter().any(|&x| x > 0.5));
            assert!(
                c.midi.iter().all(|&m| (40.0..65.0).contains(&m)),
                "{voice:?}"
            );
        }
    }

    #[test]
    fn empty_and_degenerate_input() {
        let settings = VoiceSettings::default();
        let p = voice_params(Voice::Tenor);
        let c = control_tracks(&[], &p, &settings, 10, Rng::from_seed(1));
        assert_eq!(c.len(), 10);
        let c = control_tracks(&song(), &p, &settings, 0, Rng::from_seed(1));
        assert!(c.is_empty());
        let bad = [
            note(-3.0, -1.0, 50.0, &[]),
            note(1e6, 1e6 + 1.0, 50.0, &[Phoneme::T]),
        ];
        let c = control_tracks(&bad, &p, &settings, 100, Rng::from_seed(1));
        assert!(c.midi.iter().chain(&c.av).all(|x| x.is_finite()));
    }

    /// Without vibrato and drift, the pitch sits within 5 cents of the note
    /// in the middle 60% of each long note without a grace note.
    #[test]
    fn pitch_track_hits_the_note() {
        let notes = song();
        let settings = VoiceSettings::default();
        let p = voice_params(Voice::Baritone);
        let syl = syllables(&notes, &p, &settings.phrasing);
        let mut m = vec![0.0; frames_for(&notes)];
        pitch_track(
            &notes,
            &syl,
            0..notes.len(),
            &settings,
            Window::song(m.len()),
            &mut m,
        );
        for n in notes
            .iter()
            .filter(|n| n.t1 - n.t0 >= 0.4 && n.grace.is_none())
        {
            let d = n.t1 - n.t0;
            let (a, b) = (frame(n.t0 + 0.2 * d), frame(n.t1 - 0.2 * d));
            for (i, &mi) in m[a..b].iter().enumerate() {
                assert!(
                    (mi - n.midi).abs() < 0.05,
                    "note {} frame {}: {mi}",
                    n.midi,
                    a + i
                );
            }
        }
        // The scoop starts the first phrase below the note; the grace note
        // starts at its grace pitch and ends on the note.
        assert!(m[frame(notes[0].t0)] < notes[0].midi - 0.3);
        let g = &notes[4];
        let low = m[frame(g.t0)..frame(g.t0 + 0.11)]
            .iter()
            .copied()
            .fold(f32::MAX, f32::min);
        assert!(low < g.midi - 1.5, "grace low {low}");
        assert!((m[frame(g.t1 - 0.15)] - g.midi).abs() < 0.05);
    }

    /// Vibrato is absent for the first 150 ms of a note and full depth in
    /// its sustain.
    #[test]
    fn vibrato_onset_is_delayed() {
        let n = note(0.5, 2.5, 52.0, &[Phoneme::Aa]);
        let frames = frame(3.5);
        let mut m = vec![52.0f32; frames];
        let w = Window::song(frames);
        Vibrato::new(0.3, 5.5, 0.0).add(&mut m, std::slice::from_ref(&n), w);
        let dev = |a: f64, b: f64| {
            (frame(a)..frame(b))
                .map(|i| (m[i] - 52.0).abs())
                .fold(0.0f32, f32::max)
        };
        assert!(dev(0.5, 0.65) < 0.03, "early {}", dev(0.5, 0.65));
        let late = dev(1.4, 2.3);
        assert!((late - 0.3).abs() < 0.02, "sustain {late}");
        // Short notes get none.
        let mut m = vec![52.0f32; frames];
        Vibrato::new(0.3, 5.5, 0.0).add(&mut m, &[note(0.5, 0.85, 52.0, &[])], w);
        assert!(m.iter().all(|&x| x == 52.0));
    }

    /// Tracks built on the clipped phrase spans of `synth::phrase_spans`
    /// (as the synthesiser renders them) against one window over the whole
    /// song, over every frame of every span. Segment tracks agree up to the
    /// smoothers' restart at a span edge, which falls in a pause (formants
    /// are compared only where a source sounds); pitch
    /// agrees within twice the drift limit (vibrato off: its phase runs
    /// only over rendered frames).
    fn check_spans(notes: &[VocalNote], voice: Voice) {
        let n = frames_for(notes);
        let settings = VoiceSettings {
            vibrato_scale: 0.0,
            ..VoiceSettings::default()
        };
        let p = settings.apply(voice_params(voice));
        let whole = control_tracks(notes, &p, &settings, n, Rng::from_seed(9));
        let mut art = Articulation::new(notes, &p, &settings, Rng::from_seed(9));
        let spans = crate::synth::phrase_spans(notes, &art, n);
        assert!(spans.len() > 1);
        let mut c = ControlTracks::default();
        for (g, s) in spans {
            let w = Window {
                start: s.start,
                len: s.len(),
            };
            art.phrase(g, w, &mut c);
            for i in 0..w.len {
                let j = w.start + i;
                // Formants and nasality matter only where a source sounds.
                let heard = whole.av[j] > 0.01 || whole.ah[j] > 0.002 || whole.af[j] > 0.002;
                for (name, a, b, tol) in [
                    ("av", &c.av, &whole.av, 0.02),
                    ("ah", &c.ah, &whole.ah, 0.01),
                    ("af", &c.af, &whole.af, 0.01),
                    ("f1", &c.f1, &whole.f1, 30.0),
                    ("f2", &c.f2, &whole.f2, 60.0),
                    ("nas", &c.nas, &whole.nas, 0.05),
                ] {
                    if !heard && matches!(name, "f1" | "f2" | "nas") {
                        continue;
                    }
                    assert!(
                        (a[i] - b[j]).abs() <= tol,
                        "{voice:?} {name} frame {j} (span {s:?}): {} vs {}",
                        a[i],
                        b[j]
                    );
                }
                assert!(
                    (c.midi[i] - whole.midi[j]).abs() < 0.25,
                    "midi frame {j}: {} vs {}",
                    c.midi[i],
                    whole.midi[j]
                );
            }
        }
    }

    #[test]
    fn phrase_windows_agree_with_whole_song() {
        check_spans(&song(), Voice::Alto);
    }

    /// A 0.35 s phrase gap puts the next phrase's lead (0.7 s) inside this
    /// phrase's short final notes; the span edge must not silence them.
    #[test]
    fn clipped_spans_keep_the_notes_before_a_short_gap() {
        let mut v = vec![
            note(0.5, 1.5, 50.0, &[Phoneme::Aa]),
            note(1.5, 1.7, 52.0, &[Phoneme::Aa]),
            note(1.7, 1.9, 53.0, &[Phoneme::Aa]),
            note(2.25, 3.0, 55.0, &[Phoneme::Aa]),
            note(3.35, 3.5, 57.0, &[Phoneme::T, Phoneme::Aa]),
            note(3.5, 3.62, 55.0, &[Phoneme::D, Phoneme::Aa, Phoneme::T]),
            note(3.97, 4.6, 52.0, &[Phoneme::S, Phoneme::Aa]),
        ];
        v[0].phrase_start = true;
        v[2].phrase_end = true;
        v[3].phrase_start = true;
        v[3].phrase_end = true;
        v[4].phrase_start = true;
        v[5].phrase_end = true;
        v[6].phrase_start = true;
        v[6].phrase_end = true;
        for voice in [Voice::Baritone, Voice::Soprano] {
            check_spans(&v, voice);
        }
    }

    /// Drift stays inside its 0.12 semitone limit.
    #[test]
    fn drift_is_bounded() {
        let mut m = vec![60.0f32; 100_000];
        add_drift(&mut m, &mut drift_walk(), &mut Rng::from_seed(3), 0.0);
        assert!(m.iter().all(|&x| (x - 60.0).abs() <= 0.1201));
        assert!(m.iter().any(|&x| (x - 60.0).abs() > 0.01));
    }
}

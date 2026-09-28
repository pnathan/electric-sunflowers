//! Song rendering: compose (`compose::prepare`), arrange
//! (`arrange::arrange`), then render the ten tracks (the four vocal tracks
//! in `vocals`, the six band tracks in `band`) into a `dsp::mix::Render`;
//! `mix` runs the mixer over it.
//!
//! `render_song` renders the tracks one after another and reports
//! progress; `render_song_threaded` renders them in parallel. Every track
//! draws only from its own streams and the choir sums its singers in plan
//! order on both paths, so the two give the same samples.

use compose::prepare::{prepare, Prepared};
use dsp::mix::{mix_song, Render, TrackSpec};
use sfcore::time::len_samples;
use sfcore::tuning::Tuning;
use song::{Song, Voice};

use crate::band::{self, BandTrack};
use crate::vocals;

/// A rendered song: its composition, its length in samples, and every
/// track loaded into a `dsp::mix::Render`.
pub struct RenderedSong {
    pub prepared: Prepared,
    pub len: usize,
    pub render: Render,
    /// With the `capture_raw` feature: a copy of each track's channels as
    /// loaded into `render` (after the body, before the channel strip), in
    /// mix order; a song without drums has no drums entry. Empty otherwise.
    pub raw_tracks: Vec<(&'static str, Vec<Vec<f32>>)>,
}

/// Progress callback: a stage label and the fraction done.
pub type ProgressFn<'a> = &'a mut dyn FnMut(&str, f64);

/// Mix key of a band track.
fn key(t: BandTrack) -> &'static str {
    match t {
        BandTrack::Guitar => "guitar",
        BandTrack::HarmonyGuitar => "hg",
        BandTrack::Bass => "bass",
        BandTrack::Drums => "drums",
        BandTrack::Harp => "harp",
        BandTrack::Violin => "violin",
    }
}

/// Renders `song` with `seed`, one track at a time. `voice` `None` uses the
/// song's voice. `progress` receives a label and a fraction before each
/// stage. `tuning` is unused (the instruments and the voice carry their
/// own settings); kept for the callers until the engine rewrite.
pub fn render_song(
    song: &Song,
    seed: u32,
    voice: Option<Voice>,
    tuning: &Tuning,
    progress: Option<ProgressFn<'_>>,
) -> RenderedSong {
    render_impl(song, seed, voice, tuning, progress, false)
}

/// `render_song` with the tracks rendered in parallel (rayon) and no
/// progress. Same samples as `render_song`.
pub fn render_song_threaded(song: &Song, seed: u32, voice: Option<Voice>, tuning: &Tuning) -> RenderedSong {
    render_impl(song, seed, voice, tuning, None, true)
}

/// The ten tracks in mix order, as (key, channels).
type Tracks = Vec<(&'static str, Vec<Vec<f32>>)>;

fn render_impl(
    song: &Song,
    seed: u32,
    voice: Option<Voice>,
    _tuning: &Tuning,
    mut progress: Option<ProgressFn<'_>>,
    threaded: bool,
) -> RenderedSong {
    sfcore::fp::flush_denormals();
    let mut step = |label: &str, frac: f64| {
        if let Some(p) = progress.as_deref_mut() {
            p(label, frac);
        }
    };

    let prepared = prepare(song, seed, voice);
    let len = len_samples(prepared.timeline.end);
    let seed = seed as u64;
    let arr = arrange::arrange(song, &prepared, seed);
    let v = &arr.vocals;

    let tracks: Tracks = if threaded {
        let (mut lead, mut harmony, mut doubles, mut choir) = (None, None, None, None);
        let mut band: [Option<Vec<Vec<f32>>>; 6] = Default::default();
        rayon::scope(|s| {
            s.spawn(|_| {
                sfcore::fp::flush_denormals();
                lead = Some(vocals::lead(v, seed, len));
            });
            s.spawn(|_| {
                sfcore::fp::flush_denormals();
                harmony = Some(vocals::harmony(v, seed, len));
            });
            s.spawn(|_| {
                sfcore::fp::flush_denormals();
                doubles = Some(vocals::doubles(v, seed, len));
            });
            s.spawn(|_| {
                sfcore::fp::flush_denormals();
                choir = Some(vocals::choir(v, seed, len, true));
            });
            for (slot, t) in band.iter_mut().zip(BandTrack::ALL) {
                let arr = &arr;
                s.spawn(move |_| {
                    sfcore::fp::flush_denormals();
                    *slot = band::render(t, arr, seed, len);
                });
            }
        });
        let [dl, dr] = doubles.unwrap_or_default();
        let [cl, cr] = choir.unwrap_or_default();
        let mut out: Tracks = vec![
            ("lead", vec![lead.unwrap_or_default()]),
            ("harmony", vec![harmony.unwrap_or_default()]),
            ("doubles", vec![dl, dr]),
            ("choir", vec![cl, cr]),
        ];
        for (t, chs) in BandTrack::ALL.into_iter().zip(band) {
            if let Some(chs) = chs {
                out.push((key(t), chs));
            }
        }
        out
    } else {
        let mut out: Tracks = Vec::with_capacity(10);
        step("Recording the lead vocal", 0.05);
        out.push(("lead", vec![vocals::lead(v, seed, len)]));
        step("Recording the harmony singer", 0.25);
        out.push(("harmony", vec![vocals::harmony(v, seed, len)]));
        step("Recording the doubled melody", 0.38);
        let [dl, dr] = vocals::doubles(v, seed, len);
        out.push(("doubles", vec![dl, dr]));
        step("Gathering the choir", 0.5);
        let [cl, cr] = vocals::choir(v, seed, len, false);
        out.push(("choir", vec![cl, cr]));
        for (t, label, frac) in [
            (BandTrack::Guitar, "Tracking the guitar", 0.72),
            (BandTrack::HarmonyGuitar, "Tracking the harmony guitar", 0.76),
            (BandTrack::Bass, "Tracking bass and drums", 0.8),
            (BandTrack::Drums, "Tracking bass and drums", 0.83),
            (BandTrack::Harp, "Tracking harp and strings", 0.86),
            (BandTrack::Violin, "Tracking harp and strings", 0.9),
        ] {
            step(label, frac);
            if let Some(chs) = band::render(t, &arr, seed, len) {
                out.push((key(t), chs));
            }
        }
        step("Mixing", 0.94);
        out
    };

    #[cfg(feature = "capture_raw")]
    let raw_tracks = tracks.clone();
    #[cfg(not(feature = "capture_raw"))]
    let raw_tracks = Vec::new();

    let mut render = Render::new(len);
    for (k, chs) in tracks {
        render.set_track(k, chs);
    }
    RenderedSong { prepared, len, render, raw_tracks }
}

/// Mixes the tracks `enabled` selects; returns (left, right).
pub fn mix(rendered: &mut RenderedSong, enabled: impl Fn(&TrackSpec) -> bool, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let result = mix_song(&mut rendered.render, enabled, seed as i64, None);
    (result.l, result.r)
}

/// `mix` with the per-track channel strips run in parallel. Same samples.
pub fn mix_threaded(rendered: &mut RenderedSong, enabled: impl Fn(&TrackSpec) -> bool, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let result = dsp::mix::mix_song_threaded(&mut rendered.render, enabled, seed as i64);
    (result.l, result.r)
}

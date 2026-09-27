//! `renderSong` and the entry point into the mix (engine.js ~867-952,
//! 1179-1216). Assembles the four vocal tracks (`vocals.rs`), the six band
//! tracks and the body-convolution pass (`band.rs`) into a `dsp::mix::Render`,
//! then exposes `mix` over `dsp::mix::mix_song`.

use compose::prepare::{prepare, Prepared};
use compose::song::Song;
use compose::voices::Voice;
use dsp::mix::{mix_song, Render, TrackSpec};
use sfcore::tuning::Tuning;
use sfcore::SR_F;

use crate::band;
use crate::vocals;

/// `renderSong`'s return value: the prepared song data (form/timeline/melody),
/// the sample length, and every track loaded into a `dsp::mix::Render`.
pub struct RenderedSong {
    pub prepared: Prepared,
    pub len: usize,
    pub render: Render,
    /// A copy of each raw track's channels exactly as `renderSong`'s
    /// `tracks[key]` holds it (post body-convolution, pre `processTrack`),
    /// in `TRACKS` order. `dsp::mix::Render` keeps the same data privately
    /// (and consumes it as tracks are processed), so this is the only way
    /// to inspect a track's raw signal, e.g. for parity comparison.
    pub raw_tracks: Vec<(&'static str, Vec<Vec<f32>>)>,
}

/// `renderSong(song,seed,voiceKey,progress)`, with the async progress ticks
/// dropped (this is a synchronous, single-threaded port; see CLAUDE.md).
/// `voice` is JS's `voiceKey`: `None` is `'auto'` (use the song's own voice).
/// Sequential path: every track is rendered one at a time, in the same
/// order `render_song_threaded` uses for its final sum, so the two are
/// bit-identical (see `crates/engine/tests/threaded_parity.rs`).
pub fn render_song(
    song: &Song,
    seed: u32,
    voice: Option<Voice>,
    tuning: &Tuning,
    progress: Option<&mut dyn FnMut(&str, f64)>,
) -> RenderedSong {
    render_song_impl(song, seed, voice, tuning, progress, false)
}

/// Same as `render_song`, but the ten tracks are rendered on separate
/// threads (via `std::thread::scope`) since every one of them draws from
/// its own independent rng stream (the one exception, the choir's shared
/// `'choirv'` stream, is handled by `vocals::render_choir_threaded`, which
/// still draws that stream sequentially and only parallelizes the
/// per-singer `render_voice` calls that follow it). The four body
/// convolutions (guitar/hg/harp/violin) also run in parallel: each is a
/// pure function of its own already-rendered track and touches no shared
/// state. No progress callback: a `&mut dyn FnMut` cannot be shared across
/// threads, and per-track ticks would arrive out of order anyway.
pub fn render_song_threaded(song: &Song, seed: u32, voice: Option<Voice>, tuning: &Tuning) -> RenderedSong {
    render_song_impl(song, seed, voice, tuning, None, true)
}

fn render_song_impl(
    song: &Song,
    seed: u32,
    voice: Option<Voice>,
    tuning: &Tuning,
    mut progress: Option<&mut dyn FnMut(&str, f64)>,
    threaded: bool,
) -> RenderedSong {
    macro_rules! step {
        ($label:expr, $frac:expr) => {
            if let Some(p) = progress.as_deref_mut() {
                p($label, $frac);
            }
        };
    }

    let prepared = prepare(song, seed, voice);
    let len = (prepared.timeline.end * SR_F).ceil() as usize;
    let form = &prepared.form;
    let tl = &prepared.timeline;

    let (lead, harmony, doubles, choir, guitar, bass, drums, harp, violin, hg);
    if threaded {
        let mut lead_s = None;
        let mut harmony_s = None;
        let mut doubles_s = None;
        let mut choir_s = None;
        let mut guitar_s = None;
        let mut bass_s = None;
        let mut drums_s = None;
        let mut harp_s = None;
        let mut vhg_s = None;
        std::thread::scope(|s| {
            s.spawn(|| lead_s = Some(vocals::render_lead(&prepared, seed, len, tuning)));
            s.spawn(|| harmony_s = Some(vocals::render_harmony(&prepared, song, seed, len, tuning)));
            s.spawn(|| doubles_s = Some(vocals::render_doubles(&prepared, seed, len, tuning)));
            s.spawn(|| choir_s = Some(vocals::render_choir_threaded(&prepared, seed, len, tuning)));
            s.spawn(|| guitar_s = Some(band::render_guitar(song, form, tl, seed, tuning)));
            s.spawn(|| bass_s = Some(band::render_bass(song, form, tl, seed)));
            s.spawn(|| drums_s = Some(band::render_drums(song, form, tl, seed)));
            s.spawn(|| harp_s = Some(band::render_harp(song, form, tl, seed)));
            s.spawn(|| {
                let bl = song.break_lead.map(|b| b.as_str());
                vhg_s = Some(band::render_violin_and_harmony_guitar(&prepared, song, seed, len, bl));
            });
        });
        lead = lead_s.unwrap();
        harmony = harmony_s.unwrap();
        doubles = doubles_s.unwrap();
        choir = choir_s.unwrap();
        guitar = guitar_s.unwrap();
        bass = bass_s.unwrap();
        drums = drums_s.unwrap();
        harp = harp_s.unwrap();
        let (v, h) = vhg_s.unwrap();
        violin = v;
        hg = h;
    } else {
        step!("Recording the lead vocal", 0.05);
        lead = vocals::render_lead(&prepared, seed, len, tuning);

        step!("Recording the harmony singer", 0.25);
        harmony = vocals::render_harmony(&prepared, song, seed, len, tuning);

        step!("Recording the doubled melody", 0.38);
        doubles = vocals::render_doubles(&prepared, seed, len, tuning);

        step!("Gathering the choir", 0.5);
        choir = vocals::render_choir(&prepared, seed, len, tuning);

        step!("Tracking the guitar", 0.72);
        guitar = band::render_guitar(song, form, tl, seed, tuning);

        step!("Tracking bass and drums", 0.8);
        bass = band::render_bass(song, form, tl, seed);
        drums = band::render_drums(song, form, tl, seed);

        step!("Tracking harp and strings", 0.86);
        harp = band::render_harp(song, form, tl, seed);
        let bl = song.break_lead.map(|b| b.as_str());
        let (v, h) = band::render_violin_and_harmony_guitar(&prepared, song, seed, len, bl);
        violin = v;
        hg = h;
    }

    step!("Mixing", 0.94);
    step!("Resonating the instrument bodies", 0.93);

    // `for(const k in BODY_OF){...}` (engine.js ~945-949), in BODY_OF's
    // declared order: guitar, hg, harp, violin. Each convolution is a pure
    // function of its own already-rendered track, so the threaded path
    // runs all four on separate threads; the result is identical either
    // way (no shared state, no summation to reorder).
    let mut guitar = guitar;
    let mut hg_track = vec![hg];
    let mut harp = harp;
    let mut violin_track = vec![violin];
    if threaded {
        let (mut g, mut h, mut hp, mut v) = (None, None, None, None);
        std::thread::scope(|s| {
            s.spawn(|| g = band::apply_body("guitar", seed, len, &guitar[0]));
            s.spawn(|| h = band::apply_body("hg", seed, len, &hg_track[0]));
            s.spawn(|| hp = band::apply_body("harp", seed, len, &harp[0]));
            s.spawn(|| v = band::apply_body("violin", seed, len, &violin_track[0]));
        });
        if let Some([l, r]) = g {
            guitar = vec![l, r];
        }
        if let Some([l, r]) = h {
            hg_track = vec![l, r];
        }
        if let Some([l, r]) = hp {
            harp = vec![l, r];
        }
        if let Some([l, r]) = v {
            violin_track = vec![l, r];
        }
    } else {
        if let Some([l, r]) = band::apply_body("guitar", seed, len, &guitar[0]) {
            guitar = vec![l, r];
        }
        if let Some([l, r]) = band::apply_body("hg", seed, len, &hg_track[0]) {
            hg_track = vec![l, r];
        }
        if let Some([l, r]) = band::apply_body("harp", seed, len, &harp[0]) {
            harp = vec![l, r];
        }
        if let Some([l, r]) = band::apply_body("violin", seed, len, &violin_track[0]) {
            violin_track = vec![l, r];
        }
    }

    // See the `capture_raw` feature doc in crates/engine/Cargo.toml: only
    // tests read `raw_tracks`, so cloning every track (~0.5 GB at the demo's
    // length) is dead weight in the real sunflower build and is skipped
    // there. Output is unaffected either way; this only changes whether a
    // second, unused copy of each track is briefly allocated.
    #[cfg(feature = "capture_raw")]
    let raw_tracks: Vec<(&'static str, Vec<Vec<f32>>)> = vec![
        ("lead", lead.clone()),
        ("harmony", harmony.clone()),
        ("doubles", doubles.clone()),
        ("choir", choir.clone()),
        ("guitar", guitar.clone()),
        ("hg", hg_track.clone()),
        ("bass", bass.clone()),
        ("drums", drums.clone()),
        ("harp", harp.clone()),
        ("violin", violin_track.clone()),
    ];
    #[cfg(not(feature = "capture_raw"))]
    let raw_tracks: Vec<(&'static str, Vec<Vec<f32>>)> = Vec::new();

    let mut render = Render::new(len);
    render.set_track("lead", lead);
    render.set_track("harmony", harmony);
    render.set_track("doubles", doubles);
    render.set_track("choir", choir);
    render.set_track("guitar", guitar);
    render.set_track("hg", hg_track);
    render.set_track("bass", bass);
    render.set_track("drums", drums);
    render.set_track("harp", harp);
    render.set_track("violin", violin_track);

    RenderedSong { prepared, len, render, raw_tracks }
}

/// `mixSong(render,enabled,seed)`, returning the stereo `(L,R)` buffers.
pub fn mix(rendered: &mut RenderedSong, enabled: impl Fn(&TrackSpec) -> bool, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let result = mix_song(&mut rendered.render, enabled, seed as i64, None);
    (result.l, result.r)
}

/// Same as `mix`, but runs `dsp::mix::mix_song_threaded` (parallel
/// per-track EQ/gain/compression, summed in `TRACKS` order as usual): see
/// its doc comment. Bit-identical to `mix`.
pub fn mix_threaded(rendered: &mut RenderedSong, enabled: impl Fn(&TrackSpec) -> bool, seed: u32) -> (Vec<f32>, Vec<f32>) {
    let result = dsp::mix::mix_song_threaded(&mut rendered.render, enabled, seed as i64);
    (result.l, result.r)
}

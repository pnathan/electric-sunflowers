//! Song rendering (design section 3.7): the arranger (`arrange_song`:
//! `compose::prepare`, then `arrange::arrange`, giving a `Performance`), then
//! the player (`play`): one `rayon::scope` of tasks (14 in a
//! solo song; a duet adds a lead B task and two more doubles takes):
//!
//! - voice: lead; lead B (a duet only); harmony; doubles takes (2 in a solo
//!   song, A's two then B's two in a duet); choir parts in plan order
//!   (bass, tenor, alto, soprano), each rendering its singers in plan order
//!   into a part stem;
//! - band: guitar; harmony guitar; bass; drums; harp; violin.
//!
//! Each task renders its instrument, convolves with its body (if any), runs
//! its channel strip (`strip::run_strip`) and stores the `ProcessedStem`.
//! The doubles stem is every take summed in index order and the choir stem
//! is the sum of the four part stems in part order; the task that finishes
//! last of its group does the sum and runs the strip.
//!
//! Determinism: every random stream derives from (seed, tag, index), no
//! stream is shared between tasks, and every sum has a fixed order, so the
//! stems are bit-identical at any thread count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use arrange::Arrangement;
use compose::prepare::{prepare_voices, Prepared, VoiceChoice};
use serde::{Deserialize, Serialize};
use sfcore::time::len_samples;
use song::events::Singer;
use song::{Band, Song, Voice};

use crate::band;
use crate::stem::{SparseBuf, Stem};
use crate::strip::{run_strip, ProcessedStem};
use crate::track::{TrackId, N_TRACKS};
use crate::vocals::{self, choir_index, singer_seed};

/// Tasks in a solo render (band 6, lead, harmony, 2 doubles, 4 choir
/// parts). A duet adds a lead B task and 2 more doubles takes.
pub const TASKS: usize = 14;

/// Render progress: called once per finished task with the count of
/// finished tasks and `TASKS`. Called from worker threads.
pub trait Progress: Sync {
    fn advance(&self, done: usize, total: usize);
}

/// Progress that reports nothing.
pub struct NoProgress;

impl Progress for NoProgress {
    fn advance(&self, _done: usize, _total: usize) {}
}

/// The processed tracks of one render: the cache the mixer sums, so band
/// toggles are a re-mix only. Each track's slapback (if its strip has one)
/// is in its `ProcessedStem.slap`.
#[derive(Clone, Debug)]
pub struct Stems {
    pub len: usize,
    /// Indexed by `TrackId as usize`; `None` for a silent or absent track.
    pub tracks: [Option<ProcessedStem>; N_TRACKS],
}

impl Stems {
    pub fn get(&self, id: TrackId) -> Option<&ProcessedStem> {
        self.tracks[id.index()].as_ref()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the tasks store their results.
struct Store {
    tracks: Mutex<[Option<ProcessedStem>; N_TRACKS]>,
}

/// Gain on a choir stem that sings word lines: the lead strip gain over
/// the choir strip gain (1.25 / 0.36, +10.8 dB), so sung words sit level
/// with the lead. The owner asked for par by ear.
const CHOIR_WORDS_LIFT: f32 = 3.472_222;

impl Store {
    /// Runs `id`'s strip over `audio` and keeps the result.
    fn put(&self, id: TrackId, audio: Stem) {
        self.put_keyed(id, audio, Vec::new());
    }

    /// `put`, with the sample spans where the stem keys the ducker.
    fn put_keyed(&self, id: TrackId, audio: Stem, key: Vec<(usize, usize)>) {
        if audio.is_silent() {
            return;
        }
        let Some(mut stem) = run_strip(id.strip(), audio) else {
            return;
        };
        if !key.is_empty() {
            stem.level *= CHOIR_WORDS_LIFT;
        }
        stem.key = key;
        lock(&self.tracks)[id.index()] = Some(stem);
    }
}

/// Stereo part stems that are summed, in index order, once all are in.
struct Joint<const N: usize> {
    parts: Mutex<[Option<[SparseBuf; 2]>; N]>,
}

impl<const N: usize> Joint<N> {
    fn new() -> Self {
        Joint {
            parts: Mutex::new(std::array::from_fn(|_| None)),
        }
    }

    /// Stores part `i`; returns the sum of all parts when this was the last.
    fn deposit(&self, i: usize, part: [SparseBuf; 2]) -> Option<[SparseBuf; 2]> {
        let mut g = lock(&self.parts);
        g[i] = Some(part);
        if g.iter().any(Option::is_none) {
            return None;
        }
        let mut it = g.iter_mut().filter_map(Option::take);
        let mut acc = it.next()?;
        for p in it {
            acc[0].add(&p[0]);
            acc[1].add(&p[1]);
        }
        Some(acc)
    }
}

/// Like `Joint`, sized at runtime: the doubles takes are 2 in a solo song,
/// 4 in a duet (A's two takes, then B's two).
struct VecJoint {
    parts: Mutex<Vec<Option<[SparseBuf; 2]>>>,
}

impl VecJoint {
    fn new(n: usize) -> Self {
        VecJoint {
            parts: Mutex::new((0..n).map(|_| None).collect()),
        }
    }

    /// Stores part `i`; returns the sum of all parts when this was the last.
    fn deposit(&self, i: usize, part: [SparseBuf; 2]) -> Option<[SparseBuf; 2]> {
        let mut g = lock(&self.parts);
        g[i] = Some(part);
        if g.iter().any(Option::is_none) {
            return None;
        }
        let mut it = g.iter_mut().filter_map(Option::take);
        let mut acc = it.next()?;
        for p in it {
            acc[0].add(&p[0]);
            acc[1].add(&p[1]);
        }
        Some(acc)
    }
}

/// The version of the `Performance` file format.
pub const PERFORMANCE_VERSION: u32 = 1;

/// Everything the player reads: the arranger's output, in a form that
/// serialises to JSON and back exactly. Version 1; a change to any field
/// or to an event type raises `PERFORMANCE_VERSION`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Performance {
    pub version: u32,
    /// The song seed; keys every random stream of the player.
    pub seed: u64,
    /// End of the timeline, seconds; sets the length of every stem.
    pub end: f64,
    /// Sample spans where the choir's word lines key the ducker; empty
    /// when the song has none.
    pub choir_key: Vec<(usize, usize)>,
    /// Which band parts the song plays; the mixer gates stems with it.
    /// The player does not read it.
    pub band: Band,
    pub arrangement: Arrangement,
}

/// Sample spans of the choir's word lines, one per line: from the line's
/// first note to its last. Empty when the song has no choir line, so the
/// ducker keys on the leads alone.
pub(crate) fn choir_line_spans(p: &Prepared, words: &[usize]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for (li, l) in p.form.lines.iter().enumerate() {
        if !l.part.is_choir() && !words.contains(&l.sec) {
            continue;
        }
        let mut it = p.comp.lead.iter().filter(|n| n.line_idx == li);
        let Some(first) = it.next() else { continue };
        let (mut t0, mut t1) = (first.t0, first.t1);
        for n in it {
            t0 = t0.min(n.t0);
            t1 = t1.max(n.t1);
        }
        spans.push((len_samples(t0), len_samples(t1)));
    }
    spans
}

/// Renders `song` with `seed`. `voice` `None` uses the song's voice. A thin
/// shim over `render_with` with `VoiceChoice { a: voice, b: None }`.
pub fn render(
    song: &Song,
    seed: u64,
    voice: Option<Voice>,
    progress: &dyn Progress,
) -> (Prepared, Stems) {
    render_with(song, seed, VoiceChoice { a: voice, b: None }, progress)
}

/// Renders `song` with `seed`, choosing both singers' voices (`voice.b` is
/// ignored outside a duet; see `VoiceChoice`). The render's primary entry;
/// `render` is a shim over this for the common one-voice case. Arranges
/// (`arrange_song`), then plays the performance (`play`).
pub fn render_with(
    song: &Song,
    seed: u64,
    voice: VoiceChoice,
    progress: &dyn Progress,
) -> (Prepared, Stems) {
    let (prepared, perf) = arrange_song(song, seed, voice);
    let stems = play(&perf, progress);
    (prepared, stems)
}

/// The arranger: composes `song` and plans every part. Returns the
/// composition (for the sheet and the lyrics) and the `Performance` the
/// player reads.
pub fn arrange_song(song: &Song, seed: u64, voice: VoiceChoice) -> (Prepared, Performance) {
    sfcore::fp::flush_denormals();
    let prepared = prepare_voices(song, seed, voice);
    let arrangement = arrange::arrange(song, &prepared, seed);
    let perf = Performance {
        version: PERFORMANCE_VERSION,
        seed,
        end: prepared.timeline.end,
        choir_key: choir_line_spans(&prepared, &[]),
        band: song.band,
        arrangement,
    };
    (prepared, perf)
}

/// The player: renders a `Performance` into processed stems. Reads nothing
/// else; the same performance always gives the same stems.
pub fn play(perf: &Performance, progress: &dyn Progress) -> Stems {
    sfcore::fp::flush_denormals();
    let seed = perf.seed;
    let len = len_samples(perf.end);
    let arr = &perf.arrangement;
    let v = &arr.vocals;
    let choir_key = &perf.choir_key;

    // 6 band tasks, lead, harmony, lead B (a duet only), each doubles take,
    // each choir part: 14 in a solo song, as `TASKS` documents.
    let tasks = 6 + 2 + usize::from(v.lead_b.is_some()) + v.doubles.len() + v.choir.len();
    debug_assert!(v.lead_b.is_some() || tasks == TASKS);

    let results = Store {
        tracks: Mutex::new(Default::default()),
    };
    let (doubles, choir, done) = (
        VecJoint::new(v.doubles.len()),
        Joint::<4>::new(),
        AtomicUsize::new(0),
    );
    let (store, doubles, choir, done) = (&results, &doubles, &choir, &done);
    let finish = move || progress.advance(done.fetch_add(1, Ordering::Relaxed) + 1, tasks);

    rayon::scope(|s| {
        // Longest tasks first: the guitar (render plus body) is the critical path.
        for id in [
            TrackId::Guitar,
            TrackId::Violin,
            TrackId::Harp,
            TrackId::HarmonyGuitar,
            TrackId::Bass,
            TrackId::Drums,
        ] {
            s.spawn(move |_| {
                sfcore::fp::flush_denormals();
                if let Some(stem) = band::render(id, arr, seed, len) {
                    store.put(id, stem);
                }
                finish();
            });
        }
        let mut leads: Vec<(TrackId, &Singer, u64)> = vec![
            (TrackId::Lead, &v.lead, vocals::LEAD),
            (TrackId::Harmony, &v.harmony, vocals::HARMONY),
        ];
        if let Some(lb) = &v.lead_b {
            leads.push((TrackId::LeadB, lb, vocals::LEAD_B));
        }
        for (id, singer, k) in leads {
            s.spawn(move |_| {
                sfcore::fp::flush_denormals();
                let mut buf = SparseBuf::new(len);
                vocals::mono_into(singer, singer_seed(seed, k), &mut buf);
                store.put(id, Stem::Mono(buf));
                finish();
            });
        }
        for (i, singer) in v.doubles.iter().enumerate() {
            s.spawn(move |_| {
                sfcore::fp::flush_denormals();
                let mut take = [SparseBuf::new(len), SparseBuf::new(len)];
                vocals::panned_into(
                    singer,
                    singer_seed(seed, vocals::DOUBLES + i as u64),
                    &mut take,
                );
                if let Some(sum) = doubles.deposit(i, take) {
                    store.put(TrackId::Doubles, Stem::Stereo(sum));
                }
                finish();
            });
        }
        for (p, part) in v.choir.iter().enumerate() {
            s.spawn(move |_| {
                sfcore::fp::flush_denormals();
                let mut stem = [SparseBuf::new(len), SparseBuf::new(len)];
                for (i, singer) in part.iter().enumerate() {
                    vocals::panned_into(singer, singer_seed(seed, choir_index(p, i)), &mut stem);
                }
                if let Some(sum) = choir.deposit(p, stem) {
                    store.put_keyed(TrackId::Choir, Stem::Stereo(sum), choir_key.clone());
                }
                finish();
            });
        }
    });

    let Store { tracks } = results;
    let tracks = tracks.into_inner().unwrap_or_else(|e| e.into_inner());
    Stems { len, tracks }
}

//! Song rendering (design section 3.7): compose (`compose::prepare`),
//! arrange (`arrange::arrange`), then one `rayon::scope` of 14 tasks:
//!
//! - voice: lead; harmony; doubles take 1; doubles take 2; choir parts in
//!   plan order (bass, tenor, alto, soprano), each rendering its singers in
//!   plan order into a part stem;
//! - band: guitar; harmony guitar; bass; drums; harp; violin.
//!
//! Each task renders its instrument, convolves with its body (if any), runs
//! its channel strip (`strip::run_strip`) and stores the `ProcessedStem`.
//! The doubles stem is take 1 plus take 2 and the choir stem is the sum of
//! the four part stems in part order; the task that finishes last of its
//! group does the sum and runs the strip.
//!
//! Determinism: every random stream derives from (seed, tag, index), no
//! stream is shared between tasks, and every sum has a fixed order, so the
//! stems are bit-identical at any thread count.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use compose::prepare::{prepare, Prepared};
use sfcore::time::len_samples;
use song::{Song, Voice};

use crate::band;
use crate::stem::{SparseBuf, Stem};
use crate::strip::{run_strip, ProcessedStem};
use crate::track::{TrackId, N_TRACKS};
use crate::vocals::{self, choir_index, singer_seed};

/// Tasks in one render.
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
/// toggles are a re-mix only.
#[derive(Clone, Debug)]
pub struct Stems {
    pub len: usize,
    /// Indexed by `TrackId as usize`; `None` for a silent or absent track.
    pub tracks: [Option<ProcessedStem>; N_TRACKS],
    /// The lead's slapback, level applied; `None` without a lead.
    pub slapback: Option<SparseBuf>,
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
    slapback: Mutex<Option<SparseBuf>>,
}

impl Store {
    /// Runs `id`'s strip over `audio` and keeps the result.
    fn put(&self, id: TrackId, audio: Stem) {
        if audio.is_silent() {
            return;
        }
        let Some((stem, slap)) = run_strip(id.strip(), audio) else { return };
        lock(&self.tracks)[id.index()] = Some(stem);
        if slap.is_some() {
            *lock(&self.slapback) = slap;
        }
    }
}

/// Stereo part stems that are summed, in index order, once all are in.
struct Joint<const N: usize> {
    parts: Mutex<[Option<[SparseBuf; 2]>; N]>,
}

impl<const N: usize> Joint<N> {
    fn new() -> Self {
        Joint { parts: Mutex::new(std::array::from_fn(|_| None)) }
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

/// Renders `song` with `seed`. `voice` `None` uses the song's voice.
pub fn render(song: &Song, seed: u64, voice: Option<Voice>, progress: &dyn Progress) -> (Prepared, Stems) {
    sfcore::fp::flush_denormals();
    let prepared = prepare(song, seed, voice);
    let len = len_samples(prepared.timeline.end);
    let arr = arrange::arrange(song, &prepared, seed);
    let v = &arr.vocals;

    let results = Store { tracks: Mutex::new(Default::default()), slapback: Mutex::new(None) };
    let (doubles, choir, done) = (Joint::<2>::new(), Joint::<4>::new(), AtomicUsize::new(0));
    let (store, doubles, choir, done, arr) = (&results, &doubles, &choir, &done, &arr);
    let finish = move || progress.advance(done.fetch_add(1, Ordering::Relaxed) + 1, TASKS);

    rayon::scope(|s| {
        // Longest tasks first: the guitar (render plus body) is the critical path.
        for id in [TrackId::Guitar, TrackId::Violin, TrackId::Harp, TrackId::HarmonyGuitar, TrackId::Bass, TrackId::Drums] {
            s.spawn(move |_| {
                sfcore::fp::flush_denormals();
                if let Some(stem) = band::render(id, arr, seed, len) {
                    store.put(id, stem);
                }
                finish();
            });
        }
        for (id, singer, k) in [(TrackId::Lead, &v.lead, vocals::LEAD), (TrackId::Harmony, &v.harmony, vocals::HARMONY)] {
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
                vocals::panned_into(singer, singer_seed(seed, vocals::DOUBLES + i as u64), &mut take);
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
                    store.put(TrackId::Choir, Stem::Stereo(sum));
                }
                finish();
            });
        }
    });

    let Store { tracks, slapback } = results;
    let tracks = tracks.into_inner().unwrap_or_else(|e| e.into_inner());
    let slapback = slapback.into_inner().unwrap_or_else(|e| e.into_inner());
    (prepared, Stems { len, tracks, slapback })
}

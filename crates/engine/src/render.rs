//! Song rendering (design section 3.7): compose (`compose::prepare`),
//! arrange (`arrange::arrange`), then one `rayon::scope` of tasks (14 in a
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

use compose::prepare::{prepare_voices, Prepared, VoiceChoice};
use sfcore::time::len_samples;
use song::events::Singer;
use song::{Song, Voice};

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

impl Store {
    /// Runs `id`'s strip over `audio` and keeps the result.
    fn put(&self, id: TrackId, audio: Stem) {
        if audio.is_silent() {
            return;
        }
        let Some(stem) = run_strip(id.strip(), audio) else { return };
        lock(&self.tracks)[id.index()] = Some(stem);
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

/// Like `Joint`, sized at runtime: the doubles takes are 2 in a solo song,
/// 4 in a duet (A's two takes, then B's two).
struct VecJoint {
    parts: Mutex<Vec<Option<[SparseBuf; 2]>>>,
}

impl VecJoint {
    fn new(n: usize) -> Self {
        VecJoint { parts: Mutex::new((0..n).map(|_| None).collect()) }
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

/// Renders `song` with `seed`. `voice` `None` uses the song's voice. A thin
/// shim over `render_with` with `VoiceChoice { a: voice, b: None }`.
pub fn render(song: &Song, seed: u64, voice: Option<Voice>, progress: &dyn Progress) -> (Prepared, Stems) {
    render_with(song, seed, VoiceChoice { a: voice, b: None }, progress)
}

/// Renders `song` with `seed`, choosing both singers' voices (`voice.b` is
/// ignored outside a duet; see `VoiceChoice`). The render's primary entry;
/// `render` is a shim over this for the common one-voice case.
pub fn render_with(song: &Song, seed: u64, voice: VoiceChoice, progress: &dyn Progress) -> (Prepared, Stems) {
    sfcore::fp::flush_denormals();
    let prepared = prepare_voices(song, seed, voice);
    let len = len_samples(prepared.timeline.end);
    let arr = arrange::arrange(song, &prepared, seed);
    let v = &arr.vocals;

    // 6 band tasks, lead, harmony, lead B (a duet only), each doubles take,
    // each choir part: 14 in a solo song, as `TASKS` documents.
    let tasks = 6 + 2 + usize::from(v.lead_b.is_some()) + v.doubles.len() + v.choir.len();
    debug_assert!(v.lead_b.is_some() || tasks == TASKS);

    let results = Store { tracks: Mutex::new(Default::default()) };
    let (doubles, choir, done) = (VecJoint::new(v.doubles.len()), Joint::<4>::new(), AtomicUsize::new(0));
    let (store, doubles, choir, done, arr) = (&results, &doubles, &choir, &done, &arr);
    let finish = move || progress.advance(done.fetch_add(1, Ordering::Relaxed) + 1, tasks);

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
        let mut leads: Vec<(TrackId, &Singer, u64)> = vec![(TrackId::Lead, &v.lead, vocals::LEAD), (TrackId::Harmony, &v.harmony, vocals::HARMONY)];
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

    let Store { tracks } = results;
    let tracks = tracks.into_inner().unwrap_or_else(|e| e.into_inner());
    (prepared, Stems { len, tracks })
}

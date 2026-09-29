//! Duet wiring (design sections 4.6): `TrackId::LeadB`, the render task
//! graph's optional lead B task, the sheet's singer markers, and the solo
//! song's bit-for-bit invariance (this wave must not move a single sample
//! of a solo render or mix).

use compose::prepare::VoiceChoice;
use engine::{
    demo_duet_song, demo_song, mix, render, render_with, song_sheet_with, NoProgress, TrackId,
};

/// FNV-1a (64-bit): a small, dependency-free content hash (no sha2 crate is
/// vendored; see CLAUDE.md and `docs/notes/features-2-wave1.md`), used only
/// to catch an accidental change to the solo demo mix, not as a
/// cryptographic hash.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn mix_bytes(l: &[f32], r: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity((l.len() + r.len()) * 4);
    for &v in l.iter().chain(r) {
        out.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    out
}

/// The demo duet fixture normalises with zero repairs and is a duet.
#[test]
fn demo_duet_normalises_with_zero_repairs() {
    let raw: serde_json::Value =
        serde_json::from_str(engine::DEMO_DUET_JSON).expect("demo_duet.json is JSON");
    let (song, repairs) = song::normalize_value(&raw).expect("demo_duet.json normalises");
    assert!(repairs.is_empty(), "{repairs:?}");
    assert!(song.is_duet());
    // demo_duet_song() builds the same song the same way.
    assert_eq!(demo_duet_song().title, song.title);
}

/// `Vocals.lead_b` (via the render's arrangement) is present iff the song
/// is a duet.
#[test]
fn lead_b_present_iff_duet() {
    let solo = demo_song();
    let p = compose::prepare::prepare(solo, 1, None);
    let a = arrange::arrange(solo, &p, 1);
    assert!(a.vocals.lead_b.is_none());

    let duet = demo_duet_song();
    let p = compose::prepare::prepare_voices(duet, 1, VoiceChoice::default());
    let a = arrange::arrange(duet, &p, 1);
    assert!(a.vocals.lead_b.is_some());
}

/// Every singer's arranged notes are sorted by onset and never overlap
/// (lines never overlap, so this holds for the combined melody-plus-second
/// notes of each singer too).
#[test]
fn singers_notes_are_sorted_and_non_overlapping() {
    let duet = demo_duet_song();
    for seed in [1u64, 2, 3] {
        let p = compose::prepare::prepare_voices(duet, seed, VoiceChoice::default());
        let a = arrange::arrange(duet, &p, seed);
        let v = &a.vocals;
        for singer in std::iter::once(&v.lead)
            .chain(v.lead_b.iter())
            .chain(std::iter::once(&v.harmony))
        {
            for w in singer.notes.windows(2) {
                assert!(w[0].t0 <= w[1].t0, "notes out of order at seed {seed}");
                assert!(w[0].t1 <= w[1].t0 + 1e-6, "notes overlap at seed {seed}");
            }
        }
    }
}

/// A duet render produces a non-silent `LeadB` stem; a solo render has
/// none.
#[test]
fn lead_b_stem_present_only_in_a_duet() {
    let (_, stems) = render(demo_song(), 1234, None, &NoProgress);
    assert!(
        stems.get(TrackId::LeadB).is_none(),
        "solo demo has a lead_b stem"
    );

    let (_, stems) = render_with(demo_duet_song(), 1234, VoiceChoice::default(), &NoProgress);
    let lb = stems
        .get(TrackId::LeadB)
        .expect("duet demo has a lead_b stem");
    let audible = match &lb.audio {
        engine::Stem::Mono(x) => x.to_dense(),
        engine::Stem::Stereo([l, _]) => l.to_dense(),
    };
    assert!(
        audible.iter().any(|&v| v.abs() > 1e-6),
        "lead_b stem is silent"
    );
}

/// The duet demo sheet has `voice_b` and at least one shared or B-only
/// line; the solo demo sheet has neither.
#[test]
fn sheet_marks_singers_only_in_a_duet() {
    let solo_sheet = song_sheet_with(demo_song(), 1, VoiceChoice::default());
    assert!(solo_sheet.voice_b.is_none());
    for l in solo_sheet.sections.iter().flat_map(|s| s.lines.iter()) {
        if let Some(s) = &l.singer {
            assert_eq!(s.part, "A");
        }
    }

    let duet_sheet = song_sheet_with(demo_duet_song(), 1, VoiceChoice::default());
    assert!(duet_sheet.voice_b.is_some());
    let parts: Vec<&str> = duet_sheet
        .sections
        .iter()
        .flat_map(|s| s.lines.iter())
        .filter_map(|l| l.singer.as_ref())
        .map(|s| s.part.as_str())
        .collect();
    assert!(parts.iter().any(|&p| p == "both" || p == "B"), "{parts:?}");
}

/// The solo demo's mixed samples are bit-for-bit unchanged by this wave's
/// duet wiring: an FNV-1a64 checksum over the mix's `f32` bit patterns.
/// `0x1c28a9a15f79a15b` was captured from HEAD (`e7647d2a`) via a `git
/// worktree` checkout, before this wave's arrange/engine edits
/// (`arrange::vocals::plan`, `engine::render`/`render_with`,
/// `mixset::default_for`) landed, with a throwaway example running the
/// same render-then-mix call over `demo_song()` at seed 1234 (built, run,
/// then removed with the worktree).
#[test]
fn solo_demo_mix_is_bit_identical() {
    let song = demo_song();
    let (_, stems) = render(song, 1234, None, &NoProgress);
    let m = mix(&stems, &song.band, 1234);
    let h = fnv1a(&mix_bytes(&m.l, &m.r));
    assert_eq!(
        h, 0x1c28a9a15f79a15b,
        "solo demo mix checksum changed: {h:#x}"
    );
}

/// A duet render is thread-count invariant, the same guarantee the solo
/// demo already has (`render_test::one_and_eight_threads_are_bit_identical`):
/// one thread and the default pool must produce the same mix bits.
#[test]
fn duet_render_is_thread_count_invariant() {
    fn pool(n: usize) -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .start_handler(|_| sfcore::fp::flush_denormals())
            .build()
            .expect("thread pool")
    }
    let song = demo_duet_song();
    let render_mix = |s: &song::Song| {
        let (_, stems) = render_with(s, 7, VoiceChoice::default(), &NoProgress);
        mix(&stems, &s.band, 7)
    };
    let a = pool(1).install(|| render_mix(&song));
    let b = pool(8).install(|| render_mix(&song));
    assert_eq!(
        fnv1a(&mix_bytes(&a.l, &a.r)),
        fnv1a(&mix_bytes(&b.l, &b.r)),
        "1 and 8 threads differ on the duet demo"
    );
}

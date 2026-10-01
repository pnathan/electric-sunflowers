//! Duet arrangement (design section 4.6): `Vocals.lead_b`, each singer's
//! notes in order, and the solo song's arrangement unchanged bit for bit.

use arrange::arrange;
use compose::prepare::{prepare, prepare_voices, VoiceChoice};
use song::{Song, Voice};

fn demo() -> Song {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../engine/src/demo.json")).unwrap();
    song::normalize_value(&v).unwrap().0
}

fn demo_duet() -> Song {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../engine/src/demo_duet.json")).unwrap();
    let (s, repairs) = song::normalize_value(&v).unwrap();
    assert!(repairs.is_empty(), "{repairs:?}");
    s
}

/// FNV-1a (64-bit): a small, dependency-free content hash (no sha2 crate is
/// vendored; see CLAUDE.md), used only to catch an accidental change to the
/// solo arrangement's shape, not as a cryptographic hash.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// `Vocals.lead_b` is `Some` iff the song is a duet.
#[test]
fn lead_b_present_iff_duet() {
    let solo = demo();
    let p = prepare(&solo, 1, None);
    assert!(arrange(&solo, &p, 1).vocals.lead_b.is_none());

    let duet = demo_duet();
    let p = prepare_voices(&duet, 1, VoiceChoice::default());
    let a = arrange(&duet, &p, 1);
    assert!(a.vocals.lead_b.is_some());
    // Singer B's voice is the duet's, not singer A's baritone.
    assert_eq!(a.vocals.lead_b.as_ref().unwrap().voice, Voice::Alto);
}

/// Every singer's notes (lead, lead B, harmony) are sorted by onset and
/// never overlap, at several seeds.
#[test]
fn singers_notes_are_ordered_and_non_overlapping() {
    let duet = demo_duet();
    for seed in 0..5u64 {
        let p = prepare_voices(&duet, seed, VoiceChoice::default());
        let a = arrange(&duet, &p, seed);
        let v = &a.vocals;
        for singer in std::iter::once(&v.lead)
            .chain(v.lead_b.iter())
            .chain(std::iter::once(&v.harmony))
            .chain(v.doubles.iter())
        {
            for w in singer.notes.windows(2) {
                assert!(
                    w[0].t0 <= w[1].t0,
                    "seed {seed}: {:?} then {:?}",
                    w[0],
                    w[1]
                );
                assert!(
                    w[0].t1 <= w[1].t0 + 1e-6,
                    "seed {seed}: overlap {:?} then {:?}",
                    w[0],
                    w[1]
                );
            }
        }
    }
}

/// Doubles: 2 takes in a solo song (A's), 4 in a duet (A's two, then B's
/// two, in that order).
#[test]
fn doubles_count_follows_duet() {
    let solo = demo();
    let p = prepare(&solo, 1, None);
    assert_eq!(arrange(&solo, &p, 1).vocals.doubles.len(), 2);

    let duet = demo_duet();
    let p = prepare_voices(&duet, 1, VoiceChoice::default());
    let a = arrange(&duet, &p, 1);
    assert_eq!(a.vocals.doubles.len(), 4);
    assert_eq!(a.vocals.doubles[0].voice, a.vocals.lead.voice);
    assert_eq!(a.vocals.doubles[1].voice, a.vocals.lead.voice);
    assert_eq!(
        a.vocals.doubles[2].voice,
        a.vocals.lead_b.as_ref().unwrap().voice
    );
    assert_eq!(
        a.vocals.doubles[3].voice,
        a.vocals.lead_b.as_ref().unwrap().voice
    );
}

/// The solo demo's arrangement content is unchanged bit for bit by this
/// wave's duet wiring: an FNV-1a64 checksum over the `Debug` text of every
/// field that existed before this wave (the band parts, and the vocals'
/// `lead`, `harmony`, `doubles` and `choir`), deliberately leaving out
/// `Vocals.lead_b` (a new field, `None` in a solo song, whose own text
/// would not appear in the pre-wave `Debug` output at all).
/// `0x6c1999626d93ac0f` was captured from commit `e7647d2a` (the commit
/// before this wave's arrange/engine work) via a `git worktree` checkout,
/// with a throwaway example (built, run, then removed with the worktree)
/// running this same formatting over `demo()` at seed 1.
#[test]
fn solo_arrangement_is_unchanged() {
    let solo = demo();
    let p = prepare(&solo, 1, None);
    let a = arrange(&solo, &p, 1);
    let v = &a.vocals;
    let stable = format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        a.guitar,
        a.bass,
        a.harp,
        a.drums,
        a.violin,
        a.harmony_guitar,
        v.lead,
        v.harmony,
        v.doubles,
        v.choir
    );
    // The melisma flag `legato` is `false` on every note of this song; the
    // checksum is of the arrangement as it was before the field existed.
    let stable = stable.replace(", legato: false", "");
    // Likewise the neutral expression marks (`VocalNote::expr`).
    let stable = stable.replace(
        ", expr: Expr { scoop: None, fall: None, vibrato: None, shape: Flat }",
        "",
    );
    let h = fnv1a(stable.as_bytes());
    assert_eq!(
        h, 0x6c1999626d93ac0f,
        "solo arrangement checksum changed: {h:#x}"
    );
}

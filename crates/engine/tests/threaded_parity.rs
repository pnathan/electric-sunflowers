//! Bit-identical check between the sequential and threaded render/mix
//! paths (`render_song`/`render_song_threaded`, `mix`/`mix_threaded`).
//! Every track's raw channels and the final stereo mix must match exactly
//! (`==` on `f32`, not a tolerance): the whole point of the threaded path
//! is that reordering *which thread* computes a track never reorders any
//! floating-point summation, so there is nothing to tolerate.

use compose::song::normalize_song;
use engine::{demo_song, mix, mix_threaded, render_song, render_song_threaded};
use sfcore::tuning::Tuning;

fn assert_bit_identical(a: &[f32], b: &[f32], label: &str) {
    assert_eq!(a.len(), b.len(), "{label}: length mismatch");
    for i in 0..a.len() {
        assert!(
            a[i].to_bits() == b[i].to_bits(),
            "{label}[{i}]: sequential {} != threaded {} (bits {:#010x} vs {:#010x})",
            a[i],
            b[i],
            a[i].to_bits(),
            b[i].to_bits()
        );
    }
}

fn check_seed(seed: u32) {
    let song = normalize_song(&demo_song()).expect("DEMO_SONG normalizes");
    let tuning = Tuning::default();

    let mut seq = render_song(&song, seed, None, &tuning, None);
    let mut par = render_song_threaded(&song, seed, None, &tuning);

    assert_eq!(seq.len, par.len, "seed {seed}: buffer length differs");
    assert_eq!(seq.raw_tracks.len(), par.raw_tracks.len());
    for ((sk, sc), (pk, pc)) in seq.raw_tracks.iter().zip(par.raw_tracks.iter()) {
        assert_eq!(sk, pk, "seed {seed}: track order differs");
        assert_eq!(sc.len(), pc.len(), "seed {seed}: {sk}: channel count differs");
        for (i, (a, b)) in sc.iter().zip(pc.iter()).enumerate() {
            assert_bit_identical(a, b, &format!("seed {seed}: {sk}[{i}]"));
        }
    }

    let (sl, sr) = mix(&mut seq, |_t| true, seed);
    let (pl, pr) = mix_threaded(&mut par, |_t| true, seed);
    assert_bit_identical(&sl, &pl, &format!("seed {seed}: mix L"));
    assert_bit_identical(&sr, &pr, &format!("seed {seed}: mix R"));
}

#[test]
fn demo_song_threaded_matches_sequential() {
    check_seed(1234);
}

#[test]
fn second_seed_threaded_matches_sequential() {
    check_seed(4242);
}

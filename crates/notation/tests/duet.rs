//! Tests of the lead sheet's duet staves (design 4.8, wave 3): the solo
//! score's SVG is byte-identical to before this wave (checked by hash, the
//! `full_layout.rs` idiom, since no crypto crate is vendored here); the
//! duet fixture (`crates/compose/tests/songs/duet.json`) produces a second
//! staff on every shared line, the "A"/"B"/"A+B" system labels, valid SVG,
//! and note boxes in time order.

use arrange::arrange;
use compose::prepare::{prepare_voices, VoiceChoice};
use notation::full::{FullScore, PartId};
use notation::{engrave, note_boxes, Score};
use resvg::usvg;
use song::Song;

fn demo() -> Song {
    engine::demo_song().clone()
}

fn duet_song() -> Song {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../../compose/tests/songs/duet.json")).expect("duet.json is JSON");
    let (s, repairs) = song::normalize_value(&raw).expect("duet fixture normalises");
    assert!(repairs.is_empty(), "{repairs:?}");
    assert!(s.is_duet());
    s
}

fn parses(svg: &str) {
    let opt = usvg::Options::default();
    usvg::Tree::from_str(svg, &opt).expect("SVG parses");
}

// ---------------------------------------------------------------------
// A minimal, self-contained SHA-256 (FIPS 180-4), copied from
// `full_layout.rs`'s idiom: each test binary is its own crate, so the
// helper cannot be shared, only duplicated.
// ---------------------------------------------------------------------
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ ((!v[4]) & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// Same expected digest `full_layout.rs` checks the wave-1 lead sheet
/// against: the wave-3 duet staves must not touch a solo song's bytes.
const LEAD_SHEET_SHA256: &str = "8dc7fe6e79b14280e0c481d0dfec8f6c84b6950292b084cf6c095f8e33d97271";

#[test]
fn solo_lead_sheet_is_unchanged() {
    let song = demo();
    let prep = compose::prepare::prepare(&song, 1234, None);
    let score = Score::new(&song, &prep);
    let svg = engrave(&score);
    let got = sha256_hex(svg.as_bytes());
    assert_eq!(got.len(), 64, "digest length");
    assert_eq!(got, LEAD_SHEET_SHA256, "the lead sheet's duet staves changed a solo song's SVG bytes");
}

#[test]
fn shared_lines_get_two_staves_and_labels() {
    let song = duet_song();
    let prep = prepare_voices(&song, 3, VoiceChoice::default());
    let score = Score::new(&song, &prep);
    let svg = engrave(&score);
    parses(&svg);

    // Every shared line's "melody" italic label marks one second staff.
    let melody_labels = svg.matches(r#"font-style="italic">melody<"#).count();
    assert!(melody_labels > 0, "no second staff drawn for the duet fixture");

    // The system labels: "A (Baritone)" first, then "A"; "B (Alto)" first
    // shared-only songs still show "B" once B sings; "A+B" on the shared
    // chorus lines.
    assert!(svg.contains("A (Baritone)"), "missing the first A label");
    assert!(svg.contains(">B"), "missing a B label");
    assert!(svg.contains("A+B"), "missing the shared-line label");

    let boxes = note_boxes(&score);
    assert!(!boxes.is_empty());
    for w in boxes.windows(2) {
        assert!(w[0].0 <= w[1].0 + 1e-9, "note boxes out of time order: {:?} then {:?}", w[0], w[1]);
    }
}

/// Part B (design 6.1): the full score's `LeadB` staff draws when
/// `Vocals.lead_b` is present (a duet) and is absent from a solo song.
#[test]
fn full_score_has_a_lead_b_staff_only_in_a_duet() {
    let song = duet_song();
    let prep = prepare_voices(&song, 5, VoiceChoice::default());
    let arr = arrange(&song, &prep, 5);
    assert!(arr.vocals.lead_b.is_some(), "the duet fixture's arrangement has no Lead B singer");
    let full = FullScore::new(&song, &prep, &arr);
    assert!(full.staves.iter().any(|s| s.part == PartId::LeadB), "no LeadB staff in the duet full score");

    let page = notation::Sheet::Full(full).page(notation::DEFAULT_WIDTH);
    let opt = usvg::Options::default();
    usvg::Tree::from_str(&page.svg, &opt).expect("full score SVG parses");

    let solo = demo();
    let solo_prep = compose::prepare::prepare(&solo, 1234, None);
    let solo_arr = arrange(&solo, &solo_prep, 1234);
    assert!(solo_arr.vocals.lead_b.is_none());
    let solo_full = FullScore::new(&solo, &solo_prep, &solo_arr);
    assert!(!solo_full.staves.iter().any(|s| s.part == PartId::LeadB), "solo full score must not have a LeadB staff");
}

#[test]
fn duet_fixture_engraves_over_several_seeds() {
    let song = duet_song();
    for seed in 0..8u64 {
        let prep = prepare_voices(&song, seed, VoiceChoice::default());
        let score = Score::new(&song, &prep);
        let svg = engrave(&score);
        parses(&svg);
        let boxes = note_boxes(&score);
        for w in boxes.windows(2) {
            assert!(w[0].0 <= w[1].0 + 1e-9, "seed {seed}: note boxes out of time order");
        }
    }
}

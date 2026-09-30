//! Tests of the full-score and part-view layout (`notation::full::layout`,
//! `notation::page`): the demo full score parses as SVG, its column boxes
//! are ordered and stay inside their system boxes, the violin part view
//! folds silent bars into multi-bar rests, and the lead sheet's SVG is
//! untouched by this wave (byte-identical to before, checked by hash since
//! no crypto crate is vendored here for a golden-file compare).

use std::time::Instant;

use arrange::arrange;
use compose::prepare::prepare;
use notation::full::{FullScore, PartId};
use notation::{engrave, Score, Sheet};
use resvg::usvg;
use song::Song;

fn demo() -> Song {
    engine::demo_song().clone()
}

fn demo_json() -> serde_json::Value {
    serde_json::from_str(engine::DEMO_JSON).expect("demo.json parses")
}

fn song_of(json: serde_json::Value) -> Song {
    song::normalize_value(&json).expect("fixture normalises").0
}

fn parses(svg: &str) {
    let opt = usvg::Options::default();
    usvg::Tree::from_str(svg, &opt).expect("SVG parses");
}

// ---------------------------------------------------------------------
// A minimal, self-contained SHA-256 (FIPS 180-4), so the lead-sheet
// invariant can be checked against a fixed digest without vendoring a
// hashing crate (none is on this machine's build cache).
// ---------------------------------------------------------------------
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The demo lead sheet (seed 1234, the song's own voice), unchanged since
/// before this wave: `SHA256(engrave(Score::new(demo, prep)))` captured
/// with `cargo run --release -p notation --example score -- demo 1234`
/// before any wave-1 edit to `layout.rs` (only visibility of its helpers
/// changed there, never their bodies; see the wave's git diff).
const LEAD_SHEET_SHA256: &str = "8dc7fe6e79b14280e0c481d0dfec8f6c84b6950292b084cf6c095f8e33d97271";

#[test]
fn lead_sheet_is_byte_identical_to_before_this_wave() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let score = Score::new(&song, &prep);
    let svg = engrave(&score);
    let got = sha256_hex(svg.as_bytes());
    assert_eq!(got.len(), 64, "digest length");
    assert_eq!(
        got, LEAD_SHEET_SHA256,
        "lead sheet SVG changed; wave 1 must not touch its bytes"
    );
}

/// The demo full score (seed 1234), captured with `cargo run --release -p
/// notation --example fullscore -- 1234` before wave 3's polish pass
/// (`StaffDef.name`/`.abbrev` becoming owned `String`; `LeadB` lyrics):
/// neither change may touch a solo song's full-score bytes, since both
/// only add behavior gated on a duet (`song.is_duet()`, or a `LeadB`
/// staff that a solo song never has).
const FULL_SCORE_SHA256: &str = "1409cb24d8f8cb535d3cdac726287efa3d812752430f883e1e16e53580632459";

#[test]
fn solo_full_score_is_byte_identical_to_before_wave_3_polish() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let full = FullScore::new(&song, &prep, &arr);
    let page = Sheet::Full(full).page(notation::DEFAULT_WIDTH);
    let got = sha256_hex(page.svg.as_bytes());
    assert_eq!(got.len(), 64, "digest length");
    assert_eq!(
        got, FULL_SCORE_SHA256,
        "full score SVG changed for a solo song; wave 3's polish pass must not touch its bytes"
    );
}

#[test]
fn full_score_parses_and_has_a_staff_per_score_staff() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let full = FullScore::new(&song, &prep, &arr);

    let t0 = Instant::now();
    let page = Sheet::Full(full.clone()).page(notation::DEFAULT_WIDTH);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("full score layout: {ms:.1} ms (aim 300 ms, not enforced)");

    parses(&page.svg);
    assert!(!page.systems.is_empty());

    let n_systems = page.systems.len();
    let staffname_count = page.svg.matches(r#"class="staffname""#).count();
    assert_eq!(
        staffname_count,
        n_systems * full.staves.len(),
        "one staff-name label per staff per system"
    );
}

#[test]
fn full_score_columns_are_ordered_and_within_their_system() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let full = FullScore::new(&song, &prep, &arr);
    let page = Sheet::Full(full).page(notation::DEFAULT_WIDTH);

    for w in page.notes.windows(2) {
        assert!(
            w[0].0 <= w[1].0 + 1e-6,
            "columns out of time order: {:?} then {:?}",
            w[0],
            w[1]
        );
    }
    for col in &page.notes {
        let sys = page
            .systems
            .iter()
            .find(|s| col.0 >= s.0 - 1e-6 && col.1 <= s.1 + 1e-6)
            .unwrap_or_else(|| panic!("column {col:?} has no covering system"));
        assert!(col.2 >= sys.2 - 1e-6, "column left of its system");
        assert!(
            col.2 + col.4 <= sys.2 + sys.4 + 1e-6,
            "column right of its system"
        );
        assert!(
            col.3 >= sys.3 - 1e-6 && col.3 + col.5 <= sys.3 + sys.5 + 1e-6,
            "column outside its system's vertical span"
        );
    }
}

#[test]
fn violin_part_view_has_a_multi_bar_rest_and_parses() {
    let mut v = demo_json();
    v["band"]["violin"] = serde_json::json!(true);
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let full = FullScore::new(&song, &prep, &arr);

    let Some(violin) = full.part(PartId::Violin) else {
        panic!("violin should have a staff when band.violin is on");
    };
    assert!(
        violin
            .bars
            .iter()
            .any(|b| matches!(b, notation::full::PartBar::MultiRest { bars } if *bars >= 2)),
        "expected at least one multi-bar rest in the violin part"
    );

    let page = Sheet::Part(full, PartId::Violin).page(notation::DEFAULT_WIDTH);
    parses(&page.svg);
    assert!(
        page.svg.contains("restHBar") || page.svg.contains("g-restHBar"),
        "multi-bar rest should draw restHBar"
    );
}

#[test]
fn part_view_of_an_absent_part_does_not_panic() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let full = FullScore::new(&song, &prep, &arr);
    // LeadB never has a staff before wave 3.
    let page = Sheet::Part(full, PartId::LeadB).page(notation::DEFAULT_WIDTH);
    parses(&page.svg);
}

/// The full score's lead staff prints a melisma's syllable once and draws
/// an extension line under each later note.
#[test]
fn full_score_melisma_has_one_lyric_and_extensions() {
    let song = song_of(serde_json::json!({
        "schema_version":2,"title":"Melisma","key":"G","mode":"major","meter":"4/4","tempo":92,
        "sections":[{"type":"verse","lines":[{"syl":"*glo~3 *hal~4 *sing~ out","chords":["G","C","D","G"]}]}]
    }));
    let prep = prepare(&song, 4, None);
    let arr = arrange(&song, &prep, 4);
    let full = FullScore::new(&song, &prep, &arr);
    let svg = Sheet::Full(full).page(notation::DEFAULT_WIDTH).svg;
    parses(&svg);
    assert_eq!(svg.matches(r#"class="lyric">"#).count(), 4);
    assert!(svg.matches(r#"stroke-width="0.90""#).count() >= 6);
}

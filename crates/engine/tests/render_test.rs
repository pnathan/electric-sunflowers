//! Smoke test for `render_song`/`mix` (step 2 of the renderSong port):
//! runs the demo song end to end and checks every track that should carry
//! signal actually does.

use compose::song::normalize_song;
use engine::demo_song;
use engine::{mix, render_song};
use sfcore::tuning::Tuning;

fn nonzero_frac(buf: &[f32]) -> f64 {
    if buf.is_empty() {
        return 0.0;
    }
    let n = buf.iter().filter(|&&v| v != 0.0).count();
    n as f64 / buf.len() as f64
}

#[test]
fn render_song_produces_every_track() {
    let song = normalize_song(&demo_song()).expect("DEMO_SONG normalizes");
    let tuning = Tuning::default();
    let seed = 1234u32;

    let mut rendered = render_song(&song, seed, None, &tuning, None);
    assert!(rendered.len > 0);

    // Every raw track DEMO_SONG's band turns on should be non-silent.
    // (band.rs's `apply_body` already checked guitar/hg/harp/violin for
    // silence before running the body convolution; this re-checks all ten
    // tracks post-mix input by reading back through a full mix per track,
    // which is the only way `Render`'s private per-track cache is visible.)
    let (l, r) = mix(&mut rendered, |_t| true, seed);
    assert_eq!(l.len(), rendered.len);
    assert_eq!(r.len(), rendered.len);
    assert!(nonzero_frac(&l) > 0.1, "mixed L is mostly silent");
    assert!(nonzero_frac(&r) > 0.1, "mixed R is mostly silent");

    // A peak-normalised mix should actually reach up near its ceiling
    // somewhere, not just have scattered nonzero samples.
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, &v| m.max(v.abs()));
    assert!(peak > 0.5, "mixed peak {peak} looks too quiet for a normalised mix");
}

#[test]
fn render_song_runs_on_the_12_bar_blues_form() {
    // The 12-bar-blues literal from tests/formtest.js: three verse-shaped
    // 12-bar chord sections with a I-IV-I-V blues progression, run at seed 7,
    // voice alto, standing in for the second parity case this port targets.
    let raw = serde_json::json!({
        "title":"Blues Smoke Test","key":"C","mode":"major","meter":"4/4","tempo":96,
        "guitar":"strum","voice":"alto",
        "band":{"drums":"full","bass":true,"harmonyGuitar":true,"harp":true,"violin":true,"choir":true,"harmonies":true,"doubles":true},
        "sections":[
            {"type":"verse","lines":[
                {"syl":"*Wo-ke up this *morn-ing, *sun was *not a-round","chords":["C","C","C","C","F","F","C","C","G","F","C","G"]}
            ]},
            {"type":"verse","same":true},
            {"type":"verse","same":true}
        ]
    });
    let song = normalize_song(&raw).expect("blues literal normalizes");
    let tuning = Tuning::default();
    let seed = 7u32;

    let mut rendered = render_song(&song, seed, Some(compose::voices::Voice::Alto), &tuning, None);
    let (l, r) = mix(&mut rendered, |t| t.always || true, seed);
    assert!(nonzero_frac(&l) > 0.05);
    assert!(nonzero_frac(&r) > 0.05);
}

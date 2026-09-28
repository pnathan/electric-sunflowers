//! Engine tests (design section 9): finite output, peak 0.89, thread-count
//! invariance, stems present or absent as the song and band say, and band
//! toggles as a re-mix of the cached stems.

use engine::{demo_song, mix, render, BandPart, NoProgress, Stems, Stereo, TrackId};
use song::{Band, DrumKit, Song};

/// A short full-band song (three 12-bar blues verses).
fn blues(drums: &str) -> Song {
    let raw = serde_json::json!({
        "title":"Blues Test","key":"C","mode":"major","meter":"4/4","tempo":112,
        "guitar":"strum","voice":"alto",
        "band":{"drums":drums,"bass":true,"harmonyGuitar":true,"harp":true,"violin":true,"choir":true,"harmonies":true,"doubles":true},
        "sections":[
            {"type":"verse","lines":[
                {"syl":"*Wo-ke up this *morn-ing, *sun was *not a-round","chords":["C","C","C","C","F","F","C","C","G","F","C","G"]}
            ]},
            {"type":"chorus","lines":[
                {"syl":"*Roll on *down the *line to *town","chords":["F","F","C","C","G","F","C","G"]}
            ]},
            {"type":"verse","same":true}
        ]
    });
    song::normalize_value(&raw).expect("blues literal normalises").0
}

fn peak(m: &Stereo) -> f32 {
    m.l.iter().chain(&m.r).fold(0.0f32, |a, &v| a.max(v.abs()))
}

fn assert_valid(m: &Stereo, len: usize) {
    assert_eq!(m.l.len(), len);
    assert_eq!(m.r.len(), len);
    assert!(m.l.iter().chain(&m.r).all(|v| v.is_finite()), "non-finite sample");
    assert!((peak(m) as f64 - 0.89).abs() < 1e-3, "peak {}", peak(m));
}

fn pool(n: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(n)
        .start_handler(|_| sfcore::fp::flush_denormals())
        .build()
        .expect("thread pool")
}

fn bits(m: &Stereo) -> Vec<u32> {
    m.l.iter().chain(&m.r).map(|v| v.to_bits()).collect()
}

fn render_mix(song: &Song, seed: u64) -> (Stems, Stereo) {
    let (_, stems) = render(song, seed, None, &NoProgress);
    let m = mix(&stems, &song.band, seed);
    (stems, m)
}

#[test]
fn demo_mix_is_finite_and_normalised() {
    let song = demo_song();
    let (stems, m) = render_mix(song, 1234);
    assert_valid(&m, stems.len);
    for id in TrackId::ALL {
        assert!(stems.get(id).is_some(), "demo has no {} stem", id.name());
    }
    assert!(stems.get(TrackId::Lead).is_some_and(|p| p.slap.is_some()));
}

#[test]
fn one_and_eight_threads_are_bit_identical() {
    // The demo: every track, including the doubles and choir joins.
    let song = demo_song();
    let (_, a) = pool(1).install(|| render_mix(song, 7));
    let (_, b) = pool(8).install(|| render_mix(song, 7));
    assert_eq!(a.l.len(), b.l.len());
    assert!(bits(&a) == bits(&b), "1 and 8 threads differ");
}

/// Whether the arrangement of `song` at `seed` has events for track `id`.
fn has_events(song: &Song, seed: u64, id: TrackId) -> bool {
    let prepared = compose::prepare::prepare(song, seed, None);
    let a = arrange::arrange(song, &prepared, seed);
    let v = &a.vocals;
    match id {
        TrackId::Lead => !v.lead.notes.is_empty(),
        TrackId::Doubles => v.doubles.iter().any(|s| !s.notes.is_empty()),
        TrackId::Harmony => !v.harmony.notes.is_empty(),
        TrackId::Choir => v.choir.iter().flatten().any(|s| !s.notes.is_empty()),
        TrackId::Guitar => a.guitar.iter().any(|s| !s.is_empty()),
        TrackId::HarmonyGuitar => !a.harmony_guitar.lead.is_empty() || !a.harmony_guitar.arp.is_empty(),
        TrackId::Bass => !a.bass.is_empty(),
        TrackId::Drums => a.drums.as_ref().is_some_and(|d| !d.is_empty()),
        TrackId::Harp => !a.harp.is_empty(),
        TrackId::Violin => !a.violin.is_empty(),
    }
}

#[test]
fn stems_follow_the_song() {
    for (drums, seed) in [("none", 3), ("full", 5)] {
        let song = blues(drums);
        let (stems, m) = render_mix(&song, seed);
        assert_valid(&m, stems.len);
        for id in TrackId::ALL {
            assert_eq!(stems.get(id).is_some(), has_events(&song, seed, id), "{} stem, drums {drums}", id.name());
        }
        assert_eq!(stems.get(TrackId::Drums).is_some(), drums != "none");
    }
}

#[test]
fn band_toggles_remix_the_cached_stems() {
    let song = blues("brushes");
    let (stems, full) = render_mix(&song, 11);
    for part in BandPart::ALL {
        let mut band: Band = song.band;
        part.switch_off(&mut band);
        let m = mix(&stems, &band, 11);
        assert_valid(&m, stems.len);
        let present = TrackId::ALL.iter().any(|id| id.strip().band == Some(part) && stems.get(*id).is_some());
        assert_eq!(bits(&m) != bits(&full), present, "switching off {}", part.name());

        // The same as mixing stems that never had the track.
        let mut without = stems.clone();
        for id in TrackId::ALL.into_iter().filter(|id| id.strip().band == Some(part)) {
            without.tracks[id.index()] = None;
        }
        assert!(bits(&m) == bits(&mix(&without, &song.band, 11)), "{}", part.name());
    }
    // The cache is not consumed: the full band mixes to the same samples.
    assert!(bits(&mix(&stems, &song.band, 11)) == bits(&full));
    // Everything off but the lead and guitar.
    let bare = Band { drums: DrumKit::None, bass: false, harmony_guitar: false, harp: false, violin: false, choir: false, harmonies: false, doubles: false };
    assert_valid(&mix(&stems, &bare, 11), stems.len);
}

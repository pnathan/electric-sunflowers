//! Tests of the full score model (`notation::full`): staves present per the
//! band, quantisation, harp split, drum positions, choir clefs, part-view
//! multi-rests.

use arrange::arrange;
use compose::prepare::{prepare, prepare_voices, VoiceChoice};
use compose::timeline::Timeline;
use notation::full::{Clef, FullScore, Notehead, PartBar, PartId};
use song::{SingerId, Song};

fn song_of(json: serde_json::Value) -> Song {
    song::normalize_value(&json).expect("fixture normalises").0
}

fn demo() -> Song {
    engine::demo_song().clone()
}

/// The wave-3 duet fixture (also used by `tests/duet.rs`).
fn duet_song() -> Song {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("../../compose/tests/songs/duet.json")).expect("duet.json is JSON");
    let (s, repairs) = song::normalize_value(&raw).expect("duet fixture normalises");
    assert!(repairs.is_empty(), "{repairs:?}");
    assert!(s.is_duet());
    s
}

/// The demo song's JSON, for tests that flip a `band` flag.
fn demo_json() -> serde_json::Value {
    serde_json::from_str(engine::DEMO_JSON).expect("demo.json parses")
}

/// Grid units per beat: `2 * sub`, matching `notation::score::Grid::of`.
fn beat_u(song: &Song) -> i64 {
    2 * song.meter.grid().sub as i64
}

/// Bar length in units.
fn bar_u(song: &Song) -> i64 {
    beat_u(song) * song.meter.grid().beats as i64
}

/// Rounds `t` (seconds) to the nearest grid unit, the same rounding
/// `notation::full` uses to quantise onsets (and so to merge chords).
fn unit_of(tl: &Timeline, song: &Song, t: f64) -> i64 {
    (tl.to_beat(t) * beat_u(song) as f64).round() as i64
}

/// Distinct quantised onsets among `times`.
fn distinct_onsets(tl: &Timeline, song: &Song, times: impl Iterator<Item = f64>) -> usize {
    let mut u: Vec<i64> = times.map(|t| unit_of(tl, song, t)).collect();
    u.sort_unstable();
    u.dedup();
    u.len()
}

/// Non-tied-in chords in one staff's voice `v`, across every bar (each such
/// chord is one onset after merging).
fn chord_starts(score: &FullScore, part: PartId, v: usize) -> usize {
    let idx = score.staves.iter().position(|s| s.part == part).expect("staff present");
    score
        .bars
        .iter()
        .flat_map(|b| b.cells[idx].voices[v].iter())
        .filter(|e| e.chord.as_ref().is_some_and(|c| !c.tie_in))
        .count()
}

#[test]
fn onsets_match_the_arrangement_after_merging() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);
    let tl = &prep.timeline;

    assert_eq!(chord_starts(&score, PartId::Lead, 0), distinct_onsets(tl, &song, prep.comp.lead.iter().map(|n| n.t0)), "lead");
    assert_eq!(
        chord_starts(&score, PartId::Guitar, 0),
        distinct_onsets(tl, &song, arr.guitar.iter().flatten().map(|n| n.t)),
        "guitar"
    );
    assert_eq!(chord_starts(&score, PartId::Bass, 0), distinct_onsets(tl, &song, arr.bass.iter().map(|n| n.t0)), "bass");
    if song.band.violin {
        assert_eq!(chord_starts(&score, PartId::Violin, 0), distinct_onsets(tl, &song, arr.violin.iter().map(|n| n.t0)), "violin");
    }
}

#[test]
fn bars_are_full_in_every_voice() {
    let song = demo();
    for seed in [1234u64, 2718, 42] {
        let prep = prepare(&song, seed, None);
        let arr = arrange(&song, &prep, seed);
        let score = FullScore::new(&song, &prep, &arr);
        let bu = bar_u(&song);
        for bar in &score.bars {
            for cell in &bar.cells {
                for voice in &cell.voices {
                    let sum: i64 = voice.iter().map(|e| e.d).sum();
                    assert_eq!(sum, bu, "seed {seed} bar {}", bar.bar);
                    // Contiguous, starting at 0.
                    let mut c = 0i64;
                    for e in voice {
                        assert_eq!(e.s, c, "seed {seed} bar {}", bar.bar);
                        c += e.d;
                    }
                }
            }
        }
    }
}

#[test]
fn harp_splits_at_middle_c() {
    let mut v = demo_json();
    v["band"]["harp"] = serde_json::json!(true);
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);

    let steps = |part: PartId| -> Vec<i32> {
        let idx = score.staves.iter().position(|s| s.part == part).expect("harp staff present");
        score
            .bars
            .iter()
            .flat_map(|b| b.cells[idx].voices[0].iter())
            .filter_map(|e| e.chord.as_ref())
            .flat_map(|c| c.heads.iter().map(|h| h.step))
            .collect()
    };
    // MIDI 60 (middle C) spells to step 28 (C4); the split is at that step.
    if score.staves.iter().any(|s| s.part == PartId::HarpUpper) {
        assert!(steps(PartId::HarpUpper).iter().all(|&s| s >= 28), "upper harp below the split");
    }
    if score.staves.iter().any(|s| s.part == PartId::HarpLower) {
        assert!(steps(PartId::HarpLower).iter().all(|&s| s < 28), "lower harp above the split");
    }
}

/// A choir chord that spans a chord segment longer than one bar must tie
/// across the barline (last event of bar k has `tie_out`, first event of
/// bar k+1 has `tie_in`), never a note cut short with a rest after it.
#[test]
fn notes_tie_across_a_barline() {
    let mut v = demo_json();
    v["band"]["choir"] = serde_json::json!(true);
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);

    let choir_parts = [PartId::ChoirS, PartId::ChoirA, PartId::ChoirT, PartId::ChoirB];
    let mut found_tie = false;
    let mut found_rest_after_tie_out = false;
    for &part in &choir_parts {
        let Some(idx) = score.staves.iter().position(|s| s.part == part) else { continue };
        for bar in 0..score.bars.len() {
            let voice = &score.bars[bar].cells[idx].voices[0];
            let Some(last) = voice.last() else { continue };
            let Some(chord) = &last.chord else { continue };
            if chord.tie_out {
                found_tie = true;
                // The tied note must be the last thing in this bar: no rest
                // can follow a tie_out event.
                if let Some(next_bar) = score.bars.get(bar + 1) {
                    let next_voice = &next_bar.cells[idx].voices[0];
                    match next_voice.first() {
                        Some(first) if first.chord.as_ref().is_some_and(|c| c.tie_in) => {}
                        _ => found_rest_after_tie_out = true,
                    }
                }
            }
        }
    }
    assert!(found_tie, "expected at least one choir note to tie across a barline");
    assert!(!found_rest_after_tie_out, "a tie_out event must be followed by a tie_in event, never a rest");
}

#[test]
fn drum_positions_and_noteheads() {
    let mut v = demo_json();
    v["band"]["drums"] = serde_json::json!("full");
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);
    let idx = score.staves.iter().position(|s| s.part == PartId::Drums).expect("drums present");

    // Voice 0 (up: everything but the kick) and voice 1 (down: the kick).
    for bar in &score.bars {
        let cell = &bar.cells[idx];
        assert_eq!(cell.voices.len(), 2, "drums have two voices");
        for e in &cell.voices[1] {
            if let Some(c) = &e.chord {
                for h in &c.heads {
                    assert_eq!((h.step, h.notehead), (31, Notehead::Normal), "kick: F4 normal");
                }
            }
        }
        for e in &cell.voices[0] {
            if let Some(c) = &e.chord {
                for h in &c.heads {
                    let ok = matches!(
                        (h.step, h.notehead),
                        (35, Notehead::Normal) // snare/tap
                            | (35, Notehead::X) // rim
                            | (35, Notehead::Slash) // swish
                            | (39, Notehead::X) // hat
                            | (38, Notehead::X) // ride
                            | (37, Notehead::Normal) // tom >= 150 Hz
                            | (33, Notehead::Normal) // tom < 150 Hz
                            | (40, Notehead::X) // shaker
                    );
                    assert!(ok, "unexpected up-voice head {h:?}");
                }
            }
        }
    }
}

#[test]
fn choir_clefs_are_fixed_per_part() {
    let mut v = demo_json();
    v["band"]["choir"] = serde_json::json!(true);
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);
    let clef_of = |p: PartId| score.staves.iter().find(|s| s.part == p).map(|s| s.clef);
    if let Some(c) = clef_of(PartId::ChoirS) {
        assert_eq!(c, Clef::Treble);
    }
    if let Some(c) = clef_of(PartId::ChoirA) {
        assert_eq!(c, Clef::Treble);
    }
    if let Some(c) = clef_of(PartId::ChoirT) {
        assert_eq!(c, Clef::Treble8vb);
    }
    if let Some(c) = clef_of(PartId::ChoirB) {
        assert_eq!(c, Clef::Bass);
    }
}

#[test]
fn staves_absent_when_the_band_is_off() {
    let mut v = demo_json();
    v["band"] = serde_json::json!({
        "drums": "none", "bass": false, "harmonyGuitar": false, "harp": false,
        "violin": false, "choir": false, "harmonies": false, "doubles": false,
    });
    let song = song_of(v);
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);

    for part in [
        PartId::Harmony,
        PartId::Doubles,
        PartId::ChoirS,
        PartId::ChoirA,
        PartId::ChoirT,
        PartId::ChoirB,
        PartId::Violin,
        PartId::HarmonyGuitar,
        PartId::HarpUpper,
        PartId::HarpLower,
        PartId::Bass,
        PartId::Drums,
        PartId::LeadB,
    ] {
        assert!(!score.staves.iter().any(|s| s.part == part), "{part:?} should be absent");
    }
    // Lead and guitar always play.
    assert!(score.staves.iter().any(|s| s.part == PartId::Lead));
    assert!(score.staves.iter().any(|s| s.part == PartId::Guitar));
}

/// The LeadB staff's notes carry singer B's own syllables (design 4.6/6.1),
/// sourced from `Prepared.comp.lead`/`comp.second` (`compose::melody::
/// LeadNote`), not from `song::events::VocalNote` (which has no lyric
/// text): every syllable text drawn on the LeadB staff must be one of B's
/// own syllables, and at least one is drawn.
#[test]
fn lead_b_carries_its_own_lyric_syllables() {
    let song = duet_song();
    let prep = prepare_voices(&song, 3, VoiceChoice::default());
    let arr = arrange(&song, &prep, 3);
    let score = FullScore::new(&song, &prep, &arr);
    let idx = score.staves.iter().position(|s| s.part == PartId::LeadB).expect("LeadB staff present in a duet");

    let expected: std::collections::HashSet<&str> = prep
        .comp
        .lead
        .iter()
        .chain(prep.comp.second.iter())
        .filter(|n| n.singer == SingerId::B)
        .map(|n| n.syl.text.as_str())
        .collect();

    let mut found = 0;
    for bar in &score.bars {
        for voice in &bar.cells[idx].voices {
            for e in voice {
                if let Some(l) = e.chord.as_ref().and_then(|c| c.lyric.as_deref()) {
                    assert!(expected.contains(l), "unexpected LeadB syllable {l:?}");
                    found += 1;
                }
            }
        }
    }
    assert!(found > 0, "no lyric syllables drawn on the LeadB staff");
}

#[test]
fn part_view_folds_silent_runs_into_multi_rests() {
    let song = demo();
    let prep = prepare(&song, 1234, None);
    let arr = arrange(&song, &prep, 1234);
    let score = FullScore::new(&song, &prep, &arr);

    // Harmony only sounds in lifted sections, so a song with an intro or an
    // unlifted first verse leaves it silent for a run of bars.
    let Some(harmony) = score.part(PartId::Harmony) else {
        return;
    };
    let total_bars = score.bars.len();
    let covered: usize = harmony
        .bars
        .iter()
        .map(|b| match b {
            PartBar::Bar { .. } => 1,
            PartBar::MultiRest { bars } => *bars,
        })
        .sum();
    assert_eq!(covered, total_bars, "part view must cover every bar exactly once");
    assert!(
        harmony.bars.iter().any(|b| matches!(b, PartBar::MultiRest { bars } if *bars >= 2)),
        "expected at least one multi-bar rest in the harmony part"
    );
}

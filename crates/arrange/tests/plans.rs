//! Properties of the arrangement plans: determinism, playable guitar
//! shapes, ascending choir voicings, the harp glissando before bridges, bass
//! roots at chord changes, finite events, and no audio code in the crate.

use arrange::guitar::{voicing, OPEN_STRINGS};
use arrange::{arrange, Arrangement};
use compose::prepare::{prepare, Prepared};
use song::{Chord, DrumKit, Pc, SectionKind, Song};

fn demo() -> Song {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../engine/src/demo.json")).unwrap();
    song::normalize_value(&v).unwrap().0
}

/// A 4/4 strummed song with every section kind, full drums and a slash chord.
fn strum_song() -> Song {
    let v = serde_json::json!({
        "key":"D","mode":"major","meter":"4/4","tempo":112,"guitar":"strum","voice":"alto",
        "band":{"drums":"full","bass":true,"harmonyGuitar":true,"harp":true,"violin":true,"choir":true,"harmonies":true,"doubles":true},
        "sections":[
            {"type":"intro","chords":["D","G/B","A","D"]},
            {"type":"verse","lines":[
                {"syl":"*walk the *line a-*long the *ridge","chords":["D","Bm7"]},
                {"syl":"*coun-ting *stones be-*neath the *bridge","chords":["G","A7sus4"]}
            ]},
            {"type":"chorus","lines":[{"syl":"*hold *on, *hold *on to the *light","chords":["G","D/F#","Em7","A"]}]},
            {"type":"verse","same":true},
            {"type":"chorus","same":true},
            {"type":"bridge","lines":[{"syl":"*ev-ery *road *bends *home","chords":["Bm","G","Em","A"]}]},
            {"type":"chorus","same":true},
            {"type":"outro","chords":["G","A","D"]}
        ]
    });
    song::normalize_value(&v).unwrap().0
}

fn plan(song: &Song, seed: u32) -> (Prepared, Arrangement) {
    let p = prepare(song, seed, None);
    let a = arrange(song, &p, seed as u64);
    (p, a)
}

#[test]
fn plans_are_deterministic_per_seed() {
    for song in [demo(), strum_song()] {
        let (_, a) = plan(&song, 1234);
        let (_, b) = plan(&song, 1234);
        assert_eq!(a, b);
        let (_, c) = plan(&song, 2718);
        assert_ne!(a.guitar, c.guitar, "another seed moves the strum timing");
    }
}

fn check_shape(ch: &Chord) {
    let v = voicing(ch);
    let fretted: Vec<u8> = v.frets.iter().flatten().copied().filter(|&f| f > 0).collect();
    assert!(fretted.len() <= 4, "{}: {} fretted", ch.symbol, fretted.len());
    if let (Some(lo), Some(hi)) = (fretted.iter().min(), fretted.iter().max()) {
        assert!(hi - lo <= 3, "{}: span {}", ch.symbol, hi - lo);
    }
    let bass = v.bass_string().expect("a voicing sounds");
    assert!(bass <= 2, "{}: bass on string {bass}", ch.symbol);
    assert_eq!(Pc::new(v.notes[bass].unwrap() as i32), ch.bass, "{}: bass note", ch.symbol);
    for (s, &open) in OPEN_STRINGS.iter().enumerate() {
        assert_eq!(v.notes[s], v.frets[s].map(|f| open + f));
        if let Some(n) = v.notes[s] {
            assert!(ch.tones.with(ch.bass).contains(Pc::new(n as i32)), "{}: non-chord tone", ch.symbol);
        }
    }
}

#[test]
fn guitar_voicings_are_playable() {
    let qualities = ["", "m", "7", "m7", "maj7", "sus2", "sus4", "7sus4", "dim", "aug", "6", "m6", "add9", "9", "m7b5", "dim7"];
    for root in ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"] {
        for q in qualities {
            let ch = Chord::parse(&format!("{root}{q}")).unwrap();
            check_shape(&ch);
        }
    }
    for s in ["G/B", "D/F#", "C/E", "Am/G", "F/C"] {
        check_shape(&Chord::parse(s).unwrap());
    }
}

#[test]
fn guitar_notes_sit_on_their_strings() {
    for song in [demo(), strum_song()] {
        let (_, a) = plan(&song, 7);
        assert!(a.guitar.iter().any(|s| !s.is_empty()));
        for (s, notes) in a.guitar.iter().enumerate() {
            for w in notes.windows(2) {
                assert!(w[0].t <= w[1].t, "string {s} not sorted");
            }
            for n in notes {
                assert_eq!(n.string as usize, s);
                assert!(n.midi >= OPEN_STRINGS[s] && n.midi <= OPEN_STRINGS[s] + 12, "string {s}: midi {}", n.midi);
                assert!(n.stop > n.t);
            }
        }
    }
}

#[test]
fn choir_voicings_ascend() {
    for song in [demo(), strum_song()] {
        let (p, _) = plan(&song, 11);
        let vs = arrange::choir::voicings(&p.form, &p.timeline, |_| true);
        assert!(!vs.is_empty());
        for cv in &vs {
            let v = cv.notes;
            assert!(v[0] < v[1] && v[1] < v[2] && v[2] < v[3], "{v:?}");
            for (k, &(lo, hi)) in arrange::choir::CHOIR_RANGE.iter().enumerate() {
                assert!(v[k] >= lo as i32 && v[k] <= hi as i32, "{v:?}");
            }
            let ch = p.form.chord(p.timeline.segs[cv.seg].chord);
            assert_eq!(Pc::new(v[0]), ch.bass);
        }
    }
}

#[test]
fn harp_glissando_before_bridges() {
    for song in [demo(), strum_song()] {
        let (p, a) = plan(&song, 5);
        let mut bridges = 0;
        for sec in p.form.sections.iter().filter(|s| s.kind == SectionKind::Bridge) {
            bridges += 1;
            let t = p.timeline.to_time(sec.beats(&p.form.meter).start);
            let run: Vec<_> = a.harp.iter().filter(|n| n.t0 >= t - 0.66 && n.t0 < t - 0.02).collect();
            assert!(run.len() >= 7, "{} notes before the bridge", run.len());
            for w in run.windows(2) {
                assert!(w[1].midi > w[0].midi && w[1].t0 > w[0].t0, "glissando rises");
            }
        }
        assert!(bridges > 0);
    }
}

#[test]
fn bass_plays_roots_at_chord_changes() {
    for song in [demo(), strum_song()] {
        let (p, a) = plan(&song, 3);
        let mut checked = 0;
        for sg in &p.timeline.segs {
            let t = p.timeline.to_time(sg.b0);
            let ch = p.form.chord(sg.chord);
            for n in a.bass.iter().filter(|n| (n.t0 - t).abs() < 0.005) {
                assert_eq!(Pc::new(n.midi as i32), ch.bass, "bass at {t:.3} s");
                checked += 1;
            }
        }
        assert!(checked > 10, "only {checked} chord changes with a bass note");
        let first_chorus = p.form.sections.iter().find(|s| s.kind == SectionKind::Chorus).unwrap();
        let t = p.timeline.to_time(first_chorus.beats(&p.form.meter).start);
        assert!(a.bass.iter().any(|n| (n.t0 - t).abs() < 0.005), "bass on the chorus downbeat");
    }
}

#[test]
fn plans_are_finite() {
    for song in [demo(), strum_song()] {
        let (_, a) = plan(&song, 99);
        let ok = |t: f64, m: f32, v: f32| t.is_finite() && t >= 0.0 && m.is_finite() && v.is_finite() && v > 0.0 && v <= 1.5;
        for n in a.guitar.iter().flatten() {
            assert!(ok(n.t, n.midi as f32, n.vel) && n.stop.is_finite());
        }
        for n in a.bass.iter().chain(&a.harp).chain(&a.harmony_guitar.lead).chain(&a.harmony_guitar.arp) {
            assert!(ok(n.t0, n.midi, n.vel) && n.t1.is_finite() && n.t1 >= n.t0, "{n:?}");
        }
        for n in &a.violin {
            assert!(ok(n.t0, n.midi, n.vel) && n.t1 > n.t0, "{n:?}");
        }
        for h in a.drums.iter().flatten() {
            assert!(ok(h.t, 0.0, h.vel) && h.pan.abs() <= 1.0, "{h:?}");
        }
        let v = &a.vocals;
        let singers = std::iter::once(&v.lead).chain([&v.harmony]).chain(&v.doubles).chain(v.choir.iter().flatten());
        for s in singers {
            assert!(s.pan.abs() <= 1.0 && s.offset.is_finite());
            for n in &s.notes {
                assert!(ok(n.t0, n.midi, n.amp) && n.t1 > n.t0, "{n:?}");
            }
        }
        assert!(!v.lead.notes.is_empty());
        assert_eq!(v.choir.iter().map(Vec::len).sum::<usize>(), 4 * arrange::vocals::CHOIR_SINGERS);
    }
}

#[test]
fn drums_follow_the_kit() {
    let mut song = strum_song();
    let (_, a) = plan(&song, 1);
    assert!(a.drums.as_ref().is_some_and(|d| !d.is_empty()));
    song.band.drums = DrumKit::None;
    let (_, a) = plan(&song, 1);
    assert!(a.drums.is_none());
}

#[test]
fn arrange_has_no_audio_code() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.contains("dsp"), "arrange depends on dsp");
    assert!(!manifest.contains("instruments") && !manifest.contains("voice"));
    for src in [
        include_str!("../src/lib.rs"),
        include_str!("../src/guitar.rs"),
        include_str!("../src/bass.rs"),
        include_str!("../src/harp.rs"),
        include_str!("../src/drums.rs"),
        include_str!("../src/choir.rs"),
        include_str!("../src/lines.rs"),
        include_str!("../src/violin.rs"),
        include_str!("../src/harmony_guitar.rs"),
        include_str!("../src/vocals.rs"),
    ] {
        for bad in ["dsp::", "SR_F", "Vec<f32>", "sfcore::js", "sfcore::rng"] {
            assert!(!src.contains(bad), "arrange source contains {bad}");
        }
    }
}

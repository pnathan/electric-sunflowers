//! Choir lines (schema 2, issue #7): the leads rest, the choir sings the
//! words in four parts of three singers, the /aa/ pad keeps off the lines,
//! and the unison and block pitch rules hold.

use arrange::arrange;
use arrange::choir::{line_notes, voicings_clear};
use compose::prepare::{prepare, Prepared};
use song::events::Singer;
use song::{Part, Pc, Song, Voice};

/// Two verses of solo lead, then a chorus with a choir line in unison and
/// one in block harmony, then a repeated chorus (a lifted repeat, so the
/// pad sounds) with one choir line.
fn choir_json() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 2,
        "title": "Shanty", "note": "", "key": "G", "mode": "major", "meter": "4/4", "tempo": 92,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false,
                 "violin": false, "choir": true, "harmonies": true, "doubles": true},
        "sections": [
            {"type": "verse", "lines": [
                {"syl": "*wind *fills the *sail", "ph": "w ih n d|f ih l z|dh ah|s ey l", "chords": ["G", "C"]},
                {"syl": "*pull *on the *rope", "ph": "p uh l|aa n|dh ah|r ow p", "chords": ["D", "G"]}]},
            {"type": "chorus", "lines": [
                {"syl": "*heave *ho *heave *ho", "ph": "hh iy v|hh ow|hh iy v|hh ow", "chords": ["G", "C"]},
                {"syl": "*roll *ye *bold", "ph": "r ow l|y iy|b ow l d", "chords": ["C", "D"],
                 "sing": "choir", "voicing": "unison"},
                {"syl": "*hal~le~lu~3 *jah", "ph": "hh ae|l ah|l uw|jh aa", "chords": ["G", "D"],
                 "sing": "choir", "voicing": "block"}]},
            {"type": "chorus", "same": true}
        ]
    })
}

fn song_of(v: serde_json::Value) -> Song {
    let (s, _) = song::normalize_value(&v).expect("song reads");
    s
}

fn prep(s: &Song) -> Prepared {
    prepare(s, 1234, None)
}

/// (t0, t1) seconds of choir line `li`'s notes.
fn span(p: &Prepared, li: usize) -> (f64, f64) {
    let ns: Vec<_> = p.comp.lead.iter().filter(|n| n.line_idx == li).collect();
    let t0 = ns.iter().map(|n| n.t0).fold(f64::INFINITY, f64::min);
    let t1 = ns.iter().map(|n| n.t1).fold(f64::NEG_INFINITY, f64::max);
    (t0, t1)
}

fn choir_lines(p: &Prepared) -> Vec<usize> {
    (0..p.form.lines.len())
        .filter(|&i| p.form.lines[i].part.is_choir())
        .collect()
}

fn inside(singer: &Singer, (t0, t1): (f64, f64)) -> Vec<&song::events::VocalNote> {
    singer
        .notes
        .iter()
        .filter(|n| n.t0 >= t0 - 0.05 && n.t0 < t1)
        .collect()
}

#[test]
fn the_leads_rest_on_choir_lines() {
    let s = song_of(choir_json());
    let p = prep(&s);
    let a = arrange(&s, &p, 1234);
    let lines = choir_lines(&p);
    assert_eq!(lines.len(), 4, "two choir lines, in two choruses");
    let mut choir_notes = 0;
    for &li in &lines {
        let sp = span(&p, li);
        choir_notes += p.comp.lead.iter().filter(|n| n.line_idx == li).count();
        let v = &a.vocals;
        for singer in [&v.lead, &v.harmony].into_iter().chain(v.doubles.iter()) {
            assert!(inside(singer, sp).is_empty(), "line {li}");
        }
    }
    assert_eq!(a.vocals.lead.notes.len(), p.comp.lead.len() - choir_notes);
}

#[test]
fn the_choir_carries_the_words() {
    let s = song_of(choir_json());
    let p = prep(&s);
    let a = arrange(&s, &p, 1234);
    let words = line_notes(&p);
    assert!(!words.is_empty());
    for (part, singers) in a.vocals.choir.iter().enumerate() {
        assert_eq!(singers.len(), 3, "part {part}");
        for singer in singers {
            assert_eq!(singer.voice, arrange::vocals::CHOIR_VOICES[part]);
            for li in choir_lines(&p) {
                let want: Vec<_> = p.comp.lead.iter().filter(|n| n.line_idx == li).collect();
                let got = inside(singer, span(&p, li));
                assert_eq!(got.len(), want.len(), "part {part} line {li}");
                for (g, w) in got.iter().zip(&want) {
                    assert_eq!(g.phones, w.syl.phones);
                    assert_eq!(g.legato, w.syl.is_continuation());
                }
                assert!(got[0].phrase_start);
            }
        }
    }
    // The melisma has continuation notes.
    assert!(words.iter().any(|w| w.note.syl.is_continuation()));
}

#[test]
fn unison_parts_sing_the_tune_in_their_ranges() {
    let s = song_of(choir_json());
    let p = prep(&s);
    let uni: Vec<_> = line_notes(&p)
        .into_iter()
        .filter(|w| matches!(p.form.lines[w.note.line_idx].part, Part::Choir(v) if v == song::ChoirVoicing::Unison))
        .collect();
    assert!(!uni.is_empty());
    for part in 0..4 {
        let shift = uni[0].midi[part] - uni[0].note.midi;
        assert_eq!(shift.rem_euclid(12), 0);
        assert!(uni.iter().all(|w| w.midi[part] - w.note.midi == shift));
        let mut m: Vec<i32> = uni.iter().map(|w| w.midi[part]).collect();
        m.sort_unstable();
        let med = m[m.len() / 2];
        let r = arrange::vocals::CHOIR_VOICES[part].range();
        assert!(
            med >= r.lo as i32 && med <= r.hi as i32,
            "part {part} median {med}"
        );
    }
}

#[test]
fn block_parts_stack_below_the_tune() {
    let s = song_of(choir_json());
    let p = prep(&s);
    let blk: Vec<_> = line_notes(&p)
        .into_iter()
        .filter(|w| matches!(p.form.lines[w.note.line_idx].part, Part::Choir(v) if v == song::ChoirVoicing::Block))
        .collect();
    assert!(!blk.is_empty());
    let mut sops: Vec<i32> = blk.iter().map(|w| w.midi[3]).collect();
    sops.sort_unstable();
    let r = Voice::Soprano.range();
    let med = sops[sops.len() / 2];
    assert!(med >= r.lo as i32 && med <= r.hi as i32, "soprano {med}");
    let shift = blk[0].midi[3] - blk[0].note.midi;
    assert_eq!(shift.rem_euclid(12), 0);
    for w in &blk {
        let [b, t, a, sp] = w.midi;
        assert_eq!(sp, w.note.midi + shift, "tune on top");
        assert!(b < t && t < a && a < sp, "no crossing: {:?}", w.midi);
        let ch = p.timeline.chord_at(&p.form, w.note.beat + 0.01);
        assert!(ch.tones.contains(Pc::new(a)) && ch.tones.contains(Pc::new(t)));
        assert_eq!(Pc::new(b), ch.bass, "bass on the root");
    }
}

#[test]
fn the_pad_is_silent_over_choir_lines() {
    let s = song_of(choir_json());
    let p = prep(&s);
    let lines = choir_lines(&p);
    let bpb = p.form.meter.grid().beats as usize;
    let all = arrange::choir::voicings(&p.form, &p.timeline, arrange::choir::sings_here);
    let clear = voicings_clear(&p.form, &p.timeline, arrange::choir::sings_here);
    assert!(!clear.is_empty(), "the repeated chorus keeps a pad");
    assert!(clear.len() < all.len(), "some pad segments were dropped");
    for cv in &clear {
        let sg = &p.timeline.segs[cv.seg];
        for &li in &lines {
            let l = &p.form.lines[li];
            assert!(
                sg.b1 <= (l.start_bar * bpb) as f64
                    || sg.b0 >= ((l.start_bar + l.n_bars) * bpb) as f64,
                "pad over line {li}"
            );
        }
    }
    // A singer's notes over a line are only the line's words.
    let a = arrange(&s, &p, 1234);
    let (b0, b1) = {
        let l = &p.form.lines[lines[0]];
        (
            p.timeline.to_time((l.start_bar * bpb) as f64),
            p.timeline.to_time(((l.start_bar + l.n_bars) * bpb) as f64),
        )
    };
    let n_words = p
        .comp
        .lead
        .iter()
        .filter(|n| n.line_idx == lines[0])
        .count();
    let got = a.vocals.choir[3][0]
        .notes
        .iter()
        .filter(|n| n.t0 > b0 - 0.05 && n.t0 < b1 - 0.1)
        .count();
    assert_eq!(got, n_words);
}

#[test]
fn a_song_without_choir_lines_has_no_word_notes() {
    let mut v = choir_json();
    for sec in v["sections"].as_array_mut().unwrap() {
        if let Some(ls) = sec["lines"].as_array_mut() {
            for l in ls {
                l.as_object_mut().unwrap().remove("sing");
                l.as_object_mut().unwrap().remove("voicing");
            }
        }
    }
    let s = song_of(v);
    let p = prep(&s);
    assert!(line_notes(&p).is_empty());
    let a = arrange(&s, &p, 1234);
    for part in &a.vocals.choir {
        for singer in part {
            assert!(singer.notes.iter().all(|n| n.phones.len() == 1));
        }
    }
}

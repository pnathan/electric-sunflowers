//! The song sheet against the preparation the renderer uses.

use compose::prepare::prepare;
use engine::{demo_song, song_sheet, SheetChord, SongSheet};
use song::Voice;

fn chords(sheet: &SongSheet) -> Vec<&SheetChord> {
    sheet
        .sections
        .iter()
        .flat_map(|s| s.lines.iter().flat_map(|l| l.chords.iter()))
        .collect()
}

/// Every lead note appears once, in order, with its time and pitch; the
/// syllable offsets index the line text.
fn check_syllables(seed: u64, voice: Option<Voice>) {
    let song = demo_song();
    let sheet = song_sheet(song, seed, voice);
    let p = prepare(song, seed, voice);
    let syls: Vec<_> = sheet
        .sections
        .iter()
        .flat_map(|s| s.lines.iter().flat_map(|l| l.syllables.iter()))
        .collect();
    let n_form: usize = p.form.lines.iter().map(|l| l.syls.len()).sum();
    assert_eq!(syls.len(), n_form, "seed {seed}");
    assert_eq!(syls.len(), p.comp.lead.len(), "seed {seed}");
    for (s, n) in syls.iter().zip(&p.comp.lead) {
        assert_eq!(s.text, n.syl.text);
        assert_eq!(
            (s.t0, s.t1, s.midi, s.stress),
            (n.t0, n.t1, n.midi, n.stress),
            "seed {seed} syllable {}",
            s.text
        );
    }
    assert_eq!(sheet.voice, p.voice);
    assert_eq!(sheet.key_shift, p.key_shift);
    assert_eq!(sheet.duration_s, p.timeline.end);
    for l in sheet.sections.iter().flat_map(|s| s.lines.iter()) {
        let text: Vec<char> = l.text.chars().collect();
        for s in &l.syllables {
            let got: String = text[s.at..s.at + s.text.chars().count()].iter().collect();
            assert_eq!(got, s.text);
            assert!(l.words[s.word].text.contains(&s.text));
        }
        for c in &l.chords {
            assert!(c.at <= text.len());
        }
    }
}

#[test]
fn every_syllable_once_at_the_lead_note_times() {
    check_syllables(7, None);
    for (k, v) in Voice::ALL.iter().enumerate() {
        check_syllables(1000 + k as u64, Some(*v));
    }
}

#[test]
fn chords_follow_the_timeline() {
    let song = demo_song();
    let sheet = song_sheet(song, 7, None);
    let p = prepare(song, 7, None);
    let cs = chords(&sheet);
    for w in cs.windows(2) {
        assert!(
            w[0].t <= w[1].t,
            "{} at {} then {} at {}",
            w[0].name,
            w[0].t,
            w[1].name,
            w[1].t
        );
    }
    // The chord changes, restatements left out, are the timeline segments.
    let changes: Vec<(String, f64)> = cs
        .iter()
        .filter(|c| !c.carried)
        .map(|c| (c.name.clone(), c.t))
        .collect();
    let segs: Vec<(String, f64)> = p
        .timeline
        .segs
        .iter()
        .map(|s| {
            (
                p.form.chord(s.chord).symbol.clone(),
                p.timeline.to_time(s.b0),
            )
        })
        .collect();
    assert_eq!(changes, segs);
    // Each chord starts under a syllable covering its onset, or on a rest.
    for l in sheet.sections.iter().flat_map(|s| s.lines.iter()) {
        for c in &l.chords {
            if let Some(s) = l.syllables.iter().find(|s| s.at == c.at) {
                let next = l
                    .syllables
                    .iter()
                    .find(|x| x.t0 > s.t0)
                    .map_or(f64::INFINITY, |x| x.t0);
                assert!(
                    s.t0 <= c.t + 1e-9 && c.t < next + 1e-9,
                    "{} at {} over {}",
                    c.name,
                    c.t,
                    s.text
                );
            }
        }
    }
}

#[test]
fn lines_and_sections_run_forward() {
    let sheet = song_sheet(demo_song(), 7, None);
    let lines: Vec<_> = sheet.sections.iter().flat_map(|s| s.lines.iter()).collect();
    for w in lines.windows(2) {
        assert!(w[0].t0 < w[1].t0);
    }
    for w in sheet.sections.windows(2) {
        assert!(w[0].t0 < w[1].t0);
        assert!((w[0].t1 - w[1].t0).abs() < 1e-9);
    }
    let labels: Vec<&str> = sheet.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Intro", "Verse 1", "Chorus 1", "Verse 2", "Chorus 2", "Bridge", "Chorus 3", "Outro"]
    );
    for s in sheet.sections.iter().filter(|s| !s.sung) {
        assert_eq!(s.lines.len(), 1);
        assert!(s.lines[0].text.is_empty() && !s.lines[0].chords.is_empty());
        assert!(!s.bars.is_empty());
    }
}

#[test]
fn text_sheet_has_every_label_and_word() {
    let sheet = song_sheet(demo_song(), 7, None);
    let text = sheet.to_text();
    for s in &sheet.sections {
        assert!(text.contains(&format!("[{}]", s.label)));
        for w in s.lines.iter().flat_map(|l| l.words.iter()) {
            assert!(text.contains(&w.text), "{} missing", w.text);
        }
    }
    let json = serde_json::to_string(&sheet).expect("sheet serialises");
    assert!(json.contains("\"sections\""));
}

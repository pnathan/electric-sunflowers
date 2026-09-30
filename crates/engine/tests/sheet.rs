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

/// A melisma is one sheet syllable with the first note's time and its note
/// count; the continuation notes are not listed, and the line covers them.
#[test]
fn a_melisma_is_one_sheet_syllable() {
    let raw = serde_json::json!({
        "schema_version":2,"title":"Melisma","key":"G","mode":"major","meter":"4/4","tempo":92,
        "sections":[{"type":"verse","lines":[{"syl":"*glo~3-ry *hal~4-le~-lu~ *jah","chords":["G","C","D","G"]}]}]
    });
    let song = song::normalize_value(&raw).expect("melisma song").0;
    let sheet = song_sheet(&song, 6, None);
    let p = prepare(&song, 6, None);
    let line = &sheet.sections[0].lines[0];
    let texts: Vec<&str> = line.syllables.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, ["glo", "ry", "hal", "le", "lu", "jah"]);
    let notes: Vec<u8> = line.syllables.iter().map(|s| s.notes).collect();
    assert_eq!(notes, [3, 1, 4, 2, 2, 1]);
    assert_eq!(line.text, "glory hallelu jah");
    // The first note's time and pitch; the line reaches the last note's end.
    let lead = &p.comp.lead;
    assert_eq!(
        (line.syllables[0].t0, line.syllables[0].t1),
        (lead[0].t0, lead[0].t1)
    );
    assert_eq!(line.syllables[0].midi, lead[0].midi);
    assert!(line.t1 >= lead[lead.len() - 1].t1);
    // The word "glory" covers the melisma's notes.
    assert_eq!(line.words[0].text, "glory");
    assert!(line.words[0].t1 >= lead[3].t1);
    assert!(sheet.to_text().contains("glory hallelu jah"));
    let json = serde_json::to_value(&sheet).unwrap();
    assert_eq!(json["sections"][0]["lines"][0]["syllables"][0]["notes"], 3);
    // Every ordinary syllable of the demo song has notes 1.
    let demo = song_sheet(demo_song(), 1, None);
    assert!(demo
        .sections
        .iter()
        .flat_map(|s| &s.lines)
        .flat_map(|l| &l.syllables)
        .all(|s| s.notes == 1));
}

/// C major, then a chorus in D major, then a copied verse in E major.
fn modulating() -> song::Song {
    let v = serde_json::json!({
        "schema_version":2,"key":"C","mode":"major","meter":"4/4","tempo":100,
        "sections":[
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
            {"type":"chorus","key":"D","lines":[{"syl":"*five *six *seven *eight","chords":["D A"]}]},
            {"type":"verse","same":true,"key":"E"},
            {"type":"chorus","same":true}
        ]
    });
    song::normalize_value(&v).unwrap().0
}

#[test]
fn sections_carry_their_key_and_the_text_says_when_it_changes() {
    let s = modulating();
    let sheet = song_sheet(&s, 4, None);
    let p = prepare(&s, 4, None);
    let name = |tonic: i32, mode: song::Mode| {
        let t = song::Pc::new(tonic).transpose(p.key_shift);
        t.name(mode.prefers_flats(t)).to_string()
    };
    let want = [
        (name(0, song::Mode::Major), song::Mode::Major),
        (name(2, song::Mode::Major), song::Mode::Major),
        (name(4, song::Mode::Major), song::Mode::Major),
        // The last chorus copies the chorus, sounding in the running key E.
        (name(4, song::Mode::Major), song::Mode::Major),
    ];
    let got: Vec<_> = sheet
        .sections
        .iter()
        .map(|x| (x.key.clone(), x.mode))
        .collect();
    assert_eq!(got, want);
    // The song's own key stays the opening key.
    assert_eq!(sheet.key, want[0].0);
    let text = sheet.to_text();
    let lines: Vec<&str> = text.lines().collect();
    let key_lines: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with("Key: "))
        .map(|(i, l)| (i, *l))
        .collect();
    assert_eq!(key_lines.len(), 2, "{text}");
    assert_eq!(key_lines[0].1, format!("Key: {} major", want[1].0));
    assert_eq!(key_lines[1].1, format!("Key: {} major", want[2].0));
    // Each sits directly above its section's header.
    assert!(lines[key_lines[0].0 + 1].starts_with("[Chorus"), "{text}");
    assert!(lines[key_lines[1].0 + 1].starts_with("[Verse 2"), "{text}");
    // The JSON carries the fields.
    let j = serde_json::to_value(&sheet).unwrap();
    assert_eq!(j["sections"][1]["key"], want[1].0);
    assert_eq!(j["sections"][1]["mode"], "major");
}

#[test]
fn a_song_without_key_change_prints_no_key_line() {
    let sheet = song_sheet(demo_song(), 7, None);
    assert!(sheet
        .sections
        .iter()
        .all(|s| s.key == sheet.key && s.mode == sheet.mode));
    assert!(!sheet.to_text().contains("Key: "));
}

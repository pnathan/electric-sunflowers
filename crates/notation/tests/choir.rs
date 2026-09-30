//! Choir lines (schema 2, issue #7) in the notation: the lead sheet labels
//! the system "Choir" and keeps the melody; the full score rests the lead
//! and gives the four choir staves the line's notes and words.

use arrange::arrange;
use compose::prepare::prepare;
use notation::full::{FullScore, PartId};
use notation::{engrave, Score, Sheet};
use song::Song;

fn choir_json(choir_line: bool) -> serde_json::Value {
    let mut chorus = serde_json::json!([
        {"syl": "*heave *ho *heave *ho", "ph": "hh iy v|hh ow|hh iy v|hh ow", "chords": ["G", "C"]},
        {"syl": "*roll *ye *bold", "ph": "r ow l|y iy|b ow l d", "chords": ["C", "D"]},
        {"syl": "*hal~le~lu~3 *jah", "ph": "hh ae|l ah|l uw|jh aa", "chords": ["G", "D"]}]);
    if choir_line {
        chorus[1]["sing"] = "choir".into();
        chorus[2]["sing"] = "choir".into();
        chorus[2]["voicing"] = "block".into();
    }
    serde_json::json!({
        "schema_version": 2,
        "title": "Shanty", "note": "", "key": "G", "mode": "major", "meter": "4/4", "tempo": 92,
        "guitar": "strum", "voice": "baritone",
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false,
                 "violin": false, "choir": choir_line, "harmonies": false, "doubles": false},
        "sections": [
            {"type": "verse", "lines": [
                {"syl": "*wind *fills the *sail", "ph": "w ih n d|f ih l z|dh ah|s ey l", "chords": ["G", "C"]}]},
            {"type": "chorus", "lines": chorus}
        ]
    })
}

fn song_of(choir_line: bool) -> Song {
    song::normalize_value(&choir_json(choir_line))
        .expect("song reads")
        .0
}

const CHOIR_PARTS: [PartId; 4] = [
    PartId::ChoirS,
    PartId::ChoirA,
    PartId::ChoirT,
    PartId::ChoirB,
];

#[test]
fn the_lead_sheet_labels_choir_systems() {
    let s = song_of(true);
    let p = prepare(&s, 1234, None);
    let svg = engrave(&Score::new(&s, &p));
    assert!(svg.contains(">Choir<"), "no Choir label");
    assert!(svg.contains("roll"), "the lyric is on the sheet");
    let plain = song_of(false);
    let p = prepare(&plain, 1234, None);
    assert!(!engrave(&Score::new(&plain, &p)).contains(">Choir<"));
}

#[test]
fn the_full_score_gives_choir_lines_to_the_choir_staves() {
    let s = song_of(true);
    let p = prepare(&s, 1234, None);
    let a = arrange(&s, &p, 1234);
    let score = FullScore::new(&s, &p, &a);
    let lead = score
        .staves
        .iter()
        .position(|x| x.part == PartId::Lead)
        .expect("lead staff");
    let words = arrange::choir::line_notes(&p);
    let choir_lines: Vec<usize> = (0..p.form.lines.len())
        .filter(|&i| p.form.lines[i].part.is_choir())
        .collect();
    assert_eq!(choir_lines.len(), 2);

    for &li in &choir_lines {
        let l = &p.form.lines[li];
        let bars = l.start_bar..l.start_bar + l.n_bars;
        // The lead rests.
        for b in bars.clone() {
            assert!(
                score.bars[b].cells[lead].voices[0]
                    .iter()
                    .all(|e| e.chord.is_none()),
                "lead sings in bar {b}"
            );
        }
        let want: Vec<&str> = words
            .iter()
            .filter(|w| w.note.line_idx == li)
            .filter(|w| !w.note.syl.is_continuation())
            .map(|w| w.note.syl.text.as_str())
            .collect();
        assert!(!want.is_empty());
        for part in CHOIR_PARTS {
            let idx = score
                .staves
                .iter()
                .position(|x| x.part == part)
                .expect("choir staff");
            assert!(score.staves[idx].lyrics, "{part:?} carries lyrics");
            let got: Vec<&str> = bars
                .clone()
                .flat_map(|b| score.bars[b].cells[idx].voices[0].iter())
                .filter_map(|e| e.chord.as_ref())
                .filter_map(|c| c.lyric.as_deref())
                .collect();
            assert_eq!(got, want, "{part:?} line {li}");
        }
    }
    // Every bar is still full on every staff, and the page lays out.
    let bar_u: i64 = score.bars[0].cells[0].voices[0].iter().map(|e| e.d).sum();
    let svg = Sheet::Full(score.clone()).page(notation::DEFAULT_WIDTH).svg;
    assert!(svg.contains("roll"));
    for b in &score.bars {
        for c in &b.cells {
            let total: i64 = c.voices[0].iter().map(|e| e.d).sum();
            assert_eq!(total, bar_u);
        }
    }
}

#[test]
fn choir_staves_of_a_song_without_choir_lines_carry_no_lyrics() {
    let s = song_of(false);
    let p = prepare(&s, 1234, None);
    let a = arrange(&s, &p, 1234);
    let score = FullScore::new(&s, &p, &a);
    for st in &score.staves {
        assert_eq!(
            st.lyrics,
            matches!(st.part, PartId::Lead | PartId::LeadB),
            "{:?}",
            st.part
        );
    }
}

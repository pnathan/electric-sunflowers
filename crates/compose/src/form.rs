//! Form: lays the song's sections, lines and bars out on the bar grid, and
//! marks each section's lift (the sections that carry the hook), intensity
//! and role. Every cross-reference is an index into the owning Vec.

use std::ops::Range;

use song::chord::transpose_symbol;
use song::{
    BarChords, Chord, ChordId, Energy, Meter, MeterGrid, Mode, Part, Pc, Phoneme, Rubato,
    SectionBody, SectionKind, SectionRole, Song, Syllable,
};

/// A metric bar: its chord(s) plus the section and line it belongs to.
#[derive(Clone, Debug)]
pub struct Bar {
    /// Ids index `Form::chords`.
    pub chords: BarChords,
    pub sec: usize,
    pub line: Option<usize>,
}

/// A lyric line placed in the form: syllables plus its bar span.
#[derive(Clone, Debug)]
pub struct FormLine {
    pub sec: usize,
    /// Line index within its section.
    pub li: usize,
    pub start_bar: usize,
    pub n_bars: usize,
    pub syls: Vec<Syllable>,
    /// Which singer(s) carry this line (design 4.5); `Part::default()`
    /// (`Solo(A)`) outside a duet.
    pub part: Part,
    /// Syllable texts joined by single spaces.
    pub text: String,
    /// The writer's tune for the line (schema 3), one entry per note of
    /// `syls`.
    pub tune: Option<Vec<song::TuneNote>>,
    /// Set by `compose_melody`; `None` until composed.
    pub pitches: Option<Vec<i32>>,
    /// Set by `compose_melody`; `None` until composed.
    pub rh: Option<crate::rhythm::RhythmResult>,
}

/// Arrangement intensity of a section, 0 (quiet) to 3 (high).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Intensity {
    Quiet = 0,
    Low = 1,
    Mid = 2,
    High = 3,
}

impl Intensity {
    /// The level as an integer, 0-3.
    pub const fn level(self) -> i32 {
        self as i32
    }
}

/// A lifted section: a chorus, or, in a form with no chorus, a verse after
/// the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lift {
    /// 0 for the first lifted section, counting up (saturates at 255).
    pub index: u8,
    /// The last lifted section of the song.
    pub is_final: bool,
}

/// A section placed in the form.
#[derive(Clone, Debug)]
pub struct Sec {
    pub kind: SectionKind,
    pub role: SectionRole,
    /// Occurrence count of this kind before this section.
    pub occ: usize,
    pub start_bar: usize,
    pub n_bars: usize,
    /// Indices into `Form::lines`.
    pub lines: Vec<usize>,
    pub lift: Option<Lift>,
    pub intensity: Intensity,
    /// The section's rubato (its own, else the song's).
    pub rubato: Rubato,
    /// The key in force in this section, transposed by the form's
    /// `transpose`: the song's key until a section changes it.
    pub key: (Pc, Mode),
    /// The writer's tune for an instrumental section (schema 3).
    pub break_tune: Option<Vec<song::BreakNote>>,
}

impl Sec {
    pub fn is_lift(&self) -> bool {
        self.lift.is_some()
    }

    /// A lifted section after the first (a repeated chorus).
    pub fn is_repeat_lift(&self) -> bool {
        matches!(self.lift, Some(l) if l.index > 0)
    }

    pub fn is_sung(&self) -> bool {
        !self.lines.is_empty()
    }

    /// The section's span in beats.
    pub fn beats(&self, meter: &Meter) -> Range<f64> {
        let bpb = meter.grid().beats as usize;
        (self.start_bar * bpb) as f64..((self.start_bar + self.n_bars) * bpb) as f64
    }
}

/// The song on the bar grid.
#[derive(Clone, Debug)]
pub struct Form {
    pub meter: Meter,
    pub bars: Vec<Bar>,
    pub lines: Vec<FormLine>,
    pub sections: Vec<Sec>,
    /// The song's chords transposed by `transpose`, indexed by the song's
    /// `ChordId`. Bars hold, for each chord, the first id with the same
    /// transposed symbol, so equal symbols compare equal as ids.
    pub chords: Vec<Chord>,
    pub transpose: i32,
    /// Bars per written bar: 2 when dense lines are spread over twice the bars.
    pub stretch: i32,
}

impl Form {
    pub fn grid(&self) -> &'static MeterGrid {
        self.meter.grid()
    }

    /// Beats per bar.
    pub fn bpb(&self) -> i32 {
        self.meter.grid().beats as i32
    }

    /// Grid slots per beat.
    pub fn sub(&self) -> i32 {
        self.meter.grid().sub as i32
    }

    /// Beat within a bar at which a two-chord bar changes chord.
    pub fn split(&self) -> i32 {
        self.meter.grid().split as i32
    }

    pub fn chord(&self, id: ChordId) -> &Chord {
        &self.chords[id.index()]
    }
}

/// A written bar as played: one bar unstretched; stretched, a two-chord bar
/// becomes one bar per chord and a one-chord bar is played twice.
fn expand(b: BarChords, stretch: i32) -> ([BarChords; 2], usize) {
    if stretch == 1 {
        return ([b, b], 1);
    }
    if b.len() == 2 {
        let s = b.as_slice();
        ([BarChords::one(s[0]), BarChords::one(s[1])], 2)
    } else {
        ([b, b], 2)
    }
}

/// The song's chords transposed by `transpose` semitones (spelled with
/// flats when the new key reads better with them), and for each pool entry
/// the first id with the same transposed symbol. Entries `0..song.chords`
/// follow the song's ids; `respell` may append more.
fn transposed_chords(song: &Song, transpose: i32) -> (Vec<Chord>, Vec<ChordId>) {
    let flats = song.mode.prefers_flats(song.key.transpose(transpose));
    let mut chords: Vec<Chord> = Vec::with_capacity(song.chords.len());
    let mut canon: Vec<ChordId> = Vec::with_capacity(song.chords.len());
    for (id, c) in song.chords.iter() {
        let t = spell(c, transpose, flats);
        push_canonical(&mut chords, &mut canon, t, id);
    }
    (chords, canon)
}

/// `c` moved by `transpose` semitones, spelled with flats or sharps.
fn spell(c: &Chord, transpose: i32, flats: bool) -> Chord {
    if transpose == 0 {
        c.clone()
    } else {
        // A transposed symbol still starts with a note name, so it parses.
        Chord::parse(&transpose_symbol(&c.symbol, transpose, flats)).unwrap_or_else(|_| c.clone())
    }
}

/// Appends `t` to the pool; its canonical id is the first entry with the
/// same symbol, else `own`.
fn push_canonical(chords: &mut Vec<Chord>, canon: &mut Vec<ChordId>, t: Chord, own: ChordId) {
    let first = chords
        .iter()
        .position(|x| x.symbol == t.symbol)
        .map_or(own, |i| canon[i]);
    chords.push(t);
    canon.push(first);
}

/// The canonical id of each song chord spelled with `flats`, for a section
/// whose key spells differently from the pool's base spelling. A symbol not
/// yet in the pool is appended, so ids stay canonical by symbol.
fn respell(
    song: &Song,
    transpose: i32,
    flats: bool,
    chords: &mut Vec<Chord>,
    canon: &mut Vec<ChordId>,
) -> Vec<ChordId> {
    song.chords
        .iter()
        .map(|(_, c)| {
            let t = spell(c, transpose, flats);
            if let Some(i) = chords.iter().position(|x| x.symbol == t.symbol) {
                return canon[i];
            }
            let own = ChordId::from_index(chords.len()).expect("chord pool within id range");
            push_canonical(chords, canon, t, own);
            own
        })
        .collect()
}

/// Notes a line sings: a melisma syllable counts its notes.
fn note_count(syls: &[Syllable]) -> usize {
    syls.iter().map(|s| (s.notes as usize).max(1)).sum()
}

/// `syls` with each melisma syllable (`notes = N > 1`) expanded to N note
/// syllables. The first keeps the text, stress, word start and `notes = N`;
/// the others are continuations (`notes = 0`, no text, unstressed, no word
/// start), and only the last carries the word end. Phones split at the
/// nucleus: the first note has the onset consonants, the last the coda,
/// and every note but the last holds the first vowel target of the nucleus
/// (a diphthong glides only on the last note). A line with no melisma is
/// returned unchanged.
pub fn expand_melismas(syls: &[Syllable]) -> Vec<Syllable> {
    let mut out = Vec::with_capacity(note_count(syls));
    for s in syls {
        let n = s.notes as usize;
        if n < 2 {
            out.push(s.clone());
            continue;
        }
        let first = s.phones.iter().position(|p| p.is_vowel());
        let last = s.phones.iter().rposition(|p| p.is_vowel());
        let (onset, nucleus, coda) = match (first, last) {
            (Some(a), Some(b)) => (&s.phones[..a], &s.phones[a..=b], &s.phones[b + 1..]),
            _ => (&s.phones[..0], &[Phoneme::Aa][..], &s.phones[..]),
        };
        let hold = match nucleus[0].diphthong_targets() {
            Some([v, _]) => v,
            None => nucleus[0],
        };
        for k in 0..n {
            let mut x = s.clone();
            let mut ph: Vec<Phoneme> = Vec::new();
            if k == 0 {
                ph.extend_from_slice(onset);
            } else {
                x.text = String::new();
                x.stress = false;
                x.word_start = false;
                x.notes = 0;
            }
            if k + 1 == n {
                ph.extend_from_slice(nucleus);
                ph.extend_from_slice(coda);
            } else {
                ph.push(hold);
                x.word_end = false;
            }
            x.phones = ph;
            out.push(x);
        }
    }
    out
}

/// Lays out `song` with its chords transposed by `transpose` semitones.
pub fn build_form(song: &Song, transpose: i32) -> Form {
    let meter = song.meter;
    let grid = meter.grid();
    let (mut chords, mut canon) = transposed_chords(song, transpose);
    let base_flats = song.mode.prefers_flats(song.key.transpose(transpose));
    // Spelling of each song chord per section spelling, when the song
    // modulates and a section's key reads differently from the song's.
    let mut alt: Option<Vec<ChordId>> = None;
    let map = |ids: &[ChordId], b: &BarChords| -> BarChords {
        let s = b.as_slice();
        if s.len() == 2 {
            BarChords::two(ids[s[0].index()], ids[s[1].index()])
        } else {
            BarChords::one(ids[s[0].index()])
        }
    };

    let mut bars: Vec<Bar> = Vec::new();
    let mut lines: Vec<FormLine> = Vec::new();
    let mut sections: Vec<Sec> = Vec::new();
    let mut occ_of = [0usize; SectionKind::ALL.len()];

    // Hypermetric stretch: spread dense lines over twice the bars.
    let mut ns = 0.0f64;
    let mut nl = 0i32;
    for s in &song.sections {
        for ln in s.lines() {
            ns += note_count(&ln.syllables) as f64 / ln.bars.len() as f64;
            nl += 1;
        }
    }
    let per_bar = if nl != 0 { ns / nl as f64 } else { 4.0 };
    let slots = grid.slots() as f64;
    let bar_dur = grid.beats as f64 * 60.0 / song.tempo_bpm;
    let stretch = if slots / per_bar < 1.75 && bar_dur * 4.0 <= 8.4 {
        2
    } else {
        1
    };

    for (si, s) in song.sections.iter().enumerate() {
        let o = occ_of[s.kind as usize];
        occ_of[s.kind as usize] = o + 1;
        let sec_idx = sections.len();
        let sec_start_bar = bars.len();
        let (sec_tonic, sec_mode) = song.key_at(si);
        let respelled = transpose != 0
            && song.modulates()
            && sec_mode.prefers_flats(sec_tonic.transpose(transpose)) != base_flats;
        if respelled && alt.is_none() {
            alt = Some(respell(
                song,
                transpose,
                !base_flats,
                &mut chords,
                &mut canon,
            ));
        }
        let ids: Vec<ChordId> = match (&alt, respelled) {
            (Some(a), true) => a.clone(),
            _ => song
                .chords
                .iter()
                .map(|(id, _)| canon[id.index()])
                .collect(),
        };
        let mut sec_lines: Vec<usize> = Vec::new();
        match &s.body {
            SectionBody::Sung(ls) => {
                for (li, ln) in ls.iter().enumerate() {
                    let start_bar = bars.len();
                    for b in &ln.bars {
                        let (xs, n) = expand(map(&ids, b), stretch);
                        for x in &xs[..n] {
                            bars.push(Bar {
                                chords: *x,
                                sec: sec_idx,
                                line: Some(lines.len()),
                            });
                        }
                    }
                    sec_lines.push(lines.len());
                    lines.push(FormLine {
                        sec: sec_idx,
                        li,
                        start_bar,
                        n_bars: bars.len() - start_bar,
                        syls: expand_melismas(&ln.syllables),
                        part: ln.part,
                        text: ln.text(),
                        tune: ln.tune.clone(),
                        pitches: None,
                        rh: None,
                    });
                }
            }
            SectionBody::Instrumental(bs) => {
                for b in bs {
                    let (xs, n) = expand(map(&ids, b), stretch);
                    for x in &xs[..n] {
                        bars.push(Bar {
                            chords: *x,
                            sec: sec_idx,
                            line: None,
                        });
                    }
                }
            }
        }
        sections.push(Sec {
            kind: s.kind,
            role: s.role,
            occ: o,
            start_bar: sec_start_bar,
            n_bars: bars.len() - sec_start_bar,
            lines: sec_lines,
            lift: None,
            intensity: Intensity::Quiet,
            rubato: song.rubato_at(si),
            key: {
                let (tonic, mode) = song.key_at(si);
                (tonic.transpose(transpose), mode)
            },
            break_tune: s.break_tune.clone(),
        });
    }

    // Lifts: every chorus, or with no sung chorus every verse after the first.
    let has_chorus = sections
        .iter()
        .any(|s| s.kind == SectionKind::Chorus && s.is_sung());
    let lifted = |s: &Sec| {
        if has_chorus {
            s.kind == SectionKind::Chorus
        } else {
            s.kind == SectionKind::Verse && s.occ > 0
        }
    };
    let lift_idx: Vec<Option<usize>> = {
        let mut k = 0usize;
        sections
            .iter()
            .map(|s| {
                lifted(s).then(|| {
                    k += 1;
                    k - 1
                })
            })
            .collect()
    };
    let n_lift = lift_idx.iter().flatten().count();
    let last_lift = lift_idx.iter().rposition(Option::is_some);

    for (j, (s, li)) in sections.iter_mut().zip(&lift_idx).enumerate() {
        let mut level = match s.kind {
            SectionKind::Intro => 0,
            SectionKind::Outro => i32::from(s.is_sung()),
            SectionKind::Bridge | SectionKind::Interlude => 1,
            SectionKind::Prechorus => 2,
            SectionKind::Verse => {
                if s.occ == 0 {
                    1
                } else {
                    2
                }
            }
            SectionKind::Chorus => 0,
        };
        if let Some(i) = *li {
            let is_final = Some(j) == last_lift;
            level = if is_final && i > 0 {
                3
            } else if i == 0 {
                2
            } else if has_chorus {
                3
            } else {
                2 + i32::from(i + 2 >= n_lift)
            };
            s.lift = Some(Lift {
                index: u8::try_from(i).unwrap_or(u8::MAX),
                is_final,
            });
        }
        s.intensity = match song.sections.get(j).and_then(|x| x.energy) {
            Some(Energy::Quiet) => Intensity::Quiet,
            Some(Energy::Low) => Intensity::Low,
            Some(Energy::Mid) => Intensity::Mid,
            Some(Energy::High) => Intensity::High,
            None => match level {
                0 => Intensity::Quiet,
                1 => Intensity::Low,
                2 => Intensity::Mid,
                _ => Intensity::High,
            },
        };
    }

    Form {
        meter,
        bars,
        lines,
        sections,
        chords,
        transpose,
        stretch,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn song_of(v: serde_json::Value) -> Song {
        song::normalize_value(&v).expect("test song normalises").0
    }

    #[test]
    fn build_form_basic() {
        let song = song_of(json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[
                {"type":"intro","chords":["C","G"]},
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":["Am F"]}]},
                {"type":"chorus","same":true}
            ]
        }));
        let form = build_form(&song, 0);
        assert_eq!(form.sections.len(), 4);
        assert_eq!(
            form.sections[2].lift,
            Some(Lift {
                index: 0,
                is_final: false
            })
        );
        assert_eq!(
            form.sections[3].lift,
            Some(Lift {
                index: 1,
                is_final: true
            })
        );
        assert!(form.sections[3].is_repeat_lift());
        assert_eq!(form.sections[1].intensity, Intensity::Low);
        assert_eq!(form.sections[1].beats(&form.meter), 8.0..12.0);
    }

    /// A version-2 song with melismas on the chorus and verse lines.
    pub(crate) fn melisma_song() -> Song {
        song_of(json!({
            "schema_version":2,"key":"G","mode":"major","meter":"4/4","tempo":92,
            "sections":[
                {"type":"verse","lines":[{"syl":"*glo~3-ry *hal~4-le~-lu~ *jah","chords":["G","C","D","G"]}]},
                {"type":"chorus","lines":[{"syl":"*sing~ out *loud","chords":["C","G"]}]}
            ]
        }))
    }

    #[test]
    fn melisma_expands_to_notes() {
        let song = melisma_song();
        let form = build_form(&song, 0);
        let l = &form.lines[0];
        // glo(3) ry hal(4) le(2) lu(2) jah = 13 notes; text is unexpanded.
        assert_eq!(l.syls.len(), 13);
        assert_eq!(l.text, "glo ry hal le lu jah");
        let notes: Vec<u8> = l.syls.iter().map(|s| s.notes).collect();
        assert_eq!(notes, vec![3, 0, 0, 1, 4, 0, 0, 0, 2, 0, 2, 0, 1]);
        for (i, s) in l.syls.iter().enumerate() {
            if s.is_continuation() {
                assert!(s.text.is_empty() && !s.stress && !s.word_start, "note {i}");
                // Vowel only, except the last note, which may carry a coda.
                assert!(s.phones.first().is_some_and(|p| p.is_vowel()));
            }
        }
        // "glo": onset consonants on the first note, the vowel alone after.
        assert!(l.syls[0].phones.first().is_some_and(|p| p.is_consonant()));
        assert!(l.syls[0].phones.last().is_some_and(|p| p.is_vowel()));
        assert_eq!(l.syls[1].phones.len(), 1);
        assert_eq!(l.syls[2].phones.len(), 1);
        assert_eq!(l.syls[1].phones[0], *l.syls[0].phones.last().unwrap());
        // "glo-ry" is one word: no note of "glo" ends it, "ry" does.
        assert!(!l.syls[2].word_end && l.syls[3].word_end);
        // "hal": the coda /l/ moves to the last of its four notes.
        let coda = |i: usize| l.syls[i].phones.iter().filter(|p| p.is_consonant()).count();
        assert_eq!((coda(4), coda(5), coda(6)), (1, 0, 0));
        assert!(l.syls[7].phones.last().is_some_and(|p| p.is_consonant()));
        // "hallelu" is one word: only its last note (of "lu") ends it.
        assert!(!l.syls[7].word_end && !l.syls[6].word_end);
        assert!(l.syls[11].word_end && !l.syls[10].word_end);
        // The chorus line: "sing" + "out" + "loud" = 2 + 1 + 1 notes.
        assert_eq!(form.lines[1].syls.len(), 4);
    }

    #[test]
    fn a_diphthong_glides_only_on_the_last_note() {
        let song = song_of(json!({
            "schema_version":2,"key":"C","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"*fly~3","ph":"f l ay","chords":["C"]}]}]
        }));
        let syls = &build_form(&song, 0).lines[0].syls;
        assert_eq!(syls.len(), 3);
        assert_eq!(syls[0].phones, vec![Phoneme::F, Phoneme::L, Phoneme::Aa]);
        assert_eq!(syls[1].phones, vec![Phoneme::Aa]);
        assert_eq!(syls[2].phones, vec![Phoneme::Ay]);
    }

    #[test]
    fn a_song_without_melismas_is_unchanged() {
        let song = song_of(json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]}]
        }));
        let form = build_form(&song, 0);
        assert_eq!(form.lines[0].syls, song.sections[0].lines()[0].syllables);
        assert!(form.lines[0].syls.iter().all(|s| s.notes == 1));
    }

    #[test]
    fn transposed_chords_share_ids_by_symbol() {
        let song = song_of(json!({
            "key":"C","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"*one *two","chords":["C# Db"]}]}]
        }));
        let f0 = build_form(&song, 0);
        let s0 = f0.bars[0].chords;
        assert_ne!(s0.first(), s0.last());
        let f2 = build_form(&song, 2);
        let s2 = f2.bars[0].chords;
        assert_eq!(s2.first(), s2.last());
        assert_eq!(f2.chord(s2.first()).symbol, "D#");
    }

    /// A section spells its transposed chords by its own key: D major moved
    /// up a semitone is E-flat (flats), B major moved up is C (sharps).
    #[test]
    fn a_section_spells_chords_by_its_own_key() {
        let song = song_of(json!({
            "schema_version":2,"key":"D","meter":"4/4","tempo":100,
            "sections":[
                {"type":"verse","lines":[{"syl":"*one *two","chords":["D A"]}]},
                {"type":"chorus","key":"B","lines":[{"syl":"*one *two","chords":["A B"]}]},
                {"type":"chorus","same":true}
            ]
        }));
        assert!(song.modulates());
        let form = build_form(&song, 1);
        let sym = |bar: usize| -> Vec<String> {
            form.bars[bar]
                .chords
                .as_slice()
                .iter()
                .map(|&c| form.chord(c).symbol.clone())
                .collect()
        };
        assert_eq!(sym(0), ["Eb", "Bb"]);
        // The chorus is in C major after the move: sharps.
        assert_eq!(sym(1), ["A#", "C"]);
        assert_eq!(sym(2), sym(1));
        // Ids stay canonical by symbol: the repeat shares its ids.
        assert_eq!(form.bars[1].chords, form.bars[2].chords);
        // Without a move, the written symbols stand.
        let f0 = build_form(&song, 0);
        assert_eq!(f0.chord(f0.bars[1].chords.first()).symbol, "A");
    }

    #[test]
    fn a_song_without_key_change_spells_as_before() {
        let song = song_of(json!({
            "key":"D","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"*one *two","chords":["D A"]}]}]
        }));
        let form = build_form(&song, 1);
        assert_eq!(form.chords.len(), song.chords.len());
        let s = form.bars[0].chords;
        assert_eq!(form.chord(s.first()).symbol, "Eb");
        assert_eq!(form.chord(s.last()).symbol, "Bb");
    }
}

#[cfg(test)]
mod energy_tests {
    use super::tests::song_of;
    use super::*;
    use serde_json::json;

    fn levels(energy: [Option<&str>; 4]) -> Vec<Intensity> {
        let mut secs = vec![
            json!({"type": "intro", "chords": ["C", "G"]}),
            json!({"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]}),
            json!({"type": "interlude", "chords": ["C", "G"]}),
            json!({"type": "chorus", "lines": [{"syl": "*three *four", "chords": ["F"]}]}),
        ];
        for (s, e) in secs.iter_mut().zip(energy) {
            if let Some(e) = e {
                s["energy"] = json!(e);
            }
        }
        let song = song_of(json!({
            "schema_version": 3, "key": "C", "mode": "major", "meter": "4/4",
            "tempo": 100, "title": "t", "sections": secs
        }));
        build_form(&song, 0)
            .sections
            .iter()
            .map(|s| s.intensity)
            .collect()
    }

    #[test]
    fn written_energy_sets_the_intensity_and_absence_keeps_the_rules() {
        use Intensity::*;
        assert_eq!(levels([None; 4]), [Quiet, Low, Low, Mid]);
        assert_eq!(
            levels([Some("high"), Some("mid"), Some("high"), None]),
            [High, Mid, High, Mid]
        );
        let f = {
            let song = song_of(json!({
                "schema_version": 3, "key": "C", "tempo": 100, "title": "t",
                "sections": [
                    {"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]},
                    {"type": "chorus", "energy": "quiet",
                     "lines": [{"syl": "*three *four", "chords": ["F"]}]}]
            }));
            build_form(&song, 0)
        };
        // The lift bookkeeping is unchanged by the override.
        assert!(f.sections[1].is_lift());
        assert_eq!(f.sections[1].intensity, Quiet);
    }
}

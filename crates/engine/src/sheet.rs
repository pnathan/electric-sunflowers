//! The song sheet: title, key, sections, lyric lines with syllable times
//! and chords placed over the syllables, as rendered.
//!
//! `song_sheet` prepares the song exactly as `render` does (same seed and
//! voice), so every time here matches the audio. `sheet_from` builds the
//! sheet from a `Prepared` already in hand (the one `render` returns).
//! `SongSheet::to_text` lays the sheet out as a classic chord sheet.

use compose::form::Form;
use compose::melody::LeadNote;
use compose::prepare::{prepare_voices, Prepared, VoiceChoice};
use compose::timeline::Timeline;
use serde::Serialize;
use song::{Meter, Mode, Part, SectionKind, SingerId, Song, Voice};

/// Tolerance in beats when matching a chord onset to a note.
const EPS: f64 = 1e-6;

/// The `<stem>.sheet.json` format version. Version 1 (no `version` key) is
/// what earlier builds wrote; version 2 adds `version`, and per-section
/// `key`, `rubato` and the choir's lines as later steps of the schema-2
/// work define them. A reader ignores unknown fields.
pub const SHEET_VERSION: u32 = 2;

/// A song as sung and played: header fields and sections.
#[derive(Clone, Debug, Serialize)]
pub struct SongSheet {
    /// `SHEET_VERSION`.
    pub version: u32,
    pub title: String,
    /// Liner note.
    pub note: String,
    /// Style key, when a style was applied.
    pub style: Option<String>,
    /// Style label; the engine does not know the style table, so the
    /// caller fills it (`songwriter::styles::style`).
    pub style_label: Option<String>,
    /// Tonic after transposition for the voice, spelled for the mode.
    pub key: String,
    pub mode: Mode,
    /// Semitones from the written key, -5..=6.
    pub key_shift: i32,
    pub meter: Meter,
    /// Beats per minute before the final ritard.
    pub tempo: f64,
    pub voice: Voice,
    /// Singer B's voice; `None` outside a duet.
    pub voice_b: Option<Voice>,
    pub seed: u64,
    /// Length of the rendered audio in seconds, tail included.
    pub duration_s: f64,
    pub sections: Vec<SheetSection>,
}

/// One section of the form.
#[derive(Clone, Debug, Serialize)]
pub struct SheetSection {
    /// "Verse 2", "Chorus", "Intro", "Instrumental".
    pub label: String,
    pub kind: SectionKind,
    /// 1-based occurrence of this kind.
    pub occurrence: usize,
    /// The voice sings in this section.
    pub sung: bool,
    /// The section carries the hook (a chorus, or a later verse when there
    /// is no chorus).
    pub lift: bool,
    /// Tonic of the key in force, after transposition for the voice,
    /// spelled for its mode.
    pub key: String,
    pub mode: Mode,
    pub t0: f64,
    pub t1: f64,
    /// Bars as played, with the chord names sounding in each.
    pub bars: Vec<SheetBar>,
    /// Lyric lines; an instrumental section has one line with no text.
    pub lines: Vec<SheetLine>,
}

/// One played bar.
#[derive(Clone, Debug, Serialize)]
pub struct SheetBar {
    pub t0: f64,
    /// One or two chord names.
    pub chords: Vec<String>,
}

/// One lyric line (or the chord line of an instrumental section).
#[derive(Clone, Debug, Serialize)]
pub struct SheetLine {
    /// From the earlier of the line's first bar and its first note.
    pub t0: f64,
    /// To the later of the line's last bar end and its last note end.
    pub t1: f64,
    /// Words joined by single spaces; syllables of a word joined directly.
    pub text: String,
    pub syllables: Vec<SheetSyllable>,
    pub words: Vec<SheetWord>,
    /// In time order.
    pub chords: Vec<SheetChord>,
    /// Which singer(s) carry this line; `None` for an instrumental
    /// section's chord line.
    pub singer: Option<SheetPart>,
}

/// A lyric line's singer(s) (design 4.6): `part` is `"A"`, `"B"` or
/// `"both"`; `melody` is whichever of `"A"`/`"B"` carries the tune; `label`
/// names the voice(s), e.g. "Baritone" or "Baritone + Alto".
#[derive(Clone, Debug, Serialize)]
pub struct SheetPart {
    pub part: String,
    pub melody: String,
    pub label: String,
}

/// `part`'s `SheetPart`, `voice_a`/`voice_b` naming each singer's voice.
fn sheet_part(part: Part, voice_a: Voice, voice_b: Option<Voice>) -> SheetPart {
    let voice_of = |id: SingerId| {
        if id == SingerId::A {
            voice_a
        } else {
            voice_b.unwrap_or(voice_a)
        }
    };
    match part {
        Part::Solo(id) => SheetPart {
            part: id.as_str().to_string(),
            melody: id.as_str().to_string(),
            label: capitalize(voice_of(id).as_str()),
        },
        Part::Choir(_) => SheetPart {
            part: "choir".to_string(),
            melody: SingerId::A.as_str().to_string(),
            label: "Choir".to_string(),
        },
        Part::Both { melody, .. } => SheetPart {
            part: "both".to_string(),
            melody: melody.as_str().to_string(),
            label: format!(
                "{} + {}",
                capitalize(voice_of(SingerId::A).as_str()),
                capitalize(voice_of(SingerId::B).as_str())
            ),
        },
    }
}

/// One sung syllable.
#[derive(Clone, Debug, Serialize)]
pub struct SheetSyllable {
    pub text: String,
    /// Char offset of the syllable in the line's `text`.
    pub at: usize,
    /// Index into the line's `words`.
    pub word: usize,
    pub t0: f64,
    pub t1: f64,
    /// Sung pitch after transposition (the first note's, in a melisma).
    pub midi: i32,
    pub stress: bool,
    /// Notes the syllable is sung over: 1, or 2 to 4 for a melisma
    /// (schema 2). A melisma is one syllable here, with the times and
    /// pitch of its first note; its continuation notes are not listed.
    pub notes: u8,
}

/// One word: its syllables joined.
#[derive(Clone, Debug, Serialize)]
pub struct SheetWord {
    pub text: String,
    pub at: usize,
    pub t0: f64,
    pub t1: f64,
}

/// A chord onset placed over the text.
#[derive(Clone, Debug, Serialize)]
pub struct SheetChord {
    /// Char offset in the line's `text`: the syllable sung at the onset;
    /// on a rest, the end of the syllable before (0 before the first, the
    /// text length after the last).
    pub at: usize,
    pub name: String,
    /// Onset in seconds.
    pub t: f64,
    /// Onset in beats from the song start.
    pub beat: f64,
    /// Bar within the section, 0-based.
    pub bar: usize,
    /// The chord was already sounding from the line before; restated at
    /// the line start.
    pub carried: bool,
}

/// The sheet of `song` rendered with `seed` and `voice` (`None` keeps the
/// song's voice). Same preparation as `render`, so the times match. A thin
/// shim over `song_sheet_with` with `VoiceChoice { a: voice, b: None }`.
pub fn song_sheet(song: &Song, seed: u64, voice: Option<Voice>) -> SongSheet {
    song_sheet_with(song, seed, VoiceChoice { a: voice, b: None })
}

/// `song_sheet`, choosing both singers' voices (`voice.b` ignored outside
/// a duet; see `VoiceChoice`).
pub fn song_sheet_with(song: &Song, seed: u64, voice: VoiceChoice) -> SongSheet {
    let p = prepare_voices(song, seed, voice);
    sheet_from(song, seed, &p)
}

/// The sheet from a preparation of `song` with `seed`.
pub fn sheet_from(song: &Song, seed: u64, p: &Prepared) -> SongSheet {
    let form = &p.form;
    let tl = &p.timeline;
    let flats = song.mode.prefers_flats(song.key.transpose(p.key_shift));
    let bpb = form.bpb() as usize;

    // Lead notes by line, then by syllable index.
    let mut notes: Vec<Vec<Option<&LeadNote>>> = form
        .lines
        .iter()
        .map(|l| vec![None; l.syls.len()])
        .collect();
    for n in &p.comp.lead {
        if let Some(slot) = notes.get_mut(n.line_idx).and_then(|v| v.get_mut(n.i)) {
            *slot = Some(n);
        }
    }

    let mut kind_total = [0usize; SectionKind::ALL.len()];
    for s in &form.sections {
        kind_total[s.kind as usize] += 1;
    }

    let mut sections = Vec::with_capacity(form.sections.len());
    for (si, sec) in form.sections.iter().enumerate() {
        let b0 = sec.start_bar * bpb;
        let b1 = (sec.start_bar + sec.n_bars) * bpb;
        let bars = (sec.start_bar..sec.start_bar + sec.n_bars)
            .filter_map(|bi| form.bars.get(bi).map(|b| (bi, b)))
            .map(|(bi, b)| SheetBar {
                t0: tl.to_time((bi * bpb) as f64),
                chords: b
                    .chords
                    .as_slice()
                    .iter()
                    .map(|&c| form.chord(c).symbol.clone())
                    .collect(),
            })
            .collect();
        let lines = if sec.is_sung() {
            sec.lines
                .iter()
                .filter_map(|&li| form.lines.get(li).map(|l| (li, l)))
                .map(|(li, l)| {
                    let lb0 = l.start_bar * bpb;
                    let lb1 = (l.start_bar + l.n_bars) * bpb;
                    let mut line = sung_line(
                        form,
                        tl,
                        si,
                        sec.start_bar,
                        l.syls.as_slice(),
                        &notes[li],
                        lb0,
                        lb1,
                    );
                    line.singer = Some(sheet_part(l.part, p.voice, p.voice_b));
                    line
                })
                .collect()
        } else {
            let chords = chords_in(form, tl, si, sec.start_bar, b0, b1);
            vec![SheetLine {
                t0: tl.to_time(b0 as f64),
                t1: tl.to_time(b1 as f64),
                text: String::new(),
                syllables: Vec::new(),
                words: Vec::new(),
                chords: chords.into_iter().map(|(c, _)| c).collect(),
                singer: None,
            }]
        };
        let kind_name = capitalize(sec.kind.as_str());
        let label = if !sec.is_sung()
            && !matches!(
                sec.kind,
                SectionKind::Intro | SectionKind::Interlude | SectionKind::Outro
            ) {
            "Instrumental".to_string()
        } else if kind_total[sec.kind as usize] > 1 {
            format!("{kind_name} {}", sec.occ + 1)
        } else {
            kind_name
        };
        sections.push(SheetSection {
            label,
            kind: sec.kind,
            occurrence: sec.occ + 1,
            sung: sec.is_sung(),
            lift: sec.is_lift(),
            key: sec
                .key
                .0
                .name(sec.key.1.prefers_flats(sec.key.0))
                .to_string(),
            mode: sec.key.1,
            t0: tl.to_time(b0 as f64),
            t1: tl.to_time(b1 as f64),
            bars,
            lines,
        });
    }

    SongSheet {
        version: SHEET_VERSION,
        title: song.title.clone(),
        note: song.note.clone(),
        style: song.style.clone(),
        style_label: None,
        key: song.key.transpose(p.key_shift).name(flats).to_string(),
        mode: song.mode,
        key_shift: p.key_shift,
        meter: song.meter,
        tempo: song.tempo_bpm,
        voice: p.voice,
        voice_b: p.voice_b,
        seed,
        duration_s: tl.end,
        sections,
    }
}

/// Chord onsets in beats `b0..b1` of section `si` (bars from `sec_bar0`),
/// with the chord sounding at `b0` restated when no onset falls there.
/// Each chord comes with its onset beat; `at` is left 0.
fn chords_in(
    form: &Form,
    tl: &Timeline,
    si: usize,
    sec_bar0: usize,
    b0: usize,
    b1: usize,
) -> Vec<(SheetChord, f64)> {
    let bpb = form.bpb() as usize;
    let (fb0, fb1) = (b0 as f64, b1 as f64);
    let mut out: Vec<(SheetChord, f64)> = Vec::new();
    let bar_of = |b: f64| ((b / bpb as f64).floor() as usize).saturating_sub(sec_bar0);
    let starts_at_b0 = tl
        .segs
        .iter()
        .any(|s| s.sec == si && (s.b0 - fb0).abs() < EPS);
    if !starts_at_b0 && !tl.segs.is_empty() {
        let s = &tl.segs[tl.seg_at(fb0)];
        out.push((
            SheetChord {
                at: 0,
                name: form.chord(s.chord).symbol.clone(),
                t: tl.to_time(fb0),
                beat: fb0,
                bar: bar_of(fb0),
                carried: true,
            },
            fb0,
        ));
    }
    for s in tl
        .segs
        .iter()
        .filter(|s| s.sec == si && s.b0 >= fb0 - EPS && s.b0 < fb1 - EPS)
    {
        out.push((
            SheetChord {
                at: 0,
                name: form.chord(s.chord).symbol.clone(),
                t: tl.to_time(s.b0),
                beat: s.b0,
                bar: bar_of(s.b0),
                carried: false,
            },
            s.b0,
        ));
    }
    out
}

/// A sung line: text, syllable and word times, chords over the syllables.
#[allow(clippy::too_many_arguments)]
fn sung_line(
    form: &Form,
    tl: &Timeline,
    si: usize,
    sec_bar0: usize,
    syls: &[song::Syllable],
    notes: &[Option<&LeadNote>],
    lb0: usize,
    lb1: usize,
) -> SheetLine {
    let bar_t0 = tl.to_time(lb0 as f64);
    let bar_t1 = tl.to_time(lb1 as f64);
    let mut text = String::new();
    let mut n_chars = 0usize;
    let mut syllables: Vec<SheetSyllable> = Vec::with_capacity(syls.len());
    let mut words: Vec<SheetWord> = Vec::new();
    // Onset and end beats of each syllable, for chord placement.
    let mut spans: Vec<(f64, f64)> = Vec::with_capacity(syls.len());
    let mut prev_word: Option<u16> = None;
    let mut last_t = bar_t0;
    // End of the last note of the line, continuations included.
    let mut end_t = bar_t0;
    for (k, s) in syls.iter().enumerate() {
        if s.is_continuation() {
            // A melisma's later note: no syllable of its own; the word, the
            // chord span and the line reach to its end.
            if let Some(n) = notes.get(k).copied().flatten() {
                last_t = n.t1;
                end_t = end_t.max(n.t1);
                if let Some(sp) = spans.last_mut() {
                    sp.1 = n.beat + n.dur;
                }
                if let Some(w) = words.last_mut() {
                    w.t1 = n.t1;
                }
            }
            continue;
        }
        let new_word = prev_word != Some(s.word);
        if new_word && k > 0 {
            text.push(' ');
            n_chars += 1;
        }
        prev_word = Some(s.word);
        let at = n_chars;
        text.push_str(&s.text);
        n_chars += s.text.chars().count();
        let (t0, t1, midi, span) = match notes.get(k).copied().flatten() {
            Some(n) => (n.t0, n.t1, n.midi, (n.beat, n.beat + n.dur)),
            None => (last_t, last_t, 0, (f64::NAN, f64::NAN)),
        };
        last_t = t1;
        end_t = end_t.max(t1);
        spans.push(span);
        if new_word {
            words.push(SheetWord {
                text: String::new(),
                at,
                t0,
                t1,
            });
        }
        let wi = words.len() - 1;
        let w = &mut words[wi];
        w.text.push_str(&s.text);
        w.t1 = t1;
        syllables.push(SheetSyllable {
            text: s.text.clone(),
            at,
            word: wi,
            t0,
            t1,
            midi,
            stress: s.stress,
            notes: s.notes.max(1),
        });
    }

    let mut chords = Vec::new();
    for (mut c, b) in chords_in(form, tl, si, sec_bar0, lb0, lb1) {
        c.at = place(b, &spans, &syllables, n_chars);
        chords.push(c);
    }

    let t0 = syllables.first().map_or(bar_t0, |s| s.t0.min(bar_t0));
    let t1 = if syllables.is_empty() {
        bar_t1
    } else {
        end_t.max(bar_t1)
    };
    SheetLine {
        t0,
        t1,
        text,
        syllables,
        words,
        chords,
        singer: None,
    }
}

/// Char offset for a chord onset at beat `b`: the syllable whose note
/// covers `b`; on a rest, the end of the syllable before, 0 when none is
/// before, `len` when none is after.
fn place(b: f64, spans: &[(f64, f64)], syls: &[SheetSyllable], len: usize) -> usize {
    if let Some(k) = spans
        .iter()
        .position(|&(s0, s1)| s0 <= b + EPS && b + EPS < s1)
    {
        return syls[k].at;
    }
    match spans.iter().rposition(|&(s0, _)| s0 < b) {
        None => 0,
        Some(k) if k + 1 >= spans.len() => len,
        Some(k) => syls[k].at + syls[k].text.chars().count(),
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

impl SongSheet {
    /// The classic chord sheet: a header, then each section as a bracketed
    /// label and chord names above the lyric at their syllables. Where two
    /// chords would collide, the lyric is widened (with '-' inside a word)
    /// so each chord still stands over its syllable. Instrumental sections
    /// print their bars, `| G | C D |`.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.title);
        out.push('\n');
        let mut head = format!(
            "{} {}, {}, {:.0} bpm, {}",
            self.key, self.mode, self.meter, self.tempo, self.voice
        );
        if let Some(l) = self.style_label.as_ref().or(self.style.as_ref()) {
            head.push_str(", ");
            head.push_str(l);
        }
        out.push_str(&head);
        out.push('\n');
        if !self.note.is_empty() {
            out.push_str(&self.note);
            out.push('\n');
        }
        let mut running = (self.key.as_str(), self.mode);
        for sec in &self.sections {
            out.push('\n');
            if (sec.key.as_str(), sec.mode) != running {
                running = (sec.key.as_str(), sec.mode);
                out.push_str(&format!("Key: {} {}\n", sec.key, sec.mode));
            }
            out.push_str(&format!("[{}]  {}\n", sec.label, clock(sec.t0)));
            if !sec.sung {
                let mut row = String::from("|");
                for b in &sec.bars {
                    row.push(' ');
                    row.push_str(&b.chords.join(" "));
                    row.push_str(" |");
                }
                out.push_str(&row);
                out.push('\n');
                continue;
            }
            for line in &sec.lines {
                let (chords, lyric) = layout(line);
                if !chords.trim().is_empty() {
                    out.push_str(chords.trim_end());
                    out.push('\n');
                }
                let prefix = match (
                    self.voice_b.is_some(),
                    line.singer.as_ref().map(|s| s.part.as_str()),
                ) {
                    (true, Some("B")) => "[B] ",
                    (true, Some("both")) => "[A+B] ",
                    _ => "",
                };
                out.push_str(prefix);
                out.push_str(lyric.trim_end());
                out.push('\n');
            }
        }
        out
    }
}

/// `m:ss` for `t` seconds.
fn clock(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The chord row and the (possibly widened) lyric row of one line.
fn layout(line: &SheetLine) -> (String, String) {
    let mut lyric: Vec<char> = line.text.chars().collect();
    let mut row: Vec<char> = Vec::new();
    let mut shift = 0usize;
    for c in &line.chords {
        let mut col = c.at.min(line.text.chars().count()) + shift;
        let need = if row.is_empty() { 0 } else { row.len() + 1 };
        if need > col {
            let pad = need - col;
            // Widen the lyric at the chord's column.
            let inside = col > 0 && col < lyric.len() && lyric[col - 1] != ' ' && lyric[col] != ' ';
            let fill = if inside { '-' } else { ' ' };
            let at = col.min(lyric.len());
            for _ in 0..pad {
                lyric.insert(at, fill);
            }
            shift += pad;
            col += pad;
        }
        while row.len() < col {
            row.push(' ');
        }
        row.extend(c.name.chars());
    }
    (row.into_iter().collect(), lyric.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(at: usize, name: &str) -> SheetChord {
        SheetChord {
            at,
            name: name.into(),
            t: 0.0,
            beat: 0.0,
            bar: 0,
            carried: false,
        }
    }

    fn line(text: &str, chords: Vec<SheetChord>) -> SheetLine {
        SheetLine {
            t0: 0.0,
            t1: 1.0,
            text: text.into(),
            syllables: Vec::new(),
            words: Vec::new(),
            chords,
            singer: None,
        }
    }

    #[test]
    fn layout_puts_chords_over_their_columns() {
        let (c, l) = layout(&line(
            "the ferry leaves",
            vec![chord(4, "G"), chord(10, "C")],
        ));
        assert_eq!(c, "    G     C");
        assert_eq!(l, "the ferry leaves");
    }

    #[test]
    fn layout_widens_the_lyric_where_chords_collide() {
        // "fer" at 4, "ry" at 7: Cmaj7 over "fer" would cover "ry".
        let (c, l) = layout(&line("the ferry", vec![chord(4, "Cmaj7"), chord(7, "D")]));
        assert_eq!(c, "    Cmaj7 D");
        assert_eq!(l, "the fer---ry");
        // Between words the lyric widens with spaces.
        let (c, l) = layout(&line("a b", vec![chord(0, "Am7"), chord(2, "G")]));
        assert_eq!(c, "Am7 G");
        assert_eq!(l, "a   b");
    }

    #[test]
    fn layout_puts_a_chord_after_the_text() {
        let (c, l) = layout(&line("home", vec![chord(0, "G"), chord(4, "D")]));
        assert_eq!(c, "G   D");
        assert_eq!(l, "home");
    }
}

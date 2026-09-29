//! The engraving model: the prepared melody quantised to sixteenths, split
//! into bars and notatable values, spelled in the key, with chord marks,
//! section labels and per-note times.

use compose::melody::LeadNote;
use compose::prepare::Prepared;
use compose::timeline::Timeline;
use song::{Meter, Mode, Pc, SectionKind, SectionRole, SingerId, Song, Voice};

/// Page width in px when none is given.
pub const DEFAULT_WIDTH: f64 = 900.0;

/// The bar grid in units of a sixteenth note.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grid {
    /// Units per bar.
    pub bar_u: i64,
    /// Units per beat: 4 in 4/4 and 3/4, 6 in 6/8.
    pub beat_u: i64,
    /// Compound meter (6/8).
    pub compound: bool,
    /// Beats per bar.
    pub bpb: i64,
}

impl Grid {
    pub(crate) fn of(meter: Meter) -> Grid {
        let g = meter.grid();
        let beat_u = 2 * g.sub as i64;
        Grid { bar_u: beat_u * g.beats as i64, beat_u, compound: meter == Meter::Six8, bpb: g.beats as i64 }
    }

    /// Beats per unit.
    pub fn unit(&self) -> f64 {
        1.0 / self.beat_u as f64
    }

    /// Whether a note (or rest) of `v` units may start at unit `st` of the
    /// bar: short values stay inside their beat; longer values start on a
    /// beat and do not cross the middle of a 4/4 bar. Rests are not dotted
    /// in simple meters.
    fn allowed(&self, st: i64, v: i64, rest: bool) -> bool {
        if st + v > self.bar_u {
            return false;
        }
        let in_beat = st % self.beat_u;
        if self.compound {
            return match v {
                12 => st == 0,
                6 => in_beat == 0,
                4 => in_beat % 2 == 0 && in_beat + v <= 6,
                3 => in_beat % 3 == 0,
                1 | 2 => in_beat + v <= 6,
                _ => false,
            };
        }
        if rest && (v == 3 || v == 6 || v == 12) {
            return false;
        }
        let four = self.bar_u == 16;
        match v {
            1..=4 => in_beat + v <= 4,
            6 | 8 => {
                if four {
                    st % 8 == 0
                } else {
                    st == 0 || st == 4
                }
            }
            12 => st == 0 || (four && st == 4),
            16 => st == 0,
            _ => false,
        }
    }

    /// `d` units from `st` as notatable values, largest first.
    pub(crate) fn split(&self, mut st: i64, mut d: i64, rest: bool) -> Vec<(i64, i64)> {
        const VALS: [i64; 8] = [16, 12, 8, 6, 4, 3, 2, 1];
        let mut out = Vec::new();
        while d > 0 {
            let v = VALS.iter().copied().find(|&v| v <= d && self.allowed(st, v, rest)).unwrap_or(1);
            out.push((st, v));
            st += v;
            d -= v;
        }
        out
    }
}

/// A sung note (or one tied part of it).
#[derive(Clone, Debug)]
pub(crate) struct NoteEv {
    /// Index into the prepared lead notes.
    pub note: usize,
    /// Index into `Form::lines`.
    pub line: usize,
    /// Diatonic staff position as written: C4 = 28, the top line F5 = 38.
    pub step: i32,
    /// Accidental to print (-1, 0 natural, 1), if any.
    pub accidental: Option<i32>,
    /// The syllable, on the first part only.
    pub lyric: Option<String>,
    /// A hyphen follows the syllable (the word goes on).
    pub hyphen: bool,
    pub tie_in: bool,
    pub tie_out: bool,
    /// Seconds.
    pub t0: f64,
    pub t1: f64,
}

/// A note or a rest in a bar; `s` and `d` in units from the bar start.
#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub s: i64,
    pub d: i64,
    pub note: Option<NoteEv>,
}

/// A chord symbol at unit `u` of a bar.
#[derive(Clone, Debug)]
pub(crate) struct ChordMark {
    pub u: i64,
    pub name: String,
}

/// One bar as engraved.
#[derive(Clone, Debug)]
pub(crate) struct Measure {
    /// Bar index in the form; -1 for a pickup before the first bar.
    pub bar: i64,
    /// First unit shown (non-zero only for the pickup bar).
    pub from_u: i64,
    /// Index into `Form::sections`.
    pub sec: usize,
    /// Break group: bars of one lyric line, or a run of bars without lyrics
    /// in one section. Each group starts a new system.
    pub chunk: usize,
    /// Section label, on the first bar of a section.
    pub label: Option<String>,
    pub events: Vec<Event>,
    pub chords: Vec<ChordMark>,
    /// The chord sounding at the bar start, for a system that starts here.
    pub sounding: Option<String>,
    /// No notes: one whole-bar rest.
    pub empty: bool,
    /// The next bar starts another section.
    pub section_end: bool,
    /// The melody singer of this bar's lyric line; `None` for a bar with no
    /// lyric line (an instrumental system, engraved on singer A's staff).
    pub singer: Option<SingerId>,
    /// Melody-staff clef (treble 8vb for bass, baritone, tenor): singer A's
    /// clef outside a duet, or an instrumental bar's.
    pub clef8: bool,
    /// Whether this bar's line is shared (`Part::Both`): a second staff is
    /// drawn. Always false outside a duet.
    pub shared: bool,
    /// The other singer's events on a shared line, same grid as `events`;
    /// empty otherwise.
    pub second: Vec<Event>,
    /// The second staff's clef, valid only when `shared`.
    pub second_clef8: bool,
    /// Whether the melody staff is the higher of the two (drawn on top),
    /// valid only when `shared`.
    pub melody_on_top: bool,
    /// Seconds.
    pub t0: f64,
    pub t1: f64,
}

/// A song's melody ready to engrave. Build with `Score::new`, draw with
/// `engrave`.
#[derive(Clone, Debug)]
pub struct Score {
    pub title: String,
    /// Voice and key, printed under the title ("Baritone, A major").
    pub caption: String,
    /// Page width in px; the height follows from the content.
    pub width: f64,
    pub(crate) meter: Meter,
    pub(crate) tempo: f64,
    /// Key signature: sharps positive, flats negative.
    pub(crate) fifths: i32,
    /// Treble clef an octave down (bass, baritone, tenor): pitches are
    /// written an octave above the sound. Singer A's clef outside a duet.
    pub(crate) clef8: bool,
    pub(crate) grid: Grid,
    pub(crate) measures: Vec<Measure>,
    /// Whether the song is a duet: gates every duet-only drawing (labels,
    /// second staves) so a solo song's SVG is exactly as before.
    pub(crate) duet: bool,
    pub(crate) voice_a: Voice,
    pub(crate) voice_b: Option<Voice>,
}

/// Whether `v` is written an octave down (treble 8vb).
fn clef8_of(v: Voice) -> bool {
    matches!(v, Voice::Bass | Voice::Baritone | Voice::Tenor)
}

/// Buckets `notes` (`Comp::lead` for the melody staff, `Comp::second` for
/// the other singer's staff on a shared line) into per-measure (start unit,
/// duration unit, `NoteEv`) triples, tied across barlines, as `Score::new`
/// did inline before the duet staff (wave 3) needed it for two note
/// sources.
fn bucket_notes(notes: &[LeadNote], grid: &Grid, tl: &Timeline, first_bar: i64, n_meas: usize) -> Vec<Vec<(i64, i64, NoteEv)>> {
    let u = grid.unit();
    let bar_u = grid.bar_u;
    let n = notes.len();
    let mut s = vec![0i64; n];
    for i in 0..n {
        let q = (notes[i].beat / u).round() as i64;
        s[i] = if i > 0 { q.max(s[i - 1] + 1) } else { q };
    }
    let mut e = vec![0i64; n];
    for i in 0..n {
        let mut q = ((notes[i].beat + notes[i].dur) / u).round() as i64;
        if i + 1 < n {
            q = q.min(s[i + 1]);
        }
        e[i] = q.max(s[i] + 1);
    }
    let mut raw: Vec<Vec<(i64, i64, NoteEv)>> = vec![Vec::new(); n_meas];
    for (i, ln) in notes.iter().enumerate() {
        let mut st = s[i];
        let mut first = true;
        while st < e[i] {
            let b = st.div_euclid(bar_u);
            let be = e[i].min((b + 1) * bar_u);
            let beat0 = st as f64 * u;
            let beat1 = be as f64 * u;
            let ev = NoteEv {
                note: i,
                line: ln.line_idx,
                step: 0,
                accidental: None,
                lyric: first.then(|| ln.syl.text.clone()),
                hyphen: first && !ln.syl.word_end,
                tie_in: !first,
                tie_out: be < e[i],
                t0: if first { ln.t0 } else { tl.to_time(beat0).min(ln.t1) },
                t1: if be >= e[i] { ln.t1 } else { tl.to_time(beat1).min(ln.t1) },
            };
            if let Some(v) = raw.get_mut((b - first_bar) as usize) {
                v.push((st - b * bar_u, be - st, ev));
            }
            first = false;
            st = be;
        }
    }
    raw
}

/// Notatable events (rests filling the gaps) for one measure from its
/// bucketed notes; `from_u` is nonzero only for a pickup bar's first
/// measure.
fn events_of(notes: Vec<(i64, i64, NoteEv)>, from_u: i64, bar_u: i64, grid: &Grid) -> Vec<Event> {
    let mut events = Vec::new();
    let mut c = from_u;
    let n_notes = notes.len();
    for (st, d, ev) in notes {
        if st > c {
            for (rs, rd) in grid.split(c, st - c, true) {
                events.push(Event { s: rs, d: rd, note: None });
            }
        }
        let parts = grid.split(st, d, false);
        let np = parts.len();
        let span = (ev.t1 - ev.t0).max(0.0);
        for (j, (ps, pd)) in parts.into_iter().enumerate() {
            let mut p = ev.clone();
            if j > 0 {
                p.lyric = None;
                p.hyphen = false;
                p.tie_in = true;
            }
            if j + 1 < np {
                p.tie_out = true;
            }
            let a = (ps - st) as f64 / d as f64;
            let b = (ps + pd - st) as f64 / d as f64;
            p.t0 = ev.t0 + span * a;
            p.t1 = ev.t0 + span * b;
            events.push(Event { s: ps, d: pd, note: Some(p) });
        }
        c = st + d;
    }
    if n_notes > 0 && c < bar_u {
        for (rs, rd) in grid.split(c, bar_u - c, true) {
            events.push(Event { s: rs, d: rd, note: None });
        }
    }
    events
}

/// Accidentals against the key and the bar, for one measure's events; `notes`
/// is the source slice `ev.note`'s indices refer into (`Comp::lead` or
/// `Comp::second`).
fn spell_events(events: &mut [Event], notes: &[LeadNote], written: i32, fifths: i32, key_alt: &[i32; 7]) {
    let mut state: Vec<(i32, i32)> = Vec::new();
    for ev in events.iter_mut() {
        let Some(ne) = ev.note.as_mut() else { continue };
        let m = notes.get(ne.note).map_or(60, |x| x.midi) + written;
        let (l, a, step) = spell(m, fifths, &key_alt);
        ne.step = step;
        let cur = state.iter().find(|x| x.0 == step).map_or(key_alt[l], |x| x.1);
        if a != cur && !ne.tie_in {
            ne.accidental = Some(a);
            state.retain(|x| x.0 != step);
            state.push((step, a));
        }
    }
}

/// Semitones of the natural notes C D E F G A B.
const NAT: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
/// Letters (C = 0) of the sharps and of the flats in key-signature order.
const SHARP_ORDER: [usize; 7] = [3, 0, 4, 1, 5, 2, 6];
const FLAT_ORDER: [usize; 7] = [6, 2, 5, 1, 4, 0, 3];

/// Key signature (sharps positive) for a tonic pitch class and mode.
pub(crate) fn key_fifths(tonic: i32, mode: Mode) -> i32 {
    let off = match mode {
        Mode::Major => 0,
        Mode::Minor => 3,
        Mode::Dorian => 10,
        Mode::Mixolydian => 5,
    };
    let rel = (tonic + off).rem_euclid(12);
    match rel {
        0 => 0,
        7 => 1,
        2 => 2,
        9 => 3,
        4 => 4,
        11 => 5,
        6 => {
            if mode.prefers_flats(Pc::new(tonic)) {
                -6
            } else {
                6
            }
        }
        1 => -5,
        8 => -4,
        3 => -3,
        10 => -2,
        _ => -1,
    }
}

/// Alteration of each letter C..B in the key signature.
pub(crate) fn key_alterations(fifths: i32) -> [i32; 7] {
    let mut alt = [0; 7];
    let n = fifths.unsigned_abs().min(7) as usize;
    if fifths > 0 {
        SHARP_ORDER[..n].iter().for_each(|&l| alt[l] = 1);
    } else {
        FLAT_ORDER[..n].iter().for_each(|&l| alt[l] = -1);
    }
    alt
}

/// Spelling of MIDI note `m` in the key: (letter 0-6, alteration, staff
/// step with C4 = 28). Prefers the key's own alteration, then a natural,
/// then the key's direction of accidental.
pub(crate) fn spell(m: i32, fifths: i32, key_alt: &[i32; 7]) -> (usize, i32, i32) {
    let pc = m.rem_euclid(12);
    let mut best = (0usize, 0i32);
    let mut best_score = i32::MIN;
    for (l, &nat) in NAT.iter().enumerate() {
        for a in [-1, 0, 1] {
            if (nat + a).rem_euclid(12) != pc {
                continue;
            }
            let mut s = 0;
            if key_alt[l] == a {
                s += 10;
            }
            if a == 0 {
                s += 2;
            }
            if (a == 1 && fifths >= 0) || (a == -1 && fifths < 0) {
                s += 1;
            }
            if s > best_score {
                best_score = s;
                best = (l, a);
            }
        }
    }
    let (l, a) = best;
    let octave = (m - a - NAT[l]).div_euclid(12) - 1;
    (l, a, l as i32 + 7 * octave)
}

/// Section label: "Verse 2", "Chorus", "Break".
fn section_label(kind: SectionKind, role: SectionRole, occ: usize, verses: usize) -> String {
    match role {
        SectionRole::Break => return "Break".into(),
        SectionRole::Tag => return "Tag".into(),
        SectionRole::Plain => {}
    }
    let name = match kind {
        SectionKind::Intro => "Intro",
        SectionKind::Verse => "Verse",
        SectionKind::Prechorus => "Pre-chorus",
        SectionKind::Chorus => "Chorus",
        SectionKind::Bridge => "Bridge",
        SectionKind::Interlude => "Interlude",
        SectionKind::Outro => "Outro",
    };
    if kind == SectionKind::Verse && verses > 1 {
        format!("{name} {}", occ + 1)
    } else {
        name.to_string()
    }
}

impl Score {
    /// The score of `song` as prepared (`compose::prepare`): the lead
    /// melody in the transposed key, chords from the prepared form.
    pub fn new(song: &Song, prep: &Prepared) -> Score {
        let grid = Grid::of(song.meter);
        let form = &prep.form;
        let tl = &prep.timeline;
        let lead = &prep.comp.lead;
        let second_notes = &prep.comp.second;
        let fifths = key_fifths(prep.tonic, song.mode);
        let key_alt = key_alterations(fifths);
        let voice_a = prep.voice;
        let voice_b = prep.voice_b;
        let voice_of = |s: SingerId| if s == SingerId::A { voice_a } else { voice_b.unwrap_or(voice_a) };
        let clef8 = clef8_of(voice_a);
        let u = grid.unit();
        let bar_u = grid.bar_u;

        // Onsets and ends in units, strictly increasing, no overlaps.
        let n = lead.len();
        let mut s = vec![0i64; n];
        for i in 0..n {
            let q = (lead[i].beat / u).round() as i64;
            s[i] = if i > 0 { q.max(s[i - 1] + 1) } else { q };
        }
        let mut e = vec![0i64; n];
        for i in 0..n {
            let mut q = ((lead[i].beat + lead[i].dur) / u).round() as i64;
            if i + 1 < n {
                q = q.min(s[i + 1]);
            }
            e[i] = q.max(s[i] + 1);
        }

        let first_bar = s.first().map_or(0, |&x| x.div_euclid(bar_u)).min(0);
        let n_form = form.bars.len() as i64;
        let last_bar = e.last().map_or(0, |&x| (x - 1).div_euclid(bar_u)).max(n_form - 1);
        let bar_info = |b: i64| -> (usize, Option<usize>) {
            let i = b.clamp(0, n_form - 1).max(0) as usize;
            form.bars.get(i).map_or((0, None), |bar| (bar.sec, bar.line))
        };

        // Notes into bars, tied across barlines: the melody (every line,
        // whichever singer) and, on shared lines, the other singer.
        let n_meas = (last_bar - first_bar + 1) as usize;
        let raw = bucket_notes(lead, &grid, tl, first_bar, n_meas);
        let raw2 = bucket_notes(second_notes, &grid, tl, first_bar, n_meas);

        // Chord marks from the timeline's segments.
        let mut marks: Vec<Vec<ChordMark>> = vec![Vec::new(); n_meas];
        for seg in &tl.segs {
            let pos = (seg.b0 / u).round() as i64;
            let b = pos.div_euclid(bar_u);
            if let Some(v) = marks.get_mut((b - first_bar) as usize) {
                v.push(ChordMark { u: pos - b * bar_u, name: form.chord(seg.chord).symbol.clone() });
            }
        }

        let verses = form.sections.iter().filter(|x| x.kind == SectionKind::Verse).count();
        let mut measures = Vec::with_capacity(n_meas);
        let mut chunk = 0usize;
        let mut prev_key: Option<(usize, Option<usize>)> = None;
        for (k, ((notes, notes2), chords)) in raw.into_iter().zip(raw2).zip(marks).enumerate() {
            let bar = first_bar + k as i64;
            let key = bar_info(bar);
            if prev_key.is_some_and(|p| p != key) {
                chunk += 1;
            }
            prev_key = Some(key);
            let (sec, line_idx) = key;
            // The label goes on the pickup bar when there is one.
            let starts = |x: &compose::form::Sec| {
                bar < 0 || (bar == x.start_bar as i64 && !(bar == 0 && first_bar < 0))
            };
            let label = form
                .sections
                .get(sec)
                .and_then(|x| starts(x).then(|| section_label(x.kind, x.role, x.occ, verses)));
            let from_u = if bar < 0 { notes.first().map_or(0, |x| x.0) } else { 0 };
            let empty = notes.is_empty();

            // This bar's melody singer and, on a shared line, the other
            // singer and blend (design 4.5/4.8); an instrumental bar (no
            // lyric line) uses singer A's staff and clef.
            let line = line_idx.and_then(|li| form.lines.get(li));
            let singer = line.map(|l| l.part.melody());
            let other = line.and_then(|l| l.part.other());
            let m_voice = singer.map_or(voice_a, voice_of);
            let m_clef8 = clef8_of(m_voice);
            let shared = other.is_some();
            let (second_clef8, melody_on_top) = match other {
                Some((os, _)) => {
                    let o_voice = voice_of(os);
                    (clef8_of(o_voice), m_voice.range().centre() >= o_voice.range().centre())
                }
                None => (false, false),
            };

            let mut events = events_of(notes, from_u, bar_u, &grid);
            spell_events(&mut events, lead, if m_clef8 { 12 } else { 0 }, fifths, &key_alt);

            let from_u2 = if bar < 0 { notes2.first().map_or(0, |x| x.0) } else { 0 };
            let mut second = if shared { events_of(notes2, from_u2, bar_u, &grid) } else { Vec::new() };
            if shared {
                spell_events(&mut second, second_notes, if second_clef8 { 12 } else { 0 }, fifths, &key_alt);
            }

            let sounding = (bar >= 0).then(|| tl.chord_at(form, (bar * grid.bpb) as f64).symbol.clone());
            let b0 = (bar * bar_u + from_u) as f64 * u;
            let b1 = ((bar + 1) * bar_u) as f64 * u;
            measures.push(Measure {
                bar,
                from_u,
                sec,
                chunk,
                label,
                events,
                chords,
                sounding,
                empty,
                section_end: false,
                singer,
                clef8: m_clef8,
                shared,
                second,
                second_clef8,
                melody_on_top,
                t0: tl.to_time(b0),
                t1: tl.to_time(b1),
            });
        }
        for i in 0..measures.len().saturating_sub(1) {
            measures[i].section_end = measures[i].sec != measures[i + 1].sec;
        }

        let flats = fifths < 0;
        let mode = match song.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
            Mode::Dorian => "Dorian",
            Mode::Mixolydian => "Mixolydian",
        };
        let caption = format!("{}, {} {}", prep.voice.label(), Pc::new(prep.tonic).name(flats), mode);
        Score {
            title: song.title.clone(),
            caption,
            width: DEFAULT_WIDTH,
            meter: song.meter,
            tempo: song.tempo_bpm,
            fifths,
            clef8,
            grid,
            measures,
            duet: song.is_duet(),
            voice_a,
            voice_b,
        }
    }

    /// The same score laid out for a page `width` px wide (at least 300).
    pub fn with_width(mut self, width: f64) -> Score {
        self.width = if width.is_finite() { width.max(300.0) } else { DEFAULT_WIDTH };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_signatures() {
        assert_eq!(key_fifths(7, Mode::Major), 1);
        assert_eq!(key_fifths(9, Mode::Minor), 0);
        assert_eq!(key_fifths(2, Mode::Dorian), 0);
        assert_eq!(key_fifths(2, Mode::Mixolydian), 1);
        assert_eq!(key_fifths(5, Mode::Major), -1);
        assert_eq!(key_fifths(1, Mode::Minor), 4);
    }

    #[test]
    fn spelling_follows_the_key() {
        let f = key_alterations(-2);
        assert_eq!(spell(70, -2, &f), (6, -1, 34)); // Bb4
        let g = key_alterations(1);
        assert_eq!(spell(66, 1, &g), (3, 1, 31)); // F#4
        assert_eq!(spell(65, 1, &g), (3, 0, 31)); // F natural
        assert_eq!(spell(60, 0, &key_alterations(0)), (0, 0, 28));
    }

    /// Every bar is tiled by its events, and every lead note starts once.
    #[test]
    fn bars_are_full_and_notes_complete() {
        let song = engine::demo_song();
        for seed in 0..20u64 {
            let prep = compose::prepare::prepare(song, seed, None);
            let sc = Score::new(song, &prep);
            let mut starts = vec![0usize; prep.comp.lead.len()];
            for m in &sc.measures {
                if m.empty {
                    assert!(m.events.is_empty());
                    continue;
                }
                let mut c = m.from_u;
                for e in &m.events {
                    assert_eq!(e.s, c, "seed {seed} bar {}", m.bar);
                    c += e.d;
                    if let Some(n) = &e.note {
                        if !n.tie_in {
                            starts[n.note] += 1;
                        }
                    }
                }
                assert_eq!(c, sc.grid.bar_u, "seed {seed} bar {}", m.bar);
            }
            assert!(starts.iter().all(|&k| k == 1), "seed {seed}");
        }
    }

    #[test]
    fn splits_are_notatable() {
        for meter in [Meter::Four4, Meter::Three4, Meter::Six8] {
            let g = Grid::of(meter);
            for st in 0..g.bar_u {
                for d in 1..=(g.bar_u - st) {
                    for rest in [false, true] {
                        let parts = g.split(st, d, rest);
                        assert_eq!(parts.iter().map(|p| p.1).sum::<i64>(), d);
                        for (ps, pd) in parts {
                            assert!([1, 2, 3, 4, 6, 8, 12, 16].contains(&pd));
                            assert!(ps + pd <= g.bar_u);
                        }
                    }
                }
            }
        }
    }
}

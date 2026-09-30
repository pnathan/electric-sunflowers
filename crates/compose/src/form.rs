//! Form: lays the song's sections, lines and bars out on the bar grid, and
//! marks each section's lift (the sections that carry the hook), intensity
//! and role. Every cross-reference is an index into the owning Vec.

use std::ops::Range;

use song::chord::transpose_symbol;
use song::{
    BarChords, Chord, ChordId, Meter, MeterGrid, Mode, Part, Pc, Rubato, SectionBody, SectionKind,
    SectionRole, Song, Syllable,
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
/// flats when the new key reads better with them), and for each song chord
/// the first id with the same transposed symbol.
fn transposed_chords(song: &Song, transpose: i32) -> (Vec<Chord>, Vec<ChordId>) {
    let flats = song.mode.prefers_flats(song.key.transpose(transpose));
    let mut chords: Vec<Chord> = Vec::with_capacity(song.chords.len());
    let mut canon: Vec<ChordId> = Vec::with_capacity(song.chords.len());
    for (id, c) in song.chords.iter() {
        let t = if transpose == 0 {
            c.clone()
        } else {
            // A transposed symbol still starts with a note name, so it parses.
            Chord::parse(&transpose_symbol(&c.symbol, transpose, flats))
                .unwrap_or_else(|_| c.clone())
        };
        let first = chords
            .iter()
            .position(|x| x.symbol == t.symbol)
            .map_or(id, |i| canon[i]);
        chords.push(t);
        canon.push(first);
    }
    (chords, canon)
}

/// Lays out `song` with its chords transposed by `transpose` semitones.
pub fn build_form(song: &Song, transpose: i32) -> Form {
    let meter = song.meter;
    let grid = meter.grid();
    let (chords, canon) = transposed_chords(song, transpose);
    let map = |b: &BarChords| -> BarChords {
        let s = b.as_slice();
        if s.len() == 2 {
            BarChords::two(canon[s[0].index()], canon[s[1].index()])
        } else {
            BarChords::one(canon[s[0].index()])
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
            ns += ln.syllables.len() as f64 / ln.bars.len() as f64;
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
        let mut sec_lines: Vec<usize> = Vec::new();
        match &s.body {
            SectionBody::Sung(ls) => {
                for (li, ln) in ls.iter().enumerate() {
                    let start_bar = bars.len();
                    for b in &ln.bars {
                        let (xs, n) = expand(map(b), stretch);
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
                        syls: ln.syllables.clone(),
                        part: ln.part,
                        text: ln.text(),
                        pitches: None,
                        rh: None,
                    });
                }
            }
            SectionBody::Instrumental(bs) => {
                for b in bs {
                    let (xs, n) = expand(map(b), stretch);
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
        s.intensity = match level {
            0 => Intensity::Quiet,
            1 => Intensity::Low,
            2 => Intensity::Mid,
            _ => Intensity::High,
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
mod tests {
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
}

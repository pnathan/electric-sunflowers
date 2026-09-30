//! Tunes (schema 3): a line's melody sketched in movable-do solfege.
//!
//! One token per sung note of the line (melisma notes included), separated
//! by white space:
//!
//! - `d r m f s l t`: the major-scale degrees above the tonic of the
//!   section's key, at 0, 2, 4, 5, 7, 9 and 11 semitones. The mode does not
//!   change them; the writer says `me` for a minor third.
//! - `di ri fi si li`: raised, one semitone up (1, 3, 6, 8, 10).
//! - `ra me se le te`: lowered, one semitone down (1, 3, 6, 8, 10).
//! - A trailing `,` lowers the note one octave (`s,`), a trailing `'`
//!   raises it (`d'`). Marks may repeat (`d''`).
//! - `.`: no hint for the note; the engine chooses it.
//! - `-`: not a note. It adds one beat of hold to the token before it, as
//!   a length hint. The engine keeps the hint and does not use it yet.
//!
//! The octave of a plain `d` is settled at composition: the tonic nearest
//! the register pitch (`compose::melody::register_of`).

use serde::Serialize;

use crate::model::Meter;

/// Token roots and their semitones above the tonic.
const ROOTS: [(&str, u8); 17] = [
    ("d", 0),
    ("di", 1),
    ("ra", 1),
    ("r", 2),
    ("ri", 3),
    ("me", 3),
    ("m", 4),
    ("f", 5),
    ("fi", 6),
    ("se", 6),
    ("s", 7),
    ("si", 8),
    ("le", 8),
    ("l", 9),
    ("li", 10),
    ("te", 10),
    ("t", 11),
];

/// Most octave marks one token may carry.
const MAX_MARKS: i32 = 4;

/// A hinted pitch: semitones above the tonic (0-11) and whole octaves away
/// from the reference octave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TunePitch {
    pub semis: u8,
    pub octave: i8,
}

/// One note of a tune.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct TuneNote {
    /// `None`: no hint (`.`).
    pub pitch: Option<TunePitch>,
    /// Beats of hold asked with `-` tokens; a length hint only.
    pub hold: u8,
}

impl TuneNote {
    /// The note as text, the form `parse_tune` reads back.
    pub fn token(&self) -> String {
        let mut s = match self.pitch {
            None => ".".to_string(),
            Some(p) => {
                // Sharp spelling for the chromatic degrees.
                let name = match p.semis {
                    0 => "d",
                    1 => "di",
                    2 => "r",
                    3 => "ri",
                    4 => "m",
                    5 => "f",
                    6 => "fi",
                    7 => "s",
                    8 => "si",
                    9 => "l",
                    10 => "li",
                    _ => "t",
                };
                let mark = if p.octave < 0 { ',' } else { '\'' };
                let mut s = name.to_string();
                for _ in 0..p.octave.unsigned_abs() {
                    s.push(mark);
                }
                s
            }
        };
        for _ in 0..self.hold {
            s.push_str(" -");
        }
        s
    }
}

/// Text of a whole tune, tokens joined by single spaces.
pub fn tune_text(tune: &[TuneNote]) -> String {
    tune.iter()
        .map(TuneNote::token)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Reads one pitch token: a root and octave marks (`s,`, `d'`).
fn parse_pitch(tok: &str) -> Option<TunePitch> {
    let root_len = tok.find([',', '\'']).unwrap_or(tok.len());
    let (root, marks) = tok.split_at(root_len);
    let &(_, semis) = ROOTS.iter().find(|(n, _)| *n == root)?;
    let mut octave: i32 = 0;
    for c in marks.chars() {
        match c {
            ',' => octave -= 1,
            '\'' => octave += 1,
            _ => return None,
        }
    }
    if marks.len() as i32 > MAX_MARKS {
        return None;
    }
    Some(TunePitch {
        semis,
        octave: octave as i8,
    })
}

/// Reads one tune line. `Err` holds the first token that is not valid.
pub fn parse_tune(text: &str) -> Result<Vec<TuneNote>, String> {
    let mut out: Vec<TuneNote> = Vec::new();
    for tok in text.split_whitespace() {
        if tok == "-" {
            match out.last_mut() {
                Some(n) => n.hold = n.hold.saturating_add(1),
                None => return Err(tok.to_string()),
            }
            continue;
        }
        if tok == "." {
            out.push(TuneNote {
                pitch: None,
                hold: 0,
            });
            continue;
        }
        let Some(pitch) = parse_pitch(tok) else {
            return Err(tok.to_string());
        };
        out.push(TuneNote {
            pitch: Some(pitch),
            hold: 0,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------- break tunes

/// Ticks in a whole note. Every length a break tune may write is a whole
/// number of ticks: a sixteenth is 6, a dotted sixteenth 9.
pub const WHOLE_TICKS: u16 = 96;

/// Ticks in one beat of `meter` (`Meter::grid`): a quarter note, 24, in 4/4
/// and 3/4; a dotted quarter, 36, in 6/8.
pub const fn beat_ticks(meter: Meter) -> u32 {
    match meter {
        Meter::Six8 => 36,
        Meter::Four4 | Meter::Three4 => 24,
    }
}

/// Ticks in one bar of `meter`: 96 in 4/4, 72 in 3/4 and 6/8.
pub const fn bar_ticks(meter: Meter) -> u32 {
    meter.grid().beats as u32 * beat_ticks(meter)
}

/// One note, or rest, of a break tune (an instrumental section's written
/// lead line).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct BreakNote {
    /// `None`: a rest.
    pub pitch: Option<TunePitch>,
    /// Length in ticks (`WHOLE_TICKS` per whole note).
    pub ticks: u16,
}

impl BreakNote {
    /// The note as text, the form `parse_break_tune` reads back.
    pub fn token(&self) -> String {
        let len = match self.ticks {
            96 => "1",
            144 => "1.",
            48 => "2",
            72 => "2.",
            24 => "4",
            36 => "4.",
            12 => "8",
            18 => "8.",
            6 => "16",
            _ => "16.",
        };
        let head = match self.pitch {
            None => "z".to_string(),
            Some(p) => TuneNote {
                pitch: Some(p),
                hold: 0,
            }
            .token(),
        };
        format!("{head}{len}")
    }
}

/// Text of a whole break tune, tokens joined by single spaces (bar lines
/// are not kept).
pub fn break_tune_text(tune: &[BreakNote]) -> String {
    tune.iter()
        .map(BreakNote::token)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Total length of a break tune in ticks.
pub fn break_ticks(tune: &[BreakNote]) -> u32 {
    tune.iter().map(|n| n.ticks as u32).sum()
}

/// A parsed break tune: the notes, and each bar whose length differs from
/// the meter's, as (bar number from 1, ticks). Bars are checked only when
/// the text has bar lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedBreak {
    pub notes: Vec<BreakNote>,
    pub bad_bars: Vec<(usize, u32)>,
}

fn parse_break_token(tok: &str) -> Option<BreakNote> {
    let at = tok.find(|c: char| c.is_ascii_digit())?;
    let (head, tail) = tok.split_at(at);
    let end = tail
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(tail.len());
    let (digits, dots) = tail.split_at(end);
    let base: u16 = match digits {
        "1" => 96,
        "2" => 48,
        "4" => 24,
        "8" => 12,
        "16" => 6,
        _ => return None,
    };
    let ticks = match dots {
        "" => base,
        "." => base * 3 / 2,
        _ => return None,
    };
    let pitch = if head == "z" {
        None
    } else {
        Some(parse_pitch(head)?)
    };
    Some(BreakNote { pitch, ticks })
}

/// Reads a break tune: solfege tokens (as for `parse_tune`) each followed by
/// a length (`8` eighth, `16` sixteenth, `4` quarter, `2` half, `1` whole;
/// a trailing `.` dots it), `z` with a length for a rest, and optional `|`
/// bar lines checked against `bar_ticks`. `Err` holds the first bad token.
pub fn parse_break_tune(text: &str, bar_ticks: u32) -> Result<ParsedBreak, String> {
    let spaced = text.replace('|', " | ");
    let mut notes = Vec::new();
    let mut bad_bars = Vec::new();
    let (mut bars, mut in_bar, mut barred) = (0usize, 0u32, false);
    for tok in spaced.split_whitespace() {
        if tok == "|" {
            barred = true;
            if in_bar > 0 {
                bars += 1;
                if in_bar != bar_ticks {
                    bad_bars.push((bars, in_bar));
                }
                in_bar = 0;
            }
            continue;
        }
        let n = parse_break_token(tok).ok_or_else(|| tok.to_string())?;
        in_bar += n.ticks as u32;
        notes.push(n);
    }
    if barred && in_bar > 0 {
        bars += 1;
        if in_bar != bar_ticks {
            bad_bars.push((bars, in_bar));
        }
    }
    Ok(ParsedBreak { notes, bad_bars })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(semis: u8, octave: i8) -> Option<TunePitch> {
        Some(TunePitch { semis, octave })
    }

    fn semis(t: &[TuneNote]) -> Vec<u8> {
        t.iter().map(|n| n.pitch.unwrap().semis).collect()
    }

    #[test]
    fn plain_degrees() {
        let t = parse_tune("d r m f s l t").unwrap();
        assert_eq!(semis(&t), [0, 2, 4, 5, 7, 9, 11]);
    }

    #[test]
    fn alterations() {
        assert_eq!(
            semis(&parse_tune("di ri fi si li").unwrap()),
            [1, 3, 6, 8, 10]
        );
        assert_eq!(
            semis(&parse_tune("ra me se le te").unwrap()),
            [1, 3, 6, 8, 10]
        );
    }

    #[test]
    fn octave_marks() {
        let t = parse_tune("s, d' m'' l,,").unwrap();
        assert_eq!(t[0].pitch, p(7, -1));
        assert_eq!(t[1].pitch, p(0, 1));
        assert_eq!(t[2].pitch, p(4, 2));
        assert_eq!(t[3].pitch, p(9, -2));
    }

    #[test]
    fn free_and_holds() {
        let t = parse_tune("d - - . m -").unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].hold, 2);
        assert_eq!(t[1].pitch, None);
        assert_eq!(t[2].hold, 1);
    }

    #[test]
    fn bad_tokens() {
        assert_eq!(parse_tune("d x m"), Err("x".into()));
        assert_eq!(parse_tune("- d"), Err("-".into()));
        assert!(parse_tune("dd").is_err());
        assert!(parse_tune("d,,,,,").is_err());
        assert!(parse_tune("").unwrap().is_empty());
    }

    #[test]
    fn text_round_trip() {
        let t = parse_tune("d, - . ri' s").unwrap();
        assert_eq!(parse_tune(&tune_text(&t)).unwrap(), t);
    }

    fn ticks(t: &[BreakNote]) -> Vec<u16> {
        t.iter().map(|n| n.ticks).collect()
    }

    #[test]
    fn break_lengths_dots_and_rests() {
        let p = parse_break_tune("d8 r16 m4 s2 l1 d4. m8. z8 z4.", 96).unwrap();
        assert_eq!(ticks(&p.notes), [12, 6, 24, 48, 96, 36, 18, 12, 36]);
        assert_eq!(p.notes[7].pitch, None);
        assert!(p.bad_bars.is_empty());
        let m = parse_break_tune("d'8 s,,8", 96).unwrap();
        assert_eq!(m.notes[0].pitch, p_(0, 1));
        assert_eq!(m.notes[1].pitch, p_(7, -2));
    }

    fn p_(semis: u8, octave: i8) -> Option<TunePitch> {
        Some(TunePitch { semis, octave })
    }

    #[test]
    fn break_bar_lines_are_checked() {
        let ok = parse_break_tune("d8 d8 d4 d2 | s2 s2 |", 96).unwrap();
        assert!(ok.bad_bars.is_empty());
        let bad = parse_break_tune("d4 d4 d4 | d4 d4 d4 d4|d2", 96).unwrap();
        assert_eq!(bad.bad_bars, [(1, 72), (3, 48)]);
        // No bar lines: no check.
        assert!(parse_break_tune("d4 d4 d4", 96)
            .unwrap()
            .bad_bars
            .is_empty());
        // 6/8 bar is 72 ticks.
        assert!(parse_break_tune("d8 d8 d8 s8 s8 s8|", 72)
            .unwrap()
            .bad_bars
            .is_empty());
    }

    #[test]
    fn break_bad_tokens() {
        for bad in ["d", "d3", "x8", "d8..", "z", "8", "d32", "d,,,,,8", "dd8"] {
            assert!(parse_break_tune(bad, 96).is_err(), "{bad}");
        }
        assert_eq!(parse_break_tune("d8 q8", 96), Err("q8".into()));
        assert!(parse_break_tune("", 96).unwrap().notes.is_empty());
    }

    #[test]
    fn break_text_round_trip() {
        let p = parse_break_tune("d8 r16. m4 z2 s,4. d'1 l16", 96).unwrap();
        let back = parse_break_tune(&break_tune_text(&p.notes), 96).unwrap();
        assert_eq!(back.notes, p.notes);
    }

    #[test]
    fn bar_lengths_by_meter() {
        assert_eq!(bar_ticks(Meter::Four4), 96);
        assert_eq!(bar_ticks(Meter::Three4), 72);
        assert_eq!(bar_ticks(Meter::Six8), 72);
    }
}

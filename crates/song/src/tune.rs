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
        let root_len = tok.find([',', '\'']).unwrap_or(tok.len());
        let (root, marks) = tok.split_at(root_len);
        let Some(&(_, semis)) = ROOTS.iter().find(|(n, _)| *n == root) else {
            return Err(tok.to_string());
        };
        let mut octave: i32 = 0;
        for c in marks.chars() {
            match c {
                ',' => octave -= 1,
                '\'' => octave += 1,
                _ => return Err(tok.to_string()),
            }
        }
        if marks.len() as i32 > MAX_MARKS {
            return Err(tok.to_string());
        }
        out.push(TuneNote {
            pitch: Some(TunePitch {
                semis,
                octave: octave as i8,
            }),
            hold: 0,
        });
    }
    Ok(out)
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
}

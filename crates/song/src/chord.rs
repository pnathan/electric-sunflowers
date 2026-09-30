//! Chord symbols: parsing, transposition and the per-song chord table.
//!
//! Grammar: `ROOT QUALITY [/BASS]`. ROOT and BASS are a letter A-G (either
//! case) with an optional `#`, `b`, U+266F or U+266D. QUALITY is matched
//! against `QUALITIES` by longest prefix, ignoring parentheses and
//! whitespace; text after the longest match is reported as ignored (so
//! "C7#9" is C7 with "#9" ignored). A slash starts a bass note only when a
//! note name follows it, so "C6/9" is a 6/9 chord.

use crate::pitch::{Pc, PcSet, FLATS, SHARPS};
use serde::Serialize;
use std::fmt;

/// Chord qualities: symbol suffix and intervals above the root in semitones.
const QUALITIES: &[(&str, &[u8])] = &[
    ("", &[0, 4, 7]),
    ("maj", &[0, 4, 7]),
    ("M", &[0, 4, 7]),
    ("m", &[0, 3, 7]),
    ("min", &[0, 3, 7]),
    ("-", &[0, 3, 7]),
    ("7", &[0, 4, 7, 10]),
    ("maj7", &[0, 4, 7, 11]),
    ("M7", &[0, 4, 7, 11]),
    ("m7", &[0, 3, 7, 10]),
    ("min7", &[0, 3, 7, 10]),
    ("-7", &[0, 3, 7, 10]),
    ("mmaj7", &[0, 3, 7, 11]),
    ("mM7", &[0, 3, 7, 11]),
    ("6", &[0, 4, 7, 9]),
    ("m6", &[0, 3, 7, 9]),
    ("69", &[0, 4, 7, 9, 2]),
    ("6/9", &[0, 4, 7, 9, 2]),
    ("9", &[0, 4, 7, 10, 2]),
    ("maj9", &[0, 4, 7, 11, 2]),
    ("m9", &[0, 3, 7, 10, 2]),
    ("add9", &[0, 4, 7, 2]),
    ("add2", &[0, 2, 4, 7]),
    ("madd9", &[0, 3, 7, 2]),
    ("sus2", &[0, 2, 7]),
    ("sus4", &[0, 5, 7]),
    ("sus", &[0, 5, 7]),
    ("7sus4", &[0, 5, 7, 10]),
    ("7sus", &[0, 5, 7, 10]),
    ("7sus2", &[0, 2, 7, 10]),
    ("dim", &[0, 3, 6]),
    ("o", &[0, 3, 6]),
    ("dim7", &[0, 3, 6, 9]),
    ("o7", &[0, 3, 6, 9]),
    ("m7b5", &[0, 3, 6, 10]),
    ("\u{f8}", &[0, 3, 6, 10]),
    ("\u{f8}7", &[0, 3, 6, 10]),
    ("aug", &[0, 4, 8]),
    ("+", &[0, 4, 8]),
    ("5", &[0, 7]),
    ("11", &[0, 4, 7, 10, 5]),
    ("m11", &[0, 3, 7, 10, 5]),
    ("13", &[0, 4, 7, 10, 9]),
    ("7b9", &[0, 4, 7, 10, 1]),
];

#[inline]
fn skippable(b: u8) -> bool {
    b == b'(' || b == b')' || b.is_ascii_whitespace()
}

/// Bytes of `q` consumed when `key` matches a prefix of `q` with
/// parentheses and whitespace in `q` skipped, or `None`.
fn match_prefix(q: &[u8], key: &[u8]) -> Option<usize> {
    let mut i = 0;
    for &k in key {
        while i < q.len() && skippable(q[i]) {
            i += 1;
        }
        if i == q.len() || q[i] != k {
            return None;
        }
        i += 1;
    }
    Some(i)
}

/// Longest quality whose symbol is a prefix of `q`: (symbol, intervals, bytes consumed).
fn match_quality(q: &str) -> (&'static str, &'static [u8], usize) {
    let q = q.as_bytes();
    // QUALITIES[0] is the empty suffix (major triad), a prefix of anything.
    let mut best = (QUALITIES[0].0, QUALITIES[0].1, 0usize);
    for &(key, iv) in QUALITIES {
        if key.len() > best.0.len() {
            if let Some(n) = match_prefix(q, key.as_bytes()) {
                best = (key, iv, n);
            }
        }
    }
    best
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChordError {
    Empty,
    /// The symbol does not start with a note name.
    NoRoot(String),
}

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordError::Empty => f.write_str("empty chord symbol"),
            ChordError::NoRoot(s) => {
                write!(f, "chord symbol {s:?} does not start with a note name")
            }
        }
    }
}

impl std::error::Error for ChordError {}

/// A parsed chord.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Chord {
    /// The symbol, trimmed, with U+266F/U+266D written as `#`/`b`.
    pub symbol: String,
    /// The matched quality suffix ("m7", "" for a major triad).
    pub quality: &'static str,
    /// The quality's intervals above the root in semitones, in the order of
    /// the quality table: ascending, except that an added tone (the 9th of
    /// 6/9, 9, add9; the 11th of 11; the 13th of 13; the b9 of 7b9) comes
    /// last. Not serialised; `quality` names it.
    #[serde(skip)]
    pub intervals: &'static [u8],
    pub root: Pc,
    /// Lowest note: the slash bass, else the root.
    pub bass: Pc,
    pub tones: PcSet,
    pub third: Option<Pc>,
    pub fifth: Option<Pc>,
    pub seventh: Option<Pc>,
    /// Tones that define the chord: root; third (or, without one, the
    /// suspended 4th and 2nd); seventh; diminished 5th without a perfect
    /// 5th; augmented 5th.
    pub essential: PcSet,
}

impl Chord {
    /// Parses a chord symbol. Ignored quality text is dropped; use
    /// `parse_detail` to see it.
    pub fn parse(s: &str) -> Result<Chord, ChordError> {
        parse_detail(s).map(|(c, _)| c)
    }

    /// Major or minor triad on `root`, for defaults.
    pub fn triad(root: Pc, minor: bool, flats: bool) -> Chord {
        let mut sym = String::from(root.name(flats));
        if minor {
            sym.push('m');
        }
        // A note name plus an optional "m" always parses.
        parse_detail(&sym)
            .map(|(c, _)| c)
            .unwrap_or_else(|_| build(sym, root, root, "", &[0, 4, 7]))
    }
}

impl Chord {
    /// The chord's pitch classes in quality-table order (see `intervals`):
    /// root first, an added tone last.
    pub fn tones_in_order(&self) -> impl Iterator<Item = Pc> + '_ {
        self.intervals
            .iter()
            .map(move |&iv| self.root.transpose(iv as i32))
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.symbol)
    }
}

impl std::str::FromStr for Chord {
    type Err = ChordError;
    fn from_str(s: &str) -> Result<Chord, ChordError> {
        Chord::parse(s)
    }
}

fn normalise(s: &str) -> String {
    let t = s.trim();
    if t.contains(['\u{266f}', '\u{266d}']) {
        t.replace('\u{266f}', "#").replace('\u{266d}', "b")
    } else {
        t.to_string()
    }
}

fn build(symbol: String, root: Pc, bass: Pc, quality: &'static str, iv: &'static [u8]) -> Chord {
    let has = |x: u8| iv.contains(&x);
    let at = |x: u8| root.transpose(x as i32);
    let tones = PcSet::from_intervals(root, iv);
    let third = if has(3) {
        Some(at(3))
    } else if has(4) {
        Some(at(4))
    } else {
        None
    };
    let seventh = if has(10) {
        Some(at(10))
    } else if has(11) {
        Some(at(11))
    } else {
        None
    };
    let fifth = has(7).then(|| at(7));
    let mut essential = PcSet::EMPTY.with(root);
    match third {
        Some(t) => essential.insert(t),
        None => {
            if has(5) {
                essential.insert(at(5));
            }
            if has(2) {
                essential.insert(at(2));
            }
        }
    }
    if let Some(s) = seventh {
        essential.insert(s);
    }
    if has(6) && !has(7) {
        essential.insert(at(6));
    }
    if has(8) {
        essential.insert(at(8));
    }
    Chord {
        symbol,
        quality,
        intervals: iv,
        root,
        bass,
        tones,
        third,
        fifth,
        seventh,
        essential,
    }
}

/// Parses a chord symbol and returns the text of the quality that was not
/// understood (after the longest known prefix), if any.
pub fn parse_detail(s: &str) -> Result<(Chord, Option<String>), ChordError> {
    let symbol = normalise(s);
    if symbol.is_empty() {
        return Err(ChordError::Empty);
    }
    let (root, rlen) =
        Pc::parse_prefix(&symbol).ok_or_else(|| ChordError::NoRoot(symbol.clone()))?;
    let body = &symbol[rlen..];
    // Bass: the last '/' followed by a note name and nothing else.
    let (qual, bass) = match body.rfind('/') {
        Some(i) => match Pc::parse_prefix(&body[i + 1..]) {
            Some((b, blen)) if i + 1 + blen == body.len() => (&body[..i], b),
            _ => (body, root),
        },
        None => (body, root),
    };
    let (qname, iv, used) = match_quality(qual);
    let rest: String = qual[used..]
        .chars()
        .filter(|c| !(c.is_whitespace() || *c == '(' || *c == ')'))
        .collect();
    let ignored = (!rest.is_empty()).then_some(rest);
    Ok((build(symbol, root, bass, qname, iv), ignored))
}

/// Moves every note name in a chord symbol (the root, and the bass after a
/// slash) by `semis` semitones, spelling with flats or sharps.
pub fn transpose_symbol(s: &str, semis: i32, flats: bool) -> String {
    let names = if flats { &FLATS } else { &SHARPS };
    let mut out = String::with_capacity(s.len() + 2);
    let mut i = 0;
    let mut at_name = true;
    while i < s.len() {
        let rest = &s[i..];
        if at_name {
            if let Some((pc, n)) = Pc::parse_prefix(rest) {
                out.push_str(names[pc.transpose(semis).get() as usize]);
                i += n;
                at_name = false;
                continue;
            }
        }
        // `rest` is non-empty and starts at a char boundary.
        let c = rest.chars().next().unwrap_or('\0');
        out.push(c);
        at_name = c == '/';
        i += c.len_utf8();
    }
    out
}

/// Index of a chord in a song's `ChordTable`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ChordId(u16);

impl ChordId {
    /// The id at table position `i`; `None` beyond the id range.
    pub fn from_index(i: usize) -> Option<ChordId> {
        u16::try_from(i).ok().map(ChordId)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The distinct chords of a song, in order of first use. `ChordId`s are
/// only made by `intern`, so every id indexes a chord in its table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ChordTable {
    chords: Vec<Chord>,
}

impl ChordTable {
    pub fn new() -> ChordTable {
        ChordTable::default()
    }

    /// Adds `c` unless a chord with the same symbol is present; returns its id.
    /// Returns `None` only when the table already holds 65536 chords.
    pub fn intern(&mut self, c: Chord) -> Option<ChordId> {
        if let Some(i) = self.chords.iter().position(|x| x.symbol == c.symbol) {
            return Some(ChordId(i as u16));
        }
        let id = u16::try_from(self.chords.len()).ok()?;
        self.chords.push(c);
        Some(ChordId(id))
    }

    /// The chord for `id`. `id` must come from this table.
    pub fn get(&self, id: ChordId) -> &Chord {
        &self.chords[id.index()]
    }

    pub fn len(&self) -> usize {
        self.chords.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chords.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (ChordId, &Chord)> {
        self.chords
            .iter()
            .enumerate()
            .map(|(i, c)| (ChordId(i as u16), c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcs(c: &Chord) -> Vec<u8> {
        c.tones.iter().map(Pc::get).collect()
    }

    #[test]
    fn qualities() {
        assert_eq!(pcs(&Chord::parse("C").unwrap()), vec![0, 4, 7]);
        assert_eq!(pcs(&Chord::parse("Dm7").unwrap()), vec![0, 2, 5, 9]);
        let (c, ign) = parse_detail("C7#9").unwrap();
        assert_eq!(pcs(&c), vec![0, 4, 7, 10]);
        assert_eq!(ign.as_deref(), Some("#9"));
        let (c, ign) = parse_detail("Dm(add9)").unwrap();
        assert_eq!(c.quality, "madd9");
        assert_eq!(ign, None);
        let c = Chord::parse("C/E").unwrap();
        assert_eq!((c.root.get(), c.bass.get()), (0, 4));
        let (c, ign) = parse_detail("C6/9").unwrap();
        assert_eq!((c.quality, c.bass.get(), ign), ("6/9", 0, None));
        let c = Chord::parse("F\u{266f}m").unwrap();
        assert_eq!((c.symbol.as_str(), c.root.get()), ("F#m", 6));
        let c = Chord::parse("B\u{f8}7").unwrap();
        assert_eq!(c.quality, "\u{f8}7");
        assert!(c.essential.contains(Pc::new(5)));
        assert_eq!(Chord::parse(""), Err(ChordError::Empty));
        assert!(matches!(Chord::parse("N.C."), Err(ChordError::NoRoot(_))));
    }

    #[test]
    fn intervals_keep_table_order() {
        assert_eq!(Chord::parse("C9").unwrap().intervals, &[0, 4, 7, 10, 2]);
        assert_eq!(Chord::parse("Cadd2").unwrap().intervals, &[0, 2, 4, 7]);
        let d: Vec<u8> = Chord::parse("Dm7")
            .unwrap()
            .tones_in_order()
            .map(Pc::get)
            .collect();
        assert_eq!(d, vec![2, 5, 9, 0]);
    }

    #[test]
    fn essential_tones() {
        let c = Chord::parse("Gsus4").unwrap();
        assert_eq!(c.third, None);
        assert_eq!(
            c.essential.iter().map(Pc::get).collect::<Vec<_>>(),
            vec![0, 7]
        );
        let c = Chord::parse("Caug").unwrap();
        assert!(c.essential.contains(Pc::new(8)));
        assert_eq!(c.fifth, None);
    }

    #[test]
    fn transpose() {
        assert_eq!(transpose_symbol("C", 2, false), "D");
        assert_eq!(transpose_symbol("C/E", 2, false), "D/F#");
        assert_eq!(transpose_symbol("Cm7", 1, false), "C#m7");
        assert_eq!(transpose_symbol("Cm7", 1, true), "Dbm7");
        assert_eq!(transpose_symbol("C6/9", 2, false), "D6/9");
        assert_eq!(transpose_symbol("Bb\u{f8}7", 2, false), "C\u{f8}7");
        assert_eq!(transpose_symbol("E\u{266d}", -3, false), "C");
    }

    #[test]
    fn table_interns() {
        let mut t = ChordTable::new();
        let a = t.intern(Chord::parse("G").unwrap()).unwrap();
        let b = t.intern(Chord::parse("D").unwrap()).unwrap();
        let c = t.intern(Chord::parse(" G ").unwrap()).unwrap();
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(t.len(), 2);
        assert_eq!(t.get(b).symbol, "D");
    }
}

//! Pitch classes, pitch-class sets and MIDI helpers.
//!
//! A pitch class is an integer mod 12 with C = 0. A `PcSet` is a 12-bit mask,
//! bit `p` set when pitch class `p` is present, so set algebra and
//! transposition (a 12-bit rotation) are single integer operations.

use serde::Serialize;
use std::fmt;

/// Sharp spellings of the twelve pitch classes, C = 0.
pub const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// Flat spellings of the twelve pitch classes, C = 0.
pub const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// A pitch class, 0..12, C = 0. The constructor reduces mod 12, so every
/// value is in range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize)]
#[serde(transparent)]
pub struct Pc(u8);

impl Pc {
    pub const C: Pc = Pc(0);

    /// Pitch class of any integer, reduced mod 12 (negative values wrap).
    #[inline]
    pub const fn new(v: i32) -> Pc {
        Pc(v.rem_euclid(12) as u8)
    }

    /// Pitch class of a MIDI note number.
    #[inline]
    pub const fn of_midi(m: i32) -> Pc {
        Pc::new(m)
    }

    #[inline]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// This pitch class moved by `semis` semitones.
    #[inline]
    pub const fn transpose(self, semis: i32) -> Pc {
        Pc::new(self.0 as i32 + semis)
    }

    /// Upward interval from `self` to `to`, 0..12.
    #[inline]
    pub const fn interval_to(self, to: Pc) -> u8 {
        Pc::new(to.0 as i32 - self.0 as i32).0
    }

    /// Note name with sharps or flats.
    pub const fn name(self, flats: bool) -> &'static str {
        if flats {
            FLATS[self.0 as usize]
        } else {
            SHARPS[self.0 as usize]
        }
    }

    /// Reads a leading note name: a letter A-G (either case) and an optional
    /// accidental (`#`, `b`, U+266F sharp, U+266D flat). Returns the pitch
    /// class and the number of bytes read, or `None` when `s` does not start
    /// with a note letter.
    pub fn parse_prefix(s: &str) -> Option<(Pc, usize)> {
        let b = s.as_bytes();
        let base: i32 = match b.first()?.to_ascii_uppercase() {
            b'C' => 0,
            b'D' => 2,
            b'E' => 4,
            b'F' => 5,
            b'G' => 7,
            b'A' => 9,
            b'B' => 11,
            _ => return None,
        };
        let rest = &s[1..];
        let (acc, n) = if rest.starts_with('#') {
            (1, 1)
        } else if rest.starts_with('b') {
            (-1, 1)
        } else if rest.starts_with('\u{266f}') {
            (1, '\u{266f}'.len_utf8())
        } else if rest.starts_with('\u{266d}') {
            (-1, '\u{266d}'.len_utf8())
        } else {
            (0, 0)
        };
        Some((Pc::new(base + acc), 1 + n))
    }
}

impl fmt::Display for Pc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name(false))
    }
}

/// A set of pitch classes as a 12-bit mask (bit p = pitch class p).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(transparent)]
pub struct PcSet(u16);

impl PcSet {
    pub const EMPTY: PcSet = PcSet(0);
    const MASK: u16 = 0x0fff;

    /// Set from a raw mask; bits above 11 are discarded.
    #[inline]
    pub const fn from_bits(bits: u16) -> PcSet {
        PcSet(bits & Self::MASK)
    }

    #[inline]
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// Set of `root + i` for each interval `i`.
    pub const fn from_intervals(root: Pc, intervals: &[u8]) -> PcSet {
        let mut s = 0u16;
        let mut i = 0;
        while i < intervals.len() {
            s |= 1 << Pc::new(root.0 as i32 + intervals[i] as i32).0;
            i += 1;
        }
        PcSet(s)
    }

    #[inline]
    pub const fn contains(self, pc: Pc) -> bool {
        self.0 & (1 << pc.0) != 0
    }

    #[inline]
    pub fn insert(&mut self, pc: Pc) {
        self.0 |= 1 << pc.0;
    }

    #[inline]
    pub fn remove(&mut self, pc: Pc) {
        self.0 &= !(1 << pc.0);
    }

    #[inline]
    pub const fn with(self, pc: Pc) -> PcSet {
        PcSet(self.0 | (1 << pc.0))
    }

    #[inline]
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn union(self, o: PcSet) -> PcSet {
        PcSet(self.0 | o.0)
    }

    #[inline]
    pub const fn intersection(self, o: PcSet) -> PcSet {
        PcSet(self.0 & o.0)
    }

    #[inline]
    pub const fn difference(self, o: PcSet) -> PcSet {
        PcSet(self.0 & !o.0)
    }

    /// Every member moved by `semis` semitones: a rotation of the 12-bit mask.
    #[inline]
    pub const fn transpose(self, semis: i32) -> PcSet {
        let k = semis.rem_euclid(12) as u32;
        if k == 0 {
            return self;
        }
        PcSet(((self.0 << k) | (self.0 >> (12 - k))) & Self::MASK)
    }

    /// Members in ascending pitch-class order.
    #[inline]
    pub fn iter(self) -> impl Iterator<Item = Pc> + Clone {
        let bits = self.0;
        (0u8..12).filter(move |p| bits & (1 << p) != 0).map(Pc)
    }

    /// MIDI notes in `lo..=hi` whose pitch class is in the set, ascending.
    #[inline]
    pub fn tones_in(self, lo: u8, hi: u8) -> impl Iterator<Item = u8> + Clone {
        let bits = self.0;
        (lo..=hi.max(lo)).filter(move |&m| m <= hi && bits & (1 << (m % 12)) != 0)
    }
}

impl FromIterator<Pc> for PcSet {
    fn from_iter<I: IntoIterator<Item = Pc>>(it: I) -> PcSet {
        let mut s = PcSet::EMPTY;
        for p in it {
            s.insert(p);
        }
        s
    }
}

/// Frequency in Hz of a fractional MIDI note, A4 = 69 = 440 Hz, equal temperament.
#[inline]
pub fn midi_to_hz(m: f64) -> f64 {
    440.0 * ((m - 69.0) / 12.0).exp2()
}

/// Fractional MIDI note of a frequency in Hz (inverse of `midi_to_hz`).
#[inline]
pub fn hz_to_midi(hz: f64) -> f64 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

/// Shifts the MIDI note `m` by whole octaves into `[lo, hi]`. The result is
/// the lowest octave transposition at or above `lo`; when the range is
/// narrower than an octave and that note lies above `hi`, the octave nearer
/// the range centre is taken (ties go up). Same rule as `sfcore::math::fold_octave`
/// on integers.
#[inline]
pub const fn fold_octave(m: i32, lo: i32, hi: i32) -> i32 {
    // Smallest k with m + 12k >= lo.
    let k = (lo - m + 11).div_euclid(12);
    let up = m + 12 * k;
    if up <= hi {
        return up;
    }
    let down = up - 12;
    // Compare distances to the centre in doubled units to stay in integers.
    let mid2 = lo + hi;
    if (2 * down - mid2).abs() < (2 * up - mid2).abs() {
        down
    } else {
        up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_wraps_and_names() {
        assert_eq!(Pc::new(-1).get(), 11);
        assert_eq!(Pc::new(25).get(), 1);
        assert_eq!(Pc::new(10).name(true), "Bb");
        assert_eq!(Pc::new(10).name(false), "A#");
        assert_eq!(Pc::new(2).interval_to(Pc::new(0)), 10);
    }

    #[test]
    fn parse_prefix_accidentals() {
        assert_eq!(Pc::parse_prefix("C"), Some((Pc::new(0), 1)));
        assert_eq!(Pc::parse_prefix("Db7"), Some((Pc::new(1), 2)));
        assert_eq!(Pc::parse_prefix("bb"), Some((Pc::new(10), 2)));
        assert_eq!(Pc::parse_prefix("F\u{266f}m"), Some((Pc::new(6), 4)));
        assert_eq!(Pc::parse_prefix("E\u{266d}"), Some((Pc::new(3), 4)));
        assert_eq!(Pc::parse_prefix("Cb"), Some((Pc::new(11), 2)));
        assert_eq!(Pc::parse_prefix("H"), None);
        assert_eq!(Pc::parse_prefix(""), None);
    }

    #[test]
    fn pcset_ops() {
        let c = PcSet::from_intervals(Pc::C, &[0, 4, 7]);
        assert_eq!(c.bits(), 0b1001_0001);
        assert!(c.contains(Pc::new(4)));
        let d = c.transpose(2);
        assert_eq!(d.iter().map(Pc::get).collect::<Vec<_>>(), vec![2, 6, 9]);
        let b = c.transpose(-1);
        assert_eq!(b.iter().map(Pc::get).collect::<Vec<_>>(), vec![3, 6, 11]);
        assert_eq!(c.transpose(12), c);
        assert_eq!(c.tones_in(58, 67).collect::<Vec<_>>(), vec![60, 64, 67]);
        assert_eq!(c.tones_in(70, 60).count(), 0);
        assert_eq!(c.tones_in(255, 255).count(), 0);
    }

    #[test]
    fn fold_octave_rule() {
        assert_eq!(fold_octave(40, 48, 60), 52);
        assert_eq!(fold_octave(75, 48, 60), 51);
        assert_eq!(fold_octave(48, 48, 60), 48);
        // Narrow range: 50..=52, note D#: up = 51 in range.
        assert_eq!(fold_octave(39, 50, 52), 51);
        // Narrow range: 50..=52, note A (57, 45): up = 57, down = 45, centre 51 -> 57 (6 vs 6: tie goes up).
        assert_eq!(fold_octave(45, 50, 52), 57);
        assert_eq!(fold_octave(46, 50, 52), 46);
    }

    #[test]
    fn hz_round_trip() {
        assert!((midi_to_hz(69.0) - 440.0).abs() < 1e-12);
        assert!((hz_to_midi(midi_to_hz(61.3)) - 61.3).abs() < 1e-12);
    }
}

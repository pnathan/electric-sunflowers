//! ARPAbet phonemes as a closed enum.
//!
//! The set is the ARPAbet subset the voice synthesises (CMU dictionary
//! symbols without stress digits), plus `oh` (a pure mid-back rounded vowel)
//! and two targets used only inside the voice: `ey0` and `ey1`, the start
//! and end of the /ey/ diphthong (Hillenbrand et al. 1995 values live in
//! `voice::phoneme`), and `dx`, the alveolar flap. Order: monophthong
//! vowels, diphthongs, consonants; the voice's acoustic tables are indexed
//! by `Phoneme as usize` in this order.

use serde::Serialize;
use std::fmt;
use std::str::FromStr;

macro_rules! phonemes {
    ($($var:ident = $s:literal),+ $(,)?) => {
        /// One ARPAbet phoneme. See the module doc for the set and order.
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Phoneme { $($var),+ }

        impl Phoneme {
            /// Every phoneme in table order.
            pub const ALL: &'static [Phoneme] = &[$(Phoneme::$var),+];
            /// Lower-case ARPAbet symbol.
            pub const fn symbol(self) -> &'static str {
                match self { $(Phoneme::$var => $s),+ }
            }
            /// Exact canonical symbol lookup (lower case, no aliases).
            pub fn from_symbol(s: &str) -> Option<Phoneme> {
                match s { $($s => Some(Phoneme::$var),)+ _ => None }
            }
        }
    };
}

phonemes! {
    // Monophthong vowels.
    Iy = "iy", Ih = "ih", Eh = "eh", Ae = "ae", Aa = "aa", Ao = "ao", Oh = "oh",
    Uh = "uh", Uw = "uw", Ah = "ah", Ax = "ax", Er = "er", Ey0 = "ey0", Ey1 = "ey1",
    // Diphthongs.
    Ay = "ay", Aw = "aw", Ey = "ey", Ow = "ow", Oy = "oy",
    // Consonants: sonorants, nasals, fricatives, aspirate, stops, affricates.
    L = "l", R = "r", W = "w", Y = "y",
    M = "m", N = "n", Ng = "ng",
    S = "s", Z = "z", Sh = "sh", Zh = "zh", F = "f", V = "v", Th = "th", Dh = "dh",
    Hh = "hh",
    P = "p", B = "b", T = "t", D = "d", Dx = "dx", K = "k", G = "g",
    Ch = "ch", Jh = "jh",
}

impl Phoneme {
    /// Number of phonemes; tables indexed by `Phoneme as usize` have this length.
    pub const COUNT: usize = Phoneme::ALL.len();

    /// Monophthong vowels and diphthongs (a syllable nucleus).
    pub const fn is_vowel(self) -> bool {
        (self as u8) <= (Phoneme::Oy as u8)
    }

    pub const fn is_diphthong(self) -> bool {
        (self as u8) >= (Phoneme::Ay as u8) && (self as u8) <= (Phoneme::Oy as u8)
    }

    pub const fn is_consonant(self) -> bool {
        !self.is_vowel()
    }

    /// The two vowel targets of a diphthong: ay = aa-ih, aw = aa-uh,
    /// ey = ey0-ey1, ow = oh-uh, oy = ao-ih.
    pub const fn diphthong_targets(self) -> Option<[Phoneme; 2]> {
        match self {
            Phoneme::Ay => Some([Phoneme::Aa, Phoneme::Ih]),
            Phoneme::Aw => Some([Phoneme::Aa, Phoneme::Uh]),
            Phoneme::Ey => Some([Phoneme::Ey0, Phoneme::Ey1]),
            Phoneme::Ow => Some([Phoneme::Oh, Phoneme::Uh]),
            Phoneme::Oy => Some([Phoneme::Ao, Phoneme::Ih]),
            _ => None,
        }
    }

    /// Parses one ARPAbet token as the model writes it: case-insensitive,
    /// stress digits removed, then aliases resolved. Aliases: h, x -> hh;
    /// j -> jh; ix -> ih; ux -> uw; el -> l; em -> m; en -> n; axr -> er;
    /// q (glottal stop) -> t; dx -> d (the model's flap is sung as d; `Dx` is
    /// produced only inside the voice). Returns `None` for an unknown token.
    /// No allocation: tokens longer than 8 bytes are unknown.
    pub fn parse_token(tok: &str) -> Option<Phoneme> {
        let mut buf = [0u8; 8];
        let mut n = 0;
        for &b in tok.trim().as_bytes() {
            if b.is_ascii_digit() {
                continue;
            }
            if n == buf.len() || !b.is_ascii_alphabetic() {
                return None;
            }
            buf[n] = b.to_ascii_lowercase();
            n += 1;
        }
        let s = std::str::from_utf8(&buf[..n]).ok()?;
        let s = match s {
            "h" | "x" => "hh",
            "j" => "jh",
            "ix" => "ih",
            "ux" => "uw",
            "el" => "l",
            "em" => "m",
            "en" => "n",
            "dx" => "d",
            "q" => "t",
            "axr" => "er",
            other => other,
        };
        match Phoneme::from_symbol(s)? {
            // Digits are stripped above, so "ey0"/"ey1" arrive as "ey";
            // these two never come from input.
            Phoneme::Ey0 | Phoneme::Ey1 | Phoneme::Dx => None,
            p => Some(p),
        }
    }
}

impl FromStr for Phoneme {
    type Err = crate::UnknownName;
    /// Same rules as `parse_token`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Phoneme::parse_token(s).ok_or_else(|| crate::UnknownName {
            kind: "phoneme",
            text: s.to_string(),
        })
    }
}

impl fmt::Display for Phoneme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

impl Serialize for Phoneme {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_order_and_classes() {
        assert_eq!(Phoneme::COUNT, 44);
        for (i, p) in Phoneme::ALL.iter().enumerate() {
            assert_eq!(*p as usize, i);
            assert_eq!(Phoneme::from_symbol(p.symbol()), Some(*p));
        }
        assert!(Phoneme::Er.is_vowel());
        assert!(Phoneme::Ey0.is_vowel() && !Phoneme::Ey0.is_diphthong());
        assert!(Phoneme::Oy.is_diphthong());
        assert!(!Phoneme::L.is_vowel());
        for p in Phoneme::ALL {
            assert_eq!(p.is_diphthong(), p.diphthong_targets().is_some());
        }
    }

    #[test]
    fn tokens_and_aliases() {
        assert_eq!(Phoneme::parse_token("AH0"), Some(Phoneme::Ah));
        assert_eq!(Phoneme::parse_token("ey1"), Some(Phoneme::Ey));
        assert_eq!(Phoneme::parse_token("dx"), Some(Phoneme::D));
        assert_eq!(Phoneme::parse_token("axr"), Some(Phoneme::Er));
        assert_eq!(Phoneme::parse_token("h"), Some(Phoneme::Hh));
        assert_eq!(Phoneme::parse_token("oh"), Some(Phoneme::Oh));
        assert_eq!(Phoneme::parse_token("zz"), None);
        assert_eq!(Phoneme::parse_token("\u{e9}"), None);
        assert_eq!(Phoneme::parse_token("abcdefghij"), None);
        assert_eq!("NG".parse::<Phoneme>(), Ok(Phoneme::Ng));
    }
}

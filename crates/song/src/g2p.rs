//! Grapheme-to-phoneme fallback for one syllable.
//!
//! Algorithm: greedy longest-match over the syllable's ASCII letters. At each
//! position the longest grapheme in `GRAPHEMES` that matches is emitted and
//! consumed; a letter with no entry is skipped. Context rules first: `y`
//! is /y/ at the start and /iy/ elsewhere; `c` before e, i or y is /s/. A
//! final silent `e` is dropped when the letter before it is a consonant and
//! an earlier vowel letter exists ("love" -> l ah v). A result without a
//! vowel gets a schwa /ax/ so every syllable has a nucleus.
//!
//! This is a crude English rule set, used only when the model's ARPAbet for
//! a syllable is missing or unusable.

use crate::phoneme::Phoneme;
use crate::phoneme::Phoneme::*;

/// Grapheme table, longest graphemes first. Matching is greedy longest
/// match; entries of equal length never overlap at one position, so order
/// within a length does not matter.
const GRAPHEMES: &[(&[u8], &[Phoneme])] = &[
    (b"ough", &[Ao]),
    (b"augh", &[Ao]),
    (b"eigh", &[Ey]),
    (b"tion", &[Sh, Ax, N]),
    (b"sion", &[Zh, Ax, N]),
    (b"tch", &[Ch]),
    (b"igh", &[Ay]),
    (b"ck", &[K]),
    (b"ch", &[Ch]),
    (b"sh", &[Sh]),
    (b"th", &[Th]),
    (b"ph", &[F]),
    (b"wh", &[W]),
    (b"ng", &[Ng]),
    (b"qu", &[K, W]),
    (b"wr", &[R]),
    (b"kn", &[N]),
    (b"gh", &[]),
    (b"ee", &[Iy]),
    (b"ea", &[Iy]),
    (b"oo", &[Uw]),
    (b"ou", &[Aw]),
    (b"ow", &[Ow]),
    (b"oa", &[Ow]),
    (b"ai", &[Ey]),
    (b"ay", &[Ey]),
    (b"oi", &[Oy]),
    (b"oy", &[Oy]),
    (b"au", &[Ao]),
    (b"aw", &[Ao]),
    (b"ie", &[Iy]),
    (b"ei", &[Ey]),
    (b"ew", &[Uw]),
    (b"ue", &[Uw]),
    (b"er", &[Er]),
    (b"ir", &[Er]),
    (b"ur", &[Er]),
    (b"ar", &[Aa, R]),
    (b"or", &[Ao, R]),
    (b"a", &[Ae]),
    (b"e", &[Eh]),
    (b"i", &[Ih]),
    (b"o", &[Aa]),
    (b"u", &[Ah]),
    (b"b", &[B]),
    (b"c", &[K]),
    (b"d", &[D]),
    (b"f", &[F]),
    (b"g", &[G]),
    (b"h", &[Hh]),
    (b"j", &[Jh]),
    (b"k", &[K]),
    (b"l", &[L]),
    (b"m", &[M]),
    (b"n", &[N]),
    (b"p", &[P]),
    (b"r", &[R]),
    (b"s", &[S]),
    (b"t", &[T]),
    (b"v", &[V]),
    (b"w", &[W]),
    (b"x", &[K, S]),
    (b"z", &[Z]),
];

#[inline]
fn vowel_letter(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'y')
}

/// Phonemes for one syllable of English text. Always returns at least one vowel.
pub fn g2p(syl: &str) -> Vec<Phoneme> {
    // Lower-case ASCII letters only (Unicode lower-casing first, so a
    // character that lower-cases to an ASCII letter counts).
    let mut s: Vec<u8> = syl
        .chars()
        .flat_map(char::to_lowercase)
        .filter(char::is_ascii_lowercase)
        .map(|c| c as u8)
        .collect();
    let n = s.len();
    if n > 2
        && s[n - 1] == b'e'
        && !vowel_letter(s[n - 2])
        && s[..n - 1].iter().any(|&b| vowel_letter(b))
    {
        s.pop();
    }
    let s = &s[..];
    let mut out = Vec::with_capacity(s.len() + 1);
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'y' {
            out.push(if i == 0 { Y } else { Iy });
            i += 1;
            continue;
        }
        if c == b'c' && matches!(s.get(i + 1), Some(b'e' | b'i' | b'y')) {
            out.push(S);
            i += 1;
            continue;
        }
        let rest = &s[i..];
        match GRAPHEMES.iter().find(|(g, _)| rest.starts_with(g)) {
            Some((g, ph)) => {
                out.extend_from_slice(ph);
                i += g.len();
            }
            None => i += 1,
        }
    }
    if !out.iter().any(|p| p.is_vowel()) {
        out.push(Ax);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_longest_first() {
        for w in GRAPHEMES.windows(2) {
            assert!(w[0].0.len() >= w[1].0.len());
        }
    }

    #[test]
    fn spot_checks() {
        assert_eq!(g2p("cat"), vec![K, Ae, T]);
        assert_eq!(g2p("sing"), vec![S, Ih, Ng]);
        assert_eq!(g2p("love"), vec![L, Aa, V]);
        assert_eq!(g2p("night"), vec![N, Ay, T]);
        assert_eq!(g2p("city"), vec![S, Ih, T, Iy]);
        assert_eq!(g2p("yes"), vec![Y, Eh, S]);
        assert_eq!(g2p("Nation's"), vec![N, Ae, Sh, Ax, N, S]);
        assert_eq!(g2p(""), vec![Ax]);
        assert_eq!(g2p("--"), vec![Ax]);
        assert_eq!(g2p("rhythm"), vec![R, Hh, Iy, Th, M]);
    }
}

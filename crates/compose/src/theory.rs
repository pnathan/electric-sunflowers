//! Scale helpers over the typed chord model (`song::Chord`).

use song::{Chord, Mode, Pc, PcSet};

/// The chord's tones in the order of its quality's interval list
/// (`Chord::intervals`): root, then ascending, an added tone last. Returns
/// the tones and their count.
pub fn ordered_tones(c: &Chord) -> ([Pc; 12], usize) {
    let mut out = [Pc::C; 12];
    let mut n = 0;
    for p in c.tones_in_order().take(12) {
        out[n] = p;
        n += 1;
    }
    (out, n)
}

/// Generic scale degree (0 = unison, 2 = third, ...) of the chord tone
/// `iv` semitones above the root, for the qualities in `song::chord`.
/// Semitones 6 and 8 are the altered fifth (dim, m7b5, aug); 9 is the
/// diminished seventh in a dim7 chord (a 6 without a perfect fifth over a
/// diminished fifth), else the sixth.
fn generic(iv: u8, intervals: &[u8]) -> usize {
    match iv % 12 {
        0 => 0,
        1 | 2 => 1,
        3 | 4 => 2,
        5 => 3,
        6..=8 => 4,
        9 if intervals.contains(&6) && !intervals.contains(&7) => 6,
        9 => 5,
        _ => 6,
    }
}

/// The scale of the song's mode on `tonic`, altered to fit `chord`, by
/// chord-scale spelling: each chord tone outside the scale is a chromatic
/// alteration of the scale step with its letter, and replaces that step.
///
/// The seven steps of a diatonic mode carry the seven letters in order. The
/// chord's root letter comes from its first tone (in `ordered_tones` order)
/// that is a scale step: step index minus the tone's generic degree. A tone
/// outside the scale then takes letter root + generic degree, and replaces
/// that step when the two are a semitone apart. So in C major, Bb (spelt
/// B-flat from its third D) replaces B, C7 gives C D E F G A Bb, D7 replaces
/// F with F#, E7 replaces G with G#.
///
/// When no chord tone is a scale step, or the lettered step is not a
/// semitone away or was already altered, the tone replaces the first
/// unaltered, non-chord step a semitone from it, or is added.
pub fn local_scale(tonic: Pc, mode: Mode, chord: &Chord) -> PcSet {
    let deg = mode.degrees();
    let mut s = [0u8; 12];
    let mut altered = [false; 12];
    let mut n = deg.len();
    for (i, &d) in deg.iter().enumerate() {
        s[i] = tonic.transpose(d as i32).get();
    }
    let iv = chord.intervals;
    // Root letter: step index of the first in-scale tone minus its degree.
    let root_letter = iv.iter().find_map(|&x| {
        let p = chord.root.transpose(x as i32).get();
        s[..n]
            .iter()
            .position(|&q| q == p)
            .map(|i| (i + 7 - generic(x, iv)) % 7)
    });
    let semi = |a: u8, b: u8| (a + 1) % 12 == b || (b + 1) % 12 == a;
    for &x in iv {
        let p = chord.root.transpose(x as i32).get();
        if s[..n].contains(&p) {
            continue;
        }
        if let Some(r) = root_letter {
            let l = (r + generic(x, iv)) % 7;
            if !altered[l] && semi(s[l], p) {
                s[l] = p;
                altered[l] = true;
                continue;
            }
        }
        // Scale steps and chord tones are distinct pitch classes, so at
        // most 12 entries are ever stored.
        let free = |i: usize| i < 7 && !altered[i] && !chord.tones.contains(Pc::new(s[i] as i32));
        match (0..n).find(|&i| free(i) && semi(s[i], p)) {
            Some(i) => {
                s[i] = p;
                altered[i] = true;
            }
            None => {
                s[n] = p;
                n += 1;
            }
        }
    }
    s[..n].iter().map(|&p| Pc::new(p as i32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(sym: &str) -> Vec<u8> {
        let c = Chord::parse(sym).unwrap();
        let (t, n) = ordered_tones(&c);
        t[..n].iter().map(|p| p.get()).collect()
    }

    #[test]
    fn tone_order_follows_the_quality() {
        assert_eq!(tones("C"), vec![0, 4, 7]);
        assert_eq!(tones("Dm7"), vec![2, 5, 9, 0]);
        assert_eq!(tones("C9"), vec![0, 4, 7, 10, 2]);
        assert_eq!(tones("Cadd2"), vec![0, 2, 4, 7]);
        assert_eq!(tones("Cadd9"), vec![0, 4, 7, 2]);
        assert_eq!(tones("C13"), vec![0, 4, 7, 10, 9]);
        assert_eq!(tones("C7b9"), vec![0, 4, 7, 10, 1]);
        assert_eq!(tones("Am11"), vec![9, 0, 4, 7, 2]);
    }

    #[test]
    fn local_scale_bends_to_the_chord() {
        let e7 = Chord::parse("E7").unwrap();
        let s = local_scale(Pc::C, Mode::Major, &e7);
        // G# replaces G.
        assert!(s.contains(Pc::new(8)) && !s.contains(Pc::new(7)));
        assert_eq!(s.len(), 7);
        let c = Chord::parse("C").unwrap();
        assert_eq!(local_scale(Pc::C, Mode::Major, &c), Mode::Major.scale());
    }

    fn scale(sym: &str, mode: Mode) -> Vec<u8> {
        let c = Chord::parse(sym).unwrap();
        let s = local_scale(Pc::C, mode, &c);
        (0..12u8)
            .filter(|&p| s.contains(Pc::new(p as i32)))
            .collect()
    }

    #[test]
    fn local_scale_replaces_the_altered_step() {
        // bVII and I7 in C major lower B, not A.
        assert_eq!(scale("Bb", Mode::Major), vec![0, 2, 4, 5, 7, 9, 10]);
        assert_eq!(scale("C7", Mode::Major), vec![0, 2, 4, 5, 7, 9, 10]);
        // V/V raises F, not G.
        assert_eq!(scale("D7", Mode::Major), vec![0, 2, 4, 6, 7, 9, 11]);
        // V/vi raises G.
        assert_eq!(scale("E7", Mode::Major), vec![0, 2, 4, 5, 8, 9, 11]);
        // bII lowers D and A.
        assert_eq!(scale("Db", Mode::Major), vec![0, 1, 4, 5, 7, 8, 11]);
        // Major V in C minor raises Bb.
        assert_eq!(scale("G7", Mode::Minor), vec![0, 2, 3, 5, 7, 8, 11]);
        // Diminished seventh on the raised seventh: B D F Ab in C major.
        assert_eq!(scale("Bdim7", Mode::Major), vec![0, 2, 4, 5, 7, 8, 11]);
    }

    fn scale_on(tonic: Pc, sym: &str, mode: Mode) -> Vec<u8> {
        let c = Chord::parse(sym).unwrap();
        let s = local_scale(tonic, mode, &c);
        (0..12u8)
            .filter(|&p| s.contains(Pc::new(p as i32)))
            .collect()
    }

    #[test]
    fn local_scale_alters_the_lettered_step_in_other_keys() {
        // D major in F major: F# replaces F (not G), Bb stays.
        assert_eq!(
            scale_on(Pc::new(5), "D", Mode::Major),
            vec![0, 2, 4, 6, 7, 9, 10]
        );
        // E major in A minor: G# replaces G (not A).
        assert_eq!(
            scale_on(Pc::new(9), "E", Mode::Minor),
            vec![0, 2, 4, 5, 8, 9, 11]
        );
    }
}

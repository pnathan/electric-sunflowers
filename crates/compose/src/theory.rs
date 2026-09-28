//! Scale helpers over the typed chord model (`song::Chord`).

use song::{Chord, Mode, Pc, PcSet};

/// Interval above the root that the chord's quality lists last although it
/// is not the highest: the added 9th of 6/9, 9, maj9, m9, add9 and madd9,
/// the 11th of 11 and m11, the 13th of 13, the b9 of 7b9. Every other
/// quality lists its intervals in ascending order.
fn added_tone(quality: &str) -> Option<u8> {
    match quality {
        "69" | "6/9" | "9" | "maj9" | "m9" | "add9" | "madd9" => Some(2),
        "11" | "m11" => Some(5),
        "13" => Some(9),
        "7b9" => Some(1),
        _ => None,
    }
}

/// The chord's tones in the order of its quality's interval list: root,
/// then the other tones by ascending interval, the added tone (see
/// `added_tone`) last. Returns the tones and their count.
pub fn ordered_tones(c: &Chord) -> ([Pc; 12], usize) {
    let mut out = [Pc::C; 12];
    let mut n = 0;
    let last = added_tone(c.quality);
    for iv in 0..12u8 {
        let p = c.root.transpose(iv as i32);
        if Some(iv) != last && c.tones.contains(p) {
            out[n] = p;
            n += 1;
        }
    }
    if let Some(iv) = last {
        let p = c.root.transpose(iv as i32);
        if c.tones.contains(p) {
            out[n] = p;
            n += 1;
        }
    }
    (out, n)
}

/// The scale of the song's mode on `tonic`, altered to fit `chord`: each
/// chord tone outside the scale replaces the first scale step (in scale
/// order) a semitone away from it, or is added when there is none. Chord
/// tones are taken in `ordered_tones` order, and replaced steps keep their
/// place in the scale order, so later tones see earlier replacements.
pub fn local_scale(tonic: Pc, mode: Mode, chord: &Chord) -> PcSet {
    let mut s = [0u8; 12];
    let mut n = 0;
    for &d in mode.degrees() {
        s[n] = tonic.transpose(d as i32).get();
        n += 1;
    }
    let (tones, nt) = ordered_tones(chord);
    for pc in &tones[..nt] {
        let p = pc.get();
        if s[..n].contains(&p) {
            continue;
        }
        // Scale steps and chord tones are distinct pitch classes, so at
        // most 12 entries are ever stored.
        match s[..n].iter().position(|&q| (q + 1) % 12 == p || (q + 11) % 12 == p) {
            Some(i) => s[i] = p,
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
}

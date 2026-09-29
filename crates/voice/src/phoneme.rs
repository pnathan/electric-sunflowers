//! Acoustic targets of the phonemes, as const tables indexed by
//! `song::Phoneme as usize`.
//!
//! Vowels: F1-F3 targets in Hz for an adult male tract (Peterson and Barney
//! 1952 with small changes; /ey/ start and end from Hillenbrand et al.
//! 1995). The voice type scales them (`VoiceParams::f1s`, `fs`).
//! Diphthongs glide between the two targets of
//! `Phoneme::diphthong_targets`.
//!
//! Consonants: synthesis-by-rule parameters (Klatt 1980; Holmes, Mattingly
//! and Shearme 1964): class, voicing, nominal duration, sonorant formants,
//! frication band, and for stops and affricates the closure time and the
//! F1-F3 locus (Delattre, Liberman and Cooper 1955) the following vowel's
//! formants start from. Velars have no fixed locus: it follows the vowel.

use song::Phoneme;

/// Manner class of a consonant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsClass {
    /// Liquids and glides (l r w y): voiced, formant targets.
    Sonorant,
    /// m n ng: voiced, formant targets, nasal murmur.
    Nasal,
    /// Frication noise, voiced or not.
    Fricative,
    /// hh: aspiration noise through the vowel's formants.
    Aspirate,
    /// Closure, burst and, when voiceless, aspiration.
    Stop,
    /// Closure, burst, then frication.
    Affricate,
}

/// Where the CV formant transition of a stop or affricate starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Locus {
    /// Fixed F1-F3 locus in Hz.
    At([f64; 3]),
    /// Velar: F1 250 Hz (times F1 scale), F2 1.1 times the vowel's F2 up to
    /// 2300 Hz, F3 the vowel's.
    Velar,
}

/// One consonant's parameters. Fields that a class does not use are zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Consonant {
    pub class: ConsClass,
    pub voiced: bool,
    /// Nominal duration in seconds (sonorants, nasals, fricatives, hh).
    pub dur: f64,
    /// F1-F3 targets of a sonorant or nasal, Hz.
    pub formants: [f64; 3],
    /// Voicing amplitude of a sonorant or nasal.
    pub av: f64,
    /// Frication (or burst) noise band centre and bandwidth, Hz.
    pub ff: f64,
    pub bw: f64,
    /// Frication amplitude.
    pub af: f64,
    /// Voicing amplitude under a voiced fricative.
    pub vv: f64,
    /// Transition start of a stop or affricate.
    pub locus: Locus,
    /// Closure duration of a stop or affricate, seconds.
    pub closure: f64,
    /// Frication duration of an affricate, seconds.
    pub fric_dur: f64,
    /// Flap: a very short voiced closure (the /t d/ between vowels).
    pub flap: bool,
}

const BLANK: Consonant = Consonant {
    class: ConsClass::Sonorant,
    voiced: true,
    dur: 0.0,
    formants: [0.0; 3],
    av: 0.0,
    ff: 0.0,
    bw: 0.0,
    af: 0.0,
    vv: 0.35,
    locus: Locus::Velar,
    closure: 0.0,
    fric_dur: 0.0,
    flap: false,
};

const fn son(class: ConsClass, formants: [f64; 3], av: f64, dur: f64) -> Consonant {
    Consonant {
        class,
        formants,
        av,
        dur,
        ..BLANK
    }
}

const fn fric(ff: f64, bw: f64, af: f64, voiced: bool, dur: f64) -> Consonant {
    Consonant {
        class: ConsClass::Fricative,
        ff,
        bw,
        af,
        voiced,
        dur,
        ..BLANK
    }
}

const fn stop(voiced: bool, ff: f64, bw: f64, locus: Locus, closure: f64) -> Consonant {
    Consonant {
        class: ConsClass::Stop,
        voiced,
        ff,
        bw,
        locus,
        closure,
        ..BLANK
    }
}

const fn aff(
    voiced: bool,
    ff: f64,
    bw: f64,
    locus: Locus,
    closure: f64,
    fric_dur: f64,
    af: f64,
) -> Consonant {
    Consonant {
        class: ConsClass::Affricate,
        voiced,
        ff,
        bw,
        locus,
        closure,
        fric_dur,
        af,
        ..BLANK
    }
}

/// F1-F3 of each monophthong vowel; `None` for diphthongs and consonants.
pub const VOWELS: [Option<[f64; 3]>; Phoneme::COUNT] = {
    use Phoneme as P;
    let mut t = [None; Phoneme::COUNT];
    t[P::Iy as usize] = Some([270.0, 2290.0, 3010.0]);
    t[P::Ih as usize] = Some([390.0, 1990.0, 2550.0]);
    t[P::Eh as usize] = Some([530.0, 1840.0, 2480.0]);
    t[P::Ae as usize] = Some([660.0, 1720.0, 2410.0]);
    t[P::Aa as usize] = Some([730.0, 1090.0, 2440.0]);
    t[P::Ao as usize] = Some([570.0, 840.0, 2410.0]);
    t[P::Oh as usize] = Some([480.0, 860.0, 2410.0]);
    t[P::Uh as usize] = Some([440.0, 1020.0, 2240.0]);
    t[P::Uw as usize] = Some([310.0, 870.0, 2240.0]);
    t[P::Ah as usize] = Some([640.0, 1190.0, 2390.0]);
    t[P::Ax as usize] = Some([560.0, 1250.0, 2420.0]);
    t[P::Er as usize] = Some([490.0, 1350.0, 1690.0]);
    t[P::Ey0 as usize] = Some([450.0, 2020.0, 2600.0]);
    t[P::Ey1 as usize] = Some([340.0, 2210.0, 2780.0]);
    t
};

/// Consonant parameters; `None` for vowels.
pub const CONSONANTS: [Option<Consonant>; Phoneme::COUNT] = {
    use ConsClass::{Nasal, Sonorant};
    use Phoneme as P;
    let bilabial = Locus::At([250.0, 900.0, 2200.0]);
    let alveolar = Locus::At([250.0, 1750.0, 2700.0]);
    let postalveolar = Locus::At([250.0, 1900.0, 2600.0]);
    let mut t = [None; Phoneme::COUNT];
    t[P::L as usize] = Some(son(Sonorant, [360.0, 1050.0, 2700.0], 0.72, 0.055));
    t[P::R as usize] = Some(son(Sonorant, [420.0, 1250.0, 1650.0], 0.78, 0.06));
    t[P::W as usize] = Some(son(Sonorant, [300.0, 650.0, 2200.0], 0.65, 0.055));
    t[P::Y as usize] = Some(son(Sonorant, [270.0, 2100.0, 3000.0], 0.65, 0.05));
    t[P::M as usize] = Some(son(Nasal, [280.0, 1100.0, 2300.0], 0.55, 0.065));
    t[P::N as usize] = Some(son(Nasal, [280.0, 1650.0, 2600.0], 0.55, 0.06));
    t[P::Ng as usize] = Some(son(Nasal, [280.0, 2100.0, 2700.0], 0.5, 0.065));
    t[P::S as usize] = Some(fric(6500.0, 3500.0, 0.62, false, 0.095));
    t[P::Z as usize] = Some(fric(6000.0, 3500.0, 0.38, true, 0.075));
    t[P::Sh as usize] = Some(fric(3100.0, 1800.0, 0.62, false, 0.095));
    t[P::Zh as usize] = Some(fric(2900.0, 1800.0, 0.38, true, 0.07));
    t[P::F as usize] = Some(fric(5500.0, 7000.0, 0.24, false, 0.08));
    t[P::V as usize] = Some(fric(5000.0, 6000.0, 0.16, true, 0.06));
    t[P::Th as usize] = Some(fric(5500.0, 6000.0, 0.18, false, 0.075));
    // Voiced "th" is mostly voicing.
    t[P::Dh as usize] = Some(Consonant {
        vv: 0.6,
        ..fric(4500.0, 5000.0, 0.05, true, 0.045)
    });
    t[P::Hh as usize] = Some(Consonant {
        class: ConsClass::Aspirate,
        voiced: false,
        dur: 0.06,
        ..BLANK
    });
    t[P::P as usize] = Some(stop(false, 1100.0, 2500.0, bilabial, 0.05));
    t[P::B as usize] = Some(stop(true, 1100.0, 2500.0, bilabial, 0.04));
    t[P::T as usize] = Some(stop(false, 5200.0, 2600.0, alveolar, 0.045));
    t[P::D as usize] = Some(stop(true, 4200.0, 3000.0, alveolar, 0.05));
    t[P::Dx as usize] = Some(Consonant {
        flap: true,
        ..stop(
            true,
            3800.0,
            3000.0,
            Locus::At([280.0, 1700.0, 2650.0]),
            0.02,
        )
    });
    t[P::K as usize] = Some(stop(false, 2300.0, 1500.0, Locus::Velar, 0.05));
    t[P::G as usize] = Some(stop(true, 2300.0, 1500.0, Locus::Velar, 0.04));
    t[P::Ch as usize] = Some(aff(false, 3200.0, 2000.0, postalveolar, 0.04, 0.07, 0.55));
    t[P::Jh as usize] = Some(aff(true, 3000.0, 2000.0, postalveolar, 0.035, 0.055, 0.35));
    t
};

/// F1-F3 of a monophthong vowel.
#[inline]
pub const fn vowel_formants(p: Phoneme) -> Option<[f64; 3]> {
    VOWELS[p as usize]
}

/// Parameters of a consonant.
#[inline]
pub const fn consonant(p: Phoneme) -> Option<&'static Consonant> {
    CONSONANTS[p as usize].as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phoneme_has_one_entry() {
        for &p in Phoneme::ALL {
            let v = vowel_formants(p).is_some();
            let c = consonant(p).is_some();
            if p.is_diphthong() {
                assert!(!v && !c, "{p}");
                for t in p.diphthong_targets().unwrap_or([p, p]) {
                    assert!(vowel_formants(t).is_some(), "{p} target {t}");
                }
            } else {
                assert!(v != c, "{p}");
                assert_eq!(v, p.is_vowel(), "{p}");
            }
        }
    }

    #[test]
    fn ey_targets_are_hillenbrand() {
        assert_eq!(vowel_formants(Phoneme::Ey0), Some([450.0, 2020.0, 2600.0]));
        assert_eq!(vowel_formants(Phoneme::Ey1), Some([340.0, 2210.0, 2780.0]));
        assert_eq!(
            consonant(Phoneme::Dh).map(|c| (c.af, c.vv)),
            Some((0.05, 0.6))
        );
    }
}

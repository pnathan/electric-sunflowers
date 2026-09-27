//! Phonetics tables and grapheme-to-phoneme fallback. Ports the "phonetics"
//! section of src/engine.js. Consumed by the voice crate (VOWELS/DIPH/CONS).

use std::collections::HashMap;
use std::sync::OnceLock;

/// One VOWELS entry: F1, F2, F3 in Hz.
pub type Formants = [f64; 3];

/// VOWELS: symbol -> (F1,F2,F3). Order matches JS declaration (insertion order).
pub fn vowels() -> Vec<(&'static str, Formants)> {
    vec![
        ("iy", [270.0, 2290.0, 3010.0]),
        ("ih", [390.0, 1990.0, 2550.0]),
        ("eh", [530.0, 1840.0, 2480.0]),
        ("ae", [660.0, 1720.0, 2410.0]),
        ("aa", [730.0, 1090.0, 2440.0]),
        ("ao", [570.0, 840.0, 2410.0]),
        ("oh", [480.0, 860.0, 2410.0]),
        ("uh", [440.0, 1020.0, 2240.0]),
        ("uw", [310.0, 870.0, 2240.0]),
        ("ah", [640.0, 1190.0, 2390.0]),
        ("ax", [560.0, 1250.0, 2420.0]),
        ("er", [490.0, 1350.0, 1690.0]),
        ("ey0", [450.0, 2020.0, 2600.0]),
        ("ey1", [340.0, 2210.0, 2780.0]),
    ]
}

pub fn vowel_map() -> HashMap<&'static str, Formants> {
    vowels().into_iter().collect()
}

/// DIPH: diphthong -> pair of vowel symbols.
pub fn diph() -> Vec<(&'static str, [&'static str; 2])> {
    vec![
        ("ay", ["aa", "ih"]),
        ("aw", ["aa", "uh"]),
        ("ey", ["ey0", "ey1"]),
        ("ow", ["oh", "uh"]),
        ("oy", ["ao", "ih"]),
    ]
}

pub fn diph_map() -> HashMap<&'static str, [&'static str; 2]> {
    diph().into_iter().collect()
}

/// CONS entry. Fields are a superset union of all consonant types in JS;
/// unused fields are None/0 per-consonant, matching what the JS object
/// literal actually declares for that entry.
#[derive(Clone, Debug, Default)]
pub struct Cons {
    pub t: &'static str,
    pub f: Option<[f64; 3]>,
    pub av: Option<f64>,
    pub d: f64,
    pub ff: Option<f64>,
    pub bw: Option<f64>,
    pub af: Option<f64>,
    pub v: Option<i32>,
    pub vv: Option<f64>,
    pub loc: Option<Option<[f64; 3]>>, // Some(None) = null loc (k/g)
    pub cl: Option<f64>,
    pub flap: Option<i32>,
    pub fr: Option<f64>,
}

/// CONS table, in JS declaration order.
pub fn cons() -> Vec<(&'static str, Cons)> {
    vec![
        ("l", Cons{t:"son", f:Some([360.0,1050.0,2700.0]), av:Some(0.72), d:0.055, ..Default::default()}),
        ("r", Cons{t:"son", f:Some([420.0,1250.0,1650.0]), av:Some(0.78), d:0.06, ..Default::default()}),
        ("w", Cons{t:"son", f:Some([300.0,650.0,2200.0]), av:Some(0.65), d:0.055, ..Default::default()}),
        ("y", Cons{t:"son", f:Some([270.0,2100.0,3000.0]), av:Some(0.65), d:0.05, ..Default::default()}),
        ("m", Cons{t:"nas", f:Some([280.0,1100.0,2300.0]), av:Some(0.55), d:0.065, ..Default::default()}),
        ("n", Cons{t:"nas", f:Some([280.0,1650.0,2600.0]), av:Some(0.55), d:0.06, ..Default::default()}),
        ("ng", Cons{t:"nas", f:Some([280.0,2100.0,2700.0]), av:Some(0.5), d:0.065, ..Default::default()}),
        ("s", Cons{t:"fric", ff:Some(6500.0), bw:Some(3500.0), af:Some(0.62), v:Some(0), d:0.095, ..Default::default()}),
        ("z", Cons{t:"fric", ff:Some(6000.0), bw:Some(3500.0), af:Some(0.38), v:Some(1), d:0.075, ..Default::default()}),
        ("sh", Cons{t:"fric", ff:Some(3100.0), bw:Some(1800.0), af:Some(0.62), v:Some(0), d:0.095, ..Default::default()}),
        ("zh", Cons{t:"fric", ff:Some(2900.0), bw:Some(1800.0), af:Some(0.38), v:Some(1), d:0.07, ..Default::default()}),
        ("f", Cons{t:"fric", ff:Some(5500.0), bw:Some(7000.0), af:Some(0.24), v:Some(0), d:0.08, ..Default::default()}),
        ("v", Cons{t:"fric", ff:Some(5000.0), bw:Some(6000.0), af:Some(0.16), v:Some(1), d:0.06, ..Default::default()}),
        ("th", Cons{t:"fric", ff:Some(5500.0), bw:Some(6000.0), af:Some(0.18), v:Some(0), d:0.075, ..Default::default()}),
        ("dh", Cons{t:"fric", ff:Some(4500.0), bw:Some(5000.0), af:Some(0.05), v:Some(1), d:0.045, vv:Some(0.6), ..Default::default()}),
        ("hh", Cons{t:"asp", d:0.06, ..Default::default()}),
        ("p", Cons{t:"stop", v:Some(0), ff:Some(1100.0), bw:Some(2500.0), loc:Some(Some([250.0,900.0,2200.0])), cl:Some(0.05), d:0.0, ..Default::default()}),
        ("b", Cons{t:"stop", v:Some(1), ff:Some(1100.0), bw:Some(2500.0), loc:Some(Some([250.0,900.0,2200.0])), cl:Some(0.04), d:0.0, ..Default::default()}),
        ("t", Cons{t:"stop", v:Some(0), ff:Some(5200.0), bw:Some(2600.0), loc:Some(Some([250.0,1750.0,2700.0])), cl:Some(0.045), d:0.0, ..Default::default()}),
        ("d", Cons{t:"stop", v:Some(1), ff:Some(4200.0), bw:Some(3000.0), loc:Some(Some([250.0,1750.0,2700.0])), cl:Some(0.05), d:0.0, ..Default::default()}),
        ("dx", Cons{t:"stop", v:Some(1), ff:Some(3800.0), bw:Some(3000.0), loc:Some(Some([280.0,1700.0,2650.0])), cl:Some(0.02), flap:Some(1), d:0.0, ..Default::default()}),
        ("k", Cons{t:"stop", v:Some(0), ff:Some(2300.0), bw:Some(1500.0), loc:Some(None), cl:Some(0.05), d:0.0, ..Default::default()}),
        ("g", Cons{t:"stop", v:Some(1), ff:Some(2300.0), bw:Some(1500.0), loc:Some(None), cl:Some(0.04), d:0.0, ..Default::default()}),
        ("ch", Cons{t:"aff", v:Some(0), ff:Some(3200.0), bw:Some(2000.0), loc:Some(Some([250.0,1900.0,2600.0])), cl:Some(0.04), fr:Some(0.07), af:Some(0.55), d:0.0, ..Default::default()}),
        ("jh", Cons{t:"aff", v:Some(1), ff:Some(3000.0), bw:Some(2000.0), loc:Some(Some([250.0,1900.0,2600.0])), cl:Some(0.035), fr:Some(0.055), af:Some(0.35), d:0.0, ..Default::default()}),
    ]
}

pub fn cons_map() -> HashMap<&'static str, Cons> {
    cons().into_iter().collect()
}

/// Cached, process-lifetime `vowel_map()`. The tables are fixed data (no
/// per-call state), so callers on a hot path (voiceControls builds these
/// per consonant and per note in the JS port) can use this instead of
/// rebuilding the HashMap every call.
pub fn vowel_map_cached() -> &'static HashMap<&'static str, Formants> {
    static MAP: OnceLock<HashMap<&'static str, Formants>> = OnceLock::new();
    MAP.get_or_init(vowel_map)
}

/// Cached, process-lifetime `diph_map()`. See `vowel_map_cached`.
pub fn diph_map_cached() -> &'static HashMap<&'static str, [&'static str; 2]> {
    static MAP: OnceLock<HashMap<&'static str, [&'static str; 2]>> = OnceLock::new();
    MAP.get_or_init(diph_map)
}

/// Cached, process-lifetime `cons_map()`. See `vowel_map_cached`.
pub fn cons_map_cached() -> &'static HashMap<&'static str, Cons> {
    static MAP: OnceLock<HashMap<&'static str, Cons>> = OnceLock::new();
    MAP.get_or_init(cons_map)
}

/// PH_OK: every known phoneme symbol (vowels + diphthongs + consonants).
pub fn ph_ok() -> std::collections::HashSet<&'static str> {
    let mut s = std::collections::HashSet::new();
    for (k, _) in vowels() {
        s.insert(k);
    }
    for (k, _) in diph() {
        s.insert(k);
    }
    for (k, _) in cons() {
        s.insert(k);
    }
    s
}

/// isVowel(p)
pub fn is_vowel(p: &str) -> bool {
    vowel_map_cached().contains_key(p) || diph_map_cached().contains_key(p)
}

/// PH_ALIAS
pub fn ph_alias(p: &str) -> &str {
    match p {
        "ah0" => "ax",
        "hh" => "hh",
        "h" => "hh",
        "j" => "jh",
        "x" => "hh",
        "ax" => "ax",
        "ix" => "ih",
        "ux" => "uw",
        "el" => "l",
        "em" => "m",
        "en" => "n",
        "dx" => "d",
        "q" => "t",
        "axr" => "er",
        "oh" => "oh",
        other => other,
    }
}

/// G2P table: multi-char graphemes first (JS declaration order matters:
/// first match at each position wins).
fn g2p_table() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("tch", &["ch"]),
        ("igh", &["ay"]),
        ("ough", &["ao"]),
        ("augh", &["ao"]),
        ("eigh", &["ey"]),
        ("tion", &["sh", "ax", "n"]),
        ("sion", &["zh", "ax", "n"]),
        ("ck", &["k"]),
        ("ch", &["ch"]),
        ("sh", &["sh"]),
        ("th", &["th"]),
        ("ph", &["f"]),
        ("wh", &["w"]),
        ("ng", &["ng"]),
        ("qu", &["k", "w"]),
        ("wr", &["r"]),
        ("kn", &["n"]),
        ("gh", &[]),
        ("ee", &["iy"]),
        ("ea", &["iy"]),
        ("oo", &["uw"]),
        ("ou", &["aw"]),
        ("ow", &["ow"]),
        ("oa", &["ow"]),
        ("ai", &["ey"]),
        ("ay", &["ey"]),
        ("oi", &["oy"]),
        ("oy", &["oy"]),
        ("au", &["ao"]),
        ("aw", &["ao"]),
        ("ie", &["iy"]),
        ("ei", &["ey"]),
        ("ew", &["uw"]),
        ("ue", &["uw"]),
        ("er", &["er"]),
        ("ir", &["er"]),
        ("ur", &["er"]),
        ("ar", &["aa", "r"]),
        ("or", &["ao", "r"]),
        ("a", &["ae"]),
        ("e", &["eh"]),
        ("i", &["ih"]),
        ("o", &["aa"]),
        ("u", &["ah"]),
        ("b", &["b"]),
        ("c", &["k"]),
        ("d", &["d"]),
        ("f", &["f"]),
        ("g", &["g"]),
        ("h", &["hh"]),
        ("j", &["jh"]),
        ("k", &["k"]),
        ("l", &["l"]),
        ("m", &["m"]),
        ("n", &["n"]),
        ("p", &["p"]),
        ("r", &["r"]),
        ("s", &["s"]),
        ("t", &["t"]),
        ("v", &["v"]),
        ("w", &["w"]),
        ("x", &["k", "s"]),
        ("z", &["z"]),
    ]
}

/// g2p(syl): crude grapheme-to-phoneme fallback, per syllable.
pub fn g2p(syl: &str) -> Vec<String> {
    let mut s: String = syl
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase())
        .collect();
    // drop a silent trailing 'e': length>2, ends 'e', not "[aeiouy]e$", has a
    // vowel before the last char
    if s.len() > 2 && s.ends_with('e') {
        let bytes: Vec<char> = s.chars().collect();
        let n = bytes.len();
        let second_last = bytes[n - 2];
        let is_vowel_letter = |c: char| "aeiouy".contains(c);
        let not_vowel_e = !is_vowel_letter(second_last);
        let has_vowel_before = bytes[..n - 1].iter().any(|c| "aeiouy".contains(*c));
        if not_vowel_e && has_vowel_before {
            s = bytes[..n - 1].iter().collect();
        }
    }
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out: Vec<String> = Vec::new();
    let table = g2p_table();
    let mut i = 0usize;
    while i < n {
        if chars[i] == 'y' {
            out.push(if i == 0 { "y".to_string() } else { "iy".to_string() });
            i += 1;
            continue;
        }
        if chars[i] == 'c' {
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if "eiy".contains(next) {
                out.push("s".to_string());
                i += 1;
                continue;
            }
        }
        let mut hit = false;
        for (g, p) in &table {
            let gl = g.chars().count();
            if i + gl <= n && chars[i..i + gl].iter().collect::<String>() == *g {
                for ph in *p {
                    out.push(ph.to_string());
                }
                i += gl;
                hit = true;
                break;
            }
        }
        if !hit {
            i += 1;
        }
    }
    if !out.iter().any(|p| is_vowel(p)) {
        out.push("ax".to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g2p_word_list() {
        // (word, expects_a_vowel, first_phoneme_optional_check)
        let words = [
            "love", "tion", "night", "though", "eight", "cat", "the", "sing",
            "church", "quick", "know", "gnome",
        ];
        for w in words {
            let ph = g2p(w);
            assert!(!ph.is_empty(), "empty phoneme list for {w}");
            assert!(ph.iter().any(|p| is_vowel(p)), "no vowel for {w}: {ph:?}");
        }
        // spot checks
        assert_eq!(g2p("cat"), vec!["k", "ae", "t"]);
        assert_eq!(g2p("sing"), vec!["s", "ih", "ng"]);
    }

    #[test]
    fn ph_ok_contains_all() {
        let ok = ph_ok();
        assert!(ok.contains("iy"));
        assert!(ok.contains("ay"));
        assert!(ok.contains("t"));
    }
}

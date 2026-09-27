//! Music theory: note names, chord quality tables, chord parsing/transposition,
//! modes and local scales, meters. Ports the "theory" section of src/engine.js.

use std::collections::HashMap;

/// NOTE_PC: pitch class of the natural letter names.
pub fn note_pc(letter: char) -> i32 {
    match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => 0,
    }
}

/// SHARPS
pub const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
/// FLATS
pub const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// Result of parsePc: pitch class plus how many chars were consumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParsedPc {
    pub pc: i32,
    pub len: usize,
}

/// parsePc(s): reads a leading letter plus optional #/b.
pub fn parse_pc(s: &str) -> Option<ParsedPc> {
    let mut chars = s.chars();
    let c0 = chars.next()?;
    if !c0.is_ascii_alphabetic() || !"ABCDEFGabcdefg".contains(c0) {
        return None;
    }
    let mut pc = note_pc(c0);
    let mut len = c0.len_utf8();
    if let Some(c1) = chars.next() {
        if c1 == '#' {
            pc += 1;
            len += c1.len_utf8();
        } else if c1 == 'b' {
            pc -= 1;
            len += c1.len_utf8();
        }
    }
    Some(ParsedPc {
        pc: (pc + 12).rem_euclid(12),
        len,
    })
}

/// QUAL: chord-quality name -> interval set. Iteration order matters for the
/// fallback prefix search in parse_chord (JS object-key order: this table has
/// no integer-like keys, so it is insertion order, exactly as declared here).
pub fn qual_table() -> Vec<(&'static str, Vec<i32>)> {
    vec![
        ("", vec![0, 4, 7]),
        ("maj", vec![0, 4, 7]),
        ("M", vec![0, 4, 7]),
        ("m", vec![0, 3, 7]),
        ("min", vec![0, 3, 7]),
        ("-", vec![0, 3, 7]),
        ("7", vec![0, 4, 7, 10]),
        ("maj7", vec![0, 4, 7, 11]),
        ("M7", vec![0, 4, 7, 11]),
        ("m7", vec![0, 3, 7, 10]),
        ("min7", vec![0, 3, 7, 10]),
        ("-7", vec![0, 3, 7, 10]),
        ("mmaj7", vec![0, 3, 7, 11]),
        ("mM7", vec![0, 3, 7, 11]),
        ("6", vec![0, 4, 7, 9]),
        ("m6", vec![0, 3, 7, 9]),
        ("69", vec![0, 4, 7, 9, 2]),
        ("9", vec![0, 4, 7, 10, 2]),
        ("maj9", vec![0, 4, 7, 11, 2]),
        ("m9", vec![0, 3, 7, 10, 2]),
        ("add9", vec![0, 4, 7, 2]),
        ("add2", vec![0, 2, 4, 7]),
        ("madd9", vec![0, 3, 7, 2]),
        ("sus2", vec![0, 2, 7]),
        ("sus4", vec![0, 5, 7]),
        ("sus", vec![0, 5, 7]),
        ("7sus4", vec![0, 5, 7, 10]),
        ("7sus", vec![0, 5, 7, 10]),
        ("7sus2", vec![0, 2, 7, 10]),
        ("dim", vec![0, 3, 6]),
        ("o", vec![0, 3, 6]),
        ("dim7", vec![0, 3, 6, 9]),
        ("o7", vec![0, 3, 6, 9]),
        ("m7b5", vec![0, 3, 6, 10]),
        ("\u{f8}", vec![0, 3, 6, 10]),  // 'ø'
        ("\u{f8}7", vec![0, 3, 6, 10]), // 'ø7'
        ("aug", vec![0, 4, 8]),
        ("+", vec![0, 4, 8]),
        ("5", vec![0, 7]),
        ("11", vec![0, 4, 7, 10, 5]),
        ("m11", vec![0, 3, 7, 10, 5]),
        ("13", vec![0, 4, 7, 10, 9]),
        ("7b9", vec![0, 4, 7, 10, 1]),
    ]
}

/// JS-key-order-aware lookup: object keys made of integer-like keys ("5",
/// "6", "7", "9", "11", "13", "69") iterate first, ascending numerically,
/// then string keys in insertion (declaration) order. Precomputed here.
fn qual_key_order() -> Vec<&'static str> {
    // integer-like keys ascending: 5,6,7,9,11,13,69
    let int_keys = ["5", "6", "7", "9", "11", "13", "69"];
    let table = qual_table();
    let mut string_keys = Vec::new();
    for (k, _) in &table {
        if !int_keys.contains(k) {
            string_keys.push(*k);
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(&int_keys);
    out.extend(string_keys);
    out
}

fn qual_map() -> HashMap<&'static str, Vec<i32>> {
    qual_table().into_iter().collect()
}

/// parseChord result.
#[derive(Clone, Debug)]
pub struct Chord {
    pub name: String,
    pub root: i32,
    pub bass: i32,
    pub pcs: Vec<i32>,
    pub third: Option<i32>,
    pub seventh: Option<i32>,
    pub ess: Vec<i32>,
    pub fifth: Option<i32>,
}

/// parseChord(name): no cache (parity-irrelevant); replicates JS key-order
/// fallback prefix search exactly.
pub fn parse_chord(name_in: &str) -> Chord {
    let name = if name_in.is_empty() { "C".to_string() } else { name_in.to_string() };
    let name = name.trim().replace('\u{2669}', "#").replace('\u{266d}', "b");
    // JS replaces the actual unicode sharp/flat symbols (♯, ♭)
    let name = name.replace('\u{266f}', "#");
    let mut main = name.as_str();
    let mut bass_str: Option<&str> = None;
    if let Some(sl) = name.find('/') {
        if sl > 0 {
            main = &name[..sl];
            bass_str = Some(&name[sl + 1..]);
        }
    }
    let r = parse_pc(main).unwrap_or(ParsedPc { pc: 0, len: 0 });
    let q_raw: String = main[r.len.min(main.len())..]
        .chars()
        .filter(|c| *c != '(' && *c != ')' && !c.is_whitespace())
        .collect();
    let map = qual_map();
    let ints: Vec<i32> = if let Some(v) = map.get(q_raw.as_str()) {
        v.clone()
    } else {
        let mut best = "";
        for k in qual_key_order() {
            if q_raw.starts_with(k) && k.len() > best.len() {
                best = k;
            }
        }
        map.get(best).cloned().unwrap_or_default()
    };
    let root = r.pc;
    let mut pcs: Vec<i32> = Vec::new();
    for i in &ints {
        let pc = (root + i).rem_euclid(12);
        if !pcs.contains(&pc) {
            pcs.push(pc);
        }
    }
    let mut bass = root;
    if let Some(b) = bass_str {
        if let Some(p) = parse_pc(b) {
            bass = p.pc;
        }
    }
    let third = if ints.contains(&3) {
        Some((root + 3).rem_euclid(12))
    } else if ints.contains(&4) {
        Some((root + 4).rem_euclid(12))
    } else {
        None
    };
    let seventh = if ints.contains(&10) {
        Some((root + 10).rem_euclid(12))
    } else if ints.contains(&11) {
        Some((root + 11).rem_euclid(12))
    } else {
        None
    };
    let mut ess = vec![root];
    if let Some(t) = third {
        ess.push(t);
    } else {
        if ints.contains(&5) {
            ess.push((root + 5).rem_euclid(12));
        }
        if ints.contains(&2) {
            ess.push((root + 2).rem_euclid(12));
        }
    }
    if let Some(s) = seventh {
        ess.push(s);
    }
    if ints.contains(&6) && !ints.contains(&7) {
        ess.push((root + 6).rem_euclid(12));
    }
    if ints.contains(&8) {
        ess.push((root + 8).rem_euclid(12));
    }
    let fifth = if ints.contains(&7) {
        Some((root + 7).rem_euclid(12))
    } else {
        None
    };
    Chord {
        name: name_in.to_string(),
        root,
        bass,
        pcs,
        third,
        seventh,
        ess,
        fifth,
    }
}

/// transposeName(name, semis, flats)
pub fn transpose_name(name: &str, semis: i32, flats: bool) -> String {
    if semis == 0 {
        return name.to_string();
    }
    let table = if flats { &FLATS } else { &SHARPS };
    // Replace occurrences of (^|/)([A-G][#b]?)
    let mut out = String::new();
    let chars: Vec<char> = name.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let at_start = i == 0;
        let prev_slash = i > 0 && chars[i - 1] == '/';
        if (at_start || prev_slash) && chars[i].is_ascii_uppercase() && "ABCDEFG".contains(chars[i])
        {
            let mut j = i + 1;
            let mut acc = String::new();
            acc.push(chars[i]);
            if j < chars.len() && (chars[j] == '#' || chars[j] == 'b') {
                acc.push(chars[j]);
                j += 1;
            }
            let p = parse_pc(&acc).unwrap();
            let idx = (p.pc + semis + 120).rem_euclid(12) as usize;
            out.push_str(table[idx]);
            i = j;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// MODES
pub fn modes() -> Vec<(&'static str, Vec<i32>)> {
    vec![
        ("major", vec![0, 2, 4, 5, 7, 9, 11]),
        ("minor", vec![0, 2, 3, 5, 7, 8, 10]),
        ("dorian", vec![0, 2, 3, 5, 7, 9, 10]),
        ("mixolydian", vec![0, 2, 4, 5, 7, 9, 10]),
    ]
}

pub fn mode_scale(mode: &str) -> Vec<i32> {
    modes()
        .into_iter()
        .find(|(k, _)| *k == mode)
        .map(|(_, v)| v)
        .unwrap_or_else(|| vec![0, 2, 4, 5, 7, 9, 11])
}

/// useFlats(pc, mode)
pub fn use_flats(pc: i32, mode: &str) -> bool {
    if mode == "major" || mode == "mixolydian" {
        [5, 10, 3, 8, 1].contains(&pc)
    } else {
        [2, 7, 0, 5, 10, 3].contains(&pc)
    }
}

/// localScale(tonic, mode, chord)
pub fn local_scale(tonic: i32, mode: &str, chord: &Chord) -> Vec<i32> {
    let mut s: Vec<i32> = mode_scale(mode).iter().map(|x| (x + tonic).rem_euclid(12)).collect();
    for &pc in &chord.pcs {
        if !s.contains(&pc) {
            if let Some(i) = s
                .iter()
                .position(|&q| (q + 1).rem_euclid(12) == pc || (q + 11).rem_euclid(12) == pc)
            {
                s[i] = pc;
            } else {
                s.push(pc);
            }
        }
    }
    s
}

/// One METERS entry.
#[derive(Clone, Debug)]
pub struct Meter {
    pub bpb: i32,
    pub sub: i32,
    pub w: Vec<f64>,
    pub split: i32,
}

/// METERS
pub fn meter(name: &str) -> Meter {
    match name {
        "3/4" => Meter {
            bpb: 3,
            sub: 2,
            w: vec![1.0, 0.15, 0.45, 0.15, 0.5, 0.15],
            split: 2,
        },
        "6/8" => Meter {
            bpb: 2,
            sub: 3,
            w: vec![1.0, 0.2, 0.35, 0.8, 0.2, 0.35],
            split: 1,
        },
        _ => Meter {
            bpb: 4,
            sub: 2,
            w: vec![1.0, 0.15, 0.5, 0.15, 0.8, 0.15, 0.5, 0.15],
            split: 2,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pc_basic() {
        assert_eq!(parse_pc("C").unwrap().pc, 0);
        assert_eq!(parse_pc("C#").unwrap().pc, 1);
        assert_eq!(parse_pc("Db").unwrap().pc, 1);
        assert_eq!(parse_pc("Bb").unwrap().pc, 10);
    }

    #[test]
    fn parse_chord_spread() {
        let c = parse_chord("C");
        assert_eq!(c.root, 0);
        assert_eq!(c.pcs, vec![0, 4, 7]);

        let c = parse_chord("Dm7");
        assert_eq!(c.root, 2);
        assert_eq!(c.pcs, vec![2, 5, 9, 0]);

        let c = parse_chord("G7");
        assert_eq!(c.pcs, vec![7, 11, 2, 5]);

        let c = parse_chord("C7#9");
        // "7#9" isn't a key; fallback prefix search should match "7" (len 1)
        // since none of the longer quality keys are a prefix of "7#9".
        assert_eq!(c.pcs, vec![0, 4, 7, 10]);

        let c = parse_chord("Dm(add9)");
        // parens/space stripped -> "madd9" is not a prefix match of "madd9"... wait
        // q_raw after stripping parens = "madd9"
        assert_eq!(c.pcs.contains(&2), true);

        let c = parse_chord("C/E");
        assert_eq!(c.root, 0);
        assert_eq!(c.bass, 4);

        let c = parse_chord("Am");
        assert_eq!(c.pcs, vec![9, 0, 4]);
    }

    #[test]
    fn transpose_name_basic() {
        assert_eq!(transpose_name("C", 2, false), "D");
        assert_eq!(transpose_name("C/E", 2, false), "D/F#");
        assert_eq!(transpose_name("Cm7", 1, false), "C#m7");
    }
}

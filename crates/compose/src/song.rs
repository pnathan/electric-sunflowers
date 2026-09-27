//! Song input normalization. Ports parseLine and normalizeSong from
//! src/engine.js. The model's JSON is loose, so input is a serde_json::Value.

use crate::phonetics::{is_vowel, ph_alias, ph_ok, g2p};
use crate::theory::{meter, parse_pc, Meter};
use sfcore::js::clamp;
use std::collections::HashSet;

pub const SEC_TYPES: [&str; 7] = [
    "intro", "verse", "prechorus", "chorus", "bridge", "interlude", "outro",
];

fn func_words() -> HashSet<&'static str> {
    "a an the and but or of to in on at by for with from as is was be are am i my me you your he she it its we our they their them his her that this than then so if nor oh o yet"
        .split(' ')
        .collect()
}

#[derive(Clone, Debug)]
pub struct Syllable {
    pub text: String,
    pub stress: bool,
    pub word_idx: usize,
    pub first: bool,
    pub last: bool,
    pub word: String,
    pub ph: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Line {
    pub syls: Vec<Syllable>,
    /// bars: each bar is up to 2 chord-name tokens
    pub bars: Vec<Vec<String>>,
    pub text: String,
}

fn jval_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// parseLine(ln)
pub fn parse_line(ln: &serde_json::Value) -> Option<Line> {
    let syl_str = {
        let s = ln.get("syl").map(jval_str).filter(|s| !s.is_empty());
        let l = ln.get("lyric").map(jval_str).filter(|s| !s.is_empty());
        let t = ln.get("text").map(jval_str).filter(|s| !s.is_empty());
        s.or(l).or(t).unwrap_or_default()
    };
    let syl_str = syl_str.trim().to_string();
    let words: Vec<&str> = syl_str.split_whitespace().collect();
    let mut syls: Vec<Syllable> = Vec::new();
    for (wi, w) in words.iter().enumerate() {
        let parts: Vec<&str> = w.split('-').filter(|p| !p.is_empty()).collect();
        let np = parts.len();
        for (pi, p) in parts.iter().enumerate() {
            let stress = p.contains('*');
            let text: String = p.chars().filter(|c| *c != '*').collect();
            let word: String = w.chars().filter(|c| *c != '*' && *c != '-').collect();
            syls.push(Syllable {
                text,
                stress,
                word_idx: wi,
                first: pi == 0,
                last: pi == np - 1,
                word,
                ph: Vec::new(),
            });
        }
    }
    if syls.is_empty() {
        return None;
    }
    if !syls.iter().any(|s| s.stress) {
        let mut by_word: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        for (i, s) in syls.iter().enumerate() {
            by_word.entry(s.word_idx).or_default().push(i);
        }
        let fw = func_words();
        for (_k, idxs) in by_word {
            if idxs.len() > 1 {
                syls[idxs[0]].stress = true;
            } else {
                let bare: String = syls[idxs[0]]
                    .word
                    .to_lowercase()
                    .chars()
                    .filter(|c| c.is_ascii_lowercase() || *c == '\'')
                    .collect();
                if !fw.contains(bare.as_str()) {
                    syls[idxs[0]].stress = true;
                }
            }
        }
    }
    let ph_field = ln.get("ph").map(jval_str).unwrap_or_default();
    let groups: Vec<Vec<String>> = ph_field
        .split('|')
        .map(|g| {
            let ok = ph_ok();
            g.trim()
                .to_lowercase()
                .split_whitespace()
                .map(|p| p.chars().filter(|c| !c.is_ascii_digit()).collect::<String>())
                .map(|p| ph_alias(&p).to_string())
                .filter(|p| ok.contains(p.as_str()))
                .collect()
        })
        .collect();
    let ok_count = groups.len() == syls.len();
    for (i, s) in syls.iter_mut().enumerate() {
        let mut ph = if ok_count { groups.get(i).cloned() } else { None };
        if ph.as_ref().map_or(true, |p| p.is_empty() || !p.iter().any(|x| is_vowel(x))) {
            ph = Some(g2p(&s.text));
        }
        s.ph = ph.unwrap();
    }
    let chords_val = ln.get("chords");
    let mut chords: Vec<String> = match chords_val {
        Some(serde_json::Value::Array(a)) => a.iter().map(jval_str).collect(),
        Some(v) => jval_str(v)
            .split(|c| c == ',' || c == '|')
            .map(|s| s.to_string())
            .collect(),
        None => vec!["C".to_string()],
    };
    chords = chords.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
    if chords.is_empty() {
        chords = vec!["C".to_string()];
    }
    if chords.len() > 4 {
        chords.truncate(4);
    }
    let bars: Vec<Vec<String>> = chords
        .iter()
        .map(|c| {
            c.split_whitespace()
                .take(2)
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    let text = syls.iter().map(|s| s.text.clone()).collect::<Vec<_>>().join(" ");
    Some(Line { syls, bars, text })
}

#[derive(Clone, Debug)]
pub struct Band {
    pub drums: String,
    pub bass: bool,
    pub harmony_guitar: bool,
    pub harp: bool,
    pub violin: bool,
    pub choir: bool,
    pub harmonies: bool,
    pub doubles: bool,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub type_: String,
    pub lines: Option<Vec<Line>>,
    /// used when lines is None (an instrumental section)
    pub bars: Vec<Vec<String>>,
    pub repeat: bool,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub title: String,
    pub note: String,
    pub key_pc: i32,
    pub mode: String,
    pub meter_name: String,
    pub tempo: f64,
    pub guitar: String,
    pub voice: String,
    pub band: Band,
    pub sections: Vec<Section>,
}

impl Song {
    pub fn meter(&self) -> Meter {
        meter(&self.meter_name)
    }
}

fn jstr(v: &serde_json::Value, key: &str, default: &str) -> String {
    v.get(key).map(jval_str).filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
}

/// normalizeSong(raw): raw is loose JSON from the model.
pub fn normalize_song(raw: &serde_json::Value) -> Result<Song, String> {
    if !raw.is_object() {
        return Err("bad_song".to_string());
    }
    let mut title = jstr(raw, "title", "Untitled");
    title.truncate(120);
    let mut note = jstr(raw, "note", "");
    note.truncate(400);
    let key_str = jstr(raw, "key", "C");
    let kp = parse_pc(&key_str);
    let key_pc = kp.map(|p| p.pc).unwrap_or(0);
    let mut mode = raw.get("mode").map(jval_str).unwrap_or_default().to_lowercase();
    let mode_valid = matches!(mode.as_str(), "major" | "minor" | "dorian" | "mixolydian");
    if !mode_valid {
        let rest = &key_str[kp.map(|p| p.len).unwrap_or(0).min(key_str.len())..];
        let re_minor = rest.to_lowercase();
        let is_min = (re_minor.contains('m') && !re_minor.contains("maj")) || re_minor.contains("minor");
        mode = if is_min { "minor".to_string() } else { "major".to_string() };
    }
    let meter_raw = raw.get("meter").map(jval_str).unwrap_or_default();
    let meter_name = if matches!(meter_raw.as_str(), "4/4" | "3/4" | "6/8") {
        meter_raw
    } else {
        "4/4".to_string()
    };
    let tempo_raw = raw.get("tempo").and_then(|v| v.as_f64()).unwrap_or(88.0);
    let tempo_round = sfcore::js::round(if tempo_raw.is_finite() { tempo_raw } else { 88.0 });
    let (lo, hi) = if meter_name == "6/8" { (36.0, 84.0) } else { (52.0, 150.0) };
    let tempo = clamp(tempo_round, lo, hi);
    let guitar_raw = raw.get("guitar").map(jval_str).unwrap_or_default();
    let guitar = if matches!(guitar_raw.as_str(), "strum" | "fingerpick" | "travis" | "arpeggio") {
        guitar_raw
    } else {
        "fingerpick".to_string()
    };
    let voice_raw = raw.get("voice").map(jval_str).unwrap_or_default();
    let voice = if matches!(voice_raw.as_str(), "baritone" | "tenor" | "alto" | "soprano") {
        voice_raw
    } else {
        "baritone".to_string()
    };
    let bv = raw.get("band").cloned().unwrap_or(serde_json::Value::Null);
    let bget = |k: &str| bv.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let bbool = |k: &str, default: bool| -> bool {
        match bget(k) {
            serde_json::Value::Bool(b) => b,
            serde_json::Value::Null => default,
            _ => default,
        }
    };
    let drums_raw = bget("drums").as_str().unwrap_or("").to_string();
    let drums = if matches!(drums_raw.as_str(), "none" | "brushes" | "soft" | "full") {
        drums_raw
    } else {
        "brushes".to_string()
    };
    let band = Band {
        drums,
        bass: bbool("bass", true),
        harmony_guitar: bbool("harmonyGuitar", true),
        harp: bbool("harp", false),
        violin: bbool("violin", true),
        choir: bbool("choir", true),
        harmonies: bbool("harmonies", true),
        doubles: bbool("doubles", true),
    };

    let mut secs: Vec<Section> = Vec::new();
    let mut last_by_type: std::collections::HashMap<String, usize> = Default::default();
    let empty = vec![];
    let raw_secs = raw.get("sections").and_then(|v| v.as_array()).unwrap_or(&empty);
    for s in raw_secs {
        let mut type_: String = s
            .get("type")
            .map(jval_str)
            .unwrap_or_else(|| "verse".to_string())
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_lowercase())
            .collect();
        if type_ == "prechorus" || type_ == "pre" {
            type_ = "prechorus".to_string();
        }
        if !SEC_TYPES.contains(&type_.as_str()) {
            type_ = if type_.starts_with("chor") || type_.starts_with("refrain") {
                "chorus".to_string()
            } else {
                "verse".to_string()
            };
        }
        let same = s.get("same").and_then(|v| v.as_bool()).unwrap_or(false);
        if same {
            if let Some(&idx) = last_by_type.get(&type_) {
                let (lines, bars) = (secs[idx].lines.clone(), secs[idx].bars.clone());
                secs.push(Section {
                    type_: type_.clone(),
                    lines,
                    bars,
                    repeat: true,
                });
                continue;
            }
        }
        let raw_lines = s.get("lines").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let lines: Vec<Line> = raw_lines.iter().filter_map(parse_line).collect();
        if !lines.is_empty() {
            secs.push(Section {
                type_: type_.clone(),
                lines: Some(lines),
                bars: vec![],
                repeat: false,
            });
            last_by_type.insert(type_.clone(), secs.len() - 1);
        } else {
            let ch_val = s.get("chords");
            let mut ch: Vec<String> = match ch_val {
                Some(serde_json::Value::Array(a)) => a.iter().map(jval_str).collect(),
                Some(v) => jval_str(v).split(|c| c == ',' || c == '|').map(|x| x.to_string()).collect(),
                None => vec![],
            };
            ch = ch.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).take(8).collect();
            if ch.is_empty() {
                continue;
            }
            let bars: Vec<Vec<String>> = ch
                .iter()
                .map(|c| c.split_whitespace().take(2).map(|s| s.to_string()).collect())
                .collect();
            let out_type = if type_ == "verse" || type_ == "chorus" {
                "interlude".to_string()
            } else {
                type_.clone()
            };
            secs.push(Section {
                type_: out_type,
                lines: None,
                bars,
                repeat: false,
            });
        }
    }
    if !secs.iter().any(|s| s.lines.is_some()) {
        return Err("no_lyrics".to_string());
    }
    Ok(Song {
        title,
        note,
        key_pc,
        mode,
        meter_name,
        tempo,
        guitar,
        voice,
        band,
        sections: secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_minimal() {
        let raw = json!({
            "sections":[{"type":"verse","lines":[{"syl":"hel-*lo world","ph":"hh eh l | ow","chords":"C G"}]}]
        });
        let s = normalize_song(&raw).unwrap();
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.mode, "major");
        assert_eq!(s.tempo, 88.0);
    }

    #[test]
    fn normalize_rejects_no_lyrics() {
        let raw = json!({"sections":[{"type":"intro","chords":"C G"}]});
        assert!(normalize_song(&raw).is_err());
    }

    #[test]
    fn normalize_minor_key_guess() {
        let raw = json!({
            "key":"Am",
            "sections":[{"type":"verse","lines":[{"syl":"one two three","chords":"C"}]}]
        });
        let s = normalize_song(&raw).unwrap();
        assert_eq!(s.mode, "minor");
    }
}

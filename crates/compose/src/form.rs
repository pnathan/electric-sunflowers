//! Form: lays sections/lines/bars onto chord slots and computes each
//! section's `lift`/`intensity`/`final`. Ports `buildForm` (engine.js).
//!
//! JS parity: the JS form links objects by reference (bar.sec, line.sec,
//! sec.next). Here every cross-reference is an index into the owning Vec.

use crate::song::Song;
use crate::theory::{meter, parse_chord, transpose_name, use_flats, Chord, Meter};

/// A metric bar: its chord(s) plus which section/line index it belongs to.
#[derive(Clone, Debug)]
pub struct Bar {
    pub chords: Vec<Chord>,
    pub sec: usize,
    pub line: Option<usize>,
}

/// A lyric line placed in the form (`L` in JS): syllables plus its bar span.
#[derive(Clone, Debug)]
pub struct FormLine {
    pub sec: usize,
    pub li: usize,
    pub start_bar: usize,
    pub n_bars: usize,
    pub syls: Vec<crate::song::Syllable>,
    pub text: String,
    /// filled in by `composeMelody` (`L.pitches`); `None` until composed.
    pub pitches: Option<Vec<i32>>,
    /// filled in by `composeMelody` (`L.rh`); `None` until composed.
    pub rh: Option<crate::rhythm::RhythmResult>,
}

/// A section placed in the form.
#[derive(Clone, Debug)]
pub struct Sec {
    pub type_: String,
    pub occ: usize,
    pub idx: usize,
    pub start_bar: usize,
    pub n_bars: usize,
    /// indices into Form.lines
    pub lines: Vec<usize>,
    pub lift: bool,
    pub lift_idx: i32,
    pub final_: bool,
    pub intensity: i32,
    pub next: Option<usize>,
}

/// buildForm's return value.
#[derive(Clone, Debug)]
pub struct Form {
    pub mi: Meter,
    pub bars: Vec<Bar>,
    pub lines: Vec<FormLine>,
    pub sections: Vec<Sec>,
    pub transpose: i32,
    pub flats: bool,
    pub stretch: i32,
}

/// expand(b): JS parity: stretch==1 -> [b]; stretch==2 and b.len==2 ->
/// [[b[0]],[b[1]]]; stretch==2 and b.len==1 -> [b,b].
fn expand(b: &[String], stretch: i32) -> Vec<Vec<String>> {
    if stretch == 1 {
        return vec![b.to_vec()];
    }
    if b.len() == 2 {
        vec![vec![b[0].clone()], vec![b[1].clone()]]
    } else {
        vec![b.to_vec(), b.to_vec()]
    }
}

/// buildForm(song, transpose)
pub fn build_form(song: &Song, transpose: i32) -> Form {
    let mi = meter(&song.meter_name);
    let mut bars: Vec<Bar> = Vec::new();
    let mut lines: Vec<FormLine> = Vec::new();
    let mut sections: Vec<Sec> = Vec::new();
    let mut occ_map: std::collections::HashMap<String, usize> = Default::default();
    let flats = use_flats((song.key_pc + transpose + 12).rem_euclid(12), &song.mode);
    let mk = |arr: &[String]| -> Vec<Chord> {
        arr.iter()
            .map(|n| parse_chord(&transpose_name(n, transpose, flats)))
            .collect()
    };

    // hypermetric stretch
    let mut ns = 0.0f64;
    let mut nl = 0i32;
    for s in &song.sections {
        if let Some(ls) = &s.lines {
            for ln in ls {
                ns += ln.syls.len() as f64 / ln.bars.len() as f64;
                nl += 1;
            }
        }
    }
    let per_bar = if nl != 0 { ns / nl as f64 } else { 4.0 };
    let slots = (mi.bpb * mi.sub) as f64;
    let bar_dur = mi.bpb as f64 * 60.0 / song.tempo;
    let stretch = if slots / per_bar < 1.75 && bar_dur * 4.0 <= 8.4 { 2 } else { 1 };

    for (si, s) in song.sections.iter().enumerate() {
        let o = *occ_map.get(&s.type_).unwrap_or(&0);
        occ_map.insert(s.type_.clone(), o + 1);
        let sec_idx = sections.len();
        let sec_start_bar = bars.len();
        let mut sec_lines: Vec<usize> = Vec::new();
        if let Some(ls) = &s.lines {
            for (li, ln) in ls.iter().enumerate() {
                let start_bar = bars.len();
                for b in &ln.bars {
                    for x in expand(b, stretch) {
                        bars.push(Bar {
                            chords: mk(&x),
                            sec: sec_idx,
                            line: Some(lines.len()),
                        });
                    }
                }
                let n_bars = bars.len() - start_bar;
                let fl = FormLine {
                    sec: sec_idx,
                    li,
                    start_bar,
                    n_bars,
                    syls: ln.syls.clone(),
                    text: ln.text.clone(),
                    pitches: None,
                    rh: None,
                };
                sec_lines.push(lines.len());
                lines.push(fl);
            }
        } else {
            for b in &s.bars {
                for x in expand(b, stretch) {
                    bars.push(Bar {
                        chords: mk(&x),
                        sec: sec_idx,
                        line: None,
                    });
                }
            }
        }
        let n_bars = bars.len() - sec_start_bar;
        sections.push(Sec {
            type_: s.type_.clone(),
            occ: o,
            idx: si,
            start_bar: sec_start_bar,
            n_bars,
            lines: sec_lines,
            lift: false,
            lift_idx: -1,
            final_: false,
            intensity: 0,
            next: None,
        });
    }

    let has_chorus = sections
        .iter()
        .any(|s| s.type_ == "chorus" && !s.lines.is_empty());
    let mut li_counter = 0i32;
    for s in sections.iter_mut() {
        s.lift = if has_chorus {
            s.type_ == "chorus"
        } else {
            s.type_ == "verse" && s.occ > 0
        };
        s.lift_idx = if s.lift {
            let v = li_counter;
            li_counter += 1;
            v
        } else {
            -1
        };
    }
    let n_lift = li_counter;
    let last_lift_idx: Option<usize> = sections
        .iter()
        .enumerate()
        .rev()
        .find(|(_, s)| s.lift)
        .map(|(i, _)| i)
        .or_else(|| {
            sections
                .iter()
                .enumerate()
                .rev()
                .find(|(_, s)| !s.lines.is_empty())
                .map(|(i, _)| i)
        });

    for (i, s) in sections.iter_mut().enumerate() {
        s.final_ = Some(i) == last_lift_idx;
        s.intensity = match s.type_.as_str() {
            "intro" => 0,
            "outro" => {
                if !s.lines.is_empty() {
                    1
                } else {
                    0
                }
            }
            "bridge" => 1,
            "interlude" => 1,
            "prechorus" => 2,
            _ => 0,
        };
        if s.type_ == "verse" {
            s.intensity = if s.occ == 0 { 1 } else { 2 };
        }
        if s.lift {
            s.intensity = if s.final_ && s.lift_idx > 0 {
                3
            } else if s.lift_idx == 0 {
                2
            } else if has_chorus {
                3
            } else {
                2 + if s.lift_idx >= n_lift - 2 { 1 } else { 0 }
            };
        }
    }
    let n = sections.len();
    for i in 0..n {
        sections[i].next = if i + 1 < n { Some(i + 1) } else { None };
    }

    Form {
        mi,
        bars,
        lines,
        sections,
        transpose,
        flats,
        stretch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_form_basic() {
        let raw = json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[
                {"type":"intro","chords":["C","G"]},
                {"type":"verse","lines":[{"syl":"one *two three *four","chords":"C G"}]},
                {"type":"chorus","lines":[{"syl":"*five *six *seven *eight","chords":"Am F"}]},
                {"type":"chorus","same":true}
            ]
        });
        let song = crate::song::normalize_song(&raw).unwrap();
        let form = build_form(&song, 0);
        assert_eq!(form.sections.len(), 4);
        assert!(form.sections[2].lift);
        assert!(form.sections[3].lift);
        assert_eq!(form.sections[2].lift_idx, 0);
        assert_eq!(form.sections[3].lift_idx, 1);
        assert!(form.sections[3].final_);
        assert_eq!(form.sections[1].intensity, 1);
    }
}

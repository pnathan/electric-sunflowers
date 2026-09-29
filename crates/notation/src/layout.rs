//! Layout and SVG output: systems, horizontal spacing, glyph placement,
//! stems and beams, ties, lyrics, chord symbols; and the timed boxes.

use std::fmt::Write as _;

use crate::drawn;
use crate::glyphs::{self, Glyph};
use crate::score::{Event, Measure, NoteEv, Score};
use song::SingerId;

/// Staff space in px.
pub(crate) const SP: f64 = 8.0;
/// Page margin in px.
pub(crate) const MARGIN: f64 = 36.0;
/// Glyph scale: 250 font units per staff space.
const GS: f64 = SP / 250.0;
pub(crate) const HEAD_W: f64 = 295.0 * GS;
pub(crate) const WHOLE_W: f64 = 422.0 * GS;
pub(crate) const STEM_LEN: f64 = 3.5 * SP;
pub(crate) const STEM_W: f64 = 0.12 * SP;
pub(crate) const PAD_L: f64 = 1.2 * SP;
pub(crate) const PAD_R: f64 = 0.6 * SP;
pub(crate) const LYRIC_PX: f64 = 12.5;
pub(crate) const CHORD_PX: f64 = 13.0;
pub(crate) const LABEL_PX: f64 = 11.5;
pub(crate) const FONT: &str = "DejaVu Serif, Liberation Serif, Georgia, serif";

/// A box on the page in px with its time span in seconds:
/// (t0, t1, x, y, w, h).
pub type TimedBox = (f64, f64, f64, f64, f64, f64);

/// The laid-out page.
pub(crate) struct Page {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    pub notes: Vec<TimedBox>,
    pub systems: Vec<TimedBox>,
}

/// Vertical position of a staff step relative to the top staff line.
pub(crate) fn y_of(step: i32) -> f64 {
    (38 - step) as f64 * SP * 0.5
}

pub(crate) fn lyric_w(t: &str) -> f64 {
    t.chars().count() as f64 * LYRIC_PX * 0.5
}

pub(crate) fn chord_w(t: &str) -> f64 {
    t.chars().count() as f64 * CHORD_PX * 0.62
}

pub(crate) fn glyph_w(g: &Glyph) -> f64 {
    (g.bbox[2] - g.bbox[0]) as f64 * GS
}

/// XML text escape.
pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// A chord symbol for print: flat and sharp signs after note letters.
pub(crate) fn chord_text(sym: &str) -> String {
    let mut o = String::new();
    let mut prev = ' ';
    for c in sym.chars() {
        let note = matches!(prev, 'A'..='G');
        o.push(match c {
            'b' if note => '\u{266D}',
            '#' if note => '\u{266F}',
            c => c,
        });
        prev = c;
    }
    esc(&o)
}

/// Appends a glyph at (x, y), scaled by `sc`.
pub(crate) fn glyph(out: &mut String, g: &Glyph, x: f64, y: f64, sc: f64) {
    let s = GS * sc;
    let _ = write!(out, r##"<use xlink:href="#g-{}" transform="translate({x:.2},{y:.2}) scale({s:.4},{:.4})"/>"##, g.name, -s);
}

pub(crate) fn line(out: &mut String, x1: f64, y1: f64, x2: f64, y2: f64, w: f64) {
    let _ = write!(out, r##"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="#111" stroke-width="{w:.2}"/>"##);
}

pub(crate) fn text(out: &mut String, x: f64, y: f64, px: f64, anchor: &str, extra: &str, body: &str) {
    let _ = write!(out, r#"<text x="{x:.1}" y="{y:.1}" font-size="{px}" text-anchor="{anchor}"{extra}>{body}</text>"#);
}

/// Width of the clef, key signature and (first system) time signature.
fn head_w(score: &Score, first: bool) -> f64 {
    let ks = score.fifths.unsigned_abs() as f64;
    3.6 * SP + if ks > 0.0 { ks * 1.05 * SP + 0.6 * SP } else { 0.0 } + if first { 3.0 * SP } else { 0.0 }
}

/// Space before the notehead (accidental, wide lyric) and the event's
/// natural width.
pub(crate) fn event_w(ev: &Event, chord: Option<&str>) -> (f64, f64) {
    let dotted = matches!(ev.d, 3 | 6 | 12);
    let mut base = SP * (1.7 + 1.2 * ((1 + ev.d) as f64).log2()) + if dotted { 0.6 * SP } else { 0.0 };
    let (mut lo, mut w) = (0.0, 0.0);
    if let Some(n) = &ev.note {
        let acc = if n.accidental.is_some() { 1.3 * SP } else { 0.0 };
        let lyr = n.lyric.as_deref().map_or(0.0, lyric_w) + if n.hyphen { 1.6 * SP } else { 0.6 * SP };
        lo = acc.max(lyr * 0.5 - HEAD_W * 0.5);
        base = base.max(HEAD_W * 0.5 + lyr * 0.5);
    }
    w += lo + base;
    if let Some(c) = chord {
        w = w.max(chord_w(c) + 0.8 * SP);
    }
    (lo, w)
}

/// Natural width of a measure and its events' (offset, width).
pub(crate) fn measure_w(m: &Measure) -> (f64, Vec<(f64, f64)>) {
    if m.empty {
        let cw: f64 = m.chords.iter().map(|c| chord_w(&c.name) + SP).sum();
        return ((7.0 * SP).max(cw + 2.0 * SP), Vec::new());
    }
    let ew: Vec<(f64, f64)> = m
        .events
        .iter()
        .map(|e| event_w(e, m.chords.iter().find(|c| c.u == e.s).map(|c| c.name.as_str())))
        .collect();
    (PAD_L + ew.iter().map(|x| x.1).sum::<f64>() + PAD_R, ew)
}

/// A placed note (one event of a system).
struct Pn<'a> {
    ev: &'a Event,
    n: &'a NoteEv,
    /// Notehead left edge and centre line y (relative to the staff top).
    x: f64,
    y: f64,
    /// Slot end (for chord interpolation).
    beat: i64,
    measure: usize,
    up: bool,
    stem_x: f64,
    stem_end: f64,
    /// In a beam group of two or more.
    beamed: bool,
}

pub(crate) fn head_glyph(d: i64) -> &'static Glyph {
    if d >= 16 {
        &glyphs::NOTEHEAD_WHOLE
    } else if d >= 8 {
        &glyphs::NOTEHEAD_HALF
    } else {
        &glyphs::NOTEHEAD_BLACK
    }
}

pub(crate) fn rest_glyph(d: i64) -> (&'static Glyph, f64) {
    match d {
        16.. => (&glyphs::REST_WHOLE, SP),
        8..=15 => (&glyphs::REST_HALF, 2.0 * SP),
        4..=7 => (&glyphs::REST_QUARTER, 2.0 * SP),
        2 | 3 => (&glyphs::REST8TH, 2.0 * SP),
        _ => (&glyphs::REST16TH, 2.0 * SP),
    }
}

pub(crate) fn fmt_time(t: f64) -> String {
    format!("{:.3}", if t.is_finite() { t } else { 0.0 })
}

/// Lays out `score`.
pub(crate) fn layout(score: &Score) -> Page {
    let width = score.width;
    let grid = score.grid;
    let ms = &score.measures;
    let nat: Vec<(f64, Vec<(f64, f64)>)> = ms.iter().map(measure_w).collect();

    // Systems: each chunk starts one; wrap when a chunk is too wide.
    let mut systems: Vec<(usize, usize, bool)> = Vec::new(); // (first, end, wrapped)
    let mut i = 0;
    while i < ms.len() {
        let avail = width - 2.0 * MARGIN - head_w(score, systems.is_empty());
        let mut j = i;
        let mut w = 0.0;
        while j < ms.len() && ms[j].chunk == ms[i].chunk && (j == i || w + nat[j].0 <= avail) {
            w += nat[j].0;
            j += 1;
        }
        let wrapped = j < ms.len() && ms[j].chunk == ms[i].chunk;
        systems.push((i, j, wrapped));
        i = j;
    }

    let mut body = String::new();
    let mut notes_out: Vec<TimedBox> = Vec::new();
    let mut sys_out: Vec<TimedBox> = Vec::new();
    // First appearance of each duet singer's system label (design 4.8):
    // full the first time, short after.
    let mut seen_a = false;
    let mut seen_b = false;

    // Title, tempo mark, caption.
    let mut cursor = MARGIN;
    text(&mut body, width * 0.5, cursor + 18.0, 22.0, "middle", "", &esc(&score.title));
    cursor += 44.0;
    {
        let (tx, ty) = (MARGIN, cursor);
        glyph(&mut body, &glyphs::NOTEHEAD_BLACK, tx, ty, 0.85);
        let sx = tx + HEAD_W * 0.85 - STEM_W * 0.5;
        line(&mut body, sx, ty, sx, ty - 3.0 * SP, STEM_W);
        let mut after = tx + 1.6 * SP;
        if grid.compound {
            glyph(&mut body, &glyphs::AUGMENTATION_DOT, tx + 1.4 * SP, ty, 0.9);
            after += 0.7 * SP;
        }
        text(&mut body, after, ty + 4.0, 13.0, "start", "", &format!("= {}", score.tempo.round()));
        text(&mut body, width - MARGIN, ty + 4.0, 13.0, "end", "", &esc(&score.caption));
    }
    cursor += 2.0 * SP;

    let last_measure = ms.len().saturating_sub(1);
    for (si, &(first, end, wrapped)) in systems.iter().enumerate() {
        let head = head_w(score, si == 0);
        let avail = width - 2.0 * MARGIN - head;
        let natural: f64 = nat[first..end].iter().map(|x| x.0).sum();
        let scale = if natural <= 0.0 {
            1.0
        } else if wrapped || natural >= 0.3 * avail {
            avail / natural
        } else {
            (avail / natural).min(2.0)
        };

        // A shared line (design 4.8): a second staff is drawn below the
        // melody staff's lyric row, with its own lyric row under it. Never
        // true outside a duet. `swap_staves` routes the physically higher
        // staff (drawn first, at the system's top) to whichever singer's
        // range centre is higher (`Measure::melody_on_top`), so the melody
        // is drawn on top only when it is also the higher singer; a
        // non-shared bar always keeps the melody on top (there is nothing
        // to swap it with).
        let is_shared = score.duet && ms[first].shared;
        let swap_staves = is_shared && !ms[first].melody_on_top;

        // Horizontal placement.
        let mut pns: Vec<Pn> = Vec::new();
        let mut rests: Vec<(f64, &Event)> = Vec::new();
        let mut mx = Vec::with_capacity(end - first); // (x0, width)
        let mut x = MARGIN + head;
        for (k, m) in ms[first..end].iter().enumerate() {
            let mw = nat[first + k].0 * scale;
            mx.push((x, mw));
            let mut ex = x + PAD_L * scale;
            let top_events = if swap_staves { &m.second } else { &m.events };
            for (e, &(lo, w)) in top_events.iter().zip(&nat[first + k].1) {
                match &e.note {
                    Some(n) => pns.push(Pn {
                        ev: e,
                        n,
                        x: ex + lo,
                        y: y_of(n.step),
                        beat: e.s / grid.beat_u,
                        measure: first + k,
                        up: n.step < 34,
                        stem_x: 0.0,
                        stem_end: 0.0,
                        beamed: false,
                    }),
                    None => rests.push((ex, e)),
                }
                ex += w * scale;
            }
            x += mw;
        }
        let sys_end = x;

        // Beam groups: eighths and shorter within one beat, contiguous.
        let mut groups: Vec<Vec<usize>> = Vec::new();
        for k in 0..pns.len() {
            let p = &pns[k];
            if p.ev.d >= 16 {
                continue;
            }
            let joins = p.ev.d < 4
                && groups.last().and_then(|g| g.last()).is_some_and(|&q| {
                    let a = &pns[q];
                    a.ev.d < 4 && a.measure == p.measure && a.beat == p.beat && a.ev.s + a.ev.d == p.ev.s
                });
            if joins {
                if let Some(g) = groups.last_mut() {
                    g.push(k);
                }
            } else {
                groups.push(vec![k]);
            }
        }
        for g in &groups {
            let up = if g.len() > 1 {
                g.iter().map(|&k| pns[k].n.step as f64).sum::<f64>() / (g.len() as f64) < 34.0
            } else {
                pns[g[0]].up
            };
            for &k in g {
                let p = &mut pns[k];
                p.up = up;
                p.stem_x = if up { p.x + HEAD_W - STEM_W * 0.5 } else { p.x + STEM_W * 0.5 };
                p.stem_end = if up { (p.y - STEM_LEN).min(2.0 * SP) } else { (p.y + STEM_LEN).max(2.0 * SP) };
                p.beamed = g.len() > 1;
            }
            if g.len() > 1 {
                let (f, l) = (g[0], g[g.len() - 1]);
                let (xa, xb) = (pns[f].stem_x, pns[l].stem_x);
                let dx = (xb - xa).max(1.0);
                let slope = ((pns[l].stem_end - pns[f].stem_end) / dx).clamp(-0.2, 0.2);
                let mut ya = pns[f].stem_end;
                // Every stem at least 3 spaces.
                for &k in g {
                    let p = &pns[k];
                    let at = ya + slope * (p.stem_x - xa);
                    if up && at > p.y - 3.0 * SP {
                        ya -= at - (p.y - 3.0 * SP);
                    } else if !up && at < p.y + 3.0 * SP {
                        ya += (p.y + 3.0 * SP) - at;
                    }
                }
                for &k in g {
                    let p = &mut pns[k];
                    p.stem_end = ya + slope * (p.stem_x - xa);
                }
            }
        }

        // Vertical extents.
        let mut top = -0.5 * SP;
        let mut bot = 4.5 * SP;
        for p in &pns {
            let (a, b) = if p.ev.d >= 16 {
                (p.y, p.y)
            } else if p.up {
                (p.stem_end - SP, p.y)
            } else {
                (p.y, p.stem_end + 0.6 * SP)
            };
            top = top.min(a.min(p.y) - 0.6 * SP);
            bot = bot.max(b.max(p.y) + 0.6 * SP);
        }
        let y_chord = (-2.4 * SP).min(top - 0.6 * SP);
        let has_label = ms[first..end].iter().any(|m| m.label.is_some());
        let y_label = y_chord - 2.2 * SP;
        let y_lyric = (7.4 * SP).max(bot + 2.2 * SP);
        let sys_top = if has_label { y_label - 1.8 * SP } else { y_chord - 2.0 * SP };
        let y2 = y_lyric + 3.0 * SP;
        let sys_bot = if is_shared { y2 + 4.0 * SP + 3.6 * SP } else { y_lyric + 1.4 * SP };
        let oy = cursor - sys_top;

        let t0 = ms[first].t0;
        let t1 = ms[end - 1].t1;
        sys_out.push((t0, t1, MARGIN, oy + sys_top, sys_end - MARGIN, sys_bot - sys_top));
        let mut s = String::new();
        let _ = write!(s, r#"<g class="system" data-bar="{}" data-t0="{}" data-t1="{}" transform="translate(0,{oy:.2})">"#, ms[first].bar, fmt_time(t0), fmt_time(t1));

        // Staff, clef, key and time signatures.
        for l in 0..5 {
            line(&mut s, MARGIN, l as f64 * SP, sys_end, l as f64 * SP, 0.13 * SP);
        }
        let clef8 = if swap_staves { ms[first].second_clef8 } else { ms[first].clef8 };
        glyph(&mut s, if clef8 { &glyphs::G_CLEF8VB } else { &glyphs::G_CLEF }, MARGIN + 0.5 * SP, 3.0 * SP, 1.0);
        let mut hx = MARGIN + 3.6 * SP;
        let (steps, acc) = if score.fifths > 0 {
            ([38, 35, 39, 36, 33, 37, 34], &glyphs::ACCIDENTAL_SHARP)
        } else {
            ([34, 37, 33, 36, 32, 35, 31], &glyphs::ACCIDENTAL_FLAT)
        };
        for &st in steps.iter().take(score.fifths.unsigned_abs().min(7) as usize) {
            glyph(&mut s, acc, hx, y_of(st), 1.0);
            hx += 1.05 * SP;
        }
        if si == 0 {
            let (num, den) = match score.meter {
                song::Meter::Four4 => (4, 4),
                song::Meter::Three4 => (3, 4),
                song::Meter::Six8 => (6, 8),
            };
            let tx = MARGIN + head - 2.6 * SP;
            for (d, y) in [(num, SP), (den, 3.0 * SP)] {
                let g = &glyphs::TIME_SIG[d];
                glyph(&mut s, g, tx - glyph_w(g) * 0.5, y, 1.0);
            }
        }

        // Duet system label ("A (Baritone)", "B", "A+B"): the melody
        // singer of this system's line, first full then short, or "A+B"
        // on a shared line. Never drawn outside a duet, so a solo song's
        // SVG is unchanged.
        if score.duet {
            let m0 = &ms[first];
            let lbl = if m0.shared {
                Some("A+B".to_string())
            } else {
                m0.singer.map(|sid| match sid {
                    SingerId::A => {
                        if seen_a {
                            "A".to_string()
                        } else {
                            seen_a = true;
                            format!("A ({})", score.voice_a.label())
                        }
                    }
                    SingerId::B => {
                        if seen_b {
                            "B".to_string()
                        } else {
                            seen_b = true;
                            format!("B ({})", score.voice_b.map_or("", song::Voice::label))
                        }
                    }
                })
            };
            if let Some(lbl) = lbl {
                text(&mut s, MARGIN, y_label, LABEL_PX, "start", r#" font-weight="bold""#, &esc(&lbl));
            }
        }

        // Section labels, chord symbols, bar lines, whole-bar rests.
        for (k, m) in ms[first..end].iter().enumerate() {
            let (x0, mw) = mx[k];
            let gi = first + k;
            if let Some(lb) = &m.label {
                let _ = write!(
                    s,
                    r#"<text class="section" x="{:.1}" y="{:.1}" font-size="{LABEL_PX}" font-weight="bold" font-style="italic">{}</text>"#,
                    x0 + 0.2 * SP,
                    y_label,
                    esc(lb)
                );
            }
            let mut marks: Vec<(i64, &str)> = m.chords.iter().map(|c| (c.u, c.name.as_str())).collect();
            if k == 0 && !marks.iter().any(|c| c.0 == 0) {
                if let Some(sn) = &m.sounding {
                    marks.insert(0, (0, sn.as_str()));
                }
            }
            for (u, name) in marks {
                let cx = if m.empty {
                    x0 + SP + (u - m.from_u) as f64 / grid.bar_u as f64 * (mw - 2.0 * SP)
                } else {
                    // At the event sounding at `u`, interpolated within it.
                    let mut ex = x0 + PAD_L * scale;
                    let mut at = ex;
                    for (e, &(lo, w)) in m.events.iter().zip(&nat[gi].1) {
                        if e.s <= u {
                            at = ex + lo + (u - e.s) as f64 / e.d as f64 * w * scale;
                        }
                        ex += w * scale;
                    }
                    at
                };
                let _ = write!(
                    s,
                    r#"<text class="chord" x="{cx:.1}" y="{y_chord:.1}" font-size="{CHORD_PX}" font-weight="bold">{}</text>"#,
                    chord_text(name)
                );
            }
            if m.empty {
                glyph(&mut s, &glyphs::REST_WHOLE, x0 + mw * 0.5 - glyph_w(&glyphs::REST_WHOLE) * 0.5, SP, 1.0);
            }
            let bx = x0 + mw;
            if gi == last_measure {
                line(&mut s, bx - 0.75 * SP, 0.0, bx - 0.75 * SP, 4.0 * SP, 0.16 * SP);
                let _ = write!(s, r##"<rect x="{:.2}" y="0" width="{:.2}" height="{:.2}" fill="#111"/>"##, bx - 0.5 * SP, 0.5 * SP, 4.0 * SP);
            } else if m.section_end {
                line(&mut s, bx - 0.5 * SP, 0.0, bx - 0.5 * SP, 4.0 * SP, 0.16 * SP);
                line(&mut s, bx, 0.0, bx, 4.0 * SP, 0.16 * SP);
            } else {
                line(&mut s, bx, 0.0, bx, 4.0 * SP, 0.16 * SP);
            }
        }

        // Rests.
        for (ex, e) in &rests {
            let (g, y) = rest_glyph(e.d);
            glyph(&mut s, g, ex + 0.2 * SP, y, 1.0);
            if matches!(e.d, 3 | 6 | 12) {
                glyph(&mut s, &glyphs::AUGMENTATION_DOT, ex + 0.2 * SP + glyph_w(g) + 0.3 * SP, 1.5 * SP, 1.0);
            }
        }

        // Notes, each a group with its timing.
        for p in &pns {
            let n = p.n;
            let _ = write!(
                s,
                r#"<g class="note" data-note="{}" data-line="{}" data-t0="{}" data-t1="{}">"#,
                n.note,
                n.line,
                fmt_time(n.t0),
                fmt_time(n.t1)
            );
            let hw = if p.ev.d >= 16 { WHOLE_W } else { HEAD_W };
            let mut lp = 28;
            while lp >= n.step {
                line(&mut s, p.x - 0.4 * SP, y_of(lp), p.x + hw + 0.4 * SP, y_of(lp), 0.16 * SP);
                lp -= 2;
            }
            let mut lp = 40;
            while lp <= n.step {
                line(&mut s, p.x - 0.4 * SP, y_of(lp), p.x + hw + 0.4 * SP, y_of(lp), 0.16 * SP);
                lp += 2;
            }
            if let Some(a) = n.accidental {
                let g = match a {
                    1 => &glyphs::ACCIDENTAL_SHARP,
                    -1 => &glyphs::ACCIDENTAL_FLAT,
                    _ => &glyphs::ACCIDENTAL_NATURAL,
                };
                glyph(&mut s, g, p.x - glyph_w(g) - 0.3 * SP, p.y, 1.0);
            }
            glyph(&mut s, head_glyph(p.ev.d), p.x, p.y, 1.0);
            if matches!(p.ev.d, 3 | 6 | 12) {
                let dy = if n.step % 2 == 0 { p.y - 0.5 * SP } else { p.y };
                glyph(&mut s, &glyphs::AUGMENTATION_DOT, p.x + hw + 0.35 * SP, dy, 1.0);
            }
            if p.ev.d < 16 {
                line(&mut s, p.stem_x, p.y, p.stem_x, p.stem_end, STEM_W);
                if p.ev.d < 4 && !p.beamed {
                    let g = match (p.ev.d == 1, p.up) {
                        (true, true) => &glyphs::FLAG16TH_UP,
                        (true, false) => &glyphs::FLAG16TH_DOWN,
                        (false, true) => &glyphs::FLAG8TH_UP,
                        (false, false) => &glyphs::FLAG8TH_DOWN,
                    };
                    glyph(&mut s, g, p.stem_x - STEM_W * 0.5, p.stem_end, 1.0);
                }
            }
            if let Some(l) = &n.lyric {
                text(&mut s, p.x + hw * 0.5, y_lyric, LYRIC_PX, "middle", r#" class="lyric""#, &esc(l));
            }
            s.push_str("</g>");

            let bw = (hw + SP).max(n.lyric.as_deref().map_or(0.0, lyric_w) + 0.4 * SP);
            notes_out.push((n.t0, n.t1, p.x + hw * 0.5 - bw * 0.5, oy - 1.0 * SP, bw, y_lyric + 0.6 * SP + SP));
        }

        // Beams.
        for g in groups.iter().filter(|g| g.len() > 1) {
            let (f, l) = (&pns[g[0]], &pns[g[g.len() - 1]]);
            let dir = if f.up { 1.0 } else { -1.0 };
            let bt = 0.5 * SP;
            let slope = (l.stem_end - f.stem_end) / (l.stem_x - f.stem_x).max(1.0);
            let at = |x: f64| f.stem_end + slope * (x - f.stem_x);
            let beam = |s: &mut String, x1: f64, x2: f64, off: f64| {
                let (y1, y2) = (at(x1) + off * dir, at(x2) + off * dir);
                let _ = write!(
                    s,
                    r##"<path d="M{x1:.2} {y1:.2}L{x2:.2} {y2:.2}L{x2:.2} {:.2}L{x1:.2} {:.2}Z" fill="#111"/>"##,
                    y2 + bt * dir,
                    y1 + bt * dir
                );
            };
            beam(&mut s, f.stem_x - STEM_W * 0.5, l.stem_x + STEM_W * 0.5, 0.0);
            for (j, &k) in g.iter().enumerate() {
                let p = &pns[k];
                if p.ev.d != 1 {
                    continue;
                }
                let next = g.get(j + 1).map(|&q| &pns[q]).filter(|q| q.ev.d == 1);
                let prev = j.checked_sub(1).map(|q| &pns[g[q]]).filter(|q| q.ev.d == 1);
                if let Some(nx) = next {
                    beam(&mut s, p.stem_x - STEM_W * 0.5, nx.stem_x + STEM_W * 0.5, 0.75 * SP);
                } else if prev.is_none() {
                    let hook = if j + 1 < g.len() { p.stem_x + SP } else { p.stem_x - SP };
                    beam(&mut s, p.stem_x.min(hook), p.stem_x.max(hook), 0.75 * SP);
                }
            }
        }

        // Ties and hyphens.
        for (k, p) in pns.iter().enumerate() {
            let hw = if p.ev.d >= 16 { WHOLE_W } else { HEAD_W };
            if p.n.tie_out {
                let nx = pns.get(k + 1).filter(|q| q.n.note == p.n.note);
                let x1 = p.x + hw + 0.15 * SP;
                let x2 = nx.map_or(sys_end - 0.3 * SP, |q| q.x - 0.15 * SP);
                let below = p.up;
                let yy = p.y + if below { 0.6 * SP } else { -0.6 * SP };
                let c = if below { 1.1 * SP } else { -1.1 * SP };
                let xm = (x1 + x2) * 0.5;
                let _ = write!(
                    s,
                    r##"<path d="M{x1:.2} {yy:.2}Q{xm:.2} {:.2} {x2:.2} {yy:.2}Q{xm:.2} {:.2} {x1:.2} {yy:.2}Z" fill="#111"/>"##,
                    yy + c,
                    yy + c * 0.7
                );
            }
            if p.n.hyphen {
                let Some(l) = &p.n.lyric else { continue };
                let a = p.x + hw * 0.5 + lyric_w(l) * 0.5;
                let nx = pns[k + 1..].iter().find(|q| q.n.lyric.is_some());
                let b = match nx {
                    Some(q) => q.x + HEAD_W * 0.5 - q.n.lyric.as_deref().map_or(0.0, lyric_w) * 0.5,
                    None => (a + 2.0 * SP).min(sys_end),
                };
                if b - a > 0.4 * SP {
                    let half = ((b - a) * 0.3).min(0.4 * SP);
                    let m = (a + b) * 0.5;
                    line(&mut s, m - half, y_lyric - 3.8, m + half, y_lyric - 3.8, 0.9);
                }
            }
        }

        // Second staff (shared lines only, design 4.8): the other singer's
        // notes on their own staff, joined to the melody staff by a
        // bracket, with a small "melody" label over whichever staff is
        // physically the melody staff (`swap_staves` above already routed
        // the higher-centre singer to the top row, per design 4.8's
        // "higher singer on top"; the label follows the melody, not the
        // top row). Rendering is simpler than the top staff's: no beam
        // grouping, each note its own stem and flag.
        if is_shared {
            for l in 0..5 {
                line(&mut s, MARGIN, y2 + l as f64 * SP, sys_end, y2 + l as f64 * SP, 0.13 * SP);
            }
            let clef2 = if swap_staves { ms[first].clef8 } else { ms[first].second_clef8 };
            glyph(&mut s, if clef2 { &glyphs::G_CLEF8VB } else { &glyphs::G_CLEF }, MARGIN + 0.5 * SP, y2 + 3.0 * SP, 1.0);
            drawn::bracket(&mut s, MARGIN - 0.3 * SP, 0.0, y2 + 4.0 * SP, 1.0);
            let melody_label_y = if swap_staves { y2 - 0.3 * SP } else { -0.3 * SP };
            text(&mut s, MARGIN + head, melody_label_y, 10.0, "start", r#" font-style="italic""#, "melody");
            let y_lyric2 = y2 + 6.2 * SP;

            for (k, m) in ms[first..end].iter().enumerate() {
                let (bx0, _) = mx[k];
                let gi = first + k;
                let mut ex = bx0 + PAD_L * scale;
                let bot_events: &Vec<Event> = if swap_staves { &m.events } else { &m.second };
                for (e, &(lo, w)) in bot_events.iter().zip(&nat[gi].1) {
                    let xx = ex + lo * scale;
                    match &e.note {
                        None => {
                            let (g, y) = rest_glyph(e.d);
                            glyph(&mut s, g, xx + 0.2 * SP, y2 + y, 1.0);
                            if matches!(e.d, 3 | 6 | 12) {
                                glyph(&mut s, &glyphs::AUGMENTATION_DOT, xx + 0.2 * SP + glyph_w(g) + 0.3 * SP, y2 + 1.5 * SP, 1.0);
                            }
                        }
                        Some(n) => {
                            let py = y_of(n.step);
                            let hw = if e.d >= 16 { WHOLE_W } else { HEAD_W };
                            let up = n.step < 34;
                            let stem_x = if up { xx + hw - STEM_W * 0.5 } else { xx + STEM_W * 0.5 };
                            let stem_end = if up { (py - STEM_LEN).min(2.0 * SP) } else { (py + STEM_LEN).max(2.0 * SP) };
                            let mut lp = 28;
                            while lp >= n.step {
                                line(&mut s, xx - 0.4 * SP, y2 + y_of(lp), xx + hw + 0.4 * SP, y2 + y_of(lp), 0.16 * SP);
                                lp -= 2;
                            }
                            let mut lp = 40;
                            while lp <= n.step {
                                line(&mut s, xx - 0.4 * SP, y2 + y_of(lp), xx + hw + 0.4 * SP, y2 + y_of(lp), 0.16 * SP);
                                lp += 2;
                            }
                            if let Some(a) = n.accidental {
                                let g = match a {
                                    1 => &glyphs::ACCIDENTAL_SHARP,
                                    -1 => &glyphs::ACCIDENTAL_FLAT,
                                    _ => &glyphs::ACCIDENTAL_NATURAL,
                                };
                                glyph(&mut s, g, xx - glyph_w(g) - 0.3 * SP, y2 + py, 1.0);
                            }
                            glyph(&mut s, head_glyph(e.d), xx, y2 + py, 1.0);
                            if matches!(e.d, 3 | 6 | 12) {
                                let dy = if n.step % 2 == 0 { py - 0.5 * SP } else { py };
                                glyph(&mut s, &glyphs::AUGMENTATION_DOT, xx + hw + 0.35 * SP, y2 + dy, 1.0);
                            }
                            if e.d < 16 {
                                line(&mut s, stem_x, y2 + py, stem_x, y2 + stem_end, STEM_W);
                                if e.d < 4 {
                                    let g = match (e.d == 1, up) {
                                        (true, true) => &glyphs::FLAG16TH_UP,
                                        (true, false) => &glyphs::FLAG16TH_DOWN,
                                        (false, true) => &glyphs::FLAG8TH_UP,
                                        (false, false) => &glyphs::FLAG8TH_DOWN,
                                    };
                                    glyph(&mut s, g, stem_x - STEM_W * 0.5, y2 + stem_end, 1.0);
                                }
                            }
                            if let Some(l) = &n.lyric {
                                text(&mut s, xx + hw * 0.5, y_lyric2, LYRIC_PX, "middle", r#" class="lyric""#, &esc(l));
                            }
                            let bw = (hw + SP).max(n.lyric.as_deref().map_or(0.0, lyric_w) + 0.4 * SP);
                            notes_out.push((n.t0, n.t1, xx + hw * 0.5 - bw * 0.5, oy + y2 - 1.0 * SP, bw, y_lyric2 - y2 + 0.6 * SP + SP));
                        }
                    }
                    ex += w * scale;
                }
            }
        }

        s.push_str("</g>");
        body.push_str(&s);
        cursor += sys_bot - sys_top;
    }

    // A shared system's second-staff boxes are appended after its melody
    // boxes; a stable sort by t0 alone puts everything in time order while
    // keeping the melody box first on a tie (design 4.8's note-box rule).
    // A no-op for a solo score, already in time order.
    notes_out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let height = (cursor + MARGIN * 0.5).ceil();
    let mut svg = String::with_capacity(body.len() + 20_000);
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}" font-family="{FONT}" fill="#111">"##
    );
    let _ = write!(svg, r##"<rect width="{width:.0}" height="{height:.0}" fill="#fff"/><defs>"##);
    for g in glyphs::ALL {
        let _ = write!(svg, r#"<path id="g-{}" d="{}"/>"#, g.name, g.path);
    }
    svg.push_str("</defs>");
    svg.push_str(&body);
    svg.push_str("</svg>\n");
    Page { svg, width, height, notes: notes_out, systems: sys_out }
}

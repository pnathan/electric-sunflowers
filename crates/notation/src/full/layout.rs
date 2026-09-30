//! Multi-staff system layout for `FullScore` (the full score) and
//! `PartScore` (one part's own view). See `docs/features-2.md` section 6.2.
//!
//! Column alignment ("align onsets across staves"): for one bar, the onset
//! units of every staff's every voice are pooled into one sorted,
//! deduplicated list; each unit becomes one x column shared by every staff
//! in the system, so a note or rest at that unit draws at the same x
//! wherever it occurs. A column's width comes from the gap to the next
//! column (in grid units, the lead sheet's own width curve from
//! `crate::layout::event_w`), widened for an accidental or a lyric on any
//! staff at that unit, and for a chord symbol at that unit.
//!
//! Staff row heights are fixed allowances (ledger-line room above and
//! below, a lyric line under the Lead and Lead B staves), not measured
//! from each system's actual content the way the lead sheet does: simpler,
//! at the cost of a little extra white space on plain systems.
//!
//! A note's vertical position reuses `crate::layout::y_of`, which assumes
//! treble-clef staff positions (top line step 38). A `Bass`/`Bass8vb` staff
//! reads the same absolute diatonic step (`Head::step`, already written in
//! the printed octave by `full::model`) through `clef_offset`, which shifts
//! it onto the same 5-line frame the treble clef uses, so every staff
//! shares one drawing routine.

use std::fmt::Write as _;

use crate::drawn;
use crate::full::model::{
    BarCol, Cell, Clef, Ev, FullScore, Group, Head, Notehead, PartBar, PartId, PartScore, StaffDef,
};
use crate::glyphs::{self, Glyph};
use crate::layout::{
    self, chord_text, chord_w, esc, fmt_time, glyph, glyph_w, head_glyph, line, lyric_w,
    rest_glyph, text, Page, HEAD_W, LABEL_PX, LYRIC_PX, MARGIN, PAD_L, PAD_R, SP, STEM_LEN, STEM_W,
    WHOLE_W,
};

const CHORD_PX: f64 = 13.0;
/// Extra top margin on a system's first staff, for the section label.
const LABEL_ROOM: f64 = 2.2 * SP;
/// Ledger/accidental/chord-symbol room above a staff, and ledger room below.
const ROW_PAD: f64 = 3.0 * SP;
/// Extra room below a staff that carries lyrics.
const LYRIC_ROOM: f64 = 3.0 * SP;

/// One bar's natural (unscaled) width, and its onset columns: (unit, left
/// offset from the column's `ex`, natural width).
type BarNat = (f64, Vec<(i64, f64, f64)>);

/// Absolute-diatonic-step offset that maps `Head::step` (already written in
/// the printed octave) onto the treble-based frame `crate::layout::y_of`
/// draws every staff in.
fn clef_offset(clef: Clef) -> i32 {
    match clef {
        Clef::Bass | Clef::Bass8vb => 12,
        Clef::Treble | Clef::Treble8vb | Clef::Percussion => 0,
    }
}

fn y_step(clef: Clef, step: i32) -> f64 {
    layout::y_of(step + clef_offset(clef))
}

fn lyric_staff(part: PartId) -> bool {
    matches!(part, PartId::Lead | PartId::LeadB)
}

/// Width contributed by a clef glyph (Bravura for treble, drawn for the
/// rest): not measured, an eyeballed footprint wide enough for the shape
/// `drawn` actually draws.
fn clef_w(clef: Clef) -> f64 {
    match clef {
        Clef::Treble => glyph_w(&glyphs::G_CLEF),
        Clef::Treble8vb => glyph_w(&glyphs::G_CLEF8VB),
        Clef::Bass | Clef::Bass8vb => 2.0 * SP,
        Clef::Percussion => 1.1 * SP,
    }
}

/// Column width: the lead sheet's own duration curve (`event_w`'s `base`),
/// widened for an accidental, a lyric, or a chord symbol at this column.
fn col_w(
    gap: i64,
    accidental: bool,
    lyric: Option<(&str, bool)>,
    chord: Option<&str>,
) -> (f64, f64) {
    let mut base = SP * (1.7 + 1.2 * ((1 + gap.max(1)) as f64).log2());
    let mut lo: f64 = if accidental { 1.3 * SP } else { 0.0 };
    if let Some((l, hyphen)) = lyric {
        let lyr = lyric_w(l) + if hyphen { 1.6 * SP } else { 0.6 * SP };
        lo = lo.max(lyr * 0.5 - HEAD_W * 0.5);
        base = base.max(HEAD_W * 0.5 + lyr * 0.5);
    }
    let mut w = lo + base;
    if let Some(c) = chord {
        w = w.max(chord_w(c) + 0.8 * SP);
    }
    (lo, w)
}

/// One bar's onset columns, pooled from every staff's every voice: (unit,
/// left offset from the column's `ex`, natural width).
fn bar_columns(bar: &BarCol, staves: &[StaffDef], bar_u: i64) -> BarNat {
    let mut units: Vec<i64> = bar
        .cells
        .iter()
        .flat_map(|c| c.voices.iter())
        .flat_map(|v| v.iter())
        .map(|e| e.s)
        .collect();
    units.sort_unstable();
    units.dedup();
    if units.is_empty() {
        units.push(0);
    }
    let mut cols = Vec::with_capacity(units.len());
    let mut total = PAD_L;
    for (i, &u) in units.iter().enumerate() {
        let next = units.get(i + 1).copied().unwrap_or(bar_u);
        let gap = next - u;
        let mut accidental = false;
        let mut lyric: Option<(&str, bool)> = None;
        for (si, cell) in bar.cells.iter().enumerate() {
            for voice in &cell.voices {
                let Some(e) = voice.iter().find(|e| e.s == u) else {
                    continue;
                };
                let Some(c) = &e.chord else { continue };
                if !c.tie_in && c.heads.iter().any(|h| h.accidental.is_some()) {
                    accidental = true;
                }
                if lyric_staff(staves[si].part) {
                    if let Some(l) = &c.lyric {
                        lyric = Some((l.as_str(), c.hyphen));
                    }
                }
            }
        }
        let chord = bar.chords.iter().find(|c| c.0 == u).map(|c| c.1.as_str());
        let (lo, w) = col_w(gap, accidental, lyric, chord);
        cols.push((u, lo, w));
        total += w;
    }
    (total + PAD_R, cols)
}

/// A staff row's vertical extent within its system: (top-line y, bottom
/// margin's own extent), relative to the system's own top.
struct Row {
    top: f64,
}

fn staff_rows(staves: &[StaffDef]) -> (Vec<Row>, f64) {
    let mut y = 0.0;
    let mut rows = Vec::with_capacity(staves.len());
    for (i, st) in staves.iter().enumerate() {
        y += if i == 0 {
            ROW_PAD + LABEL_ROOM
        } else {
            ROW_PAD
        };
        rows.push(Row { top: y });
        y += 4.0 * SP + ROW_PAD;
        if lyric_staff(st.part) {
            y += LYRIC_ROOM;
        }
    }
    (rows, y)
}

/// Contiguous runs of staves sharing one `Group` (brackets and the harp
/// brace); `ALL_PARTS` already orders staves so each group is contiguous.
fn groups(staves: &[StaffDef]) -> Vec<(Group, usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < staves.len() {
        let g = staves[i].group;
        let mut j = i + 1;
        while j < staves.len() && staves[j].group == g {
            j += 1;
        }
        out.push((g, i, j));
        i = j;
    }
    out
}

/// Head x-offsets for a chord's heads, in the order given: a second (heads
/// one step apart) puts the higher head on the other side of the stem.
fn chord_offsets(heads: &[Head]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..heads.len()).collect();
    order.sort_by_key(|&i| heads[i].step);
    let mut off = vec![0.0; heads.len()];
    for k in 1..order.len() {
        let (a, b) = (order[k - 1], order[k]);
        if heads[b].step - heads[a].step == 1 && off[a] == 0.0 {
            off[b] = HEAD_W;
        }
    }
    off
}

fn notehead_glyph(nh: Notehead, d: i64) -> Option<&'static Glyph> {
    match nh {
        Notehead::Normal => Some(head_glyph(d)),
        Notehead::X | Notehead::Slash => None,
    }
}

/// Draws one staff's clef (and, for a pitched staff, the key signature) at
/// the system's start. `x` is the left edge; returns the x just past what
/// was drawn.
fn draw_clef(out: &mut String, staff: &StaffDef, x: f64, row_top: f64, fifths: i32) {
    let mid = row_top + y_step(staff.clef, 34); // the staff's own middle line
    match staff.clef {
        // The G clef's Bravura anchor sits on the G4 line (the lead
        // sheet's own convention: `y_of(32)` is `3.0 * SP` below the top).
        Clef::Treble => glyph(out, &glyphs::G_CLEF, x, row_top + 3.0 * SP, 1.0),
        Clef::Treble8vb => glyph(out, &glyphs::G_CLEF8VB, x, row_top + 3.0 * SP, 1.0),
        Clef::Bass | Clef::Bass8vb => {
            // F3 (abs step 24): the F clef's own line, the second line from
            // the top of a plain bass staff.
            let f_line = row_top + y_step(staff.clef, 24);
            drawn::f_clef(out, x, f_line, 1.0);
            if staff.clef == Clef::Bass8vb {
                // Ottava bassa: the "8" sits below the clef, near the
                // staff's bottom line.
                drawn::clef_8(out, x + 0.9 * SP, f_line + 2.3 * SP, 9.0);
            }
        }
        Clef::Percussion => drawn::percussion_clef(out, x, mid, 1.0),
    }
    if matches!(staff.clef, Clef::Percussion) || fifths == 0 {
        return;
    }
    let mut hx = x + clef_w(staff.clef) + 0.5 * SP;
    let (steps, acc): (&[i32], &Glyph) = if fifths > 0 {
        (&[38, 35, 39, 36, 33, 37, 34], &glyphs::ACCIDENTAL_SHARP)
    } else {
        (&[34, 37, 33, 36, 32, 35, 31], &glyphs::ACCIDENTAL_FLAT)
    };
    for &st in steps.iter().take(fifths.unsigned_abs().min(7) as usize) {
        glyph(out, acc, hx, row_top + y_step(staff.clef, st), 1.0);
        hx += 1.05 * SP;
    }
}

fn key_w(fifths: i32) -> f64 {
    if fifths == 0 {
        0.0
    } else {
        fifths.unsigned_abs() as f64 * 1.05 * SP + 0.6 * SP
    }
}

/// Space at a system's start: the widest clef plus the key signature (and,
/// on the first system, the time signature).
fn head_w(staves: &[StaffDef], fifths: i32, first: bool) -> f64 {
    let clef = staves
        .iter()
        .map(|s| clef_w(s.clef))
        .fold(0.0_f64, f64::max);
    let pitched = staves.iter().any(|s| s.clef != Clef::Percussion);
    1.2 * SP
        + clef
        + if pitched {
            key_w(fifths) + 0.6 * SP
        } else {
            0.6 * SP
        }
        + if first { 3.0 * SP } else { 0.0 }
}

/// One voice's stem direction: fixed by voice index when the staff carries
/// two voices (up for voice 0, down for voice 1); otherwise by the note's
/// position, as the lead sheet does.
fn stem_up(n_voices: usize, voice: usize, rep_step: i32) -> bool {
    if n_voices > 1 {
        voice == 0
    } else {
        rep_step < 34
    }
}

/// Draws one system (bars `first..end` of `bars`) for every staff, appends
/// its markup to `body`, and pushes its column and system boxes.
#[allow(clippy::too_many_arguments)]
fn draw_system(
    body: &mut String,
    notes_out: &mut Vec<(f64, f64, f64, f64, f64, f64)>,
    sys_out: &mut Vec<(f64, f64, f64, f64, f64, f64)>,
    staves: &[StaffDef],
    bars: &[BarCol],
    first: usize,
    end: usize,
    width: f64,
    meter: song::Meter,
    is_first_system: bool,
    cursor: f64,
    nat: &[BarNat],
    bar_u: i64,
) -> f64 {
    // The system's key signature: that of its first bar's section.
    let fifths = bars[first].fifths;
    let head = head_w(staves, fifths, is_first_system);
    let avail = width - 2.0 * MARGIN - head;
    let natural: f64 = nat[first..end].iter().map(|x| x.0).sum();
    let wrapped = end < bars.len() && bars[end].chunk == bars[first].chunk;
    let scale = if natural <= 0.0 {
        1.0
    } else if wrapped || natural >= 0.3 * avail {
        avail / natural
    } else {
        (avail / natural).min(2.0)
    };

    let (rows, sys_h) = staff_rows(staves);
    let oy = cursor;
    let t0 = bars[first].t0;
    let t1 = bars[end - 1].t1;
    sys_out.push((t0, t1, MARGIN, oy, width - 2.0 * MARGIN, sys_h));

    let mut s = String::new();
    let _ = write!(
        s,
        r#"<g class="system" data-bar="{}" data-t0="{}" data-t1="{}" transform="translate(0,{oy:.2})">"#,
        bars[first].bar,
        fmt_time(t0),
        fmt_time(t1)
    );

    // Staves, clefs, key signature, time signature (first system only).
    let sys_end = MARGIN + head + nat[first..end].iter().map(|x| x.0).sum::<f64>() * scale;
    for (i, st) in staves.iter().enumerate() {
        for l in 0..5 {
            line(
                &mut s,
                MARGIN,
                rows[i].top + l as f64 * SP,
                sys_end,
                rows[i].top + l as f64 * SP,
                0.13 * SP,
            );
        }
        draw_clef(&mut s, st, MARGIN + 0.6 * SP, rows[i].top, fifths);
        if is_first_system {
            let (num, den) = match meter {
                song::Meter::Four4 => (4, 4),
                song::Meter::Three4 => (3, 4),
                song::Meter::Six8 => (6, 8),
            };
            let tx = MARGIN + head - 2.6 * SP;
            for (d, dy) in [(num, 0.0), (den, 2.0 * SP)] {
                let g = &glyphs::TIME_SIG[d];
                glyph(&mut s, g, tx - glyph_w(g) * 0.5, rows[i].top + SP + dy, 1.0);
            }
        }
    }

    // Brackets and braces.
    for (g, gi, gj) in groups(staves) {
        if gj - gi < 2 {
            continue;
        }
        let y_top = rows[gi].top;
        let y_bot = rows[gj - 1].top + 4.0 * SP;
        if g == Group::Harp {
            drawn::brace(&mut s, MARGIN - 0.3 * SP, y_top, y_bot, 1.0);
        } else {
            drawn::bracket(&mut s, MARGIN - 0.3 * SP, y_top, y_bot, 1.0);
        }
    }

    // Part names, full on the first system, abbreviated after.
    for (i, st) in staves.iter().enumerate() {
        let name = if is_first_system {
            &st.name
        } else {
            &st.abbrev
        };
        text(
            &mut s,
            MARGIN - 0.6 * SP,
            rows[i].top + 2.2 * SP,
            11.0,
            "end",
            r#" class="staffname""#,
            &esc(name),
        );
    }

    // Bar-by-bar content.
    let mut x = MARGIN + head;
    let last_bar = bars.len() - 1;
    for k in first..end {
        let bar = &bars[k];
        let (bw_nat, cols) = &nat[k];
        let bw = bw_nat * scale;
        let mut col_x: Vec<(i64, f64)> = Vec::with_capacity(cols.len());
        {
            let mut ex = x + PAD_L * scale;
            for &(u, lo, w) in cols {
                col_x.push((u, ex + lo * scale));
                let bw_col = w * scale;
                let t_a = bar.t0 + (u as f64 / bar_u as f64) * (bar.t1 - bar.t0);
                let next_u = cols.iter().find(|c| c.0 > u).map_or(bar_u, |c| c.0);
                let t_b = bar.t0 + (next_u as f64 / bar_u as f64) * (bar.t1 - bar.t0);
                notes_out.push((
                    t_a,
                    t_b,
                    ex,
                    oy + rows[0].top - ROW_PAD - LABEL_ROOM,
                    bw_col,
                    sys_h,
                ));
                ex += bw_col;
            }
        }

        // Section label and chord symbols above the top staff (and the
        // guitar staff, if present, at the same units).
        if let Some(lb) = &bar.label {
            text(
                &mut s,
                x + 0.2 * SP,
                rows[0].top - ROW_PAD - 0.2 * SP,
                LABEL_PX,
                "start",
                r#" font-weight="bold" font-style="italic""#,
                &esc(lb),
            );
        }
        let guitar_row = staves.iter().position(|s| s.part == PartId::Guitar);
        for (u, cname) in &bar.chords {
            let cx = col_x.iter().find(|c| c.0 == *u).map_or(x, |c| c.1);
            for ri in std::iter::once(0).chain(guitar_row) {
                let _ = write!(
                    &mut s,
                    r#"<text class="chord" x="{cx:.1}" y="{:.1}" font-size="{CHORD_PX}" font-weight="bold">{}</text>"#,
                    rows[ri].top - 0.7 * SP,
                    chord_text(cname)
                );
            }
        }

        // Barlines through each bracketed group (and lone staves), and
        // through the whole system at the last bar.
        let bx = x + bw;
        for (_, gi, gj) in groups(staves) {
            let y_top = rows[gi].top;
            let y_bot = rows[gj - 1].top + 4.0 * SP;
            if k == last_bar {
                line(
                    &mut s,
                    bx - 0.75 * SP,
                    y_top,
                    bx - 0.75 * SP,
                    y_bot,
                    0.16 * SP,
                );
                let _ = write!(
                    &mut s,
                    r##"<rect x="{:.2}" y="{y_top:.2}" width="{:.2}" height="{:.2}" fill="#111"/>"##,
                    bx - 0.5 * SP,
                    0.5 * SP,
                    y_bot - y_top
                );
            } else {
                line(&mut s, bx, y_top, bx, y_bot, 0.16 * SP);
            }
        }

        // Each staff's cell.
        for (si, st) in staves.iter().enumerate() {
            let cell = &bar.cells[si];
            let n_voices = cell.voices.len();
            for (vi, voice) in cell.voices.iter().enumerate() {
                draw_voice(
                    &mut s,
                    voice,
                    &col_x,
                    st.clef,
                    n_voices,
                    vi,
                    rows[si].top,
                    sys_end - MARGIN,
                );
            }
        }
        x += bw;
    }

    s.push_str("</g>");
    body.push_str(&s);
    sys_h
}

/// One voice's events in one bar: rests, noteheads (or drawn heads),
/// stems, dots, ties, lyrics.
#[allow(clippy::too_many_arguments)]
fn draw_voice(
    s: &mut String,
    voice: &[Ev],
    col_x: &[(i64, f64)],
    clef: Clef,
    n_voices: usize,
    vi: usize,
    row_top: f64,
    sys_end_local: f64,
) {
    let x_of = |u: i64| col_x.iter().find(|c| c.0 == u).map_or(MARGIN, |c| c.1);
    let low_rest = n_voices > 1 && vi == 1;
    for (ei, e) in voice.iter().enumerate() {
        let x = x_of(e.s);
        let Some(chord) = &e.chord else {
            let (g, mut y) = rest_glyph(e.d);
            if low_rest {
                y += 1.5 * SP;
            }
            glyph(s, g, x + 0.2 * SP, row_top + y, 1.0);
            continue;
        };
        let steps: Vec<i32> = chord.heads.iter().map(|h| h.step).collect();
        let rep = *steps
            .iter()
            .min_by_key(|&&a| (a - 34).unsigned_abs())
            .unwrap_or(&34);
        let up = stem_up(n_voices, vi, rep);
        let offsets = chord_offsets(&chord.heads);
        let hw = if e.d >= 16 { WHOLE_W } else { HEAD_W };
        let stem_x = if up {
            x + hw - STEM_W * 0.5
        } else {
            x + STEM_W * 0.5
        };
        let (y_min, y_max) = (
            chord
                .heads
                .iter()
                .map(|h| y_step(clef, h.step))
                .fold(f64::INFINITY, f64::min),
            chord
                .heads
                .iter()
                .map(|h| y_step(clef, h.step))
                .fold(f64::NEG_INFINITY, f64::max),
        );
        let stem_end = if up {
            y_min - STEM_LEN
        } else {
            y_max + STEM_LEN
        };
        for (hi, h) in chord.heads.iter().enumerate() {
            let hx = x + offsets[hi];
            let hy = row_top + y_step(clef, h.step);
            let mut lp = 28;
            while lp >= h.step {
                line(
                    s,
                    hx - 0.4 * SP,
                    row_top + y_step(clef, lp),
                    hx + hw + 0.4 * SP,
                    row_top + y_step(clef, lp),
                    0.16 * SP,
                );
                lp -= 2;
            }
            let mut lp = 40;
            while lp <= h.step {
                line(
                    s,
                    hx - 0.4 * SP,
                    row_top + y_step(clef, lp),
                    hx + hw + 0.4 * SP,
                    row_top + y_step(clef, lp),
                    0.16 * SP,
                );
                lp += 2;
            }
            if let Some(a) = h.accidental {
                let g = match a {
                    1 => &glyphs::ACCIDENTAL_SHARP,
                    -1 => &glyphs::ACCIDENTAL_FLAT,
                    _ => &glyphs::ACCIDENTAL_NATURAL,
                };
                glyph(s, g, hx - glyph_w(g) - 0.3 * SP, hy, 1.0);
            }
            match notehead_glyph(h.notehead, e.d) {
                Some(g) => glyph(s, g, hx, hy, 1.0),
                None if h.notehead == Notehead::X => drawn::x_notehead(s, hx, hy, 1.0),
                None => drawn::slash_notehead(s, hx, hy, 1.0),
            }
            if matches!(e.d, 3 | 6 | 12) {
                let dy = if h.step % 2 == 0 { hy - 0.5 * SP } else { hy };
                glyph(s, &glyphs::AUGMENTATION_DOT, hx + hw + 0.35 * SP, dy, 1.0);
            }
        }
        if let Some(txt) = &chord.text {
            text(
                s,
                x + hw * 0.5,
                row_top + y_max + 2.4 * SP,
                10.0,
                "middle",
                "",
                &esc(txt),
            );
        }
        if e.d < 16 {
            line(
                s,
                stem_x,
                row_top + if up { y_min } else { y_max },
                stem_x,
                row_top + stem_end,
                STEM_W,
            );
            let flag_d = e.d;
            if flag_d < 4 {
                let g = match (flag_d == 1, up) {
                    (true, true) => &glyphs::FLAG16TH_UP,
                    (true, false) => &glyphs::FLAG16TH_DOWN,
                    (false, true) => &glyphs::FLAG8TH_UP,
                    (false, false) => &glyphs::FLAG8TH_DOWN,
                };
                glyph(s, g, stem_x - STEM_W * 0.5, row_top + stem_end, 1.0);
            }
        }
        if chord.tie_out {
            let x2 = voice
                .get(ei + 1)
                .map_or(x + hw + 2.0 * SP, |_| x + hw + 2.0 * SP)
                .min(MARGIN + sys_end_local);
            let x1 = x + hw + 0.15 * SP;
            let below = up;
            let yy = row_top + y_step(clef, rep) + if below { 0.6 * SP } else { -0.6 * SP };
            let c = if below { 1.1 * SP } else { -1.1 * SP };
            let xm = (x1 + x2) * 0.5;
            let _ = write!(
                s,
                r##"<path d="M{x1:.2} {yy:.2}Q{xm:.2} {:.2} {x2:.2} {yy:.2}Q{xm:.2} {:.2} {x1:.2} {yy:.2}Z" fill="#111"/>"##,
                yy + c,
                yy + c * 0.7
            );
        }
        let y_lyric = row_top + 4.0 * SP + ROW_PAD + 1.6 * SP;
        if chord.ext {
            // Melisma: an underscore in the lyric row from the note before
            // (or its lyric's end) to the end of this note.
            let a = match ei.checked_sub(1).and_then(|j| voice.get(j)) {
                Some(q) => {
                    let qw = if q.d >= 16 { WHOLE_W } else { HEAD_W };
                    let lyr = q.chord.as_ref().and_then(|c| c.lyric.as_deref());
                    x_of(q.s) + qw * 0.5 + lyr.map_or(0.0, |l| lyric_w(l) * 0.5 + 0.3 * SP)
                }
                None => x - 0.3 * SP,
            };
            let b = (x + hw + 0.2 * SP).min(MARGIN + sys_end_local);
            if b > a {
                line(s, a, y_lyric + 1.0, b, y_lyric + 1.0, 0.9);
            }
        }
        if chord.slur_out {
            let x1 = x + hw * 0.5;
            let x2 = voice
                .get(ei + 1)
                .map_or(x + hw + 2.0 * SP, |q| x_of(q.s) + HEAD_W * 0.5)
                .min(MARGIN + sys_end_local);
            let yy = row_top + y_step(clef, rep) + if up { SP } else { -SP };
            let c = if up { 1.6 * SP } else { -1.6 * SP };
            let xm = (x1 + x2) * 0.5;
            let _ = write!(
                s,
                r##"<path d="M{x1:.2} {yy:.2}Q{xm:.2} {:.2} {x2:.2} {yy:.2}Q{xm:.2} {:.2} {x1:.2} {yy:.2}Z" fill="#111"/>"##,
                yy + c,
                yy + c * 0.8
            );
        }
        if let Some(l) = &chord.lyric {
            text(
                s,
                x + hw * 0.5,
                row_top + 4.0 * SP + ROW_PAD + 1.6 * SP,
                LYRIC_PX,
                "middle",
                r#" class="lyric""#,
                &esc(l),
            );
            if chord.hyphen {
                let a = x + hw * 0.5 + lyric_w(l) * 0.5 + 0.3 * SP;
                let b = (a + 1.4 * SP).min(MARGIN + sys_end_local);
                if b > a {
                    line(
                        s,
                        a,
                        row_top + 4.0 * SP + ROW_PAD + 1.2 * SP,
                        b,
                        row_top + 4.0 * SP + ROW_PAD + 1.2 * SP,
                        0.9,
                    );
                }
            }
        }
    }
}

fn svg_document(width: f64, height: f64, body: &str) -> String {
    let mut svg = String::with_capacity(body.len() + 20_000);
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}" font-family="{}" fill="#111">"##,
        layout::FONT
    );
    let _ = write!(
        svg,
        r##"<rect width="{width:.0}" height="{height:.0}" fill="#fff"/><defs>"##
    );
    for g in glyphs::ALL {
        let _ = write!(svg, r#"<path id="g-{}" d="{}"/>"#, g.name, g.path);
    }
    svg.push_str("</defs>");
    svg.push_str(body);
    svg.push_str("</svg>\n");
    svg
}

/// Lays out the full score: one system per chunk, wrapped when too wide,
/// every staff of the score in every system.
pub(crate) fn layout_full(score: &FullScore) -> Page {
    let width = score.width;
    let bar_u = {
        let g = score.meter.grid();
        2 * g.sub as i64 * g.beats as i64
    };
    let nat: Vec<BarNat> = score
        .bars
        .iter()
        .map(|b| bar_columns(b, &score.staves, bar_u))
        .collect();

    let mut systems: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < score.bars.len() {
        let head = head_w(&score.staves, score.bars[i].fifths, systems.is_empty());
        let avail = width - 2.0 * MARGIN - head;
        let mut j = i;
        let mut w = 0.0;
        while j < score.bars.len()
            && score.bars[j].chunk == score.bars[i].chunk
            && score.bars[j].fifths == score.bars[i].fifths
            && (j == i || w + nat[j].0 <= avail)
        {
            w += nat[j].0;
            j += 1;
        }
        systems.push((i, j));
        i = j;
    }

    let mut body = String::new();
    let mut notes_out = Vec::new();
    let mut sys_out = Vec::new();
    let mut cursor = MARGIN + 44.0;
    text(
        &mut body,
        width * 0.5,
        MARGIN + 18.0,
        22.0,
        "middle",
        "",
        &esc(&score.title),
    );
    text(
        &mut body,
        width - MARGIN,
        MARGIN + 40.0,
        13.0,
        "end",
        "",
        &esc(&score.caption),
    );
    cursor += 1.0 * SP;

    for (si, &(first, end)) in systems.iter().enumerate() {
        let h = draw_system(
            &mut body,
            &mut notes_out,
            &mut sys_out,
            &score.staves,
            &score.bars,
            first,
            end,
            width,
            score.meter,
            si == 0,
            cursor,
            &nat,
            bar_u,
        );
        cursor += h + 2.0 * SP;
    }

    let height = (cursor + MARGIN * 0.5).ceil();
    Page {
        svg: svg_document(width, height, &body),
        width,
        height,
        notes: notes_out,
        systems: sys_out,
    }
}

/// Lays out one part's own view: bars flow and wrap to the page width (no
/// per-line chunks), silent runs already folded into `MultiRest` by
/// `FullScore::part`, drawn as a stretched `restHBar` with the bar count
/// in time-signature digits above it. Section labels are printed as
/// rehearsal marks (boxed letters are not attempted; the label text
/// itself, as the lead sheet prints it).
pub(crate) fn layout_part(part: &PartScore, meter: song::Meter, title: &str, width: f64) -> Page {
    let staves = std::slice::from_ref(&part.staff);
    let bar_u = {
        let g = meter.grid();
        2 * g.sub as i64 * g.beats as i64
    };

    // One synthetic bar per PartBar: a MultiRest becomes one wide column.
    struct PB {
        bar: BarCol,
        multi: Option<usize>,
    }
    let mut pbs = Vec::with_capacity(part.bars.len());
    for (chunk, pb) in part.bars.iter().enumerate() {
        match pb {
            PartBar::Bar {
                bar,
                sec,
                fifths,
                chunk: _,
                label,
                chords,
                t0,
                t1,
                cell,
            } => {
                pbs.push(PB {
                    bar: BarCol {
                        bar: *bar,
                        sec: *sec,
                        fifths: *fifths,
                        chunk,
                        label: label.clone(),
                        chords: chords.clone(),
                        t0: *t0,
                        t1: *t1,
                        cells: vec![cell.clone()],
                    },
                    multi: None,
                });
            }
            PartBar::MultiRest { bars, fifths } => {
                let rest_ev = Ev {
                    s: 0,
                    d: bar_u,
                    chord: None,
                };
                pbs.push(PB {
                    bar: BarCol {
                        bar: 0,
                        sec: 0,
                        fifths: *fifths,
                        chunk,
                        label: None,
                        chords: Vec::new(),
                        t0: 0.0,
                        t1: 0.0,
                        cells: vec![Cell {
                            voices: vec![vec![rest_ev]],
                        }],
                    },
                    multi: Some(*bars),
                });
            }
        }
    }
    let bars: Vec<BarCol> = pbs.iter().map(|p| p.bar.clone()).collect();
    let nat: Vec<BarNat> = bars
        .iter()
        .enumerate()
        .map(|(k, b)| match pbs[k].multi {
            Some(_) => (7.0 * SP, vec![(0, 0.0, 7.0 * SP - PAD_L - PAD_R)]),
            None => bar_columns(b, staves, bar_u),
        })
        .collect();

    // Wrap purely by width: as many bars as fit, ignoring chunk (part view
    // has no per-line chunking).
    let mut systems: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < bars.len() {
        let head = head_w(staves, bars[i].fifths, systems.is_empty());
        let avail = width - 2.0 * MARGIN - head;
        let mut j = i;
        let mut w = 0.0;
        // A key change starts a system, so the new signature is drawn.
        while j < bars.len()
            && bars[j].fifths == bars[i].fifths
            && (j == i || w + nat[j].0 <= avail)
        {
            w += nat[j].0;
            j += 1;
        }
        systems.push((i, j));
        i = j;
    }

    let mut body = String::new();
    let mut notes_out = Vec::new();
    let mut sys_out = Vec::new();
    let mut cursor = MARGIN + 44.0;
    text(
        &mut body,
        width * 0.5,
        MARGIN + 18.0,
        22.0,
        "middle",
        "",
        &esc(title),
    );
    cursor += SP;

    for (si, &(first, end)) in systems.iter().enumerate() {
        let h = draw_system(
            &mut body,
            &mut notes_out,
            &mut sys_out,
            staves,
            &bars,
            first,
            end,
            width,
            meter,
            si == 0,
            cursor,
            &nat,
            bar_u,
        );
        // Multi-bar rests in this system: overdraw the restHBar and count.
        for k in first..end {
            if let Some(n) = pbs[k].multi {
                let bw = nat[k].0.min(width) * 1.0;
                let x0 = MARGIN
                    + head_w(staves, bars[first].fifths, si == 0)
                    + nat[first..k].iter().map(|x| x.0).sum::<f64>();
                let row_top = staff_rows(staves).0[0].top;
                let mid = cursor + row_top + 2.0 * SP;
                let mut g = String::new();
                let gw = glyph_w(&glyphs::REST_HBAR);
                let scale = (bw * 0.7) / gw.max(1.0);
                glyph(&mut g, &glyphs::REST_HBAR, x0 + bw * 0.15, mid, scale);
                draw_multirest_count(&mut g, x0 + bw * 0.5, mid - 2.4 * SP, n);
                body.push_str(&g);
            }
        }
        cursor += h + 2.0 * SP;
    }

    let height = (cursor + MARGIN * 0.5).ceil();
    Page {
        svg: svg_document(width, height, &body),
        width,
        height,
        notes: notes_out,
        systems: sys_out,
    }
}

/// The bar count over a multi-bar rest, in time-signature digits.
fn draw_multirest_count(out: &mut String, cx: f64, y: f64, n: usize) {
    let digits: Vec<usize> = if n == 0 {
        vec![0]
    } else {
        n.to_string()
            .chars()
            .map(|c| c.to_digit(10).unwrap() as usize)
            .collect()
    };
    let widths: Vec<f64> = digits
        .iter()
        .map(|&d| glyph_w(&glyphs::TIME_SIG[d]) * 0.7)
        .collect();
    let total: f64 = widths.iter().sum();
    let mut x = cx - total * 0.5;
    for (&d, &w) in digits.iter().zip(&widths) {
        glyph(out, &glyphs::TIME_SIG[d], x, y, 0.7);
        x += w;
    }
}

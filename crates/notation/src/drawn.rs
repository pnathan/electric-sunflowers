//! SVG geometry for symbols the Bravura table (`glyphs.rs`) does not cover.
//!
//! No Bravura font file exists on this machine (`gen_glyphs.py` only ever
//! read `src/glyphs.js`, which never carried these), so every shape here is
//! hand-drawn, not traced from SMuFL outlines. If Bravura data for one of
//! these becomes available, add its SMuFL name to `gen_glyphs.py` and drop
//! the matching function here.
//!
//! Proportions follow the SMuFL spec where it gives one (the F clef is 2.6
//! staff spaces tall, matching `fClef`); the rest (percussion clef bar
//! width and gap, notehead stroke widths, the bracket hook, the brace
//! curve) are plain shapes sized by eye to sit in a 4-space staff, not
//! measurements of any real typeface.
//!
//! Every function takes an already-transformed `(x, y)` in the caller's
//! local SVG coordinates (the same convention as `crate::layout::glyph`)
//! and a `scale` in staff spaces (1.0 is normal size), and appends its
//! markup to `out`.

use std::fmt::Write as _;

use crate::layout::{HEAD_W, SP};

fn fill_path(out: &mut String, d: &str) {
    let _ = write!(out, r##"<path d="{d}" fill="#111"/>"##);
}

/// F clef (bass clef): a filled curl 2.6 staff spaces tall (Bravura
/// `fClef`'s own height) with its two dots either side of the F line.
/// `(x, y)` is the curl's left edge and the F line's own y (the line the
/// clef marks; for a plain `Bass` clef this is F3).
pub fn f_clef(out: &mut String, x: f64, y: f64, scale: f64) {
    let s = SP * scale;
    // A thick hook, drawn as ten points around a closed loop: starts above
    // the F line, bows out to the right and back to make the curl's ball
    // just below the line, then a thinner return path back to the start.
    let pts: [(f64, f64); 10] = [
        (x, y - 1.3 * s),
        (x + 1.3 * s, y - 1.3 * s),
        (x + 1.3 * s, y - 0.35 * s),
        (x + 1.3 * s, y + 0.35 * s),
        (x + 0.75 * s, y + 0.85 * s),
        (x + 0.3 * s, y + 0.85 * s),
        (x - 0.1 * s, y + 0.85 * s),
        (x - 0.5 * s, y + 0.55 * s),
        (x - 0.5 * s, y + 0.05 * s),
        (x - 0.5 * s, y - 0.35 * s),
    ];
    let mut d = format!("M{:.2} {:.2}", pts[0].0, pts[0].1);
    for chunk in pts[1..].chunks(3) {
        if chunk.len() == 3 {
            let _ = write!(
                d,
                "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
                chunk[0].0, chunk[0].1, chunk[1].0, chunk[1].1, chunk[2].0, chunk[2].1
            );
        }
    }
    let _ = write!(
        d,
        "C{:.2} {:.2} {:.2} {:.2} {:.2} {:.2}Z",
        x - 0.15 * s,
        y - 0.55 * s,
        x + 0.35 * s,
        y - 0.95 * s,
        x,
        y - 1.3 * s
    );
    fill_path(out, &d);
    let r = 0.16 * s;
    let cx = x + 1.55 * s;
    let _ = write!(
        out,
        r##"<circle cx="{cx:.2}" cy="{:.2}" r="{r:.2}" fill="#111"/>"##,
        y - 0.45 * s
    );
    let _ = write!(
        out,
        r##"<circle cx="{cx:.2}" cy="{:.2}" r="{r:.2}" fill="#111"/>"##,
        y + 0.45 * s
    );
}

/// The small "8" under (or, reused, over) a clef, for an octave-transposed
/// staff (`Bass8vb`; `Treble8vb` already has Bravura's own `gClef8vb`,
/// which needs no help from this module).
pub fn clef_8(out: &mut String, x: f64, y: f64, px: f64) {
    let _ = write!(
        out,
        r#"<text x="{x:.1}" y="{y:.1}" font-size="{px:.1}" text-anchor="middle" font-style="italic">8</text>"#
    );
}

/// Percussion clef: two thick bars, 1 staff space tall and 0.25 space
/// thick, 0.5 space apart, centred on the staff's middle line. `(x, y)` is
/// the left bar's left edge and the middle line's y.
pub fn percussion_clef(out: &mut String, x: f64, y: f64, scale: f64) {
    let s = SP * scale;
    let w = 0.25 * s;
    let h = 1.0 * s;
    let gap = 0.5 * s;
    for i in 0..2 {
        let bx = x + i as f64 * (w + gap);
        let _ = write!(
            out,
            r##"<rect x="{bx:.2}" y="{:.2}" width="{w:.2}" height="{h:.2}" fill="#111"/>"##,
            y - h * 0.5
        );
    }
}

/// An x notehead: two crossed strokes inside the black notehead's box.
/// `(x, y)` is the box's left edge and centre line, as `crate::layout::glyph`
/// places a normal notehead.
pub fn x_notehead(out: &mut String, x: f64, y: f64, scale: f64) {
    let w = HEAD_W * scale;
    let h = 0.9 * SP * scale;
    let t = 0.16 * SP * scale;
    for (y1, y2) in [(y - h * 0.5, y + h * 0.5), (y + h * 0.5, y - h * 0.5)] {
        let _ = write!(
            out,
            r##"<line x1="{:.2}" y1="{y1:.2}" x2="{:.2}" y2="{y2:.2}" stroke="#111" stroke-width="{t:.2}" stroke-linecap="round"/>"##,
            x + 0.08 * w,
            x + w - 0.08 * w
        );
    }
}

/// A slash notehead (drum swish): a slanted parallelogram in the same box
/// a normal notehead would fill.
pub fn slash_notehead(out: &mut String, x: f64, y: f64, scale: f64) {
    let w = HEAD_W * scale;
    let h = 0.6 * SP * scale;
    let d = format!(
        "M{x0:.2} {y0:.2}L{x1:.2} {y1:.2}L{x2:.2} {y1:.2}L{x3:.2} {y0:.2}Z",
        x0 = x,
        y0 = y + h,
        x1 = x + w * 0.55,
        y1 = y - h,
        x2 = x + w,
        x3 = x + w * 0.45,
    );
    fill_path(out, &d);
}

/// A system bracket: a thick 0.5-space bar from `y_top` to `y_bot`, with
/// small hooked ends, at `x` (its right edge; it extends to the left of
/// the staves it brackets).
pub fn bracket(out: &mut String, x: f64, y_top: f64, y_bot: f64, scale: f64) {
    let w = 0.5 * SP * scale;
    let hook = 0.7 * SP * scale;
    let _ = write!(
        out,
        r##"<rect x="{:.2}" y="{y_top:.2}" width="{w:.2}" height="{:.2}" fill="#111"/>"##,
        x - w,
        y_bot - y_top
    );
    for (y, dir) in [(y_top, -1.0), (y_bot, 1.0)] {
        let d = format!(
            "M{x0:.2} {y:.2}C{x1:.2} {y:.2} {x0:.2} {y1:.2} {x0:.2} {y1:.2}L{x0:.2} {y:.2}Z",
            x0 = x - w,
            x1 = x - w - hook,
            y1 = y + dir * hook * 0.6,
        );
        fill_path(out, &d);
    }
}

/// A brace (grand staff, or the harp's two staves): a filled path of two
/// mirrored cubic curves from `y_top` to `y_bot`, bulging left at the
/// middle, at `x` (its right edge).
pub fn brace(out: &mut String, x: f64, y_top: f64, y_bot: f64, scale: f64) {
    let mid = (y_top + y_bot) * 0.5;
    let bulge = 0.9 * SP * scale;
    let thick = 0.3 * SP * scale;
    let d = format!(
        "M{x0:.2} {yt:.2}\
         C{xo:.2} {y1:.2} {xo:.2} {y2:.2} {x0:.2} {ym:.2}\
         C{xo:.2} {y3:.2} {xo:.2} {y4:.2} {x0:.2} {yb:.2}\
         C{xi:.2} {y4:.2} {xi:.2} {y3:.2} {xi:.2} {ym:.2}\
         C{xi:.2} {y2:.2} {xi:.2} {y1:.2} {x0:.2} {yt:.2}Z",
        x0 = x,
        yt = y_top,
        xo = x - bulge,
        y1 = y_top + (mid - y_top) * 0.5,
        y2 = mid - (mid - y_top) * 0.1,
        ym = mid,
        y3 = mid + (y_bot - mid) * 0.1,
        y4 = y_bot - (y_bot - mid) * 0.5,
        yb = y_bot,
        xi = x - bulge + thick,
    );
    fill_path(out, &d);
}

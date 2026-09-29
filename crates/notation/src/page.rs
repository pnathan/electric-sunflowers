//! The kind-agnostic page: `Sheet` wraps whichever thing is being shown
//! (the lead sheet, the full score, or one part's own view) and lays it
//! out the same way, so a viewer needs one code path for all three. See
//! `docs/features-2.md` section 6.4.

use crate::full::{FullScore, PartId};
use crate::{Score, TimedBox};

/// A laid-out page: px at the SVG's own width and height, with every note
/// group's and every system's time span in seconds, on the page.
#[derive(Clone, Debug)]
pub struct Page {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// For the lead sheet, one box per engraved note (a tied note's parts
    /// separately). For the full score, one box per onset column of a
    /// system, spanning every staff. For a part view, one box per note or
    /// rest event of that one staff.
    pub notes: Vec<TimedBox>,
    /// One box per system, from its first bar's start to its last bar's
    /// end.
    pub systems: Vec<TimedBox>,
}

/// What a page shows: the lead sheet, the full multipart score, or one
/// part's own view of it.
#[derive(Clone, Debug)]
pub enum Sheet {
    Lead(Score),
    Full(FullScore),
    Part(FullScore, PartId),
}

/// An empty page (a part asked for is not in the score): no crash, one
/// line of text saying so.
fn empty_page(width: f64, title: &str, msg: &str) -> Page {
    let height = 120.0;
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}" font-family="sans-serif" fill="#111"><rect width="{width:.0}" height="{height:.0}" fill="#fff"/><text x="24" y="30" font-size="16">{}</text><text x="24" y="56" font-size="13">{}</text></svg>"##,
        crate::layout::esc(title),
        crate::layout::esc(msg)
    );
    Page { svg, width, height, notes: Vec::new(), systems: Vec::new() }
}

impl Sheet {
    /// Lays out this sheet for a page `width` px wide.
    pub fn page(&self, width: f64) -> Page {
        match self {
            Sheet::Lead(score) => {
                let p = crate::layout::layout(&score.clone().with_width(width));
                Page { svg: p.svg, width: p.width, height: p.height, notes: p.notes, systems: p.systems }
            }
            Sheet::Full(full) => {
                let p = crate::full::layout::layout_full(&full.clone().with_width(width));
                Page { svg: p.svg, width: p.width, height: p.height, notes: p.notes, systems: p.systems }
            }
            Sheet::Part(full, part) => match full.part(*part) {
                Some(ps) => {
                    let p = crate::full::layout::layout_part(&ps, full.meter, full.fifths, &full.title, width.max(300.0));
                    Page { svg: p.svg, width: p.width, height: p.height, notes: p.notes, systems: p.systems }
                }
                None => empty_page(width.max(300.0), &full.title, "this part does not play in this song"),
            },
        }
    }

    /// The song's title.
    pub fn title(&self) -> &str {
        match self {
            Sheet::Lead(score) => &score.title,
            Sheet::Full(full) | Sheet::Part(full, _) => &full.title,
        }
    }
}

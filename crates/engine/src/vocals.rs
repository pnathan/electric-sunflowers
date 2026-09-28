//! The vocal tracks: each singer of `arrange::Vocals` rendered phrase by
//! phrase (`voice::render_phrases`, `VoiceSettings::from(&singer.style)`)
//! straight into a sparse stem; only the phrase scratch is dense.
//!
//! - Lead and harmony: mono, unit gain.
//! - Doubles and choir singers: stereo, equal-power pan (`dsp::pan`).
//!
//! A singer's `offset` shifts its notes before rendering. Singer k draws
//! from the stream seed `Rng::event(seed, SINGER, k).next_u32()` widened to
//! u64: lead 0, harmony 1, doubles 2 and 3, choir 16 + 16 part + i.

use dsp::pan::equal_power;
use sfcore::random::{tag, Rng, Tag};
use song::events::{Singer, VocalNote};
use voice::{render_phrases, VoiceSettings};

use crate::stem::SparseBuf;

/// Per-singer seed stream.
const SINGER: Tag = tag("voice.singer");

pub const LEAD: u64 = 0;
pub const HARMONY: u64 = 1;
pub const DOUBLES: u64 = 2;
pub const CHOIR: u64 = 16;

/// Seed of singer `index` (see the module doc).
pub fn singer_seed(seed: u64, index: u64) -> u64 {
    Rng::event(seed, SINGER, index).next_u32() as u64
}

/// Seed index of choir part `part`, singer `i`.
pub fn choir_index(part: usize, i: usize) -> u64 {
    CHOIR + 16 * part as u64 + i as u64
}

/// Renders `singer` and hands each phrase to `emit(start, samples)`.
fn render_singer(singer: &Singer, seed: u64, len: usize, emit: impl FnMut(usize, &[f32])) {
    let settings = VoiceSettings::from(&singer.style);
    if singer.offset == 0.0 {
        render_phrases(&singer.notes, singer.voice, &settings, seed, len, emit);
    } else {
        let shifted: Vec<VocalNote> =
            singer.notes.iter().map(|n| VocalNote { t0: n.t0 + singer.offset, t1: n.t1 + singer.offset, ..n.clone() }).collect();
        render_phrases(&shifted, singer.voice, &settings, seed, len, emit);
    }
}

/// `singer` into a mono stem channel at unit gain.
pub fn mono_into(singer: &Singer, seed: u64, out: &mut SparseBuf) {
    let len = out.len();
    render_singer(singer, seed, len, |start, x| out.add_at(start as isize, x, 1.0));
}

/// `singer` panned (equal power at `singer.pan`) into a stereo stem.
pub fn panned_into(singer: &Singer, seed: u64, out: &mut [SparseBuf; 2]) {
    let len = out[0].len();
    let [gl, gr] = equal_power(singer.pan as f64);
    let [l, r] = out;
    render_singer(singer, seed, len, |start, x| {
        l.add_at(start as isize, x, gl);
        r.add_at(start as isize, x, gr);
    });
}

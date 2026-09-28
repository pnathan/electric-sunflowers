//! The band tracks: each part of `arrange::Arrangement` rendered by its
//! instrument (`instruments`), convolved with its body if the strip has
//! one, and moved into a sparse stem.
//!
//! - Guitar: `guitar::render_strings` with `PluckParams::GUITAR`, then the
//!   `Sympathetic` open strings at `guitar::TUNING.sympathetic`.
//! - Harmony guitar: lead and fill notes with `PluckParams::HG_LEAD`,
//!   arpeggio notes with `PluckParams::HG_ARP`, summed.
//! - Bass: `guitar::render_bass` (dark pluck plus sine sub layer), mono.
//! - Harp: `PluckParams::HARP`. Violin: `violin::render_violin`.
//! - Drums: each hit rendered by `drums::render_hit` into a hit-length
//!   scratch and added to the stereo stem; no drum stem without a kit.
//!
//! Bodies: `Body::impulse_response` from `Rng::stream(seed + offset, BODY)`
//! (the level trim is inside `Body`), applied by
//! `dsp::conv::convolve_mono_to_stereo` in `CONV_CHUNK` pieces (overlap-add,
//! parallel inside the task, identical at any thread count). A silent track
//! yields no stem.
//! The instruments separate their random streams by their own tags.

use dsp::conv::convolve_mono_to_stereo;
use instruments::drums::{hit_len, render_hit, DrumScratch, DRUM_HIT};
use instruments::guitar::{render_bass, render_plucks, render_strings, Sympathetic, TUNING};
use instruments::pluck::PluckParams;
use instruments::violin::render_violin;
use sfcore::random::{tag, Rng, Tag};
use sfcore::time::sample_at;
use song::events::DrumHit;

use arrange::Arrangement;

use crate::stem::{SparseBuf, Stem};
use crate::track::{BodyMount, TrackId};

/// Stream for the body impulse responses.
const BODY: Tag = tag("body");
/// Note streams of the plucked parts.
const HARP_NOTE: Tag = tag("harp.note");
const HG_LEAD_NOTE: Tag = tag("hg.lead.note");
const HG_ARP_NOTE: Tag = tag("hg.arp.note");

/// Renders band track `id` of `arr` over `len` samples, body applied.
/// `None` for a vocal id, a silent track or a song without drums.
pub fn render(id: TrackId, arr: &Arrangement, seed: u64, len: usize) -> Option<Stem> {
    let mono = match id {
        TrackId::Guitar => {
            let mut out = render_strings(&arr.guitar, &PluckParams::GUITAR, seed, len);
            Sympathetic::new(TUNING.sympathetic).process(&mut out);
            out
        }
        TrackId::HarmonyGuitar => {
            let hg = &arr.harmony_guitar;
            let mut out = render_plucks(&hg.lead, &PluckParams::HG_LEAD, seed, HG_LEAD_NOTE, len);
            if !hg.arp.is_empty() {
                let arp = render_plucks(&hg.arp, &PluckParams::HG_ARP, seed, HG_ARP_NOTE, len);
                for (o, a) in out.iter_mut().zip(&arp) {
                    *o += a;
                }
            }
            out
        }
        TrackId::Bass => render_bass(&arr.bass, seed, len),
        TrackId::Harp => render_plucks(&arr.harp, &PluckParams::HARP, seed, HARP_NOTE, len),
        TrackId::Violin => render_violin(&arr.violin, len, seed),
        TrackId::Drums => return drums(arr.drums.as_deref()?, seed, len),
        TrackId::Lead | TrackId::Doubles | TrackId::Harmony | TrackId::Choir => return None,
    };
    if mono.iter().all(|&v| v == 0.0) {
        return None;
    }
    Some(match id.strip().body {
        Some(mount) => Stem::Stereo(apply_body(mount, seed, len, &mono)),
        None => Stem::Mono(SparseBuf::from_dense(&mono)),
    })
}

/// Input samples per convolution call. Bounds the dense output buffers of
/// one call (2 x 4 MB) instead of two whole-song channels.
pub const CONV_CHUNK: usize = 1 << 20;

/// Convolves mono `x` with `mount`'s body into a stereo stem of `len`
/// samples. The input is cut into `CONV_CHUNK` pieces; each piece's full
/// convolution (piece plus IR tail) is added at its offset, which by
/// linearity equals the whole convolution. All-zero pieces are skipped.
pub fn apply_body(mount: BodyMount, seed: u64, len: usize, x: &[f32]) -> [SparseBuf; 2] {
    let mut rng = Rng::stream(seed.wrapping_add(mount.seed_offset), BODY);
    let ir = mount.body.impulse_response(&mut rng);
    let mut out = [SparseBuf::new(len), SparseBuf::new(len)];
    let x = &x[..x.len().min(len)];
    for (c, seg) in x.chunks(CONV_CHUNK).enumerate() {
        if seg.iter().all(|&v| v == 0.0) {
            continue;
        }
        let start = c * CONV_CHUNK;
        let out_len = (seg.len() + ir.len() - 1).min(len - start);
        let [l, r] = convolve_mono_to_stereo(seg, &ir, out_len);
        out[0].add_at(start as isize, &l, 1.0);
        out[1].add_at(start as isize, &r, 1.0);
    }
    out
}

/// The drum hits into a stereo stem. Hit k draws from
/// `Rng::event(seed, DRUM_HIT, k)`; each hit is rendered at time 0 into a
/// hit-length scratch pair, then added at `sample_at(hit.t)`.
fn drums(hits: &[DrumHit], seed: u64, len: usize) -> Option<Stem> {
    let mut stem = [SparseBuf::new(len), SparseBuf::new(len)];
    let mut scratch = DrumScratch::new();
    let (mut l, mut r) = (Vec::new(), Vec::new());
    for (k, hit) in hits.iter().enumerate() {
        if !hit.t.is_finite() {
            continue;
        }
        let n = hit_len(hit.kind);
        l.clear();
        l.resize(n, 0.0f32);
        r.clear();
        r.resize(n, 0.0f32);
        let mut rng = Rng::event(seed, DRUM_HIT, k as u64);
        render_hit(&DrumHit { t: 0.0, ..*hit }, &mut rng, &mut l, &mut r, &mut scratch);
        let at = sample_at(hit.t);
        stem[0].add_at(at, &l, 1.0);
        stem[1].add_at(at, &r, 1.0);
    }
    let stem = Stem::Stereo(stem);
    (!stem.is_silent()).then_some(stem)
}

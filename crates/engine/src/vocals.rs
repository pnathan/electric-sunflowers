//! The vocal tracks: the singers of `arrange::Vocals` rendered by
//! `voice::render_phrases` with `VoiceSettings::from(&singer.style)`.
//!
//! - Lead and harmony: mono tracks.
//! - Doubles: both singers panned (equal power) into one stereo track.
//! - Choir: every singer panned into one stereo track, summed in plan order
//!   (part low to high, singer within the part).
//!
//! A singer's `offset` shifts its notes before rendering. Singer k draws
//! from the stream seed `Rng::event(seed, SINGER, k).next_u32()` widened to
//! u64: lead 0, harmony 1, doubles 2 and 3, choir 16 + 16 part + i.

use dsp::pan::{add_mono, equal_power};
use rayon::prelude::*;
use sfcore::random::{tag, Rng, Tag};
use song::events::{Singer, VocalNote};
use voice::{render_phrases, VoiceSettings};

use arrange::Vocals;

/// Per-singer seed stream.
const SINGER: Tag = tag("voice.singer");

/// Singer indices for the seed stream.
const LEAD: u64 = 0;
const HARMONY: u64 = 1;
const DOUBLES: u64 = 2;
const CHOIR: u64 = 16;

/// Choir singers rendered at once on the threaded path; bounds the
/// full-length buffers alive at one time.
const CHOIR_BATCH: usize = 4;

fn singer_seed(seed: u64, index: u64) -> u64 {
    Rng::event(seed, SINGER, index).next_u32() as u64
}

/// Renders `singer` and hands each rendered phrase to `emit(start, samples)`.
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

/// `singer` into a new mono buffer of `len` samples.
fn mono(singer: &Singer, seed: u64, len: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; len];
    render_singer(singer, seed, len, |start, x| {
        if start < len {
            let n = x.len().min(len - start);
            for (o, v) in out[start..start + n].iter_mut().zip(x) {
                *o += v;
            }
        }
    });
    out
}

/// `singer` panned into `l`/`r`.
fn pan_into(singer: &Singer, seed: u64, l: &mut [f32], r: &mut [f32]) {
    let len = l.len();
    let gains = equal_power(singer.pan as f64);
    render_singer(singer, seed, len, |start, x| add_mono(l, r, start as isize, x, gains));
}

/// The lead vocal, mono.
pub fn lead(v: &Vocals, seed: u64, len: usize) -> Vec<f32> {
    mono(&v.lead, singer_seed(seed, LEAD), len)
}

/// The harmony vocal, mono (silent without harmony notes).
pub fn harmony(v: &Vocals, seed: u64, len: usize) -> Vec<f32> {
    mono(&v.harmony, singer_seed(seed, HARMONY), len)
}

/// The two doubles, stereo.
pub fn doubles(v: &Vocals, seed: u64, len: usize) -> [Vec<f32>; 2] {
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    for (k, s) in v.doubles.iter().enumerate() {
        pan_into(s, singer_seed(seed, DOUBLES + k as u64), &mut l, &mut r);
    }
    [l, r]
}

/// `singer`'s phrases as (start sample, samples), in render order.
fn phrases(singer: &Singer, seed: u64, len: usize) -> Vec<(usize, Vec<f32>)> {
    let mut out = Vec::new();
    render_singer(singer, seed, len, |start, x| out.push((start, x.to_vec())));
    out
}

/// The choir, stereo. `threaded` renders `CHOIR_BATCH` singers at a time in
/// parallel and keeps each singer's phrases; the sequential path pans each
/// phrase as it is rendered. Both add the same phrases with the same gains
/// in plan order, so both give the same samples.
pub fn choir(v: &Vocals, seed: u64, len: usize, threaded: bool) -> [Vec<f32>; 2] {
    let singers: Vec<(&Singer, u64)> = v
        .choir
        .iter()
        .enumerate()
        .flat_map(|(p, part)| part.iter().enumerate().map(move |(i, s)| (s, CHOIR + 16 * p as u64 + i as u64)))
        .map(|(s, k)| (s, singer_seed(seed, k)))
        .collect();
    let mut l = vec![0.0f32; len];
    let mut r = vec![0.0f32; len];
    if threaded {
        for batch in singers.chunks(CHOIR_BATCH) {
            let rendered: Vec<Vec<(usize, Vec<f32>)>> = batch.par_iter().map(|&(s, k)| phrases(s, k, len)).collect();
            for (&(s, _), ph) in batch.iter().zip(&rendered) {
                let gains = equal_power(s.pan as f64);
                for (start, x) in ph {
                    add_mono(&mut l, &mut r, *start as isize, x, gains);
                }
            }
        }
    } else {
        for &(s, k) in &singers {
            pan_into(s, k, &mut l, &mut r);
        }
    }
    [l, r]
}

//! The band tracks: each part of `arrange::Arrangement` rendered by its
//! instrument (`instruments`), then the bodied tracks convolved with their
//! body impulse response.
//!
//! - Guitar: `instruments::guitar::render_strings` with
//!   `PluckParams::GUITAR`, then `Sympathetic` open strings at
//!   `guitar::TUNING.sympathetic`. Body: guitar.
//! - Harmony guitar: lead and fill notes with `PluckParams::HG_LEAD`,
//!   arpeggio notes with `PluckParams::HG_ARP`. Body: guitar, seed offset 17.
//! - Bass: `instruments::guitar::render_bass` (pluck plus sine sub layer).
//! - Harp: `PluckParams::HARP`. Body: harp.
//! - Violin: `instruments::violin::render_violin`. Body: violin.
//! - Drums: `instruments::drums::render_drums`, stereo; no track for a song
//!   without drums.
//!
//! Bodies: `Body::impulse_response` from `Rng::stream(seed + offset, BODY)`
//! (the level trim is inside `Body`), applied by
//! `dsp::conv::convolve_mono_to_stereo`. A silent track keeps its mono
//! buffer and skips the convolution.
//!
//! Instrument seeds: each track's instrument draws from the song seed; the
//! instruments separate their streams by their own tags.

use dsp::conv::convolve_mono_to_stereo;
use instruments::body::Body;
use instruments::drums::render_drums;
use instruments::guitar::{render_bass, render_plucks, render_strings, Sympathetic, TUNING};
use instruments::pluck::PluckParams;
use instruments::violin::render_violin;
use sfcore::random::{tag, Rng, Tag};

use arrange::Arrangement;

/// Stream for the body impulse responses.
const BODY: Tag = tag("body");
/// Note streams of the plucked parts.
const HARP_NOTE: Tag = tag("harp.note");
const HG_LEAD_NOTE: Tag = tag("hg.lead.note");
const HG_ARP_NOTE: Tag = tag("hg.arp.note");

/// A track's body: which body and the seed offset of its impulse response.
#[derive(Clone, Copy, Debug)]
pub struct BodyMount {
    pub body: Body,
    pub seed_offset: u64,
}

pub const GUITAR_BODY: BodyMount = BodyMount { body: Body::Guitar, seed_offset: 0 };
pub const HG_BODY: BodyMount = BodyMount { body: Body::Guitar, seed_offset: 17 };
pub const HARP_BODY: BodyMount = BodyMount { body: Body::Harp, seed_offset: 0 };
pub const VIOLIN_BODY: BodyMount = BodyMount { body: Body::Violin, seed_offset: 0 };

/// A band track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandTrack {
    Guitar,
    HarmonyGuitar,
    Bass,
    Drums,
    Harp,
    Violin,
}

impl BandTrack {
    /// Every band track, in mix order.
    pub const ALL: [BandTrack; 6] =
        [BandTrack::Guitar, BandTrack::HarmonyGuitar, BandTrack::Bass, BandTrack::Drums, BandTrack::Harp, BandTrack::Violin];

    /// The body the track is played through, if any.
    pub const fn body(self) -> Option<BodyMount> {
        match self {
            BandTrack::Guitar => Some(GUITAR_BODY),
            BandTrack::HarmonyGuitar => Some(HG_BODY),
            BandTrack::Harp => Some(HARP_BODY),
            BandTrack::Violin => Some(VIOLIN_BODY),
            BandTrack::Bass | BandTrack::Drums => None,
        }
    }
}

/// Renders `track` of `arr` into `len` samples with its body applied: one
/// channel (bass; a silent bodied track) or two. `None` for a song with no
/// drums.
pub fn render(track: BandTrack, arr: &Arrangement, seed: u64, len: usize) -> Option<Vec<Vec<f32>>> {
    let mono = match track {
        BandTrack::Guitar => {
            let mut out = render_strings(&arr.guitar, &PluckParams::GUITAR, seed, len);
            Sympathetic::new(TUNING.sympathetic).process(&mut out);
            out
        }
        BandTrack::HarmonyGuitar => {
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
        BandTrack::Bass => render_bass(&arr.bass, seed, len),
        BandTrack::Harp => render_plucks(&arr.harp, &PluckParams::HARP, seed, HARP_NOTE, len),
        BandTrack::Violin => render_violin(&arr.violin, len, seed),
        BandTrack::Drums => {
            let [l, r] = render_drums(arr.drums.as_deref()?, seed, len);
            return Some(vec![l, r]);
        }
    };
    Some(match track.body().and_then(|b| apply_body(b, seed, len, &mono)) {
        Some([l, r]) => vec![l, r],
        None => vec![mono],
    })
}

/// Convolves the mono track `x` with `mount`'s body and returns the stereo
/// track of `len` samples, or `None` when `x` is silent.
pub fn apply_body(mount: BodyMount, seed: u64, len: usize, x: &[f32]) -> Option<[Vec<f32>; 2]> {
    if x.iter().all(|&v| v == 0.0) {
        return None;
    }
    let mut rng = Rng::stream(seed.wrapping_add(mount.seed_offset), BODY);
    let ir = mount.body.impulse_response(&mut rng);
    Some(convolve_mono_to_stereo(x, &ir, len))
}

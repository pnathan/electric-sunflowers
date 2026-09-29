//! Stem printing (design section 3.3): one track as it enters the main bus
//! (post fader, post pan, ducked, its slapback included, dry), or the FDN
//! reverb return of every audible track's send bus, each times the mix's
//! final normalisation gain (`mix_gain`). Printed one track at a time, so
//! peak memory is one stereo stem plus the duck track, not every stem at
//! once.
//!
//! The bus compressor is not applied to a printed stem or the printed
//! reverb: the sum of every printed stem plus the printed reverb equals
//! `engine::mix::premix` (the pre-compressor mix) times `gain`, not the
//! shipped (compressed, then peak-normalised) mix. `tests/mixer.rs` checks
//! this sum for the demo song.

use dsp::reverb::{Fdn8, T60_DC, T60_NYQ};
use sfcore::SR_F;
use song::Band;

use crate::mix::{self, MIX_BLOCK, WET};
use crate::mixset::MixSettings;
use crate::render::Stems;
use crate::track::TrackId;
use crate::Stereo;

/// The full mix's final peak-normalisation factor (`PEAK / peak` after the
/// bus compressor): the `gain` every `print_stem` and `print_reverb` call
/// for this render should use, so the printed stems sum to the shipped
/// mix's level.
pub fn mix_gain(stems: &Stems, band: &Band, seed: u64, settings: &MixSettings) -> f32 {
    mix::compressed(stems, band, seed, settings).1 as f32
}

/// One track as it enters the main bus: post fader, post pan, ducked when
/// `duck` is given and the track is not a lead, its slapback included, dry
/// (no reverb, no bus compressor), times `gain`. `duck` is `mix::duck_gains`
/// for this render, computed once and shared across every `print_stem`
/// call. `None` when `id` does not sound: muted, soloed out, off in `band`,
/// or absent from the stems.
pub fn print_stem(
    stems: &Stems,
    band: &Band,
    _seed: u64,
    settings: &MixSettings,
    id: TrackId,
    duck: Option<&[f32]>,
    gain: f32,
) -> Option<Stereo> {
    if !(id.plays(band) && settings.audible(id)) || stems.get(id).is_none() {
        return None;
    }
    let routes = mix::routes(stems, band, settings);
    let slaps = mix::slap_sources(stems, band, settings);
    let mut out = Stereo {
        l: vec![0.0; stems.len],
        r: vec![0.0; stems.len],
    };
    mix::add_track_dry(&mut out, &routes, id, duck);
    mix::add_track_slap(&mut out, &slaps, id);
    for v in out.l.iter_mut().chain(out.r.iter_mut()) {
        *v *= gain;
    }
    Some(out)
}

/// The FDN reverb return of the send bus of every audible track (dry
/// stems' sends plus their slapback's send, exactly as `mix::premix` builds
/// it), times `gain`. `duck` is `mix::duck_gains` for this render.
pub fn print_reverb(
    stems: &Stems,
    band: &Band,
    seed: u64,
    settings: &MixSettings,
    duck: Option<&[f32]>,
    gain: f32,
) -> Stereo {
    let routes = mix::routes(stems, band, settings);
    let slaps = mix::slap_sources(stems, band, settings);
    let len = stems.len;
    let mut sl = vec![0.0f32; len];
    let mut sr = vec![0.0f32; len];
    mix::add_send_bus(&mut sl, &mut sr, &routes, duck);
    mix::add_send_slap(&mut sl, &mut sr, &slaps);
    let mut fdn = Fdn8::new(SR_F, seed, T60_DC, T60_NYQ);
    let mut out = Stereo {
        l: vec![0.0; len],
        r: vec![0.0; len],
    };
    for s in (0..len).step_by(MIX_BLOCK) {
        let n = MIX_BLOCK.min(len - s);
        fdn.process_block(
            [&sl[s..s + n], &sr[s..s + n]],
            [&mut out.l[s..s + n], &mut out.r[s..s + n]],
            WET,
        );
    }
    for v in out.l.iter_mut().chain(out.r.iter_mut()) {
        *v *= gain;
    }
    out
}

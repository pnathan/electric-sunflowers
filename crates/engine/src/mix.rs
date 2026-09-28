//! The mixer (design section 5.13): a streamed block mixer over the cached
//! stems, then the bus compressor and peak normalisation, parameterised by
//! `MixSettings` (design section 3.2-3.3).
//!
//! Per 1024-frame block: clear the main and send buses; add every audible
//! stem the band enables, in `TrackId` order, with its pan gains times
//! strip gain times fader times level (mono: equal-power law; stereo:
//! balance law; `dsp::pan`), and the same times the strip's send into the
//! send bus; add each track's slapback to both buses at its fader gain,
//! unpanned; run the send bus through the FDN reverb (`dsp::reverb::Fdn8`,
//! Jot and Chaigne 1991; T60 2.2 s at DC, 0.8 s at Nyquist) at wet 0.55 into
//! the main bus; write L/R; accumulate the bus power over every frame with
//! |L| + |R| above -80 dB.
//!
//! Then the bus compressor (`dsp::dynamics`, Giannoulis, Massberg, Reiss
//! 2012): 2.2:1, attack 20 ms, release 300 ms, 10 dB soft knee, threshold
//! 5 dB over the bus RMS, stereo-linked on max(|L|, |R|). Last, the sample
//! peak is normalised to 0.89.
//!
//! With `MixSettings::default_for`, every fader multiplies by exactly 1.0
//! (`db_to_gain(0.0)` is `1.0` exactly) and every pan equals the strip pan,
//! so `mix` reproduces today's output bit for bit; `tests/mixer.rs` checks
//! this against a verbatim copy of the pre-mixer-settings function.
//!
//! Vocal ducking: every stem but the lead tracks is turned down by up to
//! `MixSettings::duck_db` while the leads sing. The key is the summed
//! post-fader power of the audible lead tracks, smoothed by a one-pole with
//! `DUCK_ATTACK` rising and `DUCK_RELEASE` falling; the duck depth is
//! proportional to the key's RMS up to the full-scale point, where it
//! reaches `duck_db`. The full-scale point stays half the lead target
//! level times the lead strip gain, with no fader term (design 3.3), so a
//! lead fader move changes how hard the band ducks: a quiet lead fader
//! ducks less. This is the broadcast "voice-over ducker", used here so the
//! words stay on top of a full band without thinning the instrumental
//! sections.
//!
//! Stem printing (`engine::print`) shares the `routes`, `slap_sources` and
//! `duck_gains` this module builds, and the `premix`/gain split described
//! below, so a printed stem and the shipped mix always start from the same
//! per-track numbers.
//!
//! Only the output is full length; the buses are one block.

use dsp::dynamics::{Compressor, GainComputer, Link, PeakDetector};
use dsp::pan::{balance, equal_power};
use dsp::reverb::{Fdn8, T60_DC, T60_NYQ};
use sfcore::math::{db_to_gain, gain_to_db};
use sfcore::SR_F;
use song::Band;

use crate::mixset::MixSettings;
use crate::render::Stems;
use crate::stem::{SparseBuf, Stem, STEM_BLOCK};
use crate::track::TrackId;

/// Frames per mixer block. Divides `STEM_BLOCK`, so a mixer block lies in
/// one stem block.
pub const MIX_BLOCK: usize = 1024;
/// Reverb return gain.
pub const WET: f64 = 0.55;
/// Final sample peak.
pub const PEAK: f64 = 0.89;
/// Bus power counts frames with |L| + |R| above this (-80 dB).
pub const BUS_FLOOR: f64 = 1e-4;
/// Bus compressor: ratio, attack s, release s, knee dB, threshold over bus RMS dB.
pub const BUS_RATIO: f64 = 2.2;
pub const BUS_ATTACK: f64 = 0.02;
pub const BUS_RELEASE: f64 = 0.3;
pub const BUS_KNEE_DB: f64 = 10.0;
pub const BUS_OVER_RMS_DB: f64 = 5.0;
/// Accompaniment reduction while a lead sings, dB; `MixSettings`'s default.
pub const DUCK_DB: f64 = 5.0;
/// Key smoothing time constants, s.
pub const DUCK_ATTACK: f64 = 0.03;
pub const DUCK_RELEASE: f64 = 0.35;

const _: () = assert!(STEM_BLOCK.is_multiple_of(MIX_BLOCK));

/// Tracks that carry the lead melody: never ducked, and their summed
/// post-fader power keys the ducker (design section 3.3). Wave 2 adds
/// `TrackId::LeadB` here.
pub(crate) const LEAD_TRACKS: [TrackId; 1] = [TrackId::Lead];

/// A stereo signal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

/// One input channel of the mix and its four bus gains.
pub(crate) struct Route<'a> {
    pub(crate) id: TrackId,
    pub(crate) src: &'a SparseBuf,
    /// Main L, main R, send L, send R.
    pub(crate) g: [f32; 4],
    /// Turned down by the vocal ducker (every track but the lead tracks).
    pub(crate) ducked: bool,
}

/// One track's slapback as it enters the buses: unpanned, at its fader gain.
pub(crate) struct SlapSrc<'a> {
    pub(crate) id: TrackId,
    pub(crate) src: &'a SparseBuf,
    pub(crate) gain: f32,
}

/// `id`'s fader gain from `settings`, as the route builder applies it:
/// `db_to_gain(gain_db)` in `f64`, rounded once to `f32`. `0.0` dB gives
/// exactly `1.0`.
fn fader_gain(settings: &MixSettings, id: TrackId) -> f32 {
    db_to_gain(settings.tracks[id.index()].gain_db as f64) as f32
}

/// Per-frame gain the ducker applies to the accompaniment: 1 where the lead
/// tracks are silent, down to `1 - db_to_gain(-duck_db)` where they sing
/// (see the module doc). `None` when `duck_db` is 0 or no lead track plays.
pub fn duck_gains(stems: &Stems, band: &Band, settings: &MixSettings) -> Option<Vec<f32>> {
    if settings.duck_db <= 0.0 {
        return None;
    }
    let leads: Vec<(&SparseBuf, f64)> = LEAD_TRACKS
        .into_iter()
        .filter(|&id| id.plays(band) && settings.audible(id))
        .filter_map(|id| {
            let p = stems.get(id)?;
            // Post-fader power keys the ducker (design 3.3), so the fader
            // belongs in k.
            let k = id.strip().gain as f64 * fader_gain(settings, id) as f64 * p.level as f64;
            let ch = p.audio.channels().first()?;
            Some((ch, k))
        })
        .collect();
    if leads.is_empty() {
        return None;
    }
    // The full-scale point stays half the lead target level times the lead
    // strip gain (design 3.3): no fader term, so a fader move changes how
    // hard the lead ducks the band without moving the reference point. With
    // several leads (wave 2), combine their full-scale points the same way
    // their post-fader power is combined above (root-sum-square), so the
    // two stay in the same units.
    let full: f64 = LEAD_TRACKS
        .into_iter()
        .filter(|&id| id.plays(band) && settings.audible(id))
        .map(|id| {
            let k_full = id.strip().gain as f64 * stems.get(id).map_or(0.0, |p| p.level as f64);
            (0.5 * crate::track::TARGET_RMS * k_full).powi(2)
        })
        .sum::<f64>()
        .sqrt();
    let depth = 1.0 - db_to_gain(-settings.duck_db as f64);
    let (up, down) = (sfcore::math::one_pole_coeff_tau(DUCK_ATTACK, SR_F), sfcore::math::one_pole_coeff_tau(DUCK_RELEASE, SR_F));
    let mut ms = 0.0f64;
    let mut out = vec![1.0f32; stems.len];
    let mut spans: Vec<Option<&[f32]>> = Vec::with_capacity(leads.len());
    for s in (0..stems.len).step_by(MIX_BLOCK) {
        let n = MIX_BLOCK.min(stems.len - s);
        spans.clear();
        for &(ch, _) in &leads {
            spans.push(ch.span(s, n));
        }
        for i in 0..n {
            let mut e = 0.0f64;
            for (&(_, k), sp) in leads.iter().zip(&spans) {
                if let Some(x) = sp {
                    let v = x[i] as f64 * k;
                    e += v * v;
                }
            }
            let a = if e > ms { up } else { down };
            ms += a * (e - ms);
            let amount = (ms.sqrt() / full).min(1.0);
            out[s + i] = (1.0 - depth * amount) as f32;
        }
    }
    Some(out)
}

/// The routes of every audible stem `band` enables, in `TrackId` order.
pub(crate) fn routes<'a>(stems: &'a Stems, band: &Band, settings: &MixSettings) -> Vec<Route<'a>> {
    let mut out = Vec::new();
    for id in TrackId::ALL {
        if !(id.plays(band) && settings.audible(id)) {
            continue;
        }
        let Some(p) = stems.get(id) else { continue };
        let strip = id.strip();
        let t = settings.tracks[id.index()];
        let fader = fader_gain(settings, id);
        let k = strip.gain * fader * p.level;
        let send = strip.send;
        let pan = t.pan as f64;
        let ducked = !LEAD_TRACKS.contains(&id);
        let mut push = |src, gl: f32, gr: f32| {
            let (gl, gr) = (gl * k, gr * k);
            out.push(Route { id, src, g: [gl, gr, gl * send, gr * send], ducked });
        };
        match &p.audio {
            Stem::Mono(x) => {
                let [gl, gr] = equal_power(pan);
                push(x, gl, gr);
            }
            Stem::Stereo([l, r]) => {
                let m = balance(pan);
                push(l, m[0][0], m[1][0]);
                push(r, m[0][1], m[1][1]);
            }
        }
    }
    out
}

/// Every audible track's slapback, in `TrackId` order.
pub(crate) fn slap_sources<'a>(stems: &'a Stems, band: &Band, settings: &MixSettings) -> Vec<SlapSrc<'a>> {
    let mut out = Vec::new();
    for id in TrackId::ALL {
        if !(id.plays(band) && settings.audible(id)) {
            continue;
        }
        let Some(p) = stems.get(id) else { continue };
        let Some(slap) = &p.slap else { continue };
        out.push(SlapSrc { id, src: slap, gain: fader_gain(settings, id) });
    }
    out
}

/// Adds the main-bus-only (dry, no reverb) contribution of `routes`'
/// entries for `id` into `out`, ducked by `duck` where the route is ducked.
pub(crate) fn add_track_dry(out: &mut Stereo, routes: &[Route], id: TrackId, duck: Option<&[f32]>) {
    for rt in routes.iter().filter(|r| r.id == id) {
        let dk = duck.filter(|_| rt.ducked);
        let [a, b, _, _] = rt.g;
        for (at, x) in rt.src.iter_blocks() {
            let n = x.len();
            let d = dk.map(|g| &g[at..at + n]);
            for i in 0..n {
                let v = x[i] * d.map_or(1.0, |g| g[i]);
                out.l[at + i] += v * a;
                out.r[at + i] += v * b;
            }
        }
    }
}

/// Adds `slaps`' entries for `id` (unpanned, unducked) into `out`.
pub(crate) fn add_track_slap(out: &mut Stereo, slaps: &[SlapSrc], id: TrackId) {
    for sp in slaps.iter().filter(|s| s.id == id) {
        for (at, x) in sp.src.iter_blocks() {
            for (i, &v) in x.iter().enumerate() {
                let v = v * sp.gain;
                out.l[at + i] += v;
                out.r[at + i] += v;
            }
        }
    }
}

/// Adds every route's send-bus contribution into `sl`/`sr`, ducked where set.
pub(crate) fn add_send_bus(sl: &mut [f32], sr: &mut [f32], routes: &[Route], duck: Option<&[f32]>) {
    for rt in routes {
        let dk = duck.filter(|_| rt.ducked);
        let [_, _, c, d] = rt.g;
        for (at, x) in rt.src.iter_blocks() {
            let n = x.len();
            let dg = dk.map(|g| &g[at..at + n]);
            for i in 0..n {
                let v = x[i] * dg.map_or(1.0, |g| g[i]);
                sl[at + i] += v * c;
                sr[at + i] += v * d;
            }
        }
    }
}

/// Adds every slap source into `sl`/`sr` (unpanned, unducked).
pub(crate) fn add_send_slap(sl: &mut [f32], sr: &mut [f32], slaps: &[SlapSrc]) {
    for sp in slaps {
        for (at, x) in sp.src.iter_blocks() {
            for (i, &v) in x.iter().enumerate() {
                let v = v * sp.gain;
                sl[at + i] += v;
                sr[at + i] += v;
            }
        }
    }
}

/// The pre-compressor mix (design section 3.3): dry main bus plus the FDN
/// reverb return, no bus compressor, no peak normalisation; and the bus RMS
/// the compressor's threshold is set from. Shared by `mix_with` and
/// `engine::print::mix_gain`, so a printed stem's math and the shipped
/// mix's math are the same code.
pub fn premix(stems: &Stems, band: &Band, seed: u64, settings: &MixSettings) -> Stereo {
    premix_rms(stems, band, seed, settings).0
}

pub(crate) fn premix_rms(stems: &Stems, band: &Band, seed: u64, settings: &MixSettings) -> (Stereo, f64) {
    let len = stems.len;
    let routes = routes(stems, band, settings);
    let duck = duck_gains(stems, band, settings);
    let slaps = slap_sources(stems, band, settings);
    let mut fdn = Fdn8::new(SR_F, seed, T60_DC, T60_NYQ);
    let mut out = Stereo { l: vec![0.0; len], r: vec![0.0; len] };
    let (mut ml, mut mr) = ([0.0f32; MIX_BLOCK], [0.0f32; MIX_BLOCK]);
    let (mut sl, mut sr) = ([0.0f32; MIX_BLOCK], [0.0f32; MIX_BLOCK]);
    let (mut energy, mut count) = (0.0f64, 0usize);

    for s in (0..len).step_by(MIX_BLOCK) {
        let n = MIX_BLOCK.min(len - s);
        let (ml, mr, sl, sr) = (&mut ml[..n], &mut mr[..n], &mut sl[..n], &mut sr[..n]);
        ml.fill(0.0);
        mr.fill(0.0);
        sl.fill(0.0);
        sr.fill(0.0);
        for rt in &routes {
            let Some(x) = rt.src.span(s, n) else { continue };
            let [a, b, c, d] = rt.g;
            let dk = duck.as_deref().filter(|_| rt.ducked).map(|g| &g[s..s + n]);
            for i in 0..n {
                let v = x[i] * dk.map_or(1.0, |g| g[i]);
                ml[i] += v * a;
                mr[i] += v * b;
                sl[i] += v * c;
                sr[i] += v * d;
            }
        }
        for sp in &slaps {
            let Some(x) = sp.src.span(s, n) else { continue };
            for i in 0..n {
                let v = x[i] * sp.gain;
                ml[i] += v;
                mr[i] += v;
                sl[i] += v;
                sr[i] += v;
            }
        }
        fdn.process_block([&*sl, &*sr], [&mut *ml, &mut *mr], WET);
        out.l[s..s + n].copy_from_slice(ml);
        out.r[s..s + n].copy_from_slice(mr);
        for (&a, &b) in ml.iter().zip(mr.iter()) {
            let (a, b) = (a as f64, b as f64);
            if a.abs() + b.abs() > BUS_FLOOR {
                energy += a * a + b * b;
                count += 2;
            }
        }
    }
    let bus_rms = (energy / count.max(1) as f64).sqrt();
    (out, bus_rms)
}

/// Runs the bus compressor over `out` in place and returns the final peak
/// normalisation gain, `PEAK / peak`.
fn compress_and_gain(bus_rms: f64, out: &mut Stereo) -> f64 {
    let mut comp = Compressor::new(
        PeakDetector::new(BUS_ATTACK, BUS_RELEASE, SR_F),
        GainComputer { thr_db: gain_to_db(bus_rms) + BUS_OVER_RMS_DB, ratio: BUS_RATIO, knee_db: BUS_KNEE_DB },
        Link::StereoMax,
    );
    let mut pk = 1e-9f64;
    for (l, r) in out.l.chunks_mut(MIX_BLOCK).zip(out.r.chunks_mut(MIX_BLOCK)) {
        comp.process_stereo(l, r);
        for (&a, &b) in l.iter().zip(r.iter()) {
            pk = pk.max((a as f64).abs()).max((b as f64).abs());
        }
    }
    PEAK / pk
}

/// The pre-normalisation (post bus compressor) mix and the final peak
/// normalisation gain; shared by `mix_with` and `engine::print::mix_gain`.
pub(crate) fn compressed(stems: &Stems, band: &Band, seed: u64, settings: &MixSettings) -> (Stereo, f64) {
    let (mut out, bus_rms) = premix_rms(stems, band, seed, settings);
    let g = compress_and_gain(bus_rms, &mut out);
    (out, g)
}

/// Mixes the stems `band` enables, per `settings`. `seed` sets the reverb
/// line offsets.
pub fn mix_with(stems: &Stems, band: &Band, seed: u64, settings: &MixSettings) -> Stereo {
    let (mut out, g) = compressed(stems, band, seed, settings);
    for v in out.l.iter_mut().chain(out.r.iter_mut()) {
        *v = (*v as f64 * g) as f32;
    }
    out
}

/// Mixes the stems `band` enables with the default mix settings (the
/// strip's pan and gain, nothing muted or soloed, the shipped ducking
/// depth): bit-identical to the mix before `MixSettings` existed.
pub fn mix(stems: &Stems, band: &Band, seed: u64) -> Stereo {
    mix_with(stems, band, seed, &MixSettings::default_for(stems))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strip::ProcessedStem;
    use crate::track::N_TRACKS;

    fn one_track_stems() -> Stems {
        let tracks: [Option<ProcessedStem>; N_TRACKS] = std::array::from_fn(|i| {
            if i == TrackId::Bass.index() {
                Some(ProcessedStem { audio: Stem::Mono(SparseBuf::from_dense(&[0.1f32; STEM_BLOCK])), level: 1.0, slap: None })
            } else {
                None
            }
        });
        Stems { len: STEM_BLOCK, tracks }
    }

    #[test]
    fn db_to_gain_zero_is_exactly_one() {
        assert_eq!(db_to_gain(0.0), 1.0);
    }

    /// The route builder: a track's route gain scales by exactly
    /// `db_to_gain(gain_db)` in `f32` (design section 3.3).
    #[test]
    fn route_gain_scales_by_exact_fader() {
        let stems = one_track_stems();
        let band = Band::default();
        let base = MixSettings::default_for(&stems);
        let mut boosted = base;
        boosted.tracks[TrackId::Bass.index()].gain_db = 6.0;
        let r0 = routes(&stems, &band, &base);
        let r1 = routes(&stems, &band, &boosted);
        assert_eq!(r0.len(), 1);
        assert_eq!(r1.len(), 1);
        // Match the route builder's own multiplication order (strip gain
        // times fader times level, then times the pan gain) rather than
        // scaling r0's result after the fact, which would round differently.
        let strip = TrackId::Bass.strip();
        let level = stems.get(TrackId::Bass).expect("bass stem").level;
        let [gl, _] = equal_power(strip.pan as f64);
        let expect0 = gl * (strip.gain * db_to_gain(0.0) as f32 * level);
        let expect1 = gl * (strip.gain * db_to_gain(6.0) as f32 * level);
        assert_eq!(r0[0].g[0], expect0);
        assert_eq!(r1[0].g[0], expect1);
    }
}

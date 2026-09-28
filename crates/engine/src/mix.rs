//! The mixer (design section 5.13): a streamed block mixer over the cached
//! stems, then the bus compressor and peak normalisation.
//!
//! Per 1024-frame block: clear the main and send buses; add every stem the
//! band enables, in `TrackId` order, with its pan gains times strip gain
//! times level (mono: equal-power law; stereo: balance law; `dsp::pan`),
//! and the same times the strip's send into the send bus; add the lead
//! slapback to both buses at unity; run the send bus through the FDN reverb
//! (`dsp::reverb::Fdn8`, Jot and Chaigne 1991; T60 2.2 s at DC, 0.8 s at
//! Nyquist) at wet 0.55 into the main bus; write L/R; accumulate the bus
//! power over every frame with |L| + |R| above -80 dB.
//!
//! Then the bus compressor (`dsp::dynamics`, Giannoulis, Massberg, Reiss
//! 2012): 2.2:1, attack 20 ms, release 300 ms, 10 dB soft knee, threshold
//! 5 dB over the bus RMS, stereo-linked on max(|L|, |R|). Last, the sample
//! peak is normalised to 0.89.
//!
//! Only the output is full length; the buses are one block.

use dsp::dynamics::{Compressor, GainComputer, Link, PeakDetector};
use dsp::pan::{balance, equal_power};
use dsp::reverb::{Fdn8, T60_DC, T60_NYQ};
use sfcore::math::gain_to_db;
use sfcore::SR_F;
use song::Band;

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

const _: () = assert!(STEM_BLOCK % MIX_BLOCK == 0);

/// A stereo signal.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stereo {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}

/// One input channel of the mix and its four bus gains.
struct Route<'a> {
    src: &'a SparseBuf,
    /// Main L, main R, send L, send R.
    g: [f32; 4],
}

/// The routes of every stem `band` enables, in `TrackId` order.
fn routes<'a>(stems: &'a Stems, band: &Band) -> Vec<Route<'a>> {
    let mut out = Vec::new();
    for id in TrackId::ALL {
        if !id.plays(band) {
            continue;
        }
        let Some(p) = stems.get(id) else { continue };
        let strip = id.strip();
        let k = strip.gain * p.level;
        let send = strip.send;
        let mut push = |src, gl: f32, gr: f32| {
            let (gl, gr) = (gl * k, gr * k);
            out.push(Route { src, g: [gl, gr, gl * send, gr * send] });
        };
        match &p.audio {
            Stem::Mono(x) => {
                let [gl, gr] = equal_power(strip.pan as f64);
                push(x, gl, gr);
            }
            Stem::Stereo([l, r]) => {
                let m = balance(strip.pan as f64);
                push(l, m[0][0], m[1][0]);
                push(r, m[0][1], m[1][1]);
            }
        }
    }
    out
}

/// Mixes the stems `band` enables. `seed` sets the reverb line offsets.
pub fn mix(stems: &Stems, band: &Band, seed: u64) -> Stereo {
    let len = stems.len;
    let routes = routes(stems, band);
    let slap = if TrackId::Lead.plays(band) && stems.get(TrackId::Lead).is_some() { stems.slapback.as_ref() } else { None };
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
            for i in 0..n {
                let v = x[i];
                ml[i] += v * a;
                mr[i] += v * b;
                sl[i] += v * c;
                sr[i] += v * d;
            }
        }
        if let Some(x) = slap.and_then(|b| b.span(s, n)) {
            for i in 0..n {
                let v = x[i];
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
    let g = PEAK / pk;
    for v in out.l.iter_mut().chain(out.r.iter_mut()) {
        *v = (*v as f64 * g) as f32;
    }
    out
}

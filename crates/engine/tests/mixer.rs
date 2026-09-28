//! Mix settings tests (design section 3.2-3.3):
//! - bit identity of `mix` (default `MixSettings`) against a verbatim copy
//!   of the pre-`MixSettings` mixer, kept below;
//! - muting a track is the same as switching off its band part;
//! - soloing one track is the same as muting every other track;
//! - `duck_db` 0 turns the ducker off;
//! - the mix sidecar's JSON round trip and lenient reading;
//! - a printed stem plus the printed reverb sums to the pre-compressor mix
//!   times the mix gain;
//! - `print_stem` of a muted track is `None`.
//!
//! The route builder's own exact-fader-gain test lives next to the builder
//! in `src/mix.rs`, since the builder is private to the crate.

use dsp::dynamics::{Compressor, GainComputer, Link, PeakDetector};
use dsp::pan::{balance, equal_power};
use dsp::reverb::{Fdn8, T60_DC, T60_NYQ};
use engine::{demo_song, mix, mix_gain, mix_with, premix, print_reverb, print_stem, render, MixSettings, NoProgress, SparseBuf, Stem, Stems, Stereo, TrackId};
use sfcore::math::{db_to_gain, gain_to_db};
use sfcore::SR_F;
use song::{Band, Song};

// --- Verbatim copy of the mixer before `MixSettings` existed -------------
// (only `stems.slapback` is rewritten to `stems.get(Lead).slap`, since that
// field moved with this wave; every gain, pan and ordering is unchanged.)

const MIX_BLOCK: usize = 1024;
const WET: f64 = 0.55;
const PEAK: f64 = 0.89;
const BUS_FLOOR: f64 = 1e-4;
const BUS_RATIO: f64 = 2.2;
const BUS_ATTACK: f64 = 0.02;
const BUS_RELEASE: f64 = 0.3;
const BUS_KNEE_DB: f64 = 10.0;
const BUS_OVER_RMS_DB: f64 = 5.0;
const DUCK_DB: f64 = 5.0;
const DUCK_ATTACK: f64 = 0.03;
const DUCK_RELEASE: f64 = 0.35;

fn old_duck_gains(stems: &Stems, band: &Band) -> Option<Vec<f32>> {
    if !TrackId::Lead.plays(band) {
        return None;
    }
    let p = stems.get(TrackId::Lead)?;
    let k = TrackId::Lead.strip().gain as f64 * p.level as f64;
    let lead = p.audio.channels().first()?;
    let full = 0.5 * engine::track::TARGET_RMS * k;
    let depth = 1.0 - db_to_gain(-DUCK_DB);
    let (up, down) = (sfcore::math::one_pole_coeff_tau(DUCK_ATTACK, SR_F), sfcore::math::one_pole_coeff_tau(DUCK_RELEASE, SR_F));
    let mut ms = 0.0f64;
    let mut out = vec![1.0f32; stems.len];
    let mut buf = vec![0.0f32; MIX_BLOCK];
    for s in (0..stems.len).step_by(MIX_BLOCK) {
        let n = MIX_BLOCK.min(stems.len - s);
        let x: &[f32] = match lead.span(s, n) {
            Some(x) => x,
            None => {
                buf[..n].fill(0.0);
                &buf[..n]
            }
        };
        for i in 0..n {
            let v = x[i] as f64 * k;
            let e = v * v;
            let a = if e > ms { up } else { down };
            ms += a * (e - ms);
            let amount = (ms.sqrt() / full).min(1.0);
            out[s + i] = (1.0 - depth * amount) as f32;
        }
    }
    Some(out)
}

struct OldRoute<'a> {
    src: &'a SparseBuf,
    g: [f32; 4],
    ducked: bool,
}

fn old_routes<'a>(stems: &'a Stems, band: &Band) -> Vec<OldRoute<'a>> {
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
            out.push(OldRoute { src, g: [gl, gr, gl * send, gr * send], ducked: id != TrackId::Lead });
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

fn old_mix(stems: &Stems, band: &Band, seed: u64) -> Stereo {
    let len = stems.len;
    let routes = old_routes(stems, band);
    let duck = old_duck_gains(stems, band);
    let slap = if TrackId::Lead.plays(band) { stems.get(TrackId::Lead).and_then(|p| p.slap.as_ref()) } else { None };
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

// --- Test helpers ----------------------------------------------------------

fn bits(m: &Stereo) -> Vec<u32> {
    m.l.iter().chain(&m.r).map(|v| v.to_bits()).collect()
}

/// A short full-band song (verse, chorus, verse), all band parts on.
fn blues(drums: &str) -> Song {
    let raw = serde_json::json!({
        "title":"Blues Test","key":"C","mode":"major","meter":"4/4","tempo":112,
        "guitar":"strum","voice":"alto",
        "band":{"drums":drums,"bass":true,"harmonyGuitar":true,"harp":true,"violin":true,"choir":true,"harmonies":true,"doubles":true},
        "sections":[
            {"type":"verse","lines":[
                {"syl":"*Wo-ke up this *morn-ing, *sun was *not a-round","chords":["C","C","C","C","F","F","C","C","G","F","C","G"]}
            ]},
            {"type":"chorus","lines":[
                {"syl":"*Roll on *down the *line to *town","chords":["F","F","C","C","G","F","C","G"]}
            ]},
            {"type":"verse","same":true}
        ]
    });
    song::normalize_value(&raw).expect("blues literal normalises").0
}

// --- Tests ------------------------------------------------------------------

#[test]
fn default_mix_is_bit_identical_to_the_old_mixer() {
    for seed in [1234, 2718, 7] {
        let song = demo_song();
        let (_, stems) = render(song, seed, None, &NoProgress);
        let a = old_mix(&stems, &song.band, seed);
        let b = mix(&stems, &song.band, seed);
        assert!(bits(&a) == bits(&b), "seed {seed}: default mix differs from the old mixer");
    }
}

#[test]
fn muting_a_non_lead_track_equals_switching_off_its_band_part() {
    let song = blues("brushes");
    let (_, stems) = render(&song, 11, None, &NoProgress);
    let settings = MixSettings::default_for(&stems);

    let mut muted = settings;
    muted.tracks[TrackId::Bass.index()].mute = true;
    let a = mix_with(&stems, &song.band, 11, &muted);

    let mut band_off = song.band;
    band_off.bass = false;
    let b = mix_with(&stems, &band_off, 11, &settings);

    assert!(bits(&a) == bits(&b), "muting bass should equal switching off its band part");
}

#[test]
fn solo_equals_muting_everything_else() {
    let song = blues("brushes");
    let (_, stems) = render(&song, 11, None, &NoProgress);
    let settings = MixSettings::default_for(&stems);

    let mut solo = settings;
    solo.tracks[TrackId::Bass.index()].solo = true;

    let mut muted = settings;
    for id in TrackId::ALL {
        if id != TrackId::Bass {
            muted.tracks[id.index()].mute = true;
        }
    }

    let a = mix_with(&stems, &song.band, 11, &solo);
    let b = mix_with(&stems, &song.band, 11, &muted);
    assert!(bits(&a) == bits(&b), "soloing bass should equal muting every other track");
}

#[test]
fn duck_db_zero_turns_the_ducker_off() {
    let song = demo_song();
    let (_, stems) = render(song, 1234, None, &NoProgress);
    let mut settings = MixSettings::default_for(&stems);
    assert!(engine::mix::duck_gains(&stems, &song.band, &settings).is_some());
    settings.duck_db = 0.0;
    let duck = engine::mix::duck_gains(&stems, &song.band, &settings);
    assert!(duck.is_none());

    // With the ducker off, an accompaniment track's own printed contribution
    // does not depend on whether the lead sings.
    let with_lead = print_stem(&stems, &song.band, 1234, &settings, TrackId::Guitar, duck.as_deref(), 1.0).expect("guitar plays");

    let mut lead_muted = settings;
    lead_muted.tracks[TrackId::Lead.index()].mute = true;
    let duck2 = engine::mix::duck_gains(&stems, &song.band, &lead_muted);
    assert!(duck2.is_none());
    let without_lead = print_stem(&stems, &song.band, 1234, &lead_muted, TrackId::Guitar, duck2.as_deref(), 1.0).expect("guitar plays");

    assert!(bits(&with_lead) == bits(&without_lead), "guitar should be unaffected by the lead when duck_db is 0");
}

#[test]
fn a_quieter_lead_fader_ducks_the_band_less() {
    // Design 3.3: the full-scale point is half the lead target level times
    // the lead strip gain, with no fader term, so turning the lead fader
    // down lowers the key relative to that fixed point and the band ducks
    // less (the minimum duck gain rises toward 1.0).
    let song = demo_song();
    let (_, stems) = render(song, 1234, None, &NoProgress);
    let full = MixSettings::default_for(&stems);
    let full_min = engine::mix::duck_gains(&stems, &song.band, &full).expect("lead sings").into_iter().fold(1.0f32, f32::min);

    // The demo's lead sings well above the full-scale point, so the ducker
    // saturates at full depth (amount clamped to 1.0) over a wide fader
    // range; -40 dB is comfortably past where it comes off that clamp.
    let mut quiet = full;
    quiet.tracks[TrackId::Lead.index()].gain_db = -40.0;
    let quiet_min = engine::mix::duck_gains(&stems, &song.band, &quiet).expect("lead sings").into_iter().fold(1.0f32, f32::min);

    assert!(quiet_min > full_min, "a -40 dB lead fader should duck less: full {full_min}, quiet {quiet_min}");
}

#[test]
fn mix_sidecar_json_round_trips_and_reads_leniently() {
    let song = demo_song();
    let (_, stems) = render(song, 1234, None, &NoProgress);
    let defaults = MixSettings::default_for(&stems);

    let mut edited = defaults;
    edited.tracks[TrackId::Violin.index()].gain_db = -3.0;
    edited.tracks[TrackId::Harp.index()].mute = true;
    edited.tracks[TrackId::Lead.index()].pan = -0.1;
    edited.duck_db = 4.0;

    let text = serde_json::to_string(&edited.to_json(&defaults)).expect("serialise sidecar");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse sidecar");
    let (back, warnings) = MixSettings::from_json(&value, &defaults);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, edited);

    let lenient = serde_json::json!({
        "tracks": {"lead": {"gain_db": 3.0}, "no_such_track": {"mute": true}},
        "duck_db": "loud",
    });
    let (out, warnings) = MixSettings::from_json(&lenient, &defaults);
    assert_eq!(out.tracks[TrackId::Lead.index()].gain_db, 3.0);
    assert_eq!(out.duck_db, defaults.duck_db);
    assert_eq!(warnings.len(), 2);
}

#[test]
fn printed_stems_and_reverb_sum_to_the_premix_times_gain() {
    let song = demo_song();
    let seed = 1234;
    let (_, stems) = render(song, seed, None, &NoProgress);
    let settings = MixSettings::default_for(&stems);
    let duck = engine::mix::duck_gains(&stems, &song.band, &settings);
    let gain = mix_gain(&stems, &song.band, seed, &settings);

    let mut total = Stereo { l: vec![0.0; stems.len], r: vec![0.0; stems.len] };
    for id in TrackId::ALL {
        if let Some(s) = print_stem(&stems, &song.band, seed, &settings, id, duck.as_deref(), gain) {
            for i in 0..stems.len {
                total.l[i] += s.l[i];
                total.r[i] += s.r[i];
            }
        }
    }
    let reverb = print_reverb(&stems, &song.band, seed, &settings, duck.as_deref(), gain);
    for i in 0..stems.len {
        total.l[i] += reverb.l[i];
        total.r[i] += reverb.r[i];
    }

    let pre = premix(&stems, &song.band, seed, &settings);
    let peak = pre.l.iter().chain(&pre.r).fold(0.0f32, |a, &v| a.max(v.abs())) as f64;
    let tol = 1e-5 * peak;
    let mut worst = 0.0f64;
    for i in 0..stems.len {
        worst = worst.max((total.l[i] as f64 - pre.l[i] as f64 * gain as f64).abs());
        worst = worst.max((total.r[i] as f64 - pre.r[i] as f64 * gain as f64).abs());
    }
    assert!(worst < tol, "worst sample error {worst}, tolerance {tol}");
}

#[test]
fn print_stem_of_a_muted_track_is_none() {
    let song = demo_song();
    let seed = 1234;
    let (_, stems) = render(song, seed, None, &NoProgress);
    let mut settings = MixSettings::default_for(&stems);
    settings.tracks[TrackId::Violin.index()].mute = true;
    let duck = engine::mix::duck_gains(&stems, &song.band, &settings);
    assert!(print_stem(&stems, &song.band, seed, &settings, TrackId::Violin, duck.as_deref(), 1.0).is_none());
}

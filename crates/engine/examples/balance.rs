//! Vocal balance probe: how loud the lead vocal sits against the accompaniment.
//!
//! Renders a song (JSON path, optional style, seed), then over the frames where
//! the lead sings reports each track's power relative to the lead after its
//! strip gain, level and the mixer's vocal ducking (pan ignored), broadband and in the 1-4 kHz presence
//! band where intelligibility lives.
//!
//! Usage: cargo run --release -p engine --example balance -- SONG.json [--style KEY] [--seed S]

use dsp::biquad::{Biquad, BiquadCoeffs};
use engine::render::{render, NoProgress};
use engine::track::TrackId;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("SONG.json");
    let (mut style, mut seed) = (None::<String>, 1u64);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--style" => style = args.next(),
            "--seed" => seed = args.next().and_then(|v| v.parse().ok()).expect("--seed N"),
            _ => panic!("unknown argument {a}"),
        }
    }
    sfcore::fp::init_pool(None);
    let json = std::fs::read_to_string(&path).expect("read song");
    let (mut song, _) = song::wire::normalize_str(&json).expect("normalize");
    if let Some(k) = &style {
        songwriter::styles::apply_style(k, &mut song).expect("style");
    }
    let (_, stems) = render(&song, seed, None, &NoProgress);
    let len = stems.len;
    let dense = |id: TrackId| -> Option<Vec<f32>> {
        let ps = stems.get(id)?;
        let g = id.strip().gain * ps.level;
        let chs = ps.audio.channels();
        let n = chs.len() as f32;
        let mut out = vec![0f32; len];
        for c in chs {
            for (o, v) in out.iter_mut().zip(c.to_dense()) {
                *o += v * g / n.sqrt();
            }
        }
        Some(out)
    };
    let lead = dense(TrackId::Lead).expect("lead stem");
    let settings = engine::MixSettings::default_for(&stems);
    let duck = engine::mix::duck_gains(&stems, &song.band, &settings);
    // Frames where the lead sings: 2048-sample blocks within 20 dB of its loudest.
    const B: usize = 2048;
    let rms: Vec<f64> = lead
        .chunks(B)
        .map(|c| (c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / c.len() as f64).sqrt())
        .collect();
    let top = rms.iter().cloned().fold(0.0, f64::max);
    let active: Vec<bool> = rms.iter().map(|r| *r > top * 0.1).collect();
    let power = |x: &[f32], band: bool| -> f64 {
        let mut y = x.to_vec();
        if band {
            let mut f = Biquad::new(BiquadCoeffs::bandpass(sfcore::SR as f64, 2000.0, 0.7));
            f.process(&mut y);
        }
        y.chunks(B)
            .zip(&active)
            .filter(|(_, a)| **a)
            .map(|(c, _)| c.iter().map(|v| (*v as f64).powi(2)).sum::<f64>())
            .sum()
    };
    let (lb, lp) = (power(&lead, false), power(&lead, true));
    let mut acc_b = vec![0f32; len];
    println!("{:<16} {:>9} {:>11}", "track", "vs lead", "1-4 kHz");
    for id in TrackId::ALL {
        if id == TrackId::Lead {
            continue;
        }
        if id.strip().band.is_some_and(|b| !b.on(&song.band)) {
            continue;
        }
        let Some(mut x) = dense(id) else { continue };
        if let Some(g) = &duck {
            for (v, g) in x.iter_mut().zip(g) {
                *v *= g;
            }
        }
        let (b, p) = (power(&x, false), power(&x, true));
        println!(
            "{:<16} {:>8.1} dB {:>8.1} dB",
            id.name(),
            10.0 * (b / lb).log10(),
            10.0 * (p / lp).log10()
        );
        for (a, v) in acc_b.iter_mut().zip(&x) {
            *a += v;
        }
    }
    let (ab, ap) = (power(&acc_b, false), power(&acc_b, true));
    println!(
        "{:<16} {:>8.1} dB {:>8.1} dB",
        "ALL accomp.",
        10.0 * (ab / lb).log10(),
        10.0 * (ap / lp).log10()
    );
}

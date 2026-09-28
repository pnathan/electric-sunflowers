//! Times 1000 guitar notes (random MIDI 40-76, 1.5 s each) through
//! `instruments::pluck::pluck_into` on one thread, best of three runs, and
//! prints the summed 1/3-octave spectrum of the first 200 notes (dB
//! relative to the total power), 50 Hz to 16 kHz.
//!
//! Run: cargo run --release -p instruments --example pluckbench

use dsp::fft::{RealFft, C32};
use instruments::pluck::{pluck_into, PluckParams, PluckScratch};
use sfcore::math::mtof;
use sfcore::random::{tag, Rng};
use sfcore::SR_F;
use std::time::Instant;

const NOTES: usize = 1000;
const SECS: f64 = 1.5;
const VEL: f64 = 0.8;

fn main() {
    sfcore::fp::flush_denormals();
    let len = (SECS * SR_F) as usize;
    let mut pick = Rng::stream(1, tag("bench.midi"));
    let midis: Vec<u8> = (0..NOTES).map(|_| 40 + pick.below(37) as u8).collect();
    let mut out = vec![0.0f32; len];

    // Best of three runs, to reduce the effect of machine load.
    let mut scratch = PluckScratch::new();
    let mut sink = 0.0f64;
    let mut best = f64::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        for (k, &m) in midis.iter().enumerate() {
            out.fill(0.0);
            let f = mtof(m as f64);
            let mut rng = Rng::event(1, tag("bench.note"), k as u64);
            let p = PluckParams::GUITAR.note(f, VEL, &mut rng);
            pluck_into(&mut out, 0, f, len, &p, &mut rng, &mut scratch);
            sink += out[len / 2] as f64;
        }
        best = best.min(t.elapsed().as_secs_f64());
    }

    println!("notes {NOTES}, {SECS} s each, MIDI 40-76");
    println!("pluck_into  {:.3} s ({:.1} us/note)", best, 1e6 * best / NOTES as f64);
    println!("(checksum {sink:.6})");

    spectrum(&midis[..200], len);
}

/// Prints the summed 1/3-octave power of `midis` (dB relative to the
/// total), 50 Hz to 16 kHz, from the first 65536 samples of each note.
fn spectrum(midis: &[u8], len: usize) {
    let n = 65536;
    let fft = RealFft::new(n);
    let mut scratch = fft.make_scratch();
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    let mut time = vec![0.0f32; n];
    let mut acc = vec![0.0f64; fft.spectrum_len()];
    let mut out = vec![0.0f32; len];
    let mut ps = PluckScratch::new();
    for (k, &m) in midis.iter().enumerate() {
        let f = mtof(m as f64);
        out.fill(0.0);
        let mut rng = Rng::event(2, tag("bench.note"), k as u64);
        let p = PluckParams::GUITAR.note(f, VEL, &mut rng);
        pluck_into(&mut out, 0, f, len, &p, &mut rng, &mut ps);
        time.copy_from_slice(&out[..n]);
        if fft.forward(&mut time, &mut spec, &mut scratch).is_err() {
            return;
        }
        for (x, c) in acc.iter_mut().zip(spec.iter()) {
            *x += (c.re as f64).powi(2) + (c.im as f64).powi(2);
        }
    }
    let df = SR_F / n as f64;
    let tot: f64 = acc.iter().sum();
    println!("1/3-octave spectrum, {} notes: band Hz, dB", midis.len());
    for b in 0..25 {
        let fc = 50.0 * 2f64.powf(b as f64 / 3.0);
        let k0 = (fc * 2f64.powf(-1.0 / 6.0) / df).ceil() as usize;
        let k1 = (fc * 2f64.powf(1.0 / 6.0) / df).floor() as usize;
        let db = 10.0 * (acc[k0..=k1].iter().sum::<f64>() / tot).log10();
        println!("{fc:7.0} {db:7.1}");
    }
}

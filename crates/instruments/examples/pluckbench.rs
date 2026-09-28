//! Times 1000 guitar notes (random MIDI 40-76, 1.5 s each) with the new
//! `instruments::pluck::pluck_into` and with the original `dsp::pluck::pluck`,
//! on one thread, best of three runs each, and prints both times and the
//! speed ratio. Then compares
//! the two models' summed 1/3-octave spectra over the first 200 notes
//! (each normalised to its total power), to show the sound stays close.
//!
//! The original model needs its own option bag and generator
//! (`dsp::pluck::PluckOpts`, `sfcore::rng`); both go when `dsp::pluck` is
//! deleted, and this comparison with them.
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

    // Best of three runs per model, to reduce the effect of machine load.
    let mut scratch = PluckScratch::new();
    let mut sink = 0.0f64;
    let mut t_new = f64::MAX;
    let mut t_old = f64::MAX;
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
        t_new = t_new.min(t.elapsed().as_secs_f64());

        // Original model, same per-note parameters as the old guitar part.
        let mut r = sfcore::rng::rng_for(1, "bench");
        let t = Instant::now();
        for &m in &midis {
            out.fill(0.0);
            let f = mtof(m as f64);
            let o = old_opts(f, &mut r);
            dsp::pluck::pluck(&mut out, 0, f, len as i64, &o, &mut r);
            sink += out[len / 2] as f64;
        }
        t_old = t_old.min(t.elapsed().as_secs_f64());
    }

    println!("notes {NOTES}, {SECS} s each, MIDI 40-76");
    println!("new pluck_into      {:.3} s ({:.1} us/note)", t_new, 1e6 * t_new / NOTES as f64);
    println!("old dsp::pluck      {:.3} s ({:.1} us/note)", t_old, 1e6 * t_old / NOTES as f64);
    println!("speedup             {:.2}x", t_old / t_new);
    println!("(checksum {sink:.6})");

    spectra(&midis[..200], len);
}

/// Old-model options for the guitar at velocity `VEL`.
fn old_opts(f: f64, r: &mut sfcore::rng::Rng) -> dsp::pluck::PluckOpts {
    dsp::pluck::PluckOpts {
        amp: VEL,
        bright: Some(0.6 + 0.25 * VEL),
        damp: Some(0.18),
        glide: 5.0 * VEL,
        atk_noise: VEL,
        t60: 7.0 * (82.0 / f).powf(0.45),
        pick: Some(0.11 + r.next() * 0.07),
        noise: Some(0.06),
        detune: Some(1.0 + r.next() * 0.8),
        rel: 0.02,
        rel_t: 0.08,
    }
}

/// Prints the 1/3-octave power of both models (dB relative to each total)
/// and their difference, 50 Hz to 16 kHz.
fn spectra(midis: &[u8], len: usize) {
    let n = 65536;
    let fft = RealFft::new(n);
    let mut scratch = fft.make_scratch();
    let mut spec = vec![C32::default(); fft.spectrum_len()];
    let mut time = vec![0.0f32; n];
    let mut acc = [vec![0.0f64; fft.spectrum_len()], vec![0.0f64; fft.spectrum_len()]];
    let mut out = vec![0.0f32; len];
    let mut ps = PluckScratch::new();
    let mut r = sfcore::rng::rng_for(2, "bench");
    for (k, &m) in midis.iter().enumerate() {
        let f = mtof(m as f64);
        for (which, a) in acc.iter_mut().enumerate() {
            out.fill(0.0);
            if which == 0 {
                let mut rng = Rng::event(2, tag("bench.note"), k as u64);
                let p = PluckParams::GUITAR.note(f, VEL, &mut rng);
                pluck_into(&mut out, 0, f, len, &p, &mut rng, &mut ps);
            } else {
                let o = old_opts(f, &mut r);
                dsp::pluck::pluck(&mut out, 0, f, len as i64, &o, &mut r);
            }
            time.copy_from_slice(&out[..n]);
            if fft.forward(&mut time, &mut spec, &mut scratch).is_err() {
                return;
            }
            for (x, c) in a.iter_mut().zip(spec.iter()) {
                *x += (c.re as f64).powi(2) + (c.im as f64).powi(2);
            }
        }
    }
    let df = SR_F / n as f64;
    let tot: Vec<f64> = acc.iter().map(|a| a.iter().sum()).collect();
    println!("1/3-octave spectra, {} notes: band Hz, new dB, old dB, new - old", midis.len());
    let mut worst = 0.0f64;
    for b in 0..25 {
        let fc = 50.0 * 2f64.powf(b as f64 / 3.0);
        let k0 = (fc * 2f64.powf(-1.0 / 6.0) / df).ceil() as usize;
        let k1 = (fc * 2f64.powf(1.0 / 6.0) / df).floor() as usize;
        let db: Vec<f64> = (0..2).map(|w| 10.0 * (acc[w][k0..=k1].iter().sum::<f64>() / tot[w]).log10()).collect();
        let d = db[0] - db[1];
        if (100.0..=10000.0).contains(&fc) {
            worst = worst.max(d.abs());
        }
        println!("{fc:7.0} {:7.1} {:7.1} {d:+6.2}", db[0], db[1]);
    }
    println!("largest |new - old| over 100 Hz-10 kHz: {worst:.2} dB");
}

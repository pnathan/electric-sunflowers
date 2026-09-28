//! Vowel distinctness probe: ten sustained vowels sung by the baritone
//! (/hh/ + vowel, MIDI 52, no vibrato, no scoop), the 1/3-octave spectrum
//! of each (Hann window, 8192 samples from 0.35 s into the note), levels
//! relative to each vowel's loudest band, and the mean RMS distance between
//! all pairs over the 12 bands from 200 Hz to 2.5 kHz. Prints
//! "mean vowel distance(200-2.5k) dB X" and the render cost in ns/sample.

use dsp::fft::{RealFft, C32};
use sfcore::SR_F;
use song::events::VocalNote;
use song::{Phoneme, Voice};
use voice::{render_phrases, VoiceSettings};

const VW: [Phoneme; 10] = [
    Phoneme::Iy,
    Phoneme::Ih,
    Phoneme::Eh,
    Phoneme::Ae,
    Phoneme::Aa,
    Phoneme::Ao,
    Phoneme::Ow,
    Phoneme::Uw,
    Phoneme::Ah,
    Phoneme::Er,
];
const BANDS: [f64; 17] = [
    200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
];

/// Hann-windowed power spectrum of the first 8192 samples (bins 0..4095).
fn spec(a: &[f32]) -> Vec<f64> {
    const N: usize = 8192;
    let fft = RealFft::new(N);
    let mut x: Vec<f32> =
        (0..N).map(|i| a[i] * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos()) as f32).collect();
    let mut out = vec![C32::default(); fft.spectrum_len()];
    let mut scratch = fft.make_scratch();
    fft.forward(&mut x, &mut out, &mut scratch).expect("buffers sized from the plan");
    out[..N / 2].iter().map(|c| c.re as f64 * c.re as f64 + c.im as f64 * c.im as f64).collect()
}

fn main() {
    let mut sp = Vec::new();
    let mut t = 0.5f64;
    for v in VW {
        sp.push(VocalNote {
            t0: t,
            t1: t + 1.2,
            midi: 52.0,
            phones: vec![Phoneme::Hh, v],
            amp: 1.0,
            stress: false,
            phrase_start: false,
            phrase_end: false,
            grace: None,
        });
        t += 1.6;
    }
    let len = ((t + 1.0) * SR_F).ceil() as usize;
    let settings = VoiceSettings { vibrato_scale: 0.0, scoop: false, ..VoiceSettings::default() };

    let start = std::time::Instant::now();
    let mut x = vec![0.0f32; len];
    render_phrases(&sp, Voice::Baritone, &settings, 3, len, |s0, v| x[s0..s0 + v.len()].copy_from_slice(v));
    let elapsed = start.elapsed();
    eprintln!("samples {} ns/sample {:.2}", x.len(), elapsed.as_nanos() as f64 / x.len() as f64);

    let df = SR_F / 8192.0;
    let mut spectra: Vec<[f64; 17]> = Vec::new();
    for n in &sp {
        let s0 = ((n.t0 + 0.35) * SR_F).round() as usize;
        let sl = &x[s0..s0 + 8192];
        let p_spec = spec(sl);
        let mut l = [0.0f64; 17];
        for (bi, &b) in BANDS.iter().enumerate() {
            let lo = (b / 1.12 / df).floor() as usize;
            let mut e = 0.0;
            let mut q = lo;
            while (q as f64) < b * 1.12 / df {
                if q < p_spec.len() {
                    e += p_spec[q];
                }
                q += 1;
            }
            l[bi] = 10.0 * (e + 1e-20).log10();
        }
        let mx = l.iter().cloned().fold(f64::MIN, f64::max);
        for v in l.iter_mut() {
            *v -= mx;
        }
        spectra.push(l);
    }

    let mut tot = 0.0;
    let mut c = 0;
    for i in 0..VW.len() {
        for j in (i + 1)..VW.len() {
            let mut d = 0.0;
            for b in 0..12 {
                d += (spectra[i][b] - spectra[j][b]).powi(2);
            }
            tot += (d / 12.0).sqrt();
            c += 1;
        }
    }
    println!("mean vowel distance(200-2.5k) dB {:.2}", tot / c as f64);
}

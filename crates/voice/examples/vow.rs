//! Rust equivalent of tests/vow.js: the mean spectral distance between ten
//! sustained vowels (200 Hz-2.5 kHz), against the baritone voice. Prints the
//! same "mean vowel distance(200-2.5k) dB" line for comparison with node.

use compose::voices::{voice_params, Voice};
use dsp::fft::make_fft;
use sfcore::rng::rng_for;
use sfcore::tuning::Tuning;
use sfcore::SR_F;
use voice::controls::{VoiceNote, VoiceOpts};
use voice::synth::render_voice;

const VW: [&str; 10] = ["iy", "ih", "eh", "ae", "aa", "ao", "ow", "uw", "ah", "er"];
const BANDS: [f64; 17] = [
    200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
];

fn spec(a: &[f32]) -> Vec<f64> {
    const N: usize = 8192;
    let fft = make_fft(N);
    let mut re = vec![0.0f64; N];
    let mut im = vec![0.0f64; N];
    for i in 0..N {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / N as f64).cos();
        re[i] = a[i] as f64 * w;
    }
    fft.run(&mut re, &mut im, false);
    (0..N / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

fn main() {
    let mut sp = Vec::new();
    let mut t = 0.5f64;
    for v in VW {
        sp.push(VoiceNote {
            t0: t,
            t1: t + 1.2,
            midi: 52,
            ph: Some(vec!["hh".to_string(), v.to_string()]),
            nu: None,
            amp: 1.0,
            phrase_start: false,
            phrase_end: false,
            grace: None,
            stress: false,
        });
        t += 1.6;
    }
    let len = ((t + 1.0) * SR_F).ceil() as usize;
    let p = voice_params(Voice::Baritone);
    let mut opts = VoiceOpts { seed: Some(3), rng: rng_for(3, "v"), vib_scale: Some(0.0), no_scoop: true, ..Default::default() };
    let tuning = Tuning::default();

    let start = std::time::Instant::now();
    let x = render_voice(&sp, &p, len, &mut opts, &tuning);
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
            let hi = (b * 1.12 / df) as usize; // matches JS's `<` bound via integer q loop
            let mut e = 0.0;
            let mut q = lo;
            while (q as f64) < b * 1.12 / df {
                if q < p_spec.len() {
                    e += p_spec[q];
                }
                q += 1;
            }
            let _ = hi;
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
    println!("rust     mean vowel distance(200-2.5k) dB {:.2}", tot / c as f64);
}

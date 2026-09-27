//! Benchmarks std vs libm vs the V8-exact port (v8math.rs) on realistic
//! argument ranges, to pick the default (non-"v8"-feature) implementation
//! per function. Run: cargo run --release -p sfcore --example mathbench
//!
//! Argument ranges are drawn from how engine.js actually calls each
//! function (see src/engine.js): sin/cos take an accumulated phase that
//! can run into the hundreds of thousands to low millions of radians over
//! a full render; exp is almost always of a negative argument (envelopes,
//! decays); log is of a positive argument (gauss(), T60 math); atan2 args
//! are near the origin (waveguide allpass delay); pow bases are small
//! (2, 10, ratios near 1) with exponents in roughly [0.4, 2].

use std::hint::black_box;
use std::time::Instant;

struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        // xorshift64*, fine for generating benchmark arguments only.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next_f64() * (hi - lo)
    }
}

const N: usize = 2_000_000;

fn bench_unary(name: &str, xs: &[f64], variants: &[(&str, fn(f64) -> f64)]) {
    for (vname, f) in variants {
        // warm up
        let mut acc = 0.0;
        for &x in xs.iter().take(1000) {
            acc += f(x);
        }
        black_box(acc);
        let t0 = Instant::now();
        let mut acc = 0.0f64;
        for &x in xs {
            acc += black_box(f(black_box(x)));
        }
        let dt = t0.elapsed();
        black_box(acc);
        println!(
            "{name:8} {vname:6} {:7.2} ns/call  (n={}, sum={:.6e})",
            dt.as_nanos() as f64 / xs.len() as f64,
            xs.len(),
            acc
        );
    }
}

fn bench_binary(name: &str, ps: &[(f64, f64)], variants: &[(&str, fn(f64, f64) -> f64)]) {
    for (vname, f) in variants {
        let mut acc = 0.0;
        for &(a, b) in ps.iter().take(1000) {
            acc += f(a, b);
        }
        black_box(acc);
        let t0 = Instant::now();
        let mut acc = 0.0f64;
        for &(a, b) in ps {
            acc += black_box(f(black_box(a), black_box(b)));
        }
        let dt = t0.elapsed();
        black_box(acc);
        println!(
            "{name:8} {vname:6} {:7.2} ns/call  (n={}, sum={:.6e})",
            dt.as_nanos() as f64 / ps.len() as f64,
            ps.len(),
            acc
        );
    }
}

// libm free functions (not the JS-wrapped ones) for direct comparison.
fn libm_sin(x: f64) -> f64 {
    libm::sin(x)
}
fn libm_cos(x: f64) -> f64 {
    libm::cos(x)
}
fn libm_log(x: f64) -> f64 {
    libm::log(x)
}
fn libm_log2(x: f64) -> f64 {
    libm::log2(x)
}
fn libm_log10(x: f64) -> f64 {
    libm::log10(x)
}
fn libm_exp(x: f64) -> f64 {
    libm::exp(x)
}
fn std_sin(x: f64) -> f64 {
    x.sin()
}
fn std_cos(x: f64) -> f64 {
    x.cos()
}
fn std_log(x: f64) -> f64 {
    x.ln()
}
fn std_log2(x: f64) -> f64 {
    x.log2()
}
fn std_log10(x: f64) -> f64 {
    x.log10()
}
fn std_exp(x: f64) -> f64 {
    x.exp()
}
fn v8_sin(x: f64) -> f64 {
    sfcore::v8math::sin(x)
}
fn v8_cos(x: f64) -> f64 {
    sfcore::v8math::cos(x)
}
fn v8_log(x: f64) -> f64 {
    sfcore::v8math::log(x)
}
fn v8_log2(x: f64) -> f64 {
    sfcore::v8math::log2(x)
}
fn v8_log10(x: f64) -> f64 {
    sfcore::v8math::log10(x)
}
fn std_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
fn libm_atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
fn std_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}
fn libm_pow(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}

fn main() {
    let mut r = Lcg(0x9E3779B97F4A7C15);

    // sin/cos: phase up to 1e6 rad, both signs.
    let phases: Vec<f64> = (0..N).map(|_| r.range(-1.0e6, 1.0e6)).collect();
    bench_unary(
        "sin",
        &phases,
        &[("std", std_sin), ("libm", libm_sin), ("v8", v8_sin)],
    );
    bench_unary(
        "cos",
        &phases,
        &[("std", std_cos), ("libm", libm_cos), ("v8", v8_cos)],
    );

    // exp: negative arguments (decay envelopes), roughly [-40, 0].
    let negs: Vec<f64> = (0..N).map(|_| r.range(-40.0, 0.0)).collect();
    bench_unary("exp", &negs, &[("std", std_exp), ("libm", libm_exp)]);

    // log/log2/log10: positive arguments, wide dynamic range (1e-9 .. 1e4).
    let pos: Vec<f64> = (0..N)
        .map(|_| {
            let e = r.range(-9.0, 4.0);
            10f64.powf(e)
        })
        .collect();
    bench_unary(
        "log",
        &pos,
        &[("std", std_log), ("libm", libm_log), ("v8", v8_log)],
    );
    bench_unary(
        "log2",
        &pos,
        &[("std", std_log2), ("libm", libm_log2), ("v8", v8_log2)],
    );
    bench_unary(
        "log10",
        &pos,
        &[("std", std_log10), ("libm", libm_log10), ("v8", v8_log10)],
    );

    // atan2: waveguide allpass, y = p*sin(w0), x = 1-p*cos(w0), p in [0.9,0.9999],
    // w0 small (audio-rate angular freq / SR).
    let atan2_args: Vec<(f64, f64)> = (0..N)
        .map(|_| {
            let p = r.range(0.9, 0.9999);
            let w0 = r.range(0.001, 1.5);
            (p * w0.sin(), 1.0 - p * w0.cos())
        })
        .collect();
    bench_binary(
        "atan2",
        &atan2_args,
        &[("std", std_atan2), ("libm", libm_atan2)],
    );

    // pow: bases 2 and 10 (octave/dB math) and ratios near 1 (t60 scaling),
    // exponents roughly [-3, 3].
    let pow_args: Vec<(f64, f64)> = (0..N)
        .map(|_| {
            let base = match (r.next_f64() * 3.0) as u32 {
                0 => 2.0,
                1 => 10.0,
                _ => r.range(0.3, 3.0),
            };
            (base, r.range(-3.0, 3.0))
        })
        .collect();
    bench_binary("pow", &pow_args, &[("std", std_pow), ("libm", libm_pow)]);
}

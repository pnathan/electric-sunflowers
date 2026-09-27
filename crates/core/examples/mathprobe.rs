fn main() {
    let p = std::env::args().nth(1).unwrap();
    let raw = std::fs::read(p).unwrap();
    let b: Vec<f64> = raw.chunks(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
    let names = ["sin", "cos", "exp", "log", "pow", "log10", "tanh", "atan", "sqrt"];
    let mut ml = [0usize; 9];
    let mut ms = [0usize; 9];
    let n = b.len() / 12;
    for i in 0..n {
        let o = &b[i * 12..i * 12 + 12];
        let (x, p, q) = (o[0], o[1], o[2]);
        let l = [libm::sin(x), libm::cos(x), libm::exp(q * 20.0), libm::log(p), libm::pow(p, q), libm::log10(p), libm::tanh(q), libm::atan(x), libm::sqrt(p)];
        let s = [x.sin(), x.cos(), (q * 20.0).exp(), p.ln(), p.powf(q), p.log10(), q.tanh(), x.atan(), p.sqrt()];
        for k in 0..9 {
            if l[k].to_bits() != o[3 + k].to_bits() { ml[k] += 1; }
            if s[k].to_bits() != o[3 + k].to_bits() { ms[k] += 1; }
        }
    }
    for k in 0..9 { println!("{:6} libm-mismatch {:7} std-mismatch {:7} of {}", names[k], ml[k], ms[k], n); }
}

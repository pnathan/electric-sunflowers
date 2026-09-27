//! Probe sfcore::js sin, cos, exp against node on wide arguments (|x| up to 1e7).
fn main() {
    let raw = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let b: Vec<f64> = raw.chunks(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
    let (mut ms, mut mc, mut me, mut ss, mut sc, mut se) = (0, 0, 0, 0, 0, 0);
    let n = b.len() / 5;
    for i in 0..n {
        let o = &b[i * 5..i * 5 + 5];
        if sfcore::js::sin(o[0]).to_bits() != o[2].to_bits() { ms += 1; }
        if sfcore::js::cos(o[0]).to_bits() != o[3].to_bits() { mc += 1; }
        if sfcore::js::exp(o[1]).to_bits() != o[4].to_bits() { me += 1; }
        if o[0].sin().to_bits() != o[2].to_bits() { ss += 1; }
        if o[0].cos().to_bits() != o[3].to_bits() { sc += 1; }
        if o[1].exp().to_bits() != o[4].to_bits() { se += 1; }
    }
    println!("of {n}: js sin {ms} cos {mc} exp {me} | std sin {ss} cos {sc} exp {se}");
}

//! Helmholtz-motion sweep of the bowed string (design section 9).
//!
//! Single notes: MIDI 55..=90, velocity 0.4/0.6/0.85, two seeds; prints
//! `stable N/216` and the failing notes as `midi:velocity`. Phrases: 30
//! four-note legato phrases across the range; prints `phrase notes N/120`
//! and the failures as `phrase.note:midi`. Criteria in
//! `instruments::violin::helmholtz`.

use instruments::violin::helmholtz::{note_sweep, phrase_sweep};

fn main() {
    let (ok, total, bad) = note_sweep();
    let bad: Vec<String> = bad.iter().map(|(m, v)| format!("{m}:{v}")).collect();
    println!("stable {ok}/{total} {}", bad.join(" "));
    let (ok, total, bad) = phrase_sweep();
    let bad: Vec<String> = bad.iter().map(|(p, i, m)| format!("{p}.{i}:{m}")).collect();
    println!("phrase notes {ok}/{total} ({:.1}%) {}", 100.0 * ok as f64 / total as f64, bad.join(" "));
}

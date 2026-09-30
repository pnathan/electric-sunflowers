//! Prints the arranger pass's view of a song: the compact text in beats that
//! the model reads (`engine::arranger::view`). Makes no model call.
//!
//! Usage: bazel run //crates/engine:arranger_view -- SONG.json [--style KEY] [--seed S]

use engine::arranger::view;
use engine::{arrange_song, VoiceChoice};

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
    let json = std::fs::read_to_string(&path).expect("read song");
    let (mut song, _) = song::wire::normalize_str(&json).expect("normalize");
    if let Some(k) = &style {
        songwriter::styles::apply_style(k, &mut song).expect("style");
    }
    let (prepared, perf) = arrange_song(&song, seed, VoiceChoice::default());
    print!("{}", view(&song, &prepared, &perf));
}

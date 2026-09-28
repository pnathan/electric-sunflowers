//! Engraves a song: writes score.svg and score.png in the current directory.
//!
//! Usage:
//!   score <song.json | demo> [seed] [voice]
//!
//! `demo` uses the engine's demo song. The seed defaults to 1234, the voice
//! to the song's own.

use std::process::ExitCode;
use std::str::FromStr;

use resvg::{tiny_skia, usvg};
use song::Voice;

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let target = args.first().ok_or("usage: score <song.json|demo> [seed] [voice]")?;
    let seed: u64 = match args.get(1) {
        Some(s) => s.parse().map_err(|e| format!("seed {s}: {e}"))?,
        None => 1234,
    };
    let voice = match args.get(2) {
        Some(v) => Some(Voice::from_str(v).map_err(|e| e.to_string())?),
        None => None,
    };
    let song = if target == "demo" {
        engine::demo_song().clone()
    } else {
        let text = std::fs::read_to_string(target).map_err(|e| format!("{target}: {e}"))?;
        let raw: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{target}: {e}"))?;
        let (song, repairs) = song::normalize_value(&raw).map_err(|e| format!("{target}: {e}"))?;
        for r in &repairs {
            eprintln!("repair: {r}");
        }
        song
    };

    let prep = compose::prepare::prepare(&song, seed, voice);
    let score = notation::Score::new(&song, &prep);
    let svg = notation::engrave(&score);
    std::fs::write("score.svg", &svg).map_err(|e| format!("score.svg: {e}"))?;

    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&svg, &opt).map_err(|e| format!("SVG: {e}"))?;
    let size = tree.size().to_int_size();
    let mut pm = tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("empty page")?;
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pm.as_mut());
    pm.save_png("score.png").map_err(|e| format!("score.png: {e}"))?;
    eprintln!(
        "wrote score.svg and score.png ({}x{} px, {} notes, {} systems)",
        size.width(),
        size.height(),
        notation::note_boxes(&score).len(),
        notation::system_boxes(&score).len()
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("score: {e}");
            ExitCode::FAILURE
        }
    }
}

//! Writes the demo song's full score and its violin part view.
//!
//! Usage: fullscore [seed]  (seed defaults to 1).
//! Writes out/fullscore.svg and out/part-violin.svg.

use std::process::ExitCode;
use std::time::Instant;

use notation::full::{FullScore, PartId};
use notation::Sheet;

fn run() -> Result<(), String> {
    let seed: u64 = std::env::args().nth(1).map_or(Ok(1), |s| s.parse().map_err(|e| format!("seed {s}: {e}"))).map_err(|e: String| e)?;

    let song = engine::demo_song().clone();
    let prep = compose::prepare::prepare(&song, seed, None);
    let arr = arrange::arrange(&song, &prep, seed);
    let full = FullScore::new(&song, &prep, &arr);

    std::fs::create_dir_all("out").map_err(|e| format!("out/: {e}"))?;

    let t0 = Instant::now();
    let page = Sheet::Full(full.clone()).page(notation::DEFAULT_WIDTH);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    std::fs::write("out/fullscore.svg", &page.svg).map_err(|e| format!("out/fullscore.svg: {e}"))?;
    eprintln!("wrote out/fullscore.svg ({} staves, {} bars, {} systems, {} columns, {ms:.1} ms)", full.staves.len(), full.bars.len(), page.systems.len(), page.notes.len());

    let violin = Sheet::Part(full, PartId::Violin).page(notation::DEFAULT_WIDTH);
    std::fs::write("out/part-violin.svg", &violin.svg).map_err(|e| format!("out/part-violin.svg: {e}"))?;
    eprintln!("wrote out/part-violin.svg ({} systems)", violin.systems.len());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("fullscore: {e}");
            ExitCode::FAILURE
        }
    }
}

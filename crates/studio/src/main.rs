//! Native song studio: write, render, play, and read a song with lyrics,
//! chords and sheet music.
//!
//! Usage: studio [SONG.json] [--dir DIR] [--open NAME] [--view lyrics|sheet|both]
//!               [--volume 0..1] [--seek SECS] [--play SECS] [--screenshot PNG] [--new]
//!
//! The library is DIR (default ~/Music/sunflower): `<stem>.json` songs with
//! `<stem>.ogg` audio and `<stem>.render.json` sidecars. `--screenshot`
//! runs a script: open the song, wait until it is ready, seek, play for
//! `--play` seconds (logging the position), save the window and quit.
//! `--new` opens the new-song form.

mod app;
mod audio;
mod jobs;
mod library;
mod lyrics;
mod sheetview;

use std::path::PathBuf;
use std::process::ExitCode;

use app::{Options, View};

const USAGE: &str = "usage: studio [SONG.json] [--dir DIR] [--open NAME] [--view lyrics|sheet|both] [--volume V] [--seek SECS] [--play SECS] [--screenshot PNG] [--new]";

fn parse_args(args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut o = Options {
        dir: library::default_dir(),
        song: None,
        open: None,
        view: View::Both,
        screenshot: None,
        seek: None,
        play: 0.0,
        volume: 0.8,
        new_song: false,
    };
    let mut it = args.peekable();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        let num = |name: &str, s: String| s.parse::<f64>().ok().filter(|x| x.is_finite() && *x >= 0.0).ok_or_else(|| format!("{name}: {s:?} is not a number >= 0"));
        match a.as_str() {
            "-h" | "--help" => return Err(USAGE.into()),
            "--dir" => o.dir = PathBuf::from(val("--dir")?),
            "--new" => o.new_song = true,
            "--open" => o.open = Some(val("--open")?),
            "--view" => {
                let v = val("--view")?;
                o.view = View::parse(&v).ok_or_else(|| format!("--view: {v:?} is not lyrics, sheet or both"))?;
            }
            "--screenshot" => o.screenshot = Some(PathBuf::from(val("--screenshot")?)),
            "--seek" => o.seek = Some(num("--seek", val("--seek")?)?),
            "--play" => o.play = num("--play", val("--play")?)?,
            "--volume" => o.volume = num("--volume", val("--volume")?)?.min(1.0) as f32,
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            s => {
                if o.song.is_some() {
                    return Err(format!("only one song path, got {s:?} too\n{USAGE}"));
                }
                o.song = Some(PathBuf::from(s));
            }
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    let opt = match parse_args(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    sfcore::fp::init_pool(None);
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1440.0, 900.0]).with_min_inner_size([640.0, 400.0]).with_title("Sunflower Studio"),
        ..Default::default()
    };
    match eframe::run_native("Sunflower Studio", native, Box::new(move |cc| Ok(Box::new(app::StudioApp::new(cc, opt))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("studio: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<Options, String> {
        parse_args(s.split_whitespace().map(str::to_string))
    }

    #[test]
    fn options_parse() {
        let o = args("song.json --dir /x --view sheet --play 3 --seek 40.5 --screenshot a.png").unwrap();
        assert_eq!(o.song, Some(PathBuf::from("song.json")));
        assert_eq!(o.dir, PathBuf::from("/x"));
        assert_eq!(o.view, View::Sheet);
        assert_eq!((o.play, o.seek), (3.0, Some(40.5)));
        assert!(args("--view nope").is_err());
        assert!(args("--play").is_err());
        assert!(args("--play -1").is_err());
        assert!(args("--bogus").is_err());
        assert!(args("a.json b.json").is_err());
    }
}

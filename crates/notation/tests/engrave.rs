//! Engraves the demo song (3/4), a 12-bar blues (4/4) and a 6/8 song;
//! checks that the SVG parses and renders with resvg, that every sung
//! syllable is printed in order, and that the timing metadata is sane.
//! Writes the PNGs to /tmp/claude-1000/notation-*.png for review.

use std::sync::{Arc, OnceLock};

use compose::prepare::prepare;
use notation::{engrave, note_boxes, system_boxes, Score};
use resvg::{tiny_skia, usvg};
use song::{Song, Voice};

const OUT_DIR: &str = "/tmp/claude-1000";

fn song_of(json: &str) -> Song {
    let v: serde_json::Value = serde_json::from_str(json).expect("fixture is JSON");
    song::normalize_value(&v).expect("fixture normalises").0
}

fn blues() -> Song {
    song_of(include_str!("songs/blues.json"))
}

fn six_eight() -> Song {
    song_of(include_str!("songs/six_eight.json"))
}

fn fonts() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

/// Parses and renders `svg`; returns the pixmap.
fn render(svg: &str) -> tiny_skia::Pixmap {
    let opt = usvg::Options {
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(svg, &opt).expect("SVG parses");
    let size = tree.size().to_int_size();
    let mut pm = tiny_skia::Pixmap::new(size.width(), size.height()).expect("page has a size");
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pm.as_mut());
    pm
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// Text of every `<text class="lyric">` element, in document order.
fn lyrics(svg: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(i) = rest.find(r#"class="lyric">"#) {
        rest = &rest[i + r#"class="lyric">"#.len()..];
        let j = rest.find("</text>").expect("lyric text closes");
        out.push(unescape(&rest[..j]));
        rest = &rest[j..];
    }
    out
}

/// Engraves `song` for `seed` and `voice`, checks it, and returns the SVG.
fn check(song: &Song, seed: u64, voice: Option<Voice>) -> String {
    let prep = prepare(song, seed, voice);
    let score = Score::new(song, &prep);
    let svg = engrave(&score);
    let at = format!("{:?} seed {seed} voice {voice:?}", song.title);

    let want: Vec<String> = prep.comp.lead.iter().map(|n| n.syl.text.clone()).collect();
    assert!(!want.is_empty(), "{at}: no sung notes");
    assert_eq!(
        lyrics(&svg),
        want,
        "{at}: lyrics differ from the sung syllables"
    );

    let boxes = note_boxes(&score);
    assert_eq!(
        boxes.len(),
        svg.matches(r#"<g class="note""#).count(),
        "{at}"
    );
    assert!(boxes.len() >= want.len(), "{at}");
    for w in boxes.windows(2) {
        assert!(w[0].0 <= w[1].0 + 1e-9, "{at}: note times out of order");
    }
    for b in &boxes {
        assert!(b.0 <= b.1 && b.4 > 0.0 && b.5 > 0.0, "{at}: bad box {b:?}");
    }
    let sys = system_boxes(&score);
    assert!(!sys.is_empty(), "{at}");
    for w in sys.windows(2) {
        assert!(w[0].1 <= w[1].0 + 1e-6, "{at}: systems overlap in time");
        assert!(
            w[0].3 + w[0].5 <= w[1].3 + 1e-6,
            "{at}: systems overlap on the page"
        );
    }
    let (t_first, t_last) = (
        prep.comp.lead[0].t0,
        prep.comp.lead[prep.comp.lead.len() - 1].t1,
    );
    assert!((boxes[0].0 - t_first).abs() < 1e-3, "{at}");
    assert!((boxes[boxes.len() - 1].1 - t_last).abs() < 1e-3, "{at}");
    svg
}

/// Renders `svg`, checks that ink was drawn, and writes the PNG.
fn write_png(svg: &str, name: &str) {
    let pm = render(svg);
    let dark = pm.pixels().iter().filter(|p| p.red() < 128).count();
    assert!(dark > 5000, "{name}: only {dark} dark pixels");
    std::fs::create_dir_all(OUT_DIR).expect("output directory");
    let path = format!("{OUT_DIR}/notation-{name}.png");
    pm.save_png(&path).expect("PNG writes");
    std::fs::write(format!("{OUT_DIR}/notation-{name}.svg"), svg).expect("SVG writes");
}

#[test]
fn demo_song_engraves_and_renders() {
    let song = engine::demo_song();
    write_png(&check(song, 1234, None), "demo");
    write_png(&check(song, 1234, Some(Voice::Alto)), "demo-alto");
}

#[test]
fn blues_engraves_and_renders() {
    write_png(&check(&blues(), 7, None), "blues");
}

#[test]
fn six_eight_engraves_and_renders() {
    write_png(&check(&six_eight(), 3, None), "six-eight");
    write_png(&check(&six_eight(), 3, Some(Voice::Bass)), "six-eight-bass");
}

#[test]
fn every_voice_and_many_seeds_engrave() {
    let songs = [engine::demo_song().clone(), blues(), six_eight()];
    for s in &songs {
        for seed in 0..12u64 {
            for &v in Voice::ALL {
                let svg = check(s, seed, Some(v));
                if seed == 0 && v == Voice::Tenor {
                    render(&svg);
                }
            }
        }
    }
}

#[test]
fn narrow_page_wraps_lines() {
    let song = engine::demo_song();
    let prep = prepare(song, 1234, None);
    let wide = Score::new(song, &prep);
    let narrow = Score::new(song, &prep).with_width(360.0);
    assert!(system_boxes(&narrow).len() > system_boxes(&wide).len());
    for b in note_boxes(&narrow) {
        assert!(
            b.2 >= 0.0 && b.2 + b.4 <= 360.0 + 1e-6,
            "box {b:?} leaves the page"
        );
    }
    write_png(&engrave(&narrow), "demo-narrow");
}

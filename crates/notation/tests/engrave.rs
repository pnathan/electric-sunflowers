//! Engraves the demo song (3/4), a 12-bar blues (4/4) and a 6/8 song;
//! checks that the SVG parses and renders with resvg, that every sung
//! syllable is printed in order, and that the timing metadata is sane.
//! Writes the PNGs to /tmp/claude-1000/notation-*.png for review.

use std::sync::{Arc, OnceLock};

use compose::prepare::prepare;
use notation::{engrave, note_boxes, system_boxes, Score};
use resvg::{tiny_skia, usvg};
use song::{Song, Voice};

/// Where the review renders go: Bazel's undeclared test outputs when set
/// (the test sandbox is read-only elsewhere), else /tmp/claude-1000.
fn out_dir() -> String {
    std::env::var("TEST_UNDECLARED_OUTPUTS_DIR").unwrap_or_else(|_| "/tmp/claude-1000".to_string())
}

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
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("output directory");
    let path = format!("{dir}/notation-{name}.png");
    pm.save_png(&path).expect("PNG writes");
    std::fs::write(format!("{dir}/notation-{name}.svg"), svg).expect("SVG writes");
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

/// A melisma prints its syllable once, under the first note; each later
/// note gets an extension line in the lyric row (a 0.9 px stroke; the
/// syllables here are whole words, so no hyphen uses that stroke); and a
/// slur joins the notes. The same words with no melisma draw none.
#[test]
fn melisma_prints_one_lyric_and_an_extension() {
    let of = |syl: &str| {
        song_of(&format!(
            r#"{{"schema_version":2,"title":"Melisma","key":"G","mode":"major","meter":"4/4","tempo":92,
               "sections":[{{"type":"verse","lines":[{{"syl":"{syl}","chords":["G","C","D","G"]}}]}}]}}"#
        ))
    };
    let ext = |svg: &str| svg.matches(r#"stroke-width="0.90""#).count();
    let song = of("*glo~3 *hal~4 *sing~ out");
    let prep = prepare(&song, 4, None);
    let score = Score::new(&song, &prep);
    let svg = engrave(&score);
    let conts = prep
        .comp
        .lead
        .iter()
        .filter(|n| n.syl.is_continuation())
        .count();
    assert_eq!(conts, 6);
    assert_eq!(lyrics(&svg), ["glo", "hal", "sing", "out"]);
    assert!(ext(&svg) >= conts, "{} extension lines", ext(&svg));
    assert_eq!(
        note_boxes(&score).len(),
        svg.matches(r#"<g class="note""#).count()
    );
    write_png(&svg, "melisma");

    let plain = of("*glo *hal *sing out");
    let svg = engrave(&Score::new(&plain, &prepare(&plain, 4, None)));
    assert_eq!(lyrics(&svg), ["glo", "hal", "sing", "out"]);
    assert_eq!(ext(&svg), 0);
}

/// C major, a chorus in D major, then a copied verse in E major.
fn modulating() -> Song {
    song_of(
        r#"{"schema_version":2,"key":"C","mode":"major","meter":"4/4","tempo":100,
        "sections":[
            {"type":"verse","lines":[{"syl":"one *two three *four","chords":["C G"]}]},
            {"type":"chorus","key":"D","lines":[
                {"syl":"*five *six *seven *eight","chords":["D A"]},
                {"syl":"*nine *ten *e-le-ven","chords":["G D"]}]},
            {"type":"verse","same":true,"key":"E"}
        ]}"#,
    )
}

/// Sharps in the signature of a major key on pitch class `pc`.
fn major_fifths(pc: i32) -> i32 {
    let f = (pc * 7).rem_euclid(12);
    if f > 6 {
        f - 12
    } else {
        f
    }
}

/// Each system draws the key signature of its own section, and a section
/// that changes key says so in its label. The melody lies in each section's
/// scale, so no note carries an accidental and every sharp or flat glyph in
/// the SVG belongs to a signature.
#[test]
fn each_system_draws_its_sections_signature() {
    let song = modulating();
    for seed in [1u64, 2, 3, 4] {
        let prep = prepare(&song, seed, None);
        let score = Score::new(&song, &prep);
        let svg = engrave(&score);
        let tl = &prep.timeline;
        let form = &prep.form;
        let starts: Vec<f64> = form
            .sections
            .iter()
            .map(|s| tl.to_time((s.start_bar as i32 * form.bpb()) as f64))
            .collect();
        let (mut sharps, mut flats) = (0i32, 0i32);
        let mut per_section = vec![0usize; starts.len()];
        for b in system_boxes(&score) {
            let si = starts.iter().rposition(|&t| t <= b.0 + 1e-6).unwrap_or(0);
            per_section[si] += 1;
            let f = major_fifths(form.sections[si].key.0.get() as i32);
            sharps += f.max(0);
            flats += (-f).max(0);
        }
        // One system per lyric line (or run of bars): the chorus has two.
        assert_eq!(per_section, vec![1, 2, 1], "seed {seed}");
        let count = |g: &str| svg.matches(&format!("href=\"#g-{g}\"")).count() as i32;
        assert_eq!(count("accidentalSharp"), sharps, "seed {seed}");
        assert_eq!(count("accidentalFlat"), flats, "seed {seed}");
        // Changed keys are named on the section label; the opening key is not.
        assert!(
            !svg.contains("Key:") || svg.contains("(Key: "),
            "seed {seed}"
        );
        assert_eq!(svg.matches("(Key: ").count(), 2, "seed {seed}");
    }
}

// ---------------------------------------------------------------------
// Bass clef and multi-bar rests on the lead sheet (issue #10).
// ---------------------------------------------------------------------

/// A one-verse song in `key`, with an intro of the given chords.
fn intro_song(key: &str, intro: &[&str]) -> Song {
    let chords = intro
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let intro_sec = if intro.is_empty() {
        String::new()
    } else {
        format!(r#"{{"type":"intro","chords":[{chords}]}},"#)
    };
    song_of(&format!(
        r#"{{"schema_version":2,"title":"Rests","key":"{key}","mode":"major","meter":"4/4","tempo":100,
           "sections":[{intro_sec}{{"type":"verse","lines":[{{"syl":"*one *two *three *four","chords":["{key}","{key}","{key}","{key}"]}}]}}]}}"#
    ))
}

fn svg_of(song: &Song, seed: u64, voice: Voice) -> String {
    let prep = prepare(song, seed, Some(voice));
    engrave(&Score::new(song, &prep))
}

fn count(svg: &str, pat: &str) -> usize {
    svg.matches(pat).count()
}

/// Horizontal ledger lines: a 1.28 px stroke with equal y ends.
fn ledger_lines(svg: &str) -> usize {
    svg.split("<line ")
        .skip(1)
        .filter(|l| {
            let attr = |k: &str| {
                let i = l.find(&format!("{k}=\"")).unwrap() + k.len() + 2;
                l[i..i + l[i..].find('"').unwrap()].to_string()
            };
            attr("stroke-width") == "1.28" && attr("y1") == attr("y2")
        })
        .count()
}

/// y of every `href="#g-<name>"` use's translate, in document order.
fn glyph_ys(svg: &str, name: &str) -> Vec<String> {
    let pat = format!("href=\"#g-{name}\" transform=\"translate(");
    svg.split(&pat)
        .skip(1)
        .map(|r| {
            let t = &r[..r.find(')').unwrap()];
            t.split(',').nth(1).unwrap().to_string()
        })
        .collect()
}

#[test]
fn bass_voice_draws_the_f_clef_only() {
    let song = intro_song("C", &[]);
    let svg = svg_of(&song, 3, Voice::Bass);
    render(&svg);
    assert!(count(&svg, r#"class="fclef""#) >= 1);
    assert_eq!(count(&svg, "#g-gClef"), 0, "no G clef for a bass singer");
    // The clef is on the F line: second line from the top (y = 8 px).
    let i = svg.find(r#"class="fclef""#).unwrap();
    assert!(svg[i..].contains("<path"));
}

#[test]
fn other_voices_keep_their_clefs() {
    let song = intro_song("C", &[]);
    for v in [Voice::Baritone, Voice::Tenor] {
        let svg = svg_of(&song, 3, v);
        assert!(count(&svg, "#g-gClef8vb") >= 1, "{v:?}");
        assert_eq!(count(&svg, r#"class="fclef""#), 0, "{v:?}");
    }
    for v in [Voice::Alto, Voice::Soprano] {
        let svg = svg_of(&song, 3, v);
        assert!(count(&svg, "#g-gClef\"") >= 1, "{v:?}");
        assert_eq!(count(&svg, "#g-gClef8vb"), 0, "{v:?}");
        assert_eq!(count(&svg, r#"class="fclef""#), 0, "{v:?}");
    }
}

/// Written at concert pitch, the bass part needs fewer ledger lines than
/// the same song on the treble-8vb staff a baritone uses.
#[test]
fn bass_staff_needs_few_ledger_lines() {
    let song = engine::demo_song();
    let bass = ledger_lines(&svg_of(song, 1234, Voice::Bass));
    let bari = ledger_lines(&svg_of(song, 1234, Voice::Baritone));
    assert!(bass <= bari, "bass {bass} vs baritone {bari}");
}

/// The signature glyph heights of the first system (drawn before any note).
fn signature_ys(svg: &str, name: &str) -> Vec<f64> {
    let end = svg.find(r#"<g class="note""#).unwrap();
    glyph_ys(&svg[..end], name)
        .iter()
        .map(|y| y.parse().unwrap())
        .collect()
}

#[test]
fn key_signature_sits_lower_on_a_bass_staff() {
    // Top-line-relative y of each accidental, in signature order.
    let sharps_bass = [8.0, 20.0, 4.0, 16.0, 28.0, 12.0, 24.0];
    let sharps_treble = [0.0, 12.0, -4.0, 8.0, 20.0, 4.0, 16.0];
    let flats_bass = [24.0, 12.0, 28.0, 16.0, 32.0, 20.0, 36.0];
    let flats_treble = [16.0, 4.0, 20.0, 8.0, 24.0, 12.0, 28.0];
    let song = intro_song("D", &[]);
    let mut checked = 0;
    for seed in 0..8u64 {
        for (v, sh, fl) in [
            (Voice::Bass, &sharps_bass, &flats_bass),
            (Voice::Baritone, &sharps_treble, &flats_treble),
        ] {
            let svg = svg_of(&song, seed, v);
            let s = signature_ys(&svg, "accidentalSharp");
            let f = signature_ys(&svg, "accidentalFlat");
            assert_eq!(s, sh[..s.len()], "{v:?} seed {seed} sharps");
            assert_eq!(f, fl[..f.len()], "{v:?} seed {seed} flats");
            checked += s.len() + f.len();
        }
    }
    assert!(checked > 0, "no key signature was drawn");
}

#[test]
fn multi_bar_rest_is_one_bar_with_a_count() {
    let song = intro_song("C", &["C", "G", "Am", "F", "C", "G", "F", "C"]);
    let svg = svg_of(&song, 5, Voice::Baritone);
    render(&svg);
    assert_eq!(count(&svg, r#"class="multirest""#), 1);
    assert_eq!(count(&svg, r#"class="multirest-count""#), 1);
    assert!(svg.contains(r#"data-bars="8""#));
    assert!(svg.contains(r#"font-weight="bold" text-anchor="middle">8</text>"#));
    // No per-bar whole rests for the eight bars.
    assert_eq!(count(&svg, "#g-restWhole"), 0);
    // Every chord of the run is printed, in order, before the verse's.
    let chords: Vec<&str> = svg
        .split(r#"class="chord""#)
        .skip(1)
        .map(|r| {
            let a = r.find('>').unwrap() + 1;
            &r[a..a + r[a..].find('<').unwrap()]
        })
        .collect();
    // The voice sets the key; the pattern is what matters.
    let i: Vec<usize> = [0, 1, 2, 3, 4, 5, 6, 7]
        .iter()
        .map(|&k| chords[..8].iter().position(|c| c == &chords[k]).unwrap())
        .collect();
    assert_eq!(i, [0, 1, 2, 3, 0, 1, 3, 0], "{:?}", &chords[..8]);
}

/// The rect width of the first multi-bar rest.
fn multirest_width(svg: &str) -> f64 {
    let i = svg.find(r#"class="multirest""#).unwrap();
    let r = &svg[i..];
    let w = &r[r.find(" width=\"").unwrap() + 8..];
    w[..w.find('"').unwrap()].parse().unwrap()
}

#[test]
fn multi_bar_rest_width_does_not_depend_on_length() {
    let two = svg_of(&intro_song("C", &["C", "G"]), 5, Voice::Baritone);
    let eight = svg_of(
        &intro_song("C", &["C", "G", "Am", "F", "C", "G", "F", "C"]),
        5,
        Voice::Baritone,
    );
    assert!(two.contains(r#"data-bars="2""#));
    let (a, b) = (multirest_width(&two), multirest_width(&eight));
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn a_single_rest_bar_is_a_whole_rest() {
    let song = intro_song("C", &["C"]);
    let svg = svg_of(&song, 5, Voice::Baritone);
    assert_eq!(count(&svg, r#"class="multirest""#), 0);
    assert_eq!(count(&svg, "#g-restWhole"), 1);
}

#[test]
fn boxes_cover_the_multi_bar_rest() {
    let song = intro_song("C", &["C", "G", "Am", "F", "C", "G", "F", "C"]);
    let prep = prepare(&song, 5, None);
    let score = Score::new(&song, &prep);
    let sys = system_boxes(&score);
    // The intro is the first system and spans its eight bars (100 bpm 4/4).
    assert!(
        (sys[0].1 - sys[0].0 - 8.0 * 4.0 * 0.6).abs() < 1e-3,
        "{:?}",
        sys[0]
    );
    assert!(sys[0].4 > 0.0 && sys[0].2 + sys[0].4 <= score.width + 1e-6);
    // The verse follows without a gap.
    assert!((sys[1].0 - sys[0].1).abs() < 1e-3);
    let first_note = note_boxes(&score)[0].0;
    assert!(first_note >= sys[0].1 - 1e-6);
}

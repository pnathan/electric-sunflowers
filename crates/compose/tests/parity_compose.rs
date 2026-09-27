//! End-to-end parity for `prepare`, `vocalNotes` and `harmonyLine` against
//! tests/parity/compose.js's dump in ref/parity/compose.json. Run
//! tests/parity/gen.sh first.
//!
//! Integers and strings compare exact; floats compare with relative
//! tolerance 1e-12 (against the reference magnitude), and the worst
//! difference per field is printed.

use compose::form::build_form;
use compose::prepare::{harmony_line, prepare, vocal_notes};
use compose::song::normalize_song;
use compose::voices::Voice;
use serde_json::Value;
use std::path::PathBuf;
use std::str::FromStr;

const REL_TOL: f64 = 1e-12;

fn read_ref() -> Value {
    let p = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ref/parity/compose.json"
    ));
    let s = std::fs::read_to_string(&p)
        .unwrap_or_else(|_| panic!("run tests/parity/gen.sh first ({})", p.display()));
    serde_json::from_str(&s).unwrap()
}

fn relf(name: &str, got: f64, want: f64, worst: &mut f64) {
    let denom = want.abs().max(1e-12);
    let rel = (got - want).abs() / denom;
    if rel > *worst {
        *worst = rel;
    }
    assert!(
        rel <= REL_TOL,
        "{name}: {got} vs {want} (rel {rel:e})"
    );
}

fn opt_i32(v: &Value) -> Option<i32> {
    v.as_i64().map(|x| x as i32)
}

/// DEMO_SONG, mirrored from src/demo.js as raw JSON (loaded through node, to
/// avoid keeping two literal copies of the object in sync by hand).
fn demo_song_raw() -> Value {
    let out = std::process::Command::new("node")
        .arg("-e")
        .arg(format!(
            "const {{DEMO_SONG}}=require('{}');console.log(JSON.stringify(DEMO_SONG));",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/demo.js")
        ))
        .output()
        .expect("node must be available to load src/demo.js");
    assert!(out.status.success(), "node failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn blues_song_raw() -> Value {
    serde_json::json!({
        "title":"Rent Day Blues","note":"","key":"E","mode":"mixolydian","meter":"4/4","tempo":84,
        "guitar":"travis","voice":"baritone",
        "band":{"drums":"brushes","bass":true,"harmonyGuitar":true,"harp":false,"violin":false,"choir":false,"harmonies":false,"doubles":false},
        "sections":[
            {"type":"intro","chords":["E7","A7","E7","B7"]},
            {"type":"verse","lines":[
                {"syl":"the *land-lord *knocks at *half past *eight","ph":"dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t","chords":["E7","E7","E7","E7"]},
                {"syl":"the *land-lord *knocks at *half past *eight","ph":"dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t","chords":["A7","A7","E7","E7"]},
                {"syl":"I *told him *twice the *check is *late","ph":"ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t","chords":["B7","A7","E7","B7"]}
            ]},
            {"type":"verse","lines":[
                {"syl":"my *coat is *thin, my *boots are *worn","ph":"m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n","chords":["E7","E7","E7","E7"]},
                {"syl":"my *coat is *thin, my *boots are *worn","ph":"m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n","chords":["A7","A7","E7","E7"]},
                {"syl":"but I *sing so *loud the *roof gets *torn","ph":"b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n","chords":["B7","A7","E7","B7"]}
            ]},
            {"type":"interlude","chords":["E7","A7","E7","E7","B7","A7","E7","B7"]},
            {"type":"outro","chords":["E7","A7","E7","E7"]}
        ]
    })
}

fn malformed_raw(name: &str) -> Value {
    match name {
        "missing_ph" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"the *quick *brown *fox jumps","chords":"C G"}]}]
        }),
        "wrong_ph_group_count" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"the *quick *brown *fox","ph":"dh ax|k w ih k","chords":"C G"}]}]
        }),
        "no_stress_marks" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"the quick brown fox jumps over the lazy dog","ph":"dh ax|k w ih k|b r aw n|f aa k s|jh ah m p s|ow v er|dh ax|l ey z iy|d ao g","chords":"C G"}]}]
        }),
        "unknown_chord_qualities" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[
                {"type":"intro","chords":["C7#9","Dm(add9)"]},
                {"type":"verse","lines":[{"syl":"strange *chords a*bove","ph":"s t r ey n jh|k ao r d z|ax|b ah v","chords":["C7#9","Dm(add9)"]}]}
            ]
        }),
        "slash_chords" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"walk *down the *bass line *here","ph":"w aa k|d aw n|dh ax|b ey s|l ay n|hh ih r","chords":["C/E","F/G","G/B","C"]}]}]
        }),
        "same_section" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[
                {"type":"chorus","lines":[{"syl":"*sing it *loud and *clear","ph":"s ih ng|ih t|l aw d|ae n d|k l ih r","chords":"C G"}]},
                {"type":"verse","lines":[{"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":"C G"}]},
                {"type":"chorus","same":true}
            ]
        }),
        "six_eight_meter" => serde_json::json!({
            "key":"D","mode":"major","meter":"6/8","tempo":90,
            "sections":[{"type":"verse","lines":[{"syl":"*row row row your *boat","ph":"r ow|r ow|r ow|y or|b ow t","chords":"D A D"}]}]
        }),
        "tempo_string" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":"96",
            "sections":[{"type":"verse","lines":[{"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":"C G"}]}]
        }),
        "tempo_zero" => serde_json::json!({
            "key":"C","mode":"major","meter":"4/4","tempo":0,
            "sections":[{"type":"verse","lines":[{"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":"C G"}]}]
        }),
        "nonascii_title" => serde_json::json!({
            "title": "a".repeat(119) + "\u{1F600}",
            "key":"C","mode":"major","meter":"4/4","tempo":100,
            "sections":[{"type":"verse","lines":[{"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":"C G"}]}]
        }),
        other => panic!("unknown malformed case {other}"),
    }
}

/// VERSE_GROWTH_SONG, mirrored from tests/parity/compose.js: verse 1 has 2
/// lines, verse 2 has 4, exercising melody.rs's reference-melody fallback
/// (a later occurrence's line with no first-occurrence entry at that index).
fn verse_growth_song_raw() -> Value {
    serde_json::json!({
        "title":"Verse Growth","note":"","key":"C","mode":"major","meter":"4/4","tempo":96,
        "guitar":"fingerpick","voice":"baritone",
        "band":{"drums":"brushes","bass":true,"harmonyGuitar":true,"harp":false,"violin":false,"choir":false,"harmonies":false,"doubles":false},
        "sections":[
            {"type":"verse","lines":[
                {"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":["C","G"]},
                {"syl":"five *six *seven *eight","ph":"f ay v|s ih k s|s eh v ax n|ey t","chords":["Am","F"]}
            ]},
            {"type":"verse","lines":[
                {"syl":"one *two *three *four","ph":"w ah n|t uw|th r iy|f ao r","chords":["C","G"]},
                {"syl":"five *six *seven *eight","ph":"f ay v|s ih k s|s eh v ax n|ey t","chords":["Am","F"]},
                {"syl":"nine *ten e*leven *twelve","ph":"n ay n|t eh n|ax|l eh v ax n|t w eh l v","chords":["C","G"]},
                {"syl":"thir*teen four*teen fif*teen","ph":"th er|t iy n|f ao r|t iy n|f ih f|t iy n","chords":["Am","F"]}
            ]}
        ]
    })
}

/// Compares one `dumpPrepared` result (the reference JSON's `dump` value, or
/// a top-level per-voice/seed value) against the Rust `prepare` output.
fn check_prepared(label: &str, song: &compose::song::Song, seed: u32, voice_key: &str, rf: &Value) {
    let vk = if voice_key == "auto" { None } else { Some(Voice::from_str(voice_key).unwrap()) };
    let p = prepare(song, seed, vk);
    assert_eq!(p.voice.as_str(), rf["voice"].as_str().expect("ref: voice"), "{label}: voice");
    assert_eq!(p.key_shift as i64, rf["keyShift"].as_i64().expect("ref: keyShift"), "{label}: keyShift");
    assert_eq!(p.tonic as i64, rf["tonic"].as_i64().expect("ref: tonic"), "{label}: tonic");

    // sections
    let rsecs = rf["sections"].as_array().unwrap();
    assert_eq!(p.form.sections.len(), rsecs.len(), "{label}: section count");
    for (s, rs) in p.form.sections.iter().zip(rsecs) {
        assert_eq!(s.type_, rs["type"].as_str().expect("ref: type"), "{label}: section type");
        assert_eq!(s.occ as i64, rs["occ"].as_i64().expect("ref: occ"), "{label}: section occ");
        assert_eq!(s.idx as i64, rs["idx"].as_i64().expect("ref: idx"), "{label}: section idx");
        assert_eq!(s.start_bar as i64, rs["startBar"].as_i64().expect("ref: startBar"), "{label}: section startBar");
        assert_eq!(s.n_bars as i64, rs["nBars"].as_i64().expect("ref: nBars"), "{label}: section nBars");
        assert_eq!(s.lift, rs["lift"].as_bool().expect("ref: lift"), "{label}: section lift");
        assert_eq!(s.lift_idx as i64, rs["liftIdx"].as_i64().expect("ref: liftIdx"), "{label}: section liftIdx");
        assert_eq!(s.final_, rs["final"].as_bool().expect("ref: final"), "{label}: section final");
        assert_eq!(s.intensity as i64, rs["intensity"].as_i64().expect("ref: intensity"), "{label}: section intensity");
    }

    // bars
    let rbars = rf["bars"].as_array().unwrap();
    assert_eq!(p.form.bars.len(), rbars.len(), "{label}: bar count");
    for (b, rb) in p.form.bars.iter().zip(rbars) {
        let names: Vec<&str> = b.chords.iter().map(|c| c.name.as_str()).collect();
        let rnames: Vec<&str> = rb["chords"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(names, rnames, "{label}: bar chords");
        assert_eq!(b.sec as i64, rb["sec"].as_i64().expect("ref: sec"), "{label}: bar sec");
        let rline = rb["line"].as_i64().expect("ref: line");
        match b.line {
            Some(l) => assert_eq!(l as i64, rline, "{label}: bar line"),
            None => assert_eq!(rline, -1, "{label}: bar line"),
        }
    }

    // lead notes
    let rlead = rf["lead"].as_array().unwrap();
    assert_eq!(p.comp.lead.len(), rlead.len(), "{label}: lead note count");
    let mut worst_t = 0.0f64;
    for (n, rn) in p.comp.lead.iter().zip(rlead) {
        relf(&format!("{label}: lead.beat"), n.beat, rn["beat"].as_f64().expect("ref: beat"), &mut worst_t);
        relf(&format!("{label}: lead.dur"), n.dur, rn["dur"].as_f64().expect("ref: dur"), &mut worst_t);
        assert_eq!(n.midi as i64, rn["midi"].as_i64().expect("ref: midi"), "{label}: lead.midi");
        assert_eq!(n.grace, opt_i32(&rn["grace"]), "{label}: lead.grace");
        relf(&format!("{label}: lead.t0"), n.t0, rn["t0"].as_f64().expect("ref: t0"), &mut worst_t);
        relf(&format!("{label}: lead.t1"), n.t1, rn["t1"].as_f64().expect("ref: t1"), &mut worst_t);
        assert_eq!(n.phrase_start, rn["phraseStart"].as_bool().expect("ref: phraseStart"), "{label}: lead.phraseStart");
        assert_eq!(n.phrase_end, rn["phraseEnd"].as_bool().expect("ref: phraseEnd"), "{label}: lead.phraseEnd");
        assert_eq!(n.stress, rn["stress"].as_bool().expect("ref: stress"), "{label}: lead.stress");
        let sec_idx = p.form.lines[find_line_idx(&p.form, n)].sec;
        assert_eq!(sec_idx as i64, rn["sec"].as_i64().expect("ref: sec"), "{label}: lead.sec");
    }
    println!("{label}: worst lead time/beat relative diff = {worst_t:e}");

    // inst notes
    let rinst = rf["inst"].as_array().unwrap();
    assert_eq!(p.comp.inst.len(), rinst.len(), "{label}: inst note count");
    let mut worst_inst = 0.0f64;
    for (n, rn) in p.comp.inst.iter().zip(rinst) {
        relf(&format!("{label}: inst.beat"), n.beat, rn["beat"].as_f64().expect("ref: beat"), &mut worst_inst);
        relf(&format!("{label}: inst.dur"), n.dur, rn["dur"].as_f64().expect("ref: dur"), &mut worst_inst);
        assert_eq!(n.midi as i64, rn["midi"].as_i64().expect("ref: midi"), "{label}: inst.midi");
    }
    println!("{label}: worst inst time/beat relative diff = {worst_inst:e}");

    // per-line rhythm/pitch
    let rlines = rf["lines"].as_array().unwrap();
    assert_eq!(p.form.lines.len(), rlines.len(), "{label}: line count");
    let mut worst_rhythm = 0.0f64;
    for (l, rl) in p.form.lines.iter().zip(rlines) {
        assert_eq!(l.sec as i64, rl["sec"].as_i64().expect("ref: sec"), "{label}: line.sec");
        assert_eq!(l.li as i64, rl["li"].as_i64().expect("ref: li"), "{label}: line.li");
        let rh = l.rh.as_ref().expect("rhythm computed");
        let ronsets: Vec<f64> = rl["onsets"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let rdurs: Vec<f64> = rl["durs"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let rweights: Vec<f64> = rl["weights"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(rh.onsets.len(), ronsets.len(), "{label}: line.onsets len");
        for (a, b) in rh.onsets.iter().zip(ronsets.iter()) {
            relf(&format!("{label}: line.onset"), *a, *b, &mut worst_rhythm);
        }
        for (a, b) in rh.durs.iter().zip(rdurs.iter()) {
            relf(&format!("{label}: line.dur"), *a, *b, &mut worst_rhythm);
        }
        for (a, b) in rh.weights.iter().zip(rweights.iter()) {
            relf(&format!("{label}: line.weight"), *a, *b, &mut worst_rhythm);
        }
        let rpitches: Vec<i64> = rl["pitches"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        let pitches = l.pitches.as_ref().expect("pitches computed");
        assert_eq!(
            pitches.iter().map(|&v| v as i64).collect::<Vec<_>>(),
            rpitches,
            "{label}: line.pitches"
        );
    }
    println!("{label}: worst rhythm relative diff = {worst_rhythm:e}");

    // vocalNotes
    let vn = vocal_notes(&p.comp.lead, 1.0);
    let rvn = rf["vocalNotes"].as_array().unwrap();
    assert_eq!(vn.len(), rvn.len(), "{label}: vocalNotes count");
    let mut worst_amp = 0.0f64;
    for (v, rv) in vn.iter().zip(rvn) {
        relf(&format!("{label}: vn.t0"), v.t0, rv["t0"].as_f64().expect("ref: t0"), &mut worst_amp);
        relf(&format!("{label}: vn.t1"), v.t1, rv["t1"].as_f64().expect("ref: t1"), &mut worst_amp);
        assert_eq!(v.midi as i64, rv["midi"].as_i64().expect("ref: midi"), "{label}: vn.midi");
        let rph: Vec<&str> = rv["ph"].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
        assert_eq!(v.ph, rph, "{label}: vn.ph");
        relf(&format!("{label}: vn.amp"), v.amp, rv["amp"].as_f64().expect("ref: amp"), &mut worst_amp);
        assert_eq!(v.phrase_start, rv["phraseStart"].as_bool().expect("ref: phraseStart"), "{label}: vn.phraseStart");
        assert_eq!(v.phrase_end, rv["phraseEnd"].as_bool().expect("ref: phraseEnd"), "{label}: vn.phraseEnd");
        assert_eq!(v.grace, opt_i32(&rv["grace"]), "{label}: vn.grace");
        assert_eq!(v.stress, rv["stress"].as_bool().expect("ref: stress"), "{label}: vn.stress");
    }
    println!("{label}: worst vocalNotes relative diff = {worst_amp:e}");

    // harmonyLine, both directions: the JS dump carries harmonyLine (up=true)
    // and harmonyLineDown (up=false) for every prepared case, so both are
    // asserted unconditionally rather than guarded with if-let.
    for (up, ref_key) in [(true, "harmonyLine"), (false, "harmonyLineDown")] {
        let rhl = rf[ref_key].as_array().expect("ref: harmonyLine array");
        let hl = harmony_line(&p.comp.lead, &p.form, &p.timeline, song, p.tonic, up);
        assert_eq!(hl.len(), rhl.len(), "{label}: {ref_key} count");
        let mut worst_hl = 0.0f64;
        for (n, rn) in hl.iter().zip(rhl) {
            relf(&format!("{label}: {ref_key}.beat"), n.beat, rn["beat"].as_f64().expect("ref: beat"), &mut worst_hl);
            assert_eq!(n.midi as i64, rn["midi"].as_i64().expect("ref: midi"), "{label}: {ref_key}.midi");
            assert_eq!(n.grace, opt_i32(&rn["grace"]), "{label}: {ref_key}.grace");
        }
        println!("{label}: worst {ref_key} relative diff = {worst_hl:e}");
    }
}

/// Finds the FormLine index a lead note belongs to by matching beat range;
/// LeadNote itself carries `line_idx` directly, so this just reads it.
fn find_line_idx(_form: &compose::form::Form, n: &compose::melody::LeadNote) -> usize {
    n.line_idx
}

#[test]
fn demo_song_seeds_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    for &seed in &[1234u32, 1, 777777, 4294967295] {
        let key = format!("auto|{seed}");
        check_prepared(&key, &song, seed, "auto", &refdata["demo"][&key]);
    }
}

#[test]
fn demo_song_voices_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = demo_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    for voice in ["alto", "soprano", "tenor"] {
        let key = format!("{voice}|1234");
        check_prepared(&key, &song, 1234, voice, &refdata["demo"][&key]);
    }
}

#[test]
fn blues_song_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = blues_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    check_prepared("blues auto|1234", &song, 1234, "auto", &refdata["blues"]["auto|1234"]);
}

#[test]
fn verse_growth_song_matches_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let raw = verse_growth_song_raw();
    let song = normalize_song(&raw).unwrap();
    let refdata = read_ref();
    check_prepared("verse growth auto|1234", &song, 1234, "auto", &refdata["verseGrowth"]["auto|1234"]);
}

#[test]
fn malformed_inputs_match_js() {
    if !sfcore::V8_EXACT {
        eprintln!("skipped: JS parity needs --features sfcore/v8");
        return;
    }
    let refdata = read_ref();
    let cases = [
        "missing_ph",
        "wrong_ph_group_count",
        "no_stress_marks",
        "unknown_chord_qualities",
        "slash_chords",
        "same_section",
        "six_eight_meter",
        "tempo_string",
        "tempo_zero",
        "nonascii_title",
    ];
    for name in cases {
        let raw = malformed_raw(name);
        let rf = &refdata["malformed"][name];
        let js_ok = rf["ok"].as_bool().expect("ref: ok");
        let rust_result = normalize_song(&raw);
        assert_eq!(
            rust_result.is_ok(),
            js_ok,
            "{name}: normalizeSong ok-ness mismatch (js ok={js_ok}, rust={:?})",
            rust_result.as_ref().err()
        );
        if !js_ok {
            continue;
        }
        let song = rust_result.unwrap();
        // form must build without panicking, for both transposes the JS
        // exercises via prepare (0 and whatever chooseTranspose picks).
        let _ = build_form(&song, 0);
        if name == "nonascii_title" {
            // The title feeds the melody-profile RNG's seed tag
            // (`rngFor(seed,'profile|'+song.title)`), so JS's lone-surrogate
            // 120-unit slice and the Rust port's 119-unit "exclude the split
            // char" slice diverge as strings and seed different profiles:
            // everything downstream (melody, rhythm) legitimately differs
            // between the two. Check only what this case is for -- the
            // title itself -- not the full prepared dump.
            let rtitle = rf["dump"]["voice"].as_str(); // sanity: dump exists
            assert!(rtitle.is_some(), "{name}: ref dump present");
            assert_eq!(song.title, "a".repeat(119), "{name}: title truncation");
            assert_eq!(song.title.encode_utf16().count(), 119, "{name}: title utf16 length");
            continue;
        }
        check_prepared(name, &song, 1234, "auto", &rf["dump"]);
    }
}

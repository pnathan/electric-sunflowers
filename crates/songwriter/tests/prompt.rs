//! Prompt content and the write path, against a mock Claude (no network).

use std::cell::RefCell;

use songwriter::claude::{Claude, ClaudeError, Effort, Reply, Request};
use songwriter::prompt::{persona_age, song_prompt, song_prompt_with, REGISTERS};
use songwriter::styles::{style_direction, DuetFit, FormId, StyleId, STYLES};
use songwriter::{
    write_song, write_song_with, DuetRequest, Rng, WriteOptions, WriteRequest, WRITE_TAG,
};

fn rng(seed: u64) -> Rng {
    Rng::stream(seed, WRITE_TAG)
}

#[test]
fn persona_age_follows_the_year() {
    let dir = style_direction(Some(StyleId::Cowboy), &mut rng(1));
    for (year, age) in [(2026, 27), (2031, 32)] {
        assert_eq!(persona_age(year), age);
        let p = song_prompt("a dance", None, &dir, "playful", year);
        assert!(
            p.contains(&format!("it is {year}, so you are {age}.")),
            "{year}"
        );
    }
}

#[test]
fn prompt_carries_direction_plan_register_and_band() {
    for seed in 0..50 {
        let dir = style_direction(None, &mut rng(seed));
        let p = song_prompt("hay in June", Some(song::Voice::Alto), &dir, "tender", 2026);
        let form = dir.form.form();
        assert!(p.contains("\"\"\"hay in June\"\"\"\nIt will be sung by a alto.\nSTYLE: "));
        assert!(p.contains(&format!("STYLE: {}. Idiom: {}.", dir.label, dir.idiom)));
        assert!(p.contains(&format!(
            "Mode: {}. Meter: {}. Tempo: {} to {} bpm",
            dir.mode, dir.meter, dir.tempo_lo, dir.tempo_hi
        )));
        assert!(p.contains(&format!(
            "FORM: {}. {}\nFollow this plan exactly, in this order:\n{}\n",
            form.label,
            form.note,
            form.plan_text()
        )));
        assert!(p.contains("- If the prompt leaves the feeling open, write it as: tender."));
        assert!(p.contains(&format!(
            "Set \"guitar\" to \"{}\" and \"band\" to {{\"drums\":\"{}\",\"bass\":{},",
            dir.guitar, dir.band.drums, dir.band.bass
        )));
        assert!(p.is_ascii());
    }
}

#[test]
fn prompt_without_voice_keeps_the_blank_line() {
    let dir = style_direction(Some(StyleId::Blues), &mut rng(3));
    assert_eq!(dir.form, FormId::Blues12);
    let p = song_prompt("x", None, &dir, "wry and funny", 2026);
    assert!(p.contains("\"\"\"x\"\"\"\n\nSTYLE: Delta and Piedmont blues."));
    assert!(p.contains("2. verse 1: 3 lines, 4 chord entries per line"));
}

#[test]
fn schema_is_the_song_crates() {
    assert_eq!(
        songwriter::schema::song_schema(),
        song::schema::json_schema()
    );
}

#[test]
fn unknown_style_is_an_error() {
    assert!("polka".parse::<StyleId>().is_err());
    assert!(songwriter::styles::style("polka").is_err());
    assert_eq!("gospel".parse::<StyleId>(), Ok(StyleId::Gospel));
}

/// Records the request and answers with a fixed text.
struct Mock {
    reply: String,
    seen: RefCell<Option<Request>>,
}

impl Claude for Mock {
    fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
        *self.seen.borrow_mut() = Some(req.clone());
        Ok(Reply {
            model: Some("mock".into()),
            stop_reason: Some("end_turn".into()),
            ..Reply::text_only(self.reply.clone())
        })
    }
}

#[test]
fn write_song_sends_schema_and_extracts_the_song() {
    let mock = Mock {
        reply: "Here it is:\n```json\n{\"title\":\"Hay {in} June\",\"sections\":[]}\n```".into(),
        seen: RefCell::new(None),
    };
    let mut req = WriteRequest::new("hay in June", 2026);
    req.style = Some(StyleId::Oldtime);
    req.effort = Effort::Medium;
    req.model = Some("claude-fable-5-1".into());
    let w = write_song(&mock, &req, &mut rng(9)).expect("mock reply parses");
    assert_eq!(w.raw["title"], "Hay {in} June");
    assert_eq!(w.direction.style, StyleId::Oldtime);
    assert!(REGISTERS.contains(&w.register));
    assert_eq!(w.model.as_deref(), Some("mock"));
    assert_eq!(w.generation.model.as_deref(), Some("mock"));
    assert_eq!(w.generation.requested_model, "claude-fable-5-1");
    assert_eq!(w.generation.effort, "medium");
    assert_eq!(w.generation.transport, songwriter::claude::Transport::Cli); // Mock does not override transport()

    let seen = mock.seen.borrow().clone().expect("request sent");
    assert_eq!(seen.model, "claude-fable-5-1");
    assert_eq!(seen.effort, Effort::Medium);
    assert_eq!(seen.json_schema, Some(song::schema::json_schema()));
    assert!(seen.prompt.contains("STYLE: Old-time string band."));
    assert!(seen
        .prompt
        .contains(&format!("write it as: {}.", w.register)));

    // Same seed, same draws.
    let again = write_song(&mock, &req, &mut rng(9)).expect("mock reply parses");
    assert_eq!(again.direction.form, w.direction.form);
    assert_eq!(again.register, w.register);
}

#[test]
fn write_song_reports_a_reply_without_json() {
    let mock = Mock {
        reply: "I would rather not.".into(),
        seen: RefCell::new(None),
    };
    let r = write_song(&mock, &WriteRequest::new("x", 2026), &mut rng(1));
    assert!(matches!(r, Err(songwriter::WriteSongError::NoJsonFound(_))));
}

// ---------------------------------------------------------------- duet and phrasing (w1-prompt)

#[test]
fn prompt_names_the_styles_duet_fit_and_phrasing() {
    for seed in 0..30 {
        let dir = style_direction(None, &mut rng(seed));
        let p = song_prompt("a barn dance", None, &dir, "playful", 2026);
        assert!(
            p.contains(&format!("In this style a duet is {}.", dir.duet.as_str())),
            "seed {seed}: {p}"
        );
        assert!(
            p.contains(&format!(
                "This style usually sings {}, endings {}.",
                dir.phrasing.delivery, dir.phrasing.endings
            )),
            "seed {seed}"
        );
    }
}

#[test]
fn solo_and_duet_requests_are_fixed_choices_verbatim() {
    let dir = style_direction(Some(StyleId::Gospel), &mut rng(1));

    let p = song_prompt_with("a hymn", None, &dir, "devotional", 2026, &DuetRequest::Solo);
    assert!(p.contains("The choice is fixed: write this as a solo song, one singer throughout."));

    let p = song_prompt_with("a hymn", None, &dir, "devotional", 2026, &DuetRequest::Auto);
    assert!(!p.contains("The choice is fixed"));

    let p = song_prompt_with(
        "a hymn",
        None,
        &dir,
        "devotional",
        2026,
        &DuetRequest::Duet {
            a: Some(song::Voice::Tenor),
            b: Some(song::Voice::Alto),
        },
    );
    assert!(p.contains(
        "The choice is fixed: write this as a duet. Singer A is tenor, singer B is alto."
    ));

    let p = song_prompt_with(
        "a hymn",
        None,
        &dir,
        "devotional",
        2026,
        &DuetRequest::Duet { a: None, b: None },
    );
    assert!(p.contains("Singer A is your choice of voice, singer B is your choice of voice."));
}

#[test]
fn every_style_has_a_duet_fit_and_a_phrasing() {
    use songwriter::styles::StyleId::*;
    let welcome = [
        Bakersfield,
        Nashville,
        Texas,
        Cajun,
        Zydeco,
        Americana,
        Laurel,
        Revival,
        Gospel,
    ];
    let rare = [Appalachian, Broadside, IrishAir, Scottish, Welsh, Blues];
    for s in &STYLES {
        let want = if welcome.contains(&s.id) {
            DuetFit::Welcome
        } else if rare.contains(&s.id) {
            DuetFit::Rare
        } else {
            DuetFit::Occasional
        };
        assert_eq!(s.duet, want, "{}", s.id);
    }

    let legato = [IrishAir, Scottish, Welsh, Gospel];
    let parlando = [Blues, Broadside, Texas, Cowboy];
    let detached = [Oldtime, Bluegrass, Cajun, Zydeco, Shanty, IrishPub];
    let held = [Gospel, Revival, Nashville, IrishAir];
    let clipped = [Oldtime, Bluegrass, Shanty, Zydeco, IrishPub];
    for s in &STYLES {
        let want_delivery = if legato.contains(&s.id) {
            song::Delivery::Legato
        } else if parlando.contains(&s.id) {
            song::Delivery::Parlando
        } else if detached.contains(&s.id) {
            song::Delivery::Detached
        } else {
            song::Delivery::Flowing
        };
        let want_endings = if held.contains(&s.id) {
            song::Endings::Held
        } else if clipped.contains(&s.id) {
            song::Endings::Clipped
        } else {
            song::Endings::Released
        };
        assert_eq!(s.phrasing.delivery, want_delivery, "{}", s.id);
        assert_eq!(s.phrasing.endings, want_endings, "{}", s.id);
    }
}

#[test]
fn style_apply_fills_phrasing_only_when_none() {
    let raw = serde_json::json!({
        "meter": "4/4", "tempo": 100,
        "sections": [{"type": "verse", "lines": [{"syl": "*one *two", "chords": ["C"]}]}]
    });
    let (mut song, _) = song::normalize_value(&raw).expect("song normalises");
    assert_eq!(song.phrasing, None);
    songwriter::styles::apply_style("gospel", &mut song).expect("gospel exists");
    assert_eq!(
        song.phrasing,
        Some(songwriter::styles::style("gospel").unwrap().phrasing)
    );

    // A written phrasing survives the style's own default.
    let (mut song, _) = song::normalize_value(&raw).expect("song normalises");
    song.phrasing = Some(song::Phrasing::default());
    songwriter::styles::apply_style("gospel", &mut song).expect("gospel exists");
    assert_eq!(song.phrasing, Some(song::Phrasing::default()));
}

/// A minimal duet reply matching the reply template, including phrasing,
/// duet and the sing/lead/blend fields the SINGERS block describes.
fn duet_reply() -> String {
    serde_json::json!({
        "title": "Two on the Porch", "note": "a call and answer", "key": "G", "mode": "major",
        "meter": "4/4", "tempo": 100, "guitar": "strum", "voice": "baritone",
        "phrasing": {"delivery": "flowing", "endings": "released"},
        "duet": {"voice": "alto", "phrasing": {"delivery": "legato", "endings": "held"}},
        "band": {"drums": "none", "bass": true, "harmonyGuitar": false, "harp": false,
                 "violin": false, "choir": false, "harmonies": false, "doubles": false},
        "sections": [
            {"type": "intro", "chords": ["G", "C", "G", "D"]},
            {"type": "verse", "sing": "A", "lines": [
                {"syl": "*one *two", "ph": "w ah n|t uw", "chords": ["G", "C"]}]},
            {"type": "verse", "sing": "B", "lines": [
                {"syl": "*three *four", "ph": "th r iy|f ao r", "chords": ["G", "C"]}]},
            {"type": "chorus", "sing": "both", "lead": "B", "blend": "harmony", "lines": [
                {"syl": "*five *six", "ph": "f ay v|s ih k s", "chords": ["G", "D"]}]},
            {"type": "outro", "chords": ["G", "C", "D"]}
        ]
    })
    .to_string()
}

#[test]
fn reply_template_field_names_appear_verbatim() {
    let dir = style_direction(Some(StyleId::Americana), &mut rng(4));
    let p = song_prompt("a duet", None, &dir, "playful", 2026);
    assert!(p.contains(
        "\"phrasing\":{\"delivery\":\"<legato|flowing|parlando|detached>\",\"endings\":\"<held|released|clipped>\"}"
    ));
    assert!(p.contains("\"duet\""));
    assert!(p.contains("\"sing\":\"A|B|both\""));
    assert!(p.contains("\"lead\":\"A|B\""));
    assert!(p.contains("\"blend\":\"harmony|octave\""));
    assert!(
        p.contains("\"type\":\"chorus\",\"sing\":\"both\",\"lead\":\"B\",\"blend\":\"harmony\"")
    );
}

#[test]
fn a_duet_reply_parses_and_normalises_with_zero_repairs() {
    let mock = Mock {
        reply: duet_reply(),
        seen: RefCell::new(None),
    };
    let mut req = WriteRequest::new("a duet", 2026);
    req.style = Some(StyleId::Americana);
    let opts = WriteOptions {
        duet: DuetRequest::Duet {
            a: None,
            b: Some(song::Voice::Alto),
        },
    };
    let w = write_song_with(&mock, &req, &opts, &mut rng(5)).expect("mock reply parses");

    let seen = mock.seen.borrow().clone().expect("request sent");
    assert!(seen
        .prompt
        .contains("Singer A is your choice of voice, singer B is alto."));

    let (song, repairs) = song::normalize_value(&w.raw).expect("valid song JSON");
    assert!(repairs.is_empty(), "{repairs:?}");
    assert!(song.is_duet());
}

#[test]
fn random_draws_are_unchanged_for_seeds_0_to_100() {
    // FNV-1a 64 over "style meter form mode" lines joined by '|', for
    // style_direction(None, ..) at seeds 0..100. Recorded once, so a future
    // change that adds or reorders a random draw in style_direction fails
    // this test loudly (design 4.7/5.1: "no new rng draws").
    fn fnv1a(data: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }
    let mut lines = Vec::with_capacity(100);
    for seed in 0u64..100 {
        let d = style_direction(None, &mut rng_from_seed(seed));
        lines.push(format!("{} {} {} {}", d.style, d.meter, d.form, d.mode));
    }
    let joined = lines.join("|");
    assert_eq!(fnv1a(joined.as_bytes()), 0x68111dd336787f93, "{joined}");
}

/// `style_direction`'s draws are keyed by plain `Rng::from_seed`, not the
/// write-path's stream tag; this mirrors that (unlike `rng()` above, which
/// streams under `WRITE_TAG` as `write_song` does).
fn rng_from_seed(seed: u64) -> Rng {
    Rng::from_seed(seed)
}

#[test]
fn fixed_duet_does_not_also_say_sung_by_one_voice() {
    let mut r = rng(1);
    let dir = style_direction(None, &mut r);
    let p = song_prompt_with(
        "a duet",
        Some(song::Voice::Tenor),
        &dir,
        "playful",
        2026,
        &DuetRequest::Duet {
            a: None,
            b: Some(song::Voice::Alto),
        },
    );
    assert!(!p.contains("It will be sung by a"), "{p}");
    assert!(p.contains("Singer A is tenor, singer B is alto"), "{p}");
    let p = song_prompt_with(
        "a solo",
        Some(song::Voice::Tenor),
        &dir,
        "playful",
        2026,
        &DuetRequest::Solo,
    );
    assert!(p.contains("It will be sung by a tenor."));
}

#[test]
fn prompt_asks_for_schema_3_and_describes_its_fields() {
    let dir = style_direction(Some(StyleId::Gospel), &mut rng(4));
    let p = song_prompt("a shout", None, &dir, "celebratory", 2026);
    assert!(p.contains("{\"schema_version\":3,\"title\":"));
    for f in [
        "\"sing\":\"choir\"",
        "\"voicing\":\"unison\"",
        "\"~N\"",
        "\"key\":\"<tonic>\"",
        "\"rubato\":\"steady|light|free\"",
        "TUNES",
        "movable-do",
        "\"tune\":\"d d s, s\"",
        "\"tunes\":[{\"name\"",
        "at least every chorus line",
        "BREAK TUNES",
        "ENERGY (schema 3)",
        "\"energy\":\"quiet|low|mid|high\"",
        "\"energy\":\"<quiet|low|mid|high>\"",
        "8 eighth, 16 sixteenth, 4 quarter, 2 half, 1 whole",
        "z8",
        "\"tune\":\"<break tune: solfege with lengths>\"",
        "\"d8 d8 r8 m8 s4 m4 | r8 m8 r8 d8 t,4 s,4\"",
    ] {
        assert!(p.contains(f), "{f}");
    }
    assert!(p.is_ascii());
    // The schema sent to Claude is version 3 and requires the version.
    let sch = song::schema::json_schema();
    assert_eq!(sch["properties"]["schema_version"]["enum"][0], 3);
    assert!(sch["properties"].get("tunes").is_some());
    assert!(sch["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == "schema_version"));
}

#[test]
fn every_style_quotes_its_break_tune_phrase() {
    for id in StyleId::ALL {
        let s = id.style();
        assert!(!s.break_tune.is_empty(), "{id}");
        assert!(s.break_tune.is_ascii(), "{id}");
        let dir = style_direction(Some(*id), &mut rng(5));
        assert_eq!(dir.break_tune, s.break_tune);
        let p = song_prompt("a tune", None, &dir, "playful", 2026);
        assert!(
            p.contains(&format!("in this style's manner: {}.", s.break_tune)),
            "{id}"
        );
    }
    let pub_ = StyleId::IrishPub.style().break_tune;
    assert!(pub_.contains("reel") && pub_.contains("jig in 6/8") && pub_.contains("AABB"));
}

#[test]
fn every_style_quotes_its_drive_phrase() {
    for id in StyleId::ALL {
        let s = id.style();
        assert!(!s.drive.is_empty() && s.drive.is_ascii(), "{id}");
        let dir = style_direction(Some(*id), &mut rng(6));
        assert_eq!(dir.drive, s.drive);
        let p = song_prompt("a tune", None, &dir, "playful", 2026);
        assert!(p.contains(&format!("- This style: {}.", s.drive)), "{id}");
    }
    for id in [
        StyleId::IrishPub,
        StyleId::Oldtime,
        StyleId::Bluegrass,
        StyleId::Zydeco,
        StyleId::Shanty,
    ] {
        assert!(id.style().drive.contains("energy mid or high from bar one"));
    }
    assert!(StyleId::IrishAir.style().drive.contains("build from quiet"));
}

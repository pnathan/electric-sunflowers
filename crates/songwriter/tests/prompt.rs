//! Prompt content and the write path, against a mock Claude (no network).

use std::cell::RefCell;

use songwriter::claude::{Claude, ClaudeError, Effort, Reply, Request};
use songwriter::prompt::{persona_age, song_prompt, REGISTERS};
use songwriter::styles::{style_direction, FormId, StyleId};
use songwriter::{write_song, Rng, WriteRequest, WRITE_TAG};

fn rng(seed: u64) -> Rng {
    Rng::stream(seed, WRITE_TAG)
}

#[test]
fn persona_age_follows_the_year() {
    let dir = style_direction(Some(StyleId::Cowboy), &mut rng(1));
    for (year, age) in [(2026, 27), (2031, 32)] {
        assert_eq!(persona_age(year), age);
        let p = song_prompt("a dance", None, &dir, "playful", year);
        assert!(p.contains(&format!("it is {year}, so you are {age}.")), "{year}");
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
        assert!(p.contains(&format!("Mode: {}. Meter: {}. Tempo: {} to {} bpm", dir.mode, dir.meter, dir.tempo_lo, dir.tempo_hi)));
        assert!(p.contains(&format!("FORM: {}. {}\nFollow this plan exactly, in this order:\n{}\n", form.label, form.note, form.plan_text())));
        assert!(p.contains("- If the prompt leaves the feeling open, write it as: tender."));
        assert!(p.contains(&format!("Set \"guitar\" to \"{}\" and \"band\" to {{\"drums\":\"{}\",\"bass\":{},", dir.guitar, dir.band.drums, dir.band.bass)));
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
    assert_eq!(songwriter::schema::song_schema(), song::schema::json_schema());
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
        Ok(Reply { text: self.reply.clone(), model: Some("mock".into()), stop_reason: Some("end_turn".into()) })
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

    let seen = mock.seen.borrow().clone().expect("request sent");
    assert_eq!(seen.model, "claude-fable-5-1");
    assert_eq!(seen.effort, Effort::Medium);
    assert_eq!(seen.json_schema, Some(song::schema::json_schema()));
    assert!(seen.prompt.contains("STYLE: Old-time string band."));
    assert!(seen.prompt.contains(&format!("write it as: {}.", w.register)));

    // Same seed, same draws.
    let again = write_song(&mock, &req, &mut rng(9)).expect("mock reply parses");
    assert_eq!(again.direction.form, w.direction.form);
    assert_eq!(again.register, w.register);
}

#[test]
fn write_song_reports_a_reply_without_json() {
    let mock = Mock { reply: "I would rather not.".into(), seen: RefCell::new(None) };
    let r = write_song(&mock, &WriteRequest::new("x", 2026), &mut rng(1));
    assert!(matches!(r, Err(songwriter::WriteSongError::NoJsonFound(_))));
}

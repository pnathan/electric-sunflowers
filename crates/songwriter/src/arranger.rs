//! The arranger pass: the prompt and the model call. Claude may act as a
//! second-pass arranger after the rule-based arranger has arranged the
//! song. It reads the song's arranging note (the writer's, free text), the
//! style's drive and break-tune phrases, and a compact view of the
//! arrangement in beats (`engine::arranger::view`), and returns edits (the
//! schema is `engine::arranger::edit_schema`; this crate does not depend on
//! `engine`, so the caller passes the view text and the schema in). Reading
//! and applying the edits is the engine's, and it is pure; the engine makes
//! no model call. The CLI saves the result as the arrangement file.
//!
//! One call through `claude::Claude`, so the retry of `ClaudeApi` and the
//! usage record (`Generation`) are the write path's.

use std::time::Instant;

use crate::claude::{Claude, Effort, Request};
use crate::styles::Style;
use crate::usage::Generation;
use crate::{call_json, WriteSongError};

/// What to ask the arranger.
#[derive(Clone, Debug)]
pub struct ArrangeRequest<'a> {
    /// `engine::arranger::view` of the rule-based arrangement.
    pub view: &'a str,
    /// The song's style, when it has one.
    pub style: Option<&'static Style>,
    /// The writer's arranging note (`Song::arranging`).
    pub writer_note: Option<&'a str>,
    /// More guidance from the person (`sunflower rearrange --note`).
    pub extra_note: Option<&'a str>,
    /// `engine::arranger::edit_schema()`.
    pub schema: serde_json::Value,
    /// Model id; `None` is `claude::DEFAULT_MODEL`.
    pub model: Option<String>,
    pub effort: Effort,
}

/// The arranger's raw reply and what it cost.
#[derive(Clone, Debug)]
pub struct Arranged {
    /// The edits JSON as the model wrote it (not yet validated).
    pub raw: serde_json::Value,
    pub generation: Generation,
}

/// The arranger prompt.
pub fn arranger_prompt(req: &ArrangeRequest) -> String {
    let style = match req.style {
        Some(s) => format!(
            "STYLE: {}. Energy and drive: {}. What its instrumental breaks are: {}.\n",
            s.label, s.drive, s.break_tune
        ),
        None => String::new(),
    };
    let writer = match req.writer_note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("THE WRITER'S ARRANGING NOTE (the songwriter's own words about how this song should sound):\n{n}\n"),
        None => "THE WRITER'S ARRANGING NOTE: none. Judge from the song and the style.\n".to_string(),
    };
    let extra = match req.extra_note.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => format!("FURTHER GUIDANCE FROM THE PRODUCER:\n{n}\n"),
        None => String::new(),
    };
    format!(
        r#"You are the arranger of a live folk band. A rule-based arranger has already arranged this song, bar by bar, for drums, bass, violin, guitars, harp, singers and choir. It knows the chords and the energy of each section and nothing else. You read what it made and change what needs changing, so that the band plays the song as the song wants to be played. Keep what works. Change what the note or the style asks for, and what you hear is weak.

{style}{writer}{extra}
PRINCIPLES
- The groove comes first. In a dance song the kick and the bass drive the floor: a steady kick, a bass that locks to it and walks or pumps with the chords, a backbeat that can be felt. In a ballad the drums stay out of the way and the bass is sparse and round.
- Energy has a curve. Quiet sections play fewer notes and softer; peaks play more and harder. Make the peak the peak: do not spend it early.
- Give each instrument a job. Do not let every part play every beat. Leave air under the voice; the singer is the point. Busy parts belong in the gaps between sung lines and in the breaks.
- A break is where the band steps forward. If the style asks for a dance tune there, a written tune (a real reel, jig or hornpipe figure in the key, with a clear A part, running eighths, and cuts at the ends of phrases) is better than a few long notes.
- Stay in the key and on the chords: bass notes are mostly chord roots and fifths with passing notes; violin and guitar notes belong to the chord of their bar or pass between its notes.
- The lead vocal is yours too, as the one part that carries words. Shape its phrasing and melody where the arrangement calls for it: hold a word, lay a phrase back behind the beat, lift a pitch at the peak of a chorus, sing a long vowel over two or three notes (a melisma) where the style does. Keep the words, in order, and keep the tune's identity: the listener should still know the tune. The harmony, the doubles and the choir follow what the lead sings.
- Change only as much as the song needs. Bars you leave alone keep the rule-based arrangement exactly.

THE VIEW
The song as it stands is listed after this text. Everything is counted in beats: a beat is a quarter note in 4/4 and 3/4 and a dotted quarter in 6/8. Bars are numbered from 0. Inside a bar, beats count from 0 at the start of the bar: in 4/4, 0 is the downbeat, 1, 2 and 3 are the other beats, 0.5 is the and of one. Each bar lists its chords, the lyric line that begins there, and what each part plays: "vocal" (the lead melody), "drums", "bass", "harp", "violin", "harmony_guitar" (its lead notes) and "guitar" (the rhythm guitar's strokes). Notes read BEAT+LENGTH MIDI VELOCITY, velocity from 0 to 1 (60 is middle C); a harp note reads BEAT MIDI VELOCITY (it rings); a drum hit reads BEAT KIND VELOCITY; a guitar stroke reads BEAT DIRECTION STRINGS VELOCITY, and a bar that repeats the one before says "as bar N". A vocal note reads #N "syllable" BEAT+LENGTH MIDI: #N is the syllable's number in the song, and the later notes of a melisma read #N~. Each section's line says whether the harmony, the doubles and the choir sing there (the choir as "pad", an /aa/ chord pad, "words" or "off"). The harmony guitar's arpeggio is not shown and cannot be changed.

YOUR REPLY
One JSON object, nothing else: "edits", an optional "mix", and a short "summary".
- An edit of drums, bass, harp, violin or harmony_guitar names the part, a from_bar and a to_bar, and a list of notes. It REPLACES that part's events in bars from_bar up to, but not including, to_bar with exactly the notes listed. Beats in the list count from the start of from_bar (so bar from_bar+1 begins at beat 4 in 4/4), and every note must fall inside the span. An empty list silences the part in those bars. Edit long ranges as one edit; write every bar of the range out in full (repeat the pattern bar after bar).
- A pitched note is {{"beat":..,"len":..,"midi":..,"vel":..}}; the violin may add "vibrato": true or false. A harp note is {{"beat":..,"midi":..,"vel":..}}. Ranges: bass MIDI 28 to 60, harp 36 to 96, violin 55 to 100, harmony guitar 40 to 88.
- A drum hit is {{"beat":..,"drum":"Kick","vel":..}}. Drums: Kick, Snare, Rim, Tap, Swish (a brush sweep; give "len" in beats), Hat, Shaker, Tom (give "hz", 80 to 200) and Ride.
- An edit of "guitar" gives "strokes" instead of notes: ONE bar's strum pattern, which plays in every bar from from_bar up to to_bar on that bar's chord. A stroke is {{"beat":..,"dir":"down","strings":"all","vel":..}} with beat from 0 to the end of the bar, dir "down" or "up", and "damp": true for a muted chop that stops at once. "strings" is "all", "bass" (the low three strings of the chord's shape), "treble" (the high three), or a single picked note: "low" (the bass note), "alt" (the string above it), "g", "b" or "e". A boom-chuck is "bass" on the beat and "treble" off it. The engine picks the chord's fingering itself.
- An edit of "lead" changes the sung notes in bars from_bar up to to_bar: {{"part":"lead","from_bar":..,"to_bar":..,"notes":[{{"syllable":N,"beat":..,"len":..,"midi":..}}]}}. List EVERY syllable that begins in those bars, in order, by its number from the view, each over one note or, for a melisma, over two to four notes in a row (the vowel holds across them; the engine lays the consonants out). Onsets must rise from note to note. You may move, lengthen or shorten any note, change its pitch, split a syllable into a melisma or merge a melisma back. You may not add, drop or reorder words: an edit that does is thrown out whole. Keep notes near the singer's range.
- An edit of "harmony", "doubles" or "choir" works on sections, numbered as the view lists them: {{"part":"harmony","sections":[1,3],"on":true,"interval":"third_above"}} (intervals: third_above, third_below, sixth_above, sixth_below, fifth_above, fifth_below; leave "interval" out for the rules' own choice), {{"part":"doubles","sections":[3],"on":false}}, {{"part":"choir","sections":[3],"mode":"unison"}} with mode "off", "pad" (the /aa/ pad), "unison" or "block" (sing the section's words, in unison or in four parts). A writer's own choir line keeps the choir: only unison or block may change it.
- "mix" sets a fader offset in dB for a track, {{"tracks":{{"drums":{{"gain_db":4}},"bass":{{"gain_db":6}}}},"duck_db":2}}, and the depth in dB (default 5) by which the band ducks under the singer. Track names: lead, lead_b, doubles, harmony, choir, guitar, harmony_guitar, bass, drums, harp, violin. Use the mix when the balance needs it: a dance band with its bass and drums low in the mix does not dance. Leave it out if the balance is fine.
- "summary": one or two plain sentences: what you changed and why.
Anything outside these limits is repaired or dropped by the engine, so stay inside them. The rhythm guitar and the lead vocal always play; any other part that the band line says is off cannot be edited.

THE SONG
{view}"#,
        view = req.view,
    )
}

/// Asks Claude for the edits. One call; the reply is the first JSON object
/// in the text (as `write_song` reads the song). The caller validates and
/// applies the edits (`engine::arranger`).
pub fn arrange(claude: &dyn Claude, req: &ArrangeRequest) -> Result<Arranged, WriteSongError> {
    let mut creq = Request::new(arranger_prompt(req));
    if let Some(m) = &req.model {
        creq.model = m.clone();
    }
    creq.effort = req.effort;
    creq.json_schema = Some(req.schema.clone());
    let start = Instant::now();
    let (raw, generation) = call_json(claude, &creq, start)?;
    Ok(Arranged { raw, generation })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::{ClaudeError, Reply};
    use crate::styles::style;
    use serde_json::json;
    use std::cell::RefCell;

    /// A mock that records the request and replies with fixed text.
    struct Mock {
        reply: String,
        seen: RefCell<Option<Request>>,
    }

    impl Claude for Mock {
        fn complete(&self, req: &Request) -> Result<Reply, ClaudeError> {
            *self.seen.borrow_mut() = Some(req.clone());
            Ok(Reply::text_only(self.reply.clone()))
        }
    }

    fn request<'a>(
        view: &'a str,
        writer: Option<&'a str>,
        extra: Option<&'a str>,
    ) -> ArrangeRequest<'a> {
        ArrangeRequest {
            view,
            style: style("oldtime").ok(),
            writer_note: writer,
            extra_note: extra,
            schema: json!({"type": "object"}),
            model: Some("test-model".into()),
            effort: Effort::Medium,
        }
    }

    #[test]
    fn the_prompt_carries_the_view_the_style_and_both_notes() {
        let r = request(
            "VIEW-TEXT",
            Some("Drive it like a barn dance."),
            Some("More bass."),
        );
        let p = arranger_prompt(&r);
        let s = style("oldtime").unwrap();
        assert!(p.contains("VIEW-TEXT"));
        assert!(p.contains(s.label));
        assert!(p.contains(s.drive));
        assert!(p.contains(s.break_tune));
        assert!(p.contains("Drive it like a barn dance."));
        assert!(p.contains("More bass."));
        assert!(p.contains("arranger of a live folk band"));
        assert!(p.contains("REPLACES"));
        for kind in [
            "\"strokes\"",
            "boom-chuck",
            "\"syllable\":N",
            "may not add, drop or reorder",
            "\"doubles\"",
            "third_below",
            "keep the tune's identity",
            "harp note is",
        ] {
            assert!(p.contains(kind), "{kind}");
        }
        // The JSON examples have single braces after format!.
        assert!(p.contains(r#"{"beat":..,"len":..,"midi":..,"vel":..}"#));
        // No note: the prompt says so and names no producer section.
        let p = arranger_prompt(&request("V", None, None));
        assert!(p.contains("ARRANGING NOTE: none"));
        assert!(!p.contains("FURTHER GUIDANCE"));
    }

    #[test]
    fn the_call_passes_the_schema_model_and_effort_and_records_usage() {
        let mock = Mock {
            reply: "Here:\n```json\n{\"edits\": [], \"summary\": \"ok\"}\n```".into(),
            seen: RefCell::new(None),
        };
        let a = arrange(&mock, &request("V", None, None)).unwrap();
        assert_eq!(a.raw["summary"], "ok");
        assert_eq!(a.generation.requested_model, "test-model");
        assert_eq!(a.generation.effort, "medium");
        let seen = mock.seen.borrow();
        let seen = seen.as_ref().unwrap();
        assert_eq!(seen.json_schema, Some(json!({"type": "object"})));
        assert_eq!(seen.model, "test-model");
    }

    #[test]
    fn a_reply_with_no_json_is_an_error() {
        let mock = Mock {
            reply: "no edits today".into(),
            seen: RefCell::new(None),
        };
        assert!(matches!(
            arrange(&mock, &request("V", None, None)),
            Err(WriteSongError::NoJsonFound(_))
        ));
    }
}

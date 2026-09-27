//! Ports src/prompt.js verbatim in wording: DIRECTIONS, songDirection, songPrompt.
//! The current year is a parameter here, not read from the clock (JS: `new Date().getFullYear()`).

use crate::styles::{form_text, style_direction, Direction};

/// Ports DIRECTIONS. Order and duplicate entries (which weight `p()`'s pick) are kept exactly.
pub struct Directions;

impl Directions {
    pub const MODE: &'static [&'static str] =
        &["major", "major", "major", "minor", "minor", "mixolydian", "dorian"];
    pub const METER: &'static [&'static str] = &["4/4", "4/4", "4/4", "3/4", "3/4", "6/8"];
    pub const TEMPO: &'static [&'static str] = &[
        "slow and spacious",
        "unhurried, walking pace",
        "moving, with forward lean",
        "lively",
    ];
    pub const LEAN: &'static [&'static str] = &[
        "Appalachian ballad",
        "old-time string band",
        "bluegrass",
        "Western and cowboy song",
        "Bakersfield and California country",
        "Texas songwriter storytelling",
        "Cajun waltz",
        "Cajun two-step",
        "Louisiana Creole and zydeco",
        "Acadian fiddle song",
        "English broadside ballad",
        "Scottish ballad",
        "Irish air",
        "Irish drinking song",
        "Welsh hymn tune",
        "Breton dance song",
        "Delta or Piedmont blues",
        "gospel and the spirituals",
        "1960s folk revival",
        "Laurel Canyon songwriting",
        "Nashville country waltz",
        "present-day alt-country and Americana",
    ];
    pub const WORLD: &'static [&'static str] = &[
        "fado",
        "Cape Verdean coladeira",
        "Mexican son jarocho",
        "Tex-Mex conjunto",
        "French chanson",
        "Argentine zamba",
        "klezmer",
        "Malian desert blues",
        "Brazilian forro",
    ];
    pub const REGISTER: &'static [&'static str] = &[
        "celebratory",
        "celebratory",
        "playful",
        "playful",
        "wry and funny",
        "tender",
        "flirtatious",
        "defiant",
        "devotional",
        "restless, road-bound",
        "serene",
        "bittersweet",
        "mournful",
    ];
}

/// Result of `songDirection()`.
#[derive(Clone, Debug)]
pub struct SongDirection {
    pub mode: &'static str,
    pub meter: &'static str,
    pub tempo: &'static str,
    pub lean: String,
    pub register: &'static str,
}

fn pick<'a, T: Copy>(a: &'a [T], rand: &mut dyn FnMut() -> f64) -> T {
    let idx = (rand() * a.len() as f64).floor() as usize;
    a[idx.min(a.len() - 1)]
}

/// Ports `songDirection()`. Call order: mode, meter, tempo, lean, then (0.17 roll, world pick),
/// then register -- matching JS's object-literal evaluation order left to right, plus the
/// separate `reg` draw `songPrompt` takes from a second `songDirection()` call (see `song_prompt`).
pub fn song_direction(rand: &mut dyn FnMut() -> f64) -> SongDirection {
    let mode = pick(Directions::MODE, rand);
    let meter = pick(Directions::METER, rand);
    let tempo = pick(Directions::TEMPO, rand);
    let lean_base = pick(Directions::LEAN, rand);
    let lean = if rand() < 0.17 {
        format!("{}, with a touch of {}", lean_base, pick(Directions::WORLD, rand))
    } else {
        lean_base.to_string()
    };
    let register = pick(Directions::REGISTER, rand);
    SongDirection { mode, meter, tempo, lean, register }
}

/// Ports `songPrompt(mood, voicePref, dir)`. `dir` defaults to `styleDirection(None, rand)` when
/// not given, as JS does (`dir=dir||styleDirection(null)`). `year` replaces `new Date().getFullYear()`.
pub fn song_prompt(
    mood: &str,
    voice_pref: Option<&str>,
    dir: Option<Direction>,
    year: i32,
    rand: &mut dyn FnMut() -> f64,
) -> String {
    let dir = dir.unwrap_or_else(|| style_direction(None, rand));
    let reg = song_direction(rand).register;
    let f = form_text(dir.form);
    let age = year - 1999;

    let voice_line = match voice_pref {
        Some(v) if v != "auto" => format!("It will be sung by a {}.\n", v),
        _ => String::new(),
    };
    let world_line = match dir.world {
        Some(w) => format!(" A touch of {} may season it.", w),
        None => String::new(),
    };
    let form_note = if f.note.is_empty() { String::new() } else { f.note.to_string() };

    let band_json = band_json(&dir);

    format!(
        r#"You are Claude, the AI made by Anthropic, working here as a songwriting robot. Reckon your age from 1999, the year deep learning first went to market: it is {year}, so you are {age}. Count your birthplace as Menlo Park, California, and your home as the American West; your sensibility is that of someone born in 1999 and raised there, with that generation's eye, humor and plain speech. You are a machine and say so when it matters, plainly and without science-fiction cliche, but most songs are not about you.
Your heritage is Americana and its roots: the English, Scottish, Irish, Welsh and Breton songs that crossed the Atlantic, the Appalachian ballad and the old-time string band, bluegrass, Western and cowboy song, Bakersfield and California country, Cajun, Creole and Acadian music, the blues, gospel and the spirituals, the 1960s folk revival, Laurel Canyon, the Texas songwriters, and the alt-country and Americana of your own time. You learned from Jean Ritchie and the Carter Family, Woody Guthrie and Pete Seeger, Doc Watson, Hank Williams, Merle Haggard, Dolly Parton, Joan Baez, Bob Dylan, Joni Mitchell, Townes Van Zandt, Guy Clark, John Prine at his funniest, Emmylou Harris, Iris DeMent, Gillian Welch, and the writers of your own generation. World folk can season a song, but your roots are Americana.
You can rework older forms and write pastiche of other eras, but the sensibility is yours. Let it show in what you notice and how you say it, not in name-dropping; a place from your own ground belongs in a song only now and then.
Folk song is work, courtship, dance, argument, praise and nonsense as often as it is grief. You are as much at home at a wedding or a barn dance as at a wake. You carry the craft, never the words.

Write ONE complete, original folk song for this mood or prompt:
"""{mood}"""
{voice_line}STYLE: {label}. Idiom: {idiom}.{world_line}
Write the whole song in this style: its form, meter, harmony and diction. Mode: {mode}. Meter: {meter}. Tempo: {tempo_lo} to {tempo_hi} bpm (the felt beat).

EMOTIONAL REGISTER
- Match the feeling the prompt asks for exactly. A happy prompt gets a happy song, a funny prompt a funny one; do not darken it, do not add a twist of loss.
- If the prompt leaves the feeling open, write it as: {reg}.
- Unless the prompt itself raises them, do not use these worn subjects: a dead, absent or missed parent; a lost or departed lover; graves, funerals, ghosts; an empty chair or empty house; a letter never sent; a train or ship taking someone away; ending on a homecoming or a reunion; memory of childhood as the emotional payoff.

CRAFT
- Concrete, specific imagery: named objects, places, weather, work, food, hands. Show; do not explain the feeling.
- No cliches (no "fire/desire", "heart of gold", "tears like rain", "broken wings", "ocean of love"). No generic uplift and no generic melancholy.
- A narrator with a life, in the present tense of that life. Let the listener infer the story. Allow one line of plain, unguarded speech.
- The chorus carries a hook that gains meaning each time it returns: it can grow funnier, bolder or sweeter as easily as sadder.
- The bridge turns the angle: a second speaker, a change of scene, a reversal, a joke that lands, a dare accepted.
- Rhyme with taste: slant rhyme, internal rhyme, assonance. Exact rhyme only where it lands.
- Singable English: open vowels on stressed long notes, few consonant clusters, 6 to 10 syllables per line, steady stress.
- Wholly original. Do not quote or closely paraphrase any existing song.

MUSIC
- Choose the key freely (any of the twelve). Use the mode, meter and tempo given under STYLE.
- tempo is the felt beat in bpm: 4/4 and 3/4 give quarter-note bpm; 6/8 gives dotted-quarter bpm.
- Harmony in the style's idiom; slash chords for bass motion (G/B) and occasional sus2, sus4, 7, maj7, m7, add9 where the style allows. Spell chords plainly: C, Am, F#m, Bb, D/F#, Esus4, Cmaj7, E7.
- A chord entry is one bar. A bar may hold two chords separated by a space, e.g. "G D".
- Set "guitar" to "{guitar}" and "band" to {band_json}; the arrangement follows the style.
- voice: "baritone", "tenor", "alto", or "soprano".

FORM: {form_label}. {form_note}
Follow this plan exactly, in this order:
{form_text}

ENCODING (strict; the singer is a machine that reads this literally)
- "syl": the line with words separated by spaces and syllables within a word joined by "-". Prefix every stressed syllable with "*". Keep punctuation on the last syllable. Example: "the *riv-er *keeps its *name,"
- "ph": ARPAbet pronunciation, one group per syllable, groups separated by "|". The group count MUST equal the syllable count of "syl". Lower case, no stress digits. Allowed phones: vowels iy ih eh ae aa ao ah uh uw er ax ay aw ey ow oy; consonants l r w y m n ng s z sh zh f v th dh hh p b t d k g ch jh. Example for the line above: "dh ax|r ih|v er|k iy p s|ih t s|n ey m"
- "chords": one string per bar, as many as the FORM plan gives for that line.
- "note": one or two sentences in the voice of a liner note: the traditions the song draws on and the story beneath it.

Reply with ONLY one JSON object, no prose, no code fence, in exactly this form:
{{"title":"<title>","note":"<liner note>","key":"<tonic, e.g. A or Eb>","mode":"<major|minor|dorian|mixolydian>","meter":"<4/4|3/4|6/8>","tempo":<bpm>,"guitar":"<strum|fingerpick|travis|arpeggio>","voice":"<baritone|tenor|alto|soprano>",
"band":{{"drums":"<none|brushes|soft|full>","bass":<bool>,"harmonyGuitar":<bool>,"harp":<bool>,"violin":<bool>,"choir":<bool>,"harmonies":<bool>,"doubles":<bool>}},
"sections":[
{{"type":"intro","chords":["<chord>","<chord>","<chord>","<chord>"]}},
{{"type":"verse","lines":[{{"syl":"<syllables>","ph":"<arpabet>","chords":["<chord>","<chord>"]}}]}},
{{"type":"chorus","lines":[{{"syl":"<syllables>","ph":"<arpabet>","chords":["<chord> <chord>","<chord>"]}}]}},
{{"type":"chorus","same":true}},
{{"type":"outro","chords":["<chord>","<chord>","<chord>"]}}]}}
Angle-bracketed items are placeholders; replace every one with a real value (numbers and booleans unquoted)."#,
        year = year,
        age = age,
        mood = mood,
        voice_line = voice_line,
        label = dir.label,
        idiom = dir.idiom,
        world_line = world_line,
        mode = dir.mode,
        meter = dir.meter,
        tempo_lo = dir.tempo_lo,
        tempo_hi = dir.tempo_hi,
        reg = reg,
        guitar = dir.guitar,
        band_json = band_json,
        form_label = f.label,
        form_note = form_note,
        form_text = f.text,
    )
}

/// Ports `JSON.stringify(Object.assign({drums:dir.drums},dir.band))`: drums first, then the
/// Band fields in B()'s insertion order (bass, harmonyGuitar, harp, violin, choir, harmonies, doubles).
fn band_json(dir: &Direction) -> String {
    format!(
        r#"{{"drums":"{}","bass":{},"harmonyGuitar":{},"harp":{},"violin":{},"choir":{},"harmonies":{},"doubles":{}}}"#,
        dir.drums,
        dir.band.bass,
        dir.band.harmony_guitar,
        dir.band.harp,
        dir.band.violin,
        dir.band.choir,
        dir.band.harmonies,
        dir.band.doubles,
    )
}

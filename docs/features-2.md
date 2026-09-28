# Features 2: design

Branch `features-2` (from trunk). Six features requested by the owner: Claude usage capture, settings, stems and a mixer, duets, singer phrasing, and a full multipart score. The build plan is `docs/features-2-plan.json`.

Principle (owner): "claude's role is to _write the song_, then the song engine works deterministically from there." A writing decision goes in the song JSON Claude writes; the engine only reads it. Duet parts and phrasing are writing decisions and go in the song JSON. Mix settings, model choice and file locations are not; they go in sidecars and the settings file.

Invariant kept by every wave: a song without the new fields renders exactly as today. The demo song has none of them, so the demo stems stay bit-identical through all waves (`scripts/gate.sh --strict`). Section 8 has the gate policy.

## 0. Current state (read before changing anything)

- `songwriter::claude`: `Claude` trait, `ClaudeCli` (runs `claude -p --output-format json`, parses `result`, `is_error`, `subtype`, `stop_reason` only; `Reply.model` is always `None`), `ClaudeApi` (Messages API; reads `model`, ignores `usage`). `Reply { text, model, stop_reason }`. `Written { raw, direction, register, model }`.
- Render sidecar `<stem>.render.json` is written by two copies of the same code: `sunflower/src/main.rs::write_sidecars` and `studio/src/jobs.rs::write_sidecars`; read by `studio/src/library.rs::RenderInfo::read`.
- `crates/settings`: an empty crate (one doc line). Deps serde and toml 0.9 (default features: std, serde, parse, display).
- Engine: `engine::render(song, seed, voice, progress) -> (Prepared, Stems)` renders 10 processed stems (post EQ, compression, level; pre pan); `engine::mix(&stems, &band, seed)` sums them with the static `STRIPS` gain and pan, the vocal ducker (`DUCK_DB` 5 dB, keyed by the lead), the lead slapback (`Stems.slapback`, at unity), the FDN reverb, the bus compressor and peak normalisation to 0.89. The studio does not keep the stems; it plays the `.ogg`.
- Voice: one lead `Singer` (`song::events`); `SingStyle` carries per-singer departures from the voice preset. Articulation constants are in `voice::articulation` (ONSET_SHARE 0.45, FIRST_ONSET 0.3, BREATH_* ) and `voice::controls` (swell, phrase-end fade in `shape_dynamics`). Note ends come from `compose::prepare::time_notes` (phrase-final notes end 0.05 s early; legato gap 4 ms; breath gap 90 ms).
- Notation: `notation::Score::new(song, prepared)` engraves the lead melody only, one treble staff (8vb for bass, baritone, tenor), one system per lyric line; glyphs are Bravura outlines copied from `src/glyphs.js` (G clefs, noteheads, flags, accidentals, rests, dot, time digits, `restHBar`). No F clef, percussion clef, x notehead, brace or bracket exists, and no Bravura font is on this machine (`fc-list`, `find`); do not fetch one.
- Studio: `jobs::load` runs `compose::prepare`, `engine::sheet_from`, `notation::Score::new`; `jobs::render` renders, mixes, writes Ogg at quality 0.6 and the sidecars; views Lyrics, Sheet, Both.

## 1. Claude usage and model

### 1.1 Data

New module `songwriter::usage`:

```rust
/// Token counts of one generation. Absent counts are None, never 0.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    /// Thinking tokens (inside output_tokens). CLI only; the Messages API
    /// does not report them separately.
    pub thinking_tokens: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage { pub model: String, pub usage: Usage, pub cost_usd: Option<f64> }

/// Everything known about one call to Claude.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Generation {
    pub transport: Transport,          // "cli" | "api"
    pub requested_model: String,
    pub effort: String,                // Effort::as_str
    /// The model that answered: the API reply's `model`; for the CLI the
    /// modelUsage entry with the most output tokens (ties: the requested model).
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Usage,                  // totals
    /// CLI: every modelUsage entry, sorted by model id. API: one entry.
    pub per_model: Vec<ModelUsage>,
    /// CLI total_cost_usd. The API reports no cost; it stays None (no price table).
    pub cost_usd: Option<f64>,
    /// CLI duration_ms.
    pub duration_ms: Option<u64>,
    /// Wall time of the call measured by this client, both transports.
    pub wall_ms: u64,
}
```

`Transport { Cli, Api }` moves to `songwriter::claude` (named `cli`, `api`; FromStr, Display, serde) and replaces the duplicated `Via` enums in the CLI and studio.

`Reply` gains `usage: Usage`, `per_model: Vec<ModelUsage>`, `cost_usd: Option<f64>`, `duration_ms: Option<u64>`. `Written` gains `generation: Generation` (the `model` field stays, equal to `generation.model`). `write_song` measures `wall_ms` with `Instant` around `claude.complete`.

The error path does not change: a failed call (refusal, max tokens, status) records no usage. Carrying usage through `ClaudeError` is left out on purpose.

### 1.2 Parsing

- CLI envelope (`parse_cli_envelope`): `usage.input_tokens`, `usage.output_tokens`, `usage.cache_read_input_tokens`, `usage.cache_creation_input_tokens`, `usage.output_tokens_details.thinking_tokens`; `modelUsage` object: per key `inputTokens`, `outputTokens`, `cacheReadInputTokens`, `cacheCreationInputTokens`, `thinkingTokens`, `costUSD`; `total_cost_usd`; `duration_ms`. Every field optional: a missing or non-numeric field reads as None; nothing fails because usage is missing.
- API reply (`parse_api_reply`): `model`; `usage.input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`. `per_model` = one entry for `model`.

### 1.3 Render sidecar

New module `songwriter::sidecar` owns the `<stem>.render.json` format for both apps:

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderSidecar {
    pub seed: Option<u64>,
    pub voice: Option<String>,        // singer A as rendered
    pub voice_b: Option<String>,      // singer B (duet), wave 2 on
    pub style: Option<String>,
    pub style_label: Option<String>,
    pub model: Option<String>,        // kept for old readers: generation.model
    pub generation: Option<Generation>,
    pub song_json: Option<String>,
    pub audio: Option<String>,
    pub sheet: Option<String>,
    pub mix: Option<String>,          // the <stem>.mix.json applied, if any
    pub stems: Option<String>,        // the <stem>.stems directory, if written
    pub created: Option<String>,
}
impl RenderSidecar {
    pub fn read(path) -> Result<RenderSidecar, String>;   // lenient: bad fields read as None
    /// Writes atomically (tmp + rename). When `generation` is None and the
    /// file already at `path` has one for the same song_json, it is kept:
    /// a re-render does not lose the record of how the song was written.
    pub fn write(&self, path) -> Result<(), String>;
}
```

Example:

```json
{
  "seed": 7, "voice": "baritone", "voice_b": "alto",
  "style": "nashville", "style_label": "Nashville country",
  "model": "claude-opus-5-5",
  "generation": {
    "transport": "cli", "requested_model": "claude-opus-5-5", "effort": "high",
    "model": "claude-opus-5-5", "stop_reason": "end_turn",
    "usage": {"input_tokens": 6120, "output_tokens": 5310, "cache_read_input_tokens": 0,
              "cache_creation_input_tokens": 4988, "thinking_tokens": 2760},
    "per_model": [{"model": "claude-opus-5-5", "usage": {"input_tokens": 6120, "output_tokens": 5310,
                   "cache_read_input_tokens": 0, "cache_creation_input_tokens": 4988,
                   "thinking_tokens": 2760}, "cost_usd": 0.41}],
    "cost_usd": 0.41, "duration_ms": 61234, "wall_ms": 62010
  },
  "song_json": "/home/u/Music/sunflower/porch-light.json",
  "audio": "/home/u/Music/sunflower/porch-light.ogg",
  "sheet": "/home/u/Music/sunflower/porch-light.sheet.json",
  "mix": null, "stems": null,
  "created": "2026-09-28T14:03:07Z"
}
```

(The numbers above illustrate the format; they are not a measurement.)

### 1.4 Display

- `sunflower write` prints, right after the call and before normalising (so a rejected song still shows its cost), one line from `Generation::summary()`:
  `claude: claude-opus-5-5 via cli (effort high): 6,120 in (cache read 0, cache write 4,988), 5,310 out (thinking 2,760), $0.41, 62.0 s`. Absent values are left out, not printed as 0.
- Studio: under the song title in the lyrics view and in the library tooltip, the same summary from the sidecar's `generation`; "not recorded" when absent.

## 2. Settings

### 2.1 File

`$XDG_CONFIG_HOME/electric-sunflowers/config.toml`; `XDG_CONFIG_HOME` unset or empty means `$HOME/.config`. `SUNFLOWER_CONFIG=<path>` overrides the path (tests, and a second profile). A missing file is not an error: defaults apply.

```toml
# electric-sunflowers settings. Written by the studio; comments are not kept.
[claude]
model = "claude-opus-5-5"      # any model id
transport = "cli"              # cli | api
effort = "high"                # low | medium | high | xhigh | max

[songwriter]
voice = "auto"                 # auto | bass | baritone | tenor | alto | soprano: singer A named in the prompt
duet = "auto"                  # auto | solo | duet: what the prompt asks for

[studio]
library = "~/Music/sunflower"  # a leading ~ is the home directory

[export]
ogg_quality = 0.6              # -0.2 to 1.0
```

### 2.2 Crate `settings`

Deps: serde, toml 0.9, and the workspace crates `song` (Voice) and `songwriter` (Effort, Transport). No other crate.

```rust
pub struct Settings { pub claude: ClaudeSettings, pub songwriter: SongwriterSettings, pub studio: StudioSettings, pub export: ExportSettings }
pub struct ClaudeSettings { pub model: String, pub transport: Transport, pub effort: Effort }
pub struct SongwriterSettings { pub voice: Option<Voice>, pub duet: DuetChoice }   // DuetChoice { Auto, Solo, Duet }
pub struct StudioSettings { pub library: PathBuf }
pub struct ExportSettings { pub ogg_quality: f32 }
impl Default for Settings   // the values in 2.1

pub fn config_path() -> Option<PathBuf>;                 // 2.1 rules; None when HOME is also unset
pub struct Loaded { pub settings: Settings, pub path: Option<PathBuf>, pub found: bool, pub warnings: Vec<String> }
pub fn load() -> Loaded;                                  // never fails
pub fn parse(text: &str) -> (Settings, Vec<String>);      // lenient
pub fn save(settings: &Settings, path: &Path) -> std::io::Result<()>;  // creates the dir; tmp + rename
pub fn expand_home(p: &str) -> PathBuf;
```

Parsing is lenient, like the song boundary: the text is read as a `toml::Table`, then each known key is taken; a wrong type or an unknown value gives the default and a warning naming the key; unknown keys and tables give a warning; a TOML syntax error gives all defaults and one warning with the parser's message. `ogg_quality` is clamped to -0.2..1.0 with a warning. `save` writes `toml::to_string` of a serde mirror struct with the header comment line; comments in the user's file are not kept (the header says so).

Precedence, highest first: command-line flag, settings file, built-in default. One function per value in the consumers, e.g. `model = cli.model.or(settings.claude.model)`. The API key never goes in the file (CLAUDE.md: never ship a key); `ANTHROPIC_API_KEY` stays the only source.

### 2.3 Consumers

- CLI: every command calls `settings::load()` once and prints its warnings. `--model`, `--via` (default from settings), new `--effort`, `--quality` (now `Option<f32>`, default from settings), and `write --voice` / duet flags default from `[songwriter]`.
- Studio: `library` is the default `--dir`; the new-song form starts from `claude.*` and `songwriter.*`; renders use `export.ogg_quality`. A Settings window edits every field and has Save (writes the file, shows the path) and Revert. A changed library directory takes effect on Save (rescan).

## 3. Stems and mixer

### 3.1 Decisions

- The engine already renders every track separately (`Stems`). The mix becomes a pure function of the cached stems and a `MixSettings` value; the render is never repeated for a mix change.
- Per-track controls: fader (dB, relative to the strip gain), pan (absolute, -1..1), mute, solo. Global: ducking depth (dB; 0 turns it off). The strip table stays the default mix.
- Mix settings are not a writing decision: they live in a sidecar `<stem>.mix.json` beside the audio, not in the song JSON.
- Stems on disk are opt-in (`--stems`): a 3-minute song's stems are about 10 x 30 MB of 24-bit FLAC. The studio exports them on request.
- The studio re-mixes the cached stems off the UI thread (debounced) and swaps the playback buffer at the current position. A true real-time mixer is rejected: the bus compressor threshold (bus RMS + 5 dB) and the peak normalisation need whole-song statistics, so a live mix would not equal the exported one.

### 3.2 Data

New module `engine::mixset`:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackMix { pub gain_db: f32, pub pan: f32, pub mute: bool, pub solo: bool }

#[derive(Clone, Debug, PartialEq)]
pub struct MixSettings {
    pub tracks: [TrackMix; N_TRACKS],    // indexed by TrackId
    /// Accompaniment reduction while a lead sings, dB (DUCK_DB = 5); 0 = off.
    pub duck_db: f32,
}
impl MixSettings {
    /// Strip pans, 0 dB faders, nothing muted or soloed, DUCK_DB. In a duet
    /// (both lead stems present) the leads pan to DUET_PAN_A = -0.25 and
    /// DUET_PAN_B = +0.25.
    pub fn default_for(stems: &Stems) -> MixSettings;
    /// Whether track `id` sounds: not muted, and soloed when any track is soloed.
    pub fn audible(&self, id: TrackId) -> bool;
    pub fn to_json(&self, defaults: &MixSettings) -> serde_json::Value;   // only fields that differ
    pub fn from_json(v: &Value, defaults: &MixSettings) -> (MixSettings, Vec<String>); // lenient, warnings
}
```

`<stem>.mix.json`:

```json
{"version": 1,
 "tracks": {"violin": {"gain_db": -3.0}, "harp": {"mute": true}, "lead": {"pan": -0.1}},
 "duck_db": 4.0}
```

Track keys are `TrackId::name()`. Absent tracks and fields keep the defaults. Unknown keys give a warning and are ignored.

### 3.3 Engine changes

- `ProcessedStem` gains `slap: Option<SparseBuf>` (the strip's slapback, gain applied); `Stems.slapback` is removed. The slapback follows its track's fader and mute (it is added to both buses with gain `db_to_gain(gain_db)`, unpanned, as today).
- `mix_with(stems, band, seed, &MixSettings) -> Stereo`. Route gain = `strip.gain * db_to_gain(gain_db) * level`; pan from the settings. A track plays when `id.plays(band)` and `settings.audible(id)`. `mix(stems, band, seed)` stays as `mix_with(.., &MixSettings::default_for(stems))`.
- Ducking (`duck_gains`) is keyed by the sum of the audible lead tracks' post-fader power (`Lead`, and `LeadB` from wave 2); lead tracks are never ducked; depth `settings.duck_db`; `duck_db == 0` skips the ducker. The key's full-scale point stays half the lead target level times the lead strip gain.
- Bit identity: with `default_for`, every multiply is by exactly 1.0 (`db_to_gain(0.0)` must return exactly 1.0; test it) and pans equal the strip pans, so `mix` output is bit-identical to today's. A test compares `mix_with(default)` against a copy of the old `mix` kept in the test file.
- Stem printing (`engine::print`): `print_stem(stems, band, seed, &MixSettings, id, duck: Option<&[f32]>, gain: f32) -> Option<Stereo>` gives one track as it enters the mix bus (post fader, post pan, ducked, slapback included, dry), times `gain`; `print_reverb(...) -> Stereo` gives the FDN return; `mix_gain(...)` is the final normalisation factor of the full mix. One stem at a time, so peak memory is one stereo stem plus the duck track (about 100 MB for the demo), not all of them. Sum of printed stems plus reverb equals the mix before the bus compressor, times the normalisation gain; the bus compressor is not in the stems (documented in the manifest).

### 3.4 CLI

- `--stems`: writes `<stem>.stems/<track>.flac` (24-bit, stereo) for every audible track, `reverb.flac`, and `<stem>.stems/stems.json` (`{"tracks": [...], "gain": g, "extra_gain": e, "note": "..."}`). When any printed sample exceeds 1.0, all stems are scaled by `e = 0.99 / max` and `e` is recorded.
- Mix sidecar: `--mix FILE` applies it; otherwise `<out-stem>.mix.json` is applied when present (printed); `--no-mix` ignores it. The render sidecar records the path in `mix`.

### 3.5 Studio mixer panel

- Opening the Mixer (a right-side panel) needs the stems: the render job now returns the `Stems` it made (kept in the app as `Arc<Stems>`, about 0.4 GB for 3 minutes; dropped on song change). For a song rendered earlier, "Load stems" re-renders in the background with the sidecar's seed and voices (deterministic; about 2 s threaded).
- One strip per present track in `TrackId` order (label with the singer's voice for the leads: "Lead A (Baritone)"): fader -60..+6 dB with a -inf detent, pan slider, M and S toggles; a Ducking slider 0..10 dB; Reset; Save (writes `<stem>.mix.json`); Apply (re-mixes and re-encodes `<stem>.ogg` with `export.ogg_quality`, updates the sidecar); Export stems.
- A change starts a re-mix job 250 ms after the last edit (earlier jobs are dropped when they finish). Playback switches to the in-memory mix at the current position (`rodio` samples buffer plus seek). Aim: re-mix of the demo in 1.0 s or less on this machine (i7-9750H); measure and report.

## 4. Duets

### 4.1 Musical model

A song is solo or duet. A duet names singer A (the top-level `voice`) and singer B. Each lyric line is sung by A, by B, or by both. On a shared line one singer carries the melody and the other sings a harmony (a third to a sixth away, chord tones first; the existing `harmony_line` rule) or the melody in another octave. The classic male/female duet is A male (baritone or tenor), B female (alto or soprano), but any two voice types are legal, including two of the same type.

### 4.2 Wire format

All new fields are optional; a song without them is a solo song as today.

```json
{
  "title": "Porch Light", "note": "...", "key": "D", "mode": "major", "meter": "4/4", "tempo": 92,
  "guitar": "travis", "voice": "baritone",
  "phrasing": {"delivery": "flowing", "endings": "released"},
  "duet": {"voice": "alto", "phrasing": {"delivery": "legato", "endings": "held"}},
  "band": {"drums": "brushes", "bass": true, "harmonyGuitar": true, "harp": false,
           "violin": true, "choir": false, "harmonies": false, "doubles": true},
  "sections": [
    {"type": "intro", "chords": ["D", "G", "D", "A"]},
    {"type": "verse", "sing": "A", "lines": [
      {"syl": "...", "ph": "...", "chords": ["D", "G"]},
      {"syl": "...", "ph": "...", "chords": ["D", "A"]}]},
    {"type": "verse", "sing": "B", "lines": [
      {"syl": "...", "ph": "...", "chords": ["D", "G"]},
      {"syl": "...", "ph": "...", "chords": ["D", "A"], "sing": "A"}]},
    {"type": "chorus", "sing": "both", "lead": "B", "blend": "harmony", "lines": [
      {"syl": "...", "ph": "...", "chords": ["G", "D"]},
      {"syl": "...", "ph": "...", "chords": ["A", "D"], "lead": "A", "blend": "octave"}]},
    {"type": "chorus", "same": true}
  ]
}
```

- `duet`: `{"voice": <voice>, "phrasing"?: <phrasing>}`. Present and valid: a duet.
- `sing` on a section: the default part of its lines; on a line: that line's part. Values `A`, `B`, `both` (case-insensitive). Default `A`.
- `lead` (who carries the melody on a shared line; default `A`) and `blend` (`harmony` | `octave`; default `harmony`), on a section as a default or on a line.
- `same: true` copies the lines with their parts.

Schema (`song::schema`): new optional properties `phrasing`, `duet` (top level), `sing`, `lead`, `blend` (section and line), with enum lists from the enums (`SingerPart::NAMES` etc.); every new object closed. `required` lists do not change.

### 4.3 Typed model (`song::model`)

```rust
named_enum! { pub enum SingerId ("singer") { A = "A", B = "B" } }
named_enum! { pub enum Blend ("blend") { Harmony = "harmony", Octave = "octave" } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum Part { Solo(SingerId), Both { melody: SingerId, blend: Blend } }
impl Default for Part { fn default() -> Part { Part::Solo(SingerId::A) } }
impl Part { pub fn melody(self) -> SingerId; pub fn other(self) -> Option<(SingerId, Blend)>; }

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Duet { pub voice: Voice, pub phrasing: Option<Phrasing> }

// Song gains:
pub phrasing: Option<Phrasing>,   // singer A and the song default (feature 5)
pub duet: Option<Duet>,
// Line gains:
pub part: Part,
// Song methods:
pub fn is_duet(&self) -> bool;
pub fn voice_of(&self, s: SingerId) -> Option<Voice>;     // A: Some(voice); B: duet voice
pub fn phrasing_of(&self, s: SingerId) -> Phrasing;       // B: duet.phrasing, else song.phrasing; else Phrasing::default()
```

### 4.4 Normalisation (`song::wire`)

Rules, each with its `Repair` (nothing silent):

| Input | Result | Repair |
|---|---|---|
| `duet` not an object, or its voice absent or unknown | solo | `DuetDropped { reason }` |
| `sing` text not A, B or both | A | `UnknownPart { section, line: Option, text }` |
| B or both in a solo song | A | `PartWithoutDuet { section, line }` (once per section) |
| `lead` or `blend` on a line that is not shared | ignored | `IgnoredPartField { section, line, field }` |
| `lead` text not A or B, `blend` unknown | default | `DefaultedField` with the path |
| duet where B sings no line | solo | `UnusedDuet` |
| `phrasing` not an object; a field unknown | default for that field; `None` if the object is unusable | `DefaultedField { "phrasing.delivery" }` etc. |

`to_wire` writes `phrasing`, `duet`, and `sing`/`lead`/`blend` per line (only when not the default; sections get no `sing`), so `normalize_value(&to_wire(&s))` gives `s` back.

### 4.5 Composition (`compose`)

Algorithm "duet register fit": compose once in relative register (as today), with singer B's lines offset, then choose one transposition for both singers.

1. Register relation of B to A: `delta = centre(B) - centre(A)` in semitones; octave `o = round(delta / 12)` in -1..=1; residual `d = clamp(delta - 12 o, -5, 5)`. Baritone/alto: delta 10, o = 1, d = -2.
2. `FormLine` gains `part`. Each line is composed by `compose_line` as today; a line whose melody singer is B gets `LineSpec.center += d`. `LineKey` gains the melody singer, so a line sung by A and the same line sung by B are composed separately. Every `LeadNote` gains `singer: SingerId` (the melody singer); B's melody notes get `+12 o` after composition.
3. One key for both (`choose_transpose_duet`): the existing range penalty (3 per semitone of the 5th/95th percentile outside the range, 0.8 per semitone of the median from the centre) computed per singer over that singer's melody notes, weighted by that singer's share of melody notes, plus 0.55 per semitone of the shift from a whole octave, over t in -30..=30. The solo case calls the same penalty with one singer, so it returns today's value (the `compose_once_equals_two_passes` test and the strict gate prove it).
4. Shared lines: `Comp.second: Vec<LeadNote>` holds the other singer's notes, same rhythm and syllables, `singer` = the other singer, no grace notes:
   - `octave`: melody + 12k, k in {-1, 0, 1} minimising the other singer's range penalty for that line (k = 0 is unison; two singers of one type sing unison).
   - `harmony`: `harmony_line` over the line's melody with `up = centre(other) >= centre(melody singer)`, then + 12k with k in {0, +1} (up) or {0, -1} (down) minimising the other singer's penalty.
5. `time_notes` times `second` as it times the melody.
6. API: `Prepared` gains `voice_b: Option<Voice>`; `prepare_voices(song, seed, VoiceChoice { a: Option<Voice>, b: Option<Voice> })` (overrides; `b` ignored in a solo song); `prepare(song, seed, voice)` stays as a shim with `b: None`.

Solo songs: `d` is never applied, the key gains a constant singer field, and the penalty is unchanged: output identical to today.

### 4.6 Arrangement and voice

- `arrange::Vocals`: `lead` is singer A; new `lead_b: Option<Singer>`; `doubles: Vec<Singer>`. A singer's notes are its melody notes plus its `second` notes, sorted by onset (no overlap: lines do not overlap). Singer B: voice B, `SingStyle::LEAD` with B's phrasing.
- Harmony vocal: in a duet it sings only lifted lines that are solo lines, and its voice type excludes both A and B.
- Doubles: two takes per melody singer in repeated lifted sections (A: seed indices 2, 3 as today; B: 4, 5), each take in its singer's voice. The doubles stem sums the takes in index order.
- Choir: unchanged.
- Engine: `TrackId::LeadB` (name `lead_b`, label "Lead vocal B"), inserted after `Lead`: `N_TRACKS` 11. The task count follows the arrangement: 14 in a solo song as today, plus the lead B task and B's two doubles takes in a duet; progress reports the real total. Strip as `Lead` (same EQ, compressor, slapback, gain), pan from `MixSettings` (-0.25 / +0.25 in a duet). Singer seed index `LEAD_B = 6`. Both leads key the ducker and are never ducked.
- `engine::render_with(song, seed, VoiceChoice, progress)`; `render` stays as a shim.
- `SongSheet` gains `voice_b: Option<Voice>`; `SheetLine` gains `singer: Option<SheetPart>` (`{ part: "A" | "B" | "both", melody: "A" | "B", label: "Baritone" | "Alto" | "Baritone + Alto" }`); `to_text` prefixes shared and B lines with `[B]` or `[A+B]` in a duet.

### 4.7 Prompt (`songwriter`)

New prompt block SINGERS, after STYLE (wording is the product's; this is its content):

- Solo or duet is your choice unless the request says. A duet suits a courtship, a quarrel, a dialogue, a story told from two sides, a call-and-response work song; it does not suit a private confession or a narrative in one voice.
- Style fit: each style has `duet: DuetFit { Welcome, Occasional, Rare }`: Welcome for bakersfield, nashville, texas, cajun, zydeco, americana, laurel, revival, gospel; Rare for appalachian, broadside, irishair, scottish, welsh, blues; Occasional for the rest. The prompt says "In this style a duet is common | occasional | rare".
- The classic duet is a man and a woman: A baritone or tenor, B alto or soprano.
- How parts trade: verses split (A takes one, B the next, or they alternate lines), the chorus together (the melody with whoever the song belongs to, the other in harmony), the bridge as call and response (alternate lines), the last chorus together; octave blend for a unison hook line.
- When the user asked for a duet or a solo, or named voices (`WriteOptions.duet: DuetRequest { Auto, Solo, Duet { a: Option<Voice>, b: Option<Voice> } }` from `--duet`, `--solo`, the settings `songwriter.duet`), the prompt says so and the choice is not Claude's.
- The reply template shows `phrasing` always, and `duet`, `sing`, `lead`, `blend` as optional fields with one line each on when to use them.

`write_song_with(claude, &WriteRequest, &WriteOptions, rng)` carries the new options; `write_song` stays as a shim with `WriteOptions::default()`, so no struct literal in other crates breaks. No new random draws (the style and register streams stay as they are).

### 4.8 Notation and studio

- Lead sheet: systems stay one per lyric line. A line sung by one singer is engraved on one staff in that singer's clef (treble, or treble 8vb for bass, baritone, tenor), with the label "A", "B" (first system: "A (Baritone)") at the system start. A shared line gets two staves joined by a bracket, the higher singer on top, each with its own notes and the lyric under both; the melody staff is marked "melody" in small italics. Note boxes: both staves' notes, in time order.
- Full score: a staff for singer B under singer A (feature 6).
- Studio lyrics view: a singer label column ("A", "B", "A+B") and per-singer colours from a palette with light and dark values (A blue, B rose, both violet); the singer legend with voice names under the title.

## 5. Singer phrasing

### 5.1 Model

A writing decision, per song (singer A and default) and per singer in a duet:

```rust
named_enum! { pub enum Delivery ("delivery") { Legato = "legato", Flowing = "flowing", Parlando = "parlando", Detached = "detached" } }
named_enum! { pub enum Endings ("endings") { Held = "held", Released = "released", Clipped = "clipped" } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Phrasing { pub delivery: Delivery, pub endings: Endings }
impl Default for Phrasing   // Flowing, Released: today's articulation exactly
```

Wire: `"phrasing": {"delivery": "parlando", "endings": "clipped"}` (4.2). Absent: the style's default, filled by `Style::apply` when `song.phrasing` is `None` (a styled song gets its style's phrasing; an unstyled song, like the demo, gets `Phrasing::default()`). Style defaults (field `phrasing` on `Style`): legato for irishair, scottish, welsh, gospel; parlando for blues, broadside, texas, cowboy; detached for oldtime, bluegrass, cajun, zydeco, shanty, irishpub; flowing otherwise. Endings: held for gospel, revival, nashville, irishair; clipped for oldtime, bluegrass, shanty, zydeco; released otherwise.

`song::events::SingStyle` gains `phrasing: Phrasing` (`SingStyle::LEAD` has the default). Lead, harmony and doubles take their singer's phrasing; the choir keeps the default.

### 5.2 Parameters (`voice::phrasing`)

`PhrasingParams::of(Phrasing)`; Flowing/Released reproduces the current constants exactly.

| Delivery | sustain | onset_share | lead_in | vibrato | glide | swell | breath |
|---|---|---|---|---|---|---|---|
| legato | 1.00 | 0.35 | 1.0 | 1.0 | 1.4 | 0.10 | 1.0 |
| flowing (today) | 1.00 | 0.45 | 1.0 | 1.0 | 1.0 | 0.16 | 1.0 |
| parlando | 0.85 | 0.55 | 0.6 | 0.5 | 0.6 | 0.00 | 0.7 |
| detached | 0.65 | 0.45 | 1.0 | 0.6 | 0.5 | 0.05 | 1.0 |

| Endings | end_len | fade_depth | fade_from |
|---|---|---|---|
| held | 1.00 | 0.25 | 0.70 |
| released (today) | 1.00 | 0.40 | 0.55 |
| clipped | 0.60 | 0.15 | 0.60 |

- sustain: a non-final note ends at `min(t1, t0 + max(0.05, sustain * (next.t0 - t0)))`. 1.0 leaves `t1` alone.
- onset_share: replaces `ONSET_SHARE` (share of the inter-onset interval the onset consonants may take).
- lead_in: the share of the onset consonants placed before the note onset; the rest pushes the vowel start later: consonants span `[t0 - lead_in * D, t0 + (1 - lead_in) * D]`, the vowel starts at the span end. 1.0 is today (vowel on the beat); 0.6 lands the consonant on the beat, as in speech.
- vibrato and glide: scales multiplied into `VoiceSettings.vibrato_scale` and `.glide`.
- swell: the per-note swell depth in `shape_dynamics` (0.16 today).
- breath: scale on the breath-noise level of pre-phrase breaths (`BREATH_AH`).
- end_len: the phrase-final note keeps this share of its written length (after compose's 0.05 s trim).
- fade_depth, fade_from: the phrase-end fade `1 - depth * smoothstep(from, 1, x)` (0.4, 0.55 today).

Algorithm names: time-scale shaping of note spans (sustain, end_len) before articulation; the rest are parameters of the existing synthesis-by-rule plan (Holmes, Mattingly, Shearme 1964; Klatt 1987) and the phrase-end fade. `phrase_notes(notes, &PhrasingParams) -> Cow<[VocalNote]>` returns the input borrowed when sustain and end_len are 1.0, so the default path cannot change a sample.

## 6. Full multipart score

### 6.1 What it shows

Every sounding part from the arrangement's note events, one system of all parts per line of music (the lead sheet's chunks: one per lyric line, one per run of instrumental bars, wrapped when too wide), staves in this order:

| Group (bracket) | Staff | Clef | Source |
|---|---|---|---|
| Vocals | Lead A | treble or treble 8vb by voice | `Vocals.lead` |
| | Lead B | same rule | `Vocals.lead_b` (wave 3) |
| | Harmony | by voice | `Vocals.harmony` |
| | Doubles | by voice | the first take (the takes sing the same notes) |
| Choir | S, A, T, B | treble, treble, treble 8vb, bass | `arrange::choir::voicings` (beats), not the humanised singers |
| Band | Violin | treble | `Arrangement.violin` |
| | Guitar | treble 8vb | the six `StringNote` lists |
| | Harmony guitar | treble 8vb | `harmony_guitar.lead` and `.arp` |
| Harp (brace) | upper, lower | treble, bass; split at MIDI 60 | `Arrangement.harp` |
| | Bass | bass 8vb (written an octave up) | `Arrangement.bass` |
| Drums | Drums | percussion | `Arrangement.drums` |

A staff appears when its part plays (`song.band`) and has at least one event. Part names in full on the first system, abbreviated after (Voc., Hrm., Dbl., S. A. T. B., Vln., Gtr., H.Gtr., Hp., Bs., Dr.). Chord symbols above the top staff and the guitar staff. Lyrics under the lead staves. Section labels above the top staff. Key signature on every pitched staff; none on drums.

### 6.2 Algorithms

- Time to beats: `Timeline::to_beat(t)`, the inverse of `to_time` (binary search on the beat times, linear inside a beat; the ritard is kept).
- Quantisation: onsets to the nearest sixteenth unit of `Grid` (the lead sheet's grid; 6/8 uses its eighth units); a humanised onset (strum spread, lateness, choir jitter) snaps to the grid. Ends quantise the same way, at least one unit after the onset, cut at the next onset in the same voice (monophonic voices; plucked strings are notated to the next onset or their damping, whichever comes first).
- Chords: onsets within half a unit on one staff merge into one chord; the chord lasts the shortest member's quantised length. Guitar strums become chords this way; rolled harp chords too.
- Harp: notes below MIDI 60 go to the lower staff.
- Drums: two voices. Up (hands): snare, tap, rim, hat, ride, toms, shaker, swish. Down (feet): kick. Positions on the five-line staff (standard drum-set notation, Weinberg, Guide to Standardized Drumset Notation, PAS 1998):

| Hit | Staff position | Notehead |
|---|---|---|
| Kick | bottom space (F4) | normal |
| Snare, Tap | third space (C5) | normal |
| Rim | third space | x |
| Hat | space above the staff (G5) | x |
| Ride | top line (F5) | x |
| Tom | E5 above 150 Hz, A4 below | normal |
| Shaker | line above the staff (A5) | x |
| Swish | third space | slash, held for its duration, text "sw." |

  The kit name ("Brushes", "Kit") is printed at the start.
- Rests: gaps split into notatable rests by `Grid::split` (the lead sheet rule); a silent bar is a whole-bar rest. The full score has no multi-bar rests, because bars align across staves.
- Part view: one staff for one part (`FullScore::part(PartId)`), systems filled to the page width, and every run of two or more silent bars in one section as one multi-bar rest (`restHBar` with the count above). This is where multi-bar rests belong.
- Spelling: `spell` with the key signature of the transposed key (lead sheet rule).
- Voices on one staff: at most two (drums; the lead sheet duet uses two staves instead). Stems up for voice 1, down for voice 2; rests of voice 2 drawn low.

### 6.3 Glyphs

Bravura outlines exist only for what `src/glyphs.js` carried. The missing symbols are drawn as SVG geometry in a new module `notation::drawn`, each documented as drawn, not Bravura: F clef (a filled notehead-like curl plus two dots, fitted to the Bravura proportions: 2.6 spaces tall, dots either side of the F line), percussion clef (two thick vertical bars, 1 space tall, 0.5 space apart), x notehead (two crossed strokes in the notehead box), slash notehead, system bracket (thick 0.5-space bar with hooked ends), brace (a filled path of two mirrored cubic curves), and the "8" under the F clef. If Bravura data becomes available later, `tools/gen_glyphs.py` gains the SMuFL names and `drawn` is removed.

### 6.4 Page model and API

Same page model as the lead sheet: px at the SVG's own width, height from the content, `data-t0`/`data-t1` on each note group.

```rust
pub struct Page { pub svg: String, pub width: f64, pub height: f64, pub notes: Vec<TimedBox>, pub systems: Vec<TimedBox> }
pub enum Sheet { Lead(Score), Full(FullScore), Part(FullScore, PartId) }
impl Sheet { pub fn page(&self, width: f64) -> Page; pub fn title(&self) -> &str; }
```

For the full score, `notes` holds one box per onset column of a system, spanning every staff, so a player highlight moves along the system in time; `systems` spans the whole system. The lead-sheet functions (`engrave`, `note_boxes`, `system_boxes`, `page_size`) stay.

Module layout: `notation/src/full/model.rs` (parts, staves, quantised bars and voices), `notation/src/full/layout.rs` (multi-staff systems), `notation/src/drawn.rs`, `notation/src/page.rs` (`Page`, `Sheet`). Shared helpers of `layout.rs` (glyph, text, line, measure widths, head glyph by value, rest glyph by value) become `pub(crate)`. Notation gains a dependency on `arrange` (arrange depends on no audio crate).

`FullScore::new(song, &Prepared, &Arrangement)`. The studio builds the arrangement with `arrange::arrange(song, &prepared, seed)` (events only, milliseconds).

### 6.5 Studio

The view selector becomes Lyrics | Lead sheet | Full score | Part [part list] | Both (lyrics beside the lead sheet). `SheetView` takes a `notation::Sheet`; the highlight and follow logic reads `Page.notes` and `Page.systems` for every kind. Layout is built off the UI thread when it takes more than 50 ms (the full score of the demo); aim 300 ms or less.

## 7. Crate and module changes

| Crate | Change |
|---|---|
| settings | the crate (2.2) |
| songwriter | `usage` (Usage, ModelUsage, Generation), `sidecar` (RenderSidecar), Transport; CLI and API usage parsing; `WriteOptions`, `write_song_with`, `DuetRequest`; prompt SINGERS and PHRASING blocks; `Style.duet`, `Style.phrasing`; `Style::apply` fills phrasing |
| song | Phrasing, Delivery, Endings, SingerId, Blend, Part, Duet; `Song.phrasing`, `Song.duet`, `Line.part`; wire, schema, repairs, to_wire; `SingStyle.phrasing` (events) |
| compose | `Timeline::to_beat`; `FormLine.part`; `LeadNote.singer`; duet register fit; `Comp.second`; `Prepared.voice_b`; `prepare_voices`, `VoiceChoice` |
| voice | `phrasing` module; articulation and controls read `PhrasingParams` |
| arrange | `Vocals.lead_b`, `Vocals.doubles: Vec`; duet-aware harmony |
| engine | `mixset` (MixSettings), `print` (stem printing), `mix_with`; `ProcessedStem.slap`; `TrackId::LeadB`; `render_with`; duet ducking key; sheet singer tags; `demo_duet.json`; stems example `--song` |
| notation | `full` (model, layout), `drawn`, `page` (Page, Sheet); lead sheet duet staves; dep on arrange |
| sunflower | settings; usage line; `--effort`; `--quality` default from settings; `--stems`; `--mix`, `--no-mix`; `--voice-b`; `write --duet [A,B] / --solo`; sidecar via `songwriter::sidecar` |
| studio | Settings window; usage under the title; mixer panel; in-memory playback; lyrics singer labels and colours; view selector with full score and parts; sidecar via `songwriter::sidecar`; `Via` replaced by `Transport` |
| scripts/gate.sh | `--song demo|duet` (default demo); the duet baseline in `tests/soundgate/duet/` |

Dependency graph additions: settings -> song, songwriter, serde, toml; notation -> arrange; studio -> settings, arrange; sunflower -> settings.

## 8. Testing and the sound gate

### 8.1 Tests per feature

1. Usage: CLI envelope fixture with every field of the task statement (usage, output_tokens_details, modelUsage with two models, total_cost_usd, duration_ms) parses to the expected `Reply`; a minimal envelope (`result` only) parses with all usage None; API fixture; answered-model choice (most output tokens; tie to requested); `Generation::summary` text for full and partial records; `RenderSidecar` round trip and keep-generation-on-rewrite; old sidecars (no `generation`) still read.
2. Settings: defaults; `config_path` with `XDG_CONFIG_HOME` set, empty, unset, and `SUNFLOWER_CONFIG`; bad values and unknown keys give warnings and defaults; syntax error gives defaults; save then load gives the same `Settings`; `~` expansion. CLI: an integration test runs `sunflower render` with `SUNFLOWER_CONFIG` pointing at a file with `ogg_quality` and checks the sidecar or output; flags beat the file.
3. Mixer: `mix_with(default)` bit-identical to the old `mix` (copy kept in the test); `db_to_gain(0.0) == 1.0`; muting a non-lead track equals switching its band part off, bit for bit; solo of one track equals muting all others; `duck_db = 0` equals a mix with no ducker; `+6 dB` on a track raises its route gain by 10^(6/20) exactly; mix.json round trip and lenient reading; printed stems plus reverb equal the pre-compressor mix times the gain within 1e-5 of peak; CLI `--stems` writes one FLAC per audible track plus reverb and the manifest.
4. Duets: normalise (every repair in 4.4; `same` copies parts; to_wire round trip on the duet demo); schema enum lists equal the enums; compose: solo output unchanged (`compose_once_equals_two_passes` still passes; strict gate), duet demo over 20 seeds: at least 90% of each singer's notes inside that singer's range, B's melody median within 4 semitones of B's centre, second voice has the same onsets and syllables as the melody on every shared line, octave blend is melody + 12k exactly; arrange: `lead_b` present iff duet, each singer's notes sorted and non-overlapping; engine: `LeadB` stem present in the duet, absent in solo; thread invariance on the duet; sheet singer tags; notation: two staves on shared lines, labels in the SVG; studio: the duet demo loads.
5. Phrasing: `PhrasingParams::of(Phrasing::default())` equals the current constants; `phrase_notes` borrows on the default; sustain and end_len caps; lead_in moves the vowel start by `(1 - lead_in) * D`; a probe (`voice/examples/phrasing.rs`) renders the demo lead with each delivery and ending and prints the voiced fraction of each inter-onset interval and the mean gap: detached < parlando < flowing <= legato; normalise and `Style::apply` fill rules; the prompt names the style's phrasing.
6. Full score: every arrangement event of a playing part lands in exactly one staff (count check per part, after chord merging); each voice of each bar sums to the bar length; drum positions and noteheads per the table; harp split at 60; choir clefs; part view merges silent runs into multi-bar rests with the right counts; the SVG parses with `usvg`; column boxes are in time order and inside their system boxes; layout of the demo full score takes 300 ms or less (reported, not enforced).

### 8.2 Sound gate

`scripts/gate.sh` as it is (8 seeds of the demo, take-robust LTAS mean, vowel distance, Helmholtz, perf, thread invariance), plus `--strict` for the per-seed 0.5/1 dB check.

What changes sound:

| Feature | Changes the sound of | Demo stems |
|---|---|---|
| 1 usage, 2 settings | nothing | identical |
| 3 stems and mixer | only a non-default mix (user settings) | identical (default mix is bit-identical) |
| 4 duets | duet songs only | identical (the demo is solo) |
| 5 phrasing | songs with a phrasing field or a style whose default is not flowing/released | identical (the demo has no style and no phrasing) |
| 6 full score | nothing | identical |

Policy:

- Every wave runs `scripts/gate.sh --strict wN` on the demo. The demo must pass strict in every wave: no feature changes it. A strict failure is a bug, not a re-baseline.
- The duet check: wave 2 adds `crates/engine/src/demo_duet.json` (a duet, baritone and alto, B with its own phrasing) and `scripts/gate.sh --song duet`, which runs the same measurements on it (lead and lead_b pitch included) against `tests/soundgate/duet/baseline-mean/`. Wave 2 captures that baseline (`--song duet --capture-baseline`) from the tree with the whole wave merged, in its own commit, and sends the renders to the owner. Later waves run `--song duet` in default (mean) mode against it.
- Phrasing is checked by the probe of 8.1 item 5 and by rendering the demo with each style applied (`sunflower render crates/engine/src/demo.json --style S --seed 1234 -o out/phr/S.wav` for S in appalachian, blues, bluegrass, gospel): the renders go to the owner. The owner waived listen-before-rebaseline: a rough about-right check suffices.
- Perf numbers are aims: re-mix 1.0 s, full-score layout 0.3 s, duet render within 10% of the solo demo render. The gate fails only on the existing +50% rule.

## 9. Build order

Waves run in order; units in a wave run in parallel on disjoint files; the workspace builds and the gate passes at the end of every wave. Old public signatures stay as shims where a caller is owned by another unit (`mix`, `render`, `prepare`, `write_song`, `song_prompt`, `song_sheet`). Unit briefs are in `docs/features-2-plan.json`.

| Wave | Unit | Owns |
|---|---|---|
| 0 | Claude usage, sidecar type, Transport, settings crate | `crates/songwriter/src/{claude,lib,usage,sidecar}.rs`, `crates/songwriter/tests/`, `crates/settings/`, the test literal in `crates/studio/src/jobs.rs` |
| 0 | song model: duet parts and phrasing | `crates/song/` except `events.rs` |
| 0 | mixer: MixSettings, stem printing, mix sidecar | `crates/engine/src/{mix,strip,render,lib,mixset,print}.rs`, engine examples and tests |
| 0 | full-score model, `Timeline::to_beat` | `crates/notation/src/full/`, `notation/src/lib.rs`, `notation/Cargo.toml`, `crates/compose/src/timeline.rs` |
| 1 | CLI: settings, usage, sidecar, `--stems`, mix sidecar | `crates/sunflower/` |
| 1 | phrasing in the voice | `crates/voice/`, `crates/song/src/events.rs`, `crates/arrange/src/vocals.rs` |
| 1 | compose: duet register fit | `crates/compose/` |
| 1 | prompt and styles: duet, phrasing, WriteOptions | `crates/songwriter/src/{prompt,styles,lib}.rs`, `crates/songwriter/tests/` |
| 1 | full-score layout, drawn glyphs, Page and Sheet | `crates/notation/` |
| 2 | duet arrangement and engine, duet demo, gate `--song` | `crates/arrange/`, `crates/engine/`, `scripts/gate.sh`, `tests/soundgate/duet/` |
| 2 | studio: settings window, usage, duet request | `crates/studio/` |
| 2 | lead sheet for duets | `crates/notation/src/{score,layout}.rs`, notation tests |
| 3 | CLI: duet voices and request | `crates/sunflower/` |
| 3 | studio: mixer, duet lyrics, score views | `crates/studio/` |
| 3 | full score: singer B | `crates/notation/src/full/`, full-score tests |
| 4 | docs | `README.md`, `docs/engine-design.md`, this file |

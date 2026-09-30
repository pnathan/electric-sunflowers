# Song schema 3: additions over schema 2

Claude may sketch the lead melody in movable-do solfege, reuse a tune across sections (old tune, new words), and write the tune of an instrumental section with note lengths. Every field is optional. A song of version 1 or 2 renders exactly as before; a version-3 song with no tune renders as the same song of version 2.

## 1. Versions

| Version | Adds |
|---|---|
| 3 | `tune` (line and section, sung or instrumental); `tunes` (song); `energy` (section); `arranging` (song). |

`song::wire::resolve_version`: a document without `schema_version` that uses a version-3 field is read as 3 with `Repair::SchemaVersionInferred(3)`. In a document that declares 1 or 2, each version-3 field is dropped with `Repair::FieldNeedsSchema { needs: 3 }`. Greater than 3 is `SongError::UnsupportedSchema`. `song::schema`: `json_schema_v3`, now the latest (`json_schema()`, requires `schema_version: 3`); `json_schema_for(3)`.

## 2. Notation

A line's `tune` is a string of space-separated tokens, one per sung note of the line. A melisma counts every one of its notes (`*glo~3-ry` is four notes).

- `d r m f s l t`: the degrees at 0, 2, 4, 5, 7, 9, 11 semitones above the tonic of the section's key (`Song::key_at`). The mode does not move them; write `me` for a minor third.
- Raised `di ri fi si li` (1, 3, 6, 8, 10); lowered `ra me se le te` (1, 3, 6, 8, 10).
- A trailing `,` lowers a note one octave (`s,`); a trailing `'` raises it (`d'`). Marks repeat, to four.
- `.`: no hint; the engine chooses the note.
- `-`: not a note. It adds one beat of hold to the token before it. First version: the hold is parsed and kept (`TuneNote::hold`) as a length hint; the rhythm model ignores it.

Octave: a plain `d` is the tonic nearest the register pitch (`compose::melody::register_of`), so `d` sits within a tritone of the register. The hinted pitches of a line then move together by whole octaves, the fewest that fit the pitch window (register - 5 to register + 14 semitones); if the line is wider than the window, each note moves alone. A move records `Repair::TuneMoved` (in `Comp.repairs`).

## 3. Named tunes

- Song `tunes`: `{"NAME": ["tune line", ...]}`. The JSON schema sent to Claude gives a list, `[{"name": "NAME", "lines": [...]}]`, because strict schemas cannot describe free keys; the reader takes both.
- Section `tune: "NAME"`: lyric line i of the section uses `tunes[NAME][i % n]`, unless the line has its own `tune`. A `same: true` section copies the source's lines, tunes included.
- Two sections that share a name hinted the same notes by construction.

## 4. Repairs

- `TuneToken { section, line, token }`: a token that is not solfege; the line's tune is dropped.
- `TuneLength { section, line, tune, notes }`: the count differs from the line's notes; the tune is dropped.
- `UnknownTune { section, name }`: no such name, or no lines; the section's tune is ignored.
- `TuneMoved { section, line, octaves }`: see section 2.
- `BreakTuneToken { section, token }`: a break tune token that is not a note with a length; the tune is dropped.
- `BreakTuneBar { section, bar, ticks, expected }`: a bar of a break tune of the wrong length (ticks, 96 to the whole note); the tune is kept.
- `BreakTuneCut { section, tune, room }`: a break tune longer than its section; cut.
- `BreakTuneRange { section, notes }`: composition; written notes outside the violin range after placement.

## 5. Pitch

`compose::pitch::pitch_line` takes `PitchProblem.hints` (MIDI per note). The hinted pitch is always a candidate, even off the scale and off the chord; every other candidate of the note pays `PitchWeights::hint_miss` (-60, above any other score). A hinted note is also exempt from the tonic-only cadence rule. A line whose notes are all hinted comes out as hinted, for any seed. Free notes are composed around the hints as before. The tune is part of the composed-line cache key. `Line.tune` and `FormLine.tune` carry it; `to_wire` writes a line's resolved tune back as `tune`, and `tunes` is not written.

## 6. Break tunes

An instrumental section (intro, interlude, outro, a break) may carry `tune`: a tune with lengths that the lead instrument plays as written. The engine composes nothing for such a section. A section without a tune is composed as before (`compose_instrumental`: 4 to 6 slow notes per two bars).

Notation: the tokens of section 2, each followed by a length.

- Lengths are note values: `1` whole, `2` half, `4` quarter, `8` eighth, `16` sixteenth. A trailing `.` dots the note (`d4.`, one and a half times as long). Nothing else follows a length.
- `z` with a length is a rest (`z8`). `-` and `.` (hold, free) are not break tokens.
- `|` is a bar line. Bar lines are optional and do not affect timing. If the tune has any, each bar is checked against the meter; a bar of the wrong length records `Repair::BreakTuneBar` and the tune is kept.
- Example: `d8 d8 r8 m8 s4 m4 | r8 m8 r8 d8 t,4 s,4`.

Beats. A beat is as `Meter::grid` counts it: a quarter note in 4/4 and 3/4, a dotted quarter in 6/8. So a quarter lasts one beat in 4/4 and 3/4 and 2/3 of a beat in 6/8; an eighth lasts 1/2 beat in 4/4 and 3/4 and 1/3 beat in 6/8; `d4.` in 6/8 is one beat. A bar is 4 beats (4/4), 3 (3/4) or 2 (6/8), that is 8, 6 and 6 eighths. Internally a whole note is 96 ticks (`song::WHOLE_TICKS`; a beat is 24 ticks, or 36 in 6/8). In a stretched song (`Form::stretch` 2, dense lyrics spread over twice the bars) each written bar spans two form bars and the lengths double with it, so the tune keeps pace with its chords.

Fit to the section. A tune shorter than the section repeats from its start until the section is full (a dance tune's repeats); the last note is cut at the section's end. A tune longer than the section is cut at its end, with `Repair::BreakTuneCut`. The section's length is counted in written bars (its chord entries).

Naming. `tune` is first looked up in the song's `tunes`; a hit supplies its lines, concatenated in order (a list of lines, or one string). Any other text is the tune itself. A single word with no length that names nothing records `Repair::UnknownTune`. A token that is not a note with a length drops the tune and records `Repair::BreakTuneToken`. A `same: true` instrumental section (version 3, no lines, no chords) copies the last instrumental section of its kind, tune included.

Pitch. `d` is the tonic of the section's key, after the transposition for the voice (`prepare` does this; the tune's pitches are placed only then). The whole tune moves by whole octaves, never note by note. The placement is the one with the most notes inside the violin's lead range (MIDI 64-86, `arrange::violin`), then the one whose plain `d` is nearest MIDI 67-74 (then nearest their middle). A written note still outside the range records `Repair::BreakTuneRange { section, notes }` (in `Comp.repairs`; `section` is a form index); the violin folds that note by octaves as it folds any lead note. Notes inside the range come out exactly.

Playing. `InstNote::written` is true for these notes. The violin plays them at velocity 0.7 when they start on a beat and 0.6 otherwise, with vibrato only on notes of at least one beat; the bowing and the range are as for other lead notes. The harmony guitar plays them as it plays any instrumental note. `to_wire` writes the tune back as a string without bar lines, in the section's `tune`.

The JSON schema sent to Claude describes the field in `tune` (section): the same property serves sung sections (a name) and instrumental ones. The prompt teaches the notation and asks for a tune in every instrumental section, in the style's `break_tune` phrase (`songwriter::styles::Style::break_tune`: for example "a reel in running eighths or a jig in 6/8, with sixteenth cuts, bars in AABB repeats"; it changes no random draw).

## 7. Energy

A section may carry `"energy": "quiet" | "low" | "mid" | "high"`. It sets the section's arrangement intensity (`compose::form::Intensity`, which drives the drums (hats from mid), the guitar's strum density, the choir and the harp) directly, in place of the engine's build-up rules (intro quiet, first verse low, interlude and bridge low, later verses and first chorus mid, later choruses high). The lift bookkeeping (which sections are lifted, for the choir pad and the key change) does not change. Absent: the rules apply, so a song of version 1 or 2, and a version-3 song with no `energy`, renders as before. A `same: true` section takes its source's energy unless it gives its own. A value that is not one of the four records `Repair::DefaultedField { field: "sections.energy" }` and the rules apply. In a document that declares version 1 or 2 the field is dropped with `FieldNeedsSchema`. `to_wire` writes it back.

The prompt tells the writer that the curve is its decision: a stomp-along dance song runs mid or high from the first bar and does not build like a ballad. Each style's `drive` phrase (`songwriter::styles::Style::drive`) is quoted there, with the matching lyric density (about one syllable per eighth note in a dance); it changes no random draw.

Density. In a version-3 song the writer sets the lyric density. The engine stretches a song (`Form::stretch` 2, each bar played as two) only when its lines average more than one syllable per grid slot; versions 1 and 2 keep the old threshold (fewer than 1.75 slots per syllable), so they render as before.

## 8. Arranging note

A song may carry `"arranging": "free text"`: the writer's note to the arranger, in plain words (the feel and groove, what each instrument should do, where the energy peaks). There is no fixed vocabulary. The text is trimmed, empty text is absent, and text over `song::wire::ARRANGING_MAX_CHARS` (1500) is cut with `Repair::TruncatedField { field: "arranging" }`. In a document that declares version 1 or 2 the field is dropped with `FieldNeedsSchema`; in a document with no version it makes the document version 3. `Song::arranging` holds it and `to_wire` writes it back. The rule-based arranger ignores it, so a song renders the same with or without the note. Only the optional arranger pass (`sunflower write --arrange`, `sunflower rearrange`; `docs/engine-design.md` section 8a) reads it.

## 9. Example

```json
{"schema_version": 3, "title": "Old Tune", "key": "G", "mode": "major",
 "tunes": {"V": ["d d s, s", "l l s s -", "m m r", "d - - -"]},
 "sections": [
   {"type": "intro", "chords": ["G", "D"], "tune": "d8 d8 r8 m8 s4 m4 | r8 m8 r8 d8 t,4 s,4"},
   {"type": "verse", "tune": "V", "lines": [
     {"syl": "*wind *in the *pine", "chords": ["G", "C G"]},
     {"syl": "*dust on the *road", "chords": ["G", "D"]},
     {"syl": "*one more *mile", "chords": ["C", "G"]},
     {"syl": "*home", "chords": ["G"]}]},
   {"type": "chorus", "lines": [
     {"syl": "*roll *on *roll", "chords": ["G"], "tune": "s, d m"}]}]}
```

The intro's tune is one phrase of two bars. Each tune line has as many notes as its lyric line has syllables (`ph` is omitted here; the reader falls back to G2P).

# Song schema 3: additions over schema 2

Claude may sketch the lead melody in movable-do solfege, and reuse a tune across sections (old tune, new words). Every field is optional. A song of version 1 or 2 renders exactly as before; a version-3 song with no tune renders as the same song of version 2.

## 1. Versions

| Version | Adds |
|---|---|
| 3 | `tune` (line and section); `tunes` (song). |

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

## 5. Pitch

`compose::pitch::pitch_line` takes `PitchProblem.hints` (MIDI per note). The hinted pitch is always a candidate, even off the scale and off the chord; every other candidate of the note pays `PitchWeights::hint_miss` (-60, above any other score). A hinted note is also exempt from the tonic-only cadence rule. A line whose notes are all hinted comes out as hinted, for any seed. Free notes are composed around the hints as before. The tune is part of the composed-line cache key. `Line.tune` and `FormLine.tune` carry it; `to_wire` writes a line's resolved tune back as `tune`, and `tunes` is not written.

## 6. Example

```json
{"schema_version": 3, "title": "Old Tune", "key": "G", "mode": "major",
 "tunes": {"V": ["d d s, s", "l l s s -", "m m r", "d - - -"]},
 "sections": [
   {"type": "verse", "tune": "V", "lines": [
     {"syl": "*wind *in the *pine", "chords": ["G", "C G"]},
     {"syl": "*dust on the *road", "chords": ["G", "D"]},
     {"syl": "*one more *mile", "chords": ["C", "G"]},
     {"syl": "*home", "chords": ["G"]}]},
   {"type": "chorus", "lines": [
     {"syl": "*roll *on *roll", "chords": ["G"], "tune": "s, d m"}]}]}
```

Each tune line has as many notes as its lyric line has syllables (`ph` is omitted here; the reader falls back to G2P).

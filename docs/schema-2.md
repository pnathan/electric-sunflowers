# Song schema 2: design

Issues: #7 (choir sings words, call and response), #14 (key changes, rubato, melismas; harp validation), #12 (duplicated logic, done separately). The song JSON is Claude's writing; every field below is a writing decision made in the JSON. The engine adds no decision.

Invariant of every step: a song without a `schema_version`, or with `schema_version: 1`, and no version-2 field, renders exactly as before. The demo song has none, so `scripts/gate.sh --strict` proves it.

## 1. Versions

| Version | Adds |
|---|---|
| 1 | The original format. |
| 2 | `schema_version`; song and section `rubato`; section `key`; choir lines (`sing: "choir"`, `voicing`); melismas (`~N` in `syl`). |

Rules (`song::wire::resolve_version`):

- `schema_version` absent: version 1, silently; version 2 with `Repair::SchemaVersionInferred` when the document uses a version-2 field.
- 1 or 2: as declared. A version-2 field in a version-1 document is dropped with `Repair::FieldNeedsSchema`. A `~` in a version-1 lyric stays text.
- Greater than 2: `SongError::UnsupportedSchema`. Not a number, or below 1: `DefaultedField`, then as absent.
- `Song.schema_version` keeps the version read; `to_wire` writes it back (version 1 writes no `schema_version`, so a version-1 file round-trips byte for byte in meaning).
- `song::schema`: `json_schema_v1`, `json_schema_v2`, `json_schema()` (latest; requires `schema_version: 2`), `json_schema_for(n)`. The songwriter asks Claude for the latest.

Other versioned files: `<stem>.mix.json` (`version`: 1, unchanged), `<stem>.render.json` (`version`: 2 from this change; absent reads as 1), `<stem>.sheet.json` (`version`: 2; absent reads as 1). A reader ignores fields it does not know.

## 2. Fields

```json
{"schema_version": 2, "rubato": "light",
 "sections": [
   {"type": "verse", "lines": [ ... ]},
   {"type": "chorus", "lines": [
     {"syl": "*glo~3-ry *ha~-le~-lu~4-jah", "ph": "...", "chords": ["G", "C"]},
     {"syl": "*roll *ye *bold", "ph": "...", "chords": ["D"], "sing": "choir", "voicing": "unison"}]},
   {"type": "chorus", "same": true, "key": "A"},
   {"type": "outro", "rubato": "free", "chords": ["A", "D"]}]}
```

- `rubato`: `steady` (default) | `light` | `free`; song default, section override. Copies inherit the source's unless they give their own.
- `key` (section): `"E"`, `"A minor"`, `"D dorian"`. From this section until the next `key`. Same rule as the song key: note name, then an optional mode word; no word keeps the running mode. Equal to the running key is no change. A `same: true` section with a `key` copies the source's lines with every chord moved by the interval between the source's key and the new one; a `same: true` section without a `key` sounds in the running key, so it is moved likewise after an earlier change. Otherwise the section's chords are written in the new key.
- `sing: "choir"` (section default or line): the choir sings the line; neither lead does. `voicing`: `unison` (default: every choir voice on the tune) | `block` (four-part harmony, the tune on top). A choir line switches `band.choir` on (`Repair::ChoirEnabled`). `Part::Choir(voicing)`; `melody()` is A (the tune is composed in singer A's register).
- `~` or `~N` after a syllable's text (before a following hyphen): the syllable is sung over N notes, 2 to 4 (`MELISMA_MAX_NOTES`); bare `~` is 2; outside 2..=4 clamps with `Repair::ClampedMelisma`. `Syllable.notes` holds N.

Model additions: `Song.schema_version`, `Song.rubato`, `Section.key_change: Option<KeyChange { tonic, mode }>`, `Section.rubato: Option<Rubato>`, `Song::key_at(section)`, `Song::rubato_at(section)`, `Song::modulates()`, `Part::Choir(ChoirVoicing)`, `Syllable.notes`.

## 3. Rubato (`compose::timeline`)

Beat lengths, not events, carry rubato, so every consumer that uses `Timeline::to_time` follows it. For each sung line in a section whose rubato is not `steady`, over the line's beats `k = 0..n`: `x = k/(n-1)`, `g = x^3`, factor `f_k = 1 + A (g_k - mean(g))`. The mean is 0, so the line takes the same time as at tempo; the last beat is longest and the first is shortest. A = 0.10 (`light`), 0.24 (`free`). The final ritard multiplies the factor. Steady lines multiply by exactly 1.0, so the times are unchanged. Instrumental bars and lines of fewer than 2 beats are steady. No random draw: same song, same seed, same recording.

## 4. Melismas

A syllable with `notes = N > 1` is sung over N notes on one held vowel: the first note carries the onset consonants, the last carries the coda; the notes between are the vowel alone, legato, with no onset noise and no new breath. Rhythm: the N notes are extra note slots after the syllable's own, metric weight of the continuations low (they fall between the beats of the syllable), each at least a sixteenth. Pitch: continuation notes move by step (at most 2 scale steps; the pitch Viterbi gets a large penalty on leaps inside a melisma) and the syllable's last note resolves as any syllable's would. Notation: one lyric on the first note, an extension line under the rest, notes beamed or slurred. Sheet and lyrics view: one syllable, N notes. Solo lead, harmony and doubles sing melismas; the choir on words too (section 5).

## 5. Choir words and call and response

- Compose: a choir line is composed as a solo line of singer A (rhythm and pitch as today), then sung by the choir.
- Arrange: the lead, harmony and double singers rest on choir lines. The choir sings the line's syllables and phonemes (with melismas) instead of the /aa/ voicings, in its four parts (three singers each, the existing spread): `unison` puts every part on the tune in its own octave (bass and tenor an octave down); `block` puts the sopranos on the tune, and alto, tenor and bass on the nearest chord tones below in the standard order, no crossing. The /aa/ pad on lifted sections keeps its rule and does not sound on a choir line. The choir stem is not ducked under the lead; the accompaniment ducks under any singing (lead or choir).
- Voice: the choir singers use the lead's phrasing renderer with text; consonant durations scale per voice as they do now.
- Notation: choir staves show the lyric; a choir line shows on the lead staff as a rest with the text in italics and on the four choir staves in the full score.
- Sheet: `SheetPart { part: "choir", melody: "A", label: "Choir" }`.

## 6. Key changes

- `compose::form`: `Sec` gains `key: (Pc, Mode)`, the key in force, transposed by the form's `transpose`. Chords transpose as now; a section's spelling (flats or sharps) follows its own key.
- `compose::melody`, `compose::prepare`, `arrange`: every use of the song's tonic or mode reads the section's key (`local_scale`, lead note choice, harmony line, the harp's scale, guitar key).
- One transposition for the whole song (`choose_transpose`), chosen over all sections.
- Notation: each system takes the key signature of its section; the full score breaks a system at a key change.
- Sheet: `SongSheet.sections[i].key`; a key change shows in the lyrics view.

## 7. Harp

The harp body curve is derived from the guitar's. There is no harp reference recording, and none can be fetched from this machine, so it stays unvalidated. Status: documented open item in CLAUDE.md; issue #14 keeps that item.

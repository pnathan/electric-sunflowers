# CLAUDE.md: Singer-Songwriter Bot

Read this before changing anything. It records what the system is, why it is built this way, how to measure it, and what is open.

## Owner and conventions

- Owner: Paul. Prose: erudite, laconic, exact; no filler, no hedging, plain ASCII, minimal formatting. Banned words: "seams", "belt and braces", "load bearing", "shape of".
- Commits and pull requests: ASD-STE100 Simplified Technical English combined with Conventional Commits. Example: `fix(voice): make the /ey/ vowel start at [e]`.
- Deploy target: the claude.ai published artifact above. Publish the built dist file to the same URL so the link is updated in place; the declared capabilities are `{"sample":{}, "downloads":true}`.

## Product intent

- Claude writes only the song text and parameters: lyrics as stressed syllables, ARPAbet per syllable, chords per bar, key, mode, meter, tempo, voice, guitar pattern, band. Everything after that is procedural, with no model calls.
- Sound quality is the priority. The owner judges by ear; the tools below are proxies.
- Songwriter persona: Claude, a robot. Age counted from 1999 (first commercial deep learning), computed from the current year. Birthplace Menlo Park, home the American West, sensibility of someone born in 1999. Heritage Americana and its roots; world folk only as seasoning.
- Emotional register must match the request. Worn subjects (absent parent, lost lover, graves, empty chair, unsent letter, homecoming ending) are barred unless the prompt raises them.
- Styles fix form, meter, tempo range, modes, harmonic idiom, guitar pattern, band, drums and the break instrument. Songwriter's choice picks a style at random.

## Pipeline

1. `styleDirection(key)` (styles.js) picks style, mode, meter, tempo range, form. `songPrompt` (prompt.js) renders it, including a numbered form plan (`formText`).
2. The page calls `sample(prompt, {modelTier:'complex'})`, parses the JSON itself, and reads `modelTierApplied`; a downgrade is shown under the song. The platform does not expose the model name.
3. `normalizeSong` validates; `applyStyle` imposes the style's arrangement and clamps tempo.
4. `buildForm`: bars, lines, sections. Sections carry `lift` (choruses, or verses after the first when the form has no chorus), `liftIdx`, `final`, `intensity`. All arrangement features key off `lift`, not the chorus type.
5. `composeMelody`: `placeRhythm` (DP over the metric grid) then `pitchLine` (Viterbi over pitch pairs). A per-song `melodyProfile` sets range per section, contour type per section, leapiness, note repetition, chorus hook interval, and rhythmic character (dotted, even, syncopated).
6. `prepare`: choose transposition for the voice; `renderSong` makes all tracks; instrument bodies are applied by FFT convolution (`convStereo`) at render time.
7. `mixSong` (pure JS): per-track EQ, compression and gain are cached in `render.proc`, and raw tracks are freed; then pan, sends, an 8-line FDN reverb, a bus compressor and peak normalisation. Band toggles only re-sum, taking about 2 s.
8. `engraveSong` (notation.js) writes SVG; `encodeOpusWebm` (export.js) exports.

## Models and the decisions behind them

Numbers are from the tools below. Keep them current when you change a model.

- Voice (`voiceControls`, `synthVoice`):
  - Source: Liljencrants-Fant glottal pulse by Rd, blended between tense and lax tables by loudness; two-pole tilt.
  - Tract: a cascade of 5 formants plus 4 higher resonances (5.5-8.8 kHz, series), and a series high shelf of 16 dB at 5.2 kHz.
  - A parallel high-frequency branch was tried and removed. It filled every vowel's spectral valleys (/uw/ went from -55..-82 dB to -41 dB), which is heard as nasality; vowel distinctness fell from 13.6 to 10.6 dB.
  - Aspiration and breath noise are low-passed at 2.6 kHz so the shelf does not make them hiss.
- Consonants:
  - Stops: the legacy burst model plus CV formant transitions (50 ms from the consonant's locus) plus F1 damping during aspiration, with bursts at 0.8. The transitions gave most of the gain in D/T recognition. A later rework (closure murmur, soft bursts) measured worse and is behind `VF` switches, off.
  - Voiced "th" is mostly voicing (af .05). Consonant durations scale per voice: bass 1.15, baritone 1.2, tenor 1.25, alto 1.35, soprano 1.4.
  - /ey/ uses targets ey0 [450,2020,2600] and ey1 [340,2210,2780] (Hillenbrand). Starting on /eh/ gave a foreign "mehk".
- Plucked strings (`pluck`):
  - The loop runs in the velocity domain. Differentiating displacement produced a spike every period.
  - Two polarisations; excitation is the pluck shape plus a release low-pass; loss by material (guitar damp .18, harp .16, bass .5).
  - Guitar extras: pick noise, a tension pitch glide, and sympathetic open strings (`GT`).
- Bodies: `BODY_CURVES` were measured from University of Iowa MIS recordings (tools/extract_curves.py). `bodyIRData` turns them into a stereo modal impulse response. The guitar curve has a steel-string correction; the harp curve is derived from the guitar curve (no harp reference exists).
- Violin (`renderViolin`):
  - A digital waveguide bowed string with a friction table. The bow position tracks the note (beta 0.13-0.15, 60 ms glide), and bow force is 0.6 plus a pitch-dependent term.
  - Stability: 213/216 single notes and 174/174 phrase notes hold Helmholtz motion (tests/sweep2.js). The filtered-sawtooth version scored 0.01 as violin.
- Choir: 3 individuated singers per part. The vowel is /aa/ only: rounded back vowels (uw, oh, ao) score 0.45-0.64 as "Organ".
- Drums: the brush swirl is 9 dB lower and stroke-modulated; the taps have a head tone and snare wires. The old swirl made the mix classify as "Sanding".

## Measurement loop (the "ear")

Nobody on the machine side can listen, so every change is judged by at least one of these, against a saved baseline:

- `tools/ear.py LABELS files...`: PANNs Cnn14 AudioSet tagger. Calibrate on the Iowa references first. Real guitar scores "Electronic tuner" about 0.57, so that label is not a defect.
- `tools/asr2.py tags...` after `tests/sing2.js TAG '{json flags}'`: Whisper base.en on 16 sung lines rich in D and T (tests/words.js), baritone and alto. Hypotheses are capped at the reference length plus 3 words. Noise is about plus or minus 3 D/T words; trust only large differences.
- `tools/cmp.py`, `tools/body.py`, `tools/ltas.py`: 1/3-octave spectra and body curves against the references. `tools/spg2.py`: spectrogram images. `tools/pit.py`: pitch and subharmonic check.
- `tests/vow.js [engine]`: mean spectral distance between ten sustained vowels, 200 Hz-2.5 kHz. The good reference is 13.2-13.6 dB.

Latest readings: violin 0.47-0.74 as violin; choir organ 0.11; guitar 0.38 as guitar; word error for the baritone about 0.40-0.43 and the alto about 0.57; D/T recognised 58-68 of 118.

## Platform constraints

- Published claude.ai pages: scripts only from cdnjs, jsdelivr, the Tailwind CDN and jQuery; no Web Workers (blob: and data: are blocked by CSP), so rendering is single-threaded. localStorage works.
- The downloads allowlist excludes .wav and .ogg; audio is Opus in .webm (WebCodecs), with MediaRecorder as the real-time fallback.
- A standalone build needs a server or proxy for the Anthropic API (never ship a key in a page). With the API, name the model explicitly: `claude-opus-5-5`, or `claude-fable-5-1` for the Mythos tier.

## Rust engine

The Rust workspace (`crates/*`) is the long-lived engine; the JS page is its reference. Crates: `sfcore` (JS number rules, V8-exact math, seeded streams, tuning), `compose`, `dsp`, `voice`, `arrange`, `engine` (render_song, threaded path), `songwriter` (styles, prompt, `trait Claude` with `ClaudeCli` and `ClaudeApi`), `sunflower` (CLI: `demo`, `render`, `write`, `styles`).

- Parity: `tests/parity/gen.sh` writes JS reference data to `ref/parity/`; each crate's `tests/parity_*.rs` compares against it when built with `--features sfcore/v8,engine/capture_raw` and skips otherwise. The demo at seed 1234 matches `tests/mt.js` in all but 383 of 16.4M 16-bit samples, each off by 1 LSB. Voice audio differs by up to 6e-9 of peak on long renders; cause not found.
- Math goes through `sfcore::js`: std math by default, the V8-exact port under the `sfcore/v8` feature. Default and exact renders differ by at most 1 LSB at 16 bits; math is about 4.5% of render time.
- JS bugs are ported as they are, marked `JS parity:`. Fixes where the JS gives NaN are marked `Deviation from JS:`.
- `shapeFor`'s per-section contour (melodyProfile's `shape` table) is now live in both engines: the duplicate second JS definition that shadowed it is removed.
- `ClaudeCli` runs `claude -p` in an empty directory with `--strict-mcp-config`, `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and no tools; not `--bare`, which disables the logged-in account.
- Export (`crates/export`): Ogg Vorbis by default (about 160 kb/s VBR), FLAC and WAV by extension; tags carry title, artist, liner note, date and style.
- Timing, demo song: 6.3 s threaded (0.89 GB peak), 16.2 s on one core (0.72 GB); node takes 25 s and 0.93 GB.

## Open issues

1. Alto and soprano intelligibility trails the baritone. Next: vowel modification at high pitch (open the vowels upward, as trained sopranos do).
2. The first word of a phrase is the least reliable; no mechanism found yet.
3. The choir scores low as "Choir" in the bridge. The choir sings vowels only; call-and-response lyrics (shanty crew, gospel response) are not supported.
4. Render time in the page: about 17-24 s in Node on one core, and about 28-38 s in a headless browser. Memory is roughly 0.5 GB for a 3-minute song; older phones may fail. The Rust engine renders the demo in 6 s.
5. The real-model writing path has one live test (`sunflower write`, cowboy style, strophic form): the reply parsed and the form plan was followed. Registers and more styles are unverified.
6. No key changes, no rubato beyond the final ritard, no melismas; the harp has no reference validation.

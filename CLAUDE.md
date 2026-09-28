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
  - Breath noise is low-passed at 2.6 kHz so the shelf does not make it hiss. Aspiration is not; low-passing it too is open work, to be decided by ear and Whisper word error.
- Consonants:
  - Stops: the legacy burst model plus CV formant transitions (50 ms from the consonant's locus) plus F1 damping during aspiration, with bursts at 0.8. The transitions gave most of the gain in D/T recognition. A later rework (closure murmur, soft bursts) measured worse and is behind `VF` switches, off (deleted in the Rust engine).
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

Latest readings, JS engine (PANNs, Whisper): violin 0.47-0.74 as violin; choir organ 0.11; guitar 0.38 as guitar; word error for the baritone about 0.40-0.43 and the alto about 0.57; D/T recognised 58-68 of 118. Rust engine (sound gate, w5-final): vowel distance 13.38 dB; Helmholtz 212/216 single notes, 119/120 phrase notes.

## Platform constraints

- Published claude.ai pages: scripts only from cdnjs, jsdelivr, the Tailwind CDN and jQuery; no Web Workers (blob: and data: are blocked by CSP), so rendering is single-threaded. localStorage works.
- The downloads allowlist excludes .wav and .ogg; audio is Opus in .webm (WebCodecs), with MediaRecorder as the real-time fallback.
- A standalone build needs a server or proxy for the Anthropic API (never ship a key in a page). With the API, name the model explicitly: `claude-opus-5-5`, or `claude-fable-5-1` for the Mythos tier.

## Rust engine

`crates/*` is the long-lived engine: everything after Claude's JSON reply, offline and deterministic. It shares no code with the JS page; `docs/engine-design.md` is its design and records each model's algorithm and source.

- Crates: `sfcore` (math, flush-to-zero, xoshiro128++ streams keyed by seed, tag and event index, the seconds-to-sample rule); `song` (typed song model, the lenient JSON boundary with listed repairs, chords, ARPAbet, G2P, note-event types, reply schema); `dsp` (biquads, one-poles, Klatt resonators, delays and allpasses, ramps, stochastic processes, FFT over realfft/rustfft, overlap-add convolution, compressor, pan laws, FDN reverb); `instruments` (plucked string, guitar with sympathetic strings, modal body IRs, bowed string, drums); `voice` (LF source, formant cascade, articulation, phrase rendering); `compose` (form, timeline, rhythm and pitch Viterbi, melody profile, prepare); `arrange` (event planners for every part; no audio); `engine` (tracks, channel strips, sparse stems, one rayon task graph, block mixer); `export` (Ogg Vorbis, FLAC, WAV); `songwriter` (styles, forms, prompt, `trait Claude` with `ClaudeCli` and `ClaudeApi`); `sunflower` (CLI); `soundgate` (measurement only; depends on no engine crate).
- Build: `cargo build --release`. Dependencies are vendored in `vendor/` (`.cargo/config.toml`); no network is needed. The release profile uses fat LTO.
- Render: `sunflower demo -o x.wav`, `sunflower render song.json [--style KEY] [--no PART] -o x.flac`, `sunflower write "mood" [--style KEY] [--via cli|api]`, `sunflower styles`. `--seed` (u64) fixes the take; without it a seed is drawn and printed. `--voice` overrides the song's voice. The extension picks the format; Ogg by default. `RAYON_NUM_THREADS=1` is the single-thread path, and its output is bit-identical to the threaded one.
- `ClaudeCli` runs `claude -p` in an empty directory with `--strict-mcp-config`, `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1` and no tools; not `--bare`, which disables the logged-in account. The real-model path has one live test (open issue 5).
- Sound gate: `scripts/gate.sh [--strict] [--against DIR] LABEL` renders the demo stems for 8 seeds, compares the per-stem mean 1/3-octave LTAS with `tests/soundgate/baseline-mean/` (3 dB in 100 Hz-10 kHz, 6 dB outside, or 2 seed-to-seed sd capped at 6/9 dB; gated RMS 1.5 dB; active fraction 15 points; mix peak 0.89), checks lead pitch by YIN, vowel distance (12.8-14.0 dB), Helmholtz (>= 208/216), thread invariance, and times the demo. `--strict` also compares seeds 1234 and 2718 one by one at 0.5/1 dB, for changes that must not alter the sound. Perf fails only above +50% of the first row of `tests/soundgate/perf.tsv`. Baselines are replaced in their own commit, and the renders go to the owner for listening.
- Measured, demo at seed 1234 to WAV, i7-9750H: 2.16 s threaded (647 MB peak), 6.53 s on one thread (491 MB). The memory aims (0.50/0.40 GB) are not met: the sparse stem cache alone holds 22.5k 4096-frame blocks, 369 MB.
- Deliberate sound changes against the first Rust port: band-limited LF source tables (mip set per half-octave), per-sample ramps of tract coefficients, violin fractional delays read by 4-point Lagrange with the bridge pole re-tuned to 0.35, PolyBLEP ride cymbal, compressor gain interpolation, modal bodies with fixed low modes and stable band levels, TPDF dither at 16 bits, and new random streams, so each seed is a different take from the same distribution.

## Open issues

1. Alto and soprano intelligibility trails the baritone. Next: vowel modification at high pitch (open the vowels upward, as trained sopranos do).
2. The first word of a phrase is the least reliable; no mechanism found yet.
3. The choir scores low as "Choir" in the bridge. The choir sings vowels only; call-and-response lyrics (shanty crew, gospel response) are not supported.
4. Render time in the page: about 17-24 s in Node on one core, and about 28-38 s in a headless browser. Memory is roughly 0.5 GB for a 3-minute song; older phones may fail. The Rust engine renders the demo in 2.2 s threaded.
5. The real-model writing path has one live test (`sunflower write`, cowboy style, strophic form): the reply parsed and the form plan was followed. Registers and more styles are unverified.
6. No key changes, no rubato beyond the final ritard, no melismas; the harp has no reference validation.

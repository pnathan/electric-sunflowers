# CLAUDE.md: electric-sunflowers

Read this before changing anything. It records what the system is, how to work in it, how to measure it, and what is open.

## Owner and conventions

- Owner: Paul. Prose: erudite, laconic, exact; no filler, no hedging, plain ASCII, minimal formatting. Banned words: "seams", "belt and braces", "load bearing", "shape of".
- Commits: ASD-STE100 Simplified Technical English combined with Conventional Commits. Example: `fix(voice): make the /ey/ vowel start at [e]`. End each message with the session's attribution lines.
- Branch and remote: `trunk` is the key branch; origin is `git@github.com:pnathan/electric-sunflowers.git`. Standing practice: commit routinely on trunk and push after each verified step. Short-lived branches are fine for in-flight work; merge them back promptly. Never force-push or rewrite trunk. No CI/CD for now.
- Network: the sandbox blocks it. Pushing, fetching crates and live model calls through the API need a sandbox bypass. Dependencies are vendored (below), so builds and tests need none.
- Delegation (Workflow and Agent): architect/planner Opus at effort medium or high; coder, QA and testing Sonnet at effort medium (low stalls on large units); reviewer Opus at effort low or medium. Parallel agents own disjoint files or crates. Re-verify every agent's result yourself (build, tests, the sound gate) before committing.

## Product intent

- Claude writes the song; the engine works deterministically from there. Claude's JSON holds every writing decision: title, liner note, lyrics as stressed syllables, ARPAbet per syllable, chords per bar, key, mode, meter, tempo, voice, form sections, band. The engine never makes a writing decision and makes no model calls after the JSON. Same song and seed, same recording.
- Sound quality is the priority. The owner judges by ear; the tools below are proxies. For sound changes a rough "about right" check is enough to move on; send renders (Ogg) for listening at each sound-changing step.
- Songwriter persona: Claude, a robot. Age counted from 1999 (first commercial deep learning), computed from the current year. Birthplace Menlo Park, home the American West, sensibility of someone born in 1999. Heritage Americana and its roots; world folk only as seasoning.
- Emotional register must match the request. Worn subjects (absent parent, lost lover, graves, empty chair, unsent letter, homecoming ending) are barred unless the prompt raises them.
- Styles (22) fix form, meter, tempo range, modes, harmonic idiom, guitar pattern, band, drums and the break instrument. Songwriter's choice picks a style at random.

## Layout

    crates/core (sfcore)  math, flush-to-zero, xoshiro128++ streams keyed by seed/tag/event, time rules
    crates/song           typed song model, lenient JSON boundary with listed repairs, chords, ARPAbet, G2P, note events, schema
    crates/songwriter     styles, forms, prompt, trait Claude with ClaudeCli and ClaudeApi, write_song
    crates/compose        form, timeline, rhythm (DP) and pitch (Viterbi over pitch pairs), melody profile, prepare
    crates/arrange        event planners for every part; no audio
    crates/voice          LF glottal source, formant cascade, articulation, phrase rendering
    crates/instruments    plucked string, guitar with sympathetic strings, modal bodies, bowed string, drums
    crates/dsp            biquads, one-poles, Klatt resonators, delays, ramps, FFT (realfft/rustfft), convolution, compressor, pan, FDN reverb
    crates/engine         tracks and channel strips, sparse stems, one rayon task graph, block mixer with vocal ducking, song sheet
    crates/export         Ogg Vorbis (default), FLAC, WAV with tags
    crates/notation       lead-sheet engraving to SVG with Bravura glyphs, per-note times
    crates/sunflower      CLI: demo, render, write, sheet, styles
    crates/studio         desktop app (eframe, X11): library, lyrics with chords, sheet music, Ogg playback, new-song form
    crates/settings       user settings file (in progress)
    crates/soundgate      measurement for the sound gate; depends on no engine crate
    docs/                 engine-design.md (the design as built, every algorithm and source); rewrite-plan.json; features-2*.md/json (next batch)
    scripts/gate.sh       the sound gate
    src/, tests/, tools/  the JavaScript prototype (a claude.ai page) and its measurement tools
    vendor/               vendored crates

## Build and run

- Song JSON is versioned (`schema_version`, latest 2; `docs/schema-2.md`). A song without a version is version 1 and must render exactly as before. Change the format only by adding a version: new fields belong to the new version, `song::schema` keeps one schema per version, `wire::resolve_version` gates fields. `<stem>.render.json` and `<stem>.sheet.json` carry a `version` too.
- Bazel is the only build system on this machine; cargo is banned here: a PreToolUse hook in the gitignored `.claude/settings.local.json` runs `.claude/hooks/no-cargo.sh`, which denies any cargo command. `Cargo.toml` and `Cargo.lock` stay as the manifests `crate.from_cargo` reads. The Bazel setup (MODULE.bazel, per-crate BUILD.bazel, `scripts/bz` with one output base per agent and a shared disk cache) is being brought up; the cargo commands below are the old way and will be replaced.
- `cargo build --release`; `cargo test --release --workspace`. Dependencies are in `vendor/` via `.cargo/config.toml`. To add a crate: add it to the manifest, run `cargo vendor vendor` outside the sandbox, commit `vendor/`. `.gitignore` anchors `/target/` and never ignores `vendor/` (cargo checks every vendored file's checksum).
- CLI: `sunflower demo`, `sunflower render song.json [--style KEY] [--no PART] [--seed N] [--voice V] -o x.ogg`, `sunflower write "mood" [--style KEY] [--via cli|api] [--model ID]`, `sunflower sheet song.json --seed N`, `sunflower styles`. The extension picks the format. `render`, `write` and `demo` write `<stem>.render.json` (seed, voice, style) and `<stem>.sheet.json` beside the audio; `write` saves Claude's raw JSON as `<stem>.json` before validating it.
- Studio: `target/release/studio [SONG.json] [--dir DIR]`; the library defaults to `~/Music/sunflower`.
- `RAYON_NUM_THREADS=1` renders on one thread with bit-identical output.
- `ClaudeCli` runs `claude -p --output-format json` in an empty directory with `--strict-mcp-config`, `CLAUDE_CODE_DISABLE_CLAUDE_MDS=1`, `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` and no tools; not `--bare`, which disables the logged-in account. Default model `claude-opus-5-5`.

## Models and the decisions behind them

`docs/engine-design.md` has each model's algorithm, source and parameters. Decisions worth knowing before touching them:

- Voice: LF source by Rd from band-limited mip tables, tense and lax blended by loudness; a Klatt cascade of 5 formants plus 4 high resonances and a 16 dB shelf at 5.2 kHz, with coefficients ramped per sample. A parallel high-frequency branch was tried and removed: it filled the vowels' spectral valleys (heard as nasality). Breath noise is low-passed at 2.6 kHz; aspiration is not.
- Consonants: stop bursts plus 50 ms CV formant transitions from each consonant's locus (most of the D/T intelligibility gain). Voiced "th" is mostly voicing. Consonant durations scale per voice (bass 1.15 to soprano 1.4). /ey/ starts at [e] (Hillenbrand targets); starting on /eh/ gave a foreign "mehk".
- Plucked strings: extended Karplus-Strong in the velocity domain (differentiating displacement spiked every period), two polarisations, loss by material; guitar adds pick noise, a tension glide and sympathetic open strings.
- Bodies: stochastic modal IRs from 1/12-octave curves measured on University of Iowa MIS recordings; fixed modes below 300 Hz and no onset step, so the band spread between seeds is 1.2-1.5 dB. The harp curve is derived from the guitar's.
- Violin: digital waveguide bowed string with a friction table; bow position tracks the note. 212/216 single notes and 119/120 phrase notes hold Helmholtz motion.
- Choir: 3 singers per part; /aa/ pad on lifted sections, and (schema 2) the words of `sing: "choir"` lines, unison or block. In a song with choir lines the choir stem is not ducked (pad included).
- Mix: each track is brought to a target loudness after its EQ, so EQ cuts do not change a track's level; use gain. The accompaniment is ducked up to 5 dB under the singing lead (30/350 ms key) and the lead strip is +2 dB. `cargo run --release -p engine --example balance -- SONG.json [--style KEY]` reports each track against the lead where it sings.

## Measurement (the "ear")

Nobody on the machine side can listen. Judge every sound change by the gate, then send renders to the owner.

- `scripts/gate.sh [--strict] LABEL`: per-stem mean 1/3-octave LTAS over 8 seeds against `tests/soundgate/baseline-mean/` (3 dB in 100 Hz-10 kHz, 6 dB outside, or 2 seed-to-seed sd capped at 6/9 dB; gated RMS 1.5 dB; active fraction 15 points; mix peak 0.89), lead pitch by YIN, vowel distance (good 13.2-13.6 dB), Helmholtz (>= 208/216), thread invariance, and demo time and memory. `--strict` also compares seeds 1234 and 2718 one by one at 0.5/1 dB, for refactors that must not change the sound. Perf fails only above +50% of the perf baseline; speed and memory are aims, not requirements. The owner waived listen-before-rebaseline: re-capture the baseline in its own commit when a change is intended.
- The Python tools (PANNs tagger, Whisper word error; `tools/`, `requirements.txt`) are not installed on this machine.
- Latest: demo at seed 1234, 12 cores: about 2.1 s threaded (650 MB), 6.5 s on one thread (490 MB). Vowel distance 13.38 dB.

## The JS prototype

`src/` builds (`python3 build.py`) a claude.ai published page where Claude writes in the browser (`sample()`, model tier `complex`). It is a separate product and a source of ideas; the Rust engine does not follow it. Its constraints: scripts only from cdnjs, jsdelivr, Tailwind and jQuery; no Web Workers; downloads exclude .wav/.ogg (audio is Opus in .webm). `dist/` has the per-section melody contour fix but is not republished; publishing needs the owner's go-ahead.

## Open work

- Next batch, designed in `docs/features-2.md` with a wave plan in `docs/features-2-plan.json`: (1) record the model and token usage of every generation; (2) a settings file for the model, transport, effort and library; (3) per-track stems and a studio mixer; (4) duets written by Claude into the song JSON; (5) singer phrasing (legato to parlando) in the song JSON; (6) a full multipart score beside the lead sheet.
- Studio: clicks, Space and slider drags are untested (no xdotool here); the sheet cursor during rests and pickup placement disagree slightly with the lyrics view; bass voices use treble-8 clef; no multi-bar rests.
- Voice: alto and soprano intelligibility trails the baritone (next: vowel modification at high pitch); the first word of a phrase is the least reliable.
- Schema 2 features (choir words, melismas, key changes, rubato; `docs/schema-2.md`) are covered by tests and by the gate for songs without them; none has been heard. Send a v2 render for listening. Choir-line levels and timbre are unmeasured. The full-score lead staff shows a plain rest over a choir line. Duet staves draw no melisma extension or slur.
- Memory is above its aim (the sparse stem cache holds about 370 MB for the demo).
- `ClaudeApi` has never made a live call; the CLI path has two live songs.
- Harp: the aim is a harp of traditional folk music (a nylon or gut lever harp), not a concert harp. Its body curve is derived from the guitar's and has no reference recording; the owner judges the render fine and leaves it. `tools/harp_compare.py` compares it with isolated-note recordings if a lever-harp set turns up (the only open set found, VSCO 2 CE, is a concert harp and does not apply; see `docs/engine-design.md`).

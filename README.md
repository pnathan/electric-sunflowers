# Singer-Songwriter Bot

A browser page in which Claude writes a folk song (lyrics, chords, form, key, meter, tempo) and the page then composes the melody and synthesizes the whole performance from nothing: a formant singing voice, harmony and choir voices, plucked-string guitar, bass and harp, a physically modelled bowed violin, and brush drums, mixed with reverb. No samples are played; every sound is generated in the browser. The page also engraves the melody as sheet music and exports the recording.

Live: https://claude.ai/artifact/GkCGoQ5zSCg5MXsnkPidyj (a claude.ai published artifact).

## Build

    python3 build.py        # writes dist/singer-songwriter-bot.html

The page needs the claude.ai artifact runtime for two features: `sample` (Claude writes the song) and `downloads` (save audio and score). Without them the demo song still renders and plays. See CLAUDE.md for the standalone option.

## Layout

    src/engine.js     theory, melody composer, synthesis, mixing (also a Node module)
    src/styles.js     22 styles and 10 form templates
    src/prompt.js     songwriter prompt built from a style direction
    src/notation.js   melody engraving to SVG (Bravura glyphs in src/glyphs.js)
    src/export.js     Opus-in-WebM encoder and muxer; real-time fallback
    src/app.js        page logic; src/shell.html page shell and CSS; src/demo.js demo song
    tests/            Node render tests, synthesis harnesses, browser tests (Playwright)
    tools/            measurement: audio classifier, speech recognition, spectra, spectrograms, pitch
    build.py          concatenates src/ into dist/

## Tests

    node --max-old-space-size=4096 tests/mt.js        # render + mix the demo; timing and integrity
    node --max-old-space-size=4096 tests/formtest.js  # 12-bar blues form, no chorus
    node tests/sweep2.js                               # bowed-string stability across range and dynamics
    CHROME=/path/to/chrome python3 tests/pw8.py        # page: style, tempo, guitar, write path (mocked Claude)
    CHROME=/path/to/chrome python3 tests/pw7.py        # page: score view, export, decode check

Measurement tools need `pip install -r requirements.txt`, the PANNs checkpoint in `~/panns_data/`, and the reference recordings from `tools/fetch_refs.sh`.

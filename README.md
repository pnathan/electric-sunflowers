<p align="center"><img src="assets/logo.svg" width="128" alt="An electric sunflower"></p>

# electric-sunflowers

Claude writes a folk song; a deterministic engine sings and plays it.

Claude's part is the song itself: title, lyrics set as stressed syllables with ARPAbet pronunciation, chords per bar, key, mode, meter, tempo, form and band. From that JSON the engine composes the melody, arranges the band, synthesizes every sound from first principles and mixes the result. No samples are played. The same song and seed always give the same recording.

The singer is a formant voice on a Liljencrants-Fant glottal source. The band is plucked strings (extended Karplus-Strong with modal bodies measured from real instruments), a waveguide bowed violin, brush and kit drums, harmony and choir voices, and a feedback-delay-network reverb. Twenty-two styles, from Appalachian ballad to sea shanty, fix the form, meter, harmony and band.

## Quick start

    cargo build --release
    target/release/sunflower demo -o demo.ogg
    target/release/sunflower write "a barn dance for the first warm night in April"

`write` asks Claude for a song, saves Claude's JSON beside the audio, then renders it. By default it runs the `claude` CLI with your logged-in account (no tools, no project context); `--via api` uses the Anthropic API with `ANTHROPIC_API_KEY`. Output format follows the extension: `.ogg` (Vorbis, default), `.flac`, `.wav`.

    sunflower styles                              # the 22 styles
    sunflower render song.json --style shanty     # render a song you have
    sunflower sheet song.json --seed 1            # chord sheet, chords over lyrics
    sunflower write "..." --style bluegrass --voice alto --seed 7

Rendering uses every core; `RAYON_NUM_THREADS=1` renders on one with identical output. The demo song (3 min) renders in about 2 s on 12 cores.

The build needs no network: dependencies are vendored in `vendor/`. It links against the system ALSA and X11/GL libraries for the studio.

## Layout

    crates/song         typed song model; the loose-JSON boundary with a list of repairs; schema
    crates/songwriter   styles and forms, the songwriter prompt, Claude clients (CLI and API)
    crates/compose      form, timeline, text setting (rhythm), melody (Viterbi over pitch pairs)
    crates/arrange      the band's parts as note events
    crates/voice        singing voice: articulation, LF glottal source, formant tract
    crates/instruments  plucked string, guitar, modal bodies, bowed string, drums
    crates/dsp          filters, delays, FFT convolution, dynamics, reverb
    crates/engine       tracks, sparse stems, the render task graph, the mixer
    crates/export       Ogg Vorbis, FLAC, WAV
    crates/sunflower    the command-line tool
    crates/notation     sheet-music engraving to SVG (in progress)
    crates/studio       native desktop app: lyrics with chords, score, playback (in progress)
    crates/soundgate    measurement for the sound gate
    docs/               engine design; the rewrite plan
    src/, tests/, tools/  the original JavaScript prototype (a browser page) and its measurement tools

## Testing

    cargo test --release --workspace
    scripts/gate.sh LABEL        # the sound gate

Nobody on the machine side can listen, so changes to the sound are judged by the sound gate: per-track 1/3-octave spectra averaged over eight seeds against a baseline, lead pitch accuracy (YIN), vowel distinctness, bowed-string stability, render time and memory, and bit-identical output at any thread count. `docs/engine-design.md` names every algorithm and its source.

## The prototype

The engine began as a single-page JavaScript app published as a claude.ai artifact, where Claude writes the song in the browser. It lives in `src/` (build with `python3 build.py`) and is kept as a separate product; the Rust engine no longer follows it sample for sample.

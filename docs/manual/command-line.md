---
title: The command line
description: sunflower demo, render, write, sheet and styles, and the studio's flags.
---

[Manual](./) | Previous: [Songs and files](songs-and-files.md) | Next: [Styles](styles.md)

# The command line

`sunflower` runs the same engine as the studio, without a window. Every rendering command normalises the song (printing each repair as a warning), renders all parts on every core, mixes them and writes the audio in the format that the output's extension names. Beside the audio it writes the same sidecars as the studio: `<stem>.render.json` and `<stem>.sheet.json` ([Songs and files](songs-and-files.md#the-files-of-a-song)).

Progress, the seed and the paths written go to standard error. On failure the command prints `sunflower: error: ...` and exits with status 1.

## Shared options

Rendering options (`demo`, `render`, `write`; `sheet` takes these two only):

| Option | Meaning |
|---|---|
| `--seed N` | The seed. Left out, a random seed is chosen and printed. |
| `--voice V` | `auto` (the default: the song's own voice), `bass`, `baritone`, `tenor`, `alto`, `soprano`. |

Output options (`demo`, `render`, `write`):

| Option | Meaning |
|---|---|
| `-o`, `--out FILE` | The output. `.ogg` is Ogg Vorbis, `.flac` FLAC, `.wav` WAV; any other extension is an error. |
| `--quality Q` | Ogg Vorbis VBR quality, about -0.2 to 1.0; default 0.6, about 192 kb/s. |
| `--flac16` | FLAC at 16 bits instead of 24. |
| `--float` | WAV as 32-bit float instead of 16-bit PCM. |

All output is 44.1 kHz stereo.

## sunflower demo

    sunflower demo [--seed N] [--voice V] [-o FILE]

Renders the built-in demo, "Every Harbor". The output defaults to `song.ogg`. The demo has no file of its own, so the command first writes its JSON beside the output (`song.json` by default) for the sidecar to name. That write replaces any file of that name.

## sunflower render

    sunflower render SONG.json [--style KEY] [--no PART]... [--seed N] [--voice V] [-o FILE]

Renders a song JSON. The output defaults to `song.ogg` in the current directory.

- `--style KEY` imposes a style: its guitar pattern, band, drums and break instrument, and its tempo range (widened by 10%) for the song's meter. `sunflower styles` lists the keys; an unknown key is an error that lists them all.
- `--no PART` switches a band part off in the mix: `drums`, `bass`, `harmonyGuitar`, `harp`, `violin`, `choir`, `harmonies`, `doubles`. Give it once per part. The sidecar does not record switched-off parts.

To make a take in the studio's library, write the output there:

    sunflower render ~/Music/sunflower/electric-sunflowers.json --style irishpub --voice alto --seed 7 \
        -o ~/Music/sunflower/electric-sunflowers-alto.ogg

## sunflower write

    sunflower write "MOOD" [--style KEY|auto] [--via cli|api] [--model ID] [--seed N] [--voice V] [-o FILE]

Asks Claude for a song, saves the reply, and renders it, as the studio's New song form does ([Writing a song](writing.md)).

- `MOOD`: what the song is about, quoted into the prompt word for word.
- `--style`: a style key; left out or `auto`, the songwriter's choice.
- `--via`: `cli` (the default) runs the logged-in `claude` CLI; `api` calls the Messages API with `ANTHROPIC_API_KEY`.
- `--model`: a model id; the default is `claude-opus-5-5`.
- `--seed` sets both the creative direction and the render; `--voice` names the singer to Claude and to the render.

The output defaults to `<title-slug>.ogg` in the current directory. Claude's JSON is saved first, as `<stem>.json` beside the output, so a song that fails to normalise is kept. Unlike the studio, the command does not add `-2` to a name already taken: it replaces the older files. It prints the title, style, form, key, meter and tempo on standard output.

    sunflower write "a barn dance for the first warm night in April"
    sunflower write "the tide table on a diner wall" --style shanty --voice bass --seed 12

## sunflower sheet

    sunflower sheet SONG.json [--style KEY] [--seed N] [--voice V] [--json]

Prints the song's chord sheet, chords above the words, by section, with each section's start time, as the renderer would sing it with that seed, voice and style. `--json` prints the full song sheet instead, the same structure as `<stem>.sheet.json`. Without `--seed` a random seed is used; to match a recording, give the seed, voice and style from its render sidecar:

    $ sunflower sheet electric-sunflowers.json --seed 1 --style shanty
    Hum, You Sunflowers
    C dorian, 6/8, 66 bpm, baritone, Sea shanty
    ...
    [Chorus 1]  0:01
        Cm                               Bb
    the sun goes down and the wires come on,
    Cm       Gm          Cm
    hum, you sunflowers, hum!

Times here are rounded to the nearest second; the studio's clock truncates, so the same section may read one second apart.

## sunflower styles

    sunflower styles

Lists the 22 style keys with their names. [Styles](styles.md) gives each in full.

## Threads

Rendering uses every core through rayon. `RAYON_NUM_THREADS=1` renders on one thread; the output is bit-identical at any thread count.

## Studio flags

    studio [SONG.json] [--dir DIR] [--open NAME] [--view lyrics|sheet|both] [--volume V]
           [--new] [--seek SECS] [--play SECS] [--screenshot PNG]

| Flag | Meaning |
|---|---|
| `SONG.json` | Open this song at start; added to the list when outside the library. |
| `--dir DIR` | The library folder; default `~/Music/sunflower`. |
| `--open NAME` | Open the library entry with this listed name or stem. |
| `--view V` | The first view: `lyrics`, `sheet` or `both` (default). |
| `--volume V` | Starting volume, 0 to 1; default 0.8. |
| `--new` | Open the New song form at start. |
| `--play SECS` | Once the song is ready (loaded, laid out, rendered if needed, audio open), play it for SECS seconds, logging the position to standard error. |
| `--seek SECS` | With `--play` or `--screenshot`: seek there first. Alone it does nothing. |
| `--screenshot PNG` | After the steps above, save the window as a PNG and quit. It needs a song to open (`SONG.json` or `--open`); the scripted run gives up after 900 s. |
| `-h`, `--help` | Print the usage line. |

The screenshots in this manual were taken this way, for example:

    studio --dir /tmp/manual-lib --open "Demo (built-in)" --volume 0 --seek 47 --play 1.5 --screenshot both.png

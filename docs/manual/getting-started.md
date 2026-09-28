---
title: Getting started
description: First launch, the library folder, the built-in demo.
---

[Manual](./) | Previous: [Install and build](install.md) | Next: [Writing a song](writing.md)

# Getting started

## First launch

    studio

The window is titled Sunflower Studio and opens at 1440 by 900. It has four parts:

- **Songs**, on the left: the library. Drag its right edge to resize it.
- **The top bar**: the New song button, the view selector (Lyrics & chords, Sheet music, Both), the sheet zoom and the Follow check box.
- **The centre**: the open song, drawn by the chosen view.
- **The transport**, at the bottom: job progress, errors, Play, the clock, the position slider and the volume.

Nothing is open at first. The centre reads "Pick a song on the left, or write a new one."

## The library folder

The library is a plain folder, `~/Music/sunflower` by default. The studio creates it when it is missing and shows its path under the Songs heading. Every song in it is a set of files that share a stem, for example `every-harbor.json`, `every-harbor.ogg`, `every-harbor.render.json`, `every-harbor.sheet.json`. [Songs and files](songs-and-files.md) describes each.

Use another folder with `--dir`:

    studio --dir ~/songs/2026

Files that other programs put in the folder, such as a `sunflower render` output, appear after you press **Rescan**.

## The demo

The first entry, **Demo (built-in)**, is "Every Harbor", a song compiled into the program. Click it.

1. The status line reads "Composing the melody" while the studio sets the text to rhythm, composes the melody and lays out the sheet.
2. The demo has no audio yet, so the studio renders it at once. A progress bar steps through Composing, Rendering (tracks done of total), Mixing, Encoding Ogg Vorbis and Writing sidecars, with the elapsed seconds. The render uses every core; on a 12-core machine the demo takes about 5 s.
3. The render writes `sunflower-demo.json`, `.ogg`, `.render.json` and `.sheet.json` into the library. The next launch plays it without rendering.

Press **Play**, or the space bar. The lit line and syllable follow the voice.

![The demo in the Both view: lyrics with chords on the left, sheet music on the right, the current line and note lit](images/both.png)

## Opening a song directly

    studio path/to/song.json

opens that song at start. A file outside the library is added to the list for this session; its audio and sidecars are written beside the JSON, not into the library.

    studio --open every-harbor

opens a library entry by its listed name or its stem. `--view lyrics`, `--view sheet` or `--view both` (the default) picks the first view, and `--volume 0.5` the starting volume (0 to 1; default 0.8). [The command line](command-line.md#studio-flags) lists every flag.

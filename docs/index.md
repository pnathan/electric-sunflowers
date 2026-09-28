---
title: electric-sunflowers
description: Claude writes a folk song; a deterministic engine sings and plays it.
---

<p align="center"><img src="assets/logo.svg" width="160" alt="An electric sunflower"></p>

Claude writes the song: title, liner note, lyrics set as stressed syllables with ARPAbet pronunciation, chords per bar, key, mode, meter, tempo, voice, form and band, all as one JSON object. From that point no model is consulted. A deterministic Rust engine composes the melody, arranges the band, synthesizes every sound from physical and signal models (a formant voice on a Liljencrants-Fant glottal source, Karplus-Strong strings on measured modal bodies, a waveguide violin, synthetic drums) and mixes the result. No samples are played. The same song and seed always give the same recording.

Sunflower Studio is the desktop front end: a native Linux window with a song library, a lyrics-with-chords view and a sheet-music view that follow playback, an Ogg Vorbis transport, and a form that asks Claude for a new song and renders it. `sunflower` is the same engine on the command line. Twenty-two styles, from Appalachian ballad to sea shanty, fix each song's form, meter, harmony and band.

## Documentation

- [User manual](manual/) for the studio and the command line. It describes the current release.
- [README](https://github.com/pnathan/electric-sunflowers/blob/trunk/README.md): quick start and repository layout.
- [Engine design](https://github.com/pnathan/electric-sunflowers/blob/trunk/docs/engine-design.md): every algorithm and its source.
- [Repository](https://github.com/pnathan/electric-sunflowers).

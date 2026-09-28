---
title: Credits
description: Fonts, measurements, libraries, and the sources of the algorithms.
---

[Manual](./) | Previous: [Troubleshooting](troubleshooting.md)

# Credits

## Songs

Claude, by Anthropic, writes every song: words, pronunciation, chords, key, mode, meter, tempo, voice, form and band. The engine composes the melody and the band's parts from that, and performs them.

## Music font

The clefs, noteheads, rests, accidentals and other music symbols of the sheet view are outlines from **Bravura**, the reference font of the Standard Music Font Layout (SMuFL), by Steinberg Media Technologies, used under the SIL Open Font License 1.1. The outlines are compiled into the program (`crates/notation/src/glyphs.rs`).

## Measured instrument bodies

The resonances of the guitar, harp and violin bodies follow magnitude curves measured from the **University of Iowa Musical Instrument Samples** (Electronic Music Studios, University of Iowa), extracted by `tools/extract_curves.py`. The harp curve is derived from the guitar's; no harp recording was used.

## Algorithms

Each model and its source is recorded in [docs/engine-design.md](https://github.com/pnathan/electric-sunflowers/blob/trunk/docs/engine-design.md). In brief:

- **Voice source**: the Liljencrants-Fant glottal flow model (Fant, Liljencrants and Lin 1985; Fant 1995), played from mip-mapped wavetables (Massie 1998).
- **Vocal tract**: cascade formant synthesis (Klatt 1980; Klatt and Klatt 1990).
- **Articulation**: synthesis by rule (Holmes, Mattingly and Shearme 1964; Klatt 1987), consonant-vowel transitions from locus equations (Delattre, Liberman and Cooper 1955), vowel targets from Hillenbrand et al. (1995).
- **Plucked strings**: extended Karplus-Strong (Karplus and Strong 1983; Jaffe and Smith 1983), loop filter design after Valimaki, Huopaniemi, Karjalainen and Janosy (1996), Thiran allpass tuning (Thiran 1971; Laakso et al. 1996).
- **Instrument bodies**: stochastic modal impulse responses in the commuted-synthesis lineage (Smith 1993; Karjalainen and Valimaki 1993).
- **Bowed string**: digital waveguide (McIntyre, Schumacher and Woodhouse 1983; Smith 1986) with the STK bow table (Cook and Scavone).
- **Drums**: band-limited oscillators by PolyBLEP (Valimaki and Huovilainen 2007).
- **Convolution**: overlap-add (Stockham 1966) over real FFTs (Sorensen, Jones, Heideman and Burrus 1987).
- **Dynamics**: soft-knee compression (Giannoulis, Massberg and Reiss 2012).
- **Reverb**: feedback delay network (Jot and Chaigne 1991) with Schroeder allpasses (Schroeder 1962).
- **Composition**: text setting and melody by Viterbi search with perturb-and-MAP sampling (Papandreou and Yuille 2011); leap recovery by gap fill (Meyer 1956).

## Libraries

The studio is built on egui and eframe (window and widgets), rodio and cpal (playback), and resvg (drawing the sheet). The engine uses rustfft and realfft, and rayon for parallel rendering. Audio is encoded by vorbis_rs (the aoTuV Vorbis encoder) and flacenc. The command line uses clap.

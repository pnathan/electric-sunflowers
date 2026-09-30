# Engine design (Rust rewrite)

Status: implemented on branch `rust-cleanup` (base commit 29c5c08; waves 0-5 of section 13). Deviations from the text as first written:

- FFT: `dsp::fft` wraps `realfft`/`rustfft` (section 5.10) instead of a hand-written Stockham transform.
- Memory: peak RSS for the demo is above the section 10 aims (measured figures in section 10). Speed meets them.
- The Helmholtz analysis (Goertzel magnitudes, the note and phrase sweeps) lives in `crates/instruments/tests/helmholtz/mod.rs`, shared by the test and the example, so the library does not depend on rayon.
- Seeds are `u64` end to end: CLI, `engine::render`, `compose::prepare`, the stems example.

Scope: everything after Claude's JSON reply: validation, composition, arrangement, synthesis, mixing, export, and the CLI.
The JS prototype (`src/engine.js`) stays in the repo as a separate product and a source of ideas. No Rust code refers to it.

## 1. Goals and non-goals

Goals

1. Same instruments, same musical behaviour, same character. Detail may change; every change is measured by the sound gate (section 12).
2. Code that names its algorithms. Each sound model is a small typed unit with a doc comment that gives the algorithm, the source, and the parameters that set the sound.
3. One code path. No sequential/threaded twins, no feature-gated capture paths, no parity shims. `RAYON_NUM_THREADS=1` is the sequential path, and the output is bit-identical at any thread count.
4. Speed and memory are aims, not gates: roughly 2.5 s threaded and 9 s on one thread for the demo, and less memory than today (at design time: 6.3 s, 16.2 s, 0.89 GB, 0.72 GB). The gate fails only on a gross regression (+50%). Clean code comes before the last seconds.
5. Typed data end to end. After the JSON boundary there are no strings in the render path except lyric text and titles, and no panics on user input.

Non-goals

- Sample parity with the JS or with the current Rust output. Every random draw moves (new generator, per-event streams); each song becomes a different take from the same distribution.
- Real-time operation. The renderer is offline. Real-time idioms (denormal hygiene, smoothing, block processing, no allocation in inner loops) are used where they make offline code faster or cleaner, not to meet a deadline.
- New instruments, key changes, melismas, rubato. Out of scope (CLAUDE.md open issue 6).
- New crates are allowed: the workspace is vendored (vendor/). The FFT uses `realfft`/`rustfft`.

## 2. Crate layout

```
sfcore       math, fp (denormals), random (Rng, stream tags), constants
song         typed song model, loose-JSON boundary (wire + Repair), chords, pitch classes, Phoneme, G2P, JSON schema,
             note-event types (events.rs) shared by arrange, instruments, voice and engine
dsp          generic primitives: biquad, onepole, resonator, delay, allpass, smoother, stochastic processes,
             fft, convolution, dynamics, pan, FDN reverb
instruments  sound models: plucked string, guitar sympathetic strings, body IR, bowed string, drum voices
voice        singing voice: parameters per voice type, phoneme acoustics, articulation, LF source, formant tract
compose      form, timeline, rhythm (text setting), pitch (melody Viterbi), melody profile, prepare
arrange      pure event planners: guitar, bass, harp, drums, choir, lines, violin, harmony guitar, vocal singers
engine       tracks and strips, stems, render task graph, mixer
export       Ogg Vorbis, FLAC, WAV writers
songwriter   styles and forms, prompt, schema use, Claude clients
sunflower    CLI
soundgate    measurement tool (analysis only; depends on no engine crate)
```

Dependency graph (arrows point at dependencies):

```
sunflower -> engine, songwriter, export, song, sfcore, rayon
engine    -> arrange, compose, voice, instruments, dsp, song, sfcore
arrange   -> compose, song, sfcore            (no dsp: arrangement produces events, not audio)
voice     -> dsp, song, sfcore
instruments -> dsp, song, sfcore
compose   -> song, sfcore
songwriter -> song, sfcore, ureq, tempfile
dsp       -> sfcore, rayon, realfft, rustfft
export    -> sfcore, vorbis_rs, flacenc, rayon
soundgate -> hound, serde_json                (own FFT; the instrument must not change with the thing measured)
```

Changes against today, and why:

- New `song` crate. Compose, songwriter, arrange and engine all need the song vocabulary (Mode, Meter, SectionKind, DrumKit, Voice, Band). Today it exists as strings in seven places and as two `Band` structs bridged by `sunflower/style_glue.rs`. Songwriter should not depend on composition algorithms, so the model gets its own crate. `style_glue.rs` is deleted.
- New `instruments` crate. `dsp` today mixes generic primitives with product sound models (pluck, violin, body) and the product track table (`mix.rs`). Primitives stay in `dsp`; sound models move to `instruments`; the track table and mixer move to `engine`.
- `arrange` loses its `dsp` dependency. `gen_guitar`, `gen_bass`, `gen_harp`, `gen_drums` interleave planning and synthesis on one random stream, so nothing can be parallelised or tested without audio. Arrange returns events; engine renders them with instruments. The violin, harmony-guitar and vocal-singer plans move from `engine/band.rs` and `engine/vocals.rs` into `arrange`.
- Phonetic acoustics (formant targets, consonant table, loci) move from `compose::phonetics` to `voice::phoneme`. The `Phoneme` enum itself and G2P live in `song`, because the JSON boundary parses ARPAbet.
- `VoiceParams` (Rd, tilt, jitter, formant scales) move from `compose::voices` to `voice::params`. Compose keeps only `Voice` and its range, via `song`.
- `soundgate` is new (section 12).
- `voice` stays separate from `instruments`: it has its own control layer (articulation) and is the largest model.

## 3. Core abstractions

### 3.1 Sample type, rate, blocks

- Audio buffers: `f32`. Coefficients and recursive state: `f64` where precision matters (section 6).
- `sfcore::SR = 44_100` stays the rendering rate. Every coefficient constructor takes `fs: f64`, so a model may run oversampled internally later.
- No global processing block. Each model has its own control rate, named in its module: voice `HOP = 64` (689 Hz frames), violin `CONTROL = 16`, compressor gain update 16. Within a control period, parameters ramp per sample (3.4).
- Stems use `STEM_BLOCK = 4096` frames. The mixer uses `MIX_BLOCK = 1024` frames.

### 3.2 Processors

A processor is a struct with its state and a `tick`:

```rust
pub struct Biquad { c: BiquadCoeffs, s1: f64, s2: f64 }
impl Biquad {
    pub fn tick(&mut self, x: f64) -> f64;
    pub fn process(&mut self, buf: &mut [f32]);
    pub fn reset(&mut self);
    pub fn flush_denormals(&mut self);       // zero state below 1e-25
}
```

Every processor exposes `tick`; the ones used over buffers also expose `process(&mut [f32])`. There is no dynamic-dispatch processor trait: chains are concrete (`Cascade<const N: usize>`, a fixed tract struct) so the compiler inlines them. A small trait `Process { fn process(&mut self, buf: &mut [f32]); }` exists only for the channel strip, whose EQ list is data.

### 3.3 Notes, instruments, voices

- A note event is plain data in seconds and fractional MIDI. The types live in `song::events` so that the producer (`arrange`) and the consumers (`instruments`, `voice`, `engine`) share them without depending on each other:
  `PluckNote { t0, t1, midi: f32, vel: f32 }`, `StringNote { t, stop, string: u8, midi: u8, vel }` (guitar, one list per string), `BowNote { t0, t1, midi, vel, vibrato: bool }`, `DrumHit { t, kind: DrumKind, vel, pan }`, `VocalNote { t0, t1, midi, phones, amp, stress, phrase_start, phrase_end, grace }`, `Singer { voice: Voice, style: SingStyle, notes: Vec<VocalNote>, pan, offset }`.
- An instrument renders events into a buffer: `fn render(&self, notes: &[Note], seed: u64, out: &mut [f32])` (mono, dense slice over the song) or a stereo pair. `engine` binds each track's events to an instrument and a preset and moves the result into a stem. Instruments own every synthesis choice (pick position, brightness, release); arrange never sets synthesis parameters.
- Parameter presets are `const` values of plain structs with `Default` (for example `PluckParams::GUITAR`, `BASS`, `HARP`, `HG_LEAD`, `HG_ARP`). No `Option` fields that mean "use the default"; no zero sentinels.
- A voice is `voice::VoiceSynth`: a `GlottalSource`, a `Tract`, a noise generator. It renders one phrase at a time from `ControlTracks` into a caller slice. `voice::render_phrases(notes, voice, settings, seed, len, emit)` splits the notes into phrases (no gap of 0.3 s or more), builds control tracks for each phrase's frame window only (0.7 s lead, 0.5 s tail, clipped so windows never overlap), renders it into a scratch buffer and calls `emit(start, samples)`; the engine adds that to the stem with pan gains.

### 3.4 Parameter smoothing

- `dsp::smoother::Ramp { value, step, left }`: linear ramp to a target over n samples. Used for control-rate to audio-rate interpolation.
- Voice tract: resonator coefficients are designed once per hop and ramped linearly per sample. For a two-pole resonator `y = a x + b y1 + c y2` the stable region in the (b, c) plane is a triangle, which is convex, so the linear path between two stable designs is stable. Klatt's unity-DC `a = 1 - b - c` is linear in (b, c) and ramps with them.
- Violin: bow velocity, friction slope, and both delay lengths ramp per sample across each 16-sample control period.
- Compressor: gain in dB computed every 16 samples, linearly interpolated in linear gain per sample.
- One-pole smoothing of a control track uses `OnePole::from_tau`. The zero-phase forward-backward smoother (`filtfilt` on a frame track) is `dsp::onepole::zero_phase_smooth` (one track) or `zero_phase_smooth_lanes` (N equal-length tracks in one interleaved loop; unequal lengths are an error) and is used only on precomputed control tracks.

### 3.5 Buffers and ownership

- `engine::stem::SparseBuf { len: usize, blocks: Vec<Option<Box<[f32; STEM_BLOCK]>>> }`. A block is allocated on first write. Reads of an absent block are zeros.
- `Stem = Mono(SparseBuf) | Stereo([SparseBuf; 2])`. Mono stems: lead, harmony, bass, and every bodied instrument before convolution. Stereo: doubles, choir, drums, and the bodied tracks after convolution.
- `SparseBuf::add_at(start: isize, src: &[f32], gain: f32)` clips negative starts; there is no negative-index emulation.
- Scratch buffers (per phrase, per note) are owned by the task and reused through a `Scratch` struct passed by `&mut`. Inner loops never allocate.
- Filters over a `SparseBuf` process present blocks; after a present block, following absent blocks are allocated and processed while the filter state holds energy above 1e-12, then the state is reset and absent blocks are skipped.
- Measured occupancy (4096-frame blocks with signal above 1e-5) on the demo: doubles 30%, harmony 45%, choir 46%, hg 58%, drums 60%, violin 75%, lead 82%, bass 82%, harp 83%, guitar 100%.

### 3.6 Tracks, strips, song

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TrackId { Lead, Doubles, Harmony, Choir, Guitar, HarmonyGuitar, Bass, Drums, Harp, Violin }

pub struct Strip {
    pub label: &'static str,
    pub gain: f32, pub pan: f32, pub send: f32,
    pub band: Option<BandPart>,          // None: always on
    pub eq: &'static [EqBand],           // EqBand { kind: EqKind, f: f64, q: f64, db: f64 }
    pub body: Option<BodyMount>,         // BodyMount { body: Body, seed_offset: u32, trim: f32 }
    pub comp: Option<CompSpec>,          // lead, harmony
    pub slapback: Option<Slapback>,      // lead
}
pub const STRIPS: [Strip; 10];            // indexed by TrackId as usize; values in section 5.8
```

- `ProcessedStem { audio: Stem, level: f32 }` is the channel-strip output: EQ and compression applied, `level = 0.1 / gated_rms` applied at pan time (folded into pan gain; the compressor threshold is shifted by `-20 log10(level)` so compressing before scaling equals the current order exactly).
- `Stems { len, tracks: [Option<ProcessedStem>; 10], slapback: Option<SparseBuf> }` is the cache that makes band toggles a re-sum only.
- A song after the boundary is `song::Song` (section 7). `compose::prepare` returns `Prepared { form, timeline, melody, key_shift, voice }`.

### 3.7 Threading

- rayon only. `sfcore::fp::init_pool(threads: Option<usize>)` builds the global pool with a `start_handler` that enables flush-to-zero and denormals-are-zero on each worker. `engine::render` calls `sfcore::fp::flush_denormals()` on the calling thread.
- Render is one `rayon::scope`. Tasks:
  - voice: lead; harmony; doubles take 1; doubles take 2; choir part S, A, T, B (each renders its 3 singers in order into a part stem);
  - band: guitar; harmony guitar; bass; drums; harp; violin.
  Each task runs instrument render, then body convolution (if any), then the channel strip, then stores its `ProcessedStem`. The choir stem is the sum of the four part stems in part order, done when the last part finishes.
- Body convolution parallelises inside its task over contiguous block ranges (3.9).
- Determinism: every random stream is derived from (seed, tag, index) (3.8), never shared between tasks; every sum has a fixed order (singers in plan order, parts S A T B, tracks in `TrackId` order, convolution ranges in time order). Output is bit-identical at any thread count, which a test asserts.
- Progress: `trait Progress: Sync { fn advance(&self, done: usize, total: usize); }` with an `AtomicUsize` count of finished tasks.

### 3.8 Random numbers

- One generator: `sfcore::random::Rng`, xoshiro128++ (Blackman and Vigna 2018), seeded through SplitMix64 (Steele, Lea, Flood 2014).
- Streams: `Rng::stream(seed: u64, tag: Tag)` and `Rng::event(seed, tag, index: u64)`. `Tag` is a `u64` built at compile time by `const fn tag(name: &str) -> Tag` (FNV-1a over bytes). Each crate declares its tags as consts next to their use (`const GUITAR_STRUM: Tag = tag("guitar.strum");`). No central enum, no `format!` tags, no UTF-16.
- Methods: `next_u32`, `uniform() -> f64 in [0,1)`, `bipolar() -> f64 in [-1,1)`, `gauss()` (Marsaglia polar method, second deviate cached), `fill_bipolar(&mut [f32])`, `pick<T>(&[T])`.
- Per-event streams (guitar stroke k, choir singer (part, i), drum hit k) make each event independent of render order and of every other event's draw count.

### 3.9 Memory model

Whole-song stems, block-sparse, are kept; the mix is streamed.

- Per-track loudness normalisation, the bus-compressor threshold and peak normalisation need whole-song statistics, and band toggles re-sum cached stems. Note-at-a-time rendering into stems is simple and parallel. Streaming the render itself is not worth its complexity for an offline renderer.
- Streamed: the mixer (1024-frame blocks produce final L/R only; no full-length main or send buses), voice phrases (scratch), drum hits (render straight into the stem), convolution (per-range buffers).
- Estimate for the demo: 11.1 channel-equivalents of occupied stem blocks x 33 MB = 0.37 GB, plus 66 MB of output, less transient buffers. Target 0.50 GB threaded, 0.40 GB single thread.

## 4. Numeric policy

- Buffers: `f32`. Final mix: `f32`.
- Biquads, Klatt resonators, one-poles with poles near 1 (low cutoffs, long time constants): `f64` coefficients and state. A 30 Hz high-pass at 44.1 kHz has poles within 4e-3 of the unit circle; `f32` coefficient quantisation there moves the corner audibly and raises the noise floor.
- Oscillator phases and phase increments: `f64` (a song is 8M samples; `f32` phase drifts in cents).
- Waveguide and FDN delay lines: `f32`. Loop filters in them: `f64` state.
- FFT: `f32` interleaved complex data, twiddles computed in `f64` and stored as `f32`. Expected round-trip error for n = 65536 is about 1e-6 relative (noise floor near -120 dB), below audibility and below the 24-bit export floor; a unit test asserts it.
- Sums of squares (RMS, LTAS): `f64` per block.
- No JS semantics: `f64::round` for rounding, `f64::max/min`, `exp2` for `mtof`. One seconds-to-sample rule: `sfcore::time::sample_at(t) = (t * SR).round() as isize`.
- Denormals: flush-to-zero and denormals-are-zero on every render thread (x86_64 MXCSR bits 15 and 6, `0x8040`; aarch64 FPCR.FZ), set by `sfcore::fp`. Measured: after exact-zero input, the current `f64` Direct Form I biquad state settles near 1.4e-321 and cycles there forever at 70.7 ns/sample against 4.1 ns with FTZ/DAZ; the demo mix EQ drops from 3.50 s to 1.74 s. As a second guard (for targets without FTZ, e.g. a future WASM build), every recursive filter has `flush_denormals()` which zeroes state below 1e-25, called at stem-block and phrase boundaries.
- NaN: no `partial_cmp().unwrap()` on data; sorts use `total_cmp`. Engine asserts finite output in debug builds and in tests.

## 5. Sound models

Each model: algorithm, source, parameters that set the sound. Values are the current shipped values unless the change list (section 11) says otherwise.

### 5.1 Voice source (`voice::glottal`)

- Algorithm: Liljencrants-Fant glottal flow derivative, parameterised by Rd (Fant, Liljencrants, Lin 1985; Fant 1995 for Ra, Rk, Rg from Rd). Epsilon by fixed-point iteration, alpha by bisection for zero net flow. Tables `D` (derivative, RMS-normalised) and `G` (normalised flow).
- Band limiting: `D` is stored as a mip set of 2048-point single-cycle tables, one per half-octave of f0 from 55 Hz to 1760 Hz. Each level has harmonics above SR/2 / f0_max(level) removed by FFT. Playback reads the two levels that bracket f0 and blends them linearly by position within the half-octave (no clicks when vibrato crosses a level). `G` stays a single table (it only modulates breath noise). Source: the mip-mapped wavetable method, Massie 1998 ("Wavetable sampling synthesis", in Kahrs and Brandenburg, Applications of DSP to Audio and Acoustics).
- Tables are built lazily per Rd step: `static LF: [OnceLock<LfTable>; N_RD]`, index `round(Rd * 40)`.
- Loudness blend: lax and tense tables mixed by `w = clamp((av - 0.42) / 0.6, 0, 1)`.
- Per-period jitter and shimmer; two cascaded one-pole spectral tilt filters at `tilt * 1.25 * tilt_scale` Hz.
- Parameters: per voice type Rd, tilt, jitter, shimmer (voice::params), `rd_scale`, `tilt_scale`.

### 5.2 Voice tract (`voice::tract`)

- Algorithm: cascade formant synthesiser (Klatt 1980; Klatt and Klatt 1990). Resonators are Klatt's unity-DC two-pole sections.
- Five formants F1-F5 plus up to four fixed high resonances at 5.5, 6.6, 7.7, 8.8 kHz times the voice formant scale, in series. F4, F5 and the high resonances are constant per voice and are designed once, not per hop.
- High shelf +16 dB at 5.2 kHz (RBJ shelf) after the cascade.
- Frication: white noise through an RBJ band-pass at the consonant's centre and bandwidth, added after the tract, gain 2.2.
- No voice bar. Only the deleted alternative stop model (closure murmur) drove it; the kept stop model voices a closure through the tract at 0.1.
- DC blocker, pole 0.995.
- Aspiration (white noise times `ah * 0.8`) and breath noise (white noise low-passed at 2.6 kHz by a one-pole, modulated by glottal flow) enter the cascade with the source. CLAUDE.md says aspiration is also low-passed; the code does not. The rewrite keeps the code's behaviour and fixes CLAUDE.md; low-passing aspiration is a later, ear-gated change (section 11).
- F1 is floored at 1.06 f0.
- Coefficient update per hop (64 samples) with per-sample ramps (3.4). Silent frames skip synthesis and reset resonator state.
- Rejected, recorded: a parallel high-frequency branch filled the spectral valleys (/uw/ from -55..-82 dB to -41 dB) and cut vowel distinctness from 13.6 to 10.6 dB.

### 5.3 Articulation (`voice::articulation`, `voice::controls`)

- Algorithm: synthesis by rule with targets and transitions (Holmes, Mattingly, Shearme 1964; Klatt 1987). CV formant transitions over 50 ms from the consonant locus (locus equations, Delattre, Liberman, Cooper 1955). F1 damping during aspiration.
- Pipeline: `plan_segments(notes, params) -> Vec<(Span, Segment)>`, `rasterise(&segments, frames) -> ControlTracks`, `shape_dynamics` (per-note swell, phrase-end fade), `pitch_track` (scoop, grace, one-sided glide), `Vibrato::add` (delayed smoothstep onset, rate wobble), pitch drift (a leaky second-order random walk, `dsp::stochastic::RandomWalk`).
- Per-phrase windows: `controls::Articulation::phrase` builds the tracks for one phrase's frame window, not the song. It plans the phrase's notes and every earlier note that still writes frames at or after the window's start (at least the note before the phrase, whose pause and breath lead in), so a window clipped to start inside the previous phrase keeps that phrase's sounding notes. Where the next phrase's 0.7 s lead would reach back before the previous phrase's end, `synth::phrase_spans` moves the boundary to the middle of the pause before the next onset, so the zero-phase smoothers and the pitch glide restart on flat tracks, not mid-note. The vibrato phase and the drift walk run on from one window to the next, over rendered frames only; frames between windows advance neither.
- Rules kept: flapping of unstressed intervocalic /t d/; onset compression to 45% of the inter-onset interval; coda compression; diphthong glides; a breath segment before phrases.
- Stops: closure with 0.1 voicing, 12 ms burst at gain 0.8, 24 ms aspiration for voiceless onsets. The alternative stop model (closure murmur, soft bursts) measured worse and is deleted, not switched off.
- /ey/ targets ey0 [450, 2020, 2600] and ey1 [340, 2210, 2780] (Hillenbrand et al. 1995).
- Consonant duration scale per voice: bass 1.15, baritone 1.2, tenor 1.25, alto 1.35, soprano 1.4. Voiced "th" af 0.05.
- Types: `Phoneme` (`#[repr(u8)]`, from `song`), `ConsClass { Sonorant, Nasal, Fricative, Aspirate, Stop, Affricate }`, `Locus { At([f64; 3]), Velar }`, `voiced: bool`. Tables are `const` arrays indexed by `Phoneme as usize`.

### 5.4 Plucked string (`instruments::pluck`)

- Algorithm: extended Karplus-Strong (Karplus and Strong 1983; Jaffe and Smith 1983). The loop runs in the velocity domain (a displacement loop differentiated at the output produced a spike every period).
- Loss: one-pole loop filter; its pole is reduced until |H(f0)| >= sqrt(rho), and loop gain is set so the fundamental decays to T60 (loop-filter design after Valimaki, Huopaniemi, Karjalainen, Janosy 1996).
- Tuning: exact one-pole phase delay subtracted; first-order Thiran allpass for the fractional delay, delta in [0.5, 1.5) (Thiran 1971; Laakso et al. 1996).
- Two polarisations: +1.4 and -0.35 cents, the second at amplitude 0.42 and 0.62 T60, summed without coupling.
- Excitation: triangular pick shape with apex at the pick position, circular moving average for finger width, differentiated, circular one-pole for pick release, DC removed, loaded by one warm-up pass through the loop.
- Options: high-passed pick noise; tension pitch glide (allpass coefficient ramped per sample, glide 5 cents); release damping from `release_start`; early stop when the per-period peak falls 74 dB; linear attack ramp; 64-sample end fade.
- High-note level trim: gain times `min(1, L/60)` where L is the loop length in samples (attenuates notes above 735 Hz). Kept and documented.
- Phases are explicit loops (warm-up, attack, sustain, release, fade), so inner loops have no per-sample branches.
- Presets: guitar damping 0.18, harp 0.16, bass 0.5 (bass brightness 0.12); harp T60 `6 * (98 / f)^0.5` s; per-note amp, T60 and pick position come from the event.

### 5.5 Guitar (`instruments::guitar`)

- Six monophonic strings: the next pluck or a changed note on a string stops the previous one (with damping).
- Sympathetic strings: six open-string Karplus-Strong loops (E2 A2 D3 G3 B3 E4) excited by 1.2% of the guitar output. One pass over the output with six ring buffers, branch wrap instead of `%`; blocks where the input is silent and all loops are below -120 dB are skipped.
- Parameters: `GuitarTuning` consts (damping 0.18, attack 1.0, glide 5 cents, sympathetic 1.0).

### 5.6 Bodies (`instruments::body`)

- Algorithm: stochastic modal synthesis of a stereo body impulse response from a measured magnitude curve (commuted synthesis lineage: Smith 1993; Karjalainen and Valimaki 1993). Curves: 1/12-octave `BODY_CURVES` measured from University of Iowa MIS recordings by `tools/extract_curves.py`; guitar with a steel-string correction; harp derived from guitar (no harp reference).
- Modes spaced max(3 Hz, 1.1% f) with +-45% jitter up to 13 kHz; Q ramps q_lo..q_hi over 200 Hz-12.8 kHz; amplitude sqrt(T df / tau) so energy density follows the curve; Gaussian amplitude and random phase. Each mode is a second-order recursive oscillator `y = 2 r cos(w) y1 - r^2 y2`, cut at 9.5 tau (82 dB). Accumulation in `f64`. Unit energy per channel; 10% linear tail fade.
- Fixed low modes (wave 3): below `F_CROSS` = 300 Hz the modes come from the body's own stream, the same for every seed; the right channel takes the left's amplitudes at phase + pi/2 (near a Hilbert pair: equal magnitude, uncorrelated). 300 Hz because the 3 Hz spacing floor holds to 273 Hz, so lower 1/3 octaves hold only 6-20 modes. Above 300 Hz each channel draws its own amplitude and phase from the seed (decorrelated stereo).
- Stable band levels (wave 3): overlapping random modes interfere, so a free draw scattered about 3 dB per 1/3 octave at every frequency, from seed to seed. Modes are summed in 1/12-octave groups (the curve's resolution, anchored at 300 Hz), merged upward until a group holds at least 3 modes (wave 4: a 1-mode group has 2 real unknowns against 2 constraints, so the step and DC removal left rounding residue that the energy scaling raised by about 1e16; this hit the lowest guitar and harp modes and the partial group under 13 kHz); each group's complex amplitudes are moved by the least weighted change that removes its onset step and its DC, then each channel of the group is scaled to its expected energy. Without the step and DC removal each group's random onset leaks a 1/f skirt above it and a shelf below it. Std over 32 seeds, largest 1/3-octave band from 80 Hz: guitar 5.3 -> 1.2 dB, harp 5.9 -> 1.5, violin 8.0 -> 1.4 (from 160 Hz; it has no modes below 180 Hz). Mean level changes, from the removed skirts: guitar -13 dB at 80 Hz and -6 dB at 100 Hz; harp -10 and -5 dB there and -4 to -7.5 dB from 3 to 13 kHz; violin -3 to -12 dB from 8 to 13 kHz; elsewhere within 2 dB. The IR now follows its curve within 0.8 dB per 1/3 octave above 300 Hz. `crates/instruments/examples/bodyspread.rs` prints the table.
- Curve extrapolation beyond the measured range: 12 dB/octave both ends.
- Level trim per body (today's `refPeak` divisors, empirical): guitar 5.4, harp 7.3, violin 2.4. Lives in `Body::spec()` with a doc line.
- API: `Body::{Guitar, Harp, Violin}`, `Body::impulse_response(self, rng) -> StereoIr` (spectra precomputed, 5.10). Do not shorten the IR: the 0.32-0.40 s modal tail is part of the body sound.

### 5.7 Bowed string (`instruments::violin`)

- Algorithm: digital waveguide bowed string (McIntyre, Schumacher, Woodhouse 1983; Smith 1986), friction by the STK bow table (Cook and Scavone, STK `BowTable`): reflection `(|dv * slope + 0.001| + 0.75)^-4` clipped to 1, slope = 5 - 4 * pressure.
- Two delay lines split at the bow point; nut reflection -1; bridge reflection -0.985 times a one-pole (pole 0.2). Loop tuning: nut delay + bridge delay + bridge-filter phase delay = SR / f0.
- Bow position tracks the note (beta 0.13-0.15, 60 ms glide). Bow force 0.6 plus a pitch-dependent term plus attack and re-bow accents. Bow speed with swell and attack.
- Four Ornstein-Uhlenbeck processes (speed, pressure, position, vibrato rate); vibrato with delay and ramp; pitch wander (random walk); bow noise.
- Stroke planner, separate from the string: `plan_strokes(notes, rng) -> Vec<Phrase>` splits long notes, places bow changes, slides and direction.
- Fractional delay reads: 4-point Lagrange (third order) instead of linear. Linear interpolation is a fraction-dependent low-pass, so vibrato currently modulates loop loss and brightness. Lagrange raises high-frequency loop gain; the bridge pole or the 0.985 reflection is re-tuned so the Helmholtz sweep holds (section 12) and the spectrum stays within the gate.
- Control rate 16 samples with per-sample ramps of speed, slope and delay lengths.
- Output taps the incoming bridge wave (documented).
- Measured today (JS): 213/216 single notes and 174/174 phrase notes hold Helmholtz motion; violin scores 0.47-0.74 as violin. The filtered-sawtooth predecessor scored 0.01. Rust, wave 5 (BRIDGE_POLE 0.35, reflection 0.985): 212/216 single notes, 119/120 phrase notes.

### 5.8 Drums (`instruments::drums`)

- `DrumKind { Kick, Snare, Rim, Tap, Swish { dur }, Hat, Shaker, Tom { hz }, Ride }`, rendered in place into the stereo stem with pan gains (no per-hit allocation).
- Kick: sine swept 119 to 47 Hz, 170 ms decay, 4 ms noise click. Snare: 188 Hz sine plus 1.4 kHz high-passed noise. Rim: 1.8 kHz Q 6 band-passed noise plus 520 Hz ping. Brush tap: two resonant noise heads at 190 and 330 Hz plus 3.8 kHz snare wires. Swish: band-passed noise with a sin^1.5 swell, 9 dB below the old level and stroke-modulated. Hat: 7.2 kHz high-passed noise. Shaker. Tom: swept sine. Ride: six square oscillators at 800-2960 Hz (808-style cluster), high-passed at 3.5 kHz, plus a 5.2 kHz ping.
- Ride squares are band-limited with PolyBLEP (Valimaki and Huovilainen 2007). Envelopes are recursive (one multiply per sample), oscillators recursive.
- A `DrumKit::None` song produces no drum stem.

### 5.9 Bass

- Root-fifth line with approach notes (arrange). Rendered as a dark pluck (brightness 0.12, damping 0.5) plus a sine sub layer (6 ms attack, 0.7 s decay, 2600-sample release ramp). The sine is a recursive oscillator; the decay is a one-multiply recursion.

### 5.10 FFT and convolution (`dsp::fft`, `dsp::conv`)

- FFT: typed wrappers over `rustfft` (mixed-radix Cooley-Tukey, radix 4/8 butterflies, AVX or SSE kernels chosen at run time) and `realfft` (a real transform of size n from a complex transform of n/2 plus the split step; Sorensen, Jones, Heideman, Burrus 1987). Interleaved `f32` complex, caller-owned scratch, length mismatches returned as errors. The hand-written Stockham transform first planned here was not built.
- Convolution: overlap-add (Stockham 1966). `StereoIr::new(l, r)` precomputes both half spectra once. Per block: one forward real FFT, two complex products, two inverse real FFTs (1.5 full-size transforms per block, down from 2). FFT size n = next power of two >= 4 * IR length. All-zero input blocks are skipped.
- Parallel: rayon over contiguous ranges of blocks; each range writes its span plus the IR tail to a private buffer; ranges are added in time order.
- Target: <= 0.5 ms per 65536-point complex transform on one core of this machine (i7-9750H), against 2.4 ms today.

### 5.11 Dynamics (`dsp::dynamics`)

- `PeakDetector { attack, release }`: branching one-pole on |x|. `GainComputer { thr_db, ratio, knee_db }`: quadratic soft knee (Giannoulis, Massberg, Reiss 2012, JAES). `Compressor { det, gc, link: Mono | StereoMax }`, gain updated every 16 samples, linearly interpolated per sample.
- Lead and harmony: 3:1, attack 8 ms, release 150 ms, knee 6 dB, threshold 1 dB above the 0.1 target level. Bus: 2.2:1, 20/300 ms, knee 10 dB, threshold bus RMS + 5 dB, stereo linked by max(|L|, |R|).

### 5.12 Reverb and slapback

- `Fdn8`: feedback delay network (Jot and Chaigne 1991). Eight lines, 1433-2903 samples plus a seed offset up to 30, in one contiguous power-of-two buffer. Householder feedback I - (2/N) 1 1^T. Per-line one-pole absorption from two T60s (2.2 s at DC, 0.8 s at Nyquist). Four series Schroeder allpasses (Schroeder 1962), lengths 142, 379, 107, 277, g 0.58-0.65. 16 ms predelay; 220 Hz high-pass (Biquad) on the input; side signal injected with alternating sign; fixed output taps. Wet 0.55. `process_block(send: [&[f32]; 2], out: [&mut [f32]; 2])`.
- Slapback (lead only): 340 ms feedback comb, feedback 0.22 through a 3.2 kHz low-pass (Biquad), level 0.07, added to main and send buses.

### 5.13 Mix

- Per track: EQ cascade (TDF-II biquads), gated loudness `active_rms` (2048-sample blocks above 5% of the loudest block; kept unweighted, see section 11), compressor where set, level to 0.1.
- Pan: mono tracks equal-power `(cos, sin)` of `(pan + 1) pi / 4`; stereo tracks the balance law (near channel at unity, a cos/sin part of the far channel folded across). One `dsp::pan` module.
- Strip values (gain, pan, send; EQ):
  - lead 1.0, 0, 0.20; HP 90/0.7, PK 250/1.0/-1.5, PK 2900/1.0/+1.5; comp; slapback
  - doubles 0.30, 0, 0.34; HP 140, HS 6000/-6
  - harmony 0.40, 0.28, 0.32; HP 130, HS 6500/-5; comp
  - choir 0.36, 0, 0.50; HP 120, LP 6500
  - guitar 0.62, -0.20, 0.16; HP 70, PK 115/0.9/+3, HS 9500/-2; body guitar
  - harmony guitar 0.36, 0.45, 0.28; HP 120, HS 9000/-2; body guitar, seed offset 17
  - bass 0.50, 0, 0.04; LP 2000, PK 85/1.0/+2
  - drums 0.42, 0, 0.14; HP 30
  - harp 0.36, 0.34, 0.40; HP 75, PK 180/0.8/+1.5; body harp
  - violin 0.34, -0.40, 0.42; HP 190, HS 2800/+7, HS 7000/+5; body violin
- Block mixer, 1024 frames: clear main and send; add each enabled stem in `TrackId` order with pan gain times strip gain times level; add slapback; run `Fdn8`; write L/R; accumulate bus power (every sample above -80 dB).
- Then the bus compressor pass and peak normalisation to 0.89 (sample peak). The 0.89 target stays.

## 6. What is deleted

- `sfcore::js` (JS number semantics, `f32r`, JS `round/min/max/pow`), `sfcore::v8math`, the `v8` feature, `V8_EXACT`, `core/examples/{mathprobe,wideprobe,mathbench}.rs`, `core/tests/parity_math.rs`, the `libm` dependency if unused.
- `sfcore::rng` (mulberry32, UTF-16 FNV `hash_str`, `rng_for`), the xorshift generator inside `voice::synth`.
- `sfcore::tuning::Tuning` as a mutable bag. Live values become named consts in their modules; dead switches go: `vf.legacy`, `vf.trans`, `vf.vbar`, `vf.nlp`, `vf.b1x`, `hfg`, `choir_vowel` as a string.
- All parity tests and data: `crates/*/tests/parity_*.rs`, `engine/tests/threaded_parity.rs`, `tests/parity/`, `ref/parity/`, the `engine/capture_raw` feature and `RenderedSong::raw_tracks`.
- Twins and aliases: `render_song`/`render_song_threaded`, `mix`/`mix_threaded`, `mix_song`/`mix_song_threaded`, `process_track`/`process_tracks_threaded`, `render_choir`/`render_choir_threaded` and `CHUNK`, `run_bq`/`run_bq_into`, `make_fft`, `ks_pluck`, the `--sequential` flag.
- Option bags and JS defaults: `PluckOpts`, `VoiceOpts` with `or_falsy/or_null/resolve_rng`, `dsp::or_default/truthy`, `ViolinNote.vib: Option<f64>`, `knee: Option<f64>`, `hf_gain`, `breath_amt`, `VoiceParams.oq`.
- String keys: `TrackSpec.key`, `track_index`, `eq_for`, `body_of`, `Body::from_name`, section/mode/meter/guitar/drums/break-lead strings, `seg_kind`, `cons_kind`.
- Dead code: the non-legacy stop branch; `hy1`; the `r.next() < 0.0` climax-strum draw; NaN-path and discarded draws in `lines.rs`; unused `_lead`, `_song`; the reverb no-op branch; negative-index loops in bass and pluck; `qual_key_order`; the U+2669 replacement; `song_direction`'s placeholder draws; `prof.tess.inst`/`prof.shape.inst` draws made only to keep stream order; compose pass 2 in `prepare` (after the equivalence test); `style_glue.rs`; `_assert_track_count`; per-track wrappers in `engine/band.rs`.
- `dsp::noise::noise_buf` (replaced by `Rng::fill_bipolar` into scratch).
- `hound` from `export` (a direct WAV writer replaces it; `soundgate` keeps `hound` for reading).

## 7. The model-JSON boundary

- `song::wire::WireSong`: serde structs with `#[serde(default)]` and small lenient deserialisers (chords as array or comma/pipe string; tempo as number or numeric string; section type aliases).
- `song::wire::normalize(w: WireSong) -> Result<(Song, Vec<Repair>), SongError>`.
  - `SongError` (hard): not JSON; no section with a parseable lyric line; no parseable chord anywhere.
  - `Repair` (soft, listed, never silent): `DefaultedField(&'static str)`, `ClampedTempo { from, to }`, `DroppedChord { section, bar, symbol }`, `UnknownChord { symbol }` (bar left without that chord), `PhonemeFallback { section, line, syllable }` (G2P for one syllable when its ARPAbet group is missing or unparseable; the whole line falls back only when the group count cannot be aligned), `MissingRepeatSource { section }`, `ModeFromKey`.
- Fixes at the boundary: case-insensitive mode suffix (`m`, `min`, `minor`) in the key; `bass` accepted as a voice; strings truncated by chars.
- After normalise, the song is fully typed and every value is in range. The render path has no `Result` and no panics on data; invariants use `debug_assert!`.
- `song::schema::json_schema()` generates the reply schema's enum lists from the same enums, so schema and parser cannot disagree.
- The CLI prints repairs as warnings and writes them beside the raw JSON on `write`.

## 8. Composition and arrangement (kept algorithms)

- Rhythm (`compose::rhythm::set_text`): Viterbi over a monotone alignment of syllables to metric-grid onsets (an explicit-duration model: state is the onset slot, transition is the gap), with perturb-and-MAP noise (Papandreou and Yuille 2011). Weights in a named `RhythmWeights` const; integer gaps in half-slot units; one fallback that quantises to the grid.
- Pitch (`compose::pitch::pitch_line`): second-order HMM Viterbi over (previous, current) pitch pairs; emissions for chord-tone fit, contour target, reference line, cadence, continuity; transitions for interval size, tritone, leap recovery (gap fill, Meyer 1956), anti-wobble; perturb-and-MAP noise. `PitchWeights` const; `Cadence` enum; emissions precomputed into a matrix; flat back-pointer array.
- One `compose_line(&LineSpec, rng) -> LineMelody` for vocal and instrumental lines. The melody cache key includes the chords.
- Compose once in the written key, then transpose. An equivalence test over 100 seeds and all keys proves pass 1 plus `key_shift` equals pass 2 before pass 2 is deleted. The octave alignment rounds half up (`(x + 0.5).floor()`), which is the current rule.
- Guitar voicing: exhaustive fretboard search with the current cost terms; `PcSet(u16)` bitmask; memoised by (pcs, bass); returns frets and notes.
- Choir voicing: exhaustive minimal voice-leading SATB search (current costs).
- Counter-line: greedy first-species-style choice (current costs). Fills: descending scalar runs.
- Shared helpers: `PcSet::tones_in(lo, hi)`, `fold_octave`, `Sec::beats`, `Sec::is_repeat_lift`, `Timeline::len_samples`.

## 8a. Writer, arranger, player

Three parts, joined by two files.

- Writer: Claude. Its output is the song JSON (section 7). Every writing decision is in it.
- Arranger: `compose::prepare` then `arrange::arrange`, run by `engine::arrange_song(song, seed, VoiceChoice)`. It turns the song into note events in seconds and makes every random draw of the arrangement. It renders no audio.
- Player: `engine::play(&Performance, &dyn Progress)`: the instruments, the voice and the channel strips, giving `Stems`; `engine::mix` then mixes them. It reads nothing but the performance and makes no writing or arranging decision. `render_with` is `arrange_song` followed by `play`, with the same output as before the split.

The file between arranger and player is the performance (`sunflower arrange`, `sunflower play`; by convention `x.arrangement.json`). One JSON object, `engine::Performance`, version 1 (`engine::PERFORMANCE_VERSION`):

- `version`: 1. `play` in the CLI refuses any other value. A change to a field here, or to an event type of `song::events`, raises the version.
- `seed`: the song seed; it keys every player stream.
- `end`: timeline end in seconds; the stem length is `len_samples(end)`.
- `choir_key`: sample spans `[start, end]` of the choir's word lines, for the ducker; empty without choir lines.
- `band`: the song's band switches. The mixer gates stems with them; the player ignores them.
- `arrangement`: `arrange::Arrangement`: `guitar` (six lists of `StringNote`), `bass`, `harp` (`PluckNote`), `drums` (`DrumHit`, or null without a kit), `violin` (`BowNote`), `harmony_guitar` (`lead`, `arp`) and `vocals` (`lead`, `lead_b`, `harmony`, `doubles`, `choir`; each `Singer` with `voice`, `style`, `notes`, `pan`, `offset`).

Field names are the snake_case names of the structs; enums are spelled as in the song JSON (`"baritone"`, `"phrasing": {"delivery": "flowing", ...}`), phonemes as lower-case ARPAbet, and a drum kind as a string (`"Kick"`) or a one-key object (`{"Swish": {"dur": 0.4}}`). Times are `f64` seconds and levels `f32`. The file round-trips exactly: `serde_json` has `float_roundtrip` on, so `f32` and `f64` survive the text, and a test plays a parsed performance and compares every stem sample with a direct render.

## 9. Testing without the JS

Unit tests state physics or mathematics, not reference dumps:

- Biquad: magnitude at design points (LP/HP -3.01 dB at f for Q = 0.7071; peaking gain at f; shelf gain at DC/Nyquist) within 0.05 dB; impulse response decays; state flush.
- One-pole: time constant (63.2% step at tau) within 1%. Klatt resonator: unity DC gain; peak at f within 1%; -3 dB bandwidth within 5%.
- FFT: against an O(n^2) DFT for n in {4, 8, 16, ..., 4096}, relative error < 1e-5; round trip < 1e-6 for n = 65536; real FFT against complex FFT.
- Convolution: against direct convolution on random input and a random 3000-tap IR, error < 1e-5 of peak; linearity; thread-count invariance.
- Pluck: f0 by autocorrelation within 2 cents from E2 to E6; fundamental T60 within 10% of the request; no DC; peak bounded.
- Violin: Helmholtz sweep (MIDI 55-90, velocities 0.4/0.6/0.85, two seeds): fundamental > 0.35 x second harmonic, subharmonic and 1.5 f0 < 0.1 x fundamental (Goertzel), >= 208/216.
- LF source: zero net flow; Rd to Rg identity; band-limited table has no energy above SR/2 at the level's top f0 (< -90 dB).
- FDN: energy decay slope matches the design T60 at low frequency within 10%; stable for 60 s.
- Compressor: static curve equals the gain computer; knee continuity.
- Dynamics and pan: equal-power law sums to unit power.
- Compose/arrange properties: stressed syllables land on slots with weight >= 0.5 at a rate above a threshold; tonic cadences end on the tonic; ranges respected; same seed gives identical output; a `same` chorus repeats pitches exactly; the demo normalises with zero repairs.
- Engine: finite output; peak 0.89 within 1e-3; `RAYON_NUM_THREADS=1` equals the default thread count bit for bit; each stem present or absent as the band says.
- Export: WAV header fields; FLAC decodes (via `flacenc` verification or length check); Ogg non-empty; 16-bit dither statistics.

The sound gate (section 12) compares whole renders.

## 10. Performance targets and plan

Today (default build, i7-9750H 6C/12T): 6.3 s threaded, 16.2 s single, 0.89 / 0.72 GB. Profile: mix `compute_track` 23.9%, `synth_voice` 23.5%, FFT 19.0%, pluck 8.8%, `active_rms` 3.5%, pan 2.5%, controls 2.0%, guitar 2.0%, FDN 1.9%, violin 1.3%.

Targets for the demo, measured as `sunflower demo --seed 1234 -o x.wav` (WAV, so the encoder is excluded): threaded <= 2.5 s, single thread <= 9 s, peak RSS <= 0.50 GB threaded, <= 0.40 GB single thread.

Measured at wave 5 close (gate run w5-final, min of 3): 2.16 s threaded, 6.53 s single thread, 647 MB and 491 MB peak RSS. Speed meets the targets; memory does not. The sparse stem cache holds 22,515 present 4096-frame blocks (guitar 4002, harp 3430, violin 3180, hg 2428, drums 2392, choir 1826, bass 1649, lead 1605, doubles 1132, harmony 871), 369 MB, plus 66 MB of output; the threaded excess is transient per-task render and convolution buffers.

Plan, in order of measured or estimated gain:

1. FTZ/DAZ on every render thread. Measured: mix 4.44 s to 2.78 s single thread.
2. Release profile: `lto = "fat"`, `codegen-units = 1`, `debug = 1` kept for profiles. Estimated 5-15%.
3. FFT and convolution (5.10): 3.4 s to about 0.8 s single thread; threaded, each body convolution runs right after its own track and in parallel ranges.
4. Task graph (3.7): the critical path becomes guitar render plus guitar body (about 1.1 s) instead of choir (4 wide) plus a barrier plus the convolutions.
5. Voice: constant resonators designed once; fixed-size arrays; lax/tense tables interleaved per index; frication by span. Estimated 1.3-1.6x on 3.8 s.
6. Pluck: phase-split loops, `f32` delay line, scratch reuse. Estimated 1.5-2x on 1.4 s.
7. Fused channel strip (cascade, block statistics and sparse skip in one pass) and block mixer. Estimated mix 2.78 s to about 1.2 s single thread, and about 100 MB less.
8. Memory: sparse stems, render-into for voices and drums, no full-length buses, no zero drum stem.
9. Export: FLAC frames encoded in parallel (+2.0 s to about +0.3 s over WAV). Vorbis stays serial.

## 11. Deliberate sound changes

In scope for the rewrite:

| Change | Expected effect | Check |
|---|---|---|
| New RNG and per-event streams | each seed is a different take; same distributions | gate LTAS tolerance for re-seeding waves |
| No `f32` rounding inside feedback loops | trajectories move; inaudible alone | gate; Helmholtz |
| FTZ/DAZ | none (below 1e-38) | gate 0.5 dB |
| TDF-II biquads, `f64` state | none (differences near -140 dB) | gate 0.5 dB |
| Band-limited LF tables | removes aliasing on alto and soprano; baritone unchanged below the fold | vowel distance; gate on lead, harmony |
| Per-sample ramps of tract coefficients | removes hop-rate steps; burst timing stays on the frame grid | vowel distance; gate |
| Compressor gain interpolation | removes zipper steps; no level change | gate |
| Violin Lagrange reads, per-sample control ramps, re-tuned loss | vibrato no longer modulates brightness; slightly brighter tone | Helmholtz >= 208/216; gate on violin |
| PolyBLEP ride | smoother above 5 kHz; if dull, raise the ping, do not keep aliasing | gate on drums, 5-16 kHz bands |
| Mode suffix parse, `bass` voice, per-syllable phoneme fallback | correct handling of hand-written JSON | unit tests |
| Compose once, transpose | none (proved by test) | equivalence test |
| TPDF dither at 16 bits | noise at about -96 dBFS in 16-bit WAV/FLAC only | export test |

Later, each gated by the owner's ear and the Python ear tools:

- Low-pass the aspiration noise at 2.6 kHz (what CLAUDE.md says); decide by ear and Whisper word error.
- Vowel modification at high pitch for alto and soprano (open issue 1).
- Klatt nasal pole-zero pair for /m n ng/.
- Slow delay modulation in the FDN.
- Coupling between the two pluck polarisations at the bridge.
- True-peak (4x oversampled) limiter or a -1.5 dBFS ceiling before lossy export.
- K-weighted loudness for track normalisation (needs re-derived strip gains).
- Use the melody profile's instrumental tessitura and contour for break melodies; honour the form plan's break and tag roles.
- Viterbi counter-line; per-string fret tracking for slides and hammer-ons.

## 12. The sound gate

A rough "about right" check that runs here without PANNs or Whisper. The owner's ear stays the judge; the gate catches regressions before the owner listens.

Components:

- `crates/soundgate` (binary `soundgate`): reads WAV (via `hound`), has its own radix-2 `f64` FFT, depends on no engine crate.
  - `soundgate ltas <dir>`: for each `*.wav` in `<dir>`, 1/3-octave long-term average spectrum over IEC 61260 centres 25 Hz-16 kHz (29 bands), power averaged over active 2048-sample blocks (blocks above 5% of the loudest), in dB relative to the file's total active power; plus gated RMS (dBFS), active-block fraction, sample peak, a NaN/inf count. Writes `<dir>/ltas.json`.
  - `soundgate mean <out.json> <dir>...`: summarises one `ltas.json` (and `lead.pitch.json`) per seed: per file and band the mean over seeds of the band level (each seed relative to its own active power, floored at -60 dB) and the seed-to-seed sample std; mean and std of gated RMS and active fraction; the peak range; the NaN/inf count; lead pitch pooled over seeds.
  - `soundgate compare-mean <base.json> <new.json> [--bands]`: the take-robust comparison of two summaries (rules below). Prints a table per file (worst band, delta, base sd, allowance), exits non-zero on failure.
  - `soundgate compare <base.json> <new.json> [--tol-mid DB] [--tol-edge DB]`: per-seed comparison of two `ltas.json`: max |delta| over 100 Hz-10 kHz and over the other bands; active fraction within 15 points; mix gated RMS within 1.5 dB; mix peak 0.89 +- 0.002; zero NaN/inf.
  - `soundgate pitch <wav> <notes.json>`: YIN f0 (de Cheveigne and Kawahara 2002) over the middle 60% of each note of 150 ms or longer; the fraction within 50 cents of the note's MIDI pitch and the count of octave errors. `soundgate pitch-compare` compares two reports.
- `crates/engine/examples/stems.rs --seed S --out DIR`: renders the demo song and writes each processed stem (post EQ, compression and level normalisation; pre pan) as 32-bit float WAV named by track, `mix.wav`, `notes.json` (lead notes: t0, t1, midi) and `render.json` (wall time per stage).
- `crates/voice/examples/vow.rs`: mean spectral distance between ten sustained vowels, 200 Hz-2.5 kHz, baritone. Prints `mean vowel distance(200-2.5k) dB X`.
- `crates/instruments/examples/helmholtz.rs`: the sweep of section 9. Prints `stable N/216`.
- `scripts/gate.sh [--strict] [--against DIR] [--capture-baseline] [--targets] LABEL`: renders the stems for 8 seeds (1234, 2718, 1, 7, 42, 99, 314, 1618: 8 takes of the demo song) into `out/gate/LABEL/sS/`, measures and summarises them, compares the summary with `tests/soundgate/baseline-mean/ltas-mean.json` (or `DIR/ltas-mean.json` with `--against`), runs the vowel and Helmholtz probes, times the demo threaded and on one thread (3 runs each), appends a row to `tests/soundgate/perf.tsv`, prints PASS/FAIL per check. WAVs are kept for seeds 1234 and 2718 only.

Why a multi-seed mean: each seed is a different take (other melody notes, other arrangement detail). A per-seed LTAS delta between two engines mixes the engine change with the take change, and on sparse stems one take differs from another by up to 20 dB in a band. The mean of 8 takes has a third of the take noise, and the seed-to-seed std says how much of a delta the takes alone explain.

Mean-gate rules (default mode; `soundgate::mean`):

| Check | Pass |
|---|---|
| LTAS per stem and mix, 100 Hz-10 kHz | \|mean delta\| <= max(3 dB, min(2 x base sd, 6 dB)) |
| LTAS outside 100 Hz-10 kHz | \|mean delta\| <= max(6 dB, min(2 x base sd, 9 dB)) |
| Mean active-block fraction per file | within 15 points |
| Mean gated RMS per file | within 1.5 dB |
| Mix peak, every seed | 0.89 +- 0.002; no NaN/inf in any file |
| Lead pitch, pooled over seeds | fraction within 50 cents >= base - 0.03; octave errors per seed <= base + 1 |
| Vowel distance | 12.8-14.0 dB (13.2-13.6 is the good reference; outside it, report) |
| Helmholtz | >= 208/216 |
| Thread invariance | `sha256` of the demo WAV equal threaded and at `RAYON_NUM_THREADS=1` |
| Render time, peak RSS | reported against the previous pass row; fail only above +50% of the first pass row (the perf baseline). Section 10's figures are aims, not requirements. |

The allowance rule. The difference of two 8-take means of one engine has std s/2 (s the seed-to-seed std), so 2s is about 4 standard errors of the null difference, and a shift smaller than two takes already differ by is not a change of character. The fixed 3/6 dB keeps stable bands from failing on sub-audible drift. The cap (6 dB mid, 9 dB edge) covers bands whose takes are bimodal (a take has notes in the band or not), where s is large and 2s would admit any shift: in the wave-3 baseline-mean, harmony 160 Hz has s = 9.6 dB (2s = 19.3), doubles 125 Hz 7.2, guitar 100 Hz 6.5; 19 of 210 stem bands in 100 Hz-10 kHz had 2s > 6 dB, none outside it had 2s > 9 dB. A robust spread (median absolute deviation) was rejected: on bimodal bands it is either larger than s (harmony 160 Hz, 14.1 dB) or the majority's spread (guitar 100 Hz, 0.6 dB). Mean gated RMS 1.5 dB holds: the largest seed-to-seed RMS std in the baseline is 0.48 dB (harmony_guitar; mix 0.32), a null std of at most 0.24 dB for the difference of two means. Because the seeds are fixed, an engine that renders the same takes compares at delta 0 plus its own change; a composition change that alters the takes can move a bimodal band by more than the cap and then needs a listen and a re-baseline.

`--strict`: for refactors that must not change the samples. In addition to the mean gate, seeds 1234 and 2718 are compared one by one against `tests/soundgate/baseline/sS/` (or `--against DIR`'s `sS`) with `soundgate compare` at 0.5 dB in 100 Hz-10 kHz and 1 dB elsewhere, and per-seed pitch (fraction within 50 cents >= base - 0.03, octave errors <= base + 2).

Baselines: `tests/soundgate/baseline-mean/` (`ltas-mean.json`, `vow.txt`, `helmholtz.txt`) for the mean gate and `tests/soundgate/baseline/sS/` (`ltas.json`, `lead.pitch.json`, `notes.json`, `render.json` of seeds 1234 and 2718) for `--strict`; JSON and text only, committed; audio stays in `out/` (ignored by git). `scripts/gate.sh --capture-baseline LABEL` writes both from one run. The first baseline was captured from commit 29c5c08 (release profile with LTO); the multi-seed baselines were captured at wave 3.

Re-baselining. The rule was: the baseline is replaced only after the owner listens, in its own commit. For this rewrite the owner waived it: renders are sent to him for listening at each sound-changing wave, and he asked for a rough about-right check, not a gate on each baseline change. A wave that changes the sound on purpose re-captures the baselines at wave close, from the tree with the whole wave merged, in its own commit, and the renders go to the owner.

## 13. Rewrite plan

Waves run in order; units within a wave run in parallel with disjoint files. The workspace builds and the gate passes at the end of every wave. A unit that changes a public API keeps the old signature as a thin shim over the new code unless it owns every caller; the last wave deletes the shims. Each unit removes `sfcore::js` and `sfcore::rng` use from the files it owns.

- Wave 0: the sound gate and its baseline.
- Wave 1: foundations and primitives (sfcore; dsp filters, dynamics, pan; dsp delay and stochastic processes; FFT and convolution; `song` crate).
- Wave 2: typed model migration across all consumers; instruments (pluck, body, guitar sympathetic; violin; drums); FDN reverb; export.
- Wave 3: voice source and tract; compose internals; arrange as events with band rendering on instruments; songwriter internals.
- Wave 4: engine (tracks, stems, task graph, strips, mixer) and CLI; voice articulation.
- Wave 5: delete the JS linkage and shims; update CLAUDE.md.

The unit briefs are kept with the plan in the orchestration output.

Units (ownership is disjoint within a wave):

| Wave | Unit | Owns |
|---|---|---|
| 0 | gate: soundgate crate, probes, gate script, baseline, release profile | `crates/soundgate/`, `crates/engine/examples/stems.rs`, `crates/dsp/examples/helmholtz.rs`, `scripts/gate.sh`, `tests/soundgate/`, root `Cargo.toml`, `.gitignore` |
| 0 | scaffold: empty modules and crates | `crates/core/src/lib.rs` + new empty modules, `crates/dsp/src/lib.rs` + new empty modules, `crates/song/`, `crates/instruments/` |
| 1 | sfcore: math, fp (FTZ), random, time; FTZ wiring | `crates/core/src/{math,fp,random,time}.rs`, `crates/engine/src/render.rs`, `crates/sunflower/src/main.rs` |
| 1 | dsp filters, smoother, dynamics, pan (old API as shims) | `crates/dsp/src/{biquad,onepole,resonator,smoother,filter,dynamics,pan}.rs`, `crates/dsp/tests/filters.rs` |
| 1 | dsp delay lines, allpasses, stochastic processes, noise | `crates/dsp/src/{delay,stochastic,noise}.rs`, `crates/dsp/tests/delay.rs` |
| 1 | FFT and convolution (old API as shims) | `crates/dsp/src/{fft,conv}.rs`, `crates/dsp/tests/fft.rs`, `crates/dsp/examples/fftbench.rs` |
| 1 | song crate | `crates/song/` |
| 2 | typed model migration of all consumers | `crates/{compose,arrange,voice,songwriter,sunflower}/`, `crates/engine/` |
| 2 | plucked string, guitar sympathetic strings, bodies | `crates/instruments/src/{pluck,body,guitar}.rs`, their tests and bench |
| 2 | bowed string | `crates/instruments/src/violin.rs`, `crates/instruments/examples/helmholtz.rs`, its tests |
| 2 | drum voices and FDN reverb | `crates/instruments/src/drums.rs`, `crates/dsp/src/reverb.rs`, their tests |
| 2 | export | `crates/export/` |
| 3 | voice source, tract, synth, phrase API | `crates/voice/` |
| 3 | compose internals, new RNG | `crates/compose/` |
| 3 | arrange as events, band rendering on instruments | `crates/arrange/`, `crates/engine/src/{band,vocals,render,lib}.rs` |
| 3 | songwriter internals | `crates/songwriter/` |
| 4 | engine: tracks, stems, task graph, strips, mixer; CLI | `crates/engine/`, `crates/dsp/src/mix.rs`, `crates/sunflower/` |
| 4 | voice articulation | `crates/voice/` |
| 5 | delete the JS linkage and shims; CLAUDE.md | everything that remains, `CLAUDE.md` |

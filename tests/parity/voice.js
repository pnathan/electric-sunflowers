// Reference dump for the voice crate's synthVoice/renderVoice parity: the
// same four cases as tests/parity/voice_controls.js (vowels, sing2 lines
// baritone and alto, the first 30 DEMO lead notes at seed 1234, a choir note
// list), plus a harmony-voice case (vibScale .8, breathScale 1.2, as
// renderSong's harmony track uses) and a doubles case (detune, noBreath, as
// renderSong's doubled-melody track uses; engine.js ~890-905).
//
// Writes ref/parity/voice/<case>.bin (float32 audio, length `len` samples)
// plus ref/parity/voice/index.json with each case's `len`.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));
const { LINES, PH } = require(path.join(__dirname, '../words.js'));

const OUT = path.join(__dirname, '../../ref/parity/voice');
fs.mkdirSync(OUT, { recursive: true });
const index = { hop: HOP, sr: SR, cases: [] };

function dump(name, audio, len) {
  const buf = audio.length === len ? audio : audio.subarray(0, len);
  fs.writeFileSync(path.join(OUT, name + '.bin'), Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength));
  index.cases.push({ name, len });
}

// ---------------- (a) ten sustained vowels ----------------
{
  const VW = ['iy', 'ih', 'eh', 'ae', 'aa', 'ao', 'ow', 'uw', 'ah', 'er'];
  const sp = [];
  let t = 0.5;
  for (const v of VW) {
    sp.push({ t0: t, t1: t + 1.2, midi: 52, ph: ['hh', v], nu: null, amp: 1, phraseStart: false, phraseEnd: false, grace: null, stress: false });
    t += 1.6;
  }
  const len = Math.ceil((t + 1) * SR);
  const P = VOICES.baritone;
  const opts = { seed: 3, rng: rngFor(3, 'v'), noScoop: true, vibScale: 0 };
  const audio = renderVoice(sp, P, len, opts);
  dump('vowels', audio, len);
}

// ---------------- (b) first 4 sing2 lines, baritone and alto ----------------
for (const vk of ['baritone', 'alto']) {
  const P = VOICES[vk];
  const base = Math.round((P.lo + P.hi) / 2) - 2;
  const mel = [0, 2, 4, 2, 0, -1, 0, 2, 4];
  LINES.slice(0, 4).forEach((ws, li) => {
    const notes = [];
    let t = 0.5, k = 0;
    for (const w of ws) {
      const sy = Array.isArray(PH[w][0]) ? PH[w] : [PH[w]];
      sy.forEach((ph, j) => {
        const d = j === 0 && sy.length > 1 ? 0.3 : 0.42;
        notes.push({ t0: t, t1: t + d * 0.92, midi: base + mel[k % mel.length], ph, amp: 1, phraseStart: k === 0, phraseEnd: false, grace: null, stress: false });
        t += d;
        k++;
      });
    }
    notes[notes.length - 1].phraseEnd = true;
    notes[notes.length - 1].t1 += 0.4;
    const len = Math.ceil((t + 1) * SR);
    const opts = { seed: 7 + li, rng: rngFor(7 + li, 's') };
    const audio = renderVoice(notes, P, len, opts);
    dump(`sing2_${vk}_${li}`, audio, len);
  });
}

// ---------------- (c) DEMO_SONG lead, first 30 notes, seed 1234 ----------------
{
  const song = normalizeSong(DEMO_SONG);
  const seed = 1234;
  const P0 = prepare(song, seed, 'auto');
  const lead = P0.comp.lead.slice(0, 30);
  const notes = vocalNotes(lead, 1);
  const VP = VOICES[P0.voice];
  const len = Math.ceil((notes[notes.length - 1].t1 + 1) * SR);
  const opts = { seed: seed ^ 11, rng: rngFor(seed, 'lead') };
  const audio = renderVoice(notes, VP, len, opts);
  dump('demo_lead_30', audio, len);

  // ---------------- (e) harmony voice, as renderSong builds it ----------------
  {
    const hOpts = { seed: seed ^ 23, rng: rngFor(seed, 'harm'), vibScale: 0.8, breathScale: 1.2 };
    const hAudio = renderVoice(notes, VP, len, hOpts);
    dump('harmony', hAudio, len);
  }

  // ---------------- (f) doubled melody, as renderSong builds it ----------------
  {
    const Pd = Object.assign({}, VP, { fs: VP.fs * 1.03, breath: VP.breath + 0.05 });
    const dOpts = { seed: seed ^ 32, rng: rngFor(seed, 'dbl1'), detune: -0.06, vibScale: 0.7, rateScale: 1.07, noBreath: true };
    const dAudio = renderVoice(notes, Pd, len, dOpts);
    dump('doubles', dAudio, len);
  }
}

// ---------------- (d) choir-style notes: ph null, nu ['aa'] ----------------
{
  const notes = [
    { t0: 0.5, t1: 1.3, midi: 60, ph: null, nu: ['aa'], amp: 0.8, phraseStart: true, phraseEnd: false, grace: null, stress: false },
    { t0: 1.3, t1: 2.1, midi: 62, ph: null, nu: ['aa'], amp: 0.8, phraseStart: false, phraseEnd: false, grace: null, stress: false },
    { t0: 2.1, t1: 3.0, midi: 64, ph: null, nu: ['aa'], amp: 0.8, phraseStart: false, phraseEnd: true, grace: null, stress: false },
  ];
  const P = Object.assign({}, VOICES.alto, { breath: VOICES.alto.breath + 0.05, fs: VOICES.alto.fs * 0.98, f1s: VOICES.alto.f1s * 1.0, jitter: VOICES.alto.jitter * 1.6, shimmer: VOICES.alto.shimmer * 1.4 });
  const len = Math.ceil((notes[notes.length - 1].t1 + 1) * SR);
  const opts = {
    seed: 999, rng: rngFor(999, 'ch00'),
    rdScale: 1.15, hfGain: 0, nHigh: CHH, avTau: 0.05,
    vibScale: 0.7, rateScale: 0.9, detune: 0.05,
    noScoop: true, noBreath: true, glide: 0.05,
  };
  const audio = renderVoice(notes, P, len, opts);
  dump('choir', audio, len);
}

fs.writeFileSync(path.join(OUT, 'index.json'), JSON.stringify(index, null, 1));
console.log('voice.js: wrote', index.cases.length, 'cases to', OUT);

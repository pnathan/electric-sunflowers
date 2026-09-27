// Reference dump for the voice crate's voiceControls parity: splitPh,
// nucTargets, consDur (exercised indirectly) and voiceControls's twelve
// per-frame control tracks, over four cases:
//  (a) the ten sustained vowels of tests/vow.js
//  (b) the first 4 sung lines of tests/sing2.js (words from tests/words.js)
//      for baritone and alto
//  (c) the first 30 lead notes of DEMO_SONG prepared at seed 1234, via
//      vocalNotes(lead,1)
//  (d) a choir-style note list with ph null and nu ['aa'], with the opts
//      renderSong uses for the choir
//
// Writes ref/parity/voice_controls/<case>.bin (float32, the twelve tracks
// concatenated in order av,ah,af,ff,fbw,f1,f2,f3,nas,m,vb,b1x) plus
// ref/parity/voice_controls/index.json describing each case's frame count.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));
const { LINES, PH } = require(path.join(__dirname, '../words.js'));

const OUT = path.join(__dirname, '../../ref/parity/voice_controls');
fs.mkdirSync(OUT, { recursive: true });
const index = { hop: HOP, sr: SR, order: ['av', 'ah', 'af', 'ff', 'fbw', 'f1', 'f2', 'f3', 'nas', 'm', 'vb', 'b1x'], cases: [] };
// voiceControls returns {AV,AH,AF,FF,FBW,F1,F2,F3,NAS,M,VB,B1X}; index.order
// is the lowercase name used on the Rust side, keyMap gives the JS key.
const keyMap = { av: 'AV', ah: 'AH', af: 'AF', ff: 'FF', fbw: 'FBW', f1: 'F1', f2: 'F2', f3: 'F3', nas: 'NAS', m: 'M', vb: 'VB', b1x: 'B1X' };

function dump(name, ctl, nF) {
  const total = nF * 12;
  const buf = new Float32Array(total);
  let o = 0;
  for (const k of index.order) { buf.set(ctl[keyMap[k]], o); o += nF; }
  fs.writeFileSync(path.join(OUT, name + '.bin'), Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength));
  index.cases.push({ name, nF });
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
  const nF = Math.ceil(len / HOP) + 2;
  const P = VOICES.baritone;
  const opts = { seed: 3, rng: rngFor(3, 'v'), noScoop: true, vibScale: 0 };
  const ctl = voiceControls(sp, P, nF, opts);
  dump('vowels', ctl, nF);
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
    const nF = Math.ceil(len / HOP) + 2;
    const opts = { seed: 7 + li, rng: rngFor(7 + li, 's') };
    const ctl = voiceControls(notes, P, nF, opts);
    dump(`sing2_${vk}_${li}`, ctl, nF);
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
  const nF = Math.ceil(len / HOP) + 2;
  const opts = { seed: seed ^ 11, rng: rngFor(seed, 'lead') };
  const ctl = voiceControls(notes, VP, nF, opts);
  dump('demo_lead_30', ctl, nF);
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
  const nF = Math.ceil(len / HOP) + 2;
  const opts = {
    seed: 999, rng: rngFor(999, 'ch00'),
    rdScale: 1.15, hfGain: 0, nHigh: CHH, avTau: 0.05,
    vibScale: 0.7, rateScale: 0.9, detune: 0.05,
    noScoop: true, noBreath: true, glide: 0.05,
  };
  const ctl = voiceControls(notes, P, nF, opts);
  dump('choir', ctl, nF);
}

fs.writeFileSync(path.join(OUT, 'index.json'), JSON.stringify(index, null, 1));
console.log('voice_controls.js: wrote', index.cases.length, 'cases to', OUT);

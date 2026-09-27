// Reference dump for the engine crate's vocals module: renderSong's four
// vocal tracks (lead, harmony, doubles, choir), over DEMO_SONG at seed 1234,
// voice 'auto' (engine.js ~870-919). This copies that slice of renderSong
// verbatim rather than calling the (async, side-effecting) renderSong
// itself, so the reference is exactly the code the Rust port is checked
// against.
//
// Each track's channel(s) are dumped as: three exact 2s windows at 25%, 50%
// and 75% of the buffer length, a strided sample (every 97th sample, a
// prime so it cannot alias the engine's periodic structure) over the whole
// buffer, and sum/sumsq over the whole buffer computed in f64 in index
// order. Writes ref/parity/song_vocals.json.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));

const OUT_DIR = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(OUT_DIR, { recursive: true });

const STRIDE = 97;
const WIN_N = 2 * SR; // 2s window, exact

function windowAt(buf, frac) {
  const len = Math.min(WIN_N, buf.length);
  let start = Math.floor(frac * buf.length);
  start = Math.max(0, Math.min(start, buf.length - len));
  return { start, data: Array.from(buf.slice(start, start + len)) };
}

function strided(buf) {
  const out = [];
  for (let i = 0; i < buf.length; i += STRIDE) out.push(buf[i]);
  return out;
}

function checksum(buf) {
  let sum = 0, sumsq = 0;
  for (let i = 0; i < buf.length; i++) {
    const v = buf[i];
    sum += v;
    sumsq += v * v;
  }
  return { sum, sumsq };
}

function dumpChannel(buf) {
  return {
    len: buf.length,
    win25: windowAt(buf, 0.25),
    win50: windowAt(buf, 0.5),
    win75: windowAt(buf, 0.75),
    stride: STRIDE,
    strided: strided(buf),
    checksum: checksum(buf),
  };
}

function dumpTrack(chs) {
  return chs.map(dumpChannel);
}

const seed = 1234;
const song = normalizeSong(DEMO_SONG);
const P0 = prepare(song, seed, 'auto');
const { form, tl, comp, tonic } = P0;
const len = Math.ceil(tl.end * SR);
const lead = comp.lead;
const VP = VOICES[P0.voice];

const inChorus = n => n.sec.lift;

// ---------------- lead ----------------
const leadTrack = [renderVoice(vocalNotes(lead, 1), VP, len, { seed: seed ^ 11, rng: rngFor(seed, 'lead') })];

// ---------------- harmony ----------------
let harmonyTrack;
{
  const up = P0.voice !== 'soprano';
  const cl = lead.filter(inChorus);
  const hl = harmonyLine(cl, tl, song, tonic, up);
  const med = hl.length ? hl.map(n => n.midi).sort((a, b) => a - b)[hl.length >> 1] : 60;
  let hv = 'tenor', bd = 1e9;
  for (const k of ['baritone', 'tenor', 'alto', 'soprano']) {
    if (k === P0.voice) continue;
    const c = (VOICES[k].lo + VOICES[k].hi) / 2;
    if (Math.abs(c - med) < bd) { bd = Math.abs(c - med); hv = k; }
  }
  const hn = vocalNotes(hl, 0.9).map(n => Object.assign(n, { t0: n.t0 + 0.008, t1: n.t1 + 0.008 }));
  harmonyTrack = [hn.length ? renderVoice(hn, VOICES[hv], len, { seed: seed ^ 23, rng: rngFor(seed, 'harm'), vibScale: .8, breathScale: 1.2 }) : new Float32Array(len)];
}

// ---------------- doubles ----------------
let doublesTrack;
{
  const dl = lead.filter(n => inChorus(n) && n.sec.liftIdx > 0);
  const L = new Float32Array(len), R = new Float32Array(len);
  if (dl.length) {
    for (const [k, pan, off, det] of [[0, -.6, .013, .07], [1, .6, .021, -.06]]) {
      const Pd = Object.assign({}, VP, { fs: VP.fs * (k ? 1.03 : .97), breath: VP.breath + .05 });
      const dn = vocalNotes(dl, 0.8).map(n => Object.assign(n, { t0: n.t0 + off, t1: n.t1 + off }));
      const v = renderVoice(dn, Pd, len, { seed: seed ^ (31 + k), rng: rngFor(seed, 'dbl' + k), detune: det, vibScale: .7, rateScale: k ? 1.07 : .94, noBreath: true });
      addPan(L, R, 0, v, pan, 1);
    }
  }
  doublesTrack = [L, R];
}

// ---------------- choir ----------------
let choirTrack;
{
  const filt = s => (s.lift && s.liftIdx > 0) || s.type === 'bridge' || (s.type === 'outro');
  const vs = choirVoicings(form, tl, filt);
  const L = new Float32Array(len), R = new Float32Array(len);
  const presets = ['bass', 'tenor', 'alto', 'soprano'];
  const cr = rngFor(seed, 'choirv');
  for (let p = 0; p < 4; p++) {
    for (let d = 0; d < CHOIR_N; d++) {
      const tune = (cr() - 0.5) * 0.22, late = 0.012 + cr() * 0.035, vs_ = 0.55 + cr() * 0.45, rate = 0.85 + cr() * 0.3, fsx = 0.95 + cr() * 0.1, br = 0.04 + cr() * 0.08;
      const notes = [];
      for (const { sg, v } of vs) {
        const sec = sg.sec;
        const nu = CHV(sec);
        const t0 = tl.toTime(sg.b0) + late + (cr() - 0.5) * 0.03,
          t1 = tl.toTime(sg.b1) - (sg.b1 === sec.startBar * form.mi.bpb + sec.nBars * form.mi.bpb ? 0.1 + cr() * 0.08 : 0.01 + cr() * 0.02);
        const prev = notes[notes.length - 1];
        notes.push({ t0, t1, midi: v[p], ph: null, nu, amp: (sec.type === 'bridge' ? 0.65 : 0.8) * (0.88 + cr() * 0.2), phraseStart: !prev || t0 - prev.t1 > 0.1, phraseEnd: false });
      }
      for (let i = 0; i < notes.length; i++) {
        const nx = notes[i + 1];
        if (!nx || nx.t0 - notes[i].t1 > 0.1) notes[i].phraseEnd = true;
      }
      if (!notes.length) continue;
      const B = VOICES[presets[p]];
      const Pc = Object.assign({}, B, { breath: B.breath + br, fs: B.fs * fsx, f1s: B.f1s * (0.97 + cr() * 0.06), jitter: B.jitter * 1.6, shimmer: B.shimmer * 1.4 });
      const v = renderVoice(notes, Pc, len, { seed: seed ^ (101 + p * 7 + d), rng: rngFor(seed, 'ch' + p + d), rdScale: 1.1 + cr() * 0.15, hfGain: 0, nHigh: CHH, avTau: .05, vibScale: vs_, rateScale: rate, detune: tune, noScoop: true, noBreath: true, glide: .05 });
      const pan = [-.5, -.2, .25, .55][p] + (d - (CHOIR_N - 1) / 2) * 0.35;
      addPan(L, R, 0, v, clamp(pan, -0.9, 0.9), 1);
    }
  }
  choirTrack = [L, R];
}

const out = {
  seed,
  len,
  lead: dumpTrack(leadTrack),
  harmony: dumpTrack(harmonyTrack),
  doubles: dumpTrack(doublesTrack),
  choir: dumpTrack(choirTrack),
};

fs.writeFileSync(path.join(OUT_DIR, 'song_vocals.json'), JSON.stringify(out));
console.log('wrote ref/parity/song_vocals.json');

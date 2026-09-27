// Reference dump for the dsp crate: run_bq, pluck/ksPluck, bodyIRData, convStereo,
// compress/stereoCompress, fdnReverb, renderViolin and mixSong.
//
// Writes ref/parity/dsp/<case>.bin (float32 little-endian, one buffer per line
// unless noted) plus ref/parity/dsp/index.json describing every case: which
// kernel it is, its inputs, and the byte layout of its .bin file.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));

const OUT = path.join(__dirname, '../../ref/parity/dsp');
fs.mkdirSync(OUT, { recursive: true });
const index = { cases: [] };

function writeF32(name, arrays) {
  // arrays: array of Float32Array/array-like, concatenated in order.
  const total = arrays.reduce((a, b) => a + b.length, 0);
  const buf = new Float32Array(total);
  let o = 0;
  for (const a of arrays) { buf.set(a, o); o += a.length; }
  fs.writeFileSync(path.join(OUT, name + '.bin'), Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength));
}

function noise(n, seed) {
  const r = rngFor(seed, 'dspnoise');
  const a = new Float32Array(n);
  for (let i = 0; i < n; i++) a[i] = r() * 2 - 1;
  return a;
}

// ---------------- run_bq ----------------
{
  const types = Object.keys(EQ).length ? ['lp', 'hp', 'bp', 'hs', 'ls', 'pk'] : ['lp', 'hp', 'bp'];
  const freqs = [80, 400, 1200, 6000, 12000];
  const src = noise(4096, 1);
  const cases = [];
  const bufs = [];
  for (const ty of types) {
    for (const f of freqs) {
      const q = 0.7, g = ty === 'hs' || ty === 'ls' || ty === 'pk' ? -4 : 0;
      const c = bq(ty, f, q, g);
      const x = Float32Array.from(src);
      runBq(x, c);
      cases.push({ type: ty, freq: f, q, gainDb: g, coeffs: c, n: x.length });
      bufs.push(x);
    }
  }
  writeF32('run_bq', [src, ...bufs]);
  index.cases.push({ name: 'run_bq', kernel: 'filter::run_bq', input_len: src.length, cases, layout: 'input first, then one buffer per case in order, all input_len samples' });
}

// ---------------- pluck / ksPluck ----------------
{
  const pitches = [82.4, 220, 440, 880];
  const optSets = [
    { amp: 0.8, t60: 4, bright: 0.5 },
    { amp: 0.6, t60: 2.5, bright: 0.8, damp: 0.1, pick: 0.2, noise: 0.1, detune: 1.0, rel: 0.05, relT: 0.12, glide: 3, atkNoise: 0.3 },
    { amp: 1.0, t60: 6, bright: 0.2, pick: 0.05 },
  ];
  const len = Math.round(1.5 * SR);
  const cases = [];
  const bufs = [];
  let seed = 100;
  for (const f of pitches) {
    for (let oi = 0; oi < optSets.length; oi++) {
      const o = Object.assign({}, optSets[oi], { rng: rngFor(seed, 'pluck' + seed) });
      const out = new Float32Array(len);
      pluck(out, 0, f, len, o);
      cases.push({ f, optIdx: oi, seed, n: out.length });
      bufs.push(out);
      seed++;
    }
  }
  writeF32('pluck', bufs);
  index.cases.push({ name: 'pluck', kernel: 'pluck::pluck', opt_sets: optSets.map(o => { const c = Object.assign({}, o); delete c.rng; return c; }), cases, len, layout: 'one buffer per case in order, all `len` samples' });
}
{
  const out = new Float32Array(Math.round(1 * SR));
  const r = rngFor(7, 'kspluck');
  ksPluck(out, 0, 220, out.length, { amp: 1, t60: 3, rng: r });
  writeF32('ks_pluck', [out]);
  index.cases.push({ name: 'ks_pluck', kernel: 'pluck::ks_pluck', f: 220, len: out.length, seed: 7, tag: 'kspluck', opts: { amp: 1, t60: 3 } });
}

// ---------------- bodyIRData ----------------
{
  const cases = [];
  const bufs = [];
  for (const name of ['guitar', 'harp', 'violin']) {
    for (const seed of [3, 17]) {
      const [l, r] = bodyIRData(name, seed);
      cases.push({ name, seed, n: l.length });
      bufs.push(l, r);
    }
  }
  writeF32('body_ir', bufs);
  index.cases.push({ name: 'body_ir', kernel: 'body::body_ir_data', cases, layout: 'per case: L then R, lengths vary by case (n field)' });
}

// ---------------- convStereo ----------------
{
  const x = noise(Math.round(2 * SR), 9);
  const [hl, hr] = bodyIRData('guitar', 3);
  const outLen = x.length + hl.length;
  const [yl, yr] = convStereo(x, hl, hr, outLen);
  writeF32('conv_stereo', [x, hl, hr, yl, yr]);
  index.cases.push({ name: 'conv_stereo', kernel: 'fft::conv_stereo', xLen: x.length, hLen: hl.length, outLen, layout: 'x, hL, hR, yL, yR concatenated' });
}

// ---------------- compress / stereoCompress ----------------
{
  const x = noise(Math.round(2 * SR), 21);
  for (let i = 1; i < x.length; i++) x[i] = 0.995 * x[i - 1] + 0.05 * x[i]; // give it some envelope structure
  const xc = Float32Array.from(x);
  compress(xc, -18, 4, 0.008, 0.15, 6);
  const l = noise(Math.round(1 * SR), 22);
  const r = noise(Math.round(1 * SR), 23);
  const lc = Float32Array.from(l), rc = Float32Array.from(r);
  stereoCompress(lc, rc, -16, 3, 0.02, 0.3);
  writeF32('compress', [x, xc]);
  writeF32('stereo_compress', [l, r, lc, rc]);
  index.cases.push({ name: 'compress', kernel: 'dynamics::compress', n: x.length, thrDb: -18, ratio: 4, atk: 0.008, rel: 0.15, knee: 6, layout: 'input, output' });
  index.cases.push({ name: 'stereo_compress', kernel: 'dynamics::stereo_compress', n: l.length, thrDb: -16, ratio: 3, atk: 0.02, rel: 0.3, layout: 'inL, inR, outL, outR' });
}

// ---------------- fdnReverb ----------------
{
  const n = Math.round(2 * SR);
  const inL = new Float32Array(n), inR = new Float32Array(n);
  const burst = noise(Math.round(0.02 * SR), 31);
  inL.set(burst, 0);
  for (let i = 0; i < burst.length; i++) inR[i] = burst[i] * 0.7;
  const outL = new Float32Array(n), outR = new Float32Array(n);
  fdnReverb(inL, inR, outL, outR, 0.55, 5);
  writeF32('fdn_reverb', [inL, inR, outL, outR]);
  index.cases.push({ name: 'fdn_reverb', kernel: 'reverb::fdn_reverb', n, wet: 0.55, seed: 5, layout: 'inL, inR, outL, outR' });
}

// ---------------- renderViolin ----------------
{
  const notes = [
    { t0: 0.0, t1: 0.6, m: 55, v: 0.6 },
    { t0: 0.62, t1: 1.1, m: 60, v: 0.7 },
    { t0: 1.15, t1: 1.9, m: 64, v: 0.55 },
    { t0: 2.0, t1: 2.4, m: 69, v: 0.8 },
    { t0: 2.5, t1: 3.6, m: 72, v: 0.5, vib: 0 },
    { t0: 3.65, t1: 4.4, m: 67, v: 0.65 },
  ];
  const len = Math.round(5 * SR);
  const out = renderViolin(notes, len, 42);
  writeF32('render_violin', [out]);
  index.cases.push({ name: 'render_violin', kernel: 'violin::render_violin', notes, len, seed: 42 });
}

// ---------------- mixSong ----------------
{
  const len = Math.round(4 * SR);
  function burstTrack(seed, chans) {
    const mk = () => {
      const a = new Float32Array(len);
      const r = rngFor(seed, 'mixburst' + chans);
      // three short noise bursts so activeRms sees signal
      for (const start of [0.2, 1.6, 3.0]) {
        const s0 = Math.round(start * SR), n = Math.round(0.3 * SR);
        for (let i = 0; i < n && s0 + i < len; i++) a[s0 + i] = (r() * 2 - 1) * 0.3 * Math.exp(-i / (0.05 * SR));
      }
      return a;
    };
    const chs = [];
    for (let c = 0; c < chans; c++) chs.push(mk());
    return chs;
  }
  function buildRender() {
    const tracks = {};
    for (const T of TRACKS) {
      const chans = T.key === 'lead' ? 1 : (T.key === 'doubles' || T.key === 'harmony' || T.key === 'choir' || T.key === 'hg' || T.key === 'harp' || T.key === 'violin') ? 2 : 1;
      tracks[T.key] = burstTrack(200 + T.key.length, chans);
    }
    tracks.drums = burstTrack(999, 2);
    return { tracks, len };
  }
  async function dumpMix(enabledPred) {
    const render = buildRender();
    return await mixSong(render, enabledPred, 77, null);
  }
  const allOn = () => true;
  const noHarp = T => T.key !== 'harp';

  (async () => {
    const r1 = await dumpMix(allOn);
    const r2 = await dumpMix(noHarp);
    writeF32('mix_song', [r1.L, r1.R, r2.L, r2.R]);
    index.cases.push({
      name: 'mix_song', kernel: 'mix::mix_song', len, seed: 77,
      track_keys: TRACKS.map(T => T.key),
      variants: [{ label: 'all', enabled: 'all tracks' }, { label: 'no_harp', enabled: 'all but harp' }],
      layout: 'all.L, all.R, no_harp.L, no_harp.R, each len samples',
    });
    fs.writeFileSync(path.join(OUT, 'index.json'), JSON.stringify(index, null, 1));
    console.log('wrote', index.cases.length, 'dsp parity cases to', OUT);
  })();
}

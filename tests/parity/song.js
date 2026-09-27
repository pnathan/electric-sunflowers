// Full-song reference dump for the engine crate's render_song/mix: the real
// (async) renderSong and mixSong, called exactly as the app calls them, not
// a hand copy of their bodies (song_vocals.js copies renderSong's vocal
// slice verbatim for a from-scratch module check; this instead exercises
// the whole pipeline, including the band tracks and the body-convolution
// loop, so it also covers what song_vocals.js cannot).
//
// Four cases:
//  - "demo": DEMO_SONG (src/demo.js), seed 1234, voice 'auto', mixSong with
//    every track enabled.
//  - "blues": the 12-bar-blues literal from tests/formtest.js, seed 7,
//    voice 'alto' (no style applied, so song.breakLead stays unset/'both'
//    as un-styled renderSong always sees it), mixSong with every track
//    enabled.
//  - "blues_guitar_lead": the same 12-bar-blues literal with the real
//    applyStyle(song,'blues') run on it first (style 'blues' has
//    lead:'guitar', so song.breakLead='guitar' and the violin break is
//    silenced; engine.js ~925/~933), seed 7, voice 'alto'.
//  - "blues_violin_lead": the same literal with applyStyle(song,'oldtime')
//    (lead:'violin', so the harmony-guitar break is silenced instead, and
//    the style's band turns the violin track on), seed 7, voice 'alto'.
//
// Each raw track's channel(s) (renderSong's tracks[key], i.e. post body
// convolution, pre processTrack/mixSong) and the final mix L/R are dumped
// as: three exact 2s windows at 25%/50%/75% of the buffer, a strided
// sample (every 97th sample) over the whole buffer, and sum/sumsq over the
// whole buffer computed in f64 in index order. Writes ref/parity/song.json.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');
require(path.join(__dirname, '../lib.js'));
// styles.js (for the real applyStyle) is a plain script, not a module (see
// tests/parity/songwriter.js, which loads it the same way); runs in this
// same global context so it sees clamp/etc. from engine.js and its own
// STYLES/applyStyle land as globals here too.
vm.runInThisContext(fs.readFileSync(path.join(__dirname, '../../src/styles.js'), 'utf8'), { filename: 'styles.js' });
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

// Many tracks (e.g. blues's choir/doubles, a demo vocal track outside its
// section) are silent for large stretches, so a fixed 25/50/75% window
// often lands on all-zero data and never exercises anything. Instead: find
// the 0.5s block with the highest RMS (win50), and the non-silent span's
// first and last nonzero sample (win25/win75 at the 25%/75% points of that
// span). A track silent everywhere reports `silent:true` and all three
// windows fall back to the fixed 25/50/75% points (all zero either way).
function energyWindows(buf) {
  const blockN = Math.max(1, Math.round(0.5 * SR));
  const nblocks = Math.ceil(buf.length / blockN);
  const rms = new Array(nblocks);
  for (let b = 0; b < nblocks; b++) {
    const s = b * blockN, e = Math.min(s + blockN, buf.length);
    let ss = 0;
    for (let i = s; i < e; i++) ss += buf[i] * buf[i];
    rms[b] = ss / (e - s);
  }
  // A block's RMS can be nonzero yet still round-trip to zero peak (e.g. a
  // single near-epsilon sample), so require an actual nonzero sample in the
  // block, not just rms>0.
  const nonzero = [];
  for (let b = 0; b < nblocks; b++) {
    const s = b * blockN, e = Math.min(s + blockN, buf.length);
    let hasNZ = false;
    for (let i = s; i < e && !hasNZ; i++) if (buf[i] !== 0) hasNZ = true;
    if (hasNZ) nonzero.push(b);
  }
  const silent = nonzero.length === 0;
  // The block's *start* (not its center): windowAt's 2s window is much
  // wider than a 0.5s block, so starting the window exactly at the block
  // start guarantees the whole block (and its nonzero sample) falls inside
  // the window, rather than risking the window's start point landing past
  // the block's only nonzero sample.
  const blockFrac = (b) => Math.min(1, (b * blockN) / buf.length);
  let maxBlock = 0, maxRms = -1;
  for (const b of nonzero) if (rms[b] > maxRms) { maxRms = rms[b]; maxBlock = b; }
  const q = (p) => nonzero[Math.floor(p * (nonzero.length - 1))];
  const win50 = windowAt(buf, silent ? 0.5 : blockFrac(maxBlock));
  const win25 = windowAt(buf, silent ? 0.25 : blockFrac(q(0.25)));
  const win75 = windowAt(buf, silent ? 0.75 : blockFrac(q(0.75)));
  return { silent, win25, win50, win75 };
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
  const { silent, win25, win50, win75 } = energyWindows(buf);
  return {
    len: buf.length,
    silent,
    win25,
    win50,
    win75,
    stride: STRIDE,
    strided: strided(buf),
    checksum: checksum(buf),
  };
}

// tests/formtest.js's 12-bar-blues literal (title/note/key/mode/meter/tempo/
// guitar/band/sections), copied verbatim; voice is overridden to 'alto' at
// the renderSong call (voiceKey), not on the song object, matching the
// task's "seed 7, voice 'alto'".
const L = (syl, ph, ch) => ({ syl, ph, chords: ch });
const BLUES = {
  title: 'Rent Day Blues', note: '', key: 'E', mode: 'mixolydian', meter: '4/4', tempo: 84,
  guitar: 'travis', voice: 'baritone',
  band: { drums: 'brushes', bass: true, harmonyGuitar: true, harp: false, violin: false, choir: false, harmonies: false, doubles: false },
  sections: [
    { type: 'intro', chords: ['E7', 'A7', 'E7', 'B7'] },
    {
      type: 'verse', lines: [
        L('the *land-lord *knocks at *half past *eight', 'dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t', ['E7', 'E7', 'E7', 'E7']),
        L('the *land-lord *knocks at *half past *eight', 'dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t', ['A7', 'A7', 'E7', 'E7']),
        L('I *told him *twice the *check is *late', 'ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t', ['B7', 'A7', 'E7', 'B7'])]
    },
    {
      type: 'verse', lines: [
        L('my *coat is *thin, my *boots are *worn', 'm ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n', ['E7', 'E7', 'E7', 'E7']),
        L('my *coat is *thin, my *boots are *worn', 'm ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n', ['A7', 'A7', 'E7', 'E7']),
        L('but I *sing so *loud the *roof gets *torn', 'b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n', ['B7', 'A7', 'E7', 'B7'])]
    },
    { type: 'interlude', chords: ['E7', 'A7', 'E7', 'E7', 'B7', 'A7', 'E7', 'B7'] },
    { type: 'outro', chords: ['E7', 'A7', 'E7', 'E7'] }]
};

const noop = () => {};

async function runCase(name, rawSong, seed, voiceKey, styleKey) {
  const song = normalizeSong(rawSong);
  // Real applyStyle (src/styles.js), same call the app makes, so
  // song.breakLead/song.style/song.band/song.tempo are set exactly as the
  // page would set them before rendering.
  if (styleKey) applyStyle(song, styleKey);
  const t0 = Date.now();
  const r = await renderSong(song, seed, voiceKey, noop);
  const renderMs = Date.now() - t0;

  // Dump the raw tracks before mixSong: processTrack (called from mixSong)
  // takes each track out of r.tracks (sets it null) as it processes it, so
  // this must run first or every track would read back empty.
  const tracks = {};
  for (const T of TRACKS) {
    const c = r.tracks[T.key];
    tracks[T.key] = c ? c.map(dumpChannel) : null;
  }

  const t1 = Date.now();
  const m = await mixSong(r, T => true, seed);
  const mixMs = Date.now() - t1;

  return {
    name, seed, len: r.len,
    renderMs, mixMs,
    tracks,
    mix: { L: dumpChannel(m.L), R: dumpChannel(m.R) },
  };
}

(async () => {
  const out = {};
  out.demo = await runCase('demo', DEMO_SONG, 1234, 'auto');
  console.log('demo: render', out.demo.renderMs, 'ms, mix', out.demo.mixMs, 'ms, len', out.demo.len);
  out.blues = await runCase('blues', BLUES, 7, 'alto');
  console.log('blues: render', out.blues.renderMs, 'ms, mix', out.blues.mixMs, 'ms, len', out.blues.len);

  // style 'blues' has lead:'guitar' (STYLES.blues.lead), meter '4/4' only
  // (matching BLUES's own meter), so applyStyle's tempo clamp also fires.
  out.blues_guitar_lead = await runCase('blues_guitar_lead', BLUES, 7, 'alto', 'blues');
  console.log(
    'blues_guitar_lead: render', out.blues_guitar_lead.renderMs, 'ms, mix', out.blues_guitar_lead.mixMs,
    'ms, len', out.blues_guitar_lead.len
  );

  // style 'oldtime' has lead:'violin', meter '4/4' only, and turns the
  // violin band track on (STYLES.oldtime.band), so this exercises both the
  // opposite breakLead branch and a violin track that BLUES's own band
  // leaves off.
  out.blues_violin_lead = await runCase('blues_violin_lead', BLUES, 7, 'alto', 'oldtime');
  console.log(
    'blues_violin_lead: render', out.blues_violin_lead.renderMs, 'ms, mix', out.blues_violin_lead.mixMs,
    'ms, len', out.blues_violin_lead.len
  );

  fs.writeFileSync(path.join(OUT_DIR, 'song.json'), JSON.stringify(out));
  console.log('wrote ref/parity/song.json');
})();

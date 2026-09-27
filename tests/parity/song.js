// Full-song reference dump for the engine crate's render_song/mix: the real
// (async) renderSong and mixSong, called exactly as the app calls them, not
// a hand copy of their bodies (song_vocals.js copies renderSong's vocal
// slice verbatim for a from-scratch module check; this instead exercises
// the whole pipeline, including the band tracks and the body-convolution
// loop, so it also covers what song_vocals.js cannot).
//
// Two cases:
//  - "demo": DEMO_SONG (src/demo.js), seed 1234, voice 'auto', mixSong with
//    every track enabled.
//  - "blues": the 12-bar-blues literal from tests/formtest.js, seed 7,
//    voice 'alto' (no style applied, so song.breakLead stays unset/'both'
//    as un-styled renderSong always sees it), mixSong with every track
//    enabled.
//
// Each raw track's channel(s) (renderSong's tracks[key], i.e. post body
// convolution, pre processTrack/mixSong) and the final mix L/R are dumped
// as: three exact 2s windows at 25%/50%/75% of the buffer, a strided
// sample (every 97th sample) over the whole buffer, and sum/sumsq over the
// whole buffer computed in f64 in index order. Writes ref/parity/song.json.
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

async function runCase(name, rawSong, seed, voiceKey) {
  const song = normalizeSong(rawSong);
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

  fs.writeFileSync(path.join(OUT_DIR, 'song.json'), JSON.stringify(out));
  console.log('wrote ref/parity/song.json');
})();

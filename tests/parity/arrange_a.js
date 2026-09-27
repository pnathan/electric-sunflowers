// Reference dump for the arrange crate's guitar/bass/harp port: genGuitar,
// genBass, genHarp and guitarVoicing.
//
// Writes ref/parity/arrange_a/<case>.bin (float32 little-endian) plus
// ref/parity/arrange_a/index.json describing every case.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));

const OUT = path.join(__dirname, '../../ref/parity/arrange_a');
fs.mkdirSync(OUT, { recursive: true });
const index = { cases: [] };

// Cap how much of a long buffer is dumped contiguously; the rest of its
// duration is covered by a strided sample (every STRIDE-th sample, a prime
// so it does not alias against any of this engine's periodic structure --
// bars, beats, pluck periods), which is cheap to store yet still catches a
// divergence anywhere in the buffer, not just the first 20 s.
const DUMP_SECONDS = 20;
const STRIDE = 97;

function writeBuf(name, arr) {
  const n = Math.min(arr.length, Math.round(DUMP_SECONDS * SR));
  const head = Float32Array.from(arr.slice(0, n));
  fs.writeFileSync(path.join(OUT, name + '.bin'), Buffer.from(head.buffer, head.byteOffset, head.byteLength));
  const strided = new Float32Array(Math.ceil(arr.length / STRIDE));
  for (let i = 0, j = 0; i < arr.length; i += STRIDE, j++) strided[j] = arr[i];
  fs.writeFileSync(path.join(OUT, name + '_stride.bin'), Buffer.from(strided.buffer, strided.byteOffset, strided.byteLength));
  return { dumped_len: n, full_len: arr.length, stride: STRIDE, strided_len: strided.length };
}

const L = (syl, ph, ch) => ({ syl, ph, chords: ch });
const BLUES_SONG = {
  title: 'Rent Day Blues', note: '', key: 'E', mode: 'mixolydian', meter: '4/4', tempo: 84, guitar: 'travis', voice: 'baritone',
  band: { drums: 'brushes', bass: true, harmonyGuitar: true, harp: false, violin: false, choir: false, harmonies: false, doubles: false },
  sections: [{ type: 'intro', chords: ['E7', 'A7', 'E7', 'B7'] },
    { type: 'verse', lines: [L('the *land-lord *knocks at *half past *eight', 'dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t', ['E7', 'E7', 'E7', 'E7']),
      L('the *land-lord *knocks at *half past *eight', 'dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t', ['A7', 'A7', 'E7', 'E7']),
      L('I *told him *twice the *check is *late', 'ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t', ['B7', 'A7', 'E7', 'B7'])] },
    { type: 'verse', lines: [L('my *coat is *thin, my *boots are *worn', 'm ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n', ['E7', 'E7', 'E7', 'E7']),
      L('my *coat is *thin, my *boots are *worn', 'm ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n', ['A7', 'A7', 'E7', 'E7']),
      L('but I *sing so *loud the *roof gets *torn', 'b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n', ['B7', 'A7', 'E7', 'B7'])] },
    { type: 'interlude', chords: ['E7', 'A7', 'E7', 'E7', 'B7', 'A7', 'E7', 'B7'] },
    { type: 'outro', chords: ['E7', 'A7', 'E7', 'E7'] }]
};

const SEED = 1234;

function dumpSong(name, rawSong) {
  const s = normalizeSong(JSON.parse(JSON.stringify(rawSong)));
  const P = prepare(s, SEED, 'auto');
  const g = genGuitar(s, P.form, P.tl, SEED);
  const b = genBass(s, P.form, P.tl, SEED);
  const h = genHarp(s, P.form, P.tl, SEED);
  index.cases.push({
    name,
    guitar: name + '_guitar',
    bass: name + '_bass',
    harp: name + '_harp',
    guitar_info: writeBuf(name + '_guitar', g),
    bass_info: writeBuf(name + '_bass', b),
    harp_info: writeBuf(name + '_harp', h),
  });
}

// Dump the raw song literals as JSON too, so the Rust test builds its
// normalize_song input from the same data instead of a hand-transcribed copy.
fs.writeFileSync(path.join(OUT, 'demo_song.json'), JSON.stringify(DEMO_SONG, null, 1));
fs.writeFileSync(path.join(OUT, 'blues_song.json'), JSON.stringify(BLUES_SONG, null, 1));

dumpSong('demo_auto', DEMO_SONG);

for (const style of ['strum', 'fingerpick', 'travis', 'arpeggio']) {
  const raw = JSON.parse(JSON.stringify(DEMO_SONG));
  raw.guitar = style;
  dumpSong('demo_' + style, raw);
}

dumpSong('blues', BLUES_SONG);

// guitarVoicing over a list of chord names.
const CHORDS = [
  'C', 'D', 'E', 'F', 'G', 'A', 'B', 'Cm', 'Dm', 'Em', 'Fm', 'Gm', 'Am', 'Bm',
  'C7', 'D7', 'E7', 'F7', 'G7', 'A7', 'B7', 'Cmaj7', 'Dmaj7', 'Em7', 'Fmaj7',
  'Gmaj7', 'Am7', 'Bm7b5', 'Csus4', 'Dsus2', 'Cdim', 'Caug', 'C/E', 'D/F#',
  'G/B', 'Am/C', 'F/C', 'C6', 'Am6', 'C9',
];
{
  const buf = new Float32Array(CHORDS.length * 6);
  for (let i = 0; i < CHORDS.length; i++) {
    const ch = parseChord(CHORDS[i]);
    const v = guitarVoicing(ch);
    for (let s = 0; s < 6; s++) buf[i * 6 + s] = v[s] === null ? -1 : v[s];
  }
  fs.writeFileSync(path.join(OUT, 'guitar_voicing.bin'), Buffer.from(buf.buffer, buf.byteOffset, buf.byteLength));
  index.cases.push({ name: 'guitar_voicing', chords: CHORDS, layout: 'one row of 6 f32 per chord, in order; null fret is -1' });
}

fs.writeFileSync(path.join(OUT, 'index.json'), JSON.stringify(index, null, 1));
console.log('wrote', OUT);

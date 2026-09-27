// Reference dump for the arrange crate's "part B": counterLine, fillsFor,
// choirVoicings and genDrums, exercised over DEMO_SONG (seed 1234, voice
// 'auto') and the 12-bar blues song from tests/formtest.js. Writes
// ref/parity/arrange_b.json.
//
// counterLine/fillsFor are dumped with exactly the arguments renderSong
// passes (engine.js ~919-928): the two violin calls (chorus counter-line,
// bridge counter-line), the violin's verse fill, and the harmony guitar's
// verse fill. choirVoicings is dumped with renderSong's choir filter.
// genDrums is dumped once per drums style ('none','brushes','soft','full')
// by cloning the song with that style substituted.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));

const OUT_DIR = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(OUT_DIR, { recursive: true });

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

// 6/8, rising intensity (verse occ0 -> verse occ1), same song as
// tests/parity/arrange_a.js's SIXEIGHT_SONG: exercises genDrums's 6/8 branch
// and its sub===3 fill (steps=3), which the 4/4 and 3/4 cases above never
// reach.
const SIXEIGHT_SONG = {
  title: 'Ferry Crossing', note: '', key: 'D', mode: 'major', meter: '6/8', tempo: 72, guitar: 'strum', voice: 'tenor',
  band: { drums: 'full', bass: true, harmonyGuitar: true, harp: false, violin: false, choir: false, harmonies: false, doubles: false },
  sections: [{ type: 'intro', chords: ['D', 'G', 'D', 'A'] },
    { type: 'verse', lines: [L('the *fer-ry *leaves at *dawn', 'dh ax|f eh|r iy|l iy v z|ae t|d aa n', ['D', 'G']),
      L('the *gulls are *call-ing *loud', 'dh ax|g ah l z|aa r|k ao|l ih ng|l aw d', ['D', 'A'])] },
    { type: 'verse', lines: [L('we *cross the *bay at *dawn', 'w iy|k r ao s|dh ax|b ey|ae t|d aa n', ['D', 'G']),
      L('the *bell rings *out so *loud', 'dh ax|b eh l|r ih ng z|aw t|s ow|l aw d', ['D', 'A'])] },
    { type: 'outro', chords: ['D', 'G', 'D', 'D'] }]
};

function dumpNotes(notes) {
  return notes.map(n => ({ t0: n.t0, t1: n.t1, m: n.m, v: n.v }));
}

function dumpVoicings(vs, tl) {
  return vs.map(({ sg, v }) => ({ segIdx: tl.segs.indexOf(sg), v: v.slice() }));
}

// checksum: order-sensitive so a transposed/misaligned buffer still fails.
function checksum(buf) {
  let sum = 0, sumsq = 0, weighted = 0;
  for (let i = 0; i < buf.length; i++) {
    const v = buf[i];
    sum += v;
    sumsq += v * v;
    weighted += v * ((i % 97) + 1);
  }
  return { len: buf.length, sum, sumsq, weighted };
}

const WIN_N = 2 * 44100; // 2s window, exact

// The exact 2s head of every drum buffer is silence (drums do not start on
// beat 0's very first sample in any of these songs), so dumping the head
// alone never actually compares a played sample. Instead dump one window
// starting at the first nonzero sample (a bar with intensity>=1 starts
// playing here) and one later window (the tail of the buffer), so both ends
// of the render are checked sample-for-sample.
function firstNonZero(buf) {
  for (let i = 0; i < buf.length; i++) if (buf[i] !== 0) return i;
  return 0;
}

function window(buf, start) {
  const len = Math.min(WIN_N, buf.length);
  const s = Math.max(0, Math.min(start, buf.length - len));
  return { start: s, data: Array.from(buf.slice(s, s + len)) };
}

function dumpBuf(buf) {
  return {
    win1: window(buf, firstNonZero(buf)),
    win2: window(buf, buf.length - WIN_N),
    full: checksum(buf),
  };
}

function dumpSong(name, songRaw, seed) {
  const song = normalizeSong(songRaw);
  const P0 = prepare(song, seed, 'auto');
  const { form, tl, comp, tonic } = P0;
  const lead = comp.lead;
  const leadT = lead.map(n => ({ t0: n.t0, t1: n.t1, midi: n.midi }));

  const ctr = counterLine(form, tl, leadT, 67, 86, s => s.lift, seed, false);
  const br = counterLine(form, tl, leadT, 62, 79, s => s.type === 'bridge', seed, true);
  const fl = fillsFor(form, tl, lead, 69, 88, s => s.type === 'verse' && s.occ > 0, song, seed);
  const flG = fillsFor(form, tl, lead, 59, 79, s => s.type === 'verse', song, seed + 1);

  const choirFilt = s => (s.lift && s.liftIdx > 0) || s.type === 'bridge' || (s.type === 'outro');
  const vs = choirVoicings(form, tl, choirFilt);

  const drums = {};
  for (const style of ['none', 'brushes', 'soft', 'full']) {
    const s2 = JSON.parse(JSON.stringify(songRaw));
    s2.band = Object.assign({}, s2.band, { drums: style });
    const song2 = normalizeSong(s2);
    // reuse the same P0's form/tl: genDrums only reads song.band.drums plus
    // form/tl, which do not depend on the drums style.
    const [Ld, Rd] = genDrums(song2, form, tl, seed);
    drums[style] = { L: dumpBuf(Ld), R: dumpBuf(Rd) };
  }

  return {
    tonic,
    ctr: dumpNotes(ctr),
    br: dumpNotes(br),
    fl: dumpNotes(fl),
    flG: dumpNotes(flG),
    choirVoicings: dumpVoicings(vs, tl),
    drums,
  };
}

const out = {
  demo: dumpSong('demo', DEMO_SONG, 1234),
  blues: dumpSong('blues', BLUES_SONG, 1234),
  sixeight: dumpSong('sixeight', SIXEIGHT_SONG, 1234),
};

fs.writeFileSync(path.join(OUT_DIR, 'arrange_b.json'), JSON.stringify(out));
console.log('wrote ref/parity/arrange_b.json');

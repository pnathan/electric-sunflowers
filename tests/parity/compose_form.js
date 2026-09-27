// Reference dump for the compose crate's form/timeline/rhythm/pitch port:
// buildForm, buildTimeline, placeRhythm and pitchLine, exercised through
// normalizeSong + composeMelody on DEMO_SONG. Writes
// ref/parity/compose_form.json.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));

const OUT_DIR = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(OUT_DIR, { recursive: true });

const song = normalizeSong(DEMO_SONG);

function dumpSections(form) {
  return form.sections.map(s => ({
    type: s.type, occ: s.occ, idx: s.idx, startBar: s.startBar, nBars: s.nBars,
    lift: s.lift, liftIdx: s.liftIdx, final: s.final, intensity: s.intensity,
    nLines: s.lines.length,
  }));
}
function dumpBars(form) {
  return form.bars.map(b => ({
    chords: b.chords.map(c => c.name),
    sec: b.sec.idx,
    line: b.line ? form.lines.indexOf(b.line) : -1,
  }));
}
function dumpLines(form) {
  return form.lines.map(L => ({
    sec: L.sec.idx, li: L.li, startBar: L.startBar, nBars: L.nBars, text: L.text,
    nSyls: L.syls.length,
  }));
}
function dumpForm(transpose) {
  const form = buildForm(song, transpose);
  return { mi: form.mi, stretch: form.stretch, flats: form.flats,
    sections: dumpSections(form), bars: dumpBars(form), lines: dumpLines(form) };
}

const out = { forms: {}, timeline: null, lines: [] };
out.forms['0'] = dumpForm(0);
out.forms['3'] = dumpForm(3);

// Timeline at transpose 0
const form0 = buildForm(song, 0);
const tl0 = buildTimeline(form0, song.tempo);
const chordSamples = [];
for (let b = 0; b <= tl0.nb + 4; b += 0.5) {
  const c = tl0.chordAt(b);
  chordSamples.push({ beat: b, chord: c.name });
}
out.timeline = {
  T: Array.from(tl0.T),
  nb: tl0.nb,
  base: tl0.base,
  end: tl0.end,
  segs: tl0.segs.map(s => ({ chord: s.chord.name, b0: s.b0, b1: s.b1, sec: s.sec.idx, bar: s.bar })),
  chordSamples,
};

// Capture placeRhythm / pitchLine args+results as composeMelody makes them,
// for the first verse and first chorus (the first 8 lyric lines in form
// order: composeMelody's line loop runs before its instrumental-lead loop).
const origPlaceRhythm = placeRhythm;
const origPitchLine = pitchLine;
const captured = [];
let callIdx = 0;
placeRhythm = function (syls, nBars, mi, rng, pr) {
  const res = origPlaceRhythm(syls, nBars, mi, rng, pr);
  captured.push({ kind: 'rhythm', idx: callIdx++, args: {
    stresses: syls.map(s => !!s.stress), nBars, meter: mi,
    pr: pr ? { dot: pr.dot, even: pr.even, sync: pr.sync, rnoise: pr.rnoise } : null,
  }, res: { onsets: res.onsets, durs: res.durs, weights: res.weights, lineBeats: res.lineBeats } });
  return res;
};
pitchLine = function (o) {
  const res = origPitchLine(o);
  const shapeVals = o.onsets.map(b => o.shape(Math.max(0, Math.min(1, b / o.lineBeats))));
  captured.push({ kind: 'pitch', idx: callIdx++, args: {
    n: o.n, onsets: o.onsets, durs: o.durs, weights: o.weights,
    chordPcs: o.chords.map(c => c.pcs), scales: o.scales,
    T: o.T, tonic: o.tonic, center: o.center, shapeVals,
    cadence: o.cadence, ref: o.ref || null, prevEnd: o.prevEnd === undefined ? null : o.prevEnd,
    lineBeats: o.lineBeats,
    prof: o.prof ? { leap: o.prof.leap, rep: o.prof.rep, noise: o.prof.noise } : null,
    hook: o.hook || 0,
  }, res });
  return res;
};

const seed = 42;
const tl0b = buildTimeline(form0, song.tempo);
composeMelody(song, form0, tl0b, seed);
placeRhythm = origPlaceRhythm;
pitchLine = origPitchLine;

// first 8 lyric-line calls = 4 verse-1 lines + 4 chorus-1 lines, each a
// (rhythm, pitch) pair in order.
out.lines = captured.slice(0, 16);

fs.writeFileSync(path.join(OUT_DIR, 'compose_form.json'), JSON.stringify(out));
console.log('wrote', path.join(OUT_DIR, 'compose_form.json'));

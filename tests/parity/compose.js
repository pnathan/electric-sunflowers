// Reference dump for the compose crate's end-to-end prepare parity: prepare,
// vocalNotes and harmonyLine, exercised over DEMO_SONG, the blues song from
// tests/formtest.js, and a set of malformed normalizeSong inputs. Writes
// ref/parity/compose.json.
'use strict';
const fs = require('fs');
const path = require('path');
require(path.join(__dirname, '../lib.js'));
const { DEMO_SONG } = require(path.join(__dirname, '../../src/demo.js'));

const OUT_DIR = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(OUT_DIR, { recursive: true });

// The 12-bar blues song literal, copied verbatim from tests/formtest.js.
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

// Malformed / edge-case normalizeSong inputs.
const MALFORMED = {
  missing_ph: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [{ type: 'verse', lines: [{ syl: 'the *quick *brown *fox jumps', chords: 'C G' }] }],
  },
  wrong_ph_group_count: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [{ type: 'verse', lines: [{ syl: 'the *quick *brown *fox', ph: 'dh ax|k w ih k', chords: 'C G' }] }],
  },
  no_stress_marks: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [{ type: 'verse', lines: [{ syl: 'the quick brown fox jumps over the lazy dog', ph: 'dh ax|k w ih k|b r aw n|f aa k s|jh ah m p s|ow v er|dh ax|l ey z iy|d ao g', chords: 'C G' }] }],
  },
  unknown_chord_qualities: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [
      { type: 'intro', chords: ['C7#9', 'Dm(add9)'] },
      { type: 'verse', lines: [{ syl: 'strange *chords a*bove', ph: 's t r ey n jh|k ao r d z|ax|b ah v', chords: ['C7#9', 'Dm(add9)'] }] },
    ],
  },
  slash_chords: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [{ type: 'verse', lines: [{ syl: 'walk *down the *bass line *here', ph: 'w aa k|d aw n|dh ax|b ey s|l ay n|hh ih r', chords: ['C/E', 'F/G', 'G/B', 'C'] }] }],
  },
  same_section: {
    key: 'C', mode: 'major', meter: '4/4', tempo: 100,
    sections: [
      { type: 'chorus', lines: [{ syl: '*sing it *loud and *clear', ph: 's ih ng|ih t|l aw d|ae n d|k l ih r', chords: 'C G' }] },
      { type: 'verse', lines: [{ syl: 'one *two *three *four', ph: 'w ah n|t uw|th r iy|f ao r', chords: 'C G' }] },
      { type: 'chorus', same: true },
    ],
  },
  six_eight_meter: {
    key: 'D', mode: 'major', meter: '6/8', tempo: 90,
    sections: [{ type: 'verse', lines: [{ syl: '*row row row your *boat', ph: 'r ow|r ow|r ow|y or|b ow t', chords: 'D A D' }] }],
  },
};

function dumpSections(form) {
  return form.sections.map(s => ({
    type: s.type, occ: s.occ, idx: s.idx, startBar: s.startBar, nBars: s.nBars,
    lift: s.lift, liftIdx: s.liftIdx, final: s.final, intensity: s.intensity,
  }));
}
function dumpBars(form) {
  return form.bars.map(b => ({
    chords: b.chords.map(c => c.name), sec: b.sec.idx,
    line: b.line ? form.lines.indexOf(b.line) : -1,
  }));
}
function dumpLeadNote(n, form) {
  return {
    beat: n.beat, dur: n.dur, midi: n.midi, grace: n.grace === null || n.grace === undefined ? null : n.grace,
    t0: n.t0, t1: n.t1, phraseStart: n.phraseStart, phraseEnd: n.phraseEnd, stress: n.stress,
    sec: n.sec.idx,
  };
}
function dumpInstNote(n, form) {
  return { beat: n.beat, dur: n.dur, midi: n.midi, sec: n.sec.idx };
}
function dumpLine(L) {
  return {
    sec: L.sec.idx, li: L.li, onsets: Array.from(L.rh.onsets), durs: Array.from(L.rh.durs),
    weights: Array.from(L.rh.weights), pitches: L.pitches.slice(),
  };
}
function dumpPrepared(song, seed, voiceKey) {
  const p = prepare(song, seed, voiceKey);
  const { form, tl, comp, voice, keyShift, tonic } = p;
  const vn = vocalNotes(comp.lead, 1.0);
  const hl = harmonyLine(comp.lead, tl, song, tonic, true);
  return {
    voice, keyShift, tonic,
    sections: dumpSections(form),
    bars: dumpBars(form),
    lead: comp.lead.map(n => dumpLeadNote(n, form)),
    inst: comp.inst.map(n => dumpInstNote(n, form)),
    lines: form.lines.map(dumpLine),
    vocalNotes: vn.map(v => ({
      t0: v.t0, t1: v.t1, midi: v.midi, ph: v.ph, amp: v.amp,
      phraseStart: v.phraseStart, phraseEnd: v.phraseEnd,
      grace: v.grace === null || v.grace === undefined ? null : v.grace, stress: v.stress,
    })),
    harmonyLine: hl.map(n => ({ beat: n.beat, midi: n.midi, grace: n.grace })),
  };
}

const out = { demo: {}, blues: {}, malformed: {} };

const demoSong = normalizeSong(JSON.parse(JSON.stringify(DEMO_SONG)));
for (const seed of [1234, 1, 777777, 4294967295]) {
  out.demo['auto|' + seed] = dumpPrepared(demoSong, seed, 'auto');
}
for (const v of ['alto', 'soprano', 'tenor']) {
  out.demo[v + '|1234'] = dumpPrepared(demoSong, 1234, v);
}

const bluesSong = normalizeSong(JSON.parse(JSON.stringify(BLUES_SONG)));
out.blues['auto|1234'] = dumpPrepared(bluesSong, 1234, 'auto');

for (const [name, raw] of Object.entries(MALFORMED)) {
  try {
    const song = normalizeSong(JSON.parse(JSON.stringify(raw)));
    out.malformed[name] = { ok: true, dump: dumpPrepared(song, 1234, 'auto') };
  } catch (e) {
    out.malformed[name] = { ok: false, error: String(e && e.message || e) };
  }
}

fs.writeFileSync(path.join(OUT_DIR, 'compose.json'), JSON.stringify(out));
console.log('wrote', path.join(OUT_DIR, 'compose.json'));

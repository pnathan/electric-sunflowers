// Reference dump for the songwriter crate: styleDirection over every style key
// (plus the null/random-pick fallback), formText for every form, applyStyle's
// tempo clamp, and songPrompt over every style key crossed with a few voice
// preferences, all driven by rngFor(seed,tag) in place of Math.random so the
// Rust side can replay the identical draw sequence with sfcore::rng::rng_for.
// Writes ref/parity/songwriter.json.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');

require(path.join(__dirname, '../lib.js')); // loads src/engine.js: clamp, rngFor, hashStr, mulberry32
vm.runInThisContext(fs.readFileSync(path.join(__dirname, '../../src/styles.js'), 'utf8'), { filename: 'styles.js' });
vm.runInThisContext(fs.readFileSync(path.join(__dirname, '../../src/prompt.js'), 'utf8'), { filename: 'prompt.js' });

const OUT_DIR = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(OUT_DIR, { recursive: true });

// Fixed clock: stub Date so `new Date().getFullYear()` in songPrompt is deterministic.
const FIXED_YEAR = 2031;
const RealDate = Date;
function FakeDate(...args) {
  if (args.length === 0) return new RealDate(FIXED_YEAR, 0, 1);
  return new RealDate(...args);
}
FakeDate.now = () => new RealDate(FIXED_YEAR, 0, 1).getTime();
global.Date = FakeDate;

// Math.random is replaced per-call below with rngFor(seed, tag)'s generator, so
// both sides draw from the same named stream instead of relying on JS's
// non-deterministic Math.random.
const REAL_RANDOM = Math.random;
function withRng(seed, tag, fn) {
  const r = rngFor(seed, tag);
  Math.random = r;
  try {
    return fn();
  } finally {
    Math.random = REAL_RANDOM;
  }
}

const styleKeys = Object.keys(STYLES);
const formKeys = Object.keys(FORMS);
const SEEDS = [1, 2, 3, 4, 5];

const out = {
  fixedYear: FIXED_YEAR,
  styleKeys,
  formKeys,
  styleDirections: {},
  nullStyleDirections: [],
  formTexts: {},
  applyStyleClamps: [],
  songPrompts: {},
};

// styleDirection for every style key, over several seeds (each seed drives the
// meter/form/mode/world picks; the key is fixed so no fallback pick is drawn).
for (const key of styleKeys) {
  out.styleDirections[key] = SEEDS.map((seed) =>
    withRng(seed, `styleDirection|${key}`, () => styleDirection(key))
  );
}

// styleDirection(null): exercises the extra fallback-pick draw for the style key
// itself, over several seeds.
out.nullStyleDirections = SEEDS.map((seed) => withRng(seed, 'styleDirection|null', () => styleDirection(null)));

// formText for every form key.
for (const fk of formKeys) {
  out.formTexts[fk] = formText(fk);
}

// applyStyle clamps: for every style, every meter it declares, and a few
// out-of-range tempos (below, above, and inside the declared range), the
// clamped tempo applyStyle produces.
for (const key of styleKeys) {
  const S = STYLES[key];
  for (const meter of S.meters) {
    const [lo, hi] = S.tempo[meter];
    const candidates = [1, lo, Math.round((lo + hi) / 2), hi, 999];
    for (const tempoIn of candidates) {
      const song = { meter, tempo: tempoIn, band: {} };
      const result = applyStyle(song, key);
      out.applyStyleClamps.push({
        key,
        meter,
        tempoIn,
        tempoOut: result.tempo,
        guitar: result.guitar,
        breakLead: result.breakLead,
        band: result.band,
        style: result.style,
      });
    }
  }
}
// applyStyle with an unknown key: JS returns the song unchanged.
{
  const song = { meter: '4/4', tempo: 123, band: {} };
  const result = applyStyle(song, 'not-a-style');
  out.applyStyleClamps.push({ key: 'not-a-style', meter: '4/4', tempoIn: 123, tempoOut: result.tempo, unchanged: true });
}

// songPrompt for voice in {undefined, 'auto', 'alto'} for every style, with a
// fixed year and a fixed dir (styleDirection(key) drawn from one seeded stream)
// so the whole prompt text, including the reg/world/form text, is reproducible.
const VOICE_CASES = [
  { label: 'undefined', voice: undefined },
  { label: 'auto', voice: 'auto' },
  { label: 'alto', voice: 'alto' },
];
for (const key of styleKeys) {
  out.songPrompts[key] = {};
  for (const vc of VOICE_CASES) {
    const text = withRng(7, `songPrompt|${key}|${vc.label}`, () => {
      const dir = styleDirection(key);
      return songPrompt(`a song about ${key}`, vc.voice, dir);
    });
    out.songPrompts[key][vc.label] = text;
  }
}

global.Date = RealDate;

fs.writeFileSync(path.join(OUT_DIR, 'songwriter.json'), JSON.stringify(out));
console.log('wrote ref/parity/songwriter.json');

// Reference dump for sfcore::v8math parity: wide-range random inputs plus
// hard cases (tiny, huge, near multiples of pi/2, near 1 for log, negative/
// zero/inf/nan) for every transcendental Math function src/engine.js uses
// that is not already known bit-exact via libm/std (sin, cos, log, log2,
// log10) plus pow and atan2 for completeness.
//
// Binary format written to ref/parity/math.bin:
//   for each function in FUNCS, in order:
//     u32le count
//     count records, each `arity` f64 (LE): unary = [x, y]; binary = [a, b, y]
'use strict';
const fs = require('fs');
const path = require('path');

function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rnd = mulberry32(0xC0FFEE);

function randRange(lo, hi) {
  return lo + rnd() * (hi - lo);
}

const N = 200000;

function buildUnaryInputs(hardCases, ranges) {
  const xs = [];
  for (const hc of hardCases) xs.push(hc);
  while (xs.length < N) {
    const [lo, hi] = ranges[Math.floor(rnd() * ranges.length)];
    xs.push(randRange(lo, hi));
  }
  return xs;
}

const HALF_PI = Math.PI / 2;

const sinCosHard = [
  0, -0, 1e-300, -1e-300, 1e-30, 1e300, -1e300, Infinity, -Infinity, NaN,
  Math.PI, -Math.PI, HALF_PI, -HALF_PI, 2 * Math.PI, 1000 * HALF_PI,
  1e6 * Math.PI, 1e10, -1e10, 1e15, 1e16, Number.MAX_VALUE, Number.MIN_VALUE,
  Number.EPSILON,
];
const sinCosRanges = [
  [-1, 1], [-Math.PI, Math.PI], [-100, 100], [-1e6, 1e6], [-1e15, 1e15],
  [-1e300, 1e300],
];

const logHard = [
  0, -0, -1, 1, 2, Infinity, -Infinity, NaN, Number.MIN_VALUE,
  Number.MAX_VALUE, 1 + Number.EPSILON, 1 - Number.EPSILON / 2, 2.220446049250313e-16,
];
const logRanges = [
  [1e-320, 1], [0.9, 1.1], [1, 2], [1e-10, 1e10], [1e-300, 1e300],
];

const powHard = [
  [0, 0], [0, 1], [1, Infinity], [-1, Infinity], [2, 1024], [2, -1024],
  [Infinity, 0], [NaN, 0], [-8, 1 / 3], [0.5, 0.5], [-2, 3], [-2, 2.5],
];

const atan2Hard = [
  [0, 0], [0, -0], [-0, 0], [1, 0], [0, 1], [-1, 0], [0, -1], [Infinity, Infinity],
  [-Infinity, Infinity], [NaN, 1], [1, NaN],
];

function dumpUnary(buf, fn, xs) {
  const header = Buffer.alloc(4);
  header.writeUInt32LE(xs.length, 0);
  buf.push(header);
  for (const x of xs) {
    const rec = Buffer.alloc(16);
    rec.writeDoubleLE(x, 0);
    rec.writeDoubleLE(fn(x), 8);
    buf.push(rec);
  }
}

function dumpBinary(buf, fn, hard, ranges) {
  const pairs = [];
  for (const h of hard) pairs.push(h);
  while (pairs.length < N) {
    const [ar, br] = ranges[Math.floor(rnd() * ranges.length)];
    pairs.push([randRange(ar[0], ar[1]), randRange(br[0], br[1])]);
  }
  const header = Buffer.alloc(4);
  header.writeUInt32LE(pairs.length, 0);
  buf.push(header);
  for (const [a, b] of pairs) {
    const rec = Buffer.alloc(24);
    rec.writeDoubleLE(a, 0);
    rec.writeDoubleLE(b, 8);
    rec.writeDoubleLE(fn(a, b), 16);
    buf.push(rec);
  }
}

const buf = [];
dumpUnary(buf, Math.sin, buildUnaryInputs(sinCosHard, sinCosRanges));
dumpUnary(buf, Math.cos, buildUnaryInputs(sinCosHard, sinCosRanges));
dumpUnary(buf, Math.log, buildUnaryInputs(logHard, logRanges));
dumpUnary(buf, Math.log2, buildUnaryInputs(logHard, logRanges));
dumpUnary(buf, Math.log10, buildUnaryInputs(logHard, logRanges));
dumpBinary(buf, Math.pow, powHard, [[[0.001, 100], [-10, 10]], [[1e-10, 1e10], [-2, 2]]]);
dumpBinary(buf, Math.atan2, atan2Hard, [[[-100, 100], [-100, 100]], [[-1e10, 1e10], [-1e10, 1e10]]]);

const outDir = path.join(__dirname, '../../ref/parity');
fs.mkdirSync(outDir, { recursive: true });
fs.writeFileSync(path.join(outDir, 'math.bin'), Buffer.concat(buf));
console.log('wrote ref/parity/math.bin');

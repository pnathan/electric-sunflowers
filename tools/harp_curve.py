#!/usr/bin/env python3
"""Apply the recorded-harp correction to the harp body curve.

usage: harp_curve.py CORRECTION.npy ALPHA [--clamp DB] > body.rs.patched

CORRECTION.npy is `harp_compare.py --write-curve` output. Reads the harp row
of BODY_CURVES from crates/instruments/src/body.rs (or the row saved in
harp_curve_orig.txt beside CORRECTION.npy, so the correction is never applied twice) and
writes body.rs with row = old + ALPHA * correction:
- the correction is relative, so its level is set to zero as the mean of
  300 Hz-1.25 kHz; below 200 Hz (no recorded partials: the engine's harp
  starts at G3, 196 Hz) it fades to zero; above 9 kHz it holds;
- boosts are clamped to CLAMP dB (default 20);
- the row is renormalised to a 0 dB maximum.
"""
import os, re, sys
import numpy as np

BODY = 'crates/instruments/src/body.rs'


def main():
    corr = np.load(sys.argv[1])
    alpha = float(sys.argv[2])
    clamp = float(sys.argv[sys.argv.index('--clamp') + 1]) if '--clamp' in sys.argv else 20.0
    src = open(BODY).read()
    m = re.search(r'(    // Harp\n    \[)(.*?)(\n    \],)', src, re.S)
    orig = os.path.join(os.path.dirname(os.path.abspath(sys.argv[1])), 'harp_curve_orig.txt')
    try:
        old = [float(v) for v in open(orig).read().split()]
    except FileNotFoundError:
        old = [float(v) for v in re.findall(r'-?\d+\.\d+', m.group(2))]
        open(orig, 'w').write(' '.join('%.1f' % v for v in old))
    fs, c = corr
    assert len(old) == len(fs) == 86
    band = (fs >= 300) & (fs <= 1250)
    c = c - c[band].mean()
    fade = np.clip((fs - 140) / 60, 0, 1)  # 0 at 140 Hz, 1 from 200 Hz
    c = np.minimum(c * fade, clamp)
    new = np.array(old) + alpha * c
    new -= new.max()
    rows = ', '.join('%.1f' % v for v in new)
    src = src[:m.start(2)] + '\n        ' + rows + ',' + src[m.end(2):]
    sys.stdout.write(src)


if __name__ == '__main__':
    main()

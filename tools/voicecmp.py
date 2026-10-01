#!/usr/bin/env python3
"""Voice-quality metrics of sung audio, to compare the engine's voice with real singers.

usage: voicecmp.py ref DIR            vocadito (Zenodo 5578807, CC BY 4.0); male-range tracks only
       voicecmp.py wav FILE...        any mono or stereo WAV (for example the lead stem)

The same estimators run on both: autocorrelation f0 (25 ms frames, 10 ms hop),
then on 4096-point FFT frames of voiced material, binned by f0 so that register
does not skew the comparison:
  H1-H2    level of the fundamental minus the second harmonic, dB
  tilt     slope of the first 10 harmonics' levels against log2(k), dB/octave
  p2v      median peak-to-valley between harmonics 2-8, dB
  hnr      harmonic energy over the rest, 0-5 kHz, dB
  step     level of the 3.2 kHz third-octave band minus the 4 kHz band, dB
  vib      share of pitch-deviation power (cents from a 300 ms median) in 4-8 Hz
           against 0.5-12 Hz
and the 1/3-octave levels, dB re the total, from 100 Hz to 10 kHz.
"""
import csv, glob, os, sys
import numpy as np, soundfile as sf

BINS = [(100, 150), (150, 200), (200, 280)]
BANDS = np.array([100, 125, 160, 200, 250, 315, 400, 500, 630, 800, 1000, 1250, 1600, 2000,
                  2500, 3150, 4000, 5000, 6300, 8000, 10000])


def load(fn):
    x, sr = sf.read(fn, dtype='float64')
    return (x.mean(1) if x.ndim > 1 else x), sr


def f0_track(x, sr, fmin=70.0, fmax=450.0):
    """(times, f0, voiced) by normalised autocorrelation, 25 ms frames, 10 ms hop."""
    n, hop = int(0.040 * sr), int(0.010 * sr)
    lo, hi = int(sr / fmax), int(sr / fmin)
    t, f, v = [], [], []
    for i in range(0, len(x) - n - hi, hop):
        s = x[i:i + n] - x[i:i + n].mean()
        e = float(s @ s)
        t.append((i + n / 2) / sr)
        if e < 1e-8 * n:
            f.append(0.0); v.append(False); continue
        S = np.fft.rfft(s, 2 * n)
        ac = np.fft.irfft(S * np.conj(S))[:hi + 1]
        ac = ac / (ac[0] + 1e-12)
        k = lo + int(np.argmax(ac[lo:hi]))
        if 1 <= k < hi and ac[k] > 0.6:
            y0, y1, y2 = ac[k - 1], ac[k], ac[k + 1]
            d = y0 - 2 * y1 + y2
            k = k + (0.5 * (y0 - y2) / d if d != 0 else 0)
            ff = sr / k
            if 60 <= ff <= 500:
                f.append(ff); v.append(True)
            else:
                f.append(0.0); v.append(False)
        else:
            f.append(0.0); v.append(False)
    return np.array(t), np.array(f), np.array(v)


def vib_share(t, f, v):
    """Share of pitch-deviation power in 4-8 Hz against 0.5-12 Hz, over long voiced runs."""
    num = den = 0.0
    i = 0
    while i < len(v):
        if not v[i]:
            i += 1; continue
        j = i
        while j < len(v) and v[j]:
            j += 1
        if j - i >= 60:  # 0.6 s
            c = 1200 * np.log2(f[i:j] / 440.0)
            w = 30
            med = np.array([np.median(c[max(0, k - w):k + w + 1]) for k in range(len(c))])
            d = (c - med) * np.hanning(len(c))
            P = np.abs(np.fft.rfft(d, 4096)) ** 2
            fr = np.fft.rfftfreq(4096, 0.01)
            num += P[(fr >= 4) & (fr <= 8)].sum()
            den += P[(fr >= 0.5) & (fr <= 12)].sum()
        i = j
    return num / den if den else float('nan')


def frame_metrics(x, sr, t, f, v):
    N = 4096
    win = np.hanning(N)
    fr = np.fft.rfftfreq(N, 1 / sr)
    rows = []
    for c, f0, ok in zip(t, f, v):
        if not ok:
            continue
        a = int(c * sr) - N // 2
        if a < 0 or a + N > len(x):
            continue
        S = np.abs(np.fft.rfft(x[a:a + N] * win)) ** 2
        Sdb = 10 * np.log10(S + 1e-20)

        def peak(fk):
            w = max(0.03 * fk, 1.5 * fr[1])
            m = (fr > fk - w) & (fr < fk + w)
            return Sdb[m].max(), fr[m][np.argmax(S[m])]
        hs = []
        for k in range(1, 11):
            if k * f0 * 1.03 > sr / 2:
                break
            hs.append(peak(k * f0))
        if len(hs) < 8:
            continue
        lv = np.array([h[0] for h in hs])
        h1h2 = lv[0] - lv[1]
        tilt = np.polyfit(np.log2(np.arange(1, len(lv) + 1)), lv, 1)[0]
        p2v = []
        for k in range(2, 9):
            m = (fr > hs[k - 1][1]) & (fr < hs[k][1])
            p2v.append(lv[k - 1] - Sdb[m].min())
        harm = np.zeros(len(fr), bool)
        for k in range(1, int(5000 / f0) + 1):
            harm |= (np.abs(fr - k * f0) < 0.12 * f0 / 2)
        band = fr < 5000
        hnr = 10 * np.log10(S[harm & band].sum() / (S[~harm & band].sum() + 1e-20) + 1e-20)
        tot = S[(fr > 80) & (fr < 12000)].sum()
        lb = np.array([10 * np.log10(S[(fr >= b / 2 ** (1 / 6)) & (fr < b * 2 ** (1 / 6))].sum() / tot + 1e-20)
                       for b in BANDS])
        rows.append((f0, h1h2, tilt, float(np.median(p2v)), hnr, lb))
    return rows


def report(name, rows, vib):
    print(f'== {name}: {len(rows)} voiced frames, vib share {vib:.2f}')
    print('  f0 bin     n   H1-H2   tilt   p2v   hnr  step(3.2k-4k)')
    for lo, hi in BINS:
        r = [x for x in rows if lo <= x[0] < hi]
        if len(r) < 20:
            print(f'  {lo}-{hi:<4} {len(r):5d}  (too few)')
            continue
        lb = np.median([x[5] for x in r], 0)
        step = lb[list(BANDS).index(3150)] - lb[list(BANDS).index(4000)]
        print('  %3d-%-4d %5d %6.1f %6.1f %5.1f %5.1f %6.1f' % (
            lo, hi, len(r), np.median([x[1] for x in r]), np.median([x[2] for x in r]),
            np.median([x[3] for x in r]), np.median([x[4] for x in r]), step))
    print('  1/3-octave re total, median over voiced frames, per f0 bin:')
    print('          ' + ' '.join('%5d' % b for b in BANDS))
    for lo, hi in BINS:
        r = [x for x in rows if lo <= x[0] < hi]
        if len(r) >= 20:
            print('  %3d-%-4d' % (lo, hi) + ' '.join('%5.1f' % v for v in np.median([x[5] for x in r], 0)))


def main():
    mode = sys.argv[1]
    if mode == 'ref':
        d = sys.argv[2]
        meta = list(csv.DictReader(open(os.path.join(os.path.dirname(d.rstrip('/')) or '.', 'vocadito_metadata.csv'))))
        ids = [m['track_id'] for m in meta if float(m['average_pitch']) <= 55]
        rows, nv, nt = [], 0.0, 0
        for i in ids:
            x, sr = load(os.path.join(d, f'vocadito_{i}.wav'))
            t, f, v = f0_track(x, sr)
            rows += frame_metrics(x, sr, t, f, v)
            s = vib_share(t, f, v)
            if not np.isnan(s):
                nv += s; nt += 1
        report(f'vocadito male-range ({len(ids)} tracks)', rows, nv / max(nt, 1))
    else:
        for fn in sys.argv[2:]:
            x, sr = load(fn)
            t, f, v = f0_track(x, sr)
            report(os.path.basename(fn), frame_metrics(x, sr, t, f, v), vib_share(t, f, v))


if __name__ == '__main__':
    main()

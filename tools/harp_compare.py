#!/usr/bin/env python3
"""Compare the engine's rendered harp notes with harp recordings, note by note.

usage: harp_compare.py REC_DIR RENDER_DIR [--write-curve]

REC_DIR holds the VSCO 2 CE samples (tools/fetch_harp.sh, KSHarp_<note>_*.wav);
RENDER_DIR holds `engine/examples/harpnotes` output (harp_<midi>_s<seed>.wav).
Both are analysed the same way as the guitar curve (tools/body.py): the
partial amplitudes 20-370 ms after the onset, times k (the ideal string
force spectrum falls as 1/k). For each partial of each note the recording's
level minus the render's level is taken; the median per 1/12 octave from
80 Hz is the correction to add to the engine's harp body curve
(BODY_CURVES[1] in crates/instruments/src/body.rs). Decay is compared by
the -60 dB slope of partials 1-3 (T60), recording over render.
"""
import glob, os, re, sys
import numpy as np, soundfile as sf

NOTE = {'C': 0, 'D': 2, 'E': 4, 'F': 5, 'G': 7, 'A': 9, 'B': 11}
LO, HI, PER_OCT = 80.0, 11000.0, 12


def midi_of(name):
    m = re.match(r'([A-G])(#?)(\d)', name)
    return 12 * (int(m.group(3)) + 1) + NOTE[m.group(1)] + (1 if m.group(2) else 0)


def load(fn):
    x, sr = sf.read(fn)
    x = x.mean(1) if x.ndim > 1 else x
    return x / np.abs(x).max(), sr


def onset(x, sr):
    hop = 256
    e = np.array([np.sqrt((x[i:i + hop] ** 2).mean()) for i in range(0, len(x) - hop, hop)])
    return max(0, int(np.argmax(e > 0.2 * e.max())) * hop - int(0.01 * sr))


def partials(x, sr, m):
    """(k, freq, level dB + 20 log10 k) of each partial, and the onset sample."""
    f0n = 440 * 2 ** ((m - 69) / 12)
    a = onset(x, sr)
    s = x[a + int(0.02 * sr):a + int(0.02 * sr) + int(0.35 * sr)]
    n = len(s)
    M = 8 * n
    S = np.abs(np.fft.rfft(s * np.hanning(n), M))
    fr = sr / M
    i0, i1 = int(f0n * 0.97 / fr), int(f0n * 1.03 / fr) + 1
    f0 = (i0 + np.argmax(S[i0:i1])) * fr
    out = []
    for k in range(1, 120):
        fk = k * f0
        if fk > HI:
            break
        j0, j1 = int(fk * 0.985 / fr), int(fk * 1.015 / fr) + 1
        A = S[j0:j1].max()
        # A partial counts only when it stands 12 dB over the spectrum around
        # it (median magnitude in +-10%); below that the peak is the noise
        # floor of a recording or of the render, and says nothing of the body.
        w0, w1 = int(fk * 0.9 / fr), int(fk * 1.1 / fr) + 1
        if A < 4.0 * np.median(S[w0:w1]):
            continue
        out.append((k, (j0 + np.argmax(S[j0:j1])) * fr, 20 * np.log10(A + 1e-12) + 20 * np.log10(k)))
    return np.array(out), a, f0


def t60(x, sr, a, f0):
    N, hop = 8192, 2048
    w = np.hanning(N)
    fr = np.fft.rfftfreq(N, 1 / sr)
    env, ts = [], []
    for i in range(a, len(x) - N, hop):
        S = np.abs(np.fft.rfft(x[i:i + N] * w))
        e = 0.0
        for k in (1, 2, 3):
            m = np.abs(fr - k * f0) <= max(0.015 * k * f0, 2 * fr[1])
            e += S[m].max() ** 2
        env.append(10 * np.log10(e + 1e-20))
        ts.append((i - a) / sr)
    env, ts = np.array(env), np.array(ts)
    env -= env.max()
    sel = (env < -3) & (env > -25) & (ts < 6)
    if sel.sum() < 4:
        return np.nan
    p = np.polyfit(ts[sel], env[sel], 1)
    return -60 / p[0] if p[0] < 0 else np.nan


def median_curve(f, d):
    fs = LO * 2 ** (np.arange(0, int(np.log2(HI / LO) * PER_OCT) + 1) / PER_OCT)
    out = []
    for c in fs:
        m = (f >= c * 2 ** (-1 / (2 * PER_OCT))) & (f < c * 2 ** (1 / (2 * PER_OCT)))
        out.append(np.median(d[m]) if m.sum() >= 3 else np.nan)
    out = np.array(out)
    ok = ~np.isnan(out)
    return fs, np.interp(np.arange(len(out)), np.where(ok)[0], out[ok]), ok


def smooth(c, w=5):
    med = np.concatenate([[c[0]], np.median(np.vstack([c[:-2], c[1:-1], c[2:]]), 0), [c[-1]]])
    k = np.ones(w) / w
    p = np.pad(med, (w // 2, w // 2), mode='edge')
    return np.convolve(p, k, 'valid')


def main():
    rec_dir, ren_dir = sys.argv[1], sys.argv[2]
    recs = {}
    for fn in sorted(glob.glob(os.path.join(rec_dir, '*.wav'))):
        m = midi_of(re.search(r'_([A-G]#?\d)_', fn).group(1))
        if 53 <= m <= 90:  # the range the engine's harp plays (55-88)
            recs[m] = fn
    F, D, T = [], [], []
    per_note = []
    for m, fn in recs.items():
        x, sr = load(fn)
        pr, a, f0 = partials(x, sr, m)
        tr = t60(x, sr, a, f0)
        ds, ts = [], []
        for rf in sorted(glob.glob(os.path.join(ren_dir, f'harp_{m}_s*.wav'))):
            y, sr2 = load(rf)
            pe, b, g0 = partials(y, sr2, m)
            _, i, j = np.intersect1d(pr[:, 0], pe[:, 0], return_indices=True)
            ds.append((pr[i, 1], pr[i, 2] - pe[j, 2]))
            ts.append(t60(y, sr2, b, g0))
        if not ds:
            continue
        for f, d in ds:
            # The level of a note is arbitrary: take it from the partials
            # between 300 Hz and 3 kHz, where both signals are clean.
            band = (f > 300) & (f < 3000)
            F.append(f)
            D.append(d - np.median(d[band] if band.sum() >= 3 else d))
        per_note.append((m, f0, tr, float(np.nanmedian(ts))))
    fs, c, ok = median_curve(np.concatenate(F), np.concatenate(D))
    cs = smooth(c)
    print('correction to the harp body curve (dB, recording minus render, per 1/3 octave):')
    for i in range(0, len(fs) - 3, 3):
        print('%7.0f Hz %6.1f' % (fs[i + 1], cs[i:i + 3].mean()))
    print('\nT60 (s) of partials 1-3, same estimator: note, f0, recording, render, ratio')
    for m, f0, tr, te in per_note:
        print('%3d %7.1f %6.1f %6.1f %5.2f' % (m, f0, tr, te, tr / te))
    if '--write-curve' in sys.argv:
        np.save('harp_correction.npy', np.vstack([fs, cs]))


if __name__ == '__main__':
    main()

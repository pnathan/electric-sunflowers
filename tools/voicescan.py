#!/usr/bin/env python3
"""Scan a rendered lead stem for anomalies (issue 22).

usage: voicescan.py DIR     DIR holds lead.wav, phones.json and plan.json (the
                            stems example writes the last two beside the audio)

plan.json lists the planned noise and closure segments [class, t0, t1]; the scan
compares the audio with it.

Findings, one line each:
  static   a 4-10 kHz burst (rise of SPIKE_RISE dB or more within 10 ms, above
           SPIKE_FLOOR dB) outside every planned noise segment, 10 ms slack
  missing  a planned fric, vfric or burst segment with no 4-10 kHz rise of
           DETECT dB over the level just before it
  hole     inside a sung note and clear of every planned consonant segment, the
           100-4000 Hz level falls HOLE_DB below its median over +-150 ms for
           12 ms or more
  click    a sample step far above the local step level
Then a table per planned class: count, median and maximum 4-10 kHz peak (dB)
and median rise over the level before it.
"""
import json
import sys

import numpy as np
import soundfile as sf
from scipy.signal import butter, sosfilt

SPIKE_RISE = 15.0
SPIKE_FLOOR = -56.0
DETECT = 6.0
HOLE_DB = 15.0
SLACK = 0.010


def band(x, sr, lo, hi):
    return sosfilt(butter(4, [lo, hi], "band", fs=sr, output="sos"), x)


def env(y, sr, ms=4.0):
    h = max(1, int(sr * ms / 1000))
    n = len(y) // h
    return 20 * np.log10(np.sqrt((y[: n * h].reshape(n, h) ** 2).mean(1)) + 1e-9), h / sr


def main(d):
    x, sr = sf.read(f"{d}/lead.wav")
    x = x if x.ndim == 1 else x.mean(1)
    notes = json.load(open(f"{d}/phones.json"))
    plan = json.load(open(f"{d}/plan.json"))
    noise = [(k, a, b) for k, a, b in plan if k in ("fric", "vfric", "burst", "asp")]
    cons = [(k, a, b) for k, a, b in plan if k != "breath"]
    hf, dt = env(band(x, sr, 4000, 10000), sr)
    lf, _ = env(band(x, sr, 100, 4000), sr)
    n = len(hf)
    at = lambda t: min(max(int(t / dt), 0), n - 1)

    def phones_at(t):
        for nt in notes:
            if nt["t0"] - 0.25 <= t <= nt["t1"] + 0.03:
                return "[" + " ".join(nt["phones"]) + "]"
        return "[?]"

    out = []
    # static
    i = 8
    while i < n - 12:
        pre = hf[i - 8 : i - 1].min()
        if hf[i] > SPIKE_FLOOR and hf[i] - pre >= SPIKE_RISE and hf[i] >= hf[i - 1] and hf[i] >= hf[i + 1]:
            j = i
            while j < n - 1 and hf[j] > hf[i] - 10:
                j += 1
            t = i * dt
            if not any(a - SLACK <= t <= b + SLACK for _, a, b in noise):
                out.append((t, f"static   {t:8.3f}s  +{hf[i]-pre:3.0f} dB  hf {hf[i]:4.0f}  width {(j-i)*dt*1000:3.0f} ms  {phones_at(t)}"))
            i = j + 3
        else:
            i += 1
    # missing, and the per-class table
    stats = {}
    for k, a, b in noise:
        if b - a < 0.004:
            continue
        ia, ib = at(a), max(at(b), at(a) + 1)
        peak = hf[ia : ib + 1].max()
        pre = np.median(hf[max(ia - 8, 0) : max(ia - 1, 1)])
        rise = peak - pre
        s = stats.setdefault(k, [[], []])
        s[0].append(peak)
        s[1].append(rise)
        if rise < DETECT and k != "asp":
            out.append((a, f"missing  {a:8.3f}s  planned {k} {1000*(b-a):.0f} ms: peak {peak:.0f} dB is {rise:+.0f} over the level before  {phones_at(a+0.02)}"))
    # holes
    for nt in notes:
        a, b = at(nt["t0"] + 0.05), at(nt["t1"] - 0.05)
        if b - a < 10:
            continue
        run = 0
        for q in range(a, b):
            near = any(ca - 0.03 <= q * dt <= cb + 0.03 for _, ca, cb in cons)
            med = np.median(lf[max(q - 40, 0) : q + 40])
            run = run + 1 if (not near and lf[q] < med - HOLE_DB) else 0
            if run == 4:
                out.append((q * dt, f"hole     {q*dt:8.3f}s  {med-lf[q]:3.0f} dB under its surroundings  [{' '.join(nt['phones'])}]"))
    # clicks
    dx = np.abs(np.diff(x))
    h = int(sr * 0.005)
    m = len(dx) // h
    loc = np.sqrt((dx[: m * h].reshape(m, h) ** 2).mean(1)) + 1e-9
    for q in range(2, m - 2):
        peak = dx[q * h : (q + 1) * h].max()
        ref = np.median(loc[q - 2 : q + 3])
        if peak > 12 * ref and peak > 0.02:
            out.append((q * h / sr, f"click    {q*h/sr:8.3f}s  step {peak:.3f} is {peak/ref:.0f}x the local level"))
    out.sort()
    for _, line in out:
        print(line)
    kinds = [o[1].split()[0] for o in out]
    print("--", {k: kinds.count(k) for k in ("static", "missing", "hole", "click")})
    print("planned noise   count  peak median/max dB   rise median dB")
    for k, (pk, rs) in sorted(stats.items()):
        print(f"  {k:6s} {len(pk):8d}  {np.median(pk):8.1f} /{max(pk):6.1f}   {np.median(rs):8.1f}")


if __name__ == "__main__":
    main(sys.argv[1])

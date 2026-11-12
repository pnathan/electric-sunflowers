#!/usr/bin/env python3
"""Fit the guitar row of BODY_CURVES (crates/instruments/src/body.rs) to a
target long-term spectrum. numpy only.

Measure: x = mean of the channels; frames of 8192 samples, Hann window, hop
4096; keep frames whose mean square exceeds 5% of the largest; P = mean power
spectrum of the kept frames. L_i = 10 log10 (power of P in
[c_i 2^(-1/6), c_i 2^(1/6))) minus 10 log10 (power in [100, 400) Hz), with
c_i = 80 * 2^(i/12), i = 0..85. Target T = median of L over reference files;
engine E = mean of L over the engine's guitar stems (the gate seeds).

Algorithm: first pass d_i = clamp(T_i - E_i, -6, +36); new row = old + d,
peak moved to 0.0, rounded to 0.1 dB. Later passes (--residual) add
0.7 * clamp(T_i - E_i, -12, 12). Accept when max |T_i - E_i| <= 3 dB for
100 Hz <= c_i <= 8 kHz.

usage:
  guitar_curve.py target OUT.json WAV...
  guitar_curve.py engine OUT.json GUITAR_WAV...
  guitar_curve.py row TARGET.json ENGINE.json [--residual]
  guitar_curve.py check TARGET.json ENGINE.json
"""
import json, os, re, struct, sys
import numpy as np

PER = 12
LO = 80.0
NB = 86
C = LO * 2 ** (np.arange(NB) / PER)
BODY = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                    "..", "crates", "instruments", "src", "body.rs")


def rd(p):
    b = open(p, "rb").read()
    i = 12
    fmt = None
    while i < len(b) - 8:
        cid = b[i:i + 4]
        sz = struct.unpack_from("<I", b, i + 4)[0]
        if cid == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", b, i + 8)
        if cid == b"data":
            tag, nch, sr, _, _, bits = fmt
            if tag == 3:
                x = np.frombuffer(b, dtype="<f4", count=sz // 4, offset=i + 8).astype(float)
            elif bits == 16:
                x = np.frombuffer(b, dtype="<i2", count=sz // 2, offset=i + 8) / 32768.
            elif bits == 24:
                a = np.frombuffer(b, dtype=np.uint8, count=sz // 3 * 3, offset=i + 8)
                a = a.reshape(-1, 3).astype(np.int32)
                x = ((a[:, 0] | (a[:, 1] << 8) | (a[:, 2] << 16)) ^ 0x800000) - 0x800000
                x = x / 8388608.
            else:
                raise SystemExit("fmt %s" % (fmt,))
            return x.reshape(-1, nch).mean(1), sr
        i += 8 + sz + (sz & 1)
    raise SystemExit("no data chunk in " + p)


def ltas(x, sr, N=8192):
    w = np.hanning(N)
    fr = np.lib.stride_tricks.sliding_window_view(x, N)[::N // 2]
    e = (fr ** 2).mean(1)
    fr = fr[e > 0.05 * e.max()]
    P = (np.abs(np.fft.rfft(fr * w, axis=1)) ** 2).mean(0)
    f = np.fft.rfftfreq(N, 1 / sr)
    out = np.array([10 * np.log10(P[(f >= c / 2 ** (1 / 6)) & (f < c * 2 ** (1 / 6))].sum() + 1e-30)
                    for c in C])
    ref = 10 * np.log10(P[(f >= 100) & (f < 400)].sum())
    return out - ref


def measure(paths):
    return np.array([ltas(*rd(p)) for p in paths])


def read_row():
    s = open(BODY).read()
    m = re.search(r"// Guitar\s*\[(.*?)\]", s, re.S)
    v = [float(t) for t in m.group(1).replace("\n", " ").split(",") if t.strip()]
    assert len(v) == NB
    return np.array(v)


def fmt_row(row):
    toks = ["%.1f," % v for v in row]
    lines, cur = [], "       "
    for t in toks:
        if len(cur) + 1 + len(t) > 100:
            lines.append(cur)
            cur = "       "
        cur += " " + t
    lines.append(cur)
    return "\n".join(["    [", *(l for l in lines), "    ],"])


def load(p):
    return json.load(open(p))


def residual(t, e):
    return np.array(t["med"]) - np.array(e["mean"])


def main(a):
    if len(a) < 2:
        raise SystemExit(__doc__)
    cmd = a[0]
    if cmd == "target":
        R = measure(a[2:])
        json.dump({"c": C.tolist(), "med": np.median(R, 0).tolist(),
                   "q1": np.percentile(R, 25, 0).tolist(),
                   "q3": np.percentile(R, 75, 0).tolist(), "n": len(R)}, open(a[1], "w"))
    elif cmd == "engine":
        R = measure(a[2:])
        json.dump({"c": C.tolist(), "mean": R.mean(0).tolist(), "n": len(R)}, open(a[1], "w"))
    elif cmd == "row":
        r = residual(load(a[1]), load(a[2]))
        old = read_row()
        if "--residual" in a:
            new = old + 0.7 * np.clip(r, -12, 12)
        else:
            new = old + np.clip(r, -6, 36)
        new = np.round(new - new.max(), 1)
        print(fmt_row(new))
    elif cmd == "check":
        r = residual(load(a[1]), load(a[2]))
        band = (C >= 100) & (C <= 8000)
        for i in range(0, NB, 3):
            print("%7.0f  %+6.1f" % (C[i], r[i]))
        k = np.argmax(np.where(band, np.abs(r), -1))
        print("worst %.0f Hz %+.1f dB" % (C[k], r[k]))
        ok = abs(r[k]) <= 3
        print("PASS" if ok else "FAIL")
        sys.exit(0 if ok else 1)
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])

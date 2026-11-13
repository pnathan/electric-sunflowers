#!/usr/bin/env python3
"""Drum targets from a real acoustic kit, and the check of the engine's drums.

  drum_target.py ref KIT_DIR --json OUT [--rust OUT.rs]
  drum_target.py eng DIR --target JSON

`ref` reads DRSKit 2.1 (DrumGizmo team, drumgizmo.org, CC BY 4.0): per
instrument a directory with <Instrument>.xml (sample name, power, file) and
samples/*.wav, 13-channel 32-bit float, 44.1 kHz. Channels (1-based): 6 OHL,
7 OHR, 5 Kdrum_front, 10 Snare_top, 12 Tom2. It writes the statistics the
engine's drum model is built from (statistics only, no audio) as JSON and as
Rust tables (crates/instruments/src/drum_data.rs).

`eng` reads the files of the example `drumhits` (<kind>_<v>.wav: 8 hits of a
kind at velocity v, 1.5 s apart, the ride and the toms 3 s) and prints, per voice and
layer (v 0.25 soft, 0.6 medium, 0.95 hard): the band-curve RMS error, the
worst band, the decay ratio per octave band, the level against the hard
layer, and the soft-to-hard change of the 2 kHz and 4 kHz octave-band share
against the target's.

Statistics per voice, per velocity layer (the samples sorted by power and
split into three equal parts; the hard layer takes the remainder):
  curve   energy of the first 0.6 s after the onset in 1/3-octave bands
          31.5 Hz .. 16 kHz, dB relative to the hit's total (the sum of the
          bands); the mean over the layer's samples of the dB values.
  tau     per octave band (63 Hz .. 16 kHz): the time t20 for the band's
          energy envelope (FFT band mask, 5 ms frames, 3-frame smoothing) to
          fall 20 dB from its peak. The energy falls as exp(-2 t / tau)
          for an amplitude time constant tau, so tau = t20 / ln(10). The
          layer's value is the mean t20 of the samples that fall 20 dB
          within 3 s; a band with none takes its nearest valid band's.
  level   mean over samples of 20 log10 RMS of the first 0.3 s, minus the
          hard layer's.
  attack  median time from the onset to the peak of |x| within 50 ms.
Strong modes (medium layer; Kick, Snare, Tom, Ride, Rim): up to 4 peaks of
the mean long-window spectrum (first 1 s, each sample normalised to its
total) below 600 Hz (Ride 1 kHz) that stand 8 dB above its 1/3-octave
mean; level = energy within +-4 bins of the peak over the total (dB); tau
from the decay of a narrow band around it (median over the samples).
Swish (sustained): the band curve of the middle half of each sample only.

Perspective: Hat, Ride, Tap, Swish, Rim use the mean of the overhead pair;
Kick, Snare and Tom use the average of the energy fractions of the overhead
pair and of the close microphone (each normalised to its own total), and
take decays, strong modes and attack from the close microphone.
Numpy only; no fitting.
"""
import argparse
import glob
import json
import math
import os
import re
import struct
import subprocess

import numpy as np

SOURCE = ("DRSKit 2.1, DrumGizmo team (drumgizmo.org), CC BY 4.0; "
          "overhead pair and close microphones; statistics only")
C = np.array([31.5, 40, 50, 63, 80, 100, 125, 160, 200, 250, 315, 400, 500, 630,
               800, 1000, 1250, 1600, 2000, 2500, 3150, 4000, 5000, 6300, 8000,
               10000, 12500, 16000.])
OCT = np.array([63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000.])
LN10 = math.log(10.0)
# voice -> (instrument, close microphone channel index or None)
VOICES = {
    "Kick": ("Kdrum_without_contact", 4),
    "Snare": ("Snare", 9),
    "Rim": ("Snare_rim", None),
    "Tap": ("Snare_whisker", None),
    "Swish": ("Snare_circle_whisker", None),
    "Hat": ("Hihat_closed", None),
    "Ride": ("Ride_tip", None),
    "Tom": ("Tom2", 11),
}
MODE_FMAX = {"Kick": 600, "Snare": 600, "Tom": 600, "Rim": 600, "Ride": 1000}
GAP = {"ride": 3.0, "tom85": 3.0, "tom110": 3.0}
ENG_FILES = {"Kick": "kick", "Snare": "snare", "Rim": "rim", "Tap": "tap",
             "Swish": "swish", "Hat": "hat", "Ride": "ride"}
VEL = ["0.25", "0.6", "0.95"]
LAYERS = ["soft", "medium", "hard"]


def rdmc(p):
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
            x = np.frombuffer(b, dtype="<f4" if tag == 3 else "<i2",
                              count=sz // (bits // 8), offset=i + 8).astype(float)
            if tag != 3:
                x /= 32768.
            return x.reshape(-1, nch), sr
        i += 8 + sz + (sz & 1)


def onset(x):
    e = np.abs(x)
    th = 0.1 * e.max()
    return max(int(np.argmax(e > th)) - 32, 0)


def band_energy(x, sr, dur):
    n = int(dur * sr)
    seg = x[:n]
    seg = np.pad(seg, (0, max(0, n - len(seg))))
    P = np.abs(np.fft.rfft(seg * np.r_[np.ones(n - 256), np.hanning(512)[256:]])) ** 2
    f = np.fft.rfftfreq(n, 1 / sr)
    return np.array([P[(f >= c / 2 ** (1 / 6)) & (f < c * 2 ** (1 / 6))].sum() for c in C])


def fractions(x, sr, dur=0.6):
    """Band energies over the total of the bands (linear)."""
    B = band_energy(x, sr, dur)
    return B / (B.sum() + 1e-30)


def t20(x, sr, lo, hi):
    """Time for the band's energy envelope to fall 20 dB from its peak (NaN: never)."""
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / sr)
    X[(f < lo) | (f >= hi)] = 0
    y = np.fft.irfft(X, len(x))
    h = int(0.005 * sr)
    e = (y[:len(y) // h * h].reshape(-1, h) ** 2).mean(1)
    e = np.convolve(e, np.ones(3) / 3, 'same')
    k = np.argmax(e)
    if e[k] <= 1e-20:
        return np.nan
    r = e[k:] / e[k]
    b = np.where(r < 0.01)[0]
    return (b[0] * h / sr) if len(b) else np.nan


def octave_t20(x, sr):
    return np.array([t20(x[:int(3 * sr)], sr, c / math.sqrt(2), c * math.sqrt(2)) for c in OCT])


def fill_nearest(v):
    v = np.array(v, float)
    ok = np.where(~np.isnan(v))[0]
    if len(ok) == 0:
        return np.full_like(v, 0.1)
    out = v.copy()
    for i in range(len(v)):
        if np.isnan(v[i]):
            out[i] = v[ok[np.argmin(np.abs(ok - i))]]
    return out


def attack(x, sr):
    return float(np.argmax(np.abs(x[:int(0.05 * sr)]))) / sr


def rms03(x, sr):
    seg = x[:int(0.3 * sr)]
    return math.sqrt(float((seg ** 2).mean()) + 1e-30)


def kit_samples(root, inst):
    d = os.path.join(root, inst)
    xml = open(glob.glob(d + "/*.xml")[0]).read()
    pw = {m[1]: float(m[0]) for m in re.findall(
        r'<sample name="[^"]*" power="([^"]+)">\s*<audiofile[^>]*file="samples/([^"]+)"', xml)}
    return [(p, os.path.join(d, "samples", fn)) for fn, p in sorted(pw.items(), key=lambda t: t[1])]


def long_spectrum(x, sr, n=65536):
    seg = x[:sr]
    seg = np.pad(seg, (0, sr - len(seg)))
    P = np.abs(np.fft.rfft(seg * np.hanning(len(seg)), n)) ** 2
    return P / P.sum(), np.fft.rfftfreq(n, 1 / sr)


def strong_modes(sig, sr, fmax):
    """Up to 4 peaks below fmax, 8 dB over the 1/3-octave mean; sig: list of aligned samples."""
    Ps = []
    for x in sig:
        P, f = long_spectrum(x, sr)
        Ps.append(P)
    P = np.mean(Ps, 0)
    cs = np.r_[0, np.cumsum(P)]
    lo = np.searchsorted(f, f / 2 ** (1 / 6))
    hi = np.searchsorted(f, f * 2 ** (1 / 6))
    hi = np.maximum(hi, lo + 1)
    S = (cs[hi] - cs[lo]) / (hi - lo)
    cand = [k for k in range(2, len(f) - 2)
            if 30 <= f[k] <= fmax and P[k] >= P[k - 1] and P[k] >= P[k + 1]
            and P[k] >= S[k] * 10 ** 0.8]
    cand.sort(key=lambda k: -P[k])
    chosen = []
    for k in cand:
        if all(abs(f[k] - f[j]) > 0.08 * f[j] for j in chosen):
            chosen.append(k)
        if len(chosen) == 4:
            break
    out = []
    for k in sorted(chosen):
        e = P[max(k - 4, 0):k + 5].sum() / P.sum()
        hw = max(3.0, 0.04 * f[k])
        ts = [t20(x[:int(3 * sr)], sr, f[k] - hw, f[k] + hw) for x in sig]
        t = np.nanmedian(ts) if not np.all(np.isnan(ts)) else np.nan
        out.append(dict(f=float(f[k]), db=float(10 * math.log10(e)),
                        tau=float(t / LN10) if not np.isnan(t) else None))
    return out


def analyse_voice(root, voice):
    inst, close = VOICES[voice]
    rows = []
    for power, path in kit_samples(root, inst):
        x, sr = rdmc(path)
        oh = x[:, 5:7].mean(1)
        oh = oh[onset(oh):]
        if voice == "Swish":
            n = len(oh)
            mid = oh[n // 4:n - n // 4]
            rows.append(dict(F=fractions(mid, sr, len(mid) / sr)))
            continue
        row = dict(power=power, F_oh=fractions(oh, sr), rms_oh=rms03(oh, sr))
        if close is not None:
            c = x[:, close]
            c = c[onset(c):]
            row.update(F_cl=fractions(c, sr), rms_cl=rms03(c, sr), sig=c)
        else:
            row.update(sig=oh)
        row["F"] = 0.5 * (row["F_oh"] + row["F_cl"]) if close is not None else row["F_oh"]
        row["rms"] = (math.sqrt(row["rms_oh"] * row["rms_cl"]) if close is not None
                      else row["rms_oh"])
        row["t20"] = octave_t20(row["sig"], sr)
        row["att"] = attack(row["sig"], sr)
        row["sr"] = sr
        rows.append(row)
    if voice == "Swish":
        return dict(curve=np.mean([10 * np.log10(r["F"] + 1e-12) for r in rows], 0).tolist(),
                    n=len(rows))
    n = len(rows)
    parts = [rows[:n // 3], rows[n // 3:2 * n // 3], rows[2 * n // 3:]]
    lv = [np.mean([20 * math.log10(r["rms"]) for r in q]) for q in parts]
    layers = []
    for q, l in zip(parts, lv):
        with np.errstate(all="ignore"):
            t = np.array([r["t20"] for r in q])
            t = np.array([np.nanmean(t[:, i]) if np.any(~np.isnan(t[:, i])) else np.nan
                          for i in range(len(OCT))])
        layers.append(dict(
            curve=np.mean([10 * np.log10(r["F"] + 1e-12) for r in q], 0).tolist(),
            tau=(fill_nearest(t) / LN10).tolist(),
            level_db=float(l - lv[2]),
            attack=float(np.median([r["att"] for r in q]))))
    res = dict(layers=layers, n=n)
    if voice in MODE_FMAX:
        res["modes"] = strong_modes([r["sig"] for r in parts[1]], rows[0]["sr"], MODE_FMAX[voice])
    if voice == "Tom":
        # fundamental: the strongest peak of the medium layer's mean spectrum, 60-300 Hz
        sig = [r["sig"] for r in parts[1]]
        Ps = np.mean([long_spectrum(x, rows[0]["sr"])[0] for x in sig], 0)
        f = long_spectrum(sig[0], rows[0]["sr"])[1]
        m = (f > 60) & (f < 300)
        res["f0"] = float(f[m][np.argmax(Ps[m])])
    return res


def fmt(v, d):
    return f"{v:.{d}f}"


def write_rust(res, path):
    L = []
    w = L.append
    w("//! Drum reference statistics: generated by tools/drum_target.py from DRSKit 2.1")
    w("//! (DrumGizmo team, drumgizmo.org, CC BY 4.0), statistics only; do not edit by hand.")
    w("//!")
    w("//! Per voice and velocity layer (soft, medium, hard): the 1/3-octave band curve")
    w("//! (dB relative to the hit's total, first 0.6 s), the amplitude decay time")
    w("//! constant per octave band, the level against the hard layer, the attack time;")
    w("//! per voice the strong modes of the medium layer. Perspective and definitions:")
    w("//! docs/engine-design.md section 5.8 and the header of tools/drum_target.py.")
    w("")
    w("/// 1/3-octave band centres of the curves, Hz.")
    w("pub const BANDS: [f64; 28] = [")
    w("    " + ", ".join(repr(float(c)) for c in C) + ",")
    w("];")
    w("/// Octave band centres of the decay tables, Hz.")
    w("pub const OCT: [f64; 9] = [")
    w("    " + ", ".join(repr(float(c)) for c in OCT) + ",")
    w("];")
    w("")
    w("/// One velocity layer.")
    w("pub struct Layer {")
    w("    /// Band curve, dB relative to the hit's total.")
    w("    pub curve: [f64; 28],")
    w("    /// Amplitude decay time constant per octave band, seconds.")
    w("    pub tau: [f64; 9],")
    w("    /// RMS of the first 0.3 s, dB relative to the hard layer.")
    w("    pub level_db: f64,")
    w("    /// Onset to peak, seconds.")
    w("    pub attack: f64,")
    w("}")
    w("")
    w("/// A strong mode of the medium layer.")
    w("pub struct StrongMode {")
    w("    pub f: f64,")
    w("    /// Energy relative to the hit's total, dB.")
    w("    pub db: f64,")
    w("    /// Amplitude decay time constant, seconds.")
    w("    pub tau: f64,")
    w("}")
    w("")
    w("/// A voice: soft, medium and hard layers and the strong modes.")
    w("pub struct VoiceData {")
    w("    pub layers: [Layer; 3],")
    w("    pub modes: &'static [StrongMode],")
    w("}")
    w("")
    for voice in ("Kick", "Snare", "Rim", "Tap", "Hat", "Tom", "Ride"):
        r = res[voice]
        w(f"pub const {voice.upper()}: VoiceData = VoiceData {{")
        w("    layers: [")
        for la in r["layers"]:
            w("        Layer {")
            w("            curve: [" + ", ".join(fmt(v, 2) for v in la["curve"]) + "],")
            w("            tau: [" + ", ".join(fmt(v, 5) for v in la["tau"]) + "],")
            w(f"            level_db: {fmt(la['level_db'], 3)},")
            w(f"            attack: {fmt(la['attack'], 5)},")
            w("        },")
        w("    ],")
        ms = r.get("modes", [])
        w("    modes: &[")
        for m in ms:
            tau = m["tau"] if m["tau"] is not None else 0.1
            w(f"        StrongMode {{ f: {fmt(m['f'], 2)}, db: {fmt(m['db'], 2)}, tau: {fmt(tau, 5)} }},")
        w("    ],")
        w("};")
        w("")
    w("/// Swish band curve (the middle half of each stroke), dB relative to the total.")
    w("pub const SWISH_CURVE: [f64; 28] = [")
    w("    " + ", ".join(fmt(v, 2) for v in res["Swish"]["curve"]) + ",")
    w("];")
    w("")
    w("/// The reference tom's fundamental, Hz (the strong mode of the medium layer).")
    w(f"pub const TOM_F0: f64 = {fmt(res['Tom']['f0'], 2)};")
    open(path, "w").write("\n".join(L) + "\n")
    try:  # the repository's files are rustfmt-formatted
        subprocess.run(["rustfmt", "--edition", "2021", path], check=False)
    except OSError:
        pass


def cmd_ref(a):
    res = {"source": SOURCE, "bands": C.tolist(), "octaves": OCT.tolist()}
    for v in VOICES:
        print("analysing", v, flush=True)
        res[v] = analyse_voice(a.kit, v)
    os.makedirs(os.path.dirname(os.path.abspath(a.json)), exist_ok=True)
    json.dump(res, open(a.json, "w"), indent=1)
    if a.rust:
        write_rust(res, a.rust)
    for v in VOICES:
        r = res[v]
        if v == "Swish":
            continue
        print(v, "n", r["n"], "level dB", [round(l["level_db"], 1) for l in r["layers"]],
              "attack ms", [round(1000 * l["attack"], 1) for l in r["layers"]])
        print("   tau s", [round(t, 3) for t in r["layers"][1]["tau"]])
        print("   modes", [(round(m["f"]), round(m["db"], 1), m["tau"] and round(m["tau"], 3))
                           for m in r.get("modes", [])])
    print("Tom f0", res["Tom"]["f0"])


# ---------------------------------------------------------------- eng

def shifted(curve, s):
    """Curve moved along the frequency axis by s (value at f is the target's at f / s)."""
    lf = np.log2(C)
    return np.interp(lf - math.log2(s), lf, curve)


def octave_share(F):
    """Energy fractions (linear, 28 bands) -> share of the 2 kHz and 4 kHz octave bands, dB."""
    out = []
    for c in (2000., 4000.):
        idx = [i for i, v in enumerate(C) if c / 1.26 <= v <= c * 1.26]
        out.append(10 * math.log10(F[idx].sum() + 1e-30))
    return out


def eng_hits(path, gap):
    x, sr = rdmc(path)
    x = x[:, 0]
    n = int(gap * sr)
    return [x[k * n:(k + 1) * n] for k in range(8)], sr


def cmd_eng(a):
    tgt = json.load(open(a.target))
    print("target:", tgt["source"])
    names = [(v, ENG_FILES[v], None) for v in ENG_FILES] + [("Tom", "tom85", 85.0), ("Tom", "tom110", 110.0)]
    for voice, fn, hz in names:
        t = tgt[voice]
        gap = GAP.get(fn, 1.5)
        layers = [dict(curve=np.array(t["curve"]), tau=None)] * 3 if voice == "Swish" else t["layers"]
        print(f"== {voice} ({fn})")
        shares = []
        levels = []
        for li, v in enumerate(VEL):
            if voice == "Swish" and li != 1:
                pass
            p = os.path.join(a.dir, f"{fn}_{v}.wav")
            if not os.path.exists(p):
                print("   missing", p)
                continue
            hits, sr = eng_hits(p, gap)
            Fs, T20, rms = [], [], []
            for h in hits:
                h = h[onset(h):]
                Fs.append(fractions(h, sr))
                T20.append(octave_t20(h, sr))
                rms.append(20 * math.log10(rms03(h, sr)))
            Fm = np.mean(Fs, 0)
            B = np.mean([10 * np.log10(f + 1e-12) for f in Fs], 0)
            shares.append(octave_share(Fm))
            levels.append(float(np.mean(rms)) - 20 * math.log10(float(v)))
            tc = np.array(layers[li]["curve"])
            if hz is not None:
                tc = shifted(tc, hz / tgt["Tom"]["f0"])
            sel = (C >= 63) & (C <= 12500) & (tc > -35)
            d = B[sel] - tc[sel]
            wi = int(np.argmax(np.abs(d)))
            msg = f"   {LAYERS[li]:6s} v={v}: curve rms {math.sqrt((d ** 2).mean()):.2f} dB  worst {d[wi]:+.1f} dB at {C[sel][wi]:g} Hz"
            if voice != "Swish":
                with np.errstate(all="ignore"):
                    te = np.nanmedian(np.array(T20), 0) / LN10
                tt = np.array(layers[li]["tau"])
                ratios = []
                for i, c in enumerate(OCT):
                    j = int(np.argmin(np.abs(C - c)))
                    if tc[j] > -35 and not np.isnan(te[i]):
                        ratios.append(f"{te[i] / tt[i]:.2f}")
                    elif tc[j] > -35:
                        ratios.append(" nan")
                    else:
                        ratios.append("   -")
                msg += "\n          decay ratio per octave 63..16k: " + " ".join(ratios)
            print(msg)
        if len(shares) == 3 and voice != "Swish":
            tF = [10 ** (np.array(l["curve"]) / 10) for l in layers]
            ts = [octave_share(f) for f in tF]
            for k, c in enumerate(("2 kHz", "4 kHz")):
                e = shares[2][k] - shares[0][k]
                tt = ts[2][k] - ts[0][k]
                print(f"   soft->hard share at {c}: engine {e:+.1f} dB, target {tt:+.1f} dB, diff {e - tt:+.1f}")
            print(f"   level re hard (first 0.3 s, velocity gain removed): soft {levels[0] - levels[2]:+.1f} medium {levels[1] - levels[2]:+.1f} dB"
                  f" (target {layers[0]['level_db']:+.1f}, {layers[1]['level_db']:+.1f})")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("ref")
    r.add_argument("kit")
    r.add_argument("--json", required=True)
    r.add_argument("--rust")
    e = sub.add_parser("eng")
    e.add_argument("dir")
    e.add_argument("--target", required=True)
    a = ap.parse_args()
    cmd_ref(a) if a.cmd == "ref" else cmd_eng(a)


if __name__ == "__main__":
    main()

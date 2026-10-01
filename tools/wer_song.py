#!/usr/bin/env python3
"""Word error of a rendered song, line by line, by Whisper.

usage: wer_song.py SHEET.json STEMS_DIR [--model small.en] [--show]

SHEET.json is `sunflower sheet SONG --seed S --json` at the render's seed;
STEMS_DIR is the stems example's output for the same song and seed. Each
sung line is cut from [t0 - 0.1 s, t1 + 0.6 s] of two signals and
transcribed alone: "voice" (lead + lead_b + choir, the sung words without
the band) and "mix". Prints WER for each, pooled over lines (jiwer), and
with --show each line's reference and both hypotheses.

Whisper is trained on human speech; read the numbers as a proxy that ranks
versions of one song, not as an absolute intelligibility score.
"""
import json
import re
import sys
import warnings

import jiwer
import numpy as np
import soundfile as sf
import whisper
from scipy.signal import resample_poly

warnings.filterwarnings("ignore")


def clean(s):
    s = s.lower().replace("-", " ").replace("~", " ")
    s = re.sub(r"\[[^\]]*\]", " ", s)
    s = re.sub(r"[0-9]", " ", s)
    return " ".join(re.sub(r"[^a-z' ]", " ", s).split())


def load(path):
    x, sr = sf.read(path, dtype="float32", always_2d=True)
    return x.mean(axis=1), sr


def main():
    args = sys.argv[1:]
    show = "--show" in args
    model = "small.en"
    if "--model" in args:
        model = args[args.index("--model") + 1]
    sheet, d = [a for a in args if not a.startswith("--") and a != model][:2]
    lines = [
        l for s in json.load(open(sheet))["sections"] for l in s.get("lines", []) if clean(l["text"])
    ]
    mix, sr = load(f"{d}/mix.wav")
    voice = np.zeros_like(mix)
    for name in ("lead", "lead_b", "choir"):
        try:
            v, _ = load(f"{d}/{name}.wav")
            voice[: len(v)] += v[: len(voice)]
        except (FileNotFoundError, RuntimeError):
            pass
    m = whisper.load_model(model)
    out = {"voice": ([], []), "mix": ([], [])}
    for l in lines:
        a, b = int((l["t0"] - 0.1) * sr), int((l["t1"] + 0.6) * sr)
        ref = clean(l["text"])
        hyp = {}
        for key, sig in (("voice", voice), ("mix", mix)):
            seg = sig[max(a, 0) : b]
            peak = np.abs(seg).max() or 1.0
            seg = resample_poly(seg / peak * 0.9, 16000, sr).astype(np.float32)
            r = m.transcribe(
                seg, language="en", fp16=False, temperature=0, condition_on_previous_text=False
            )
            hyp[key] = clean(r["text"])
            out[key][0].append(ref)
            out[key][1].append(hyp[key] or "x")
        if show:
            print(f"  {ref}\n    voice | {hyp['voice']}\n    mix   | {hyp['mix']}")
    for key, (refs, hyps) in out.items():
        print(f"WER {key:5s} {jiwer.wer(refs, hyps):.3f} over {len(refs)} lines")


if __name__ == "__main__":
    main()

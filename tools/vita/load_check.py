"""Load .vital files in the open Vital engine and report what it kept.

Runs only as a separate process with the `vita` package, the GPL-3.0 Python
binding of the open engine. No code of this project links it.

    python load_check.py FILE...

Prints one JSON object per file on its own line:

- loaded: whether the engine accepted the file.
- changed: [key, written, read] for each numeric control the engine did not
  keep within 1e-6 relative or one 32-bit float step.
- dropped and added: settings keys of the file the engine dropped, and
  keys it added.
- peak, rms and finite: a render of note 60 at velocity 0.8 for 1 s. peak
  is the largest absolute sample of either channel. rms is the root mean
  square of the mean of the two channels, so it never exceeds peak. finite
  is false when any sample of either channel is NaN or infinite.
- bands: the energy of the mean of the two channels in 24 bands, in dB.

A number that is NaN or infinite prints as null, because JSON has no
spelling for it.
"""

import json
import math
import sys

import numpy as np
import vita

SAMPLE_RATE = 44100


def close(written, read):
    step = float(np.spacing(np.float32(abs(written))))
    return abs(written - read) <= max(1e-6 * abs(written), step)


def finite_or_none(value):
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, list):
        return [finite_or_none(item) for item in value]
    if isinstance(value, dict):
        return {key: finite_or_none(item) for key, item in value.items()}
    return value


def bands(audio):
    spectrum = np.abs(np.fft.rfft(audio)) ** 2
    freqs = np.fft.rfftfreq(len(audio), 1 / SAMPLE_RATE)
    edges = np.geomspace(20, 20000, 25)
    out = []
    for lo, hi in zip(edges[:-1], edges[1:]):
        energy = spectrum[(freqs >= lo) & (freqs < hi)].sum()
        out.append(round(float(10 * np.log10(energy + 1e-30)), 3))
    return out


def check(path):
    text = open(path, encoding="utf-8").read()
    written = json.loads(text)["settings"]
    synth = vita.Synth()
    synth.set_sample_rate(SAMPLE_RATE)
    if not synth.load_json(text):
        return {"file": path, "loaded": False}
    read = json.loads(synth.to_json())["settings"]
    changed = [
        [key, value, read.get(key)]
        for key, value in written.items()
        if isinstance(value, (int, float))
        and not isinstance(value, bool)
        and key in read
        and not close(value, read[key])
    ]
    audio = synth.render(60, 0.8, 1.0, 1.5)
    mono = (audio[0] + audio[1]) / 2
    return {
        "file": path,
        "loaded": True,
        "changed": changed,
        "dropped": [key for key in written if key not in read],
        "added": [key for key in read if key not in written],
        "peak": float(np.max(np.abs(audio))),
        "rms": float(np.sqrt(np.mean(mono**2))),
        "finite": bool(np.all(np.isfinite(audio))),
        "bands": bands(mono),
    }


def main():
    for path in sys.argv[1:]:
        print(json.dumps(finite_or_none(check(path)), allow_nan=False), flush=True)


if __name__ == "__main__":
    main()

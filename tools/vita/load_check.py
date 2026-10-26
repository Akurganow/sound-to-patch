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
- peak, rms and finite: a render of note 60 at velocity 0.8 for 1 s.
- bands: the energy of the render in 24 bands, in dB.
"""

import json
import sys

import numpy as np
import vita

SAMPLE_RATE = 44100


def close(written, read):
    step = float(np.spacing(np.float32(abs(written))))
    return abs(written - read) <= max(1e-6 * abs(written), step)


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
    mono = audio[0] + audio[1]
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
        print(json.dumps(check(path)), flush=True)


if __name__ == "__main__":
    main()

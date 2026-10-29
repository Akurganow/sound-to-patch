"""Render one note of a .vital preset in the open Vital engine.

Runs only as a separate process with the `vita` package, the GPL-3.0 Python
binding of the open engine. No code of this project links it.

    python render.py < request.json

Reads one JSON request on standard input and prints one JSON reply on
standard output:

- {"op": "version"} gives {"vita", "python", "arch"}.
- {"op": "render", "preset", "bpm", "sample_rate", "key", "velocity",
  "note_frames", "render_frames", "warm_up", "out"} renders one note and
  writes the clip to `out` as a 32-bit float stereo WAV file. It gives
  {"frames"}, which equals `render_frames`.
- {"op": "save", "preset", "bpm", "sample_rate"} gives {"state"}, the
  preset JSON the engine holds after the load.

`preset` is a path to the preset JSON, or null for the engine's init
patch. `velocity` is a MIDI velocity, 1 to 127. `warm_up` is a note
{"key", "velocity", "note_frames", "render_frames"} rendered after the
load and thrown away, or null. The engine takes lengths in seconds and
truncates their float32 product with the rate to frames, so a length of
n frames goes in as (n + 0.5) / rate.

The engine's own prints go to standard error, so only the reply reaches
standard output. An engine refusal, a preset it does not load or samples
that are not finite, exits with code 3 and a message on standard error.
Any other failure exits with another code.

A plug-in in a host takes its tempo from the host and saves it in
`beats_per_minute`, in beats per second. The script writes `bpm / 60`
there before the load, so the engine and its saved state agree with the
host's rule. Every `render` call of the engine starts with no sound left
from the call before.
"""

import json
import os
import platform
import struct
import sys

# Keep the engine's prints off the reply: from here on standard output
# goes to standard error, and the reply goes to the saved descriptor.
REPLY = os.fdopen(os.dup(1), "w")
os.dup2(2, 1)

import numpy as np  # noqa: E402
import vita  # noqa: E402


def synth(request):
    engine = vita.Synth()
    engine.set_sample_rate(int(request["sample_rate"]))
    engine.set_bpm(float(request["bpm"]))
    path = request.get("preset")
    if path is None:
        engine.load_init_preset()
        return engine
    with open(path, encoding="utf-8") as file:
        preset = json.load(file)
    preset.setdefault("settings", {})["beats_per_minute"] = float(request["bpm"]) / 60.0
    if not engine.load_json(json.dumps(preset)):
        refuse("the open engine did not load the preset")
    return engine


REFUSED = 3


def refuse(message):
    print(message, file=sys.stderr, flush=True)
    sys.exit(REFUSED)


def seconds(frames, rate):
    """The length in seconds that the engine truncates to `frames`: it
    multiplies the length by the rate in float32 and truncates."""
    length = (int(frames) + 0.5) / rate
    if int(np.float32(length) * np.float32(rate)) != int(frames):
        raise SystemExit(f"{frames} frames do not round trip at {rate} Hz")
    return length


def play(engine, note, rate):
    return engine.render(
        int(note["key"]),
        note["velocity"] / 127.0,
        seconds(note["note_frames"], rate),
        seconds(note["render_frames"], rate),
    )


def write_wav(path, audio, rate):
    frames = audio.shape[1]
    data = np.ascontiguousarray(audio.T, dtype="<f4").tobytes()
    header = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt "
    header += struct.pack("<IHHIIHH", 16, 3, 2, rate, rate * 8, 8, 32)
    header += b"data" + struct.pack("<I", len(data))
    with open(path, "wb") as out:
        out.write(header)
        out.write(data)
    return frames


def render(request):
    rate = int(request["sample_rate"])
    engine = synth(request)
    warm = request.get("warm_up")
    if warm is not None:
        play(engine, warm, rate)
    audio = play(engine, request, rate)
    if not np.all(np.isfinite(audio)):
        refuse("the open engine gave samples that are not finite")
    return {"frames": write_wav(request["out"], audio, rate)}


def main():
    request = json.load(sys.stdin)
    op = request.get("op")
    if op == "version":
        reply = {
            "vita": vita.__version__,
            "python": platform.python_version(),
            "arch": platform.machine(),
        }
    elif op == "render":
        reply = render(request)
    elif op == "save":
        reply = {"state": synth(request).to_json()}
    else:
        raise SystemExit(f"unknown op {op!r}")
    with REPLY:
        REPLY.write(json.dumps(reply) + "\n")


if __name__ == "__main__":
    main()

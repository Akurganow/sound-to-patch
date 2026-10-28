"""Compares the music dictionary's chord tones with two oracle libraries.

Reads one JSON object from stdin: {"chords": [{"symbol": "Cmaj7",
"classes": [0, 4, 7, 11]}, ...]}. Writes one JSON object to stdout: for
each oracle, the symbols whose pitch classes agree, differ, or that the
oracle cannot parse. It writes no oracle output row, and it reads no file:
the caller passes the dictionary on stdin and keeps the repository out of
reach.
"""

import json
import sys
from importlib.metadata import version

import music21
import pychord


def pychord_classes(symbol):
    return sorted({value % 12 for value in pychord.Chord(symbol).components(visible=False)})


def music21_classes(symbol):
    return sorted(set(music21.harmony.ChordSymbol(symbol).pitchClasses))


ORACLES = {"pychord": pychord_classes, "music21": music21_classes}


def compare(chords, classes_of):
    result = {"agree": [], "differ": [], "unsupported": []}
    for chord in chords:
        try:
            classes = classes_of(chord["symbol"])
        except Exception:  # noqa: BLE001 - any parse failure counts as unsupported
            result["unsupported"].append(chord["symbol"])
            continue
        verdict = "agree" if classes == sorted(chord["classes"]) else "differ"
        result[verdict].append(chord["symbol"])
    return result


def main():
    request = json.load(sys.stdin)
    report = {
        "tools": {
            "python": ".".join(str(part) for part in sys.version_info[:3]),
            "pychord": version("pychord"),
            "music21": version("music21"),
        },
        "oracles": {name: compare(request["chords"], classes_of) for name, classes_of in ORACLES.items()},
    }
    json.dump(report, sys.stdout, indent=2, sort_keys=True)


if __name__ == "__main__":
    main()

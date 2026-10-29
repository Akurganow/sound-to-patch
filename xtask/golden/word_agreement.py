"""Listener agreement of the producer words, from the pinned listener tables.

Run it through `cargo xtask agreement`, which fetches the EQ and reverb
tables of the pinned revision by SHA-256, installs the hash-locked tools and
calls this script with the two parquet paths. It prints derived numbers
only, as JSON on stdout: no curve, no parameter set and no row text.

EQ: rows are grouped by their lowercase text. Each curve holds 40 band
values. Agreement is the mean pairwise Pearson correlation of the curves of
one word, rounded to two decimals. The class is known only from 8 curves.
Band means and the peak band need 5 curves, so no single curve shows. A
word with fewer than 5 curves is written only when the word dictionary has
an entry for it: the list of entry words comes on stdin, so no free-text
label of a single listener reaches the output.

Reverb: a row's text is a comma-separated list of words, and the row counts
once for each. A word's effect on a parameter is its mean minus the global
mean, over the global standard deviation. The spread ratio is the word's
variance over the global variance, averaged over the five parameters. Both
need 5 rows. Variances are population variances.
"""

import json
import re
import sys
from collections import defaultdict

import numpy as np
import pyarrow
import pyarrow.parquet as pq

TOOLS = {
    "python": ".".join(str(part) for part in sys.version_info[:3]),
    "numpy": np.__version__,
    "pyarrow": pyarrow.__version__,
}
KNOWN_FROM = 8
AGGREGATE_FROM = 5
BANDS = [
    ("low", 0.0, 260.0),
    ("low_mid", 260.0, 1000.0),
    ("mid", 1000.0, 3100.0),
    ("high", 3100.0, 9000.0),
    ("air", 9000.0, float("inf")),
]
REVERB_KEYS = ["delay_time", "decay", "stereo_spread", "cutoff_freq", "wet_gain"]


def two(value):
    # The sum with 0.0 turns -0.0 into 0.0.
    return round(float(value), 2) + 0.0


def agreement_class(value, n):
    if n < KNOWN_FROM:
        return "unknown"
    if value >= 0.45:
        return "high"
    if value >= 0.20:
        return "medium"
    return "low"


def eq_words(path, keep):
    rows = pq.read_table(path).to_pylist()
    keys = rows[0]["param_keys"]
    if len(keys) != 40 or any(row["param_keys"] != keys for row in rows):
        sys.exit("the EQ table no longer holds 40 named bands per curve")
    hz = np.array([float(re.search(r"(\d+)Hz", key)[1]) for key in keys])
    curves = defaultdict(list)
    for row in rows:
        curves[row["text"].strip().lower()].append(row["param_values"])
    words = []
    for word in sorted(curves):
        matrix = np.array(curves[word])
        n = len(matrix)
        if n < AGGREGATE_FROM and word not in keep:
            continue
        value = None
        if n >= 2:
            pairs = np.triu_indices(n, 1)
            value = two(np.corrcoef(matrix)[pairs].mean())
        entry = {
            "word": word,
            "n": n,
            "value": value,
            "class": "unknown" if value is None else agreement_class(value, n),
            "few_sample": n < KNOWN_FROM,
        }
        if n >= AGGREGATE_FROM:
            mean = matrix.mean(axis=0)
            entry["bands"] = {
                name: two(mean[(hz >= low) & (hz < high)].mean())
                for name, low, high in BANDS
            }
            entry["peak_hz"] = int(hz[mean.argmax()])
        words.append(entry)
    return words, rows


def reverb_words(path):
    rows = pq.read_table(path).to_pylist()
    for row in rows:
        if row["param_keys"][: len(REVERB_KEYS)] != REVERB_KEYS:
            sys.exit("the reverb table no longer starts with the five parameters")
    values = np.array([row["param_values"][: len(REVERB_KEYS)] for row in rows])
    mean, sd, var = values.mean(axis=0), values.std(axis=0), values.var(axis=0)
    groups = defaultdict(list)
    for row, value in zip(rows, values):
        for word in {part.strip() for part in row["text"].lower().split(",")}:
            if word:
                groups[word].append(value)
    words = []
    for word in sorted(groups):
        matrix = np.array(groups[word])
        if len(matrix) < AGGREGATE_FROM:
            continue
        effects = (matrix.mean(axis=0) - mean) / sd
        words.append(
            {
                "word": word,
                "n": len(matrix),
                "spread_ratio": two((matrix.var(axis=0) / var).mean()),
                "effects": {key: two(e) for key, e in zip(REVERB_KEYS, effects)},
            }
        )
    distinct = len({tuple(value) for value in values})
    return words, rows, distinct


def holds_a_row(document, eq_rows, reverb_rows):
    """Whether any list or record of numbers in the output equals a row."""
    rows = {tuple(two(v) for v in row["param_values"]) for row in eq_rows + reverb_rows}
    found = []

    def walk(node):
        if isinstance(node, dict):
            numbers = tuple(v for v in node.values() if isinstance(v, (int, float)))
            if numbers in rows:
                found.append(node)
            for child in node.values():
                walk(child)
        elif isinstance(node, list):
            numbers = all(isinstance(v, (int, float)) for v in node)
            if numbers and tuple(node) in rows:
                found.append(node)
            for child in node:
                walk(child)

    walk(document)
    return found


def main():
    eq_path, reverb_path = sys.argv[1], sys.argv[2]
    keep = set(json.load(sys.stdin)["entry_words"])
    eq, eq_rows = eq_words(eq_path, keep)
    reverb, reverb_rows, distinct = reverb_words(reverb_path)
    document = {
        "tools": TOOLS,
        "eq_curves": len(eq_rows),
        "reverb_rows": len(reverb_rows),
        "reverb_distinct_settings": distinct,
        "eq": eq,
        "reverb": reverb,
    }
    stray = [e["word"] for e in eq if e["n"] < AGGREGATE_FROM and e["word"] not in keep]
    if stray:
        sys.exit(f"the output holds labels of fewer than {AGGREGATE_FROM} rows: {len(stray)}")
    if holds_a_row(document, eq_rows, reverb_rows):
        sys.exit("the output holds a row of the source data")
    json.dump(document, sys.stdout)


if __name__ == "__main__":
    main()

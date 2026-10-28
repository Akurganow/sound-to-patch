"""Ranked producer describing words, recomputed from the pinned public data.

Run it through `cargo xtask word-ranks`, which fetches the pinned files by
SHA-256, installs the hash-locked tools and calls this script with the file
paths. It prints derived counts only, as JSON on stdout: no preset name, tag,
caption or comment text, and no uploader name.

Items are presets, sounds and clips. An item counts once per word. Words
that run together in camel case are split, the text is lowercased, URLs and
HTML entities are dropped, and spelling variants of one word merge, as in
`dictionary/words/lexicon.json`.

Groups:
- Preset text: the names of six banks plus the comments of two of them. A
  comment that repeats within a bank counts once.
- Sound tags: tags, title and description of the sounds that carry a synth
  tag, counted by distinct uploader because a few uploaders own most
  of those sounds.
- Captions: caption and aspect list of the clips that mention "synth".

A word enters the main list when two groups reach their floor. Its score is
the reciprocal rank fusion of the three groups, the sum of 1/(10 + rank).
Within a group a word's rank counts the words with a hit, with ties sharing
the best place. Tier W1 holds ranks 1 to 25, W2 ranks 26 to 60.
"""

import argparse
import csv
import html
import json
import re
import sys
import tarfile
from collections import defaultdict

import numpy
import pyarrow
import pyarrow.parquet as pq

TOOLS = {
    "python": ".".join(str(part) for part in sys.version_info[:3]),
    "numpy": numpy.__version__,
    "pyarrow": pyarrow.__version__,
}
FLOORS = {"preset_hits": 3, "uploaders": 10, "captions": 5}
RRF_OFFSET = 10
W1_LAST = 25
W2_LAST = 60
SYNTH_TAGS = {
    "synth",
    "synthesizer",
    "synthesiser",
    "synthesized",
    "synthesised",
    "synths",
    "synthesizers",
    "synthetizer",
}
# The Vital metadata sits in the first bytes of the file; the rest is
# wavetable data.
VITAL_HEAD = 8192
PATCH_DIRS = ("resources/data/patches_factory/", "resources/data/patches_3rdparty/")
SERUM2_DIR = "EXAMPLE PRESETS/Factory/"
PRESET_DIR = "assets/Soundbanks/Factory Presets/"
PATCH_BANK_DIR = "Factory Presets/"

URL = re.compile(r"(?:https?://|www\.)\S+")
ENTITY = re.compile(r"&(?:#\d+|#x[0-9a-f]+|[a-z][a-z0-9]*);")
CAMEL = re.compile(r"([a-z0-9])([A-Z])")
TOKEN = re.compile(r"[a-z0-9]+")
# Spellings that split into several tokens or differ by a hyphen.
SPELLINGS = [
    (re.compile(r"\blo[- ]fi\b"), "lofi"),
    (re.compile(r"\b8[- ]bit\b"), "8bit"),
    (re.compile(r"\be[- ]piano\b"), "epiano"),
    (re.compile(r"\bsci[- ]?fi\b"), "scifi"),
    (re.compile(r"\bhip[- ]?hop\b"), "hiphop"),
    (re.compile(r"\bold[- ]?school\b"), "oldschool"),
    (re.compile(r"\bdrum(?: ?n ?| ?& ?| and |'n')bass\b"), "dnb"),
]


def tokens(text):
    # Preset names run words together in camel case, so split before the
    # lowercase step that would hide the boundary.
    text = CAMEL.sub(r"\1 \2", text)
    text = ENTITY.sub(" ", URL.sub(" ", text.lower()))
    for pattern, spelling in SPELLINGS:
        text = pattern.sub(spelling, text)
    return TOKEN.findall(text)


def features(text):
    """The tokens of a text plus its adjacent pairs and triples."""
    found = tokens(text)
    keys = set(found)
    keys.update(" ".join(found[i : i + 2]) for i in range(len(found) - 1))
    keys.update(" ".join(found[i : i + 3]) for i in range(len(found) - 2))
    return keys


def phrase(text):
    return " ".join(tokens(text))


def lookup(lexicon):
    """Maps each spelling of a word to the ids of the entries it counts for."""
    describing, style, technique = defaultdict(set), defaultdict(set), defaultdict(set)
    for entry in lexicon["describing"]:
        for spelling in [entry["word"], *entry["variants"]]:
            describing[phrase(spelling)].add(entry["word"])
    for group, table in (("style", style), ("technique", technique)):
        for entry in lexicon[group]:
            for spelling in entry["matches"]:
                table[phrase(spelling)].add(entry["label"])
    return describing, style, technique


class Counter:
    """Hits per table and word: how many items each owner contributes."""

    def __init__(self, tables):
        self.tables = tables
        self.items = 0
        self.owners = set()
        self.hits = [defaultdict(lambda: defaultdict(int)) for _ in tables]

    def add(self, text, owner):
        self.items += 1
        self.owners.add(owner)
        keys = features(text)
        for table, hits in zip(self.tables, self.hits):
            for word in {word for key in keys & table.keys() for word in table[key]}:
                hits[word][owner] += 1

    def items_of(self, index, word):
        return sum(self.hits[index][word].values())

    def owners_of(self, index, word):
        return len(self.hits[index][word])


def preset_banks(args):
    """Yields (bank, name, comment) for every preset of the six banks."""
    yield from names_from_tree(
        "bank-a", args.bank_a, SERUM2_DIR, ".SerumPreset"
    )
    yield from vital_archive(args.bank_b)
    yield from patch_xml(args.bank_c)
    yield from names_from_tree("bank-d", args.bank_d, PRESET_DIR, ".odin")
    yield from names_from_tree("bank-e", args.bank_e, PATCH_BANK_DIR, ".helm")
    yield from bank_f_names(args.bank_f)


def stem(path):
    return path.rsplit("/", 1)[-1].rsplit(".", 1)[0]


def names_from_tree(bank, path, directory, suffix):
    with open(path, encoding="utf-8") as handle:
        tree = json.load(handle)
    if tree.get("truncated"):
        sys.exit(f"the {bank} tree listing is truncated")
    for node in tree["tree"]:
        name = node["path"]
        if node["type"] == "blob" and name.startswith(directory) and name.endswith(suffix):
            yield bank, stem(name), ""


def vital_archive(path):
    name_key = re.compile(rb'"preset_name":"((?:[^"\\]|\\.)*)"')
    comment_key = re.compile(rb'"comments":"((?:[^"\\]|\\.)*)"')
    with tarfile.open(path, "r|gz") as archive:
        for member in archive:
            if not (member.isfile() and member.name.endswith(".vital")):
                continue
            head = archive.extractfile(member).read(VITAL_HEAD)
            name, comment = name_key.search(head), comment_key.search(head)
            yield (
                "bank-b",
                decode(name[1]) if name else stem(member.name),
                decode(comment[1]) if comment else "",
            )


def decode(raw):
    return json.loads(b'"' + raw + b'"')


def patch_xml(path):
    meta = re.compile(rb"<meta ([^>]*)>")
    attribute = re.compile(rb'(\w+)="([^"]*)"')
    with tarfile.open(path, "r|gz") as archive:
        for member in archive:
            name = member.name.split("/", 1)[-1]
            if not (member.isfile() and name.endswith(".fxp") and name.startswith(PATCH_DIRS)):
                continue
            found = meta.search(archive.extractfile(member).read())
            fields = {
                key.decode(): html.unescape(value.decode("utf-8", "replace"))
                for key, value in attribute.findall(found[1])
            } if found else {}
            yield "bank-c", fields.get("name") or stem(name), fields.get("comment", "")


def bank_f_names(paths):
    """The unique `.fxp` names of the Serum rows in the three splits."""
    names = set()
    for name_path, synth_path in zip(paths[0::2], paths[1::2]):
        label = numpy.load(name_path, allow_pickle=False)
        synth = numpy.load(synth_path, allow_pickle=False)
        names.update(str(n) for n in label[synth == "serum"] if str(n).endswith(".fxp"))
    for name in sorted(names):
        yield "bank-f", stem(name), ""


def count_presets(args, tables):
    counter = Counter(tables)
    sizes = defaultdict(int)
    seen_comments = defaultdict(set)
    for bank, name, comment in preset_banks(args):
        sizes[bank] += 1
        comment = comment.strip().lower()
        if comment and comment not in seen_comments[bank]:
            seen_comments[bank].add(comment)
            name = f"{name} {comment}"
        counter.add(name, bank)
    return counter, dict(sizes)


def count_sounds(path, tables):
    counter = Counter(tables)
    table = pq.read_table(path)
    tag_column = next(name for name in table.column_names if name.startswith("tags"))
    for row in table.select([tag_column, "title", "description", "username"]).to_pylist():
        raw = row[tag_column] or ""
        if SYNTH_TAGS.isdisjoint(tag.strip().lower() for tag in raw.split(",")):
            continue
        text = " ".join([raw.replace(",", " "), row["title"] or "", row["description"] or ""])
        counter.add(text, row["username"])
    return counter


def count_captions(path, tables):
    counter = Counter(tables)
    with open(path, encoding="utf-8", newline="") as handle:
        for index, row in enumerate(csv.DictReader(handle)):
            text = f"{row['caption']} {row['aspect_list']}"
            if "synth" in text.lower():
                counter.add(text, index)
    return counter


def rank_table(values):
    """Competition ranks by descending value: 1, 2, 2, 4."""
    ordered = sorted(values.values(), reverse=True)
    return {word: ordered.index(value) + 1 for word, value in values.items()}


def fuse(words, preset, sound, caption):
    """The reciprocal rank fusion score of every word that enters the list."""
    counts = (
        {w: preset.items_of(0, w) for w in words},
        {w: sound.owners_of(0, w) for w in words},
        {w: caption.owners_of(0, w) for w in words},
    )
    floors = (FLOORS["preset_hits"], FLOORS["uploaders"], FLOORS["captions"])
    entered = [
        w for w in words if sum(c[w] >= floor for c, floor in zip(counts, floors)) >= 2
    ]
    scores = dict.fromkeys(entered, 0.0)
    for group in counts:
        for word, rank in rank_table({w: n for w, n in group.items() if n}).items():
            if word in scores:
                scores[word] += 1 / (RRF_OFFSET + rank)
    return scores


def holds_source_text(document, allowed):
    """Strings of the output that are not a lexicon word or a fixed key."""
    stray = []

    def walk(node):
        if isinstance(node, dict):
            for key, child in node.items():
                if key not in allowed:
                    stray.append(key)
                walk(child)
        elif isinstance(node, list):
            for child in node:
                walk(child)
        elif isinstance(node, str) and node not in allowed:
            stray.append(node)

    walk(document)
    return stray


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--lexicon", required=True)
    for source in (
        "bank-a",
        "bank-b",
        "bank-c",
        "bank-d",
        "bank-e",
        "sound-tags",
        "captions",
    ):
        parser.add_argument(f"--{source}", required=True)
    parser.add_argument("--bank-f", nargs=6, required=True, metavar="FILE")
    args = parser.parse_args()
    with open(args.lexicon, encoding="utf-8") as handle:
        lexicon = json.load(handle)
    tables = lookup(lexicon)
    preset, bank_sizes = count_presets(args, tables)
    sound = count_sounds(args.sound_tags, tables)
    caption = count_captions(args.captions, tables)

    def row(word, index):
        return {
            "preset_hits": preset.items_of(index, word),
            "banks": preset.owners_of(index, word),
            "uploaders": sound.owners_of(index, word),
            "sounds": sound.items_of(index, word),
            "captions": caption.owners_of(index, word),
        }

    words = [entry["word"] for entry in lexicon["describing"]]
    scores = fuse(words, preset, sound, caption)
    order = sorted(scores, key=lambda word: (-round(scores[word], 9), word))
    rank = {word: place + 1 for place, word in enumerate(order)}
    describing = []
    for word in words:
        place = rank.get(word)
        tier = None
        if place:
            tier = "W1" if place <= W1_LAST else "W2" if place <= W2_LAST else None
        describing.append(
            {
                "word": word,
                **row(word, 0),
                "score": round(scores[word], 4) if word in scores else None,
                "rank": place,
                "tier": tier,
            }
        )
    describing.sort(key=lambda r: (r["rank"] is None, r["rank"] or 0, r["word"]))
    document = {
        "tools": TOOLS,
        "presets": sum(bank_sizes.values()),
        "banks": dict(sorted(bank_sizes.items())),
        "sounds": sound.items,
        "uploaders": len(sound.owners),
        "captions": caption.items,
        "describing": describing,
        "style": [
            {"label": entry["label"], **row(entry["label"], 1)} for entry in lexicon["style"]
        ],
        "technique": [
            {"label": entry["label"], **row(entry["label"], 2)}
            for entry in lexicon["technique"]
        ],
    }
    keys = [
        "tools", "presets", "banks", "sounds", "uploaders", "captions", "describing",
        "style", "technique", "word", "label", "preset_hits", "score", "rank", "tier",
    ]
    allowed = {*keys, *TOOLS, *TOOLS.values(), "W1", "W2", *bank_sizes}
    allowed.update(entry["word"] for entry in lexicon["describing"])
    allowed.update(entry["label"] for group in ("style", "technique") for entry in lexicon[group])
    stray = holds_source_text(document, allowed)
    if stray:
        sys.exit(f"the output holds {len(stray)} string(s) that are not lexicon words")
    json.dump(document, sys.stdout)


if __name__ == "__main__":
    main()

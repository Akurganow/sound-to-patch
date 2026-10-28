# Agent reference

The agent reference is a folder of plain files, `reference/`. It tells an
agent which controls a synth has, by layer, with the preset key, the unit,
the range, the default, how sure each value is and how to tune it by ear.
Two thin entry points route an agent into it, and both read the same
files:

- the plugin `plugins/sound-to-patch-reference`, the entry point for local
  sessions. Its skill `synth-reference` links the folder as `reference`.
  The audio path and the text path will ship as plugins of their own that
  depend on it;
- the project skill `.claude/skills/synth-reference`, a link to the skill
  of the plugin, which a session opened on a clone of this repository
  loads.

## Layout

```text
reference/                             the shared reference, one copy
  README.md                            which file answers which question
  core/<topic>.md                      generated: one music file per topic
  vital/<layer>.md                     generated: one file per layer
  vital/global.md                      generated: controls outside the layers
  vital/examples/*.json                edit lists that CI applies
.claude-plugin/marketplace.json        the marketplace: one entry per plugin
plugins/sound-to-patch-reference/
  .claude-plugin/plugin.json           name, description, licence; no version
  README.md
  skills/synth-reference/
    SKILL.md                           the rules to keep and a pointer
    reference -> ../../../../reference
.claude/skills/synth-reference -> ../../plugins/sound-to-patch-reference/skills/synth-reference
dictionary/
  music/<topic>.json                   music entries, one file per topic
  music/stats.json                     the shares the music entries rest on
  music/plan.json                      the planned entry count per group and tier
  words/<axis>.json                    neutral word entries, one file per axis
  sources.json                         every source the dictionary cites
  probes.json                          one probe question per reference file
  examples.json                        the expected number of examples per synth
schemas/music.schema.json, schemas/stats.schema.json,
schemas/words.schema.json, schemas/sources.schema.json
```

Edit the reference only in `reference/`. An installed plugin keeps the
linked files: the harness copies a linked folder into its cache when it
installs a plugin from a repository, while files outside the plugin with
no link are left out. A zip of the plugin folder carries them too, because
`zip` stores the linked files.

The plugin carries no version. Users who install it from the marketplace
track the commits of `main`, and each release also ships the folder as
`sound-to-patch-reference-vX.Y.Z.zip`.

## The generated catalogue

`cargo xtask reference` writes the files under `reference/<synth>/` for
each synth that has an adapter. It reads the parameter registry, the same
data that `preset apply` checks edit lists against, so the reference and
the writer never disagree. Never edit a generated file by hand: change the
registry or the generator, then run the command again. `cargo xtask
reference --check` fails when a committed file differs; CI runs it.

- The layer of each key comes from the adapter of its synth
  (`preset::catalogue`). Keys that belong to no layer, such as the tempo
  or the volume, go to `global.md`.
- A control with several instances is one row, with `N` in its key and the
  instance numbers in a column. Vital has 248 such patterns for its 794
  keys. Instances that differ in their tag get a row each.
- The tag comes from the provenance of the registry entry: `from facts`,
  `measured`, or `uncalibrated` for an interim law. An uncalibrated row
  shows its tuning note.
- Legacy keys, which readers accept and writers refuse, and keys that only
  newer builds of the app write, are listed apart in `global.md`.
- Numbers show five significant digits. The registry keeps the exact
  values.

## The music dictionary

The music dictionary turns a producer's note words into checkable rules:
keys, scales, chords, voicings, progressions, rhythm, articulation,
phrase and genre words. The data lives in `dictionary/music/`, one file
per topic, and the module `src/music` embeds those files with
`include_str!` and parses them once. So the binary and the generated
reference read the same data, and nothing can drift between them.

Each entry is one sense of a word:

- `id` is `<group>.<word>` or `<group>.<word>.<sense>`, such as
  `scale.phrygian` or `position.offbeat.eighth`.
- `rule` has a kind from a fixed list, such as `pitch_set`, `chord_tones`,
  `cycle`, `onset_set` or `register`, and the numbers of that kind. Free
  text is rejected. `none` marks a word that names no note rule, such as a
  section word.
- `example_pass` and `example_fail` are small note fragments that meet and
  break the rule. The loader checks that they are well formed and in
  range; the text-to-notes task evaluates them.
- `tier` says how much the entry is needed: every planned M1 entry must
  land, at least half of M2 and a quarter of M3. `source` names ids from
  `dictionary/sources.json`.
- `tag` says how the claim is known, one of `source states`, `measured`,
  `derived` (our computation on public data), `inferred` (our reading),
  `snippet` (a search excerpt), `single source` and `proposal` (a value or
  policy we chose, such as a default pick or a velocity band). An entry
  that is mostly facts takes the tag of its facts.
- A word with several senses has one entry per sense and exactly one
  default. An ask word carries the one question on its default sense.
- Chord entries list their spellings in `symbols`; the chord parser reads
  only those, so a spelling outside the dictionary is an error that names
  it.

Roman numerals take the mode of the key when it is given. Without one, a
line that starts with `I` reads as major, because a major key borrows
`III`, `VI` and `VII` as secondary chords. Any other line is minor when it
starts with `i` or holds a bare `III`, `VI` or `VII`. A line read as major
without a minor sign is flagged as an assumption. `X/Y` is a secondary
chord read in the key of Y; `I/Y` is refused, since it could be a slash
chord.

The middle-C convention is named by the label a program gives note 60:
`C3`, `C4` (scientific) or `C5`. Notes are stored as MIDI numbers, and a
note name with no stated convention is read in the role range and flagged
as an assumption. `SynthCapabilities::middle_c` holds the label of each
synth's own note display: Serum 2 labels note 60 C3, and the Vital label
stays unset because its source draws no note names; a probe of the app
settles it.

To add or change a music entry:

1. Edit the topic file under `dictionary/music/`.
2. Run `cargo nextest run --test it music::`; fix each problem it names.
3. Add any new share to `dictionary/music/stats.json`, with counts when the source gives them.
4. Raise the group's count in `dictionary/music/plan.json` when the entry is planned.
5. Run `cargo xtask reference` to rewrite `reference/core/`.

The oracle libraries never enter the repository, and they cover only part
of the dictionary:

| Oracle | Where it runs | What it compares |
| :-- | :-- | :-- |
| `rust-music-theory` | CI, dev-dependency | 13 of the 57 scale rows; 23 chord qualities |
| `chordparser` | CI, dev-dependency | every chord spelling; 14 of 137 do not parse |
| `pychord` | golden tools, local | every chord spelling; 87 of 137 do not parse |
| `music21` | golden tools, local | every chord spelling; 65 of 137 do not parse |

The npm table the published lexicon was checked against is not run, so
the lexicon's 56-of-57 scale and 22-of-31 chord comparisons are not
repeated here. `cargo xtask golden` passes the spellings on stdin to
`xtask/golden/music_oracle.py`, which runs in the temp folder and writes
only which spellings agree, differ or do not parse. The tests pin the
spellings that differ, so a change in either the dictionary or an oracle
fails them. Snapshot tests pin every scale's offsets, every onset set and
pattern, every tempo and progression root and every stated share, so a
changed number shows in review.

Most shares in `dictionary/music/stats.json` are transcribed from the
published lexicon, whose aggregation scripts are not committed; they carry
the tag `single source` and cite `lexicon` next to their corpus. Only a
share with counts is recomputed. A source whose terms forbid bulk
extraction or model evaluation, or whose fields come from a commercial
API, gives no share. A non-commercial chord database gives aggregate
shares only, never a row or a song. A genre entry that cites the
non-commercial genre guide keeps its tempo range or drum skeleton only
when a second source agrees and cites both; otherwise it carries the tag
`single source`. Tempos and skeletons are written in this dictionary's
own form: a range with a centre and slot sets on a 16-step grid.

## Checks

`cargo xtask check-plugins` runs in the docs job. It fails on:

- a marketplace entry whose source does not start with `./`, leaves the
  repository or names another plugin, and a plugin folder with no entry;
- a manifest whose `name` differs from its folder or whose description
  lacks the non-affiliation line, and a plugin README without that line;
- a `SKILL.md` with a frontmatter field outside the six fields of the
  skill specification, a `name` that differs from its folder, a
  description over 1,024 characters, 500 lines or more, more than 20,000
  characters, or no non-affiliation line;
- a reference file over 300 lines, or over 100 lines without a
  `## Contents` list before its first section;
- a plugin skill whose `reference` is not a relative link to `reference/`,
  and a project skill under `.claude/skills/` that is not a relative link
  to the skill of the plugin; a link checked out as a plain file is
  reported as "symlink not checked out";
- a file under `reference/` that is neither Markdown nor JSON;
- a phrase that a folder must not hold: for Vital, the name of the other
  synth; for `core/`, a program or service name other than Serum 2 and
  Vital, kept in the source as SHA-256 digests so the names stay out of
  the tree; for both, version comparisons such as "older version";
- a reference file without a probe in `dictionary/probes.json`, and a
  probe answer that appears in no file or in more than one.

`cargo xtask reference` fails when a planned M1 or M2 group of the music
dictionary falls under its floor, and names the group.

The integration tests check the dictionary files against their schemas,
apply every example edit list to the built-in template of its synth, and
compare the number of examples with `dictionary/examples.json`. A local test,
ignored by default, checks that the catalogue covers every key of a preset
that the app saved; set `VITAL_PROBE_PRESET` to the preset's path and run
it with `--run-ignored only`.

`cargo xtask check-credits` also reads the plugin pages and
`dictionary/sources.json`: every source they cite needs a README credit.

To check a plugin with the agent harness itself, run it locally:

```sh
claude plugin validate .
claude plugin validate plugins/sound-to-patch-reference
```

## Plugin loading

Checked on 2026-10-28, in a local session:

| Check | Result |
| :-- | :-- |
| Files outside the plugin folder after an install from the repository on GitHub | absent: the plugin is copied into a cache. A link inside the plugin to a folder elsewhere in the repository is copied as that folder, with its files |
| `plugin validate` on the marketplace root and on each plugin folder | passes; it warns that no `version` is set, which is intended |
| `marketplace add <path>` and `install` | the plugin's skill loads; a local folder is read in place |
| Installing a plugin whose `plugin.json` lists another in `dependencies` | installs both; uninstalling it marks the other as no longer needed |
| A zip of the plugin folder, nested or flat, as `--plugin-dir` | loads the skill |
| A plugin folder under `.claude/skills/` as `<name>@skills-dir` | not listed by `plugin list` or `plugin details`. The docs say it loads once the folder is trusted, from the primary working folder only, and in a worktree from the main checkout. Not used |
| The linked project skill `.claude/skills/synth-reference` in a clean export of a commit | a headless session lists `synth-reference`, and its `reference` link resolves into `reference/` |
| Cloud sessions | not a target and not run. The docs say a committed project skill loads there and an installed plugin does not |

Which install paths carry `reference/`:

| Install path | Carries `reference/`? |
| :-- | :-- |
| `marketplace add Akurganow/sound-to-patch` and `install` (a copy from GitHub into the cache) | yes, checked: the link is copied as the folder |
| A release zip, through `--plugin-dir` | yes, checked: `zip` stores the linked files, and the release checks it |
| `marketplace add <local folder>` and `install` | yes, checked: the plugin is read in place, and its `reference/` lists `README.md` and every file of `vital/` |
| A plugin copied from a local path, or a `command` source in its default copy mode | no: the docs say such a copy keeps only links inside the plugin's own folder |
| A clone of the repository, for the project skill | yes, where the checkout makes symbolic links |

Windows: with symbolic links off in git (`core.symlinks=false`), both links
check out as small text files. The plugin then installs with no reference,
and the project skill does not load. Before you clone or install from
GitHub, let git create symbolic links: enable Developer Mode (or run the
shell elevated), then clone with `git clone -c core.symlinks=true …`, or
set `git config --global core.symlinks true` so the clone the agent
harness makes gets it too. `cargo xtask check-plugins` reports such a file
as "symlink not checked out".

## Sources

- Plugin dependencies: https://code.claude.com/docs/en/plugins/dependencies
- Plugin and marketplace reference: https://code.claude.com/docs/en/plugins-reference
- Cloud environments: https://code.claude.com/docs/en/cloud-environments
- Skills: https://code.claude.com/docs/en/skills
- Sharing files within a marketplace with symlinks: https://code.claude.com/docs/en/plugins/host-marketplace#share-files-within-a-marketplace-with-symlinks
- Agent Skills specification: https://agentskills.io/specification

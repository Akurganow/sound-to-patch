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
  words/<axis>.json                    neutral word entries, one file per axis
  sources.json                         every source the dictionary cites
  probes.json                          one probe question per reference file
  examples.json                        the expected number of examples per synth
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
- a phrase that a synth folder must not hold: for Vital, the name of the
  other synth and version comparisons such as "older version";
- a reference file without a probe in `dictionary/probes.json`, and a
  probe answer that appears in no file or in more than one.

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
| A plugin folder under `.claude/skills/` as `<name>@skills-dir` | not listed by `plugin list` or `plugin details`; the docs say it loads after the trust prompt, in the session's own folder only. Not used |
| The linked project skill `.claude/skills/synth-reference` in a clean export of a commit | a headless session lists `synth-reference`, and its `reference` link resolves into `reference/` |
| Cloud sessions | not a target and not run. The docs say a committed project skill loads there and an installed plugin does not |

Which install paths carry `reference/`:

| Install path | Carries `reference/`? |
| :-- | :-- |
| `marketplace add Akurganow/sound-to-patch` and `install` (a copy from GitHub into the cache) | yes, checked: the link is copied as the folder |
| A release zip, through `--plugin-dir` or an `archive` source | yes, checked: `zip` stores the linked files, and the release checks it |
| `marketplace add <local folder>` and `install` | yes, checked: the folder is read in place, so the link resolves |
| A plugin copied from a local path, or a `command` source in its default copy mode | no: the docs say such a copy keeps only links inside the plugin's own folder |
| A clone of the repository, for the project skill | yes, where the checkout makes symbolic links |

Windows: with symbolic links off in git (`core.symlinks=false`), both links
check out as small text files. The plugin then installs with no reference,
and the project skill does not load. Enable symbolic links before you
clone or install from GitHub: turn on Developer Mode or use `mklink`, and
set `core.symlinks=true`. `cargo xtask check-plugins` reports such a file
as "symlink not checked out".

## Sources

- Plugin dependencies: https://code.claude.com/docs/en/plugins/dependencies
- Plugin and marketplace reference: https://code.claude.com/docs/en/plugins-reference
- Cloud environments: https://code.claude.com/docs/en/cloud-environments
- Skills: https://code.claude.com/docs/en/skills
- Sharing files within a marketplace with symlinks: https://code.claude.com/docs/en/plugins/host-marketplace#share-files-within-a-marketplace-with-symlinks
- Agent Skills specification: https://agentskills.io/specification

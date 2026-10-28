# Agent reference

The agent reference is an agent plugin in this repository,
`plugins/sound-to-patch-reference`. Its skill `synth-reference` tells an
agent which controls a synth has, by layer, with the preset key, the unit,
the range, the default, how sure each value is and how to tune it by ear.
The audio path and the text path will ship as plugins of their own that
depend on it.

## Layout

```text
.claude-plugin/marketplace.json        the marketplace: one entry per plugin
plugins/sound-to-patch-reference/
  .claude-plugin/plugin.json           name, description, licence; no version
  README.md
  skills/synth-reference/
    SKILL.md                           the router and the rules to keep
    reference/vital/<layer>.md         generated: one file per layer
    reference/vital/global.md          generated: controls outside the layers
    examples/vital/*.json              edit lists that CI applies
    examples/count.json                the expected number of examples per synth
dictionary/
  words/<axis>.json                    neutral word entries, one file per axis
  sources.json                         every source the dictionary cites
  probes.json                          one probe question per reference file
schemas/words.schema.json, schemas/sources.schema.json
```

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
- a phrase that a synth folder must not hold: for Vital, the name of the
  other synth and version comparisons such as "older version";
- a reference file without a probe in `dictionary/probes.json`, and a
  probe answer that appears in no file or in more than one.

The integration tests check the dictionary files against their schemas,
apply every example edit list to the built-in template of its synth, and
compare the number of examples with `examples/count.json`. A local test,
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

Checked on 2026-10-28 with two stub plugins in a scratch marketplace, in a
local session:

| Check | Result |
| :-- | :-- |
| `plugin validate` on the marketplace root and on each plugin folder | passes; it warns that no `version` is set, which is intended |
| `marketplace add <path>` and `install` | the plugin's skill loads |
| Installing a plugin whose `plugin.json` lists the other in `dependencies` | installs both; uninstalling it marks the other as no longer needed |
| A zip of the plugin folder, nested or flat, as `--plugin-dir` | loads the skill |
| A plugin folder under `.claude/skills/` (`<name>@skills-dir`) | not listed by the non-interactive commands; the docs say it loads after the trust prompt, in the session's own folder only |
| Cloud sessions | the docs say an installed plugin does not load there, while a committed project skill does |

Cloud sessions are not supported for now: the tool runs locally and
writes the preset files, and committed copies under `.claude/skills/`
would also load in every session that works on this repository.

## Sources

- Plugin dependencies: https://code.claude.com/docs/en/plugins/dependencies
- Plugin and marketplace reference: https://code.claude.com/docs/en/plugins-reference
- Cloud environments: https://code.claude.com/docs/en/cloud-environments
- Skills: https://code.claude.com/docs/en/skills
- Agent Skills specification: https://agentskills.io/specification

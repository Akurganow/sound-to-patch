# Architecture

## Purpose

sound-to-patch gives a producer a starting point in a synth for one sound.
Two synths are equal targets: Vital and Serum 2. On the audio path a stem
and its BPM become notes, a preset and an explanation. On the text path a
written description becomes a preset and an explanation. This file is the
map for finding the code that does a thing.

## Bird's-eye view

The audio path reads a stem, estimates eight synth layers in a fixed order
(notes and pitch, amplitude envelope, unison, oscillator, filter,
modulation, distortion, space), and writes a layer report. An agent session
reads the report, settles the flagged ambiguities and writes an edit list.
The `preset` code validates the edit list against a template preset of the
target synth and writes the preset file. The text path skips the analysis: the agent writes
the edit list from the description.

The library so far holds the command-line skeleton, the unit types and
the confidence type.

## Synth adapter seam

Estimators speak physical units, and adapters speak synth parameters. The
neutral core holds stem intake, notes, the estimators, the layer report,
compare and refine. It never rounds an estimate to what one synth offers.
Each synth has one adapter behind these traits. They are planned and not
yet in the code map.

- `PresetFormat` and `PresetModel`: read and write the file, name its header
  fields and supply the template through a `TemplateSource`.
- `ParamRegistry` and `ParamLaw`: the key, range and unit of each parameter,
  the law between the stored value and the physical unit, and a provenance
  tag (`from facts`, `measured` or `interim`).
- `SynthMapper`: maps layers to edits and returns an `unrepresentable` list
  with the residual for what the synth cannot reach.
- `RenderStateAdapter`: sets the plugin state in the VST3 host.
- The agent reference: the controls, the gotchas and the word-to-control
  maps of the synth.

`SynthCapabilities` holds what differs in structure: the oscillator
layout, the effect set, the LFO kinds, the glide and velocity rules and the
bend range. A `Synth` enum picks the adapter from `--synth vital|serum2`.
One contract test suite runs once per adapter. Vital is built first.

## Code map

- `cli`: the command-line definition. The binary parses it, and `xtask`
  renders the man page and the shell completions from the same tree.
- `units`: unit newtypes such as `Hz`, `Cents` and `Normalized`. Each is
  checked once when built, from a number, JSON or a unit string such as
  `"1.2 kHz"`. The conversions between units live here too.
- `confidence`: the confidence of an estimate: a level, the reasons with
  their signal values and a spread in a unit type, or no estimate.
- `src/lib.rs`: the crate root and the crate docs.
- `src/main.rs`: the binary. It parses arguments and calls the library.
- `xtask`: development tasks. It generates the man page and completions,
  fetches pinned datasets, checks the credits, this code map, the Rust
  version named in the docs and pull request descriptions, and writes the
  release version.

## Invariants

- The library never prints. Only the binary writes to the terminal.
- The binary holds no logic beyond parsing arguments and printing.
- No `unsafe` code. The workspace forbids it; code that hosts a plugin
  through FFI will live in a crate of its own.
- No test needs the network, a synth plugin (Vital or Serum 2) or a dataset
  unless it is `#[ignore]` with a reason.
- Estimators report physical units with a confidence. They never round a
  value to what one synth offers. Only an adapter speaks synth parameters.

## Layer boundaries

- Library: all logic, behind unit-typed public APIs.
- Binary: argument parsing and printing.
- `xtask`: tasks for developers and CI. Nothing in it ships.
- Tests: unit tests next to the code, one integration-test binary in
  `tests/it/`, command-line cases in `tests/cmd/`, fixtures in
  `tests/fixtures/` with their provenance.

## Cross-cutting concerns

- Lints: one table in the root `Cargo.toml`, applied to every member.
- Versions: `cog bump` sets the version from the commit history; `xtask`
  writes it into the manifest.
- Notices: `cargo-about` builds the third-party license file for the
  release from `about.toml` and `about.hbs`.
- Docs: the book in `docs/`, the API docs from rustdoc, both published
  together.

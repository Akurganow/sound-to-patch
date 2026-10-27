# Architecture

## Purpose

sound-to-patch gives a producer a starting point in a synth for one sound.
Two synths are equal targets: Vital and Serum 2. On the audio path a stem
and its BPM become notes, a preset and an explanation. On the text path a
written description becomes a preset and an explanation. This file is the
map for finding the code that does a thing.

## Bird's-eye view

The audio path reads a stem, estimates eight synth layers in a fixed order
(notes and pitch, amplitude envelope, unison, oscillator, filter, modulation,
distortion, space), and writes a layer report. An agent session reads the
report, settles the flagged ambiguities and writes an edit list. The `preset`
code validates the edit list against a template preset of the target synth and
writes the preset file. The text path skips the analysis: the agent writes the
edit list from the description.

The library so far holds the command line with `analyze`, `preset inspect`
and `preset apply`, the unit types, the confidence type, the synth seam,
the Vital read and write paths, stem intake, the note list with its masks
and its MIDI reader, the signal core that the estimators share, and the
report with the `stem` group and the spectrogram, the transcriber with its
profiles and its register detection, the f0 track, and the MIDI writer.

## Synth adapter seam

Estimators speak physical units, and adapters speak synth parameters. The
neutral core holds stem intake, notes, the estimators, the layer report,
compare and refine. It never rounds an estimate to what one synth offers.
Each synth has one adapter behind these traits. A fact that belongs to one
synth lives in its adapter, never in the neutral core.

- `PresetFormat` and `PresetModel` (`preset::format`): read and write the
  file, name its header fields and supply the template through a
  `TemplateSource` (`preset::template`).
- `ParamRegistry` and `ParamLaw` (`preset::registry`): the key, range and
  unit of each parameter, the law between the stored value and the
  physical unit, and a provenance tag (`from facts`, `measured` or
  `interim`). A calibration set replaces laws with measured tables.
- `SynthMapper` (`preset::mapper`): maps layers to edits and returns an
  `unrepresentable` list with the residual for what the synth cannot reach.
  The trait and the fitting helper exist; the layer groups join with the
  estimators.
- `RenderStateAdapter`, planned: sets the plugin state in the VST3 host.
- The agent reference: the controls, the gotchas and the word-to-control
  maps of the synth.

`SynthCapabilities` holds what differs in structure: the oscillator
layout, the effect set, the LFO kinds, the glide and velocity rules and the
bend range. A `Synth` enum picks the adapter from `--synth vital|serum2`;
without the flag, a preset's extension or Vital. One contract test suite
runs once per synth in `Synth::READY`. Vital is built first, and Serum 2
joins `READY` when its adapter lands.

## Code map

- `audio`: stem intake. It decodes a file at its native rate with every
  channel kept, mixes to mono as the channel mean and resamples for a
  consumer with a fixed rate. The `Stem` adds the beat grid and note
  divisions, the band edge, the flat-top flag and the stereo screen.
- `cast`: the audited numeric casts. Every `as` between number types goes
  through one function here, with its reason and a test.
- `cli`: the command-line definition, one module per command. The binary
  parses it, and `xtask` renders the man page and the shell completions
  from the same tree.
- `dsp`: the signal core: periodic windows, the centred STFT, the peak
  picker, active frames with median and interquartile range, the analytic
  signal with the heterodyne, decay fits by backward integration, the stem
  descriptors, the modulation spectrum with the division snap, and the
  Lomb-Scargle periodogram.
- `notes`: the note list that the transcriber and the MIDI reader produce
  and every layer reads. The transcriber runs the model once per stem and
  decodes that run with the settings of a profile or of the register and
  voicing detection; the f0 track feeds the detection and the note
  contours, and the quiet-note guard drops notes in near-silent passages.
  The MIDI writer stores a list as a format 0 file. It also holds the
  MIDI reader behind `analyze --notes` and the harmonic masks built from
  the list.
- `units`: unit newtypes such as `Hz`, `Cents` and `Normalized`. Each is
  checked once when built, from a number, JSON or a unit string such as
  `"1.2 kHz"`. The conversions between units live here too.
- `confidence`: the confidence of an estimate: a level, the reasons with
  their signal values and a spread in a unit type, or no estimate.
- `preset`: preset files. `format`, `registry` and `template` hold the
  synth-neutral traits and the `inspect` command. `edits` parses an edit
  list and resolves it against a registry, `apply` writes resolved edits
  into a model, and `mapper` holds the mapper trait. `vital` is the Vital
  adapter: the JSON codec, the registry built from `registry.json`, the
  built-in template, the writers of routes, wavetables and LFO shapes, the
  capabilities and the laws that span controls.
- `report`: the `analyze` pipeline and its outputs. `layers` writes
  `layers.json`, whose committed schema is `schemas/layers.schema.json`,
  and `spectrogram` draws the two-panel spectrogram PNG.
- `synth`: the `Synth` enum, the shared `--synth` argument and
  `SynthCapabilities`.
- `src/lib.rs`: the crate root and the crate docs.
- `src/main.rs`: the binary. It parses arguments and calls the library.
- `xtask`: development tasks. It generates the man page and completions,
  fetches pinned datasets, runs the pinned golden-number generator and
  checks its vectors, scores the notes step on generated parts, checks
  the lock file for the ONNX Runtime crate, checks the credits, this code
  map, the Rust version
  named in the docs and pull request descriptions, writes the release
  version, regenerates the Vital registry data from a local checkout of
  the open source, and generates the Vital test fixtures.
- `fuzz`: the fuzz targets, a workspace of its own on a pinned nightly.
- `tools/vita`: the load check that loads written presets in the open Vital
  engine. It runs only as a separate Python process with `vita`.

## Invariants

- The library never prints. Only the binary writes to the terminal.
- The binary holds no logic beyond parsing arguments and printing.
- No `unsafe` code. The workspace forbids it; code that hosts a plugin
  through FFI will live in a crate of its own.
- No test needs the network, a synth plugin (Vital or Serum 2) or a dataset
  unless it is `#[ignore]` with a reason.
- Estimators report physical units with a confidence. They never round a
  value to what one synth offers. Only an adapter speaks synth parameters,
  and a mapper picks the nearest option and reports the residual.
- A registry law never rounds. A stored value that must be whole and is
  not is an error.
- A model keeps every key it read, unknown keys included, so a rewrite
  loses no feature of a newer build.

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

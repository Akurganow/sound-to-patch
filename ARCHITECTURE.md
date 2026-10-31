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

The library so far holds the command line with `analyze`, `notes`,
`preset inspect`, `preset apply` and `render`, the unit types, the
confidence type, the synth seam, the Vital read and write paths, stem
intake with the tempo refinement, the note list with its masks and its
MIDI reader, the signal core that the estimators share, and the report
with the `stem` group and the spectrogram, the transcriber with its
profiles and its register detection, the f0 track, the note cleanup, the
MIDI writer, the layer map of the agent reference, and the render core:
the recipe, the renderer trait, a fake renderer, the Vital plug-in state,
the renderer that drives the plug-in host, the load check, the render
session, the timing table, the repeat-render spread and the `render`
command. The reference itself is the folder `reference/`, which an agent
plugin and a project skill both read.

## Synth adapter seam

Estimators speak physical units, and adapters speak synth parameters. The
synth-neutral part of the library holds stem intake, notes, the
estimators, the layer report, compare and refine. It never rounds an
estimate to what one synth offers. Each synth has one adapter behind
these traits. A fact that belongs to one synth lives in its adapter,
never in the synth-neutral part.

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
- `preset_state` (`render::state`): builds the plug-in state of a preset
  for the VST3 host. Vital's is the preset JSON with the recipe's
  oversampling and a tuning, followed by a NUL. A preset without a tuning
  gets Vital's default, because the plug-in keeps its last tuning when a
  state has none.
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
  consumer with a fixed rate. The `Tempo` refines the given BPM from the
  onsets. The `Stem` adds the beat grid and note divisions, the band
  edge, the flat-top flag and the stereo screen.
- `cast`: the audited numeric casts. Every `as` between number types goes
  through one function here, with its reason and a test. It lives in
  `core`.
- `cli`: the command-line definition, one module per command. The binary
  parses it, and `xtask` renders the man page and the shell completions
  from the same tree.
- `dsp`: the signal core: periodic windows, the centred STFT, the peak
  picker, active frames with median and interquartile range, the analytic
  signal with the heterodyne, decay fits by backward integration, the stem
  descriptors, the modulation spectrum with the division snap, and the
  Lomb-Scargle periodogram.
- `estimate`: the layer estimators that read the note list and the f0
  track. `pitch` holds the pitch layer of a monophonic line: the sub-band
  line, pitch drops at the strike of a low note, glides between notes and
  vibrato, each record with its own confidence. `envelope` holds the
  envelope layer: per-note level tracks, the attack, hold, decay, sustain
  and note-off knee pooled over the notes, and the sidechain pump read on
  the beats that a held note covers.
- `notes`: the note list that the transcriber and the MIDI reader produce
  and every layer reads. The transcriber runs the model once per stem and
  decodes that run with the settings of a profile or of the register and
  voicing detection; the f0 track feeds the detection, the note contours
  and the octave check. The cleanup moves octave copies onto their notes,
  merges them, joins split notes and drops notes in near-silent passages
  with the quiet-note guard. `transcribe_stem` runs those steps in order
  for one stem. The MIDI writer stores a list as a format 0 file. It also holds the
  MIDI reader behind `analyze --notes` and the harmonic masks built from
  the list.
- `music`: the music dictionary for notes. It embeds the topic files of
  `dictionary/music/` and parses them once, checks the rules every entry
  meets, and holds the key, note-name, Roman-numeral and chord-symbol
  parsers, Camelot and Open Key, the snap rule, Euclidean patterns, swing
  and tick arithmetic. The binary reads nothing else from the dictionary.
- `words`: the word dictionary for sounds. It embeds the axis files of
  `dictionary/words/` with the listener numbers, the intensity ladder and
  the style and technique lists, and parses them once. Its check
  recomputes each agreement class and few-sample flag from the stored
  numbers and matches every number's tag to its source count. It picks a
  word's sense by role or by default and writes the word's question.
- `units`: unit newtypes such as `Hz`, `Cents` and `Normalized`. Each is
  checked once when built, from a number, JSON or a unit string such as
  `"1.2 kHz"`. The conversions between units live here too. The module
  lives in `core`, and the library re-exports it.
- `confidence`: the confidence of an estimate: a level, the reasons with
  their signal values and a spread in a unit type, or no estimate.
- `preset`: preset files. `format`, `registry` and `template` hold the
  synth-neutral traits and the `inspect` command. `edits` parses an edit
  list and resolves it against a registry, `apply` writes resolved edits
  into a model, `mapper` holds the mapper trait, and `catalogue` places
  each control in a layer of the agent reference. `vital` is the Vital
  adapter: the JSON codec, the registry built from `registry.json`, the
  built-in template, the writers of routes, wavetables and LFO shapes, the
  capabilities, the laws that span controls and the layer map of its
  controls.
- `render`: rendering a preset and its notes to audio. `recipe` holds the
  versioned render recipe and the tail rule that sets the clip length,
  `fake` a seeded additive synth that stands in for a plug-in in tests,
  `state` the plug-in state of a preset and the check of a saved state,
  `check` the load check against the plug-in's init patch, `file` the
  `render` command's run from files to a WAV file and a record,
  `wire` the protocol of the host process, `plugin` the renderer that
  starts that process and picks its architecture from the plug-in bundle,
  `vita` the open engine for single notes, `session` the worker pool that
  every consumer renders through, `timing`
  the table of load and render times, `spread` the repeat-render spread,
  its sequential draws and the comparison against it, and the module
  root the render job, the record and the `Renderer` trait. `recipe`,
  `wire`, the notes and the note events live in `core`.
  The record's committed schema is `schemas/render-record.schema.json`.
- `report`: the outputs of a stem in its output folder: the `analyze`
  pipeline with `layers.json` and the spectrogram, and the `notes` command
  with `notes.mid`. `layers` writes `layers.json`, whose committed schema
  is `schemas/layers.schema.json`, and `spectrogram` draws the two-panel
  spectrogram PNG.
- `synth`: the `Synth` enum, the shared `--synth` argument and
  `SynthCapabilities`, with the octave label each synth gives MIDI note
  60.
- `src/lib.rs`: the crate root and the crate docs.
- `src/main.rs`: the binary. It parses arguments and calls the library.
- `core`: the crate `sound-to-patch-core`, the render core that the
  library and the host share: `units`, `cast`, the audio types and the
  WAV writer, the MIDI velocity, the `Synth` enum, the notes of a
  render, the kinds of controller change and the frame events, the
  recipe and `render::wire`. The library re-exports each at its own
  path, such as `sound_to_patch::units` or
  `sound_to_patch::render::Recipe`. It depends on `serde` and
  `serde_json` only, and on `clap` for the library's command line.
- `host`: the crate of `sound-to-patch-host`, the process that loads a
  VST3 plug-in headless through `vst3-host` and renders through it. Only
  it depends on `vst3-host`. It speaks `render::wire` and uses the core
  crate's recipe, note events and tail watch, so both sides render by
  one rule. It depends on `core` and not on the library, so it links
  none of the transcriber or the analysis. It points file descriptor 1
  at standard error before it loads a plug-in and writes replies to the
  saved standard output, so plug-in output never mixes with a reply. A
  render holds the state several times over: the preset, its JSON, the
  base64 request and the host's copies. Memory peaks at about 20 times
  the preset size, about 800 MB for a 40 MB preset.
- `xtask`: development tasks. It generates the man page and completions,
  fetches pinned datasets, runs the pinned golden-number generator and
  checks its vectors, scores the notes step on generated parts, checks
  the lock file for the ONNX Runtime crate, checks the credits, this code
  map and the Rust version named in the docs and pull request
  descriptions, writes the release version, regenerates the Vital
  registry data from a local checkout of the open source, generates
  the Vital test fixtures, writes the catalogue files of the agent
  reference from the registry and the music and word files from their
  dictionaries, recomputes the listener agreement of the sound words
  from the pinned listener data, runs the chord oracles of the music dictionary with the
  golden tools, and checks the agent plugins.
- `reference`: the agent reference, one folder of plain files: the router
  `README.md`, the generated catalogue files per synth and layer, the
  generated music and word files under `core/`, and example edit lists. Nothing
  else holds a copy.
- `plugins`: the agent plugins, each folder shipped as its own release
  archive and listed in `.claude-plugin/marketplace.json`.
  `sound-to-patch-reference` holds the skill `synth-reference`, which
  links `reference/`. The plugin also holds its eval cases under
  `evals/`. `.claude/skills/synth-reference` links that skill, so a
  session on a clone of the repository finds it too.
- `dictionary`: the data of the agent reference: the music entries per
  topic with their stated shares and planned counts, the word entries per
  axis with the derived listener numbers, the intensity ladder, the style
  and technique lists, the source list and the probe questions, with their schemas in
  `schemas/`.
- `fuzz`: the fuzz targets, a workspace of its own on a pinned nightly.
- `tools/vita`: the load check that loads written presets in the open Vital
  engine, and the script that renders one note in it for
  `render::vita`. Both run only as a separate Python process with `vita`,
  from the hash-pinned `requirements.txt`.

## Invariants

- The library never prints. Only the binary writes to the terminal.
- The binary holds no logic beyond parsing arguments and printing.
- No `unsafe` code. The workspace forbids it; the FFI of plug-in hosting
  stays inside the `vst3-host` and `rustix` dependencies of the `host`
  crate.
- No test needs the network, a synth plugin (Vital or Serum 2) or a dataset
  unless it is `#[ignore]` with a reason.
- Estimators report physical units with a confidence. They never round a
  value to what one synth offers. Only an adapter speaks synth parameters,
  and a mapper picks the nearest option and reports the residual.
- A registry law never rounds. A stored value that must be whole and is
  not is an error.
- A model keeps every key it read, unknown keys included, so a rewrite
  loses no feature of a newer build.
- Many renders go through `render::session`, the one worker pool. A
  consumer starts no pool of its own and adds no retry or flush. A render
  that fails twice is a `RenderFailure` with a large finite score, never a
  panic or an aborted run.

## Layer boundaries

- Library: all logic, behind unit-typed public APIs.
- Binary: argument parsing and printing.
- Core: `core/`, the render core. It holds no logic that only the
  library needs, so the host stays small.
- Host: `host/`, the plug-in host process. A plug-in crash or hang ends
  that process, never the caller: the renderer kills a host that stops
  answering, and stops one that lingers after its pipes close or after
  `quit`. The renderer refuses a host whose protocol or recipe version
  differs from its own.
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

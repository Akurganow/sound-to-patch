# Intent

## Problem

Suno turns an idea into a draft track and exports stems. Rebuilding a stem
by hand in a DAW is slow. The producer has to transcribe the notes by ear.
Then they have to design a similar sound in Serum 2. Without years of sound
design experience, the second step is the hardest part.

## Goal

Give the producer a starting point for one stem: the notes and a Serum 2
preset, plus an explanation of how the preset works.

## Input

One stem of one instrument exported from Suno, for example the whole Bass
track, plus the BPM that Suno reports. Supported material:

- bass;
- leads and plucks;
- pads and chords, including polyphony.

## Output

A folder next to the stem with:

- `notes.mid`: the notes of the whole stem, chords included, ready for a
  DAW piano roll. One track, tempo from the BPM. Each note ends at its
  estimated note-off. A glide is two overlapping notes. Pitch drops and
  vibrato are left to the preset. Pitch bends are written only for motion
  the preset cannot model, with a range of two semitones.
- `preset.SerumPreset`: a Serum 2 preset that starts close to the sound.
- `explanation.md`: one section per synth layer. It states what was
  measured, what the preset sets, how sure the estimate is, which controls
  to turn next and what the tool could not tell.
- `layers.json`: the layer report that the agent read. It stays next to the
  stem as a record of what was measured.

## Success

- The preset reproduces each synth layer that the audio can identify: pitch
  behaviour, oscillator, unison, filter, amplitude envelope, modulation,
  distortion and effects. Each layer states how sure it is. The producer
  finishes the preset by hand.
- The explanation teaches which controls shape the sound.
- The MIDI drops into the piano roll without transcription by ear.

## Approach

A Rust command-line toolkit does the deterministic work. It splits the
stem into eight synth layers and estimates each one. An agent session reads
the layer report, settles the ambiguities that the toolkit flags and
writes the preset edits. The project trains no models of its own.

The layers follow the signal path: pitch and glide, oscillator source,
unison and width, filter, amplitude envelope, modulation, distortion, and
space and FX. The estimation order differs. Notes and pitch come first, then
the amplitude envelope, because reverb needs the release. Unison,
oscillator, filter, modulation, distortion and space follow. Each layer
reports values with units, a confidence, and the parameters that only a
render can calibrate.

The fact base behind every decision below lives in `research/`, outside
git: one fact file per studied project or paper, one research file per
layer, plus syntheses with the contradictions and how to settle them. The
layer synthesis is `research/synthesis/layers.md`.

### `analyze`

Estimates the eight synth layers. Writes the layer report as JSON
(`layers.json`) and a spectrogram PNG.

- Each layer writes values with units, a confidence, flags, the parameters
  that need a render to calibrate, and a suggested edit in display units.
- Pitch and glide: f0 track, glides, pitch envelope, vibrato, mono or poly.
- Oscillator source: waveform family, noise, sub, sync and FM flags, table
  candidates.
- Unison and width: voice offsets in cents, relative levels, stereo
  statistics, the source of the width.
- Filter: equivalent cutoff, slope, resonance, key tracking, cutoff
  movement.
- Amplitude envelope: attack, hold, decay, sustain, note-off, release, tail,
  pump, velocity spread.
- Modulation: LFO rate mapped to the nearest note division at the given
  BPM, mode, shape, target.
- Distortion: evidence of a nonlinearity and candidate families.
- Space and FX: reverb time, delay division and feedback, dynamics and EQ
  hints.
- Global descriptors (spectral centroid, rolloff, flatness, band energy
  shares, crest factor) stay in a `stem` group. Centroid and attack time are
  the best-supported timbre descriptors. The layer estimators are proposals
  to validate, and each layer carries a confidence field with its signals.
- One documented convention per number, named in the JSON. Spectral shape
  uses magnitude weights, band shares use power. Conventions in the
  literature differ by up to 15x on the same sound.
- `analyze` reads the stem at its native sample rate. Harmonic analysis
  uses frames of 8192 samples when the fundamental is below 100 Hz, else
  4096. Time-resolved tracks use their own windows: RMS over two
  fundamental periods for amplitude, a 200 Hz hop for modulation.
- `analyze` needs the note list. It runs the Basic Pitch transcription once
  and shares the result with `notes`.
- The agent gets the numbers as text. The spectrogram is for coarse shape
  only: no study shows that models read timbre from images, and centroid
  cannot be recovered from an audio embedding.

### `notes`

Transcribes the stem to MIDI with the `basic-pitch-rs` crate. It bundles
the Basic Pitch model and runs it on `tract`, so the binary needs no ONNX
Runtime library.

- The crate takes mono 22,050 Hz f32 samples. Our code decodes with
  `symphonia` and resamples with `rubato`. We write the single-track
  `notes.mid` with `midly` ourselves.
- Pin the exact version (`=0.1.0`). Never enable its `ort` features: the
  crate pins `ort` to an old release candidate.
- One model run per stem, then `decode_notes` once per profile.
- One profile per stem type: pitch range, minimum note length, melodia and
  bend settings.
  - Store ranges as MIDI pitches. The maximum frequency bound is exclusive
    after rounding to a semitone.
  - The effective minimum note length is one frame longer than configured
    (127.7 ms gives 139 ms). Fast bass and lead lines need a shorter value
    derived from the BPM.
- After `analyze`, `notes` writes the final `notes.mid` from the layer
  report:
  - a glide becomes two overlapping notes, with at least 5 ms of overlap
  - a pitch drop and vibrato leave the notes and become preset routes
  - each note ends at its estimated note-off, not at the transcriber's end
  - velocity comes from the measured note level and varies only when the
    preset has a velocity route
  - ghost notes (octave-up copies and split notes, seen at 110 Hz and
    below) are removed, and the octave check corrects flagged pitches
  - the minimum note length derives from the BPM
- The f0 track for glides and drops comes from `pyin` and the sub-band
  Hilbert line. CREPE through `tract` joins if the bench shows a gain. Basic
  Pitch bends no longer carry glides.
- Known limits: no notes below A0 (27.5 Hz). Bends have a resolution of a
  third of a semitone, are clipped at two semitones and are dropped for
  overlapping notes, so the default writes no bends. In-tune notes read one
  bin sharp unless `bend_offset = -1`, pending a check. Note ends fall
  late: by the release time plus 25 to 60 ms on dry material, and past
  1.5 s with reverb. Expect octave errors on bass and, at 110 Hz and
  below, extra octave-up notes.

### `preset`

Reads and writes `.SerumPreset` files and applies an edit list to an Init
preset.

- Layout: `XferJson\0`, JSON metadata length (u64 LE), JSON metadata,
  uncompressed CBOR size (u32 LE), encoding flag (u32 LE, always 2 = zstd;
  reject other values), one zstd frame of CBOR.
- The metadata `hash` is the MD5 of the compressed bytes in files saved by
  Serum 2.0.19 and later. Recompute it on write. A mismatch on read is
  reported, not an error: Serum loaded files with a stale hash in three
  reports.
- Start every preset from an Init preset saved by Serum 2 itself. Change
  `plainParams`, mod slots, FX racks, wavetable fields, macro names and the
  name fields.
- The Init preset may fall under the Xfer EULA, so it never enters the
  repository. The tool reads it at run time from the user's local Serum 2
  preset folder, or from a path given on the command line. If the file is
  missing, the tool stops and says where it looked.
- Never write a parameter at its default value. An absent key and an
  explicit default sound different. No official table of defaults exists.
- Sort CBOR map keys bytewise, as Serum does. Write every `plainParams`
  number as float64, integers only for counts and ids. A native CBOR bool
  in `plainParams` crashed a DAW.
- `ciborium` narrows floats to f16 or f32 on write, and Serum never writes
  f16. Read with `ciborium`, write with our own small encoder. Check one
  written file in Serum 2 before trusting the writer.
- Reference factory wavetables by relative path. Copy `numFrames`,
  `numChannels` and `sampleRate` from a Serum-saved preset.
- Modulation source ids: serum2gen's table is wrong. serum2vital and
  serum-mcp agree (Mod Wheel 1, Env 1-4 = 2-5, Velocity 16, Note 17).
  Re-verify one probe preset on the current Serum 2.
- The voice-filter cutoff is a 0..1 knob position, not Hz.
- Layer estimates arrive in physical units (Hz, cents, octaves, ms). The
  writer converts them to knob positions through calibration tables
  measured on a render. Until a table exists, the writer uses the interim
  law, marks the parameter uncalibrated, and offers no Hz constructor for a
  knob without a measured law.
- Keys not yet known: unison blend, range, mode and stack, the
  per-oscillator ENV 1 button, DC bias in Distortion, the multiband
  compressor controls and the extra reverb controls. Save one Serum-built
  probe preset per control and diff the CBOR.
- Crates: `zstd` 0.14 (BSD-3-Clause), `ciborium` (read only), `md-5` (no
  hex formatter, format bytes ourselves), `serde_json`.
- Every format fact records the Serum build it was verified on. The
  metadata version changed between builds (4.0, 7.0, 9.0, 10.0).

### Agent session

1. The producer hands a stem and its BPM to the agent.
2. The agent runs `analyze`, which also runs `notes`.
3. The agent reads `layers.json` as numbers and uses the spectrogram only
   for coarse shape. It settles each flagged ambiguity, accepts or
   overrides the suggested edits of each layer, and writes an edit list in
   display units with a reason per edit.
4. `preset` validates the edit list against the Init preset and writes the
   preset. An unknown name or an out-of-range value returns a message with
   the fix.
5. The agent writes `explanation.md` from the reasons, one section per
   layer.

A Claude Code skill in this repository describes the loop. The agent does
not classify the stem into a style.

- The skill holds one file per layer: what the report fields mean, the
  Serum parameters the layer fills (name, unit, range), the rules for the
  layer's ambiguities, the defaults for low confidence and the controls to
  turn next. The agent loads a layer file when the layer is flagged,
  uncertain or ambiguous.
- Style cards (reese, neuro, wobble, 808, pluck, lead, pad) are optional
  priors. They give typical values for parameters that a layer cannot
  identify, such as filter drive, distortion mode and effect order. The
  report decides which blocks are on and which knobs `refine` searches. A
  card never does.
- Each layer has two worked examples: a report excerpt, the resolved edits
  and the reasons. One of the two is an ambiguity case. Two full examples
  cover all eight layers.
- Evidence: the only published test of in-context help for an LLM setting
  parameters (LLM2Fx, text to EQ and reverb) found examples gave the
  largest gain on reverb. No result exists for synth presets or audio
  input, so the test bench measures it.
- Recipes from articles are retold in our own words, never copied.

## Licensing of reused work

- Code and weights under MIT, Apache-2.0, BSD, ISC, CC0 or CC BY may be
  reused with attribution.
- GPL, LGPL, AGPL and MPL-2.0 projects serve only as sources of facts, or
  run as a separate process. An MPL-2.0 crate may be a dependency if we do
  not modify its files.
- Weights under a non-commercial license (CC BY-NC) are inspiration only.
- Code without a license may be read, not copied.
- README and `docs/` credit every project or paper we studied as
  inspiration, whatever its license. A credit gives the name, a link to the
  repository or paper, and the license. Projects under a non-permissive
  license or with no license carry the note "inspiration only".
- Crates we install are dependencies, not inspiration. They stay out of
  the credits.
- License notices of the dependencies ship with the binary as
  `THIRD-PARTY-LICENSES.html`, generated by `cargo-about` for
  `aarch64-apple-darwin` from the normal dependencies only:
  - the Basic Pitch `NOTICE` for the bundled model, added as static text
    because `cargo-about` does not see it;
  - the BSD-3-Clause `zstd` notice;
  - the MPL-2.0 text for `symphonia`, which we use unmodified;
  - `clarify` entries for crates whose package has no license file.
- Test fixtures taken from other projects keep their license text in
  `tests/fixtures/PROVENANCE.md`. Tools that only build, test or document
  the project do not ship and need no notice.

## Dependencies

- Every new dependency starts at its latest stable release. A pre-release
  (alpha, beta, rc) is not stable. A crate with only pre-releases needs the
  owner's decision.
- Pre-1.0 crates that change fast (`basic-pitch-rs`, `vst3-host`,
  `plughost`) get an exact pin (`=x.y.z`).
- Exception: when a dependency pins another crate exactly, we do not add
  that crate ourselves.
- Dev-dependencies follow the same rules and raise `rust-version` like any
  other dependency. Installed tools (nextest, cargo-fuzz and the like) do
  not.
- GitHub Actions are pinned by commit SHA. Dependabot alerts and security
  updates are on, and a Dependabot entry for `github-actions` keeps the
  pins current.
- Open question: when to enable Renovate. Not at the start, only once the
  project is mature enough.

## Maintenance

- Later, once the project is mature enough: put the repository under
  [Lawman](https://github.com/Akurganow/lawman) patrol by adding its
  constitution file, `LAWMAN.md`.

## Out of scope now

- Training neural networks.
- Rendering presets to audio, and everything that depends on it.
- Tempo detection: Suno already reports the BPM.
- Drums.
- Cloud execution of the toolkit. It runs locally on macOS. The agent is a
  hosted model and receives only the layer report and the spectrogram,
  never the audio.

## Later

- `render`: render a preset with MIDI to WAV through Serum 2.
  - Serum 2 ships as VST3, AU and AAX, with no CLAP build.
  - Load the preset through a VST3 processor state: the preset CBOR with
    the preset-only keys removed, `"default"` sentinels replaced by `{}`,
    processor markers added, rewrapped as XferJson. JUCE hosts need an
    extra `VC2!` envelope. Raw preset bytes fall back to the init patch,
    and the host still reports success, so check the render against an
    init render.
  - Try the native `vst3-host` crate first, then `plughost`, then
    DawDreamer as a Python subprocess (the only route proven on Serum 2).
  - Renders are not deterministic, and batch renders carry state between
    presets. Render 0.5 s of silence after each load and a warm-up render,
    then compare with tolerances.
  - Pad the MIDI with release time: hosts cut the render at the requested
    length.
  - Rendering needs a Serum 2 authorized on the machine, so CI cannot do it.
- Calibration set, the first use of `render`. Measured once per Serum build.
  It settles the knob laws and route depths that the layer report marks
  uncalibrated: cutoff law per filter type, resonance, key tracking, mod
  amount per octave, pitch-route depth, portamento time and curve, ENV 1
  curve shapes and sustain scale, unison detune in cents, blend and pan law,
  reverb size to RT60, delay time scalar, compressor threshold, distortion
  curves, and the probe-preset questions of the layer synthesis.
- `compare`: return named distances, not one scalar: MFCC (40 coefficients,
  cosine over the full frame map), multi-scale spectral loss, envelope
  distance, descriptor deltas and pitch contour, with an optional CLAP
  distance. On human timbre ratings MFCC is the best simple baseline
  (triplet agreement 0.724, spectral loss 0.661, chance 0.5). No single loss
  wins on every synth, and within one system the metrics do not track
  listeners, so scores never replace listening. Exclude or level-match the
  first MFCC coefficient: it carries loudness.
- `compare` also runs the layer estimators on the target and on the render
  and reports the difference of the two layer reports. The per-layer
  difference shows which layer is wrong.
- `refine`: improve a starting preset with CMA-ES (`fcmaes-core`, ask/tell
  with bounds) over render and compare. It searches the parameters that the
  layer report marks as needing a render or as low confidence, 12 to 24 at
  a time, inside bounds set by the report's own uncertainty, with pitch
  fixed from `notes`. It runs in pipeline order, with the FX parameters
  last and the synth fixed. It scores audio only, on two or three
  representative notes, and stops on a budget of about 10,000 evaluations
  or when the score stops improving beyond the render noise floor. Without
  an agent it starts from the suggested edits of the report. Wide bounds
  made a published search diverge.
- A local test bench in four tiers. It compares audio rendered from the
  result with the target, never parameters.
  - T0: render determinism, the noise floor and the calibration set.
  - T1: Serum 2 factory presets and random perturbations of them, where a
    perfect answer exists. The perturbation log is the ground truth. Each
    layer estimator also reports its own metric here.
  - T2: audio from other synths (the Surge and NSynth sets), where no
    perfect answer exists.
  - T3: real Suno stems, judged by metric and by ear.
  - The agent runs in arms: full report, numbers only, image only, report
    without confidences, layers hidden, wrong layers, no priors, no
    examples, a no-agent baseline built from the suggested edits, and floor
    baselines. The layers-hidden arm gives the stem type and the priors
    only.
  - No public dataset pairs Serum 2 presets with audio. Factory presets and
    renders stay out of the repository.
- Our own labelled dataset of Serum 2 presets and renders. No public one
  exists: a sweep of Hugging Face, GitHub, GitLab, Zenodo, Kaggle and paper
  data sections found none. It is built from generated presets and, for
  local use, the owner's factory presets. The generation recipe and render
  settings are recorded with it.
  - The dataset may be public. It is published as a versioned package tied
    to this repository, for example an OCI artifact on the GitHub Container
    Registry, and `xtask` fetches it by version and checksum. The choice of
    registry and its size limits need a check.
  - Only the generated part is published by default. Factory presets and
    their renders stay local until the Xfer license terms are checked.
- Parser fixtures without audio: `RecklessSerum2Bank` (MIT, 250 generated
  `.SerumPreset` files) may enter `tests/fixtures/` with its license.
- Several different sounds in one stem produce several presets.
- Splitting a full Suno track into clean stems.

## Delivery

The first stage after the research is a decomposition into epics.

- Each epic has a deliverable that can be tested on its own.
- Each epic picks the test layers it needs. The layers run from compiler
  lints and unit tests up to labelled datasets. Some epics need unit tests
  only, others the full stack. The layer matrix lives in `docs/`.
- Tests follow Rust community layout:
  - unit tests in a `#[cfg(test)] mod tests` next to the code;
  - one integration-test binary in `tests/it/`, fixtures in
    `tests/fixtures/`;
  - CLI cases as declarative files in `tests/cmd/`;
  - doc tests in rustdoc examples, runnable examples in `examples/`;
  - snapshot tests for the shape of our own outputs;
  - property tests where an invariant exists (round trips, unit
    conversions);
  - golden-number tests with explicit tolerances for numbers that must
    match an outside reference;
  - fuzz targets in `fuzz/` for code that reads untrusted bytes;
  - benchmarks in `benches/`;
  - dataset evaluations in `xtask/`, outside CI when they need Serum 2 or
    large data.
- A test that needs Serum 2 or a dataset is `#[ignore]` with a reason and
  fails loudly when its input is missing. Cargo features never gate tests.
- Test data is generated in code first. Committed fixtures are small and
  listed with origin and license. `xtask` downloads datasets into an
  ignored directory. Serum factory presets, the Init preset, their renders
  and Suno stems never enter the repository.
- Each epic closes with its documentation: crate and item rustdoc, README
  sections, pages in `docs/`, `CHANGELOG.md` through cog, examples. An epic
  that adds a command also regenerates the CLI reference, man page and
  shell completions.
- `docs/` is an mdBook. It holds the user guide, CLI reference, the
  `.SerumPreset` format spec, architecture, per-layer method notes with
  their formulas, and the testing guide.
- Tracking lives on GitHub:
  - an issue per epic, with sub-issues for tasks and subtasks;
  - `blocked by` links for the order between epics;
  - a milestone per epic;
  - labels for kind, area and layer (issue types need an organization, so
    they wait);
  - a GitHub Project board across epics.

## Engineering

- The project is an application, not a library for others. It is never
  published to crates.io: `publish = false` in every `Cargo.toml`. The
  binary ships through GitHub Releases, and the API docs go to GitHub Pages.
- Golden numbers for metric tests come from a pinned, dev-only Python
  script in `xtask/`. It runs once per reference change, and its numbers
  are committed as JSON. CI never needs Python.
- Agent-written changes follow `CONTRIBUTING.md`: the pull request says an
  agent wrote it and shows the verification commands with their output.
  The owner reviews every such pull request before merge.
- Rust, edition 2024, MIT license. `rust-version` is the highest MSRV among
  the dependencies, dev-dependencies included (1.91 now).
- The repository is public.
- Conventional Commits. A `commit-msg` hook from `cog install-hook` checks
  local commits. CI checks the pull request title, because squash merge
  turns it into the commit subject.
- `main` takes pull requests only, behind a ruleset with required checks.
- Squash is the only merge method.
  - The repository setting `squash_merge_commit_title` is `PR_TITLE`. The
    pull request title becomes the commit subject and the release name.
  - The required `commits` check validates the pull request title with
    `cog verify`.
  - Its workflow triggers on `opened`, `edited`, `synchronize` and
    `reopened`. A title changed after a green run is checked again.
  - GitHub appends ` (#N)` to the squash subject. Verify on the live
    repository that `cog verify` accepts the suffix.
  - A ruleset `commit_message_pattern` rule requires Conventional Commits
    on the server, as a guard behind the check.
- CI on pull requests and pushes, one job per check, no path filters:
  `fmt`, `clippy`, `test` (nextest plus `cargo test --doc`), `doc`,
  `commits`, `licenses`, `docs` once `docs/` exists, and CodeQL as an
  advanced workflow (`Analyze (rust)`).
- Mutation tests, coverage, fuzzing and property soak run on a schedule and
  at epic close. They report and do not gate.
- Release on push to `main`, after the checks pass: `cog bump --auto`
  updates `CHANGELOG.md` and tags the version. A deploy key pushes the
  version commit. A commit-prefix guard stops the loop. No bump means no
  release. A release publishes a GitHub Release with a macOS arm64 binary,
  the man page, shell completions and `THIRD-PARTY-LICENSES.html`. Releases
  are immutable.
- The book in `docs/` and the API docs deploy to GitHub Pages from `main`.

### Documentation rules

- README, `docs/`, rustdoc, code comments, commit messages and pull request
  descriptions name no third-party brands except Serum.
- Credits name the projects and papers that inspired the work, as the
  Licensing section allows. A credit is not a brand mention.
- `INTENT.md` is the only file that names Suno. It is deleted when the work
  is complete, so nothing links to it.

### Strict code

Strict types and strict lints are a hard requirement. Agents write most of
the code, and agent output varies, so the compiler and linters must catch
the typical mistakes.

- Types carry units. Public APIs take newtypes such as `Hz`, `Seconds`,
  `Semitones`, `Cents`, `Octaves`, `Decibels`, `Percent` and `Normalized`
  (0..1), never a bare `f32`. The research found unit traps: the
  voice-filter cutoff is a 0..1 knob position, while FX frequencies are in
  Hz. Every estimate carries a `Confidence` with its value, spread and
  reason codes.
- Parse, don't validate: values are checked once at the boundary (file,
  CLI, JSON) and are valid by type afterwards.
- Lints sit in `[workspace.lints]` in the root `Cargo.toml`, and each member
  sets `lints.workspace = true`, so every build applies them:
  - rustc: `unsafe_code = "forbid"`, `missing_docs = "deny"`,
    `unreachable_pub = "deny"`;
  - rustdoc: `broken_intra_doc_links`, `private_intra_doc_links` and
    `missing_crate_level_docs` denied;
  - Clippy: `all`, `pedantic` and `too_long_first_doc_paragraph` denied;
    `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` and
    `dbg_macro` denied outside tests. `clippy.toml` allows `unwrap`,
    `expect` and `panic` in tests. Examples and benches return `Result`.
- `forbid` cannot be lowered in an inner module. Plugin-hosting code that
  needs `unsafe` for FFI lives in its own crate. That crate sets `deny` and
  allows `unsafe` on its FFI module only, with a documented reason. The
  core crate keeps `forbid`. If a safe wrapper crate is enough, the host
  crate keeps `forbid` too.
- Cargo features are additive. None of them enables the `ort` features of
  `basic-pitch-rs`.
- CI fails on any warning: `cargo clippy --all-targets --all-features -- -D
  warnings` and `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`, run again
  with `--document-private-items`.

## Risks

- Suno stems carry bleed from other instruments and artifacts. Nobody has
  measured transcription accuracy on Suno synth stems. On acoustic
  double-bass stems Basic Pitch reached recall 81% and precision 51%. Expect
  extra notes and octave errors, and a manual pass.
- The layer estimators are unproven on Suno stems. Every accuracy figure
  comes from synthetic signals with known notes and an exact BPM. A stem
  from an audio generator may show smeared voices instead of discrete
  unison offsets, a drifting tempo and baked-in compression.
- Several Serum 2 mappings are unpublished: the cutoff law (uncertain by
  0.6 octave), route depths, envelope curves, the portamento time and
  curve, detune in cents and the effect laws. Until `render` calibrates
  them, the preset marks these parameters uncalibrated.
- Some layers cannot be separated from audio alone: filter and wavetable,
  unison and chorus, release and reverb, distortion and noise oscillator,
  LFO and unison beating. The report keeps both candidates and a default.
- Designing a preset from a layer report is unproven. Benchmarks for LLMs
  setting synth parameters exist (synth-rl for Vital), but none publishes
  results. Until render and compare exist, the producer judges the result
  by ear.
- The `.SerumPreset` format is reverse-engineered. A Serum 2 update can
  break it. Float64 everywhere loads in Serum 2.0.11, 2.0.24 and 2.1.4.
  Whether Serum 2 accepts float16 values or unknown keys is unknown.
- `basic-pitch-rs` is a v0.1.0 crate with one author, and its parity claims
  are self-run. Its model weights carry only an implicit Apache-2.0 grant.
  The CREPE weights, if used, carry only an implied MIT grant.
- A paid plugin already offers audio-to-preset for Serum 2 with stem
  splitting. Its method and quality are not public.

## Prior work

The README credits list the works this project builds on.

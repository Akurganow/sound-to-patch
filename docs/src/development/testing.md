# Testing

Tests follow the usual Rust layout. Agents write most of the code, so the
compiler, the lints and the tests must catch their typical mistakes.

## Layers

L0 is the compiler. L1 to L11 are tests. Each epic picks the layers it
needs from the matrix below and names them in its issue.

| Layer | What | Where |
| :-- | :-- | :-- |
| L0 | rustc, Clippy pedantic, rustdoc lints, units in types | `[workspace.lints]` in `Cargo.toml`, `clippy.toml` |
| L1 | Unit tests | `#[cfg(test)] mod tests` next to the code |
| L2 | Integration tests | the one test binary in `tests/it/`, one module per feature |
| L3 | Command-line cases | declarative files in `tests/cmd/`, and the console blocks of the README, the guide and the CLI reference, run by `trycmd` |
| L4 | Doc tests and runnable examples | rustdoc examples, `examples/` |
| L5 | Snapshots of the shape of our own output | `--help`, report and explanation templates, with `insta` or `trycmd` |
| L6 | Property tests | round trips, unit conversions, invariants |
| L7 | Golden numbers with explicit tolerances | JSON vectors in `tests/fixtures/golden/` |
| L8 | Fuzzing | `fuzz/`, for code that reads untrusted bytes |
| L9 | Benchmarks | `benches/`; CI only compiles them |
| L10 | Test-quality reports | mutation testing and coverage, at epic close |
| L11 | Dataset evaluation and the per-synth bench | `xtask`, local only |

`●` needed, `○` optional, `–` not applicable.

| Epic kind | L1 | L2 | L3 | L4 | L5 | L6 | L7 | L8 | L9 | L10 | L11 |
| :-- | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: |
| Foundation | ○ | – | ● | – | ○ | – | – | – | – | – | – |
| Types and units | ● | – | – | ● | – | ● | – | – | – | ● | – |
| `preset` | ● | ● | ● | ● | ● | ● | ○ | ● | ○ | ● | ○ |
| `analyze` | ● | ● | ● | ● | ● | ● | ● | – | ○ | ○ | ● |
| `notes` | ● | ● | ● | ● | ○ | ○ | ● | – | ● | ○ | ● |
| `compare` | ● | ● | ● | ● | ○ | ● | ● | – | ● | ● | ● |
| `render` | ○ | ○ | ● | ● | – | – | ○ | – | – | – | ● |
| `refine` | ● | ● | ● | ● | ○ | ○ | ○ | – | ● | ○ | ● |
| Test bench | ● | ○ | – | – | ○ | ○ | – | – | – | – | ● |
| Skill and docs | – | – | ● | ● | ○ | – | – | – | – | – | ○ |

## Benchmarks

`cargo bench --bench notes` runs the criterion groups of the notes step on
one generated 60 s stem: the model load, the model run at one thread and
at all threads, the f0 track, the MIDI write and the whole step at both
thread counts. A run takes about 40 minutes, most of it in the f0 track.
CI compiles the benches and never runs them.

The notes budget is the first run's median plus 25 %, per minute of audio:

| Threads | First median | Budget |
| :-- | :-- | :-- |
| 1 | 38.4 s | 48.0 s |
| All | 32.5 s | 40.7 s |

A run above the budget names its step: the f0 track takes 35.8 s of the
38.4 s. The [pipeline page](../analysis/pipeline.md#time-budget) gives
every group.

## Running the tests

```sh
cargo nextest run --workspace
cargo test --workspace --doc
```

nextest runs each test in its own process, so an in-process mutex does not
serialize tests; use a nextest test group instead. nextest does not run
doc tests, hence the second command. CI uses the `ci` profile from
`.config/nextest.toml`: no fail-fast and a slow-test timeout.

## CI versus local

- A test that needs a synth (Vital or Serum 2), `vita` or a dataset
  carries `#[ignore = "needs <thing>"]`, such as `#[ignore = "needs Vital"]`.
  When its input is missing, it panics with a message that names the input
  and how to get it. Run such tests with `cargo nextest run --run-ignored only`.
- Never skip a test at run time with an early `return`. It reports a pass
  that never happened.
- Never gate a test behind a Cargo feature. CI builds with `--all-features`,
  so the gate would not hold, and a local run without the feature would
  silently drop the test.
- Datasets download through `cargo xtask fetch`, which keeps a file only
  when its SHA-256 matches the pin. Evaluations on datasets run through
  `xtask`, outside CI.

## Naming and assertions

- Name a test after the behaviour it checks, with no `test_` prefix:
  `hz_from_midi_69_is_440`.
- Do not use `#[should_panic]`. Return or match an `Err` and assert on it.
- `clippy::float_cmp` denies `==` on floats. Compare with a tolerance.
- Always pass both `epsilon` and `max_relative` to `approx`: its default
  tolerance is machine epsilon (`f32::EPSILON`, about `1.2e-7`, and
  `f64::EPSILON`, about `2.2e-16`), too tight for signal processing.
- Clippy allows `unwrap`, `expect` and `panic` inside `#[test]` functions and
  `#[cfg(test)]` modules only. A helper module of `tests/it/` is declared
  with `#[cfg(test)] mod support;` and its items are `pub(crate)`, because
  `unreachable_pub` denies `pub`.
- Examples and benches are not test code for Clippy. They return `Result`
  and use `?`.

## Test data

- Generated signals are the default input: sines, band-limited saws,
  detuned pairs, noise from a seeded generator written in the test code.
- A committed fixture is small and has a row in
  `tests/fixtures/PROVENANCE.md`: origin, version, license, copyright line,
  SHA-256, date and purpose. Its license text goes in the same file.
- Nothing licensed or private enters git: no factory presets, no renders of
  them, no user stems, no personal Init preset. No Git LFS.
- A Vital fixture is a file our code generated, or a file a maintainer
  saved from the app after loading a file we generated. The generated files
  come from `cargo xtask vital-fixtures`; after a change to the writers, run
  it and update the SHA-256 rows. `cargo xtask vital-fixtures --check`
  compares the committed files with the generator and checks every row,
  the size limit of 256 KiB, the licence section and the wavetable authors.
  `cargo xtask vital-fixtures --coverage` lists the registry keys no
  fixture holds.
- Expected values never come from an AI. They come from a formula in the
  facts, a file saved by the synth, a render or an independent reference.
- A snapshot accepts every diff as the new truth, so CI runs `insta` with
  `INSTA_UPDATE=no`. Snapshot the decoded values of a preset, never its
  bytes, and never a number that must match an outside reference. Review
  a changed snapshot like code; record one locally with
  `INSTA_UPDATE=always` or `TRYCMD=overwrite`.

A Markdown page whose commands write files keeps its inputs in a
`<page>.in/` folder and holds a `<page>.out/` folder. With the `.out` folder,
`trycmd` runs the commands in a copy of `.in`, so the files they write never
land in the repository. It compares only the files that `.out` holds, so a
copy of one input is enough.

## The open Vital engine

Two ignored tests check written presets against Vital. They need tools
that CI does not install by default:

- `written_presets_load_in_the_open_engine` writes the load-check variants
  and loads each in the open Vital engine through
  `tools/vita/load_check.py`. It needs `VITA_PYTHON`, a Python with the
  GPL-3.0 `vita` package, made outside the repository:

  ```sh
  python3 -m venv /tmp/vita-env
  /tmp/vita-env/bin/pip install vita numpy
  VITA_PYTHON=/tmp/vita-env/bin/python cargo nextest run --run-ignored only -E 'test(written_presets)'
  ```

  `vita` runs only as a separate process. No crate of ours depends on it,
  and CI does not run this test.
- `a_preset_saved_by_vital_keeps_the_edits` reads a file the retail app
  saved after loading a written preset, from `VITAL_SAVED_PRESET`.

## Fuzzing

The fuzz targets live in `fuzz/`, a workspace of its own pinned to a dated
nightly in `fuzz/rust-toolchain.toml`. CI lints them on stable, and a
scheduled job runs each one for five minutes. Run one locally from the
repository root, seeded from the fixtures:

```sh
mkdir -p fuzz/corpus/vital_read
cp tests/fixtures/vital/*.vital fuzz/corpus/vital_read/
cargo +nightly-2026-10-20 fuzz run vital_read -- -max_total_time=600
```

Seed `edits_apply`, which runs edit lists through `preset apply` on the
routes fixture, from `tests/cmd/data/*.json`. The corpus and the crash
artifacts stay out of git. A crash becomes a
unit test with the input bytes inline, next to the code it broke.

## Golden numbers

- The recipe of the test signal lives in code, in the JSON vector that the
  generator writes and the Rust test reads.
- The expected value comes from an independent reference: librosa, SciPy or
  a closed form, never our own code. The generator,
  `xtask/golden/generate.py`, computes it once. Its output is committed as
  JSON under `tests/fixtures/golden/` with the recipe, the generator, the
  tool versions, the values and the tolerance.
- `xtask/golden/.python-version` pins the interpreter and
  `xtask/golden/requirements.txt` pins every package with its hashes.
  `cargo xtask golden` installs them with uv into `target/golden-venv` and
  rewrites every vector. A second run with the same pins writes
  byte-identical files.
- `requirements.in` lists the direct packages. To change a pin, edit it and
  regenerate the lock from the repository root:

  ```bash
  uv pip compile xtask/golden/requirements.in --generate-hashes --python-version 3.14.7 --output-file xtask/golden/requirements.txt
  ```

  Add `--upgrade` to move every package to its latest release. The command
  also stands in the header of `requirements.txt`.
- CI never runs Python. The `test` job runs `cargo xtask golden-check`, which
  fails when a vector lacks a field, lacks the Python version in `tools`,
  names a version other than the pin, or when a prototype table has no
  recipe file. The same job runs `cargo xtask check-no-ort`, which fails
  when `Cargo.lock` holds the ONNX Runtime crate of the transcription
  model.
- `cargo run --release -p xtask -- eval` scores the notes step on generated
  parts with known notes and prints its tables. It stays local, renders
  its audio on each run and commits nothing. Run it as a release build:
  the f0 tracker's generic code compiles at our crate's level, so a debug
  run is about 55 times slower. `--part TEXT` runs the matching parts and
  prints their notes, raw and clean. It fails when a golden of the
  cleanup breaks: the dry 110 Hz saw must show its octave copy at MIDI 57
  before the cleanup and none after, and its six notes through reverb
  must give six notes after it. Those stems take minutes in a debug
  build, so no test in CI runs them.
- Noise comes from SplitMix64, which the generator and
  `tests/it/support/rng.rs` implement bit for bit, so a noise recipe gives
  the same signal in both languages.
- Tests compare through `assert_close` in `tests/it/support/golden.rs` with
  the stored tolerance, `|actual - expected| <= abs + rel |expected|`, and
  name the source of the number in a comment.
- `tests/fixtures/golden/prototype/` holds the tables that task issues
  quote. Each is the issue's method run on a recipe given in the file; where
  the issue stated no recipe, the file's recipe defines it and its numbers
  replace the issue's.
- To change a reference: edit the generator or the pins, run
  `cargo xtask golden`, and commit the vectors with the change.

## Closing an epic

The epic issue holds a definition-of-done checklist. Two reports are read
at close. Both report and do not gate:

- Mutation testing on the new modules:
  `cargo mutants --test-tool nextest --in-place`. Do not set
  `RUSTFLAGS=-D warnings` for it: a deleted line trips `deny(unused)` and
  turns a mutant unviable.
- Coverage: `cargo llvm-cov nextest --workspace`, which needs the
  `llvm-tools-preview` component. The scheduled workflow publishes the same
  report every week. A coverage percentage is never a gate, because it
  rewards tests that run code without asserting.

Tests write only to temporary directories, because mutation testing runs
them against broken code.

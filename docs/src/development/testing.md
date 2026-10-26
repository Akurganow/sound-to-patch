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
| L3 | Command-line cases | declarative files in `tests/cmd/`, run by `trycmd` |
| L4 | Doc tests and runnable examples | rustdoc examples, `examples/` |
| L5 | Snapshots of the shape of our own output | `--help`, report and explanation templates |
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

- A test that needs a synth plugin (Vital or Serum 2), `vita` or a dataset carries `#[ignore = "needs <thing>"]`.
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
- Expected values never come from an AI. They come from a file saved by
  Serum 2 or from an independent reference.

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
  recipe file.
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

# Architecture

## Purpose

sound-to-serum gives a producer a Serum 2 starting point for one sound. On
the audio path a stem and its BPM become notes, a preset and an
explanation. On the text path a written description becomes a preset and
an explanation. This file is the map for finding the code that does a
thing.

## Bird's-eye view

The audio path reads a stem, estimates eight synth layers in a fixed order
(notes and pitch, amplitude envelope, unison, oscillator, filter,
modulation, distortion, space), and writes a layer report. An agent session
reads the report, settles the flagged ambiguities and writes an edit list.
The `preset` code validates the edit list against an Init preset and
writes the preset file. The text path skips the analysis: the agent writes
the edit list from the description.

The first release holds only the command-line skeleton.

## Code map

- `cli`: the command-line definition. The binary parses it, and `xtask`
  renders the man page and the shell completions from the same tree.
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
- No test needs the network, Serum 2 or a dataset unless it is
  `#[ignore]` with a reason.

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

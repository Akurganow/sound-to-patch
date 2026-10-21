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
track. Supported material:

- bass;
- leads and plucks;
- pads and chords, including polyphony.

## Output

A folder next to the stem with:

- `notes.mid`: the notes of the whole stem, chords included, ready for a
  DAW piano roll;
- `preset.SerumPreset`: a Serum 2 preset that starts close to the sound;
- `explanation.md`: why the preset is built this way and which controls to
  turn next.

## Success

- The preset matches the type and architecture of the sound: wavetables,
  filter, envelopes, modulation and main effects. The producer finishes it
  by hand.
- The explanation teaches which controls shape the sound.
- The MIDI drops into the piano roll without transcription by ear.

## Approach

A Rust command-line toolkit does the deterministic work. An agent session
does the sound design. The project trains no models of its own.

### `analyze`

Writes audio features as JSON and a spectrogram PNG for the agent to look
at. Features: pitch, amplitude envelope with attack time, spectral
centroid, rolloff and flatness, modulation rate, stereo width (side/mid),
distortion. Spectral centroid and attack time have the strongest published
link to perceived timbre. The other features are a proposal to validate.

### `notes`

Transcribes the stem to MIDI with the `basic-pitch-rs` crate. It bundles
the pretrained Basic Pitch model and runs it on `tract`, so the binary
needs no ONNX Runtime library. It also covers decoding, resampling and MIDI
writing.

- Pin the crate version: it is a young v0.1 port.
- Use one profile per stem type: frequency range, onset threshold and
  minimum note length. The profiles cut false notes from bleed.
- Known limits: no notes below A0 (27.5 Hz), and pitch bends cover about
  two semitones. Long 808 glides break into steps.

### `preset`

Reads and writes `.SerumPreset` files and builds a preset from JSON.

- Layout: `XferJson\0`, JSON metadata length (u64 LE), JSON metadata,
  uncompressed CBOR size (u32 LE), encoding flag (u32 LE, 2 = zstd), one
  zstd frame of CBOR.
- The metadata `hash` is the MD5 of the compressed bytes. Recompute it on
  write.
- Start every preset from an Init preset saved by Serum 2 itself. Change
  only `plainParams`, because no public table of defaults exists.
- Keep CBOR map order and write floats as float64. Check one written file
  in Serum 2 before trusting the writer.
- Reference factory wavetables by their relative path.
- The open-source packers disagree on modulation source IDs. Verify them
  against presets saved by Serum 2.
- Crates: `zstd`, `ciborium`, `md-5`, `serde_json`. No Rust crate for
  Serum 2 presets exists yet.

### Agent session

1. The producer hands a stem to the agent.
2. The agent runs `analyze` and `notes`.
3. The agent designs the preset from the features and the spectrogram.
4. The agent writes the preset with `preset`.
5. The agent writes `explanation.md`.

A Claude Code skill in this repository describes the loop. The skill
carries style cards for reese, neuro, wobble, 808, pluck and pad sounds,
each with a worked example preset. Few-shot examples are the most reliable
published gain for LLM audio-parameter tasks. Recipes from articles are
retold in our own words, never copied.

## Licensing of reused work

- Code from MIT or Apache-2.0 projects may be reused with attribution.
- GPL projects serve only as sources of facts, or run as a separate
  process.
- Code without a license may be read, not copied.
- Docs link to code only under a permissive license. Every project or
  paper we studied may be credited as inspiration, whatever its license.
- Crates we install are dependencies, not inspiration. They stay out of
  the credits.

## Dependencies

- Every new dependency starts at its latest available version.
- Open question: when to enable Renovate. Not at the start, only once the
  project is mature enough.

## Out of scope now

- Training neural networks.
- Rendering presets to audio, and everything that depends on it.
- Tempo detection: Suno already reports the BPM.
- Drums.
- Cloud execution. Everything runs locally on macOS.

## Later

- `render`: render a preset with MIDI to WAV through Serum 2.
  - Serum 2 ships as VST3, AU and AAX, with no CLAP build.
  - Load the preset through a VST3 processor state blob. Raw preset bytes
    in the state fall back to the init patch.
  - Try the native `vst3-host` crate first. A separate host process is the
    fallback.
  - Renders are not deterministic, so compare with tolerances.
  - Rendering needs a Serum 2 authorized on the machine, so CI cannot do it.
- `compare`: score two WAV files with a composite metric: MFCC distance,
  multi-scale spectral loss and a CLAP embedding distance. MFCC tracks
  human timbre ratings well. Spectral loss alone does not.
- `refine`: tune preset parameters automatically with CMA-ES over render
  and compare, so the toolkit works from a terminal without an agent.
- A local test bench: render factory presets, run them through the toolkit
  and compare the result with the source preset. Add a control run with the
  features hidden from the agent, to measure what the features add. No
  public dataset pairs Serum 2 presets with audio. Factory presets and
  renders stay out of the repository.
- A pitch tracker for bass glides, if Basic Pitch bends prove too coarse.
- Several different sounds in one stem produce several presets.
- Splitting a full Suno track into clean stems.

## Engineering

- Rust, MIT license.
- Conventional Commits, enforced by a `commit-msg` hook from
  `cog install-hook`.
- CI on pull requests and pushes: `cargo fmt --check`, `cargo clippy`,
  `cargo test`, CodeQL.
- Release on push to `main`: `cog bump --auto` updates `CHANGELOG.md`, tags
  the version and publishes a GitHub Release with a macOS arm64 binary.

### Strict code

Strict types and strict lints are a hard requirement. Agents write most of
the code, and agent output varies, so the compiler and linters must catch
the typical mistakes.

- Types carry units. Public APIs take newtypes such as `Hz`, `Seconds`,
  `Semitones`, `Decibels` and `Normalized` (0..1), never a bare `f32`. The
  research found unit mix-ups between sources, for example a filter cutoff
  stored as 0..1 in one place and in Hz in another.
- Parse, don't validate: values are checked once at the boundary (file,
  CLI, JSON) and are valid by type afterwards.
- Lints sit in `[lints]` in `Cargo.toml`, so every build applies them:
  - rustc: `unsafe_code = "forbid"`, `missing_docs = "deny"`,
    `unreachable_pub = "deny"`;
  - Clippy: `all` and `pedantic` denied; `unwrap_used`, `expect_used`,
    `panic`, `todo`, `unimplemented` and `dbg_macro` denied outside tests.
- A future plugin-hosting module may need `unsafe` for FFI. It then gets a
  narrow, documented exception in that module only.
- CI fails on any warning: `cargo clippy --all-targets --all-features -- -D
  warnings` and `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`.

## Risks

- Suno stems carry bleed from other instruments and artifacts. Nobody has
  measured transcription accuracy on such stems. The MIDI will contain
  errors and needs a quick manual pass.
- Designing a preset from features and a spectrogram is unproven. No
  benchmark of LLMs writing synth presets from audio exists. Until render
  and compare exist, the producer judges the result by ear.
- The `.SerumPreset` format is reverse-engineered. A Serum 2 update can
  break it. Whether Serum 2 accepts float16 values or unknown keys is
  unknown.
- `basic-pitch-rs` is new and self-benchmarked by its author.

## Prior work

The README credits lists the works this project builds on.

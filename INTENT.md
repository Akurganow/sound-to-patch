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
  DAW piano roll. One track, tempo from the BPM, a pitch-bend range of two
  semitones when bends are written.
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

The fact base behind every decision below lives in `research/`, outside
git: one fact file per studied project or paper, plus three syntheses with
the contradictions and how to settle them.

### `analyze`

Writes audio features as JSON and a spectrogram PNG.

- Features: pitch and detune evidence, amplitude envelope (attack, decay,
  sustain level, tail), spectral centroid, rolloff, flatness, band energy
  shares, even-to-odd harmonic ratio, modulation rate mapped to the nearest
  note division at the given BPM, stereo width (side/mid), distortion,
  crest factor.
- Centroid and attack time are the best-supported timbre descriptors. The
  other features are proposals to validate, and each feature group carries
  a confidence field.
- One documented convention per feature, named in the JSON. Spectral shape
  uses magnitude weights, band shares use power. Conventions in the
  literature differ by up to 15x on the same sound.
- `analyze` reads the stem at its native sample rate. Frames are 8192
  samples when the fundamental is below 100 Hz, else 4096.
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
- Known limits: no notes below A0 (27.5 Hz). Bends have a resolution of a
  third of a semitone and are clipped at two semitones, so long 808 glides
  break into steps. Bends are dropped for overlapping notes. In-tune notes
  read one bin sharp unless `bend_offset = -1`, pending a check. Expect
  octave errors on bass.

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
- Crates: `zstd` 0.14 (BSD-3-Clause), `ciborium` (read only), `md-5` (no
  hex formatter, format bytes ourselves), `serde_json`.
- Every format fact records the Serum build it was verified on. The
  metadata version changed between builds (4.0, 7.0, 9.0, 10.0).

### Agent session

1. The producer hands a stem and its BPM to the agent.
2. The agent runs `analyze` and `notes`.
3. The agent reads the features as numbers, uses the spectrogram only for
   coarse shape and loads the matching style card. It writes an edit list
   in display units, with a reason per edit.
4. `preset` validates the edit list against the Init preset and writes the
   preset. An unknown name or an out-of-range value returns a message with
   the fix.
5. The agent writes `explanation.md` from the reasons.

A Claude Code skill in this repository describes the loop. It carries a
card per style: reese, neuro, wobble, 808, pluck, lead and pad. Sub bass
lives in the common part.

- Each card holds the audio signature, the block graph, 12 to 24 free knobs
  with narrow ranges, typical values, the modulation and FX order, and the
  controls to turn next. The knob list is machine-readable, so `refine`
  reuses it.
- Each card has two worked example edit lists in the reply schema.
- Evidence: the only published test of in-context help for an LLM setting
  parameters (LLM2Fx, text to EQ and reverb) found examples gave the
  largest gain on reverb. No result exists for synth presets or audio
  input, so the test bench measures it.
- Recipes from articles are retold in our own words, never copied.

## Licensing of reused work

- Code from MIT or Apache-2.0 projects may be reused with attribution.
- GPL projects serve only as sources of facts, or run as a separate
  process.
- Code without a license may be read, not copied.
- Docs link to code only under a permissive license. Every project or
  paper we studied may be credited as inspiration, whatever its license.
- Crates we install are dependencies, not inspiration. They stay out of
  the credits.
- License notices of the dependencies ship with the binary: the Basic Pitch
  `NOTICE` for the bundled model, the BSD-3-Clause `zstd` notice and the
  others, collected with `cargo-about`.

## Dependencies

- Every new dependency starts at its latest available version.
- Exception: when a dependency pins another crate exactly, we do not add
  that crate ourselves.
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
  hosted model and receives only the feature JSON and the spectrogram,
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
- `compare`: return named distances, not one scalar: MFCC (40 coefficients,
  cosine over the full frame map), multi-scale spectral loss, envelope
  distance, descriptor deltas and pitch contour, with an optional CLAP
  distance. On human timbre ratings MFCC is the best simple baseline
  (triplet agreement 0.724, spectral loss 0.661, chance 0.5). No single loss
  wins on every synth, and within one system the metrics do not track
  listeners, so scores never replace listening. Exclude or level-match the
  first MFCC coefficient: it carries loudness.
- `refine`: improve a starting preset with CMA-ES (`fcmaes-core`, ask/tell
  with bounds) over render and compare. It searches the 12 to 24 knobs named
  by the style card, inside narrow bounds, with pitch fixed from `notes`.
  It scores audio only, on two or three representative notes, and stops on
  a budget of about 10,000 evaluations or when the score stops improving
  beyond the render noise floor. Without an agent it starts from the card's
  template. Wide bounds made a published search diverge.
- A local test bench in four tiers. It compares audio rendered from the
  result with the target, never parameters.
  - T0: render determinism and the noise floor.
  - T1: Serum 2 factory presets and random perturbations of them, where a
    perfect answer exists. The perturbation log is the ground truth.
  - T2: audio from other synths (the Surge and NSynth sets), where no
    perfect answer exists.
  - T3: real Suno stems, judged by metric and by ear.
  - The agent runs in arms: full features, numbers only, image only,
    features hidden, no style card, no examples, and baselines. The
    features-hidden arm gives the card and the stem type only.
  - No public dataset pairs Serum 2 presets with audio. Factory presets and
    renders stay out of the repository.
- A pitch tracker for bass glides, if Basic Pitch bends prove too coarse.
  Candidates: CREPE through `tract` (reaches 31.7 Hz) with a segmenter
  written from the CREPE Notes paper, or `pyin` for an octave check.
  SwiftF0 stops at 46.9 Hz.
- Several different sounds in one stem produce several presets.
- Splitting a full Suno track into clean stems.

## Engineering

- Rust, edition 2024, MIT license. `rust-version` is the highest MSRV among
  the dependencies (1.91 now).
- Conventional Commits, enforced by a `commit-msg` hook from
  `cog install-hook`.
- CI on pull requests and pushes: `cargo fmt --check`, `cargo clippy`,
  `cargo test`, CodeQL.
- Release on push to `main`: `cog bump --auto` updates `CHANGELOG.md`, tags
  the version and publishes a GitHub Release with a macOS arm64 binary and
  the third-party license notices.

### Strict code

Strict types and strict lints are a hard requirement. Agents write most of
the code, and agent output varies, so the compiler and linters must catch
the typical mistakes.

- Types carry units. Public APIs take newtypes such as `Hz`, `Seconds`,
  `Semitones`, `Decibels` and `Normalized` (0..1), never a bare `f32`. The
  research found unit traps: the voice-filter cutoff is a 0..1 knob
  position, while FX frequencies are in Hz.
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
  measured transcription accuracy on Suno synth stems. On acoustic
  double-bass stems Basic Pitch reached recall 81% and precision 51%. Expect
  extra notes and octave errors, and a manual pass.
- Designing a preset from features and a spectrogram is unproven.
  Benchmarks for LLMs setting synth parameters exist (synth-rl for Vital),
  but none publishes results. Until render and compare exist, the producer
  judges the result by ear.
- The `.SerumPreset` format is reverse-engineered. A Serum 2 update can
  break it. Float64 everywhere loads in Serum 2.0.11, 2.0.24 and 2.1.4.
  Whether Serum 2 accepts float16 values or unknown keys is unknown.
- `basic-pitch-rs` is a v0.1.0 crate with one author, and its parity claims
  are self-run. Its model weights carry only an implicit Apache-2.0 grant.
- A paid plugin already offers audio-to-preset for Serum 2 with stem
  splitting. Its method and quality are not public.

## Prior work

The README credits list the works this project builds on.

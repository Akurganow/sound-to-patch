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
does the sound design.

Commands:

- `analyze`: writes audio features as JSON (pitch, amplitude envelope,
  spectrum, modulation rate, stereo width, distortion) and a spectrogram
  PNG for the agent to look at.
- `notes`: transcribes the stem to MIDI with a pretrained model (Basic Pitch
  through ONNX Runtime).
- `preset`: reads and writes `.SerumPreset` files and builds a preset from
  JSON. The format is XferJson header, CBOR, Zstandard. Existing open-source
  packers document it.

Session loop:

1. The producer hands a stem to the agent.
2. The agent runs `analyze` and `notes`.
3. The agent designs the preset from the features and the spectrogram.
4. The agent writes the preset with `preset`.
5. The agent writes `explanation.md`.

A Claude Code skill in this repository describes the loop.

Pretrained models are fine. The project trains no models of its own.

## Out of scope now

- Training neural networks.
- Rendering presets to audio, and everything that depends on it.
- Tempo detection: Suno already reports the BPM.
- Drums.
- Cloud execution. Everything runs locally on macOS.

## Later

- `render`: render a preset with MIDI to WAV through Serum 2, so the
  producer can compare it with the stem by ear.
- `compare`: score two WAV files with multi-scale spectral loss and MFCC
  distance.
- `refine`: tune preset parameters automatically through render and compare,
  so the toolkit works from a terminal without an agent.
- A test bench: render factory presets, run them through the toolkit and
  compare the result with the source preset. Factory presets belong to Xfer
  Records and stay out of the repository.
- A labeled public dataset of audio and Serum presets as an extra test bench,
  if one exists.
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

## Risks

- Suno stems carry bleed from other instruments and artifacts. The MIDI
  will contain errors and needs a quick manual pass.
- Designing a preset from features and a spectrogram is unproven. Until the
  render and compare commands exist, the producer judges the result by ear.
- The `.SerumPreset` format is reverse-engineered. A Serum 2 update can
  break it.

## Prior work

- Kevin Thor Hansen,
  [Sound2Serum: Neural Sound Matching System for Serum 2](https://projekter.aau.dk/projekter/files/835339029/Master_Thesis_Kevin_Thor_Hansen.pdf),
  master thesis, Aalborg University Copenhagen, 2026. It predicts 45
  oscillator and envelope parameters from a single note with a neural
  network. Listeners rated its matches below a 300 Hz low-pass anchor in
  82% of trials. This project takes its preset-format and evaluation
  lessons and skips the training.
- [node-serum2-preset-packager](https://github.com/CharlesBT/node-serum2-preset-packager)
  and [serum2gen](https://github.com/dougwithseismic/serum2gen): open-source
  `.SerumPreset` readers and writers.
- [Basic Pitch](https://github.com/spotify/basic-pitch): polyphonic
  audio-to-MIDI transcription.

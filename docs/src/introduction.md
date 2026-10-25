# Introduction

sound-to-serum gives a producer a starting point in Serum 2 for one sound.
It has two paths:

- **Audio path.** One stem of one instrument and its BPM go in. The notes as
  `notes.mid` and a `preset.SerumPreset` come out, with an `explanation.md`
  that says how the preset works and which controls to turn next.
- **Text path.** The producer describes the sound in their own words. An
  agent turns the description into a preset and its explanation, and into
  notes when the description asks for them.

The project is at its first release. The `sound-to-serum` command prints
its help and version; the commands of both paths arrive in later releases.

The [API documentation](api/sound_to_serum/index.html) covers the library.
The development chapters describe how the project is tested and released.

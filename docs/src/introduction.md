# Introduction

sound-to-patch gives a producer a starting point in a synth for one sound.
Vital and Serum 2 are the target synths, and Vital comes first. It has two
paths:

- **Audio path.** One stem of one instrument and its BPM go in. The notes as
  `notes.mid` and a preset for the target synth come out (`preset.vital` or
  `preset.SerumPreset`), with an `explanation.md` that says how the preset
  works and which controls to turn next.
- **Text path.** The producer describes the sound in their own words. An
  agent turns the description into a preset and its explanation, and into
  notes when the description asks for them.

The project is at its first release. The `sound-to-patch` command prints
its help and version; the commands of both paths arrive in later releases.

The project has no affiliation with the makers of Vital or Serum 2.

The [API documentation](api/sound_to_patch/index.html) covers the library.
The development chapters describe how the project is tested and released.

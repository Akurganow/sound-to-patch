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

The project is at an early release. `sound-to-patch preset inspect` prints
the header and the decoded settings of a Vital preset, and
`sound-to-patch preset apply` writes one from an edit list, as the
[preset guide](guide/preset.md) shows. The analysis and the text path
arrive in later releases.

Vital and Serum 2 are trademarks of their owners. This project is not
affiliated with or endorsed by them.

The [API documentation](api/sound_to_patch/index.html) covers the library.
The development chapters describe how the project is tested and released.

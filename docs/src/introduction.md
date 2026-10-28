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

The project is at an early release. `sound-to-patch analyze` reads a stem
and writes a report of its tempo, notes, level, spectrum, band edge and
stereo image with a spectrogram, as the [analysis guide](guide/analyze.md) shows.
`sound-to-patch preset inspect` prints the header and the decoded settings
of a Vital preset, and `sound-to-patch preset apply` writes one from an
edit list, as the [preset guide](guide/preset.md) shows. The layer
estimators and the text path arrive in later releases.

The [API documentation](api/sound_to_patch/index.html) covers the library.
The development chapters describe how the project is tested and released.

Vital and Serum 2 are trademarks of their owners. This project is not
affiliated with or endorsed by them.

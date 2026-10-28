# Synth reference

The controls of each synth that sound-to-patch writes presets for, by synth layer. Vital is ready now. Serum 2 joins when its adapter lands.

Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.

The files under `<synth>/` are generated from the parameter registry of sound-to-patch, the same data that `preset apply` checks edit lists against. Each layer file has one row per control: the on-screen label, the preset key, the unit, the range, the default, the tag, the verified build and the tuning note. In a key, `N` stands for each instance number in the row.

## Which file answers what

The files shared by every synth will sit under `core/`: the music dictionary and the word entries. It holds no file yet.

| Question | Vital |
| :-- | :-- |
| Pitch, glide, voices and bend | [vital/pitch.md](vital/pitch.md) |
| Oscillators, wavetable frame, sample | [vital/oscillator.md](vital/oscillator.md) |
| Unison voices, detune and stereo spread | [vital/unison.md](vital/unison.md) |
| Filters, cutoff and resonance | [vital/filter.md](vital/filter.md) |
| Envelopes and velocity | [vital/envelope.md](vital/envelope.md) |
| LFOs, random sources, macros and modulation slots | [vital/modulation.md](vital/modulation.md) |
| Distortion | [vital/distortion.md](vital/distortion.md) |
| Effects: chorus, compressor, delay, EQ, flanger, phaser, reverb and their order | [vital/space.md](vital/space.md) |
| Tempo, volume, oversampling, legacy keys and keys of newer builds | [vital/global.md](vital/global.md) |
| A worked edit list: a plucked bass | [vital/examples/pluck-bass.json](vital/examples/pluck-bass.json) |

## Tags

- `from facts`: read from a source the project may use.
- `measured`: confirmed in the app or in a render.
- `uncalibrated`: an estimate. Write the value, then tell the producer how to tune it by ear, from the tuning note.

## The synth's own documentation

Before you explain a control in depth, ask the producer whether they have the synth's documentation as a local file. If they give a path, read the page about that control and cite it. Otherwise explain from these files, and give the producer the link to the maker's site to read themselves.

## Edit list shape

An edit list is a JSON object with `synth` and `edits`. Each edit has a `reason`. The worked example shows the record kinds that matter most: `param` for a value, `menu` for a menu choice, `route` for a modulation route, and a flag as `true` or `false`.

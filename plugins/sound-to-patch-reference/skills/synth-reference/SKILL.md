---
name: synth-reference
description: The controls of each synth that sound-to-patch writes presets for, grouped by synth layer, with on-screen labels, preset keys, units, ranges, defaults and tuning notes. Use when writing or checking a sound-to-patch edit list, when explaining what a preset control does, or when choosing which control changes a sound. Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.
license: MIT
compatibility: Writing a preset needs the sound-to-patch command-line tool on the same machine.
metadata:
  project: sound-to-patch
  synths: vital
---

# Synth reference

Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.

This skill is the map of the synth controls that sound-to-patch can set. Vital is ready now. Serum 2 joins when its adapter lands.

## Rules to keep

1. Name a control by its preset key in an edit list, and by its on-screen label when you talk to the producer.
2. Write values with units, such as `"400 Hz"`, `"5 ms"` or `"-6 dB"`. Write a stored number only for a control whose unit is `none`.
3. Take every number from the layer report, the producer's words or a reference file. Never write a number from memory.
4. A control tagged `uncalibrated` is still written. Say in the explanation how to tune it by ear, from its tuning note.
5. Check every edit list with `sound-to-patch preset apply <edits.json> -o <out.vital>`. An unknown key or an out-of-range value returns a message with the fix. Apply the fix and run it again.
6. Give pitch as a MIDI number or in Hz. An octave name alone, such as "C3", means different notes in different programs.
7. Describe one synth at a time, on its own terms. Never compare synths or versions of a synth.
8. Never write a key from a "Legacy keys" list. Keep the keys of newer builds as the template has them.
9. Do not fetch the synth maker's web pages. Give the producer the link instead.

## Which file answers what

Each layer file has one row per control, with its preset key, unit, range, default, tag, verified build and tuning note. In a key, `N` stands for each instance number in the row.

| Question | Vital |
| :-- | :-- |
| Pitch, glide, voices and bend | [reference/vital/pitch.md](reference/vital/pitch.md) |
| Oscillators, wavetable frame, sample | [reference/vital/oscillator.md](reference/vital/oscillator.md) |
| Unison voices, detune and stereo spread | [reference/vital/unison.md](reference/vital/unison.md) |
| Filters, cutoff and resonance | [reference/vital/filter.md](reference/vital/filter.md) |
| Envelopes and velocity | [reference/vital/envelope.md](reference/vital/envelope.md) |
| LFOs, random sources, macros and modulation slots | [reference/vital/modulation.md](reference/vital/modulation.md) |
| Distortion | [reference/vital/distortion.md](reference/vital/distortion.md) |
| Effects: chorus, compressor, delay, EQ, flanger, phaser, reverb and their order | [reference/vital/space.md](reference/vital/space.md) |
| Tempo, volume, oversampling, legacy keys and keys of newer builds | [reference/vital/global.md](reference/vital/global.md) |
| A worked edit list: a plucked bass | [examples/vital/pluck-bass.json](examples/vital/pluck-bass.json) |

## The synth's own documentation

Before you explain a control in depth, ask the producer whether they have the synth's documentation as a local file. If they give a path, read the page about that control and cite it. Otherwise explain from the reference files, and give the producer the link to the maker's site to read themselves.

## Edit list shape

An edit list is a JSON object with `synth` and `edits`. Each edit has a `reason`. The worked example shows the four record kinds that matter most: `param` for a value, `menu` for a menu choice, `route` for a modulation route, and a flag as `true` or `false`.

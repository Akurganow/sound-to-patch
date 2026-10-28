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

Start at [reference/README.md](reference/README.md). It names the file that answers each question, one file per synth layer.

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

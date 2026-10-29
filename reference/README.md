# Synth reference

The controls of each synth that sound-to-patch writes presets for, by synth layer. Vital is ready now. Serum 2 joins when its adapter lands.

Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.

The files under `<synth>/` are generated from the parameter registry of sound-to-patch, the same data that `preset apply` checks edit lists against. Each layer file has one row per control: the on-screen label, the preset key, the unit, the range, the default, the tag, the verified build and the tuning note. In a key, `N` stands for each instance number in the row. The files under `core/` are generated from the music and word dictionaries of sound-to-patch.

## Which file answers what

The music files under `core/` serve every synth. Each entry has a checkable note rule, a tier and a tag; pitches are MIDI numbers and onsets are slots of a sixteenth grid.

| Question | Core file |
| :-- | :-- |
| Note names, middle C, keys, Camelot and Open Key, Roman numerals | [core/notation.md](core/notation.md) |
| The pitch range and default voice of a role | [core/roles.md](core/roles.md) |
| Scales, modes, mood words and allowed outside notes | [core/scales.md](core/scales.md) |
| Chord qualities and the symbols that spell them | [core/chords.md](core/chords.md) |
| Voicings, voice leading and low-interval limits | [core/voicing.md](core/voicing.md) |
| Named progressions and genre chord habits | [core/progressions.md](core/progressions.md) |
| Positions, swing, feel, named rhythms and Euclidean patterns | [core/rhythm.md](core/rhythm.md) |
| Note length, articulation and bass, lead, arp and pad techniques | [core/technique.md](core/technique.md) |
| Phrase length, repetition and section words | [core/phrase.md](core/phrase.md) |
| Genre tempos and drum skeletons | [core/genres.md](core/genres.md) |
| Words with several senses and the one question that picks a sense | [core/senses.md](core/senses.md) |
| Corpus shares of chords and progressions | [core/statistics-harmony.md](core/statistics-harmony.md) |
| Corpus shares of keys, modes, melody and rhythm | [core/statistics-keys.md](core/statistics-keys.md) |

The word files under `core/` serve every synth too. Each row is one sense of a producer word with its meaning, sourced numbers, listener agreement and the analyzer number that checks it; the synth files bind the sense to controls.

| Question | Core file |
| :-- | :-- |
| Brightness and balance words: bright, dark, warm, crisp | [core/words-tone.md](core/words-tone.md) |
| Weight and low-end words: fat, deep, punchy, boomy | [core/words-body.md](core/words-body.md) |
| Grit, noise and era words: gritty, clean, lo-fi, analog | [core/words-texture.md](core/words-texture.md) |
| Harmonic and resonance words: hollow, nasal, metallic | [core/words-structure.md](core/words-structure.md) |
| Width, room and distance words: wide, spacious, wet | [core/words-space.md](core/words-space.md) |
| Words for change over time: wobble, evolving, glide | [core/words-movement.md](core/words-movement.md) |
| Attack, decay and dynamics words: plucky, sustained | [core/words-envelope.md](core/words-envelope.md) |
| Mood and character words, which name no control | [core/words-mood.md](core/words-mood.md) |
| Slightly, very and other modifiers; style and technique words | [core/words-lists.md](core/words-lists.md) |

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

# Transcribing a stem

`sound-to-patch notes` reads one stem and writes its notes as a MIDI file,
`notes.mid`, with the tempo you give. It finds the pitch range and whether
the part plays one note at a time or chords, decodes the notes, and cleans
them up. The [CLI reference](../reference/cli.md#notes) lists every flag,
and the [pipeline page](../analysis/pipeline.md#the-notes-step) gives each
step with its settings and limits.

## Run it

Export one instrument as a WAV, AIFF, FLAC, MP3 or AAC file and pass its
tempo:

```sh
sound-to-patch notes bass.wav --bpm 120
```

```text
detected range: MIDI 30 to 48, from the f0 track
detected voicing: monophonic, chord share 0.00
register window: 20 dB
settings: detected, MIDI 30 to 48, minimum note 5 frames (58.0 ms), melodia on
decoded: 24 notes
octave check: 24 notes, 4 moved down an octave, 4 flagged, 0 unchecked
ghost merge: 18 notes
split join: 18 notes
quiet-note guard: 18 notes, 0 dropped below 40 dB
wrote bass/notes.mid
```

This run read a generated bass line: 16 sawtooth notes from A1 to A2,
0.2 s each on every eighth note at 120 BPM. The cleanup took it from 24
decoded notes to 18; the two extra notes are left for the manual pass.

The file goes into a folder named after the stem, next to it, the same
folder `analyze` writes into. `--out` picks another folder. Only
`notes.mid` is replaced; nothing else in the folder changes.

Each line names one step:

- `detected range` and `detected voicing`: the pitch range the stem
  plays in and whether it plays chords, read from the stem itself.
- `register window`: the f0 frames that set the range lie within this
  many decibels of the stem's loud passages, so quiet bleed does not widen
  it.
- `settings`: what the decoder used, the detected settings or those of
  `--profile`, with the shortest note it keeps.
- `decoded` to `quiet-note guard`: the note count after each cleanup step.
  `moved down an octave` counts octave copies above low notes, `flagged`
  the notes whose pitch the f0 track disputes, and the guard drops notes
  that sound only far below the stem's loudest passage.

## Tempo

`--bpm` sets the tempo of the file and the shortest note the decoder
keeps: half a sixteenth note, between 58 ms and 127.7 ms. Pass the tempo
of the song; a wrong tempo moves every note off the grid of your DAW.

## Profiles

Detection fits most stems. When it misreads one, `--profile` sets the
range and the minimum length for a kind of stem:

| Profile | Range | Minimum length | Cleanup |
| :-- | :-- | :-- | :-- |
| `bass` | B0 to F#4 (MIDI 23 to 66) | from the BPM | one note at a time |
| `lead` | C2 to B6 (MIDI 36 to 95), plucks too | from the BPM | one note at a time |
| `pad` | C2 to B6 (MIDI 36 to 95), chords too | 127.7 ms | chords: no octave check |

Detection still runs and prints its findings, so you can compare them
with the profile. Two cases need a profile: bleed from another
instrument louder than -20 dB in a passage where the part rests, and a
line with held double stops (`--profile pad`).

## Check the file

Open `notes.mid` in a piano roll at the same tempo. It holds one track on
MIDI channel 1 with a tempo event and 4/4 at the start; every note has
velocity 100. Then check what the model cannot get right:

- No note below A0 (27.5 Hz): the model's lowest key.
- Note ends come late: a note often lasts until the next one starts.
- Octave errors at 110 Hz and below. The cleanup moves an octave copy
  that sounds over its true note, but a lone note an octave off stays and
  is only flagged.
- A repeated note can merge into the one before it when the model misses
  its onset.

Plan a manual pass in the piano roll. The [pipeline
page](../analysis/pipeline.md#evaluation) gives the scores on generated
parts; real stems score lower.

## When it fails

A stem that gives no note, such as silence, writes nothing and exits with
code 1. The message names the stem and suggests a profile. A folder that
holds the report of another stem stops the run unless you pass `--out` or
`--force`.

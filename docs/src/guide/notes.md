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
detected voicing: monophonic, chord share 0.12
register window: 20 dB
cleanup voicing: monophonic, as detected
settings: detected, MIDI 30 to 48, minimum note 5 frames (58.0 ms), melodia on
decoded: 24 notes
octave check: 24 notes, 4 moved down an octave, 4 flagged, 0 unchecked
ghost merge: 20 notes
attack split: 20 notes
split join: 20 notes
quiet-note guard: 20 notes, 0 dropped below 40 dB
wrote bass/notes.mid
```

This run read a generated bass line, a mono 16-bit WAV at 44.1 kHz, 4.5 s
long. It holds 16 notes on the eighth notes of 120 BPM from 0.5 s, each
0.2 s long with no fade: MIDI 33, 36, 40, 43, 45, 43, 40 and 36, twice.
Each note is a band-limited sawtooth, the sum of $\sin(2 \pi h f_0 t) / h$
for every harmonic $h$ below 22.05 kHz, at amplitude 0.3. The file holds all 16 notes at the right pitch, each
within 21 ms of its onset. The 4 extra notes are short fragments of the
attack, 70 to 106 ms before a true note of the same key.

The file goes into a folder named after the stem, next to it, the same
folder `analyze` writes into. `--out` picks another folder. Only
`notes.mid` is replaced; nothing else in the folder changes. The file
names its stem in its track name, so a run of another stem into the same
folder stops unless you pass `--out` or `--force`.

Each line names one step:

- `detected range` and `detected voicing`: the pitch range the stem
  plays in and whether it plays chords, read from the stem itself.
- `register window`: the f0 frames that set the range lie within this
  many decibels of the stem's loud passages, so quiet bleed does not widen
  it.
- `cleanup voicing`: whether the cleanup treats the part as one note at a
  time, which runs the octave check, or as chords, and whether that came
  from detection or from `--profile`.
- `settings`: what the decoder used, the detected settings or those of
  `--profile`, with the shortest note it keeps.
- `decoded` to `quiet-note guard`: the note count after each cleanup step.
  `moved down an octave` counts octave copies above low notes, `flagged`
  the notes whose pitch the f0 track disputes, and the guard drops notes
  that sound only far below the stem's loudest passage.

## Tempo

`--bpm` sets the tempo of the file and the shortest note the decoder
keeps: half a sixteenth note, between 58 ms and 127.7 ms. A tempo from a
file name is often close but not exact, so the command refines it from the
stem's onsets, from 2.91 % below to 3.09 % above it, and prints the result
first:

```text
tempo: 119.62 BPM, refined from --bpm 120 (peak ratio 8.05)
```

This line comes from a generated 16 s bass line at 119.62 BPM, eighth
notes of A1, C2, E2 and G1, passed as `--bpm 120`. The file then plays at
119.62 BPM. Set your DAW project to the printed tempo, or the notes drift
off the grid over the song. When the onsets do not settle the tempo, as in
a pad that changes chord once a bar, the line says why and the file keeps
`--bpm`. `--bpm-exact` uses `--bpm` as it is.
The [tempo section](../analysis/pipeline.md#tempo) gives the method.

## Profiles

Detection fits most stems. When it misreads one, `--profile` sets the
range and the minimum length for a kind of stem:

| Profile | Range | Minimum length | Cleanup |
| :-- | :-- | :-- | :-- |
| `bass` | B0 to F#4 (MIDI 23 to 66) | from the BPM | one note at a time |
| `lead` | C2 to B6 (MIDI 36 to 95), plucks too | from the BPM | one note at a time |
| `pad` | C2 to B6 (MIDI 36 to 95), chords too | 127.7 ms | chords: no octave check |

Detection still runs and prints its findings, so you can compare them
with the profile. `notes` decides voicing once for the whole stem. A
stem that mixes a chord part and a line gets a chord share between the
two and can read as one note at a time. The cleanup then runs the octave
check on its chords. When `detected voicing` reads monophonic on a stem
with held chords in it, pass `--profile pad`. Three more cases need a
profile: bleed from another instrument louder than -20 dB in a passage
where the part rests, a line with held double stops (`--profile pad`), and a strummed part
(`--profile lead`), where the pad settings lose notes.

## Check the file

Open `notes.mid` in a piano roll at the same tempo. It holds one track on
MIDI channel 1 with a tempo event and 4/4 at the start; every note has
velocity 100. Then check what the model cannot get right:

- No note below A0 (27.5 Hz): the model's lowest key.
- Note ends come late: a note often lasts until the next one starts.
- Octave errors at 110 Hz and below. The cleanup moves a weak octave copy
  that sounds inside its true note, but a lone note an octave off stays
  and is only flagged. A chord part keeps its octave copies: three
  generated triads give 10 notes for 9.
- A repeated note can merge into the one before it when the model misses
  its onset and the stem has no clear attack before it. A clear attack
  is a fall of 19 dB or more and a rise back about as fast as a step.
  Over a steady floor the fall must reach 20 dB below the note. The
  note before it must also have held for 60 ms and for 1.5 times the
  gap. These repeats merge this way:
  - a legato repeat;
  - an attack slower than about 10 ms;
  - a repeat after a note shorter than about 60 ms;
  - a gap longer than about two thirds of the note before it, such as
    staccato eighths at half length;
  - a gap that a tail or reverb fills.

  Most natural attacks are slower than 10 ms: a bowed, blown or sung
  re-attack rises like a tremolo, and the cleanup reads only the level.
  The model can read a low repeat with a short gap as part of one note:
  a 55 Hz saw gated with 30 ms gaps decodes as 9 notes for 16. From A1
  (55 Hz) up the cleanup splits such a note at a clear attack inside it,
  and the saw gives its 16. Below A1 the note stays whole, because there
  a fast sidechain duck looks like a gate.
- A square LFO that cuts the level for 20 to 40 % of each cycle looks,
  by level, like a gated note with short gaps. The cleanup reads it as a
  gate and can split a held note into repeats: an A1 under a 6 Hz square
  off for 30 % of each cycle gives 24 notes, one a pulse, where the
  model's onsets alone give 6. A vibrato under such a square splits the
  same way. Only a cue such as a phase reset at each repeat could tell
  the two apart. A square off half of each cycle holds only as long as
  it dips. No generated one split a held note further than the model's
  onsets do, and no sine tremolo of 2 to 20 Hz did.
- A held low note under a louder gated note of another key can split at
  that note's gates when the model splits it there: an A1 20 dB under a
  gated A2 gives 12 notes. Below A2 the cleanup cannot tell the two keys
  apart by their harmonics.

Plan a manual pass in the piano roll. The [pipeline
page](../analysis/pipeline.md#evaluation) gives the scores on generated
parts; real stems score lower.

## When it fails

A stem that gives no note writes nothing and exits with code 1. The
message names the stem and suggests a profile. Only digital silence,
every sample exactly 0, is sure to give no note: the guard gate is
relative to the loudest passage, so a file of DC alone or of faint hiss
gives notes. A folder that
holds the report of another stem stops the run unless you pass `--out` or
`--force`.

# Command line

The binary is `sound-to-patch`. This page shows the help of each command.
The help texts run in the test suite, so they match the built binary.
`cargo xtask dist` also renders them as man pages.

## sound-to-patch

```console
$ sound-to-patch --help
Turn an audio stem or a written sound description into a starting point in Vital or Serum 2.

Usage: sound-to-patch <COMMAND>

Commands:
  analyze  Analyse a stem and write layers.json and spectrogram.png
  notes    Transcribe a stem and write notes.mid
  preset   Read, inspect and write synth presets
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

```

## analyze

```console
$ sound-to-patch analyze --help
Analyse a stem and write layers.json and spectrogram.png.

Example: sound-to-patch analyze bass.wav --bpm 140

Usage: sound-to-patch analyze [OPTIONS] --bpm <BPM> <STEM>

Arguments:
  <STEM>
          The audio stem: WAV, AIFF, FLAC, MP3, AAC or M4A

Options:
      --bpm <BPM>
          The tempo of the stem in beats per minute, 4 to 1000

      --notes <FILE>
          A MIDI file with the stem's notes. Its tempo must match --bpm

  -o, --out <DIR>
          The output folder [default: a folder named after the stem, next to it]

      --force
          Replace a report of another stem in the output folder

      --synth <SYNTH>
          The target synth that the report names [default: vital]

          Possible values:
          - vital:  Vital, with `.vital` preset files
          - serum2: Serum 2, with `.SerumPreset` preset files

  -h, --help
          Print help (see a summary with '-h')

```

`analyze` decodes the stem at its native rate, runs the stem estimators
and writes two files into the output folder, which it creates:

- `layers.json`: the report. The [layer report page](formats/layers.md)
  lists every field.
- `spectrogram.png`: a 1500 by 512 pixel image of the busiest 10 s, in two
  panels. The [pipeline page](../analysis/pipeline.md#the-report) gives its
  layout.

Both files are replaced on the next run of the same stem; nothing else in
the folder changes. The same stem and flags give the same bytes. Each file
is written to a temporary sibling and renamed into place, `layers.json`
last, so an interrupted run never leaves a cut-short file, and a
`layers.json` that exists belongs to the spectrogram next to it. The
command prints one `wrote <path>` line per file to standard output, and a
warning line for each corrupt packet that became silence to standard
error.

`layers.json` records the stem's file name in `source`. When the folder
already holds the report of another stem, such as `bass.aiff` next to
`bass.wav` sharing `bass/`, the command stops and names both; `--out`
picks another folder, and `--force` replaces the report. An output folder
path that is a file, such as the default folder of a stem without an
extension, stops the command with a request for `--out`.

Without `--out`, the folder is named after the stem file and sits next to
it: `stems/bass.wav` writes `stems/bass/`. With `--notes`, the MIDI file
gives the note list. Its note times follow the file's tempo map, and every
tempo in the file must lie within 0.01 BPM of `--bpm`; a file without a
tempo event plays at the MIDI default of 120 BPM. Notes on channel 10, the
percussion channel, are left out. No transcription runs in
this release, so the report's `transcription` group is `null` with or
without `--notes`.

| Exit code | Meaning |
| :-- | :-- |
| 0 | Both files were written. |
| 1 | The stem or the note file cannot be read or is invalid, the selected synth has no adapter yet, the output folder is a file or holds the report of another stem, an output cannot be written, or the analysis fails internally. The message names the cause and the fix, and the file when one is involved. |
| 2 | A usage error, such as a missing `--bpm` or a tempo outside 4 to 1000. |

## preset

```console
$ sound-to-patch preset --help
Read, inspect and write synth presets

Usage: sound-to-patch preset <COMMAND>

Commands:
  inspect  Print the header fields and the decoded tree of a preset file
  apply    Apply an edit list to a template and write a preset
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help

```

## preset inspect

```console
$ sound-to-patch preset inspect --help
Print the header fields and the decoded tree of a preset file.

Example: sound-to-patch preset inspect bass.vital

Usage: sound-to-patch preset inspect [OPTIONS] <FILE>

Arguments:
  <FILE>
          The preset file, such as `bass.vital`

Options:
      --synth <SYNTH>
          The target synth [default: the extension of the preset file, else vital]

          Possible values:
          - vital:  Vital, with `.vital` preset files
          - serum2: Serum 2, with `.SerumPreset` preset files

      --full
          Print every list element and every text in full

      --shown <N>
          List elements printed before the rest is counted
          
          [default: 3]

  -h, --help
          Print help (see a summary with '-h')

```

## preset apply

```console
$ sound-to-patch preset apply --help
Apply an edit list to a template and write a preset.

Values are written with their unit, such as "1.2 kHz", "800 ms", "-6 dB", "+12 st" or "25%".

Example: sound-to-patch preset apply edits.json -o bass.vital

Usage: sound-to-patch preset apply [OPTIONS] --out <FILE> <EDITS>

Arguments:
  <EDITS>
          The edit list, a JSON file

Options:
  -o, --out <FILE>
          The preset to write, such as `bass.vital`

      --template <FILE>
          A preset to start from instead of the synth's built-in template

      --name <NAME>
          The preset name. Without it, the name of the output file

      --force
          Replace the output file if it exists

      --synth <SYNTH>
          The target synth [default: the extension of the preset file, else vital]

          Possible values:
          - vital:  Vital, with `.vital` preset files
          - serum2: Serum 2, with `.SerumPreset` preset files

  -h, --help
          Print help (see a summary with '-h')

```

The output file:

- must carry the synth's extension, such as `.vital`;
- must not exist, unless `--force` is given;
- must not be the edit list or the template;
- must not lie inside Vital's own `Factory` folder.

The output keeps the template's stamp, and the summary prints it. A stamp
with a newer first or second number than the stable build 1.0.7, such as
1.1.0, gives a warning: that build refuses it. The load gate ignores the
third number, so 1.0.8 gives none. A failed run leaves no file.

## The edit list

`preset apply` reads an edit list: a JSON object with the target synth and
a list of edits. Each edit is one of four records, and each carries a
`reason`.

| Record | Fields | Example |
| :-- | :-- | :-- |
| A control | `param`, `value` | `{"param": "filter_1_cutoff", "value": "400 Hz", "reason": "dark"}` |
| A menu choice | `menu`, `choice` | `{"menu": "filter_1_model", "choice": "Ladder", "reason": "warm"}` |
| A route | `route`: `source`, `dest`, `amount`, optional `polarity` | `{"route": {"source": "env_2", "dest": "filter_1_cutoff", "amount": "+24 st"}, "reason": "pluck"}` |
| A raw key | `raw`: `key`, `value` | `{"raw": {"key": "custom_warps_count", "value": 2}, "reason": "a newer build"}` |

- A control value carries its unit, as the [units page](units.md) lists:
  `"1.2 kHz"`, `"800 ms"`, `"-6 dB"`, `"+12 st"` or `"25%"`. A switch takes
  `true` or `false`, and a count takes a number.
- A route amount is in semitones when the destination is stored in
  semitones, such as a cutoff or a pitch, and in percent otherwise. A
  depth in semitones is divided by the destination's stored span, as Vital
  shows it.
- A route may target a number, a count, a menu such as `lfo_1_tempo`, or
  the amount or power of another slot. A switch and the bipolar, stereo and
  bypass flags of a slot take no route.
- A route that joins the same source and destination as an existing one
  edits it in place and keeps its polarity, curve and flags unless the edit
  sets them.
- A raw key is written as it is, outside the registry, and the output lists
  it as raw.
- Every problem of a list is reported at once, each with the edit, the
  cause and the fix.

The [guide](../guide/preset.md) walks through one edit list.

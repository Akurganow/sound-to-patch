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
  preset  Read, inspect and write synth presets
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

```

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

      --synth <SYNTH>
          The target synth [default: the extension of the preset file, else vital]

          Possible values:
          - vital:  Vital, with `.vital` preset files
          - serum2: Serum 2, with `.SerumPreset` preset files

  -h, --help
          Print help (see a summary with '-h')

```

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
  semitones, such as a cutoff or a pitch, and in percent otherwise.
- A raw key is written as it is, outside the registry, and the output lists
  it as raw.
- Every problem of a list is reported at once, each with the edit, the
  cause and the fix.

The [guide](../guide/preset.md) walks through one edit list.

# Presets

The `preset` commands read a preset file and write a new one from an edit
list. An edit list names controls and their values, such as a filter cutoff
of `400 Hz`. This page shows the steps for Vital. The
[CLI reference](../reference/cli.md) lists every flag, and the
[format page](../reference/formats/vital-preset.md) states what is known
about the file.

The commands on this page run in the test suite, so their output is the
output you get. They read `bass.json` and `typo.json` from
[`preset.in/`](https://github.com/Akurganow/sound-to-patch/tree/main/docs/src/guide/preset.in).

## Vital

### Choose the synth

`--synth vital` picks Vital. You can leave it out: the extension of the
preset file picks the synth, and Vital is the default.

### Write an edit list

A Vital preset holds about 800 controls. A **modulation slot** joins a
source, such as an envelope, to a control it moves. Vital has 64 slots.
This edit list turns on filter 1, picks its ladder model, sets the cutoff
and the attack, and lets envelope 2 open the filter:

```json
{{#include preset.in/bass.json}}
```

Each value carries its unit. A route amount is in semitones when its
destination is a pitch or a cutoff, and in percent otherwise. Every edit
needs a reason.

### Apply it

```console
$ sound-to-patch preset apply bass.json -o Bass.vital
wrote Bass.vital ("Bass") from built-in default
  set filter_1_on = on, stored 1.0
  set filter_1_model = Ladder, stored 2.0
  set filter_1_cutoff = 400 Hz, stored 67.34995771509334
  set env_1_attack = 0.005 s, stored 0.26591479484724945
  set slot 1 = env_2 -> filter_1_cutoff, amount 0.1875

```

The command starts from the **built-in template**: every control at its
default, our own sawtooth **wavetables** and an empty modulation matrix. A
wavetable is a set of single-cycle waveforms that an oscillator plays. The
built-in template is not Vital's Init preset.

The preset takes its name from the output file, so name the file after the
preset. Vital also shows the file name. The command never writes over its
edit list or its template, and it refuses a path inside a `Factory` folder.

A wrong unit stops the command before it writes anything. The message names
the edit, the cause and the fix:

```console
$ sound-to-patch preset apply typo.json -o Typo.vital
? failed
error: the edit list has 1 problem(s):
  edit 1 (env_1_attack): env_1_attack: cannot read "5 Hz" as Seconds: unknown suffix "Hz". Write a number followed by one of: s, ms. Write a value such as "800 ms"

```

### Read the result

`preset inspect` prints the header and the controls:

```console
$ sound-to-patch preset apply bass.json -o Bass.vital
...
$ sound-to-patch preset inspect Bass.vital
build         1.0.6
name          Bass
author        sound-to-patch
style         (none)
controls      772
unknown keys  0
...
  env_1_attack: 0.26591479484724945
...
  filter_1_cutoff: 67.34995771509334
...
  filter_1_model: 2.0
...
  filter_1_on: 1.0
...
```

The first line is the **stamp**, the Vital build the file claims. Vital
refuses a file whose stamp is newer than the app in its first two numbers.
Long lists show their first items; `--full` prints everything.

### Start from your own preset

A preset you saved from Vital carries the stamp your app expects. Pass it
with `--template`, and the edits apply on top of it:

```console
$ sound-to-patch preset apply bass.json -o Bass.vital
...
$ sound-to-patch preset apply bass.json --template Bass.vital -o Again.vital
...
```

The built-in template is stamped `1.0.6`, the open-source build, which
Vital 1.0.7 loads. For a newer build, such as the early-access 1.6, start
from a preset saved by your own app.

### Load it in Vital

1. Copy the file into `~/Music/Vital/User/Presets`, or use the menu item
   "Open External Preset".
2. Load the preset in a fresh instance.
3. Check that a control you edited shows its new value.
4. Play a note and check that the sound differs from the default patch.

A file is untrusted until Vital has loaded it. Check the sound, not only
the name: in a plugin host, Vital ignores a file it rejects and keeps the
last sound without a dialog.

- Results on Vital 1.0.7, the stable build:

  | Check | Result |
  | :-- | :-- |
  | A preset written from the two edits of the README quick start | loads; the attack shows 1 s and the note swells |
  | The same file stamped `1.5.5`, `1.6.0` or `1.7.0` | refused: the stamp is newer than the app |
  | The same file with keys of later builds | loads |
  | Save it again from the app | every control kept |

  The [format page](../reference/formats/vital-preset.md#open-items) lists
  what is still open.
- The 1.0.7 plugin bundles for macOS are Intel-only. The app runs on Apple
  silicon under Rosetta, but a plugin host running natively on Apple
  silicon cannot load the plugin.
- Presets saved by Vital 1.6 do not open in 1.5.
- Factory presets, wavetables and samples cannot be shared. Do not share a
  preset that embeds them.

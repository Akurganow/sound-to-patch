# Rendering a preset

`sound-to-patch render` plays a MIDI file through a preset in the synth's
own plug-in and writes the audio as a WAV file. It runs on macOS with
Vital. Serum 2 follows. The [CLI reference](../reference/cli.md#render)
lists every flag.

## What it needs

- Vital, installed as a VST3 plug-in in `/Library/Audio/Plug-Ins/VST3` or
  in the same folder under your user's `Library`. `--plugin` names another
  bundle.
- `sound-to-patch-host`, the program that loads the plug-in. It ships in
  the release archive next to `sound-to-patch`, and `render` looks for it
  there. `SOUND_TO_PATCH_HOST` names another copy.
- Rosetta on Apple silicon. Vital 1.0.7 holds Intel code only, so the host
  runs as an Intel program for it. `softwareupdate --install-rosetta`
  installs Rosetta.

The plug-in loads as a library, with no window, and the Vital app never
starts.

## Run it

Pass the preset, a MIDI file with its tempo, and the WAV file to write:

```sh
sound-to-patch render bass.vital --midi notes.mid --bpm 120 -o bass.wav
```

```text
plug-in: Vital 1.0.7 from /Library/Audio/Plug-Ins/VST3/Vital.vst3, in sound-to-patch-host 0.18.0 (vst3-host 0.10.0) as x86_64
preset: routes, 2x oversampling
notes: 3, the last note-off at 3.000 s, 120 BPM
init patch: 2 renders, 0.04 dB apart
load check: passed; the saved state matches, and the render lies 58.20 dB from the init patch, beyond the margin of 0.11 dB
clip: 5.589 s, the tail went quiet
wrote bass.wav
wrote bass.json
```

This is one local run: it played the `routes.vital` test fixture, the
built-in template with three routes, through Vital 1.0.7. The MIDI file
holds MIDI 48, 55 and 60 at velocity 100, one second each, back to back.
The dB figures change a little from run to run.

Each line names one step:

- `plug-in`: the synth, its build, the bundle and the host, with the
  architecture the host runs as.
- `preset`: the preset's name and the oversampling of the render. Every
  Vital render uses 2x oversampling, whatever the preset holds, because
  oversampling changes the sound.
- `notes`: how many notes the MIDI file holds, when the last one ends and
  the tempo. The file must be one that `analyze --notes` reads: every
  tempo in it within 0.01 BPM of `--bpm`, and the notes off channel 10.
- `init patch`: the plug-in renders the same notes twice with its own init
  patch before the preset loads. The two renders differ a little: Vital's
  oscillators randomize their start phase fully by default
  (`osc_1_random_phase` defaults to 1 in the registry).
- `load check`: see below.
- `clip`: the length of the WAV file. A clip is at least 4 s long. It ends
  once the output has stayed below -96 dBFS for 1 s after the last
  note-off. A tail still sounding 30 s after the last note-off is cut
  there, and the line says so.

## The load check

A plug-in can report that it loaded a preset and still play its old sound.
So a render counts only when two checks pass:

1. The clip sounds. A clip whose every sample stays below -96 dBFS is
   silent, and the error names what in the preset can explain it, such as
   no oscillator and no sample being on.
2. The state the plug-in saves after the render holds the preset: its
   name, its tuning and every numeric control, each number within one step
   of a 32-bit float. A preset without a tuning plays in Vital's default
   tuning, never in the tuning of the preset rendered before it. When
   the saved state is the init patch, the plug-in did not load the preset
   at all, as with a preset saved by a newer build of Vital.

Each failure has its own message, and the command writes no file.

A third check only warns. Each clip is measured as its level in octave
bands, and the distance from the init render is the largest band
difference in dB. A clip within three times the distance between the two
init renders, and within at least 0.1 dB, sounds like the init patch. The
render still counts, and `render` prints a warning. Mean band levels miss
a change of envelope or modulation: a slower attack on the init patch can
lie within the margin. The margin is a stopgap. Two init renders give no
distribution, and a later release measures the spread over many renders.

## The output

The WAV file holds 32-bit float stereo at 44.1 kHz. The record next to it,
`bass.json` here, names everything the audio depends on: the synth and its
build, the host, the recipe version, the sample rate and block size, the
settle time, the oversampling, the tempo, the clip length, the SHA-256 of
the MIDI file and the load check. The
[render record page](../reference/formats/render-record.md) lists every
field.

Pass the WAV file and the MIDI file to `analyze` to measure the render as
you would measure a stem:

```sh
sound-to-patch analyze bass.wav --bpm 120 --notes notes.mid
```

Two renders of one preset are not identical, so compare renders within
their spread, never sample by sample.

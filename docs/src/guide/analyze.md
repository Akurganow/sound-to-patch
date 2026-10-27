# Analysing a stem

`sound-to-patch analyze` reads one stem and writes a report of its level,
spectrum, band edge and stereo image, with a spectrogram image. The report
is what an agent reads before it writes an edit list for the
[preset commands](preset.md). The [CLI reference](../reference/cli.md#analyze)
lists every flag, and the [layer report page](../reference/formats/layers.md)
every field.

## Run it

Export one instrument as a WAV, AIFF, FLAC, MP3 or AAC file and pass its
tempo:

```sh
sound-to-patch analyze bass.wav --bpm 120
```

```text
wrote bass/layers.json
wrote bass/spectrogram.png
```

The files go into a folder named after the stem, next to it. `--out` picks
another folder. A warning about a packet that did not decode goes to
standard error; the stem keeps its timing, with silence in its place.

## Give the notes

When the MIDI file that played the stem exists, pass it:

```sh
sound-to-patch analyze bass.wav --bpm 120 --notes bass.mid
```

The notes set the frame length, 8192 samples for a bass below 100 Hz, and
let the report fill `centroid_over_f0` and the stereo phase `ipd_deg`. The
file's tempo must match `--bpm`. Notes on MIDI channel 10, the drum
channel, are left out, because their keys name drums, not pitches.

## Read the report

These excerpts come from a generated 4 s stereo tone, a 55 Hz sawtooth
gated in eighth notes at 120 BPM:

```json
{
  "schema": "layers/1",
  "source": "bass.wav",
  "synth": "vital",
  ...
}
```

`source` names the stem. A second stem that would write into the same
folder, such as `bass.aiff` next to `bass.wav`, stops with an error unless
you pass `--out` or `--force`.

```json
"centroid_hz": {
  "median": 352.12513307096737,
  "iqr": 102.05399835788126,
  "confidence": {
    "level": "high",
    "reasons": [{"code": "active_frames", "value": 159}],
    "spread": 102.05399835788126,
    "estimate": true
  }
}
```

A statistic over frames gives the median and the interquartile range of
the frames with sound, and its confidence names the evidence: here 159
active frames. A clip with fewer than 16 active frames gets no value and
the reason `too_few_frames`.

```json
"band_edge_hz": {
  "value": 1324.2919921875,
  "confidence": {
    "level": "low",
    "reasons": [
      {"code": "active_frames", "value": 159},
      {"code": "band_edge_drop", "value": 0.6521134784414393}
    ],
    "spread": 10.7666015625,
    "estimate": true
  }
}
```

The band edge is low here: the sawtooth has no steep cut, only its last
harmonics fading into the floor, so the fallback rule bounds where the
content ends. `band_edge_drop` says the steepest fall was under 1 dB.

The `conventions` group says how each number was measured, under the same
key as the number.

## Read the spectrogram

`spectrogram.png` shows the busiest 10 s of the stem in two panels: the
whole range from 20 Hz to 20 kHz, and 20 to 500 Hz with finer frequency
steps for the bass. Bright is loud; the colour bar spans 80 dB below the
loudest point of the stem. The image shows shape; the numbers are in the
report.

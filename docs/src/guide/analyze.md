# Analysing a stem

`sound-to-patch analyze` reads one stem and writes a report of its tempo,
notes, level, spectrum, band edge, stereo image, glides, pitch drops,
vibrato, amplitude envelope and sidechain pump, with a spectrogram image. The report is what an agent reads
before it writes an edit list for the [preset commands](preset.md). The
[CLI reference](../reference/cli.md#analyze) lists every flag, and the
[layer report page](../reference/formats/layers.md) every field.

## Run it

Export one instrument as a WAV, AIFF, FLAC, MP3 or AAC file and pass its
tempo:

```sh
sound-to-patch analyze bass.wav --bpm 120
```

```text
tempo: 120 BPM as given; the stem holds 7 beats, too few to refine it
detected range: MIDI 30 to 36, from the f0 track
detected voicing: monophonic, chord share 0.00
register window: 20 dB
cleanup voicing: monophonic, as detected
settings: detected, MIDI 30 to 36, minimum note 5 frames (58.0 ms), melodia on
decoded: 16 notes
octave check: 16 notes, 0 moved down an octave, 0 flagged, 0 unchecked
ghost merge: 16 notes
attack split: 16 notes
split join: 16 notes
quiet-note guard: 16 notes, 0 dropped below 40 dB
glides: 0, 0 too short
pitch drops: 0 of 15 low strikes
drop skips: 1 no onset, 15 no fall
vibrato: 0 of 0 steady spans
envelope: 16 of 16 notes read, 16 pooled, 16 with a note-off knee
envelope: attack 19 ms (upper bound), hold 0 ms, decay none, sustain 0.0 dB
pump: too few sustained beats
wrote bass/layers.json
wrote bass/spectrogram.png
```

The first line is the tempo: `--bpm` refined from the stem's onsets, as
[notes](notes.md#tempo) does it; `--bpm-exact` keeps `--bpm` as it is. This
4 s stem is too short to refine it. The next lines are the notes step of
[notes](notes.md), which `analyze` runs once to know the stem's notes;
`--profile` overrides its settings as there. `analyze` writes no MIDI file.
The four lines after them are the [pitch layer](../analysis/pitch.md): this
saw repeats one note with no glide or vibrato, and none of its 15 low
strikes falls in pitch. Each line lists what it found, with its numbers;
`drop skips` counts the low notes with no drop by the gate that stopped
each: no onset, settling, quiet, no strike, weak fundamental, no fall, tau
on bound, residual or off note.
The next three lines are the [envelope layer](../analysis/envelope.md).
The first counts the notes read, those pooled and those with a note-off
knee, and any skipped. The second gives the pooled medians: this saw
starts each note at full level, under 1.5 periods of 55 Hz, so the attack
is an upper bound; it holds its level, so it has no decay and a sustain
of 0 dB. The last is the pump: no beat here lies inside one held note from
50 ms before it, so none counts.
The files go into a folder named after the stem, next to it. `--out` picks
another folder. A warning about a packet that did not decode goes to
standard error; the stem keeps its timing, with silence in its place.

## Give the notes

When the MIDI file that played the stem exists, pass it:

```sh
sound-to-patch analyze bass.wav --bpm 120 --notes bass.mid
```

The file's notes replace the notes step, and the transcription model does
not run. The f0 track still reads the stem for the pitch layer, unless the
notes play chords.
The notes set the frame length, 8192 samples for a bass below
100 Hz, and let the report fill `centroid_over_f0` and the stereo phase
`ipd_deg`. The file's tempo must match `--bpm` or the refined tempo, so a
file that `notes` wrote at the refined tempo reads too. Notes on MIDI
channel 10, the drum channel, are left out, because their keys name drums,
not pitches.

## Read the report

These excerpts come from the run above: a generated 4 s stereo tone at
48 kHz, a band-limited 55 Hz sawtooth at amplitude 0.25 that sounds for
0.2 s of every 0.25 s, with the right channel at 0.8 of the left:

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
  "median": 3596.5198523563367,
  "iqr": 22.42862967814881,
  "confidence": {
    "level": "high",
    "reasons": [{"code": "active_frames", "value": 94}],
    "spread": 22.42862967814881,
    "estimate": true
  }
}
```

A statistic over frames gives the median and the interquartile range of the
frames with sound, and its confidence names the evidence: here 94 active
frames of 8192 samples, the frame length that a note list with a median f0
below 100 Hz picks. A clip with fewer than 16 active frames gets no value
and the reason `too_few_frames`.

```json
"band_edge_hz": {
  "value": 24000.0,
  "confidence": {
    "level": "low",
    "reasons": [
      {"code": "active_frames", "value": 94},
      {"code": "band_edge_drop", "value": 2.6500554517088695},
      {"code": "no_band_edge", "value": true}
    ],
    "spread": 5.859375,
    "estimate": true
  }
}
```

The sawtooth has harmonics up to the Nyquist frequency and no steep cut:
`band_edge_drop` says the steepest fall was under 3 dB. No bin rises the
fallback margin above the floor either, so the band edge is the Nyquist
frequency at low confidence, with the reason `no_band_edge`.

The `conventions` group says how each number was measured, under the same
key as the number.

## Read the spectrogram

`spectrogram.png` shows the busiest 10 s of the stem in two panels: the
whole range from 20 Hz to 20 kHz, and 20 to 500 Hz with finer frequency
steps for the bass. Bright is loud; the colour bar spans 80 dB below the
loudest point of the stem. The image shows shape; the numbers are in the
report.

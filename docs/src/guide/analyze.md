# Analysing a stem

`sound-to-patch analyze` reads one stem and writes a report of its tempo,
notes, level, spectrum, band edge, stereo image, glides, pitch drops,
vibrato, amplitude envelope, sidechain pump and voices, with a spectrogram image
and the final `notes.mid`. The report is what an agent reads
before it writes an edit list for the [preset commands](preset.md). The
[CLI reference](../reference/cli.md#analyze) lists every flag, and the
[layer report page](../reference/formats/layers.md) every field.

## Run it

Export one instrument as a WAV, AIFF, FLAC, MP3 or AAC file at 8000 to
192000 Hz and pass its tempo. A file at another rate is refused with its
rate and the range; 44.1 or 48 kHz is the usual export. A file of three or
more channels is read from its first two, with a warning that names the
channel count:

```sh
sound-to-patch analyze bass.wav --bpm 120
```

```text
tempo: 120 BPM as given; the stem holds 7 whole beats at 116.51 BPM, the slowest tempo of the search, and refining needs 8
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
tail: release none, rt60 none, weight 0.01, 1 of 5 reverb votes
pump: too few sustained beats
voices: mono, up to 1, mean 1.00; 15 joins, 0 legato
wrote bass/layers.json
wrote bass/spectrogram.png
wrote bass/notes.mid
```

The first line is the tempo: `--bpm` refined from the stem's onsets, as
[notes](notes.md#tempo) does it; `--bpm-exact` keeps `--bpm` as it is. This
4 s stem is too short to refine it. The next lines are the notes step of
[notes](notes.md), which `analyze` runs once to know the stem's notes;
`--profile` overrides its settings as there.
The four lines after them are the [pitch layer](../analysis/pitch.md): this
saw repeats one note with no glide or vibrato, and none of its 15 low
strikes falls in pitch. Each line lists what it found, with its numbers;
`drop skips` counts the low notes with no drop by the gate that stopped
each: no onset, settling, quiet, no strike, weak fundamental, no fall, tau
on bound, residual or off note.
The next four lines are the [envelope layer](../analysis/envelope.md).
The first counts the notes read, those pooled and those with a note-off
knee, and any skipped. The second gives the pooled medians: this saw
starts each note at full level, under 1.5 periods of 55 Hz, so the attack
is an upper bound; it holds its level, so it has no decay and a sustain
of 0 dB. The third gives the release, the RT60 and weight of any tail, and how
many of five signs say the tail is reverb. The last is the pump: no beat here lies inside one held note from
50 ms before it, so none counts.
The last line before the files is the [voices layer](../analysis/voices.md):
one voice at a time, and each of the 15 joins falls to silence, so none reads
legato.
The files go into a folder named after the stem, next to it. `notes.mid`
is the final note list, [below](#the-final-notesmid); it replaces the
first file of `notes`. `--out` picks
another folder. A warning about a packet that did not decode goes to
standard error; the stem keeps its timing, with silence in its place.

## The final notes.mid

`analyze` rewrites the note list with the layers and writes it as
`notes.mid`, one track on channel 1 at the used tempo, 480 ticks per
quarter note. The preset carries what the routes model, so the file holds
only notes.

Every rule below reads one voicing (owner decision, 2026-11-05):

| Line | When | Rules |
| :-- | :-- | :-- |
| Mono | The [voices layer](../analysis/voices.md) reads at most one voice at a time, or counts none, and the notes step ran without `--profile pad` | Glide joins, next-note cuts, one note per tick, bends; a level span stops at the next later onset |
| Polyphonic | The voices layer reads two voices or more at once, or the notes step ran with `--profile pad` | Pad ends; a level span runs past the next onset |

Both lines end the other notes at their knees or 50 ms before their list
ends. The voicing of the notes step, from its chord share, sets only the
notes step's settings and the pitch layer. So a stem of chords whose
chord share stays under 0.68 is still a polyphonic line here. Given
notes take no profile, so `--notes` leaves the voicing to the voices
layer. A layer that counts no voices leaves the line mono, because the
mono rules only join and cut notes.

- Each note ends at its note-off knee, so the release lives in the amp
  envelope and a render does not play it twice. A note without a knee,
  such as a pluck that decays at one rate, ends 50 ms before its end in the
  list; so does a note the envelope layer could not read, marked
  `unread`.
- A note of a polyphonic line ends where its level falls 20 dB below its
  plateau, marked `pad`: the plateau of its one harmonic, or of the full
  band when no harmonic of it is free, as in an octave stack. The search
  stops at the list end: the note-off is the gate, and the release past it
  belongs to the preset's amp envelope (owner decision, 2026-11-05). A note
  whose level falls short of 20 dB by then ends by the other rules.
- No note ends after its end in the list, except at a glide join. A knee,
  no-knee or pad cut never makes a note shorter than the minimum the BPM
  rule of the decoder gives at the tempo, 58 to 128 ms. The decoder drops
  notes that short on mono lines, and on polyphonic and pad lines it drops
  notes of 128 ms or less, whatever the tempo; the cleanup and a given
  `--notes` file can still hold shorter ones. A note the list gives
  shorter keeps its length. In a mono line a note ends where
  the next starts, even when that is shorter than the minimum.
- A glide becomes two overlapping notes on its two pitches. The later note
  starts at the glide's start, or at its own onset when the glide starts
  after it, and the earlier one ends 5 ms later, rounded up to whole
  ticks. In a mono line no other notes overlap. Of notes that start on
  one tick, one stays and the report counts the others: a note the
  transcriber does not flag as an octave guess before one it flags, then
  the longer, then the lower key. Their levels cannot rank them: both
  read the same mix from the same start.
- A pitch drop becomes one note at its settled pitch, and a note under a
  vibrato takes the key of the vibrato's mean pitch. Only the glides,
  drops and vibratos the report trusts above low count.
- Velocity follows the full-band level of each note when the spread of
  those levels exceeds 3 dB: the loudest note gets 127, and a note `d` dB
  under it `127 * 10^(-d / 40)`, at least 1. Otherwise every note gets 100.
  The full-band level is the peak of the one-period RMS of the whole mix
  from the onset to the note-off knee, so two notes compare even when they
  read different harmonics. On a chord it is the level of the mix, and
  tones of different levels do not tell apart. Each tone reads the mix
  with its own one-period window: the tones of a C3, E3 and G3 chord read
  within 0.65 dB of each other, and in an octave stack the lower
  tones read up to about 3 dB less. A stagger of the onsets moves no
  tone's level by 1 dB [synthetic]. A note with no frame to read reads its
  written span, which on a mono line stops at the next later onset; with
  no frame there either it gets 100.
- Pitch motion that no route models becomes pitch bends: a note of a mono
  line whose pitch stays 50 cents or more off its key, on one side, for at
  least the pitch track's reach (a quarter of its frame plus one period,
  26 ms at 392 Hz to 49 ms at 39 Hz), outside every glide, drop and
  vibrato. A shorter excursion fits inside what one track frame reads, so
  jitter on a low note or a hit inside a merged note writes no bend
  (owner decision, 2026-11-05). The pitch track reads a little of the note
  before at each note change, and while two notes sound it follows only
  one of them, so those frames count for no bend. Bends are clamped to 2 semitones, so set the synth's
  bend range to 2 semitones. [`render`](../reference/cli.md#render) reads
  no bend.

The `notes` group of `layers.json` lists each written note with the rule
that set its end, its level and the transcriber's octave flag.

### Limits

- A bend of a semitone or more becomes its own note at the transcriber,
  so it plays as a note change, not a bend. Of generated tones with a
  held bend, every +100 and +200 cent bend (120 of 120) split into its
  own note; of the +60 cent bends, 41 of 60 wrote bends, 13 split, 5
  stayed flat and 1 had no note [synthetic].
- A slow ramp that passes 50 cents only in its last frame writes no
  bend, because the offset never persists for the track's reach. Of the
  four generated bends of the tally set, two lost their bends this way:
  a rise of 0.5 semitones over 1.5 s, whose note reads +5 to +55 cents,
  and a 2-semitone ramp over 1 s that the transcriber split into notes
  57, 58 and 59, where note 58 reads -35 to +55 cents [synthetic].
- Of the tally's signals with motion that no route models, 7 runs lost
  bends to the persistence rule: 2 bends, 2 trills and 3 kick drops
  [synthetic]. The rule also took bends from 51 runs of glides, drops
  and vibratos that the routes model.
- Steady lines still bend in 27 of 502 generated runs without pitch
  motion, 3,194 events, against 156 runs and 28,807 events without the
  persistence rule [synthetic].
- A pad note whose level falls 20 dB only after its list end keeps the
  knee, no-knee or minimum end. Against the gate note-offs of the
  generated tally's 1,776 transcribed notes, a search past the list end
  raised the error from 239 to 346 ms MAE, and gave 177 ends more than
  1 s late [synthetic].
- The transcriber's notes of a mono line can overlap, so the voices layer
  reads the line as polyphonic and its notes get pad ends but no glide
  joins or bends: 41 of 133 transcribed runs of generated 808 and bass
  lines in one tally set [synthetic].
- The [pitch layer](../analysis/pitch.md) still reads the notes step's
  voicing, from its chord share, not the voices layer. On a stem of chords
  whose chord share stays under 0.68 it reads glides, drops and vibratos
  as on one line, while `notes.mid` treats the line as polyphonic and
  writes no glide join or bend.
  [#327](https://github.com/Akurganow/sound-to-patch/issues/327) aligns
  the two.

## Give the notes

When the MIDI file that played the stem exists, pass it:

```sh
sound-to-patch analyze bass.wav --bpm 120 --notes bass.mid
```

The file's notes replace the notes step, and the transcription model does
not run. The f0 track still reads the stem for the pitch layer, unless the
notes play chords.
The notes set the frame length: at 32 to 48 kHz, 8192 samples for a bass
below 100 Hz and 4096 above, scaled with the rate at other rates, so
96 kHz takes 16384 and 8192. They also let the report fill `centroid_over_f0` and the stereo phase
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
below 100 Hz picks at 48 kHz. A clip with fewer than 16 active frames gets no value
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

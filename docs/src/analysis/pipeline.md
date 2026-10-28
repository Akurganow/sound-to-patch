# Analysis pipeline

`sound-to-patch analyze` turns a stem into `layers.json` and
`spectrogram.png`, and `sound-to-patch notes` turns it into `notes.mid`.
This page follows the stem through each stage, with the formula, the
convention and the limits of each. The
[common methods page](methods.md) holds the rules that every stage
shares: the window, the frame rule, active frames and the statistics.

Every statement carries a tag:

- **[fact]**: the code does this, or a cited work states it.
- **[synthetic]**: a figure measured on generated signals. Generated
  signals are a best case: a real stem adds bleed, effects and noise.
- **[proposal]**: a threshold or rule chosen for this project that later
  work may tune.

`analyze` runs decoding, the tempo, the notes step (unless `--notes` gives
the notes), the stem, the stereo screen, the descriptors and the report
today [fact]. `notes` runs decoding, the tempo and the notes step [fact].
The analytic signal, the masks, the modulation spectrum, the periodogram and
the decay fits are library stages that the layer estimators of later
releases call [fact].

## Decoding

- The stem decodes at its native sample rate with every channel kept, as
  `f64` samples [fact]. The default audio track is the one the container
  flags as default, else the first with a known codec [fact].
- The build reads WAV, AIFF, FLAC, MP3, AAC (ADTS), MP4 or M4A, Ogg, MKV
  and WebM containers [fact]. A missing codec fails with its name and, if
  one exists, the decoder feature that adds it [fact].
- A packet that fails to decode becomes silence of its duration, so later
  samples keep their time, and the command prints a warning [fact]. More
  than 4 such packets fail the file [proposal].
- Mono is the channel mean, $m = (L + R) / 2$, with no gain normalisation
  [fact]. Analysis never resamples [fact].
- Resampling serves only a consumer with a fixed input rate, such as the
  22050 Hz transcription model [fact]. It is a band-limited FFT conversion
  with $\lceil n \cdot f_{to} / f_{from} \rceil$ output samples [fact]. Some
  rate pairs, among them 48 kHz to 22050 Hz, lag half an output sample, 23
  µs; the lag is documented and not compensated [synthetic]. A NaN or
  infinite sample stops the conversion with its index [fact].

## Tempo

`--bpm` is close to the stem's tempo, not exact: a tempo taken from a file
name is often 1 to 2 % off, and that drifts notes off the grid within a few
bars [proposal]. Both commands refine it from the stem before any step that
reads it: the minimum note length, the MIDI tempo and the beat grid
[fact]. `--bpm-exact` skips the search and uses `--bpm` as given [fact].

- Onset strength: the mono mix in periodic Hann frames of 46.4 ms (2048
  samples at 44.1 kHz; the power of two nearest 46.4 ms at other rates)
  with a hop of about 1 ms [fact]. The bins sum to power bands a third of
  an octave wide from 30 Hz, each at least 6 bins, so a band holds whole
  main lobes and a steady partial keeps its energy from frame to frame
  [fact]. Each band power is compressed as
  $\ln(1 + P / P_{knee})$, with the knee 60 dB below a full-scale bin
  [fact]. The strength is the summed rise of every band against the frame
  10 ms earlier [fact].
- The strength is low-passed at 20 Hz forward and backward, its centred
  mean over 100 ms is removed, and what stays below 10 % of the largest
  rise becomes 0, so only onsets remain [fact].
- Search: every tempo $t$ in steps of 0.01 BPM with
  $|t_{given} / t - 1| \le 0.03$ [fact]. Each scores the unbiased
  autocorrelation of the strength, read by linear interpolation at
  $k \cdot 60 / t$ seconds and summed over $k = 1$ to $K$ beats, with
  $K = \min(32, \lfloor \text{beats} \rfloor - 4)$ [fact]. The search
  spans 2.91 % below to 3.09 % above `--bpm`, so a `--bpm` 3 % off either
  way still holds the truth [fact]. The window is so narrow that half and
  double the tempo lie outside it [fact].
- Trust: the best tempo replaces `--bpm` when three things hold
  [proposal]:
  - Its score is at least 1.5 times the median score of the search
    (`tempo_peak`).
  - The onsets are hits, not swells: at least 0.9 of the energy of the
    band rise, before its low-pass, lies within 25 ms of its peaks, each
    the largest within 30 ms (`onset_sharpness`).
  - One of two shares is high enough. The beat share is the best score over
    $K$ times the zero-lag autocorrelation, and needs 0.4
    (`beat_periodicity`). The sparse share reads the same sum on every
    second or every fourth beat lag alone, the better of the two, and needs
    0.75 (`sparse_periodicity`), with at most 0.8 peaks of the onset
    strength per beat (`onsets_per_beat`). Both shares stop at 1.

  The sparse share is read once the stem holds 4 such lags, 12 beats for
  every second lag and 20 for every fourth [fact]. Otherwise the tempo is
  reported at low confidence and `--bpm` stays [fact]. A stem of fewer
  than 8 beats, or one without onsets, keeps `--bpm` and reports no
  refined tempo [fact].
- Generated drum, bass and lead parts at 70.3, 97.55, 123.4, 147.81 and
  178.2 BPM, 37 beats each at 22,050 Hz, with `--bpm` off by -3, -1, +1
  and +3 %: every refined tempo lies within 0.05 BPM of the truth
  [synthetic]. Their `beat_periodicity` reads 0.78 to 0.99; pad chords that
  change once a bar read 0.04 at most and keep `--bpm` [synthetic]. The
  0.4 threshold sits between the two [proposal].
- One kick a bar repeats on a quarter of the beat lags, so its
  `beat_periodicity` reads 0.22 to 0.24 and its `sparse_periodicity` 0.95
  to 0.97 at 37 beats [synthetic]. Kicks a bar and kicks on every other
  beat, on the same grid, refine to the exact tempo [synthetic]. A kick a
  bar clears 0.75 from about 24 beats: 0.81 at 24, 0.34 at 20 [synthetic].
- The sparse share needs more than the beat share, because its fewer lags
  line up by chance more often [fact]. Kick parts that keep `--bpm`, with
  it 3.5 % or 4 % off or under a tempo ramp of 1 %, read at most 0.63;
  sparse parts read 0.8 or more, swung or with onsets moved by 8 ms
  [synthetic]. The 0.75 threshold sits between them [proposal].
- A dense grid lines up with any spacing that is a whole number of its
  steps [fact]. Sixteenth hats at 70.3 BPM with `--bpm` 3.5 % off read a
  `sparse_periodicity` of 0.98 at 16/15 of the truth, where 4 beats are 15
  sixteenths [synthetic]. Kick-a-bar parts read 0.19 to 0.26 onsets per
  beat, kicks on every other beat 0.48 to 0.52, leads 1.23 or more and
  hats 3 or more [synthetic]. The 0.8 limit sits between them, so a lead or
  a hat part needs the beat share [proposal].
- A pad under a smooth tremolo of 1, 2 or 4 beats repeats as well as a
  drum part, and reads a beat or sparse share near 1 [synthetic]. Its rises
  spread over 180 ms or more, and its `onset_sharpness` reads 0.87 at most;
  chord pads read 0.76 at most, and drum, hat, bass, lead and kick parts
  0.985 or more [synthetic]. The 0.9 threshold sits between them
  [proposal]. Every tremolo case keeps `--bpm` [synthetic].
- Swing at two thirds of the beat refines within 0.05 BPM, and onsets moved
  by up to 8 ms at random within 0.1 BPM [synthetic]. A `--bpm` 3.5 % or
  4 % off, and a tempo ramp of 2 % or 3 % either way, keep `--bpm`
  [synthetic].
- A loop whose bar is not 4 beats long but lies within 3 % of it, such as
  a kick every 3.9 or 4.1 beats, refines to the tempo at which the loop is
  4 beats [fact]. The onsets cannot tell such a loop from one at that
  tempo [fact].
- A steady tone whose period divides the beat, such as 110 Hz at 120 BPM,
  finds no onset and keeps `--bpm` [synthetic].
- The beat offset stays 0: only the period is refined [fact]. A stem whose
  tempo drifts is not followed [fact].

The command prints one line for the step: the tempo used and why.

## The notes step

`sound-to-patch notes` turns a stem into `notes.mid`: one model run, one f0
track, the decode settings, the decode, the cleanup and the MIDI file, in
that order [fact]. `analyze` runs the same step once, without the MIDI file,
unless `--notes` gives the notes, and the model then never runs [fact]. The
notes set the frame length of the stem and fill `centroid_over_f0` and the
stereo phase [fact].

### Transcription model

- The notes come from a transcription model that ships inside the binary
  and runs on a pure-Rust inference engine, with no native library
  [fact]. The model and its decoder are credited on the
  [credits page](../credits.md).
- Licence of the weights [fact]. The bundled model file has no licence
  statement of its own. It equals the file in the upstream repository
  byte for byte: both have the SHA-256
  `2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec`,
  and a test pins that hash against the bundled bytes. The upstream
  repository grants Apache-2.0 to every file it holds, the model
  included. The upstream NOTICE text ships in `THIRD-PARTY-LICENSES.html`
  with every release.
- The model reads the mono stem resampled to 22050 Hz as 32-bit floats,
  in 2 s windows that start every 1.64 s [fact]. It returns three
  activation matrices with one frame every 256 samples (11.6 ms): note,
  onset and fine pitch, 88 keys from A0 to C8 [fact].
- It runs once per stem, and every decode reads that one run [fact]. The
  thread count does not change the activations; a test compares 1 and 8
  threads bit for bit [fact]. The inference engine's output differs
  between processors by up to about 5e-4, so a note near a threshold can
  flip from one machine to another [fact].
- A NaN or infinite sample stops the run with its index before any window
  runs [fact]. Silence gives no note and no error from the model [fact].

### Profiles

| Profile | Range | Minimum length | Voicing for the cleanup |
| :-- | :-- | :-- | :-- |
| `bass` | B0 to F#4, MIDI 23 to 66 | from the BPM | one note at a time |
| `lead` | C2 to B6, MIDI 36 to 95 | from the BPM | one note at a time |
| `pad` | C2 to B6, MIDI 36 to 95 | 11 frames, 127.7 ms | chords |

- A range becomes the decoder's frequency bounds: the frequency of its
  lowest key and of the key above its highest. Every range from MIDI 21
  to 108 keeps exactly its own keys [fact].
- Every profile keeps the decoder's thresholds (onset 0.5, note 0.3, 11
  quiet frames end a note), turns on the decode of notes without an
  onset, and turns pitch bends off [fact]. The ranges and lengths are
  [proposal].

### Minimum note length

- With the BPM rule, the shortest kept note is half a sixteenth note,
  $0.5 \cdot 15000 / \text{BPM}$ ms, clamped to 58 to 127.7 ms and
  rounded to whole model frames, half to even: 5 to 11 frames [proposal].
  At 120 BPM that is 5 frames, 58.0 ms [fact].
- The decoder keeps a note strictly longer than the setting: 11 frames
  keep notes of 12 frames, 139.3 ms, or more [fact]. A note grown without
  an onset needs one frame more [fact].

### f0 track

- One probabilistic YIN run covers the whole stem at 22050 Hz: frames of
  2048 samples, a hop of 128 (5.8 ms), pitch bins of a tenth of a
  semitone, from 25 Hz to 4200 Hz [fact]. The ceiling sits just above C8,
  the top of the transcription model: with a 400 Hz ceiling every tone
  above it read as a subharmonic [synthetic].
- The copy the tracker reads is scaled to a peak of 1, because the
  tracker treats a frame below a fixed energy as silence [fact]. A sine
  bank at 30.87, 41.2 and 55 Hz reads within 5 cents with no frame an
  octave off [synthetic]. Glides of 150 and 400 ms rise within 5 % of
  their true 10-90 % time; a 50 ms glide reads 41 ms, where the reference
  implementation reads 23 ms, because a 93 ms frame cannot resolve it
  [synthetic].
- The track fills each note's f0 contour with its voiced frames, and
  sets the register and the octave check [fact].

### Register and voicing

- Without `--profile`, the detection sets the decode settings from a
  probe decode over the model's whole range and the f0 track [fact].
- Voicing: a part is polyphonic when 68 % or more of the probe notes
  share at least half their span with a note of another pitch class
  [proposal]. On the generated set lines read 0.56 or less, the highest a
  melody with reverb, and chord parts 0.79 or more, the lowest a broken
  chord with the sustain held [synthetic]. Chords with releases 150 ms
  apart, a moving inner voice under held notes and strums 30 ms between
  strings all read 1.00 [synthetic]. An octave-up ghost shares its note's
  pitch class and does not count [fact].
- Register of a monophonic part: the 1st to 99th percentile of the voiced
  f0 frames within 20 dB of a reference level, plus 2 semitones on each
  side [proposal]. The reference is the 95th percentile of the voiced
  frames' levels, so a short loud burst, ring or thump does not move it
  [synthetic]. A polyphonic part takes the lowest and highest probe notes
  instead, because the tracker reads a chord as one low common period
  [synthetic].
- The range of a monophonic part widens to every hold candidate outside
  it, so a note the tracker misreads still lands inside [proposal]. A hold
  candidate is a probe note whose median frame lies within 10 dB of the
  reference and that sounds alone for at least half its span [proposal].
  A line plays one note at a time, so an octave-up ghost or bleed over the
  line shares its span with another pitch and does not count; a weaker
  note an octave away does not count against a note, since the decoder
  gives every note such a twin [synthetic]. In a fast run at 140 BPM that
  jumps from A2 to A3, the tracker reads 43 to 52 and the A3 widens the
  range to 41 to 59 [synthetic].
- A monophonic part takes the minimum note length from the BPM; a
  polyphonic part keeps the decoder's 127.7 ms [proposal].
- Limit: bleed within 20 dB of the stem's loud frames, in a passage where
  the part rests, enters the register window, so the range widens to its
  notes [fact]. Pass `--profile` for such a stem.
- Limit: a line with an occasional held double stop reads as monophonic,
  and the upper note of the stop can fall outside the range [synthetic].
  Pass `--profile pad` for such a stem.

### Cleanup

The cleanup runs four steps and prints the note count after each [fact].

- Octave check, on a part played one note at a time: each note meets the
  median pitch of the voiced f0 frames inside it, given 3 or more
  [proposal]. Within 1 semitone the note is fine. A note 12 semitones
  above the median, within half a semitone, over a median no more than
  half a semitone above A2 (110 Hz), is a copy when a note of the lower
  key holds it within 2 model frames at each end and has a higher
  velocity [proposal]. The copy moves onto that note, its key and span
  [fact]. Any other gap keeps the pitch and sets the note's octave flag
  [fact]. A chord part skips the check, because the tracker reads a chord
  as one low common period [synthetic].
- Why the velocity: the decoder gives every note of an octave bass a twin
  an octave away with nearly its span, and the tracker can hold the lower
  octave through a true upper note [synthetic]. On a bass that alternates
  A2 and A3 in sixteenths, true notes decode at velocity 81 to 100 and
  their twins at 34 to 52 [synthetic]. The velocity of a decoded note is
  its mean activation [fact].
- Ghost merge: notes of one key that overlap become one note, from the
  first onset to the last offset [fact]. A moved copy has its lower note's
  span, so it merges into that note alone and never bridges two repeats
  [fact]. The decoder gives no two notes of one key that overlap, so a
  decoded repeat never merges [fact]: on every generated part the merge
  removes exactly the moved copies [synthetic].
- Split join: a note that starts within 2 model frames (23 ms) of the end
  of a note of the same key, on an onset activation below 0.68, with a
  velocity no higher than that note's, joins it [proposal]. On the
  generated set every piece of a held note starts on 0.66 or less, and
  every repeated note on 0.70 or more: the weakest are sixteenths at
  175 BPM [synthetic]. The margin is thin on both sides, so a faster
  repeat can still join the note before it [synthetic]. A weaker piece before a
  stronger note is an early fragment of the attack and stays, since
  joining it would move the onset [synthetic].
- Quiet-note guard: a note whose span holds no 25 ms frame within 40 dB
  of the stem's loudest frame is dropped [proposal]. The model scales each
  window by its own level, so near-silent bleed fills its range and gives
  notes [fact].
- The ghost: a dry 110 Hz saw (a band-limited saw at amplitude 0.3, 0.5 s
  notes with a 5 ms attack and a 0.3 s linear release) decodes with the
  bass profile as the true note plus a 70 ms copy at MIDI 57 that ends
  about 80 ms after the onset [synthetic]. The copy needs a minimum length
  below 70 ms: the 11 frames of the `pad` profile remove it [synthetic].
  After the cleanup the stem gives the A2 alone [synthetic]. Six such
  notes through a 1.5 s reverb give seven notes, one of them a tail
  fragment, and six after the cleanup [synthetic].
- Limit: a lone note an octave above the tracker's reading is flagged,
  never moved [synthetic]. Only a weaker copy inside its lower note moves.
- Limit: a chord part keeps its octave copies, since it skips the check.
  Three generated triads decode as 10 notes for 9 [synthetic].
- Limit: the guard gate is relative to the loudest frame, so only digital
  silence, every sample exactly 0, gives no note. A file of DC alone or of
  hiss at -70 dBFS gives notes [synthetic].
- Limit: a repeated note whose onset the model misses merges into the
  note before it, and note ends stay late; a later release fixes ends
  [synthetic].

### MIDI file

- Format 0, one track, 480 ticks per beat, a tempo event of
  $\text{round}(60 \cdot 10^6 / \text{BPM})$ µs per beat and 4/4 at
  tick 0, every note on channel 1 [fact]. Our own MIDI reader reads the
  file back [fact].
- Every note has velocity 100: the decoder's velocity is a confidence,
  not a loudness [proposal].
- Times become ticks with $\text{round}(t \cdot \text{BPM} / 60 \cdot
  480)$ [fact]. A note lasts at least 10 ms and one tick; a note of one key
  is cut at the next onset of that key, and a note left with no length is
  dropped [fact]. A fractional pitch rounds to the nearest key [fact].
- A list with no note is an error, so a silent stem writes nothing
  [fact].

### Evaluation

`cargo xtask eval` scores the notes step on 37 generated parts with known
notes: saws, sines, an 808, plucks, pads, strums, a broken chord with
sustain, glides, octave basses, sixteenths at 175 BPM with repeats, a
fast run with an octave jump, reverb, bleed, loud transients over a bass line,
silence and an A6 saw [fact]. A note matches when its onset lies within
50 ms and its pitch within 50 cents, one-to-one [fact]. Onset F1 with
matched, estimated and reference notes, raw and after the cleanup
[synthetic]:

| Group | Right profile, raw | Right profile, clean | Detected, raw | Detected, clean |
| :-- | :-- | :-- | :-- | :-- |
| bass, 13 clean parts | 0.755 (239/381/252) | 0.893 (239/283/252) | 0.791 (239/352/252) | 0.905 (239/276/252) |
| bass, 4 parts with bleed | 0.457 (32/108/32) | 0.627 (32/70/32) | 0.615 (32/72/32) | 0.865 (32/42/32) |
| bass, 3 parts with transients | 0.622 (23/50/24) | 0.821 (23/32/24) | 0.742 (23/38/24) | 0.920 (23/26/24) |
| lead, 9 parts | 0.945 (130/143/132) | 0.974 (130/135/132) | 0.945 (130/143/132) | 0.974 (130/135/132) |
| pad, 7 parts | 0.784 (80/111/93) | 0.856 (80/94/93) | 0.784 (80/111/93) | 0.856 (80/94/93) |

Recall over each class, raw and after the cleanup, is the same for the
right profile and for detection [synthetic]:

| Class | Raw | Clean |
| :-- | :-- | :-- |
| bass, 20 parts | 0.955 | 0.955 |
| lead, 9 parts | 0.985 | 0.985 |
| pad, 7 parts | 0.860 | 0.860 |

- The cleanup removes no matched note on any part [synthetic].
- Silence gives no note under any setting [synthetic]. Detection scores
  no worse than the right profile on every class, raw and clean
  [synthetic].
- The strummed part matches 19 of its 32 notes under the pad settings,
  and 31 under the lead settings [synthetic]. Pass `--profile lead` for a
  strummed part.
- The 808 parts score an onset F1 of 0: the pitch is right, but the
  onsets come 100 to 170 ms late, after the pitch drop [synthetic].
- These are generated signals, a best case. Real stems add bleed, effects
  and noise, and score lower [proposal].

### Time budget

`cargo bench --bench notes` times each step on one generated 60 s stem at
22050 Hz, saw and sine notes from A2 to G4, with 10 samples per heavy
step [fact]. Medians, from the benchmark's own `median` estimate, with
nothing else running [synthetic]:

| Step | 1 thread | All threads |
| :-- | :-- | :-- |
| Model load | 17.6 ms | |
| Model run | 0.816 s | 0.101 s |
| f0 track | 29.31 s | |
| MIDI write | 3.9 µs | |
| End to end, without decoding the file | 30.05 s | 29.17 s |

- The budget is the median plus 25 %: 37.6 s per minute of audio at one
  thread and 36.5 s at all threads [proposal]. The cost grows linearly
  with the stem [fact].
- The f0 track runs on one thread and takes 98 % of the step at one
  thread; at all threads the step costs the track plus the model run of
  0.1 s, and the two medians lie within their run-to-run spread
  [synthetic].
- Peak memory, measured as the peak resident size of one process per
  stage with `cargo run --release --example notes_memory -- <stage>`
  [synthetic]:

| Stage | Peak memory |
| :-- | :-- |
| The stem alone | 14 MB |
| Model run | 165 MB |
| f0 track | 1.17 GB |
| Model run, decode and cleanup | 183 MB |
| End to end | 1.28 GB |

- The f0 track holds the peak; the decode and the cleanup add 18 MB to the
  model run [synthetic]. On a 4-minute stem the track alone peaked at
  4.1 GB and took 120 s [synthetic]. This release measures the cost and
  does not reduce it [fact].

## The stem

- Mid and side are $M = (L + R) / 2$ and $S = (L - R) / 2$ [fact]. A
  one-channel file's mid is its channel and it has no side [fact].
- The beat grid takes the period $60 / \text{BPM}$ seconds from `--bpm`
  and an offset of 0. The offset is given, never estimated [fact].
- The frame length of harmonic analysis is 8192 samples when the note list
  has a median f0 below 100 Hz, else 4096 [fact]. A Hann main lobe is about
  4 bins wide, so harmonics $f_0$ apart need $N \ge 4 f_s / f_0$ [fact].

### Band edge

The band edge is the highest frequency that stays above the noise floor.
A cutoff estimate within one octave of it is never reported [fact].

1. The long-term power spectrum of the mid is averaged over the active
   frames that lie wholly inside the stem, in dB [fact].
2. A running median over one semitone on each side smooths it [fact].
3. Above 2 kHz, the steepest fall of the smoothed level over one semitone
   is found. When it is at least 25 dB and the median level of the octave
   below lies within 20 dB of where the fall starts, the edge is its
   half-depth point, at high confidence [fact, proposal for the 25 and
   20 dB].
4. Otherwise the floor is the lowest level above 1 kHz, and the edge is
   the highest bin at least 30 dB above it, at low confidence. With no such
   bin the edge is the Nyquist frequency with the reason `no_band_edge`
   [fact, proposal for the 30 dB].

The reason `band_edge_drop` carries the depth of the steepest fall [fact].
The plateau check of step 3 keeps a lone tone's skirt from counting as a
cut [fact].

- Synthetic cuts fall 57 to 91 dB over one semitone, and a 2-pole roll-off
  at 3 kHz about 1.3 dB, so the 25 dB threshold separates them with room
  on both sides [synthetic].
- On white, $1/\sqrt{f}$ and $1/f$ noise cut at 8 and 16 kHz at 44.1 and
  48 kHz, the steepest fall erred +0.01 to +0.07 semitone and the margin
  rule +0.03 to +0.16 [synthetic]. With a noise shelf 60 dB down above a
  16 kHz cut, the margin rule erred +3.2 semitones and the steepest fall
  +0.02 [synthetic]. Full-band white noise and the 2-pole roll-off have no
  steep fall, and both rules give the same margin value [synthetic].

### Flat tops

`hard_limited` is true when runs of at least 1 ms of samples within 1e-5
of the channel peak cover at least 0.1 % of a channel [fact, proposal for
the thresholds]. A run counts only when the samples on both sides lie at
least half the peak on the same side: clipping runs into the ceiling,
while a square or pulse wave jumps to it [fact]. Square and pulse waves at
55 and 220 Hz do not flag [synthetic]. The level descriptors carry the
flag as a reason [fact].

## Stereo screen

The screen runs on L and R with 8192-point Hann frames for every f0
[fact].

- $\rho$ is the Pearson correlation of L and R at zero lag [fact].
- $\text{sm}_{dB} = 20 \log_{10}(\text{rms}(S) / \text{rms}(M))$ [fact].
- Per band the screen sums $|L_k|^2$, $|R_k|^2$ and
  $\operatorname{Re}(L_k \overline{R_k})$ over the bins inside the band,
  DC left out, and forms both values from the sums [fact]. The bands are
  sub below 120 Hz, low_mid from 120 Hz to 1 kHz and high above [fact,
  proposal for the edges].
- `ipd_deg` needs the median f0 of a note list. At each harmonic
  $h f_0$, $h = 1 \ldots 20$, it takes the largest cross-spectrum bin
  within 2 bins and the phase $|\angle(L \overline{R})|$, weighted by
  $|L \overline{R}|$, then the mean over active frames [fact].
- A frame is active from the RMS over both channels,
  $\sqrt{(\text{rms}_L^2 + \text{rms}_R^2) / 2}$, so an anti-phase stem with
  no mid still counts [fact]. $L = -R$ reads 180 degrees [synthetic].
- The screen cannot see an anti-phase chorus or a light reverb, and a
  mono source can still get width from a reverb tail. Its confidence
  carries the reason `stereo_blind_spot` [fact].
- Detuned unison voices share one bin at harmonics 1 and 2 of an
  8192-point frame, so their beat phase raises `ipd_deg` to 12 to 43
  degrees for panned 3- and 7-voice unison; harmonics where the voices
  resolve read 0.2 to 4.6 degrees [synthetic].

## STFT and peaks

- Frames are centred: frame $m$ covers samples $m H - N/2$ to
  $m H - N/2 + N - 1$, zero-padded past the ends [fact]. A signal of $n$
  samples gives $1 + \lfloor n / H \rfloor$ frames for an even $N$ [fact].
- The transform is not normalised:
  $X[m, k] = \sum_n w[n]\, x[mH - N/2 + n]\, e^{-j 2 \pi k n / (pN)}$,
  with oversampling $p$ that zero-pads each frame [fact]. Oversampling
  refines peak positions and changes no resolution [fact].
- A frame is computed one at a time, so a long stem never holds its whole
  spectrogram [fact].
- A peak is a strict local maximum, refined by a parabola through the log
  magnitudes $a, b, c$ of the bin and its neighbours:
  $\delta = \frac{1}{2} \frac{a - c}{a - 2b + c}$ [fact]. Each peak also
  records its height over its neighbours in dB [fact].
- For one 100 Hz sine at 30 dB SNR in 8192-point frames at 44.1 kHz, 300
  trials, the Hann peak has a standard deviation of 1.6 mHz against a
  bound of 0.7 mHz, and the rectangular window 22 mHz [synthetic]. Without
  noise the Hann bias stays below 0.016 bin without padding and 1.1 mHz
  with 4 times padding [synthetic].

## Analytic signal

- The analytic signal $z = x + j \mathcal{H}\{x\}$ comes from a zero-padded
  FFT [fact]. It means something only for one narrowband component, so a
  caller band-passes around one partial first [fact]. Its memory grows
  with the segment, so it serves note-sized segments only [fact].
- The heterodyne multiplies by $2 e^{-j 2 \pi f_c t}$, low-passes with a
  fourth-order Butterworth and decimates [fact]. A partial
  $A \cos(2 \pi f t + \varphi)$ comes out as
  $A e^{j(2 \pi (f - f_c) t + \varphi)}$, so the envelope reads $A$ [fact].
- The zero-phase low-pass runs the Butterworth forward and backward: -6 dB
  at the cutoff, 48 dB per octave, no delay [fact]. Each end is extended by
  an odd reflection of three cutoff periods, and each pass starts in the
  steady state of its input, so a constant or a ramp passes without a
  transient [fact]. Attack envelopes use a 150 Hz cutoff, because a 5 Hz
  smoother smears attacks shorter than tens of milliseconds [fact,
  proposal for 150 Hz].
- The band envelope mixes the band centre to DC, low-passes at half the
  bandwidth forward and backward with an even reflection at each end, and
  takes the magnitude [fact]. Its memory stays linear in the length
  [fact]. A 10 % AM at 55 Hz reads a depth of 0.1013 over the whole track
  [synthetic].

## Note masks

- The mask of note $n$ at frequency $f$ and time $t$ is
  $M_n(f, t) = \sum_{h=1}^{H} e^{-(f - h f_n(t))^2 / (2 \sigma^2)}$, with
  $f_n(t)$ following the note's contour [fact].
- The defaults are $H = 20$ harmonics, $\sigma = f_s / N$ (one unpadded
  bin) and a threshold of $e^{-2}$, the mask value two $\sigma$ from a
  harmonic, which covers the 4-bin Hann main lobe [fact, proposal for the
  values]. Harmonics at or above the Nyquist frequency are skipped [fact].
- Each bin of a frame records the note that claims it and whether a second
  note claims it too [fact].
- The out-of-mask share is the power $|X|^2$ outside every mask over all
  cells: bleed plus any note the list missed [fact].
- `analyze --notes` reads the note list from a Standard MIDI File: note-on
  to the next note-off of the same key and channel, first in, first out,
  with the times from the file's tempo map [fact]. Notes on channel 10,
  the percussion channel, are left out, because their keys name drums
  [fact].

## Modulation spectrum

- Band envelopes are tracked at 200 Hz, a 5 ms hop [fact].
- The relative modulation of a track is its natural log, floored 60 dB
  below the peak, minus its least-squares line [fact]. The log turns a
  decay in dB times an LFO into a line plus the LFO [fact]. A cubic lost a
  swell's 5 Hz LFO and a quintic misread a 2-cycle 4-bar LFO, where the
  line kept both [synthetic].
- The spectrum is the magnitude of the Hann-windowed track, zero-padded 8
  times; the resolution stays $1/T$ [fact].
- The scan runs from two semitones below $\min(0.2\ \text{Hz}, \text{the
  4-bar rate})$ to 20 Hz, so the 2-bar and 4-bar divisions have room for
  their peak [fact]. Above 20 Hz a modulation shows as sidebands, not in
  the envelope [fact].
- The rate is the strongest strict maximum in the scan, with guard points
  outside it as neighbours, refined by a least-squares sine fit [fact]. It
  needs a line 8 times the median amplitude of the scan, a depth of at
  least 0.5 % and two full cycles, else no rate [fact, proposal for the
  thresholds]. The reasons `line_prominence` and `cycles_in_span` carry
  both values, with a rate or without one [fact].
- On 200 seeded noise tracks the largest line stood at most 4.4 times the
  median [synthetic]. On a 104-case grid of periods, depths and rates the
  strongest line missed 0 sine, 8 saw, 4 square and 4 triangle cases
  [synthetic]. Noise, a steady tone and a pure decay give no rate
  [synthetic].
- The period search uses the cumulative mean normalised difference of the
  detrended track: the first lag below 0.1, else the global minimum,
  refined by a parabola [fact]. A 7.3 Hz modulation reads 7.30 Hz
  [synthetic].
- Folding averages the track over a period into 64 phase bins [fact]. The
  shape reads square-like with a plateau of 35 % of the cycle, a ramp up
  with a rise of 75 % or more, a ramp down with 25 % or less, and smooth
  otherwise [proposal].
- A rate snaps to the nearest note division in log distance, at the BPM
  and at tempos within 2 % in 0.1 % steps [fact].

## Periodogram

Note gaps make a plain FFT see the note rhythm as modulation, so gated
samples go to the Lomb-Scargle periodogram [fact].

- A sample counts when its level lies within 40 dB of the track's peak,
  inside a note, at least 60 ms after the onset [fact, proposal for 60 ms].
- Each trial frequency fits a mean, a sine and a cosine together: the
  floating-mean form [fact]. On gated notes the plain form put a rate at the
  note rate on its harmonics [synthetic].
- The power is $A^2 N / 4$ for a sine of amplitude $A$ over $N$ samples,
  and a constant offset changes nothing [fact]. The line amplitude is
  $2 \sqrt{P / N}$ of the log level, the same depth as in the modulation
  spectrum [fact].
- The grid steps by 0.01 Hz, and the pick and its thresholds match the
  modulation spectrum [fact]. The spread is the resolution $1/T$ of the
  span [fact].

## Backward integration

- The decay curve of a tail after a stop is the Schroeder integral of the
  squared signal, from each sample to the end [fact]. A stem has no impulse
  response, so the stop comes from the note list, never from a silence
  [fact].
- The analysis starts at the loudest 10 ms block. The floor is the mean
  power of the last 10 % of the tail [fact].
- A line through the 10 ms block levels down to 10 dB above the floor gives
  the time where the decay meets it; the integral stops there and adds the
  energy the fitted decay would add after it [fact].
- The decay time is $-60 / \text{slope}$ of a line fitted between -5 and
  -25 dB (T20) or -5 and -35 dB (T30) [fact]. A fit fails when the floor
  lies less than 10 dB below the bottom of its range [fact, proposal for
  10 dB].

## Descriptors

Descriptors run on the mid with Hann frames of the frame rule, over active
frames [fact].

- Shape descriptors use magnitude weights $|X_k|$ divided by the window
  sum [fact]. The centroid is $\sum_k f_k |X_k| / \sum_k |X_k|$, the
  spread $\sqrt{\sum_k (f_k - c)^2 |X_k| / \sum_k |X_k|}$, and the rolloff
  the lowest bin where the cumulative magnitude reaches 85 % or 95 %
  [fact].
- Flatness is the geometric over the arithmetic mean of the magnitude from
  100 Hz to 10 kHz, each bin floored at $10^{-5}$ after the window-sum
  scale [fact].
- Band shares use power $|X_k|^2$ summed over active frames per band,
  divided by the power in the bands reported; the edges are 20, 60, 250
  Hz, 1, 4, 10 and 20 kHz [fact, proposal for the edges]. A band that
  starts at or above the Nyquist frequency is left out, and one that
  crosses it ends there, so a 16 kHz file reports five bands, the last
  from 4 to 8 kHz [fact].
- Every statistic over frames, the band shares and the band edge need at
  least 16 active frames, else no estimate with the reason
  `too_few_frames` [fact, proposal for 16]. On noise, sawtooth and sine
  clips at 48 kHz the worst error of the medians against a 10 s run was 4 %
  at 10 frames and 2 % at 16, and many times the value at 8 or fewer
  [synthetic].
- Peak and RMS are of the mono mix over the whole stem in dB re full scale
  1.0, and the crest is their difference [fact]. Digital silence gives no
  estimate with the reason `silent_stem`, never $-\infty$ [fact].
- The conventions of published tools differ in weights, floors and
  rolloff arguments, so a value is comparable only with its convention
  [fact]. An ideal 55 Hz sawtooth has a magnitude-weighted centroid of
  3348 Hz and a power-weighted one of 220 Hz, 15 times lower [synthetic].
- `centroid_over_f0` divides each frame's centroid by the median f0 of the
  note list [fact].

## The report

- `layers.json` holds the `schema` and `synth` fields, the `stem` group,
  the `transcription` group and the `conventions` group; the
  [layer report page](../reference/formats/layers.md) lists them [fact].
- Output is deterministic: two runs on the same input write the same bytes
  [fact]. `layers.json` names its stem in `source`, and a run of another
  stem into the same folder stops unless forced [fact].
- Each file is written to a temporary sibling and renamed into place, and
  `layers.json` comes last, after the old one is removed. A `layers.json`
  that exists therefore always belongs to the spectrogram next to it, and
  neither file is ever cut short [fact].
- `spectrogram.png` is 1500 by 512 pixels, below the size at which a
  hosted image reader downscales [fact]. The image gives coarse shape; the
  numbers live in the JSON [fact].
- The top panel uses 4096-point frames from 20 Hz to 20 kHz, the bottom
  panel 16384-point frames from 20 to 500 Hz, both on a log-frequency axis
  [fact]. Each row takes the largest bin magnitude in its band, or the
  linear interpolation at its centre when the row is narrower than a bin
  [fact]. One frame is centred on each column [fact].
- The passage is the 10 s window with the most active 4096-point frames,
  the earliest on a tie, or the whole stem when shorter [fact, proposal for
  the rule].
- Colour is viridis over 80 dB below the stem peak: the loudest bin above
  DC over the whole stem at the panel's frame length [fact]. A relative
  scale keeps strong partials from clipping to the top colour [fact].
  Every level is finite and clamped before the colour map [fact].

## Sources

Read for the problem statements and the formulas; the code is our own.

- Window choice and the peak accuracy bound: Harris,
  [On the use of windows for harmonic analysis with the discrete Fourier transform](https://doi.org/10.1109/PROC.1978.10837);
  Rife and Boorstyn,
  [Single-tone parameter estimation from discrete-time observations](https://doi.org/10.1109/TIT.1974.1055282).
- Analytic signal: Boashash,
  [Estimating and interpreting the instantaneous frequency of a signal](https://doi.org/10.1109/5.135376).
- Harmonic masks: Klapuri,
  [Multiple fundamental frequency estimation based on harmonicity and spectral smoothness](https://doi.org/10.1109/TSA.2003.815516).
- Modulation spectrum and period search: Atlas and Shamma,
  [Joint Acoustic and Modulation Frequency](https://doi.org/10.1155/S1110865703305013);
  de Cheveigné and Kawahara,
  [YIN, a fundamental frequency estimator for speech and music](https://doi.org/10.1121/1.1458024).
- Periodogram: Lomb,
  [Least-squares frequency analysis of unequally spaced data](https://doi.org/10.1007/BF00648343);
  Scargle,
  [Studies in astronomical time series analysis. II](https://doi.org/10.1086/160554);
  Zechmeister and Kürster,
  [The generalised Lomb-Scargle periodogram](https://doi.org/10.1051/0004-6361:200811296).
- Backward integration: Schroeder,
  [New Method of Measuring Reverberation Time](https://doi.org/10.1121/1.1909343).
- Descriptor definitions: Peeters et al.,
  [The Timbre Toolbox](https://doi.org/10.1121/1.3642604).

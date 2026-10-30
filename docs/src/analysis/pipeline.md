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
the notes), the [pitch layer](pitch.md), the
[envelope layer](envelope.md), the stem, the stereo screen, the
descriptors and the report today [fact]. With `--notes` the f0 track still
reads the stem, for the pitch layer [fact]. `notes` runs decoding, the tempo and the notes step [fact].
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
  bar at 120 BPM refines from 21 beats, once the share reads every fourth
  lag; at 20 beats it reads 0.34 [synthetic].
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
- A pad under a smooth tremolo repeats as well as a drum part, and reads a
  beat or sparse share near 1 [synthetic]. Its rises spread over 180 ms or
  more [fact]. A chord under a cosine tremolo of half a beat to 4 beats
  reads an `onset_sharpness` of 0.80 at most where its shares pass, and
  keeps `--bpm` [synthetic]. Chord pads read 0.76 at most; drum, hat,
  bass, lead and kick parts read 0.985 or more, and a bass with an 80 ms
  attack 0.92 [synthetic]. The 0.9 threshold sits between them
  [proposal].
- Only a smooth tremolo keeps `--bpm`: one at an eighth-note rate or
  slower, with a gentle shape [synthetic]. A gate reads as hits [fact]. A
  chord gated by a square LFO, or pumped with an 80 ms recovery as by a
  sidechain, every half beat to 2 beats at 117 or 123 BPM with `--bpm 120`,
  refines to the tempo of the gate [synthetic]. That is the right tempo
  when the gate is locked to the song [fact].
- Swing at two thirds of the beat refines within 0.05 BPM, and onsets moved
  by up to 8 ms at random within 0.1 BPM [synthetic]. A `--bpm` 3.5 % or
  4 % off, and a tempo ramp of 2 % or 3 % either way, keep `--bpm`
  [synthetic].
- A kick a bar at any tempo from 40 to 180 BPM in 0.5 BPM steps, 37 to 64
  beats long, with `--bpm` 1 % off, refines to within 0.01 BPM
  [synthetic]. Most candidates score exactly 0 for such a part, so the
  median score can be 0; the peak ratio divides by at least $10^{-9}$ of
  the best score, and reads at most $10^9$ [fact].
- Limits:
  - A loop whose bar is not 4 beats long but lies within 3 % of it, such
    as a kick every 3.9 or 4.1 beats, refines to the tempo at which the
    loop is 4 beats [synthetic]. The onsets cannot tell such a loop from
    one at that tempo [fact].
  - Silence dilutes the sparse share [fact]. A kick a bar of 37 beats keeps
    `--bpm` with 12 beats of silence after it or 24 before it [synthetic].
  - A soft attack lowers the sharpness [fact]. A bass in eighth notes with
    a linear attack of 100 ms keeps `--bpm` at 178.2 BPM, and one of
    120 ms at every tempo tested [synthetic]. Another sound may reach that
    point sooner.
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

The cleanup runs five steps and prints the note count after each [fact].

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
- Attack split: a note of A1 (55 Hz) or above splits where the stem has
  an attack of its own inside it [proposal]. A 25 ms frame of the note
  marks a dip when it is no louder than the frame before it, quieter
  than the frame after it, and 19 dB or more below the loudest frame in
  the 50 ms after it. The note must hold a frame at six tenths of that
  loudest frame or more, centred 60 ms or more before the dip. The split
  falls on the last envelope reading at a tenth of the loudest reading
  after the dip, before the first reading at nine tenths after the
  quietest of the dip frame's 26 readings, 1 ms apart, read as in the
  rise test below. The note before the dip can read louder than the one
  after it, so the climb starts at the quietest reading, and a deeper
  gap later in the 50 ms belongs to a dip of its own, so it cannot move
  the cut past a repeat [fact]. At the same times the level at the
  note's fundamental alone, one DFT bin over one period, must reach 23
  dB above its quietest reading before the split in a reading from the
  split on. The piece after the split must last 50 ms, and the attack
  rule below must hold between the two pieces. Each piece keeps the
  note's key, octave flag and velocity [fact]. A last piece of at most
  100 ms that runs into the decoder's next note of its key, within 2
  model frames, starts that note instead, unless that note has an attack
  of its own after the piece by the same rule [fact].
- Why the split: the decoder can read a low repeat with a short gap as
  part of one long note, and no later step sees two notes [synthetic].
  With 30 ms gaps the gated 55 Hz saw decodes as 9 notes for 16, or 8
  with a 5 ms tail, and the split gives all 16 in both [synthetic].
- Why the loud frame before the dip: the decoder can start a note in the
  gap before its own attack, and a split there would cut off a piece of
  silence [proposal].
- Why the fundamental: below about A2 the harmonics' window of the
  attack rule cannot tell a note from a key nearby, so a held note under
  a louder gated voice of another key dips at its harmonics as well as
  in the stem [fact]. A bin over one period has its zeros on every
  harmonic of the note when the period is a whole number of samples, so
  a tone an octave up reads nothing there [fact]. The period is rounded
  to whole samples: at 22,050 Hz and at the worst phase a harmonic leaks
  about -70 dB at A1 and -46 to -50 dB from A2 to A5 [fact]. The bin
  rejects only the note's harmonics: a key 1 to 3 semitones up reads
  within 1 dB of the note, and a fifth up 2 to 6 dB under it [fact]. A
  held A1 20 dB under an A2 gated for 0.2 s in every 0.25 s gave 16
  notes where the split join alone keeps 12; with the fundamental's dip
  it keeps the 12 [synthetic]. Held A1s 6 to 30 dB under a gated D2, C2,
  E2, A2 or A3 give the split join's notes in every case [synthetic].
  Held A1s 6 to 30 dB under a gated key 1 to 3 semitones above or below,
  F#1 to C2, give main's note counts too, though the bin reads such a
  key nearly as loud as the note [synthetic]. There the decoder already
  cuts the A1 at the gates or drops it, so no dip falls inside an A1
  note: where the A1 decodes across a gate, 6 to 14 dB down, the stem
  dips there by about that much, short of 19 dB [synthetic]. No
  generated semitone case reaches the fundamental test [synthetic].
- Why 23 dB: from 21 to 25 dB the generated sets give the same notes,
  and 23 dB lies in the middle [synthetic]. At 19 and 20 dB squares
  loud for 60 to 80 % of the cycle split a held note further in 72 runs
  where 23 dB gives 52, and at 30 dB gated runs hit 8 fewer gates
  [synthetic].
- Why the next note starts at the cut: on a real vocal stem the decoder
  started the next note of a key 58 ms after its attack, and the cut
  there left a fragment of 58 ms before it [fact].
- Why 100 ms: the real fragment lasted 58 ms, and a piece much longer
  than the decoder's lag is more likely a note of its own [proposal]. A2
  gates to 200 ms, then a repeat after a gap of 10 to 20 ms, which the
  decoder starts 5 ms early: the attack rule finds no attack there, and
  without the limit the 200 ms piece would start the repeat [synthetic].
  The limit leaves the same loss for a shorter gate, as the limits below
  give [synthetic].
- Why the 50 ms after the split: a decoded note that starts late in one
  gated repeat runs into the next, and a split at that repeat left a
  fragment of about 17 ms before the gate closed [synthetic].
- Why A1: below it the envelope window of one period lasts 19 ms or more,
  as long as the recovery of a fast sidechain duck [fact]. Ducks to
  -40 dB split at B0 and C#1, and so do ducks to -40 dB that recover in
  30 to 45 ms on every key from D1 to G#1 [synthetic]. Full-depth sine
  tremolos of 10 Hz split at A0 and B0 [synthetic]. At E1 such a duck
  and a gate with a 30 ms gap and a 5 ms tail read the same trough,
  0.069 and 0.066 of the peak, and the same onset activation, 0.27 to
  0.44 and 0.23 to 0.39 [synthetic]. From A1 to C4 no duck to -40 or
  -48 dB that recovers in 25 to 45 ms splits [synthetic].
- What the split was measured on [synthetic]: the four sets below and 16
  A1 sixteenths with gaps of 35 to 44 ms, each stem decoded with the
  detected profile and with the bass profile, a sweep of 1053 ducked and
  tremolo stems, and the 37 eval parts. The sweep holds ducks to -40 and
  -48 dB that recover in 25 to 60 ms, linear or exponential in decibels,
  at three phases, on every key from D1 to A2 and on C3, E3, A3 and C4,
  with full-depth sine tremolos of 8 to 14 Hz from D1 to A1.
- On those sets no held run splits further than without the split, but
  for the squares the limits below give [synthetic]. On the first set
  8 of the 716 runs come closer to the true notes, and gated runs hit
  1245 gates where they hit 1205. On the second set 12 of the 130 gated
  runs and 10 of the 24 two-voice runs keep more repeats, 1666 gates hit
  where 1558 were. The fourth set keeps the same notes. Every A1
  sixteenths run keeps more: 872 of 1024 gates hit where 214 were
  [synthetic]. On the eval the dry 110 Hz saw with a 0.3 s release keeps
  all 6 of its notes where it kept 5 [synthetic]. The bass clean group's
  onset F1 rises by 0.002 to 0.003 under every setting and the bass
  recall after the cleanup from 0.955 to 0.958; no other cell moves
  [synthetic].
- Split join: a note that starts within 2 model frames (23 ms) of the end
  of a note of the same key joins it when three things hold [proposal].
  Its onset activation lies below 0.68, its velocity is no higher than
  that note's, and the stem gives it no attack of its own.
- The attack rule tells a repeat from a split [proposal]. It reads the
  stem in the guard's 25 ms frames, one every 6.25 ms, with any DC offset
  removed. The quiet frames are those centred from 75 ms before the onset
  to the onset, but not before the earlier note starts. The loud frames
  are those centred from the onset to 50 ms after it, but not past the
  note's end. A note has its own attack when three things hold:
  - The stem's loudest frame lies 19 dB or more above its quietest one.
  - The level at the note's first 8 harmonics, read with a Hann window
    over each frame, lies 22 dB or more above its quietest one.
  - The stem does not rise slowly. Its envelope, read every 1 ms over one
    period of the note and at least 4 ms, climbs from a tenth of its
    loudest reading after the onset to nine tenths of it. The fastest
    such climb, from a start no later than 20 ms after the onset, takes
    at most 0.8 of the window plus 7 ms, the limit. A step climbs through
    the window in 0.8 of it, the difference of the squares of the two
    shares. The readings run to 20 ms plus three limits after the onset.
    An envelope that never falls to a tenth climbs from no silence and
    fails, so a steady floor must lie 20 dB down. A climb counts only
    after a hold. The dip runs from the last reading at six tenths of the
    loudest one or more, at most 400 ms before the climb, to the first
    such reading after it starts. Before the dip the envelope stayed
    above half of the loudest reading for 60 ms and for 1.5 times the
    dip. An envelope that stays below six tenths for the whole 400 ms
    dipped too long for a tremolo, and holds. The level before the stem
    starts is unknown, so a dip or a hold that reaches the start is too
    short.
- Why the stem's dip: under a line that moves to other keys, a split
  piece of the old key sees the new note's attack in the stem, but the
  stem never falls far between them [synthetic]. On the generated set
  such pieces dip 14.6 dB or less, a sidechain duck to -16 dB 14.3 dB or
  less, and a gated repeat 19.6 dB or more [synthetic].
- Why the harmonics' dip: through reverb, the old key's tail sits under a
  new note of another key, and the stem then dips up to 19.8 dB
  [synthetic]. The tail's own harmonics dip 18.1 dB or less there
  [synthetic]. Below about A2 the window cannot tell a note from a tone a
  fifth or a tritone away, so there the harmonics' dip is a dip of the
  stem's low band [fact].
- Why the rise test: a tremolo to silence dips as far as a gate, but
  rises slowly [synthetic]. A full-depth sine tremolo at `r` Hz climbs in
  about 0.3 / `r` s: 37 ms at 8 Hz, 21 ms at 14 Hz [fact]. A gate climbs
  in 0.8 of the window plus about its attack. One period reads a tone
  steady and keeps the dip of a tremolo at A1 [synthetic]. Two periods
  smooth that dip until its climb cannot be timed, and allow about 36 ms
  at A1 [synthetic]. A window of 1.5 periods ripples, and a gate then
  climbs too slowly [synthetic]. A tremolo that climbs within the limit
  peaks about 1.7 limits after its trough [fact]. Readings that end
  before that peak take a level on the climb for the loudest one, and a
  slow climb to it reads fast: at A1 a full-depth tremolo of 12 Hz split
  at some phases with readings to 50 ms [synthetic]. Three limits past
  20 ms hold the peak [synthetic].
- Why the hold: below A1 one period spans 24 to 36 ms, as long as the
  climb of a full-depth tremolo of 14 to 20 Hz, so the climb alone cannot
  tell it from a gate [synthetic]. A gated note holds its level until its
  gap. A full-depth sine tremolo stays above half its peak for 0.47 of
  its cycle before it falls to six tenths, under 60 ms above 7.8 Hz
  [fact]. The fall of a gate ripples by a few hundredths through the
  window, so a hold that ended at half could end on a ripple: at E1 a
  repeat after a 50 ms gap read just under half its peak 1 ms before the
  fall and joined [synthetic]. On the generated sets 50 to 70 ms keep
  every repeat and split no sine tremolo [synthetic]. At 45 ms
  full-depth tremolos of 14 Hz on E1 split, and at 80 ms sixteenths on
  E1 with attacks of 19 and 23 ms join [synthetic].
- Why 1.5 times the dip: a square tremolo rises like a gate, and slower
  than about 9 Hz it holds through each pulse for 60 ms [synthetic]. Off
  half its cycle it holds as long as it dips, and a gated note holds
  longer than its gap [fact]. At 1.5 no square tremolo off half its cycle
  splits a held note on the generated sets [synthetic]. At 1.25 a 9.1 Hz
  square on C2 splits a held note into 10 where the onset rule gives 5.
  At 2 eighths on E1 with a 10 ms attack and a 10 ms tail over a -40 dB
  floor keep 3 of 16 repeats in the bass profile, as the onset rule
  does, where 1.5 keeps 12 [synthetic].
- Why 400 ms: a sine tremolo that reads as fast runs faster than about
  8 Hz, and at full depth stays below six tenths of its peak for about
  70 ms of each cycle at 8 Hz [fact]. A square tremolo off half its cycle
  stays low for 167 ms at 3 Hz, and the search must reach its fall for
  the ratio to weigh it [fact]. On the generated sets 200 to 500 ms give
  the same notes; at 150 ms an E1 under a 3 Hz square off half its cycle
  splits into 12 notes where the onset rule gives 7 [synthetic].
- Why the stem start counts as unknown: a stem can start in the trough of
  a tremolo, and its first climb then follows a dip that the search
  cannot see the start of [synthetic]. Read as silence, that dip held.
  On the third set one stem split that way: an A2 under a full-depth
  14 Hz square loud for half its cycle, with 12 ms edges, that turns
  off 2 ms after the stem starts. It gave 4 notes where the onset rule
  gives 2, with the detected profile and with the bass profile
  [synthetic].
- Why the onset rule alone fails: the decoder can fill the gap before a
  low repeat and read its onset weak [synthetic]. A 55 Hz saw gated on
  for 0.2 s in every 0.25 s decodes as 16 notes that touch, 12 of them on
  onsets of 0.54 to 0.59. The onset rule alone joins them into 6, and the
  attack rule keeps all 16 [synthetic].
- What the rule was measured on [synthetic]: four generated sets, each
  stem decoded with the detected profile and with the bass profile, and
  the 37 eval parts.
  - The first set holds 358 stems. Held saws from A0 to A4 sit under
    full-depth sine tremolos of 2 to 20 Hz, at three phases up to A2,
    triangle tremolos, tremolos of depth 0.8 and 0.9, a vibrato of 50
    cents, and sidechain ducks of -16 to -40 dB that recover in 25 to
    100 ms. Gated saws at B0, E1, A1, E2 and A2 have gaps of 30 to
    100 ms, tails of up to 40 ms, noise floors at -40 and -60 dB and
    attacks of 5 to 25 ms. Two voices a fifth and an octave apart and a
    DC offset complete it.
  - The second set holds 523 stems with random settings. Held saws from
    A0 to A2 sit under sine, triangle and square tremolos of 2 to 20 Hz,
    ducks of -20 to -40 dB and tremolos with vibrato. 150 more stems
    sweep full-depth sine tremolos of 12 to 20 Hz on A0, B0, E1, A1 and
    A2 at six random phases each. Gated saws and two voices a fifth or
    an octave apart complete it.
  - The third set holds 528 held saws at E1, A1, E2 and A2 under square
    tremolos of 3 to 20 Hz. Each is loud for 50 to 80 % of its cycle,
    with edges of 1 to 12 ms and depths of 0.8 to 1. 48 of them add a
    vibrato of 30 to 80 cents.
  - The fourth set holds 40 gated saws at B0, E1, A1, E2 and A2, one
    every 0.25 s, with gaps of 80 to 150 ms.
- On those sets no gated run keeps fewer repeats than the onset rule
  alone, and 45 of the 716 first-set runs, 69 of the 154 gated
  second-set runs and 14 of the 80 fourth-set runs keep more
  [synthetic]. The rule keeps every repeat the decoder gives for the
  gated A1 with 50 ms gaps, with 30 ms gaps, with a DC offset and with
  A2 above it [synthetic]. It keeps part of the rest: 11 of 14 A1
  repeats under a fifth, 13 of 16 eighths with a 20 ms tail, and fewer
  as the attack slows past 10 ms [synthetic]. Every sine tremolo, duck,
  triangle tremolo and tremolo with vibrato gives the same notes as the
  onset rule alone, at every phase swept, and so does every square
  tremolo loud for half its cycle [synthetic]. Squares loud for 60 to
  80 % of the cycle can split a held note further, as the limits below
  give [synthetic]. The eval scores match main in every cell
  [synthetic]. A slack of 6 to 8 ms gives the same held notes on the
  first set. At 9 ms a duck to -40 dB on E1 splits, and on the second
  set 8 ms splits one on A1; each ms less keeps fewer repeats
  [synthetic]. Off these sets the margins are thinner: a
  55 Hz repeat under a reverb tail 20 dB down can read a harmonics' dip
  near 20 dB and join [synthetic].
- A weaker piece before a stronger note is an early fragment of the
  attack and stays, since joining it would move the onset [synthetic].
- Limits that remain [proposal]:
  - A repeat played legato, with no drop in level, still depends on the
    onset margin, which is thin on both sides [synthetic]. The weakest
    repeat onsets on the generated set are sixteenths at 175 BPM, at
    0.70 [synthetic].
  - A repeat whose gap a tail or reverb fills to within 19 dB, or to
    within 22 dB at its harmonics, joins the note before it [synthetic].
    So does one whose attack is slower than about 7 ms plus 0.8 of the
    envelope window: at A2 a linear attack of 13 ms stays apart and one
    of 14 ms joins [synthetic].
  - The rise test passes a full-depth tremolo to silence faster than
    about 0.3 / (0.8 / `f0` + 7 ms) Hz: 8 Hz at A0, 11 Hz at E1, 14 Hz at
    A1, 21 Hz at A2 and 29 Hz from B3 up, where the window stays 4 ms
    [fact]. The hold fails such a tremolo above about 7.8 Hz, and the
    window lengthens the hold a little, so at A0 a tremolo near 8 Hz can
    pass both tests [fact]. None of the generated ones did [synthetic].
  - A square LFO off for 20 to 40 % of each cycle is, by level, a gated
    repeat with short gaps: it rises like a gate and holds for 1.5 times
    its dip or more [fact]. To the cleanup it is a gate. On the
    third set squares loud for 60, 70 and 80 % of the cycle split a held
    note further in 38, 57 and 22 of their 240 runs: an A1 under a 6 Hz
    square loud for 70 % into 22 notes where the onset rule alone gives
    6, and with the attack split into 24, one a pulse [synthetic]. A
    vibrato under such a square splits the same way, in 18 of the 96
    runs, all loud for 60 or 70 % [synthetic]. The attack
    split cuts such a note at its pulses as well: squares loud for 60, 70
    and 80 % split further than without the split in 15, 20 and 7 of
    their 240 runs, and a vibrato under one in 10 of 96. An A1 under an
    8 Hz square loud for 70 % gives 32 notes, one a pulse, where it gave
    8 [synthetic]. The model's onset activation at those pulses, 0.25 to
    0.67, matches that of gated repeats [synthetic]. Only a cue such as a
    phase reset at each repeat could tell the two apart [proposal].
  - Below A1 the attack split stays off, so a repeat the decoder reads as
    part of one note stays joined there [synthetic]. From E1 the split,
    measured before its fundamental test, would keep more repeats in 33
    of the 242 gated runs of the second and fourth sets and the A1
    sixteenths, and in 8 of the 24 two-voice runs, 238 more gates hit; it
    would also split the ducks above [synthetic].
  - Below about A2 the harmonics' window cannot tell a note from a key
    nearby, so a held note under a louder gated voice of another key
    dips at its harmonics too, and the split join keeps the decoder's
    pieces of it apart [fact]. A held A1 20 dB under an A2 gated for
    0.2 s in every 0.25 s gives 12 notes, and under a gated E2 13
    [synthetic]. The attack split adds no cut there, since the held
    note's fundamental never dips [synthetic].
  - From about A6 up a period lasts 13 samples or fewer at 22,050 Hz,
    and at the worst phase a harmonic leaks into the fundamental's bin
    at -23 dB at A6 and -17 dB at A7 [fact]. There the fundamental's dip
    guards less well against a louder gated voice an octave or more up
    [fact]. The generated sets hold no such pair [synthetic].
  - A gated repeat of 60 to 90 ms after a gap of 10 to 20 ms, which the
    decoder starts 5 ms early and with a higher velocity, still loses
    its onset: the cut piece before it lasts 100 ms or less and starts
    it, so the stem gives 2 notes for 3 and the repeat starts at the cut
    [synthetic]. The 60 ms hold and the 25 ms dip frame hide so short a
    gap from the attack rule [fact]. After a 25 ms gap all but the 80 ms
    gate lose it too; gates of 95, 100 and 120 ms keep 3 [synthetic].
    The join exists for the real 58 ms fragment above, which would stay
    a note of its own without it [fact].
  - A held A1 under a 6 dB amplitude LFO of one cycle per eighth note at
    120 BPM gives 2 notes, as the onset rule alone does: the decoder
    reads a strong onset of its own at 0.19 s [synthetic]. The attack
    split adds no cut there, under such an LFO at 1/8 or 1/4, at A1 or
    E1 [synthetic]. Keeping such a note whole is
    [#306](https://github.com/Akurganow/sound-to-patch/issues/306).
  - A repeat after a note that holds above half its level for less than
    60 ms joins the note before it when the decoder reads its onset weak
    [synthetic]. At A2 a note of 59 ms before a 30 ms gap holds long
    enough and one of 58 ms does not [synthetic]. Sixteenths played at
    three quarters of their length last 11250 / BPM ms, under 59 ms
    above about 190 BPM [fact].
  - A repeat whose gap lasts longer than about two thirds of the note
    before it joins as by the onset rule alone, since that note holds
    for less than 1.5 times the dip [fact]. Staccato eighths at half
    length, gated for 125 ms in every 250 ms, keep 3 of 16 repeats at E1
    in the bass profile, as the onset rule does; with 100 ms gaps they
    keep 8 where it keeps 3 [synthetic]. Sixteenths pay it too: A1
    sixteenths with 41 to 44 ms gaps and a 10 ms attack kept 3 to 6 of
    32 repeats where a hold of 60 ms alone kept 7 or 8; the attack split
    now gives them 14 to 27 [synthetic].
  - Most bowed, blown and sung re-attacks rise slower than about 10 ms,
    and such a re-attack joins when the decoder reads its onset weak
    [proposal]. The rule reads only the stem's level, and a slow
    re-attack climbs like a tremolo. A spectral cue such as new partials
    or a phase reset could tell them apart, but the generated sets
    change only the level, so they cannot measure it [synthetic].
  - The decoder itself can miss a repeat: with 30 ms gaps the gated
    55 Hz saw decodes as 9 notes for 16, or 8 with a 5 ms tail
    [synthetic]. From A1 up the attack split restores them; below it
    nothing does [synthetic]. At 110 Hz the bass profile
    decodes the same signal as 29 or 30 notes, an octave copy over most
    of them, and the cleanup gives the 16 [synthetic].
- Quiet-note guard: a note whose span holds no 25 ms frame within 40 dB
  of the stem's loudest frame is dropped [proposal]. The model scales each
  window by its own level, so near-silent bleed fills its range and gives
  notes [fact].
- The ghost: a dry 110 Hz saw (a band-limited saw at amplitude 0.3, 0.5 s
  notes with a 5 ms attack and a 0.3 s linear release) decodes with the
  bass profile as the true note plus a 70 ms copy at MIDI 57 that ends
  about 80 ms after the onset [synthetic]. The copy needs a minimum length
  below 70 ms: the 11 frames of the `pad` profile remove it [synthetic].
  After the cleanup the stem gives the A2 alone, all 6 of its notes: the
  model misses the onset of the fourth, and the attack split restores it
  [synthetic]. Six such
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
| bass, 13 clean parts | 0.755 (239/381/252) | 0.896 (240/284/252) | 0.791 (239/352/252) | 0.907 (240/277/252) |
| bass, 4 parts with bleed | 0.457 (32/108/32) | 0.627 (32/70/32) | 0.615 (32/72/32) | 0.865 (32/42/32) |
| bass, 3 parts with transients | 0.622 (23/50/24) | 0.821 (23/32/24) | 0.742 (23/38/24) | 0.920 (23/26/24) |
| lead, 9 parts | 0.945 (130/143/132) | 0.974 (130/135/132) | 0.945 (130/143/132) | 0.974 (130/135/132) |
| pad, 7 parts | 0.784 (80/111/93) | 0.856 (80/94/93) | 0.784 (80/111/93) | 0.856 (80/94/93) |

Recall over each class, raw and after the cleanup, is the same for the
right profile and for detection [synthetic]:

| Class | Raw | Clean |
| :-- | :-- | :-- |
| bass, 20 parts | 0.955 | 0.958 |
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

### Time cost

`cargo run --release --example notes_memory -- <stage>` runs one stage of
the notes step on one generated 60 s stem at 22050 Hz, saw and sine notes
from A2 to G4 [fact]. CPU time (user) and peak resident size of one
process per stage, the median of five runs of a release build
[synthetic]:

| Stage | CPU time | Peak memory |
| :-- | :-- | :-- |
| The stem alone | 0.08 s | 14 MB |
| Model run | 1.20 s | 175 MB |
| f0 track | 1.08 s | 65 MB |
| Model run, decode and cleanup | 1.22 s | 190 MB |
| End to end | 2.26 s | 230 MB |

- The f0 track costs about the same CPU time as the model run, on one
  thread. Its time and its memory grow linearly with the stem [fact].
- The decode and the cleanup add 15 MB to the model run [synthetic].
- On a real stem of 221 s, `sound-to-patch notes` took 11.8 s and peaked at
  0.73 GB. The dense Viterbi search that the track used before took 290 s
  and 4.2 GB on the same stem, with the same notes [fact].
- A debug build runs the f0 track at about half a second of CPU time per
  second of audio, 25 times slower than a release build [synthetic].
- `cargo bench --bench notes` times the same steps by wall clock. Its
  medians depend on a quiet machine and were not taken for the current
  tracker [fact].

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
  the `transcription` group, the `pitch` group and the `conventions`
  group; the
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

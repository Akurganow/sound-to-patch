# Pitch layer

`analyze` reads the pitch layer of a monophonic line after the notes step:
the glides between notes, the pitch drop at the strike of a low note, and
vibrato. The layer writes the `pitch` group of `layers.json`; the
[layer report page](../reference/formats/layers.md#the-pitch-group) lists
its fields. The tags are those of the [pipeline page](pipeline.md).

The glides and the vibrato read the f0 track. The drops read the sub-band
line, because the track averages over a 93 ms frame and smears a drop
that settles in 20 ms [fact]. With `--notes` the model does not run, but
the f0 track still reads the stem, unless the notes play chords or are
empty and the layer is skipped [fact].

Every figure below comes from generated signals at 22,050 Hz, through the
transcriber's notes and the f0 track of the release build [synthetic]. A
real stem adds filters, reverb and separation artefacts.

## What the layer reads

- The voicing of the line. A line that plays chords gets
  `values: null` with the reason `polyphonic_line` and the flag
  `chord_skipped`: no method here separates the voices [fact]. The
  voicing comes from the notes step, or with `--notes` from the same chord
  rule applied to the given notes [fact]. That rule calls a part chords
  from a chord share of 0.68 [fact]. Real stems split into shares of 0.41
  or less and 0.52 to 0.67, so a part at 0.52 to 0.67 runs this layer as
  a line; on those stems the layer read 1 false drop, 1 false glide and
  4 glides an independent f0 confirmed [fact].
- A stem without notes gets `values: null` with the reason
  `no_note_list` and the flag `no_notes` [fact].
- Every time in the group is audio time from the start of the stem
  [fact].

## The track's lag

The tracker compares the first half of each centred frame with the
samples one period later. A frame therefore reads the audio from a
quarter frame before its centre to one period after it, so the track lags
the audio by a quarter frame, 23.2 ms, less half a period [fact]. That
is 18.7 ms at 110 Hz and 14.1 ms at 55 Hz [fact]. The layer subtracts the
lag at the pitch it reads; a mono glide's start then lands within 4 ms of
the truth at 110 Hz [synthetic].

## The sub-band line

- A fourth-order Butterworth high-pass at 20 Hz and low-pass at 200 Hz
  keep the fundamental of a low note, each run forward and backward with
  the odd reflection of the [analytic signal](pipeline.md#analytic-signal)
  stage [fact].
- The analytic signal of the band, zero-padded by its own length, gives
  the frequency between consecutive samples,
  $f[n] = \arg(z[n+1] \overline{z[n]}) f_s / 2\pi$ [fact].
- A centred median of $\mathrm{round}(1.4\ \mathrm{ms} \cdot f_s)$ samples,
  made odd, removes the phase spikes: 31 samples at 22,050 Hz, 63 at
  44,100 Hz and 67 at 48,000 Hz [fact]. Near an end the window shrinks to
  the samples that exist, so the end is not pulled towards a padding
  value [fact].
- The line matches SciPy's `sosfiltfilt`, `hilbert` and a shrinking median
  on the same 808 tone within 3.2e-4 Hz, a relative 1e-5 [synthetic].
- The analytic signal of a segment ripples at twice the tone near each cut
  end, by an amount that falls as one over the distance from the end
  [fact]. A steady 41.2 Hz sine reads its frequency within 0.01 Hz on
  average and within 0.5 Hz everywhere from 0.23 s inside each end
  [synthetic]. The layer keeps 0.1 s before the onset search and 0.5 s
  after the fit span [fact].
- A tone switched on from silence overshoots the line too: by about 11 Hz
  at 3 ms after the onset, 1 to 4 Hz at 7 ms, and within 1 Hz by 30 ms,
  at 30 to 150 Hz [synthetic]. A plain 41.2 Hz tone reads 9.4 Hz high at
  3 ms and 0.9 Hz high at 7 ms [synthetic].
- The band keeps the fundamental only. When the second harmonic is
  stronger the line follows it; the drop then fails the weak-fundamental
  check below [fact].

## Pitch drops

A drop is the fast fall of an 808 or a pluck from a higher start into its
settled pitch.

- Candidates. Each note whose pitch lies in 20 to 200 Hz is a candidate
  strike [fact, proposal for the rule].
- Onset. The transcriber places the onset of a falling note late, by up to
  230 ms on the rows below [synthetic]. The layer looks for the onset from
  0.5 s before the note, or from where the previous note's search ended,
  to 0.1 s after it [proposal]. The band envelope is averaged over a
  centred 10 ms; the onset is the first sample after its lowest point in
  that span where it reaches half the peak that follows [proposal]. When
  the lowest point already stands above half that peak, no attack lies
  inside the span and the note is no candidate [fact]. The average keeps a
  held saw from reading as a strike: its partials beat inside the band, so
  the raw envelope dips every period [synthetic]. A centred average leaves
  the midpoint of a step in place [fact]. A note that the transcriber
  splits off a held note, as under vibrato, has no attack and no candidate
  [fact].
- Strike. The rise is the full-band RMS over the 20 ms after the onset
  against the RMS from 45 to 25 ms before it [fact]. A drop needs a rise
  of 6 dB when its $\tau$ is under 10 ms, when its fall starts at 200 Hz
  or above, or when the band envelope takes over 30 ms from 10 to 90 % of
  its peak; otherwise it needs no rise [proposal]. A drop with $\tau$
  under 10 ms also needs an attack of 30 ms or less: the fall ends inside
  a slower attack, where the line wanders on the rising level [proposal].
  On a real stem a fall with $\tau$ = 5.8 ms in the noise floor read an
  attack of 208 ms; of the generated drops, only a false one at a soft
  retrigger had both [fact, synthetic].
  - A saw falling from 220 Hz into the band raises the band envelope with
    no strike; without the rule its 4-semitone glide also read as a drop
    of 4.6 semitones [synthetic]. A 7-voice unison falling the same way
    reads an attack of 191 ms [synthetic].
  - Where a ringing tone restarts its phase, the line swings and fits a
    fast fall: a plain retriggered 808 read 32 semitones with $\tau$ of
    4 ms [synthetic].
  - An 808 note struck while the one before still rings rises only by
    that note's decay. With 0.4 s notes the rise is 9 to 10 dB at a decay
    of 0.3 s, 4 to 5 dB at 0.6 s, 2 to 3 dB at 1 s and under 1.5 dB at 2 s
    [synthetic]. Its drop starts near 100 Hz with an attack of 9 to 13 ms,
    so it needs no rise [synthetic].
  - The span before ends 25 ms early, since the band onset of white noise
    lags the start of the noise by 14 ms [synthetic].
- Level. A strike whose RMS over the 20 ms after the onset lies more than
  40 dB below the loudest 25 ms frame of the stem is no candidate, the
  gate of active frames [proposal]. On a real stem a fast fall settling
  on a note two octaves under the line read as a drop at -60 dB, where an
  independent f0 found no voice [fact]. A generated drop 60 dB below a
  loud passage reads none, 30 dB below it reads [synthetic]. A strike
  whose 20 to 200 Hz RMS over those 20 ms lies more than 18 dB below the
  loudest 25 ms frame of that band is no candidate either [proposal].
  Checked against an independent IF on real stems, the drops of a line's
  own low notes read 3 to 12 dB below the loudest passage of the band;
  two quiet falls under a louder line in the band read 22 and 28.5 dB below
  it, and the fall in the noise floor 33 dB [fact]. A generated drop
  24 dB under an 87 Hz line reads none, 10 dB under it reads
  [synthetic].
- Fit. $f(t) = f_\text{end} + \Delta f\, e^{-t/\tau}$ is fitted by least
  squares to the line from 7 ms to 400 ms after the onset, or to the end of
  the note [fact]. For each $\tau$ the fit is linear in $f_\text{end}$ and
  $\Delta f$; a log grid of 240 points and a golden-section search find
  $\tau$ from 3.04 ms, where a tenth of the fall is left at 7 ms,
  $e^{-7\ \mathrm{ms}/\tau} = 0.1$, to 1 s [fact, proposal for the floor].
  The fit matches SciPy's `least_squares` with the same bounds within a
  relative 1.5e-6 [synthetic].
- Fit start. From 3 ms the onset overshoot of the line reads as a fast
  fall: a plain 41.2 Hz tone with a 4 Hz drop fitted a drop of 9.9
  semitones with $\tau$ = 3 ms [synthetic]. The fit starts at 7 ms
  [proposal].
- Refit. A fit with $\tau$ under 10 ms, or after a strike that rises
  20 dB, is fitted again on a line whose high-pass sits at 10 Hz, and the
  refit decides [proposal]. Where a tone below 40 Hz starts from silence
  or rises, the 20 Hz high-pass raises the line for about 10 ms, which fits
  as a fast fall; how far depends on the tone's start phase [synthetic].
  On first notes at 32.7 and 36.7 Hz, 6 of 162 random start phases read
  falls of 10 to 14 semitones with $\tau$ of 3 to 5 ms; on the 10 Hz line
  none does [synthetic]. The 10 Hz line everywhere loses 3 of 18 drops on
  ringing 808 lines, so slow fits keep the 20 Hz line [synthetic].
- Residual. For a fit with $\tau$ of 10 ms or more, the error is averaged
  over half a period of $f_\text{end}$ before its RMS [proposal]. The
  third harmonic of a driven note beats with the fundamental and swings
  the line at twice the note; the average removes that swing and keeps a
  slower error, from a wrong model [fact]. A driven 808 drop left a raw
  residual of 23 % and fitted $\Delta f$ and $\tau$ within 1 %
  [synthetic]. Over a full period the average also hid the swing where a
  retrigger restarts the phase: two same-pitch retriggers read falls of 4
  and 11 semitones with $\tau$ of 12 ms [synthetic]. A fall faster than
  10 ms ends inside one period, and the average would spread its misfit,
  so it keeps the raw RMS [fact]. A fall whose $\tau$ spans 1.5 periods
  of $f_\text{end}$ or more changes little within one, so its error is
  averaged over a full period, which removes the swing of a second
  harmonic too [proposal]. A sweep into 32.7 Hz with $\tau$ = 90 ms and a
  second harmonic at half the fundamental failed on the residual with half
  a period and reads with a full one [synthetic].
- Rule. A drop counts when $\Delta f$ in semitones,
  $12 \log_2((f_\text{end} + \Delta f) / f_\text{end})$, is above 2, when
  the fitted fall from 7 ms on, $\Delta f\, e^{-7\ \mathrm{ms}/\tau}$, is
  above 2 semitones too, and when the RMS residual is below 15 % of that
  fall [fact]. Both tests read the fall the fit span shows, not the one
  extrapolated to the onset [fact]. Against the extrapolated $\Delta f$
  the residual misleads: on a 55 Hz tone with five harmonics, three of them
  inside the band, the line swings at the onset and the fit found
  $\tau$ = 0.7 ms with an extrapolated $\Delta f$ of 776 kHz, a residual of
  0.003 % of it but 48 % of the fall from 7 ms on [synthetic].
- Bounds. $\tau$ must lie above the floor of the search, and $\Delta f$
  at most 36 semitones, three octaves [proposal]. A fit on the floor means
  the best fit lies below it [fact]. Retriggered 808 hits restart the
  sine's phase and the line swings at each join: with the floor at 0.5 ms
  and no cap, five abutting hits at 43.65 Hz read falls of 141 and 152
  semitones with $\tau$ of 0.7 ms [synthetic]. The line reads up to
  200 Hz, so a fall from more than three octaves above a low note starts
  far outside the band and is extrapolated from its tail [fact].
- Weak fundamental. After the fit, the medians of the half-octave bands
  around the note's pitch and twice it are read from 50 ms after the onset
  or the note's start, whichever is later, to 400 ms after the onset or the
  note's end, whichever is earlier [fact, proposal for the window]. The
  window starts no earlier than the time the fitted pitch falls inside
  the band around $f_\text{end}$: a slow sweep from 80 Hz into 32.7 Hz
  crosses the band around twice the note, and without that start a pure
  sine read as a weak fundamental [synthetic]. When
  the upper band exceeds the lower by a factor of 2, 6 dB, the line would
  follow the second harmonic: the layer fits nothing for that note and sets
  the flag `weak_fundamental` [proposal]. White noise reads the upper band
  1.41 times the lower, since it is twice as wide [fact]. Below the factor
  a fit that reads the harmonic settles 1200 cents off the note and fails
  the settling rule [fact].
- Settling. A drop also settles on a note: one note sounds while the drop
  is settled, from five time constants after the onset to the end of the
  fit span, with its pitch within 100 cents of $f_\text{end}$ [proposal].
  A slow fall not settled by the end of the span needs the note at its
  end: with $\tau$ = 90 ms on a 350 ms note, five time constants end after
  the note, and the sweep read as a glide from silence [synthetic]. At a saw's onset
  several partials share the band and the line swings; on a generated 55
  Hz saw that swing fitted a fall of 62 semitones with $\tau$ = 2 ms
  settling at 49.7 Hz, which this rule rejects [synthetic].
- Skips. The summary line `drop skips` counts the low notes the search
  read no drop at, by the first gate that stopped each: no onset, quiet,
  no strike, weak fundamental, no fall, tau on bound, residual and off
  note [fact].
- Confidence. Low when the attack exceeds 30 ms: a slow swell is no
  strike [proposal]. High needs a residual below 5 %,
  $\tau$ of 10 ms or more and an attack of 12 ms or less, else medium
  [proposal]. The reasons are `fit_residual` and `attack_time` [fact].
  - The fit sees $e^{-7\ \mathrm{ms}/\tau}$ of the fall, a quarter at
    $\tau$ = 5 ms: kick drops with $\tau$ of 3 to 8 ms read $\Delta f$
    from -31 to +25 % [synthetic].
  - During a slower attack the pitch falls before the onset, and
    $\Delta f$ reads low: by up to 30 % with a 10 ms ramp, 46 % with
    20 ms and 73 % with 40 ms [synthetic]. The 10 ms average spreads a
    sharp strike to an attack of 9 to 11 ms, a 20 ms ramp to about 17.5 ms
    [synthetic].
  - On the generated sets, high-confidence drops read $\Delta f$ within
    17 %, with one at -30 % (a 10 ms ramp on a 15 ms fall), $\tau$ within
    8.3 % and $f_\text{end}$ within 0.1 Hz [synthetic].

Drops on an 808 tone, a sine times a 0.5 s amplitude decay, through the
transcriber's notes [synthetic]:

| True $f_\text{end}$ / $\Delta f$ / $\tau$ | Read $f_\text{end}$ | Read $\Delta f$ | Read $\tau$ |
| :-- | :-- | :-- | :-- |
| 41.2 Hz / 41.2 Hz / 20 ms | 41.17 Hz | 37.57 Hz (-8.8 %) | 21.6 ms (+8.0 %) |
| 41.2 Hz / 41.2 Hz / 50 ms | 41.15 Hz | 40.34 Hz (-2.1 %) | 51.3 ms (+2.6 %) |
| 55 Hz / 165 Hz / 30 ms | 54.99 Hz | 156.34 Hz (-5.2 %) | 30.0 ms (0.0 %) |
| 41.2 Hz / 13.8 Hz / 100 ms | 40.67 Hz | 13.20 Hz (-4.3 %) | 118.8 ms (+18.8 %) |

The acceptance bounds are 1.5 Hz on $f_\text{end}$, 25 % on $\Delta f$,
and 25 % on $\tau$ up to 50 ms, 40 % at 100 ms [fact]. White noise 30 dB
below the tone's peak keeps $f_\text{end}$ within 1 Hz and the drop
accepted [synthetic].

Limits:

- Drive and harmonics. `tanh(4 x)` puts the third harmonic, 124 Hz,
  inside the band, and a saw below 100 Hz its second and third. With the
  average, the drop of the 41.2 Hz row through `tanh(4 x)` reads
  $\Delta f$ 14 % low and $\tau$ 13 % long [synthetic]. A 12-semitone
  drop with $\tau$ = 30 ms on a driven sine at 41.2, 55 and 82.4 Hz reads
  within 0.7 semitones and 7 % on $\tau$, at high confidence
  [synthetic]. On a saw the second harmonic swings the line at the note
  itself, which half a period does not average out, and no drop with
  $\tau$ under 1.5 periods is read [synthetic]. A second harmonic stronger
  than the fundamental takes the line: of 36 generated sweeps from silence
  with one 3.5 dB above it, none read [synthetic].
- A fall under a louder line in the band, or under a louder passage of
  the same line, is not read 18 dB or more below it, so a soft note after
  loud ones loses its drop [fact].
- A note above 200 Hz is not read: the band stops there [fact].
- A kick with no pitch drop and a noise click, over a sub, reads a fast
  drop of 9 to 27 semitones at medium when it starts from silence, like a
  kick whose pitch drops [synthetic]. Inside the first period of the note
  the click and a fall of $\tau$ under 10 ms look alike; rejecting fast
  falls from silence would lose 26 true kick drops of the generated sets
  [synthetic].
- A soft retrigger or an envelope dip on a low note mid-line can still
  read a false fast drop at medium: of 82 generated lines of that kind,
  one does with the transcriber's notes, another with the given notes,
  15 to 16 semitones with $\tau$ under 4 ms [synthetic].
- A legato note, which follows the previous run with no fall of the band
  envelope, has no onset and no drop [fact].
- A struck note whose band envelope does not dip first has no onset and
  no drop: with no release before it and a decay of 1 s or more, the dip
  of an 808's 3 ms attack is lost in the 10 ms average [synthetic]. Of 18
  drops on generated 808 lines with given notes, 14 read; the misses are
  those notes and a soft note after a loud one [synthetic]. Through the
  transcriber's notes fewer read, since its onsets move the search
  [synthetic].
- A fast drop, $\tau$ under 10 ms, after a ringing note needs a 6 dB
  strike, and a drop with a slow attack needs one too [fact].
- Retriggered 808 hits at one pitch, with and without gaps, ringing or
  not, and 808 lines stepping up to 7 semitones report no drop with
  $\tau$ of 10 ms or more [synthetic]. A false fast drop, $\tau$ under
  10 ms, needs a 6 dB strike and survives the refit on the 10 Hz line
  only where the limits above say [synthetic].

## Glides

A glide is pitch moving from one stable pitch to another.

- The track's voiced frames, in cents, split into runs of consecutive
  voiced frames [fact].
- Gaps. A run that starts within 100 ms of the one before joins it when
  the full-band RMS does not rise 6 dB in the gap, and the pitch keeps
  moving after the gap the way it jumped across it: by 20 cents or more
  over the jump and over the next two frames [proposal]. A unison whose
  voices start in phase beats hard enough to leave the track unvoiced for
  75 ms inside a glide; read apart, the rest read as a glide from silence
  of 3.9 semitones for 7 [synthetic]. At a step between two notes the
  track goes unvoiced too, but lands on the new pitch and holds it, or
  swings back; at an 808 step the new strike raises the level [synthetic].
  The jump across the gap must be 30 cents per millisecond or slower
  [proposal]: a 50 ms glide of 7 semitones jumps 20 across two unvoiced
  frames, the unison above 3.6 [synthetic]. On a real stem a note struck
  again 10 semitones lower, across three unvoiced frames, read as a glide;
  that is about 40 cents per millisecond [fact]. Nor is a gap bridged
  where the full-band RMS, over 10 ms, falls 20 dB below the 20 ms before
  or after it, or 40 dB below the loudest frame of the stem [proposal]: on
  real stems steps read as glides across a dip of 41 dB and at a noise
  floor of -60 dB [fact].
- Steps across a gap. Two stable pitches with a bridged gap between them
  join no glide when the pitch jumps across the gap against the interval
  by 20 cents or more, or by more than 60 % of it [proposal]. A step and
  the settle after it keep moving the way the step jumped, at 5 to 17
  cents per millisecond, so neither the slope limit nor the motion test
  tells them from a glide [fact]. On real stems steps of 3 to 10.5
  semitones read so, and one read +2 semitones where the f0 fell 6.7
  across the gap [fact]. Unison glides whose voices leave the track
  unvoiced for 75 ms jump 0.53 and 0.54 of the interval across the gap; a
  step between 808 notes jumped 0.72 [synthetic].
- A stable pitch is a span of 50 ms or more whose smoothed pitch stays
  within 30 cents of its running mean and whose least-squares slope stays
  at 86 cents per second or less [proposal]. 86 cents per second is the
  floor of a published portamento detector, untuned [fact].
- A glide joins two consecutive stable pitches 100 cents or more apart
  that each match a note within 50 cents, with the two notes of different
  pitch [proposal]. The note test keeps vibrato out, which the transcriber
  gives as notes of one pitch at ±35 cents [synthetic]. No glide is read
  on any vibrato signal below, ±35 or ±60 cents [synthetic].
- The motion is the span between the middles of the two stable pitches
  where the slope of the 5-frame moving average, 29 ms, stays at 86 cents
  per second or more in the direction of the motion [fact]. Under 35 ms
  it is a step between notes [fact, proposal for 35 ms].
- Otherwise the fitted time decides with the same bounds: under 35 ms a
  step, from 35 to 90 ms a glide too short to read, counted and flagged
  `glide_too_short`, from 90 ms on a glide [proposal]. The track smears a
  step between two 808 notes over a detected span of 100 to 450 ms, while
  the fit finds the motion short [synthetic]. A motion whose fit fails is
  too short when its detected span lies from 35 to 90 ms [fact].
- Fit. $s(x) = 1 - (1 - x)^p$ between the two stable pitches is fitted by
  least squares over the motion and 50 ms of each stable pitch, with the
  start, the full time and $p$ free [fact]. $p > 1$ starts fast, $p < 1$
  starts slow, $p = 1$ is linear in pitch [fact]. A grid of 9 powers, 8
  times and 13 starts seeds a Nelder-Mead simplex; $p$ stays within 0.2 to
  5 [fact].
- Shape. A fit of 90 ms or more with $p$ within 5 % of 0.2, or with an RMS
  residual above 40 cents, does not follow the curve and is no glide
  [proposal]. The glides of the generated signals fit within 22 cents; the
  false glides at 808 steps sat on a bound of $p$ or fitted under 90 ms
  [synthetic].
- Curve bound. An exponential (RC) portamento, $1 - e^{-t/\tau}$,
  approaches its target slower than any power of the curve, so its $p$
  sits on the upper bound of 5 [fact]. A fit within 5 % of that bound
  counts when its residual is 20 cents or less, its detected span lasts
  90 ms or more, and it does not come from silence; it reads low with the
  reason `curve_bound` [proposal]. Without those conditions a melody step
  on a sine and two driven drops read as glides on the bound [synthetic].
- Level jump. A glide on the bound under 2 semitones also needs a steady
  level: the full-band RMS of two adjacent windows, each one period of the
  lower pitch or 10 ms if longer, must differ by under 3 dB from 30 ms
  before the fitted start to the end of the motion or 150 ms into it
  [proposal]. A semitone step between 808 notes struck again on one
  oscillator that keeps its phase fits the bound within 5 cents; at such
  steps the level jumped 3.4 to 5.0 dB, on RC slides of 1 and 2 semitones
  over sines and saws by 2.2 dB at most [synthetic]. A 2-semitone slide
  often reads 1.9 on the tracker's 10-cent grid, so a hard cut at 2
  semitones lost those too, as it lost 20 small slides of 1 to 1.8
  semitones on real stems that an independent f0 confirmed [synthetic,
  fact]. An RC slide of 1.2 semitones at 110 Hz reads on the bound
  [synthetic]; one of a semitone reads 90 cents on the grid, under the
  100-cent interval, and is not read [synthetic].
- `time_s` is the full 0 to 100 % time [fact]. `start_s` is the fitted
  start less the track's lag at the start pitch [fact].
- From silence. When a voiced run starts away from its first stable pitch
  by 100 cents or more, and the motion into it lasts 90 ms or more, the
  glide carries `from_silence: true` [fact]. The sound must start up to
  100 ms before the run: the full-band RMS over 30 ms rises 20 dB above
  the RMS from 60 to 30 ms before, searched in 5 ms steps [proposal]. The
  track voices a run some frames after the sound starts, and a beating
  unison dips without falling silent [synthetic]. The run's first frame
  fixes the start and the fit also finds the start pitch [fact]. A run
  that starts within 0.2 s of an accepted drop is the drop, not a glide
  [proposal].
- Confidence. The reasons are the motion span, the RMS residual of the fit
  in cents and the jitter: the RMS deviation of the two stable pitches
  from their medians, and `curve_bound` [fact]. High needs a residual of
  15 cents or less, a jitter of 3 cents or less, a motion of 150 ms or
  more and $p$ of 3 or less; low comes with a jitter above 3.4 cents or a
  curve on the bound [proposal].
  - A faster start reads its rise less well: on the generated sets, up to
    11 % off at $p$ 4.4, while high-confidence glides read their rise
    within 7.1 % [synthetic].
  - A unison beats and the track wanders on the stable pitches, but the
    jitter tells a unison only roughly. On the generated sets the unison
    glides jitter 3.77 to 7.07 cents at 22.05 kHz and 3.45 to 6.93 at
    44.1 kHz, the mono ones 0.66 to 8.03 at both rates [synthetic]. All
    unison glides read low; from 3 to 3.4 cents only mono glides fall, and
    they read medium, and a mono glide above 3.4 cents reads low too
    [synthetic]. One threshold would raise those mono glides to high on no
    measured gain, so the two stay [proposal].

Glides of 7 semitones up from 110 Hz on a band-limited saw, 0.5 s holds
[synthetic]. The rise is the 10 to 90 % time derived from the fitted time
and $p$, $(x_{90} - x_{10})\,T$ with $x_q = 1 - (1 - q)^{1/p}$; its error is
against the true rise:

| Glide | Voices | Read time | Read $p$ | Rise error | Confidence |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 150 ms, $p$ 1 | 1 | 151 ms | 1.02 | +0.3 % | high |
| 150 ms, $p$ 2.5 | 1 | 156 ms | 2.53 | +3.6 % | high |
| 400 ms, $p$ 1 | 1 | 396 ms | 1.01 | -1.3 % | high |
| 400 ms, $p$ 2.5 | 1 | 387 ms | 2.41 | -1.2 % | high |
| 150 ms, $p$ 1 | 7, ±25 ct | 158 ms | 1.05 | +4.2 % | low |
| 150 ms, $p$ 2.5 | 7, ±25 ct | 133 ms | 1.83 | +4.4 % | low |
| 400 ms, $p$ 1 | 7, ±25 ct | 405 ms | 1.02 | +1.0 % | low |
| 400 ms, $p$ 2.5 | 7, ±25 ct | 411 ms | 2.45 | +3.8 % | low |
| 200 ms, $p$ 1 | 7, ±25 ct | 202 ms | 1.00 | +1.0 % | low |
| 200 ms, $p$ 1 | 7 in phase | 206 ms | 0.93 | +4.4 % | low |
| 300 ms, $p$ 1 | 7 in phase | 304 ms | 1.00 | +1.3 % | low |
| 300 ms, $p$ 2.5 | 7 in phase | 375 ms | 3.32 | +4.5 % | low |
| 200 ms, $p$ 2.5 | 7 in phase | none | | | |
| 50 ms, $p$ 1 or 2.5 | 1 | none, too short | | | |
| step | 1 | none, no flag | | | |

The acceptance bound is 5 % on the rise from 150 ms on, mono [fact]. The
mono interval reads 6.9 semitones for 7: the track's 10-cent bins round
the start 5 cents up and the end 5 cents down [synthetic]. The fast-start
unison at 150 ms reads its rise within 4.4 % but its $p$ at 1.83, and its
time 11 % short; the low confidence says so [synthetic]. "In phase" voices
start at phase 0, so the unison beats hardest [fact].

Other glides, mono [synthetic]:

| Glide | Read |
| :-- | :-- |
| 220 Hz down 4 semitones, 200 ms, $p$ 2.5 | -4.00 st, 184 ms, $p$ 2.26, rise -2.3 %, high; no drop |
| 82.4 Hz up 5 semitones, 50 ms | none, too short (fitted 61 ms) |
| 220 Hz down 4 semitones, 50 ms | none, too short (fitted 43 ms), no drop |
| From silence into 82.4 Hz, up 5 semitones, 150 ms, $p$ 1 | +4.70 st, 150 ms, $p$ 0.85, rise +1.9 %, low |
| From silence into 82.4 Hz, up 5 semitones, 300 ms, $p$ 2.5 | +4.48 st, 252 ms, $p$ 1.74, rise +1.0 %, low |
| 808 lines stepping up to 7 semitones, legato, driven or staccato | none |
| RC portamento from 110 Hz up 7 semitones, $\tau$ 50 ms | +6.9 st, $p$ 5.00, low, `curve_bound` |
| RC portamento from 196 Hz down 5 semitones, $\tau$ 80 ms | -5.0 st, $p$ 5.00, low, `curve_bound` |

The track's first frames of a tone read low, so a glide from silence reads
its interval up to 0.6 semitones short [synthetic]. Of 16 generated RC
glides, $\tau$ 30 to 120 ms at 110 to 247 Hz, all read, at low confidence
[synthetic].

Limits:

- A glide that the transcriber merges into one note, with its two ends in
  one note, is not read [fact].
- A step between two 808 notes can raise `glide_too_short` when the fit
  finds 35 to 90 ms: one of three generated 808 lines does [synthetic].
- A vibrato of ±100 cents at 3 Hz holds each extreme long enough to read
  as a stable pitch, and its swings read as glides of 1.9 semitones, each
  at low confidence [synthetic].
- An 808 slide reads low: its stable pitches jitter like a unison's
  [synthetic].
- A falling slide of 7 semitones from 49 Hz is not read [synthetic].
- A glide inside chords has no method here [fact].
- Short glides of 1 to 3 semitones on real stems mostly read low, yet an
  independent f0 confirmed most of them: their confidence is pessimistic
  [fact].
- A step between notes struck again at a matched level, with no jump of
  3 dB, still reads as a small glide on the bound [synthetic]. A slide
  over a level that swings 3 dB within a period, as a tremolo can, reads
  none [fact].
- A glide that moves more than 60 % of its interval inside an unvoiced
  gap is not read: a 7-voice unison with a fast start jumped 0.76 of it
  across 75 ms [synthetic].

## Vibrato

- Spans. The voiced runs of the track split where two stable pitches lie
  150 cents or more apart, so a step between notes never reads as a
  vibrato, while a vibrato of ±60 cents, whose stable pitches lie 120
  cents apart, stays whole [proposal]. Each span loses 50 ms at each end,
  the frames of every glide and the settling of a drop: five time
  constants, at least 100 ms [fact].
- A span of 0.5 s or more is searched [fact]. A least-squares line and
  sine are fitted together at each rate from 1 Hz, or 1.5 cycles over the
  span, to 20 Hz in 0.01 Hz steps; the rate whose sine explains the most
  power beyond the line wins, refined by golden-section search [fact].
  Fitting the line and the sine together keeps a sine over a fraction of a
  cycle from leaking into the line [fact].
- `extent_raw_ct` is the sine's amplitude, half the peak-to-peak swing
  [fact]. `share` is the power the sine explains over the power left after
  the line alone [fact].
- Rule. A vibrato counts at 3 to 12 Hz with 1 % slack, an extent of 10
  cents or more after the correction, a share of 0.5 or more and 2.5
  cycles or more over the span [fact, proposal for the share and the
  cycles]. Checked against an independent f0 on real stems, every vibrato
  of 2.5 cycles or more matched it; the shorter ones, 1.6 to 2.4 cycles at
  3 to 4.7 Hz over spans of 0.5 to 0.8 s, did not, or matched under 2
  cycles [fact]. Through the track the rates below read within
  0.8 % of the truth, so the slack keeps a vibrato at either edge
  [synthetic].
- Correction. The track's 93 ms frame smooths a fast swing, so the raw
  extent falls with the rate [synthetic]. `extent_factor` is one over the
  share of the true extent the track reads, interpolated linearly between
  the measured rates and held at the ends [fact]. `extent_ct` is
  `extent_raw_ct` times `extent_factor` [fact].
- Confidence. Low when the kurtosis of the contour after the line, its
  fourth moment over the squared second, is below 1.25: a sine reads 1.5,
  a square wave 1.0 [fact, proposal for 1.25]. Low too on a pitch below
  100 Hz [proposal]. Else high with 4 cycles or more in the span, else
  medium [proposal]. The generated vibratos read a kurtosis of 1.36 to
  1.66, a semitone trill at 6 Hz 1.06 [synthetic].
  - On real stems every vibrato above 100 Hz with 4 cycles or more matched
    an independent f0, at a share of 0.51 to 0.64; the share did not tell
    the real ones from the rest, which read 0.52 to 0.70 [fact].
  - Below 100 Hz few matched, and the extent read up to 5 times too wide
    [fact]. A generated low saw with amplitude modulation, two beating
    voices or none reads no vibrato, and one with a ±30-cent vibrato reads
    it within 2 cents down to 41 Hz, so the cause is not known [synthetic].

The share of the true extent the track reads, on one 1 s mono saw at 220
Hz with ±35 cents per rate [synthetic]:

| Rate (Hz) | 3 | 4 | 5 | 5.5 | 6 | 7 | 8 | 10 | 12 |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| Share | 1.003 | 0.973 | 0.944 | 0.939 | 0.909 | 0.812 | 0.817 | 0.800 | 0.732 |

Each share comes from one signal, so the table carries that signal's
quantisation to the track's 10-cent bins: 7 Hz reads lower than 8 Hz
[synthetic]. The share belongs to this track: another pYIN implementation
at another frequency ceiling reads another share [fact].

| Case | Read rate | Read extent |
| :-- | :-- | :-- |
| 5.5 Hz, ±35 ct, mono | 5.51 Hz | 35.0 ct |
| 5.5 Hz, ±35 ct, 7 voices over ±25 ct | 5.49 Hz | 33.5 ct |
| 5.5 Hz, ±60 ct, mono | 5.51 Hz | 60.8 ct |
| 2 Hz, 15 Hz, or 5 ct | none | |

The acceptance bounds are 0.1 Hz on the rate and 15 % on the extent after
the correction [fact]. The correction is measured at ±35 cents only; the
±60 case reads 1.4 % high with it, and ±25 cents at 9 Hz reads 11 to
14 % low [synthetic].

Limits:

- A trill between two notes a semitone apart at a vibrato rate reads as a
  vibrato, since its stable pitches lie 100 cents apart, but at low
  confidence: its contour jumps between two levels [fact].
- From 4.5 Hz the transcriber gives a ±100-cent vibrato as overlapping
  notes, the line reads as chords, and the layer is skipped [synthetic].
- A vibrato whose depth ramps in over the span reads its extent 9 to 51 %
  low, since the sine averages the ramp [synthetic].
- A vibrato of 3.5 Hz over 0.7 s holds 2.2 cycles and is not read
  [synthetic].
- A triangle LFO reads its extent 20 to 26 % low at high confidence: the
  fitted sine holds $8/\pi^2$ of a triangle's amplitude, and the extent is
  the sine's [fact, synthetic for the range].
- A vibrato whose rate sweeps from 4 to 6 Hz reads no vibrato
  [synthetic].
- A vibrato that starts late in a note is diluted by the steady part
  before it [fact].

## Sources

- Glide floor and minimum length: Yang, Rajab and Chew,
  [AVA: an interactive system for visual and quantitative analyses of vibrato and portamento performance styles](https://archives.ismir.net/ismir2016/paper/000314.pdf),
  ISMIR 2016, CC BY 4.0. Inspiration only.
- Analytic signal: Boashash,
  [Estimating and interpreting the instantaneous frequency of a signal](https://doi.org/10.1109/5.135376).
- f0 track: Mauch and Dixon,
  [pYIN: a fundamental frequency estimator using probabilistic threshold distributions](https://doi.org/10.1109/ICASSP.2014.6853678).

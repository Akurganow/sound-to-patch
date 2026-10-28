# Pitch layer

`analyze` reads the pitch layer of a monophonic line after the notes step:
the glides between notes, the pitch drop at the strike of a low note, and
vibrato. The layer writes the `pitch` group of `layers.json`; the
[layer report page](../reference/formats/layers.md#the-pitch-group) lists
its fields. The tags are those of the [pipeline page](pipeline.md).

The glides and the vibrato read the f0 track. The drops read the sub-band
line, because the track averages over a 93 ms frame and smears a drop
that settles in 20 ms [fact]. With `--notes` the model does not run, but
the f0 track still reads the stem [fact].

Every figure below comes from generated signals at 22,050 Hz, through the
transcriber's notes and the f0 track of the release build [synthetic]. A
real stem adds filters, reverb and separation artefacts.

## What the layer reads

- The voicing of the line. A line that plays chords gets
  `values: null` with the reason `polyphonic_line` and the flag
  `chord_skipped`: no method here separates the voices [fact]. The
  voicing comes from the notes step, or with `--notes` from the same chord
  rule applied to the given notes [fact].
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
  at 30 to 150 Hz [synthetic].
- The band keeps the fundamental only. When the second harmonic is
  stronger the line follows it; the drop then fails the weak-fundamental
  check below [fact].

## Pitch drops

A drop is the fast fall of an 808 or a pluck from a higher start into its
settled pitch.

- Candidates. Each note whose pitch lies in 20 to 200 Hz is a candidate
  strike [fact, proposal for the rule].
- Onset. The transcriber places the onset of a falling note late, by up
  to 230 ms on the rows below [synthetic]. The layer looks for the onset
  from 0.5 s before the note, or from where the previous note's search
  ended, to 0.1 s after it [proposal]. The band envelope is averaged over a centred
  10 ms; the onset is the first sample after its lowest point in that span
  where it reaches half the peak that follows [proposal]. When the lowest
  point already stands above half that peak, no attack lies inside the
  span and the note is no candidate [fact]. The average keeps a held saw
  from reading as a strike: its partials beat inside the band, so the raw
  envelope dips every period [synthetic]. A centred average leaves the
  midpoint of a step in place [fact]. A note that the transcriber splits
  off a held note, as under vibrato, has no attack and no candidate
  [fact].
- Fit. $f(t) = f_\text{end} + \Delta f\, e^{-t/\tau}$ is fitted by least
  squares to the line from 7 ms to 400 ms after the onset, or to the end of
  the note [fact]. For each $\tau$ the fit is linear in $f_\text{end}$ and
  $\Delta f$; a log grid of 240 points from 0.5 ms to 1 s and a
  golden-section search find $\tau$ [fact]. The fit matches SciPy's
  `least_squares` within a relative 1.5e-6 [synthetic].
- The task asked for a fit from 3 ms. At 3 ms the onset overshoot of the
  line reads as a fast fall: a plain 41.2 Hz tone with a 4 Hz drop fitted
  a drop of 9.9 semitones with $\tau$ = 3 ms [synthetic]. The fit starts
  at 7 ms [proposal].
- Rule. A drop counts when $\Delta f$ in semitones,
  $12 \log_2((f_\text{end} + \Delta f) / f_\text{end})$, is above 2, when
  the fitted fall from 7 ms on, $\Delta f\, e^{-7\ \mathrm{ms}/\tau}$, is
  above 2 semitones too, and when the RMS residual is below 15 % of that
  fall [fact]. Both tests read the fall the fit span shows, not the one
  extrapolated to the onset [fact]. The task measured the residual against
  $\Delta f$; on a 55 Hz tone with five harmonics, three of them inside the
  band, the line swings at the onset and the fit found $\tau$ = 0.7 ms
  with an extrapolated $\Delta f$ of 776 kHz, a residual of 0.003 % of it
  but 48 % of the fall from 7 ms on [synthetic].
- Weak fundamental. Before the fit, the medians of the half-octave bands
  around the note's pitch and twice it are read from 50 ms after the onset
  to 400 ms [fact]. When the upper band is the louder, the line would
  follow the second harmonic: the layer fits nothing for that note and
  sets the flag `weak_fundamental` [fact, proposal for the window].
- Settling. A drop also settles on a note: one note sounds while the drop
  is settled, from five time constants after the onset to 400 ms, with its
  pitch within 100 cents of $f_\text{end}$ [proposal]. At a saw's onset
  several partials share the band and the line swings; on a generated 55
  Hz saw that swing fitted a fall of 62 semitones with $\tau$ = 2 ms
  settling at 49.7 Hz, which this rule rejects [synthetic].
- Confidence. High below a residual of 5 %, else medium [proposal].

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

- Drive. `tanh(4 x)` keeps $f_\text{end}$ within 1 Hz, but it puts the
  third harmonic, 124 Hz, inside the band. The line then swings between
  the two partials, the residual reaches 29 % of $\Delta f$, and no drop
  is reported [synthetic].
- A note above 200 Hz is not read: the band stops there [fact].
- A low note rich in harmonics, such as a saw below 100 Hz, puts its
  second and third harmonics inside the band; the line then swings
  between the partials and no drop is read [synthetic].
- A legato note, which follows the previous run with no fall of the band
  envelope, has no onset and no drop [fact].

## Glides

A glide is pitch moving from one stable pitch to another.

- The track's voiced frames, in cents, split into runs of consecutive
  voiced frames [fact].
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
  it is a step between notes; from 35 to 90 ms it is a glide too short to
  read, counted and flagged `glide_too_short`; from 90 ms on it is a glide
  [fact, proposal for 35 ms]. A step reads no motion at all [synthetic].
- Fit. $s(x) = 1 - (1 - x)^p$ between the two stable pitches is fitted by
  least squares over the motion and 50 ms of each stable pitch, with the
  start, the full time and $p$ free [fact]. $p > 1$ starts fast, $p < 1$
  starts slow, $p = 1$ is linear in pitch [fact]. A grid of 9 powers, 8
  times and 13 starts seeds a Nelder-Mead simplex; $p$ stays within 0.2 to
  5 [fact].
- `time_s` is the full 0 to 100 % time [fact]. `start_s` is the fitted
  start less the track's lag at the start pitch [fact].
- From silence. When a voiced run starts away from its first stable pitch
  by 100 cents or more, and the motion into it lasts 90 ms or more, the
  glide carries `from_silence: true` [fact]. The run's first frame fixes
  the start and the fit also finds the start pitch [fact]. A run that
  starts within 0.2 s of an accepted drop is the drop, not a glide
  [proposal].
- Confidence. The reasons are the motion span, the RMS residual of the fit
  in cents and the jitter: the RMS deviation of the two stable pitches
  from their medians [fact]. High needs a residual of 15 cents or less, a
  jitter of 3.5 cents or less and a motion of 150 ms or more; low comes
  with a residual above 40 cents or a jitter above 4 cents [proposal]. A
  unison beats and the track wanders on the stable pitches: the mono
  glides below jitter 2.8 cents at most, the unison ones 5.1 cents at
  least [synthetic].

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
| 50 ms, $p$ 1 or 2.5 | 1 | none, too short | | | |

The acceptance bound is 5 % on the rise from 150 ms on, mono [fact]. The
mono interval reads 6.9 semitones for 7: the track's 10-cent bins round
the start 5 cents up and the end 5 cents down [synthetic]. The fast-start unison at 150 ms reads its
rise within 4.4 % but its $p$ at 1.83, and its time 11 % short; the low
confidence says so [synthetic].

Limits:

- A glide that the transcriber merges into one note, with its two ends in
  one note, is not read [fact].
- A glide inside chords has no method here [fact].

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
  cents or more after the correction, and a share of 0.5 or more [fact,
  proposal for the share]. Through the track the rates below read within
  0.8 % of the truth, so the slack keeps a vibrato at either edge
  [synthetic].
- Correction. The track's 93 ms frame smooths a fast swing, so the raw
  extent falls with the rate [synthetic]. `extent_factor` is one over the
  share of the true extent the track reads, interpolated linearly between
  the measured rates and held at the ends [fact]. `extent_ct` is
  `extent_raw_ct` times `extent_factor` [fact].
- Confidence. High with a share of 0.8 or more and 4 cycles or more in the
  span, else medium [proposal].

The share of the true extent the track reads, on one 1 s mono saw at 220
Hz with ±35 cents per rate [synthetic]:

| Rate (Hz) | 3 | 4 | 5 | 5.5 | 6 | 7 | 8 | 10 | 12 |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| Share | 1.003 | 0.973 | 0.944 | 0.939 | 0.909 | 0.812 | 0.817 | 0.800 | 0.732 |

Each share comes from one signal, so the table carries that signal's
quantisation to the track's 10-cent bins: 7 Hz reads lower than 8 Hz
[synthetic]. The task quoted 0.69 at 5.5 Hz from another pYIN
implementation at another ceiling; this track reads 0.939 [synthetic].

| Case | Read rate | Read extent |
| :-- | :-- | :-- |
| 5.5 Hz, ±35 ct, mono | 5.51 Hz | 35.0 ct |
| 5.5 Hz, ±35 ct, 7 voices over ±25 ct | 5.49 Hz | 33.5 ct |
| 5.5 Hz, ±60 ct, mono | 5.51 Hz | 60.8 ct |
| 2 Hz, 15 Hz, or 5 ct | none | |

The acceptance bounds are 0.1 Hz on the rate and 15 % on the extent after
the correction [fact]. The correction is measured at ±35 cents only; the
±60 case reads 1.4 % high with it [synthetic].

Limits:

- A trill between two notes a semitone apart at a vibrato rate reads as a
  vibrato, since its stable pitches lie 100 cents apart [fact].
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

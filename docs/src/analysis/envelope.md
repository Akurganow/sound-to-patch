# Envelope layer

`analyze` reads the envelope layer after the pitch layer: the amplitude
envelope of each note, pooled over the notes, and the sidechain pump of
the stem. The layer writes the `envelope` group of `layers.json`; the
[layer report page](../reference/formats/layers.md#the-envelope-group)
lists its fields. The tags are those of the [pipeline page](pipeline.md).

The stages follow the envelope of a synth: an attack from the note-on to
the peak, a hold at the peak, a decay from the peak to the sustain level,
the sustain level, and a release from the note-off [fact]. The layer
reads the attack, hold, decay and sustain, and the time of the note-off
knee; the release time is a later step [fact].

Every figure below comes from `cargo xtask eval --layer envelope`, 1111
generated parts at 44,100 Hz scored against their recipes, or from the
acceptance tests at 22,050 Hz, through the release build and with the
true note list [synthetic]. A real stem adds filters, reverb, bleed and
separation artefacts.

The eval is local [fact]. It writes one file, the `--json` path, and only
when that flag is given [fact]. Run it with a release build; a debug build
of the library is far slower:

```sh
cargo run --release -p xtask -- eval --layer envelope --json records.jsonl
```

`--json` writes one record per part, so two builds compare record by
record [fact].

## What the layer reads

- The note list after the cleanup, or the notes that `--notes` gives
  [fact]. A stem without notes gets `values: null` with the reason
  `no_note_list` and the flag `no_notes` [fact].
- The mono mix at the native rate [fact].
- The beat grid of the used tempo, from 0 s, for the pump [fact]. The
  offset is never estimated: a stem whose first beat is late shifts every
  beat window [fact].
- A note with no harmonic to read, or that is too short for a track, is
  skipped and counted in the summary [fact]. A harmonic cannot be read
  when it lies within 30 Hz of a harmonic of a note it overlaps, or when
  its band reaches 0 Hz or the Nyquist frequency; the summary counts these
  as no free harmonic [fact]. A note whose amp proxy peaks at -120 dB or lower is
  skipped as silent: over a silent stretch, a listed note would read a 0 dB
  sustain at the tracks' floor [fact, proposal for -120 dB].
- Every time is audio time from the start of the stem [fact].

## Level tracks

Each note is read on a 1 ms hop from 30 ms before its onset to its end
[fact]. Three tracks exist:

- Centred RMS over whole periods of the fundamental: one period for a
  note's attack, the fewest whole periods that span 2 ms or more on a
  high note [fact]. One period gave the rise times the issue measured;
  two periods read a 16 ms rise at 55 Hz as 34 ms [synthetic].
- A sliding DFT of one harmonic over one period of the fundamental, for a
  note that overlaps no other [fact]. It has no filter transient, so the
  peak does not overshoot [fact]. A fourth-order low-pass on the band
  magnitude overshot the peak and read decays long [synthetic].
- The zero-phase band magnitude of one harmonic, for a note that overlaps
  another [fact]. The low-pass cutoff is half the distance to the nearest
  harmonic of another sounding note, or to the note's own next harmonic,
  clamped to 12 to 40 Hz [fact]. The filter runs forward and backward, so
  the track does not lag [fact].

The amp proxy is the note's lowest harmonic among the first three that
lies 30 Hz or more from every harmonic of the notes sounding with it
[fact]. A filter sweep moves the high harmonics and leaves the low ones,
so the lowest free harmonic follows the amp envelope [fact]. On a 110 Hz
saw under a closing low-pass, the sum of harmonics 1 to 3 read the fall
8 % short, and the fundamental alone within 10 % [synthetic]. Octaves and
unisons leave the upper note no free harmonic; the layer skips it and has
no residual method [fact].

Levels are dB from the note's peak on the amp proxy, read no lower than
-60 dB [fact].

## Attack

- `attack_s` is 1.25 times the 10 to 90 % rise of the linear amplitude:
  the full 0 to 100 % time of a linear ramp [fact]. The rise reads the RMS
  track for a note that overlaps no other, and the amp proxy for one that
  does [fact].
- The rise ends at the attack's top: the first frame from the onset that
  the level exceeds by less than 1 dB over as long again as it took to
  rise to it from under 10 % of it, and at least over the shortest rise
  the track can tell, the same figure as the upper bound's below [fact].
  Without that
  floor, a wobble of the window early in the rise read as a top [synthetic]. The 10 and 90 % shares are of the highest
  frame over that span [fact]. A slow attack has no top before its end,
  since a linear ramp keeps rising by more than 1 dB until 1 dB below its
  peak [fact].
- The peak of the whole note is no top: a real note swells or ripples after
  its attack [fact]. At 110 and 261.63 Hz, a 50 ms attack that swells by
  3 dB over 600 ms read 514 ms against the peak and reads 52 to 55 ms
  against the top; at 261.63 Hz, a 10 ms attack under a 6 dB ripple at
  5 Hz read 30 ms and reads 12.5 ms [synthetic].
- A swell finds no top when it rises 1 dB or more over the attack's own
  length [fact]. A 300 ms attack under a swell over 0.8 s reads 324 to
  370 ms up to 2.9 dB and 640 to 706 ms from 3.1 dB, at 55 to 440 Hz and
  22.05 to 48 kHz; at 3 dB it reads either [synthetic]. The 50 ms attacks
  above lie far from that edge [synthetic].
- The factor of 1.25 holds for a linear rise, so a rise that flattens near
  its top reads long [fact]. Loud isolated bass notes on real stems read
  141 and 156 ms where the level crosses -1 dB at 103 and 105 ms and -3 dB
  at 72 and 42 ms, on 9 and 22 notes [fact]. Loud isolated keyboard and
  synth notes on real stems read 15 and 20 ms against -1 dB crossings at
  18 and 14 ms [fact].
- A periodic wave gives one amplitude sample per period, so a rise under
  about 1.5 periods cannot be told from a shorter one [fact]. The attack
  is an upper bound when the rise is shorter than 1.5 RMS windows, or, on
  the amp proxy, shorter than 0.98 over its cutoff, which is twice the
  rise of the filter on an instant step [fact]. `attack_is_upper_bound` is
  set when more than half the pooled attacks are upper bounds [fact].
  `attack_s` then still reports the read, and a preset should not take it
  [fact]. Each note record carries its own `attack_upper_bound` [fact].
- #69 writes an attack from 1.5 periods. The layer tests the rise, 0.8 of a
  linear attack, so its flag covers attacks up to about 1.9 periods, which
  is 45 ms at 41.2 Hz [fact]. It departs from #69 on the safe side: a 2.2
  period attack is the shortest in the tests that reads its time
  [synthetic].

The attack ladder: one saw note with a linear attack, read attack in ms,
`bound` for an upper bound [synthetic]:

| f0 | 1 ms | 5 ms | 10 ms | 20 ms | 50 ms | 100 ms | 300 ms |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| 41.2 Hz | bound | bound | bound | bound | 53.3 | 95.9 | 285.2 |
| 55 Hz | bound | bound | bound | bound | 50.8 | 94.1 | 295.3 |
| 110 Hz | bound | bound | bound | 19.4 | 47.5 | 100.7 | 297.5 |
| 220 Hz | bound | bound | 9.4 | 18.7 | 50.5 | 99.0 | 300.1 |
| 440 Hz | bound | 5.2 | 9.5 | 20.0 | 49.6 | 99.9 | 299.6 |

Every attack over 4 periods reads within 15 %, and every one under 1.5
periods reads as a bound [synthetic].

## Hold, decay and sustain

- `hold_s` is the time from the first frame within 1 dB of the peak to
  the start of the decay, 0 when under 10 ms [fact, proposal for 10 ms].
- `decay_s` is the time between the first frames after the peak at which
  the level falls through 10 % and 90 % of the sustain level in dB, linear
  between frames, divided by the gap between the two shares [fact]. The
  first share is at least 0.5 dB down, so ripple at the peak does not end
  the hold, and it rises above 10 % for a sustain shallower than -5 dB
  [fact]. The decay starts that share of its time before the first share's
  crossing [fact]. A regression in dB from the peak read the decay 6 to 22 %
  long, since the knee at the sustain level bends the line [synthetic].
- `sustain_db` is the median of the last 30 % of the held part [fact].
  The held part runs from the first frame within 1 dB of the peak to the
  note-off knee, or to 25 ms before the note's end without one [fact].
- Decay and sustain are null when a note with a knee holds for less than
  the attack, the hold and three decay times [fact]. A note without a
  knee, such as a pluck, sustains at its end level [fact].
- `fit_residual_db` is the median over the pool of the RMS in dB of each
  held part against the model: 0 dB over the hold, a fall linear in dB to
  the sustain level over the decay, then the sustain level [fact].
- `ripple_db` is the median, over the pooled notes whose sustain is
  observable, of the 90th minus the 10th percentile of the sustain window
  after its line [fact]. Unison and chord beating show here [fact].
- A Levenberg-Marquardt refine of a piecewise linear model over the pooled
  dB curve, with a floor, a peak and a fixed note-off, is not applied
  [fact]. Fitted from each note's top with the peak free, it moved the
  decay error on 72 lines with decays of 50 to 600 ms from a median of
  4.4 % and a 90th percentile of 8.7 % to 9.9 % and 26 %, and left the
  median sustain error at 0.24 dB [synthetic]. A fit in dB trades the two
  ends of a fall against each other, which the read at 10 and 90 % of the
  fall does not [fact].

On a grid of 240 lines of eight saw notes, decays of 50 to 600 ms, sustain
levels of 0 to -24 dB, holds of 0 and 30 ms and releases of 20 to 300 ms
[synthetic]:

| Decay law | Decay read over true | Sustain error | Hold, true 30 ms | Fit residual |
| :-- | :-- | :-- | :-- | :-- |
| Linear in dB | 0.925 to 1.015 | at most 1.1 dB | 28.5 to 30.5 ms | 0.02 to 0.63 dB |
| Linear in amplitude | 0.88 to 1.0 | at most 0.51 dB | up to 130 ms | 0.02 to 1.55 dB |

A decay linear in amplitude starts slowly in dB, so its first 0.5 dB
reads as hold: up to 104 ms with no true hold [synthetic]. The fit
assumes a decay linear in dB, and its residual rises on the other law
[fact].

Plucks on 55 and 220 Hz with an exponential decay and no note-off knee
sustain at their end level: -57 dB for a time constant of 150 ms,
-21.5 dB for 400 ms and -8.6 dB for 1 s [synthetic].

## Note-off knee

The knee is the time the note-off starts the release [fact]. It is read
on the linear amplitude of the amp proxy, from the first frame within
1 dB of the peak to 25 ms before the note's end [fact]:

- At each split, a line over up to 100 ms before it and one over up to
  200 ms after it are fitted against one line over both [fact]. The right
  line stops where the level falls under 2 % of the mean before it, so
  silence after the release does not flatten it [fact].
- A split counts when the right line is steeper by half the mean before
  it per second, falls by half that mean over its side, and the two lines
  leave at most half the squared error of one [fact, proposal for the
  shares].
- Splits that count in a row form a corner, and the split with the
  largest gain in the last corner wins [fact]. A hold that breaks into a
  linear decay forms an earlier corner; the largest gain over all splits
  took it for the note-off [synthetic].
- The knee is where the left line crosses a line over the first 10 ms
  after the split, or the split itself when the crossing lies outside the
  two lines [fact]. A step down followed by a slow fall crossed 20 ms
  before the search started [synthetic]. The crossing of the two fitted lines read a
  linear release late, since the right line averages the release's tail
  [synthetic].
- No split counts on a pluck that decays at one rate, and its knee is
  null [fact].

`note_off_offset_s` is the note's end in the note list minus its knee,
over the pooled notes with a knee [fact]. On the grid, all 1920 knees lie
-2.0 to +0.01 ms from the true note-off [synthetic]. Read in dB, a 200 ms
linear release placed the knee 168 ms late, because a linear fall in
amplitude bends in dB [synthetic].

## Pooling and confidence

- The pool leaves out a note whose peak lies 12 dB or more below the peak
  of a note it overlaps: a quiet second voice, such as another part's
  bleed or a ghost note, has an envelope of its own [fact, proposal for
  12 dB]. Eight A2 notes decaying over 300 ms, each over an E3 18 dB
  quieter decaying over 50 ms, read the A2 decay within 20 % with the
  gate; the E3 notes would otherwise fill half the pool [synthetic].
- Of the other notes, the pool holds those whose length to the knee, or to
  the end without one, lies within a factor of 2 of their median length,
  and whose pitch lies within 12 semitones of their median pitch [fact,
  proposal].
- Each statistic is the median and the interquartile range over the pool
  [fact]. It reads low over fewer than 3 notes, or with an interquartile
  range above half its median [fact]. Its reasons are `too_few_notes` and
  `relative_spread`, and the attack adds `upper_bound_share` [fact]. A
  median of 0 under an interquartile range above 0 has no ratio, so the
  statistic reads low with `zero_median`, that range, in place of
  `relative_spread` [fact].
- `level_spread_db` is the 90th minus the 10th percentile of the pooled
  peak levels, null under 3 pooled notes and always low: no stem has
  tested the rule [fact, proposal]. Peaks 0, 3, 6 and 12 dB apart read
  0.04, 2.97, 5.97 and 11.97 dB [synthetic].
- The group's own confidence is medium with 3 or more pooled notes, else
  low, and no estimate when no note was read [fact].

## Sidechain pump

A sidechain compressor keyed by a kick ducks a held part on every beat
and lets it recover until the next [fact]. The layer reads the dip on
the beats that one held note covers, and strips it before it reads the
notes [fact].

- Track. The full-band centred RMS in dB over two periods of the lowest
  note, on a 1 ms hop [fact].
- Beats. A beat of the grid counts when the held part of one note covers
  it from 50 ms or more before its start to its end [fact]. A pluck struck
  on the beat never counts, nor a release at the end of a beat [fact].
  The held parts come from a first read of the notes without the pump,
  and the notes are read again with it stripped [fact]. Plucks on every
  beat read 13.9 dB of ducking without this rule [synthetic].
- Curve. Each counted beat is binned into 64 phase bins and taken
  relative to the 90th percentile of its bins; the curve is the median
  over the beats [fact]. The pump needs 4 counted beats; with fewer it is
  null [fact].
- Period. A sidechain follows the song's tempo, and a stem's tempo may be
  off by 1 or 2 % [fact]. Over 32 beats a 1 % error moves the duck a third
  of a beat, so its beats no longer fold onto one curve [fact]. The layer
  searches periods within 2.5 % of the grid's for the one whose counted
  beats fold onto the deepest mean curve, over up to 64 beats of the
  longest run of counted beats and then over all of them, in steps that
  move the last beat of the span by 1 bin [fact, proposal for 2.5 %]. The
  second stage searches one step either side of the first, so a read can
  reach about 2.7 % [fact]: 0.9732 to 1.0268 of the grid [synthetic]. Of 72
  ducks up to 2 % off the grid over 32 and 64 beats, 15 locked and stripped
  on the grid alone, and all 72 do with the search [synthetic].
  `pump_bpm` reports the tempo of the read that stays [fact]. The search's
  read must lock to the beat, and it replaces the grid's read as follows
  [fact, proposal for 1.5 dB, 60 cents, 3 bins and half a window]:
  - The grid's read follows the notes or nothing: always.
  - The grid's read locks to the beat: when the search's read dips 1.5 dB
    or more deeper and does not give up the strip. A slower period counts
    fewer beats, so a grid lock over 8 beats can read deeper over 7
    [synthetic]. A 12 dB duck 2.5 % off over 16 beats reads 4.4 dB on the
    grid and 9.8 dB at its own period [synthetic].
  - The grid's read runs free: when the line lies within 60 cents of a
    division, and the search's curve falls faster than it recovers and
    takes long enough from half its depth back to a quarter: 3 bins, and
    half the level window in bins. A gate near a division folds onto one
    curve as well, but comes back at once, within the smear of the window
    [synthetic].

  The set behind the numbers below holds 123,840 generated signals: a
  three-note chord from 55, 110 and 220 Hz, 60 to 174 BPM, runs of 8.9 to
  32 beats, four phases for ducks and two for gates and saw tremolos. Each
  cell counts the signals that strip [synthetic].

  | Signal | Off the tempo | Signals | Search skips free reads | No free rule | This rule |
  | :-- | :-- | :-- | :-- | :-- | :-- |
  | Duck, release 80 ms or more | up to 2.5 % | 47520 | 35121 | 35220 | 35858 |
  | | 3 % | 8640 | 1242 | 4927 | 4867 |
  | | 3.5 % | 8640 | 168 | 4131 | 1716 |
  | | 4 to 5 % | 17280 | 128 | 4723 | 134 |
  | Square gate | 3 % | 2880 | 434 | 1181 | 438 |
  | | 3.5 % | 2880 | 181 | 806 | 186 |
  | | 4 to 5 % | 5760 | 35 | 624 | 35 |
  | Saw tremolo | 3 % | 720 | 71 | 498 | 498 |
  | | 3.5 % | 720 | 0 | 352 | 148 |
  | | 4 to 5 % | 1440 | 0 | 293 | 0 |

  The first read's strips on gates come from their own lock on the grid.
  A gate up to 2.5 % off strips either way; see Limits [synthetic].
- `pump_depth_db` is minus the curve's minimum [fact].
- `pump_recovery_s` is the time from the minimum until the curve comes
  back within 1.5 dB, going on over the beat's wrap, so a duck that sits
  late in the beat recovers into the next [fact]. It is null when the
  curve does not come back within one beat [fact].
- `pump_tau_ms` is the recovery over $\ln(\text{depth} / 1.5)$, the time
  constant of an exponential recovery in dB, and null unless the pump
  locks to the beat: a dip that follows no beat has no recovery to model
  [fact].
- `pump_beats` counts the beats behind the curve [fact].

`pump_locked_to` names what the dip follows, tested in this order
[fact, proposal for the thresholds]:

1. `note`: the beats with a note onset inside dip 3 dB or more, and the
   others under 1.5 dB, each group of 2 beats or more. An onset up to 50 ms
   before a beat belongs to that beat, as the transcriber places onsets a
   few milliseconds early [fact].
2. `free`: the level swings 1 dB or more at a rate of 0.5 to 20 Hz that
   lies 50 cents or more from every note division, read on the modulation
   spectrum over the longest covered span [fact]. It comes before the
   beat: a 5.3 Hz tremolo over 17 beats at 128 BPM left a 3.6 dB dip in
   the median curve that passed the beat test [synthetic]. A duck's
   strongest line lies on the beat rate, so it reads free when that rate
   lies 50 cents or more from every division of the grid [fact]. That
   holds 3 % off the tempo, and over a short span from 2.5 %, where the
   line reads 50 to 52 cents [synthetic]. The period search then decides,
   as above [fact].
3. `beat`: the curve dips 3 dB or more in one run of bins half its depth
   down, and falls into its dip within 8 bins, reading on into the beat
   before and not round the beat's wrap. The odd beats and the even beats
   must each do the same on their own median curves, to half its depth at
   its dip. Half the beats must fall by half its depth into its dip: from
   their highest bin over the 8 bins before the 3 bins either side of it
   to their lowest within those. A note that decays across the beat falls
   over most of it, and fails the 8 bins [fact]. The one run stops a
   tremolo at a division of the beat [fact]. A level that rises over each
   bar or two beats and drops back makes every beat's curve start low, and
   round the wrap that read as a fall on every beat: 12 dB per bar and 6 and
   12 dB per two beats all stripped [synthetic]. Those falls only come on
   the first beat of each bar or pair, so the rule reads no pump there
   [synthetic].
4. `none` otherwise.

The layer strips the curve from the note tracks only when it locks to the
beat over 8 counted beats or more, and sets `pump_stripped` [fact,
proposal for 8]. A stem can duck in one section only, so a note takes the
strip only when the median fall of the beats over it reaches half the
curve's depth [fact]. With fewer beats, half of them is two or three, and
other sounds on the beat pass the lock: a sine sub under a kick on every
beat, whose tail beats against the sub, read 3.4 to 4.6 dB locked to the
beat over 4 or 5 beats [synthetic]. On 1375 runs over the generated stems
of the pitch layer, which hold no duck, 4 read such a lock, and none is
stripped [synthetic].

A duck's dip can read as a note-off knee in the first read and cut the
held parts short [synthetic]. A read of the held parts that finds no pump,
or one that locks to the beat over fewer than 8 beats, is therefore counted
again over each note to 25 ms before its end, and strips when that count
locks to the beat over 8 beats or more [fact]. A 6 dB duck on the chord at 174 BPM under noise at -20 dB
read a false knee on each of its three notes and was not stripped; counted
over the notes, it strips, and no knee remains [synthetic].

A constant three-note saw chord reads 1.64 to 1.94 dB from its own beating,
so the 3 dB floor sits above it [synthetic]. Over all 1111 parts, all 826
locks read as built [synthetic].

### Depth bias

The read depth is shallower than the duck, and the bias depends on the
duck's shape [synthetic]. A duck whose gain recovers exponentially,
$1 - (1 - g_0) e^{-t / \tau}$, has a sharp minimum in dB; the RMS window
and the 64 bins round it off [fact]. A cosine dip has a round minimum and
reads near its depth [synthetic].

Measured minus true depth on a held saw chord and a held 55 Hz sub, two
seeds, noiseless and with white noise at -30 and -20 dB, releases of 50,
80, 150 and 300 ms; median and range over 48 parts per cell [synthetic]:

| Depth | 90 BPM | 128 BPM | 174 BPM |
| :-- | :-- | :-- | :-- |
| 3 dB | -0.46 (-1.18 to 0.50) | -0.48 (-1.12 to 0.51) | -0.63 (-1.39 to 0.81) |
| 6 dB | -1.39 (-2.69 to -0.32) | -1.45 (-2.60 to -0.39) | -1.75 (-2.83 to -0.25) |
| 9 dB | -2.65 (-4.57 to -1.07) | -2.77 (-4.39 to -1.13) | -3.03 (-4.68 to -1.41) |
| 12 dB | -4.29 (-6.75 to -1.91) | -4.18 (-6.45 to -2.03) | -4.69 (-6.79 to -2.42) |
| 18 dB | -8.39 (-11.76 to -3.62) | -8.03 (-11.29 to -4.31) | -8.40 (-11.63 to -4.87) |

- No duck reads 0 to 0.09 dB on the sub, and 1.64 to 1.94 dB on the chord
  from its beating [synthetic].
- Noise at -20 dB fills the dip: a 12 dB duck reads 1.5 dB shallower than
  without noise [synthetic].
- At 128 BPM on the noiseless chord, the bias over the releases is -0.2
  to -5.5 dB for a duck linear in dB, -0.3 to -7.2 dB for one with a 5 ms
  fall, and +1.2 to -2.6 dB for a cosine dip [synthetic].
- The bias changes with the shape at the same depth, so no single factor
  corrects it, and the layer reports the depth as read [fact].

The recovery over the true time back within 1.5 dB, at depths of 6 dB or
more, is 0.64 to 1.44 by release on the chord and 0.55 to 0.80 on the sub
[synthetic]. The shallower read reaches 1.5 dB sooner on the sub, and the
chord's beating moves the crossing [synthetic].

## Limits

- An attack under about 1.9 periods of the fundamental is an upper bound,
  and at 41.2 Hz that covers attacks up to 45 ms [fact].
- A decay linear in amplitude reads part of its start as hold [synthetic].
- The upper note of an octave or a unison has no free harmonic and is
  skipped [fact].
- A resonant filter that closes onto the fundamental changes the amp
  proxy itself [fact].
- A release that falls, flattens and falls again reads its knee at the
  second fall, since the last corner wins to pass over a decay [synthetic].
  Its sustain window then reaches into the first fall [fact].
- The beat grid starts at 0 s with the tempo as used. The period search
  finds one constant tempo within 2.5 % of it; a tempo that moves inside
  the stem still smears the curve [fact]. Its first stage reads up to 64
  beats of the longest run, so a duck off the tempo that starts later in
  one held note can go unfound [fact]. On the 110 Hz chord, over runs of
  10, 12, 16 and 32 beats and four phases, up to 2.5 % off the tempo, of 176
  cells at each tempo [synthetic]:
  - A 12 dB duck with an 80 ms release locks in all 176 from 60 to 128 BPM,
    and in 174 at 174 BPM.
  - A 6 dB duck with a 150 ms release locks in 172, 165, 135, 159 and 131 at
    60, 80, 96, 128 and 174 BPM. It locks in 8 of 16 on the tempo at 96 BPM,
    and in 1 of 16 at 2.5 % above at 174 BPM.
- A read that stays on the grid reports the grid's tempo, though the duck
  can lie off it [fact]. Of 14,502 ducks up to 2.5 % off that stayed, the
  tempo was off by a median of 0.99 % and at most 2.56 %; of 32,480 fitted
  reads, by a median of 0.07 % and at most 2.22 % [synthetic].
- A duck on the tempo can move: up to 34 of 4847 grid locks gain 1.5 dB by
  fitting the chord's beating, nearly all 18 dB at 220 Hz, and read up to
  0.22 % off [synthetic].
- The search takes the deepest fold, so a second modulation near the tempo
  can take it [fact]. Under a duck on the tempo, a deeper second duck 0.5 to
  2.5 % off took 53 of 64 locks, and a shallower sine, duck or gate took 15
  of 623 [synthetic].
- A gate cuts across notes on the beat as a pump does; no test tells the
  two apart on a read that does not run free [fact]. A lock on a level with a
  duck's shape off a division, such as a saw tremolo that falls at once and
  rises over the beat, strips as a duck too. Of 360 saw tremolos each, 266
  strip at 3 % above the tempo against 63 with no search on free reads, 232
  against 8 at 3 % below, 138 against 0 at 3.5 % above and 10 against 0 at
  3.5 % below, and none at 4 to 5 %, on the set above [synthetic].
- The release time is not read yet [fact].

## Sources

- Envelope stages: Xfer Records,
  [Serum 2 manual](https://xferrecords.com/manual/serum-2). Facts only.
- Attack and decay descriptors: Peeters, Giordano, Susini, Misdariis and
  McAdams,
  [The Timbre Toolbox: extracting audio descriptors from musical signals](https://doi.org/10.1121/1.3642604),
  JASA 2011. Inspiration only.

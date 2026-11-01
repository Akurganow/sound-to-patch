# Envelope layer

`analyze` reads the envelope layer after the pitch layer: the amplitude
envelope of each note, pooled over the notes, and the sidechain pump of
the stem. The layer writes the `envelope` group of `layers.json`; the
[layer report page](../reference/formats/layers.md#the-envelope-group)
lists its fields. The tags are those of the [pipeline page](pipeline.md).

The stages follow the envelope of a synth: an attack from the note-on to
the peak, a hold at the peak, a decay from the peak to the sustain level,
the sustain level, and a release from the note-off [fact]. The layer
reads the attack, hold, decay and sustain, the time of the note-off knee,
and the release and tail after it [fact].

Figures come from `cargo xtask eval --layer envelope`, 1191 generated parts
at 44,100 Hz scored against their recipes, or from the acceptance tests at
22,050 Hz, through the release build and with the true note list [synthetic].
The tables of the release and tail come from one-off generated sets, named
where they appear, that are not committed [synthetic]. A real stem adds filters, reverb, bleed and
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

## Release and tail

After the knee, relative to the median level over the 50 ms before it, the
level follows a linear release plus a decaying tail [fact]:

`env(t) = max(1 - t / release_s, 0) + tail_weight * 10^(-3 t / tail_rt60_s)`

- The fit is a simplex on the squared error in dB, from 4 releases and 3
  RT60s as starts, down to -55 dB [fact]. It keeps every frame for 200 ms and
  every fifth after that, weighted to count the same [fact]. The project has no
  least-squares crate, so it uses the simplex of the pitch fits [fact].
- It reads the one-period RMS, not the amp proxy: a reverb of one harmonic fades
  deeply, and many harmonics fade apart [synthetic]. A note that overlaps another
  reads the band of the amp proxy [fact].
- It stops at the note's end or the next note that overlaps it, half a level
  window before that note [fact]. At 41 and 55 Hz that window otherwise reads the
  next note and left residuals of 6 dB on dry lines [synthetic].
- A note with no knee has no fit, so a pluck reports no release [fact]. A given
  note list that ends at the note-off shows no knee and reads no release [fact].

The write rule reads the medians over the pooled notes with a fit, and never the
release, which is what fails to fit [fact]:

- A `tail_weight` over 0.5 or a residual over 3 dB writes no `release_s`, and
  `release_confidence` reads low [fact, proposal for 3 dB]. Both still report the
  tail [fact].
- `tail_rt60_s` reads over the notes with a `tail_weight` of 0.05 or more whose
  frames span a 15 dB fall of it, and is null under a median `tail_weight` of 0.05, because a dry note often leaves a
  little weight whose RT60 is noise [fact, proposal for both].
- A written `release_s` reads low, with `tail_votes` or `tail_weight`, when any vote
  below is true or the median `tail_weight` is 0.05 or more [fact, proposal]. A knee
  under a reverb can lie in the tail, where the release reads a fade [synthetic]. On
  the set below this leaves 3 reverb stems at medium with a release over 50 % off,
  against 143 without it, and takes medium from 20 of 757 dry stems [synthetic].
- `tail_rt60_s` leaves out each note heavier than 0.5 at a residual of at most 1.1 dB,
  and each note that overlaps another under 6 periods of its narrowest band's cutoff
  [fact]. The two rules next read the same over the pool's medians.
- `tail_rt60_s` is null, with `tail_smooth`, when the median `tail_weight` is over
  0.5 and the median residual is at most 1.1 dB [fact, proposal for 1.1 dB]. A dry
  exponential release fits the tail term alone, at a weight of about 1 and an RT60
  equal to its fall, while a reverb's level wobbles [synthetic]. Of 15 dry falls of
  60 dB over 0.1 to 1.5 s at 55 to 220 Hz, 14 read this way, and the fall of 0.1 s at
  55 Hz stays a false RT60 at a residual of 2.7 dB [synthetic]. A curved release
  model waits for the calibration of the decay law under #69 [fact].
- `tail_rt60_s` is null, with `tail_band`, when a note that overlaps another reads a
  median RT60 under 6 periods of its narrowest band's cutoff [fact, proposal for 6].
  The band rings by itself: a dry chord with a release of 20 or 50 ms read 0.13 to
  0.2 s on 18 of 18 stems without this rule and on none with it [synthetic].
- `release_confidence` is null when no pooled note has a fit [fact].

The set: 4000 stems sampled over 41 to 440 Hz, releases of 20 to 800 ms, dry or
with an RT60 of 0.3 to 3 s at 0.05 to 0.8 wet, three timbres, 1 to 6 notes of 0.3
to 1.2 s, gaps of 0.2 to 1.5 s, sustains of 0 to -12 dB, mono or wide reverbs, and
a reverb whose highs fade first [synthetic]. Shares hold within about 2 percentage
points at this size. On it:

| Stems | Count | Release written | Within 50 % of the truth |
| :-- | :-- | :-- | :-- |
| Dry, release up to 400 ms | 1157 | 1152 | 1108 |
| Dry, release 800 ms | 215 | 0 | - |
| Reverb, release up to 400 ms | 2227 | 1232 | 977 |
| Reverb, release 800 ms | 401 | 25 | 3 |

- The median written release over its truth on dry stems is 0.98 to 1.12 by
  release, with the 1.12 at 20 ms [synthetic].
- Over the true RT60, the median read is 1.21 at 0.3 s, 0.98 at 0.6 s, 0.90 at
  1.5 s and 0.86 at 3 s, and 28 to 39 % lie within 10 % [synthetic].
- `tail_weight` over 0.5 holds out 19 % of stems at a true 0.05 wet, 43 % at 0.4
  and 55 % at 0.8 [synthetic]. Of the 937 reverb stems over 0.5, 109 read as smooth and
  lose their RT60 [synthetic].
- A knee lies within 15 ms of the note-off in 98.8 % on dry stems and 42.1 % under
  a reverb, with a 90th percentile of 1.1 s [synthetic].

### Votes that a tail is reverb

`tail_votes` counts five signs, each over the pooled notes with a fit and by
majority where it reads notes, and null where nothing could be read [fact]. All
thresholds are proposals [fact].

| Vote | True when | Reads true, dry / reverb |
| :-- | :-- | :-- |
| `outlasts_gap` | Before the next note, with its 20 ms window past the release and no other note sounding, the level lies above -40 dB of the note body | none read / 53 % |
| `side_grows` | Side over mid grows 3 dB or more from the body to the tail | none read / 67 % |
| `flatness_rises` | Flatness rises 0.1 or more from the body to the tail | none read / 77 % |
| `same_rate` | The RT60 of 3 or more notes that show one spreads under a quarter of its median | 88 % of 25 / 82 % |
| `high_band_faster` | The share of the bands from 1 kHz falls 3 dB or more over the tail | none read / 37 % |

A dry stem reads none of the first three, as its tail ends with the release
[synthetic]. Two or more votes pass on none of the 1372 dry stems and on 60 % of the
2628 under a reverb [synthetic]. A noise floor reads as a tail: on dry lines at 55 to
220 Hz under noise at -50 and -40 dBFS, 54 of 54 stems pass two votes [synthetic]. `tail_is_reverb` stays false: a reverb is stated only
when the votes agree and a space layer reads medium or better, and no space layer
exists yet [fact].

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
  second stage searches one step either side of the first, so the reach
  follows the longest run of counted beats: up to 2.73 % at 4 beats, 2.81 % at
  5, 2.60 % at 6, 2.68 % at 7, 2.66 % at 10, 2.58 % at 20 and 2.51 % at 64
  [fact]. Of 72
  ducks up to 2 % off the grid over 32 and 64 beats, 15 locked and stripped
  on the grid alone, and all 72 do with the search [synthetic].
  `pump_bpm` reports the tempo of the read that stays [fact]. The search's
  read must lock to the beat, and it replaces the grid's read as follows
  [fact, proposal for 0.9, 0.15 dB, 60 cents, 3 bins and half a window]:
  - The grid's read follows the notes: always, with no shape test.
  - The grid's read locks to the beat: when both reads strip or neither does,
    the search's fold error must be under 0.9 of the grid's and 0.15 dB lower.
    The fold error is the standard
    deviation of the level minus the curve over the counted beats, each beat
    about its own mean, so a fade or a level step between beats does not
    count as misfit [fact]. When only one read strips, as a
    slower period counts fewer beats, the search's read must be the one that
    strips and have the lower fold error. A 12 dB duck 2.5 % off over 16
    beats reads 4.4 dB on the grid and 9.8 dB at its own period [synthetic].
    Depth is no test: the search deepens a duck on the tempo by fitting a
    chord's beating, and on the flat level of the 76,836 stems of Limits a gain
    of 1.5 dB moved 15 ducks on the tempo, against 3 with this rule
    [synthetic]. A sub's fold error is small enough
    that a share alone moved ducks on the tempo at 174 BPM, and the sustain
    with them by up to 2.6 dB [synthetic].
  - The grid's read runs free: when the line lies within 60 cents of a
    division, and the search's curve falls faster than it recovers and
    takes long enough from half its depth back to a quarter: 3 bins, and
    half the level window in bins. A gate near a division folds onto one
    curve as well, but comes back at once, within the smear of the window
    [synthetic].
  - The grid's read reads none: when the search's curve falls faster than it
    recovers and takes 3 bins from half its depth back to a quarter. A gate off
    the tempo often reads no lock on the grid, and the level window's share is
    left out because it costs real ducks under a low note; see Limits
    [synthetic].

  The set behind the numbers below holds 123,840 generated signals: a
  three-note chord from 55, 110 and 220 Hz, 60 to 174 BPM, runs of 8.9 to
  32 beats, four phases for ducks and two for gates and saw tremolos. Each
  cell counts the signals that strip, from one build. The columns differ
  only for a grid read that runs free: it stays free, it takes any search
  read that locks, or it takes one under this rule. A free read never
  strips, so this rule reads no more than the column before it [synthetic].

  | Signal | Off the tempo | Signals | Free read stays | No free rule | This rule |
  | :-- | :-- | :-- | :-- | :-- | :-- |
  | Duck, release 80 ms or more | up to 2.5 % | 47520 | 36008 | 36107 | 36078 |
  | | 3 % | 8640 | 1343 | 5028 | 4883 |
  | | 3.5 % | 8640 | 169 | 4132 | 1716 |
  | | 4 to 5 % | 17280 | 122 | 4717 | 128 |
  | Duck, release 30 ms | up to 2.5 % | 15840 | 7440 | 7462 | 7453 |
  | | 3 % | 2880 | 622 | 801 | 734 |
  | | 3.5 % | 2880 | 426 | 612 | 459 |
  | | 4 to 5 % | 5760 | 160 | 287 | 160 |
  | Square gate | 3 % | 2880 | 21 | 768 | 25 |
  | | 3.5 % | 2880 | 42 | 667 | 47 |
  | | 4 to 5 % | 5760 | 14 | 603 | 14 |
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
so the 3 dB floor sits above it [synthetic]. Over all 1191 parts, all 826
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
- A release over about 400 ms shows no knee, because its right line must fall by
  half the held level over 200 ms: no dry 800 ms release reads [synthetic]. Under a
  reverb, 25 of 401 such stems wrote a release, 3 of them within 50 %, from a knee in
  the tail [synthetic].
- A reverb moves the knee: the last corner wins, and the fades of a tail form corners
  too, so a knee can lie over 1 s past the note-off [synthetic]. On a line of 6 notes,
  over 60 noise reverbs each, these knees lie within 15 ms [synthetic]:

  | Release | RT60 | Wet | Knees of 6 | Release written | Within 35 to 60 ms | RT60 within 10 % |
  | :-- | :-- | :-- | :-- | :-- | :-- | :-- |
  | 50 ms | 0.6 s | 0.15 | 2.77 | 38 of 60 | 32 | 49 of 60 |
  | 50 ms | 1.5 s | 0.15 | 0.00 | 0 of 60 | - | 15 of 60 |
  | 200 ms | 1.5 s | 0.4 | 0.02 | 0 of 60 | - | 4 of 60 |
  | 800 ms | 1.5 s | 0.4 | 0.00 | 0 of 60 | - | 13 of 60 |

- A later corner replaces an earlier one with no test of the level between them.
  Tested rules that kept the note-off under a reverb lost dry notes with a small
  tremolo [synthetic]. The cost stays: a sustain shorter than its decay reads 108 of
  144 knees in the eval, and a dry tremolo with a release of 200 ms or more 9 of 64
  [synthetic].
- A dry note can report a false RT60: 55 of 1372 dry stems on the set above, 107 of
  1500 random ADSR lines and 359 of 1620 on an ADSR grid, and 1 of 15 exponential falls
  [synthetic].
- A release of 20 ms at 41 Hz is shorter than the level window: of the 255 reverb
  stems written more than 50 % off with a release up to 400 ms, 98 have a release of
  20 ms [synthetic].
- The fit has no echo term, so a delay reads as release or tail [fact].
- The bench has no pre-delay, and the reverb is noise [fact].
- The beat grid starts at 0 s with the tempo as used. The period search
  finds one constant tempo within up to 2.81 % of it; a tempo that moves inside
  the stem still smears the curve [fact]. Its first stage reads up to 64
  beats of the longest run, so a duck off the tempo that starts later in
  one held note can go unfound [fact]. On the 110 Hz chord, over runs of
  10, 12, 16 and 32 beats and four phases, up to 2.5 % off the tempo, of 176
  cells at each tempo [synthetic]:
  - A 12 dB duck with an 80 ms release locks in 173, 175, 176, 176 and 173 at
    60, 80, 96, 128 and 174 BPM.
  - A 6 dB duck with a 150 ms release locks in 172, 165, 135, 159 and 131 at
    60, 80, 96, 128 and 174 BPM. It locks in 8 of 16 on the tempo at 96 BPM,
    and in 1 of 16 at 2.5 % above at 174 BPM.
- A read that stays on the grid reports the grid's tempo, though the duck
  can lie off it [fact]. Of 11,161 ducks up to 2.5 % off that stayed, the
  tempo was off by a median of 0.99 % and at most 2.56 %; of 33,787 fitted
  reads, by a median of 0.08 % and at most 2.22 % [synthetic].
- A duck on the tempo can move from a grid lock. On the 123,840 signals,
  1 of 4847 grid locks does, 6 dB at 110 Hz and 60 BPM, by 0.89 %. The 76,836
  stems, read through the layer, hold runs of 8.9 to 32 beats, ducks of 6, 12
  and 18 dB and 4 phases, on a flat level, under fades of 12, 18 and 36 dB
  and under 6 dB steps every 4 beats. There 3 of 4595 grid locks move, by up to
  0.89 %: 3 on the flat level and 0 under each fade or the steps. A wider set
  reaches more: 9,600 stems on the tempo per level, from 3 pitches and 5
  tempos, with 10 runs of 8.9 to 32 beats, 8 duck settings of 3 to 18 dB and 8
  phases. Its grid locks move in 6 stems on a flat level, 7, 12 and 10 under
  those fades, and 0 under the steps. These are ducks of 6 dB or less over 8.9
  to 12.3 beats, by up to 2.7 %. A 6 dB, 150 ms duck at 110 Hz and 60 BPM over
  10 beats, under the 36 dB fade, reads 59.479 BPM [synthetic].
- A grid read of none takes a fitted beat lock only when its curve falls faster
  than it recovers and takes 3 bins from half its depth back to a quarter, which
  holds out most gates. The price in ducks is small for releases of 80 ms or
  more and large at 30 ms, which loses 2004 of 6406 below. The set: 38,400 ducks on the
  tempo, from 3 pitches, 5 tempos, 10 runs of 8.9 to 32 beats, 8 duck settings of
  3 to 18 dB and 8 phases; 46,080 ducks 0.5 to 2.5 % off, of 6, 12 and 18 dB with
  releases of 80 and 150 ms; 13,824 gates; flat and under fades of 12, 18 and
  36 dB. Counted on this path alone [synthetic]:

  | Fitted read over a grid read of none | Moves on the tempo | Ducks off the tempo that strip | Gates that strip |
  | :-- | :-- | :-- | :-- |
  | No test | 839 | 16,427 | 1733 |
  | This test | 653 | 16,150 | 59 |

  - The lost ducks are 277, or 1.7 %. Of them 132 lie at 55 Hz, 117 have a release
    of 80 ms and 160 of 150 ms [synthetic].
  - Short releases lose more, and slow tempos with them, as the test asks for 3
    bins of a beat. On the 123,840 signals, on this path, ducks that would strip
    lose 2004 of 6406 at 30 ms, 61 of 4773 at 80 ms, 7 of 4079 at 150 ms and 2 of
    3976 at 300 ms, and 998 of 4065 at 60 BPM, 523 of 3649 at 80, 326 of 4930 at
    96, 141 of 3422 at 128 and 86 of 3168 at 174. Over all ducks of 30 ms up to
    2.5 % off, 7453 of 15,840 strip, against 8996 with no test. A 12 dB, 10 ms
    duck over 480 beats does not strip [synthetic].
  - The moves left are 118 on a flat level and 181, 177 and 177 under the fades,
    by up to 2.46 %. There the duck is real: a move mostly shows that the grid read
    found no lock [synthetic].

- A chord whose level wanders with no pump can read as a pump. Under linear
  interpolation of Gaussian dB knots, spaced 0.25 to 1 beat, with sigmas of 1, 2
  and 4 dB, at 55, 110 and 220 Hz, 60 to 174 BPM and 9, 16 and 32 beats, 5 of
  2,592 strip, at 3 to 4 dB, 4 of them at a sigma of 4 dB [synthetic].
- A level step inside a beat stays in that beat's fold error, and a step on a
  beat edge does not, so a slightly different period can win by moving an edge
  onto the step. One step of 6, 12 or 24 dB, about 0.35 or 0.65 through the run
  and up to 0.15 of a beat from a beat edge, at 55 and 110 Hz, 96 to 174 BPM and
  16 and 32 beats, moves 21 of 4465 grid locks, by up to 1.15 % and with up to
  1.09 dB of lost depth. A 6 dB, 150 ms duck at 110 Hz and 128 BPM over 16
  beats, with a 12 dB step at 0.63 of the stem, reads 127.571 BPM. Its recovery
  reads 130 ms and its ripple 1.31 dB, against 203 ms and 0.69 dB on the grid.
  The same cause can hold a duck off its tempo. At 1 % off, a 48 dB fade over
  the last quarter finds the duck's tempo in 18 of 29, and 6 dB steps every 4
  beats in 22 of 31, against 28 of 31 with no steps [synthetic].
- A chord's own beating sets a floor of about 1.3 dB under the fold error,
  so a smeared duck can stay on the grid. On the 76,836 stems, of 9205 ducks
  that stayed where both reads lock, 654 would have read 1 dB or more less
  ripple at the fitted period, 2.12 dB on average. A 12 dB duck 1 % slow over
  12.3 beats at 96 BPM reads 6.5 dB of ripple on the grid and 1.1 dB at its
  own period, at fold errors of 1.42 and 1.33 dB [synthetic].
- The shape test can pass a gate. Over 8 phases and 9 to 32 beats, a 12 dB gate
  3 % below the tempo at 55 Hz and 174 BPM strips in 4 of 376 runs [synthetic].
- The depth reads shallow, and the bias depends on the shape: an 18 dB duck
  reads up to 11.8 dB short in the set of Depth bias. On the noiseless chords
  of the 123,840 signals, on the tempo, with releases of 80 ms or more, it
  reads a median of 4.9 dB and at most 9.2 dB short [synthetic].
- `peak_db` compares two notes directly only when they read the same
  harmonic, so the pool's level gate can leave out a note that reads a
  higher harmonic of a timbre with weak upper harmonics [fact].
- A second sound on the beat, such as a kick in the same stem, can read
  as a beat-locked pump; over fewer than 8 beats it is not stripped
  [synthetic].
- The search takes the deepest fold, so a second modulation near the tempo
  can take it [fact]. Under a duck on the tempo, a deeper second duck 0.5 to
  2.5 % off took 53 of 64 locks, and a shallower sine, duck or gate took 15
  of 623 [synthetic].
- A gate cuts across notes on the beat as a pump does; no test tells the
  two apart on a read that locks on the grid [fact]. A lock on a level with a
  duck's shape off a division, such as a saw tremolo that falls at once and
  rises over the beat, strips as a duck too. Of 360 saw tremolos each, 266
  strip at 3 % above the tempo against 63 with no search on free reads, 232
  against 8 at 3 % below, 138 against 0 at 3.5 % above and 10 against 0 at
  3.5 % below, and none at 4 to 5 %, on the 123,840 signals [synthetic].
- A heavy tail hides the release, so `release_s` is null over a median `tail_weight` of 0.5 [fact].

## Sources

- Envelope stages: Xfer Records,
  [Serum 2 manual](https://xferrecords.com/manual/serum-2). Facts only.
- Attack and decay descriptors: Peeters, Giordano, Susini, Misdariis and
  McAdams,
  [The Timbre Toolbox: extracting audio descriptors from musical signals](https://doi.org/10.1121/1.3642604),
  JASA 2011. Inspiration only.

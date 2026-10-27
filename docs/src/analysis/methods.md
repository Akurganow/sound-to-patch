# Common methods

The rules on this page bind every stage of the
[pipeline](pipeline.md). Units and confidence have their own page,
[Units](../reference/units.md), which this page does not restate. The tags
are those of the pipeline page.

## Samples in f64

Samples, spectra and every intermediate value are `f64` from the decoder
on, at the stem's native rate [fact]. `f32` appears only where a file
format stores 32-bit floats [fact]. Analysis never resamples; only a
consumer with a fixed input rate does [fact].

## The window

The analysis window is the periodic Hann,
$w[n] = 0.5 - 0.5 \cos(2 \pi n / N)$ for $n = 0 \ldots N - 1$: the first
$N$ samples of a symmetric window of length $N + 1$ [fact]. Its main lobe
is 4 bins wide null to null, and its -3 dB width is 1.44 bins [fact]. The
symmetric form would shift every bin slightly [fact]. A four-term
Blackman-Harris window, with lower side lobes and a wider main lobe, and
the rectangular window exist for comparison only [fact].

## The frame rule

- Frames are centred and zero-padded, with hop $N / 4$ [fact].
- The frame length is 8192 samples when the note list has a median f0
  below 100 Hz, else 4096, and 4096 without a note list [fact]. Harmonics
  $f_0$ apart need $N \ge 4 f_s / f_0$ for the 4-bin Hann main lobe; at
  48 kHz, 4096 points resolve harmonics down to 47 Hz, and 8192 down to
  23 Hz [fact].
- The stereo screen uses 8192 points for every f0, and the spectrogram's
  zoom panel 16384 [fact].
- `layers.json` records the length used as `conventions.frame_size`
  [fact].

## Active frames and statistics

- A frame is active when its RMS lies within 40 dB of the loudest frame's
  [fact, proposal for 40 dB]. The stereo screen takes the RMS over both
  channels [fact].
- A statistic over active frames is the median and the interquartile range
  [fact]. Both resist outliers, such as the frames of a note onset [fact].
- Quantiles interpolate linearly between the sorted values at position
  $q (n - 1)$, the convention of the common numeric libraries [fact]. The
  IQR is the 75th minus the 25th percentile [fact].
- A statistic without an active frame is no estimate with the reason
  `silent_stem`, never a number [fact].

## Conventions next to every number

Published tools disagree on weights, floors, windows and statistics, so a
number means something only with its convention [fact]. Every number in
the `stem` group of `layers.json` has a convention under the same key in
`conventions`, and a test fails when a field lacks one [fact]. The
convention names the weighting, the window, the floor and the statistic
[fact].

## Golden vectors

Expected values in tests come from an independent reference, never from
our own code [fact]. The [testing page](../development/testing.md#golden-numbers)
gives the commands.

- A pinned Python generator computes each reference value once, from a
  recipe that the JSON vector stores [fact]. The interpreter and every
  package are pinned with hashes, so a second run writes byte-identical
  files [fact].
- Each vector holds the recipe, the generator, the tool versions, the
  values and the tolerance [fact]. The Rust test rebuilds the signal from
  the recipe and compares with
  $|\text{actual} - \text{expected}| \le \text{abs} + \text{rel}\,|\text{expected}|$
  [fact].
- Noise comes from a SplitMix64 generator that both languages implement
  bit for bit [fact]. A statistic over noise is still compared as a
  statistic, with a tolerance that covers its spread [fact].
- CI never runs Python. It checks that every vector has its fields and
  names the pinned versions [fact].
- The figures that task issues quoted live as prototype tables with their
  recipe. A figure the recipe does not reproduce is recorded with a
  `reproduced: false` flag, and the committed table replaces it [fact].

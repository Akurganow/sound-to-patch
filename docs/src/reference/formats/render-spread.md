# Render spread

Two renders of one preset are not identical: on one Vital instance, each
render differs a little from the one before. The spread file says how
far renders of one preset lie apart, so a comparison can tell a real
difference from render noise. It is the noise floor that calibration,
compare, refine and the bench cite.

`cargo xtask render-spread --out spread.json` writes it: the
[testing page](../../development/testing.md#repeat-render-spread) gives
the method. The file is local and depends on the synth's build and its
quality setting. The repository keeps no spread of record; the
per-recording metrics are on the
[reference recordings](../../development/reference-recordings.md) page
only.

## The draws

A fresh Vital instance renders bit-identically to any other fresh
instance, and each later render on one instance differs: an ignored
Vital test pins both. So the 20 renders of a spread are drawn in
sequence on one fresh instance, and a set with a repeated render fails.
Two workers would each render the same sequence and turn 20 renders into
10 distinct ones.

A spread takes 3 to 256 renders. Fewer leave no leave-one-out median of
two distances. After 256 renders the render session restarts its
instance, whose first render repeats the first draw.

## The statistic

For each output, each of the 190 pairs of renders gives a distance: the
absolute difference of the output's values, or for an output of two
clips, such as the envelope distance, the distance itself. The spread is
the 95th percentile of those distances: `numpy.quantile`'s default,
linear between the two nearest pairs. It says how far two renders lie
apart, in the output's own unit.

## The tolerance

A comparison of one clip with the renders takes the median of the clip's
distances from each render. The tolerance measures the renders the same
way: for each render, the median of its distances from the other 19.
Sorted in ascending order, the tolerance is the median at rank
⌈0.95 (n + 1)⌉, at most n. For 20 renders that is the 20th, the largest.

The rank is the quantile of conformal prediction. If a clip of the same
sound and the n renders were exchangeable, the clip's median would lie
at or below the median at rank k with a probability of k / (n + 1): for
20 renders 20 / 21, 95.2 %. The clip's median runs over n distances and
each render's over n - 1, so this holds only nearly. Below 19 renders
the rank stops at n and the coverage falls below 95 %: 3 renders cover
3 / 4. The file records the coverage.

`held_out_within` measures the coverage on the renders themselves. On
the renders of the 38 reference recordings, each render held out in
turn against the other 19 lies within their limit in 94.6 % to 95.5 %
of the cases per output, as the
[reference recordings page](../../development/reference-recordings.md#the-draws)
shows. The 95th percentile of the medians, the tolerance before the
rank, lies near the second-largest of 20 and covered 90.7 % to 91.3 % in
the same check.

## Fields

| Field | Value |
| :-- | :-- |
| `identity` | The synth, its build, the host and the architecture, as in the [render record](render-record.md). |
| `recipe_version` | The recipe version of the renders. |
| `oversampling` | The quality setting: `"2x"` for every Vital render of recipe version 1. |
| `n` | How many renders, 3 to 256 and 20 by default, each a distinct draw. |
| `statistic` | `"p95_pairwise_distance"`. |
| `tolerance` | `"conformal_95_leave_one_out_median"`. |
| `coverage` | The share of clips of the same sound that lie within the tolerance: the rank over n + 1, 20 / 21 for 20 renders. |
| `preset_sha256` | The SHA-256 of the preset file. |
| `midi_sha256` | The SHA-256 of the MIDI file. |
| `bpm` | The tempo. |
| `outputs` | One row per output: `output`, its `unit`, `spread`, `tolerance`, `floor`, `held_out_within`, `adjacent_ratio` and `lag1_autocorrelation`. |

`held_out_within` counts the renders that lie within the limit of the
other renders, each held out in turn and compared as a clip. It checks
the coverage on the renders themselves: with 19 renders left, the rank
covers 19 in 20.

Two diagnostics describe the draws. Nothing acts on them.

- `adjacent_ratio` is the median distance of the 19 pairs of renders
  next to each other in the sequence over the median distance of all 190
  pairs. It lies near 1 when a draw does not depend on the one before.
  With 19 pairs against 190 it is noisy. It is absent when every pair
  lies at 0.
- `lag1_autocorrelation` is the lag-1 autocorrelation of the output's
  value over the sequence. For independent draws it lies near -1 / n,
  within about 2 / √n: -0.05 ± 0.45 for 20 renders. A drift over the
  sequence takes it towards 1. It is absent for the envelope distance,
  which only two clips have, and for a value that every render repeats.

## Outputs

Each clip is measured from its onset, the first sample within 40 dB of
its peak, to its end, the last sample above the recipe's tail floor of
-96 dBFS. Both channels count. The first five rows measure the clip:

| Output | Unit | What |
| :-- | :-- | :-- |
| `rms_db` | dB | The RMS level, in dBFS. Two clips compare over their common span: from the onset for the shorter clip's length, so a cut tail does not change the level. |
| `peak_db` | dB | The largest sample magnitude, in dBFS. |
| `peak_to_rms_db` | dB | The peak over the RMS level, the crest factor. Two clips compare with the RMS level over their common span. A gain shifts the peak and the RMS level alike, so it moves this output only where it moves the clip's end. |
| `duration_s` | s | From onset to end. |
| `envelope_distance_db` | dB | The distance of two envelopes of 10 ms frames, aligned at their onsets: the mean absolute level difference over the frames where either lies within 40 dB of the loudest frame of both. A frame below that range, or past the end of the shorter clip, counts at the bottom of the range, so a longer tail adds to the distance. |

The other rows come from the [layer report](layers.md) of each render,
which `analyze` writes with the render's MIDI file and the exact tempo.
A row's name is the path of the number, such as `stem.rms_dbfs` or
`stem.band_share[1].share`, without its `value` or `median` key. Its unit
comes from the field name's suffix: `_hz`, `_s`, `_db`, `_dbfs` or
`_deg`. `centroid_over_f0`, `flatness`, `rho` and `share` are ratios. A
field with neither fails the run, so a new field gets a unit before it
gets a spread. Confidences, IQRs, band edges and the run's inputs,
`bpm`, `bpm_given`, `bpm_refined` and `sample_rate_hz`, have no row. Nor
does a number that some render left without an estimate. A layer that a
later release adds to the report gets its rows the same way.

## Comparing with the spread

A comparison of one clip with the renders reports, for each clip output,
the clip's value and the renders' median, the difference, the limit,
their ratio and whether the difference lies within the limit. The ratio
shows how close a match is, beyond a yes or no: a ratio near 1 lies at
the edge of the renders. The clip and the renders must share a sample
rate, and the spread must hold a row for every clip output.

A recording made elsewhere can differ in level alone. A gain fit takes
that out before the verdict:

1. Fit the gain: the median over the renders of their RMS level minus
   the recording's, each pair over its common span.
2. Apply the gain to the recording and measure it again. Its end moves
   with the gain, since the tail floor stays fixed.
3. Compare it on the outputs of the sound's shape: `peak_to_rms_db`,
   `duration_s` and `envelope_distance_db`.

The duration and the envelope distance still depend on the level, which
is why they compare only after the gain; the peak-to-RMS ratio moves
with it only where the clip's end moves. The recording matches when all
three lie within their limits. A gain
beyond ±3 dB is a finding to explain, not a failure: `compare` matches
the loudness of two sounds before its distances.

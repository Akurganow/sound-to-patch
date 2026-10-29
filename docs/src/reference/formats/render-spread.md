# Render spread

Two renders of one preset are not identical: a synth starts its voices
at random phases. The spread file says how far renders of one preset lie
apart, so a comparison can tell a real difference from render noise. It
is the noise floor that calibration, compare, refine and the bench cite.

`cargo xtask render-spread --out spread.json` writes it: the
[testing page](../../development/testing.md#repeat-render-spread) gives
the method. The file is local and depends on the synth's build and its
quality setting. The repository keeps no spread of record; the
per-recording metrics are on the
[reference recordings](../../development/reference-recordings.md) page
only.

## The draws

A fresh Vital instance renders bit-identically to any other fresh
instance, and each later render on one instance differs. So the 20
renders of a spread are drawn in sequence on one fresh instance, and a
set with a repeated render fails. Two workers would each render the same
sequence and turn 20 renders into 10 distinct ones.

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
way: for each render, the median of its distances from the other 19, and
the 95th percentile of those 20 medians. A clip from the same sound lies
within it 19 times in 20.

An output that every render repeats has a tolerance of 0, and float
noise would then fail a clip of the same sound. A floor sets the least
tolerance: 0.01 dB for a level and one sample for a duration. A number
of the layer report in another unit takes the float32 resolution of its
largest value. The comparison uses the larger of the tolerance and the
floor, its limit.

## Fields

| Field | Value |
| :-- | :-- |
| `identity` | The synth, its build, the host and the architecture, as in the [render record](render-record.md). |
| `recipe_version` | The recipe version of the renders. |
| `oversampling` | The quality setting: `"2x"` for every Vital render of recipe version 1. |
| `n` | How many renders, 20, each a distinct draw. |
| `statistic` | `"p95_pairwise_distance"`. |
| `tolerance` | `"p95_leave_one_out_median"`. |
| `preset_sha256` | The SHA-256 of the preset file. |
| `midi_sha256` | The SHA-256 of the MIDI file. |
| `bpm` | The tempo. |
| `outputs` | One row per output: `output`, its `unit`, `spread`, `tolerance`, `floor` and `adjacent_ratio`. |

`adjacent_ratio` is the median distance of the 19 pairs of renders next
to each other in the sequence over the median distance of all 190 pairs.
It lies near 1 when a draw does not depend on the one before. It is
absent when every pair lies at 0.

## Outputs

Each clip is measured from its onset, the first sample within 40 dB of
its peak, to its end, the last sample above the recipe's tail floor of
-96 dBFS. Both channels count. The first four rows measure the clip:

| Output | Unit | What |
| :-- | :-- | :-- |
| `rms_db` | dB | The RMS level, in dBFS. Two clips compare over their common span: from the onset for the shorter clip's length, so a cut tail does not change the level. |
| `peak_db` | dB | The largest sample magnitude, in dBFS. |
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
rate.

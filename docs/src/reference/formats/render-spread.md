# Render spread

Two renders of one preset are not identical: a synth starts its voices
at random phases. The spread file says how far renders of one preset lie
apart, so a comparison can tell a real difference from render noise. It
is the noise floor that calibration, compare, refine and the bench cite.

`cargo xtask render-spread --out spread.json` writes it: the
[testing page](../../development/testing.md#repeat-render-spread) gives
the method. The file is local. It depends on the synth's build and its
quality setting, and the repository holds no measured spread.

## The statistic

The preset renders 20 times with one recipe. For each output, each of the
190 pairs of renders gives the absolute difference of the output's
values, and the spread is the 95th percentile of those differences:
`numpy.quantile`'s default, linear between the two nearest pairs. For a
distance between two clips, such as the envelope distance, each pair
gives its distance instead. The spread is in the output's own unit.

## Fields

| Field | Value |
| :-- | :-- |
| `identity` | The synth, its build, the host and the architecture, as in the [render record](render-record.md). |
| `recipe_version` | The recipe version of the renders. |
| `oversampling` | The quality setting: `"2x"` for every Vital render of recipe version 1. |
| `n` | How many renders, 20. |
| `statistic` | `"p95_abs_pairwise"`. |
| `preset_sha256` | The SHA-256 of the preset file. |
| `midi_sha256` | The SHA-256 of the MIDI file. |
| `bpm` | The tempo. |
| `outputs` | One row per output: `output`, its `unit` and its `spread`. |

## Outputs

Each clip is measured from its onset, the first sample within 40 dB of
its peak, to its end, the last sample above -96 dBFS. Both channels
count. The first four rows measure the clip:

| Output | Unit | What |
| :-- | :-- | :-- |
| `rms_db` | dB | The RMS level from onset to end, in dBFS. |
| `peak_db` | dB | The largest sample magnitude, in dBFS. |
| `duration_s` | s | From onset to end. |
| `envelope_distance_db` | dB | The distance of two envelopes of 10 ms frames, aligned at their onsets: the mean absolute level difference over the frames where either lies within 40 dB of the loudest frame of both. A frame below that range, or past the end of the shorter clip, counts at the bottom of the range, so a longer tail adds to the distance. |

The other rows come from the [layer report](layers.md) of each render,
which `analyze` writes with the render's MIDI file and the exact tempo.
A row's name is the path of the number, such as `stem.rms_dbfs` or
`stem.band_share[1].share`, without its `value` or `median` key. Its unit
comes from the field name's suffix, and a field without one is a ratio.
Confidences, IQRs, band edges and the run's inputs, `bpm`, `bpm_given`,
`bpm_refined` and `sample_rate_hz`, have no row. Nor does a number that
some render left without an estimate. A layer that a later release adds
to the report gets its rows the same way.

## Comparing with the spread

A comparison of one clip with renders of the same preset takes, for each
output, the difference between the clip and each render, and the median
of those differences over the renders. A pair of renders differs by more
than the 95th percentile one time in twenty, so a single pair would miss
often by chance. The clip matches on an output when that median lies
within the spread.

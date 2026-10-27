# Layer report

`analyze` writes `layers.json`, the report that an agent reads before it
writes an edit list. This page lists the fields of version `layers/1`. The
committed JSON Schema,
[`schemas/layers.schema.json`](https://github.com/Akurganow/sound-to-patch/blob/main/schemas/layers.schema.json),
holds the same contract, and the test suite validates every generated
report against it.

## Top level

| Field | Value |
| :-- | :-- |
| `schema` | `"layers/1"`. |
| `source` | The file name of the stem, without its folder. A run of another stem into the same folder stops unless `--force` is given. |
| `synth` | The `--synth` value, `"vital"` or `"serum2"`. Later mappers pick the synth from it; the layer fields stay synth-neutral. |
| `stem` | The stem group below. |
| `transcription` | The transcriber's model and settings, the group below. `null` in this release: `analyze` runs no transcriber, and with `--notes` none would. |
| `conventions` | The convention of every number in `stem`, keyed by its field name, plus `frame_size`, `statistics`, `spectral` and `confidence`. |

The top level takes no other field. Later releases add one group per layer,
each with `values`, `confidence`, `flags`, `needs_render` and
`suggested_edits`, and extend the schema in the same change. A group with
too little evidence writes `values: null` and says why in its confidence.
`suggested_edits` maps a parameter name with its unit suffix to a plain
number, such as `"filter1_cutoff_hz": 410`.

## The transcription group

The group records what produced the notes, so a note that flips between
two machines can be traced to its settings.

| Field | Value |
| :-- | :-- |
| `model_sha256` | The SHA-256 of the bundled model weights, 64 lowercase hex digits. |
| `crate_version` | The version of the transcription crate, `"0.1.0"`. |
| `profile` | `"bass"`, `"lead"` or `"pad"`, or `"detected"` when the stem chose the settings. |
| `note_params` | Every decoder parameter by the crate's own name: `onset_threshold`, `frame_threshold`, `minimum_note_length_ms`, `minimum_frequency`, `maximum_frequency` (both in Hz), `infer_onsets`, `melodia_trick`, `include_pitch_bends`, `energy_tolerance` and `bend_offset`. Numbers and booleans only. |

## Naming

- Field names are snake_case ASCII.
- A field with a unit carries it as a suffix: `_hz`, `_s`, `_db`, `_dbfs`
  or `_deg`. A field without a suffix is a count, a ratio, a share from 0
  to 1, a flag or a tempo in BPM.
- Integers are written as integers; every other number is a float.

## Value objects

A single estimate is `{"value": ..., "confidence": ...}`. A statistic over
active frames is `{"median": ..., "iqr": ..., "confidence": ...}`, the
median and the interquartile range. The value, or the median and the IQR,
is `null` exactly when the confidence states no estimate.

The confidence object follows the [units page](../units.md#confidence):

```json
{"level": "high", "reasons": [{"code": "active_frames", "value": 412}], "spread": 310.0, "estimate": true}
```

- `level` is `low`, `medium` or `high`.
- `reasons` holds at least one `{code, value}` pair. The value is a count,
  a number or a flag.
- `spread` is optional, in the unit of the value: the IQR of a statistic,
  one bin width for the band edge.
- `"estimate": false` means no value. Such an object has reasons and
  neither a level nor a spread.
- An unknown field is an error.

## The stem group

Every statistic over frames, `band_share` and `band_edge_hz` need at
least 16 active frames; with fewer they give no estimate with the reason
`too_few_frames`. The ranges are those of the unit types and the schema. Each convention is
the text that `conventions` holds under the same key; the
[pipeline page](../../analysis/pipeline.md) explains the rules behind
them.

| Field | Unit | Range | Shape | Convention |
| :-- | :-- | :-- | :-- | :-- |
| `bpm` | beats per minute | 4 to 1000 | number | Given by `--bpm`, not estimated. |
| `duration_s` | seconds | 0 or more | number | Decoded frames over the sample rate. |
| `sample_rate_hz` | hertz | 1 or more | integer | The native rate of the file; no resampling. |
| `band_edge_hz` | hertz | 0 to Nyquist | value | The steepest fall of at least 25 dB per semitone above 2 kHz on a plateau, else the highest bin 30 dB above the floor at low confidence. |
| `hard_limited` | flag | | boolean | Flat runs of at least 1 ms within 1e-5 of the channel peak, entered and left from at least half the peak, cover 0.1 % of a channel. |
| `peak_dbfs` | dB re full scale 1.0 | 0 or less for unclipped audio | value | Peak of the mono mix over the whole stem. |
| `rms_dbfs` | dB re full scale 1.0 | below `peak_dbfs` | value | RMS of the mono mix over the whole stem; a full-scale sine reads -3.01 dB. |
| `crest_db` | dB | 0 or more | value | `peak_dbfs - rms_dbfs`. |
| `centroid_hz` | hertz | 0 to Nyquist | statistic | Magnitude-weighted mean frequency per active frame. |
| `centroid_over_f0` | ratio | 0 or more | statistic or `null` | The centroid over the median f0 of the note list; `null` without `--notes`. |
| `spread_hz` | hertz | 0 or more | statistic | Magnitude-weighted standard deviation around the centroid. |
| `rolloff85_hz` | hertz | 0 to Nyquist | statistic | Frequency below which 85 % of the frame's magnitude lies. |
| `rolloff95_hz` | hertz | 0 to Nyquist | statistic | The same at 95 %. |
| `flatness` | ratio | 0 to 1 | statistic | Geometric over arithmetic mean of the magnitude, 100 Hz to 10 kHz, floored at 1e-5. |
| `band_share` | share | each 0 to 1, sum 1 | value with a list | Power per band over active frames divided by the power in the bands reported; each item is `{lo_hz, hi_hz, share}` for the bands 20, 60, 250 Hz, 1, 4, 10 and 20 kHz. A band at or above the Nyquist frequency is left out, and one that crosses it ends there. |
| `stereo` | | | object | See below. |

`stereo` holds `full`, `sub` (below 120 Hz), `low_mid` (120 Hz to 1 kHz)
and `high` (above 1 kHz), each `{rho, sm_db, ipd_deg}`, and one
confidence for the screen:

| Field | Unit | Range | Convention |
| :-- | :-- | :-- | :-- |
| `rho` | correlation | -1 to 1 | Pearson correlation of L and R at zero lag; per band from the 8192-point STFT. |
| `sm_db` | dB | any | `20 log10(rms(S) / rms(M))` with `M = (L + R) / 2`, `S = (L - R) / 2`. |
| `ipd_deg` | degrees | 0 to 180 | The magnitude-weighted phase difference of L and R at harmonics 1 to 20 of the median f0, averaged over active frames; `null` without `--notes`. |

A stereo value is `null` when it does not exist: a one-channel file, a
silent channel, or a zero mid or side.

## Conventions

| Key | Holds |
| :-- | :-- |
| `frame_size` | The STFT frame length of the descriptors and the band edge: 8192 when the note list has a median f0 below 100 Hz, else 4096, and 4096 without a note list. |
| `statistics` | Median and IQR over active frames, with linear-interpolation quantiles; the active-frame gate. |
| `spectral` | Magnitude weights for shape, power for band shares; the window and the hop. |
| `confidence` | What the level, the reasons and the spread mean. |
| every `stem` field | The convention of that field, as in the tables above. |
